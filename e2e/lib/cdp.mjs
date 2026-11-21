import { spawn } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const DEFAULT_CHROMIUM_BIN = process.env.XMEDIA_CHROMIUM_BIN || '/usr/sbin/chromium';
const DEFAULT_ALLOWED_ERROR_PATTERNS = [/\/_app\/health(?:$|\?)/, /\/favicon\.ico(?:$|\?)/];

const activeCdpClients = new Set();
let cdpSignalHandlersInstalled = false;

function cleanupAllActiveCdpClientsSync() {
  for (const client of [...activeCdpClients]) {
    client.forceKillSync();
  }
}

const onCdpExit = () => {
  cleanupAllActiveCdpClientsSync();
};
const onCdpSigint = () => {
  cleanupAllActiveCdpClientsSync();
  detachCdpSignalHandlers();
  if (process.listenerCount('SIGINT') === 0) {
    process.kill(process.pid, 'SIGINT');
  }
};
const onCdpSigterm = () => {
  cleanupAllActiveCdpClientsSync();
  detachCdpSignalHandlers();
  if (process.listenerCount('SIGTERM') === 0) {
    process.kill(process.pid, 'SIGTERM');
  }
};

function attachCdpSignalHandlers() {
  if (cdpSignalHandlersInstalled) return;
  cdpSignalHandlersInstalled = true;
  process.on('exit', onCdpExit);
  process.on('SIGINT', onCdpSigint);
  process.on('SIGTERM', onCdpSigterm);
}

function detachCdpSignalHandlers() {
  if (!cdpSignalHandlersInstalled) return;
  cdpSignalHandlersInstalled = false;
  process.off('exit', onCdpExit);
  process.off('SIGINT', onCdpSigint);
  process.off('SIGTERM', onCdpSigterm);
}

function matchesAnyPattern(url, patterns) {
  if (!url) return false;
  for (const pat of patterns) {
    if (typeof pat === 'string') {
      if (url.includes(pat)) return true;
    } else if (pat instanceof RegExp) {
      if (pat.test(url)) return true;
    }
  }
  return false;
}

export class CdpClient {
  constructor({
    chromiumBin = DEFAULT_CHROMIUM_BIN,
    allowedErrorUrls = DEFAULT_ALLOWED_ERROR_PATTERNS,
    headless = true,
    launchTimeoutMs = 15000,
    keepProfile = false,
  } = {}) {
    this.chromiumBin = chromiumBin;
    this.allowedErrorUrls = [...allowedErrorUrls];
    this.headless = headless;
    this.launchTimeoutMs = launchTimeoutMs;
    this.keepProfile = keepProfile;

    this.proc = null;
    this.userDataDir = null;
    this.ws = null;
    this.nextId = 1;
    this.pending = new Map();
    this.eventListeners = new Map();

    this.exceptions = [];
    this.consoleErrors = [];
    this.httpErrors = [];
    this.networkResponses = [];
    this.networkRequests = [];
    this.closed = false;
  }

  async launch() {
    if (this.proc) {
      throw new Error('CdpClient already launched');
    }
    this.closed = false;
    this.userDataDir = mkdtempSync(path.join(os.tmpdir(), 'xmedia-cdp-'));

    const args = [
      ...(this.headless ? ['--headless'] : []),
      '--no-sandbox',
      '--disable-gpu',
      '--disable-dev-shm-usage',
      '--no-first-run',
      '--no-default-browser-check',
      '--autoplay-policy=no-user-gesture-required',
      '--remote-debugging-port=0',
      `--user-data-dir=${this.userDataDir}`,
      'about:blank',
    ];

    this.proc = spawn(this.chromiumBin, args, {
      detached: true,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    activeCdpClients.add(this);
    attachCdpSignalHandlers();

    const browserWsUrl = await new Promise((resolve, reject) => {
      let stderrBuf = '';
      let settled = false;

      const timer = setTimeout(() => {
        if (!settled) {
          settled = true;
          reject(new Error(`Timed out waiting for Chromium DevTools port. stderr:\n${stderrBuf}`));
        }
      }, this.launchTimeoutMs);

      const onData = (chunk) => {
        stderrBuf += chunk.toString('utf8');
        const match = stderrBuf.match(/DevTools listening on (ws:\/\/[^\s]+)/);
        if (match && !settled) {
          settled = true;
          clearTimeout(timer);
          resolve(match[1]);
        }
      };

      this.proc.stderr.on('data', onData);
      this.proc.once('error', (err) => {
        if (!settled) {
          settled = true;
          clearTimeout(timer);
          reject(err);
        }
      });
      this.proc.once('exit', (code, signal) => {
        if (!settled) {
          settled = true;
          clearTimeout(timer);
          reject(
            new Error(
              `Chromium exited early (code=${code}, signal=${signal}). stderr:\n${stderrBuf}`,
            ),
          );
        }
      });
    });

    const parsedWs = new URL(browserWsUrl);
    const listUrl = `http://${parsedWs.host}/json/list`;
    let pageTarget = null;
    for (let attempt = 0; attempt < 20; attempt += 1) {
      const targets = await (await fetch(listUrl)).json();
      pageTarget = targets.find((t) => t.type === 'page' && t.webSocketDebuggerUrl);
      if (pageTarget) break;
      await new Promise((r) => setTimeout(r, 50));
    }
    if (!pageTarget) {
      throw new Error(`No CDP page target found at ${listUrl}`);
    }

    await this.#connectWebSocket(pageTarget.webSocketDebuggerUrl);
    await this.send('Page.enable');
    await this.send('Runtime.enable');
    await this.send('Network.enable');
    await this.send('Log.enable');
    await this.send('Emulation.setDeviceMetricsOverride', {
      width: 1280,
      height: 800,
      deviceScaleFactor: 1,
      mobile: false,
    });

    this.#attachTelemetryListeners();
    return this;
  }

  #connectWebSocket(wsUrl) {
    return new Promise((resolve, reject) => {
      const ws = new WebSocket(wsUrl);
      this.ws = ws;

      const onOpen = () => {
        ws.removeEventListener('error', onErr);
        resolve();
      };
      const onErr = (ev) => {
        reject(new Error(`CDP WebSocket error: ${ev?.message || 'connection failed'}`));
      };

      ws.addEventListener('open', onOpen, { once: true });
      ws.addEventListener('error', onErr, { once: true });

      ws.addEventListener('message', (ev) => {
        let msg;
        try {
          msg = JSON.parse(typeof ev.data === 'string' ? ev.data : ev.data.toString());
        } catch {
          return;
        }
        if (msg.id !== undefined) {
          const entry = this.pending.get(msg.id);
          if (entry) {
            this.pending.delete(msg.id);
            clearTimeout(entry.timer);
            if (msg.error) {
              entry.reject(
                new Error(`CDP error (${entry.method}): ${msg.error.message} [${msg.error.code}]`),
              );
            } else {
              entry.resolve(msg.result || {});
            }
          }
        } else if (msg.method) {
          const listeners = this.eventListeners.get(msg.method);
          if (listeners) {
            for (const fn of [...listeners]) {
              try {
                fn(msg.params || {});
              } catch {
                // ignore listener error
              }
            }
          }
        }
      });

      ws.addEventListener('close', () => {
        for (const [, entry] of this.pending) {
          clearTimeout(entry.timer);
          entry.reject(new Error(`CDP WebSocket closed while awaiting ${entry.method}`));
        }
        this.pending.clear();
      });
    });
  }

  #attachTelemetryListeners() {
    this.on('Runtime.exceptionThrown', (params) => {
      const details = params.exceptionDetails || {};
      const desc = details.exception?.description || details.text || 'Uncaught exception';
      this.exceptions.push({
        text: details.text || 'Exception',
        description: desc,
        url: details.url || '',
        lineNumber: details.lineNumber ?? null,
        columnNumber: details.columnNumber ?? null,
        timestamp: Date.now(),
      });
    });

    this.on('Runtime.consoleAPICalled', (params) => {
      if (params.type !== 'error') return;
      const text = (params.args || [])
        .map((a) => a.value ?? a.description ?? a.type ?? '')
        .join(' ')
        .trim();
      this.consoleErrors.push({
        text,
        timestamp: Date.now(),
      });
    });

    this.on('Log.entryAdded', (params) => {
      const entry = params.entry || {};
      if (entry.level !== 'error') return;
      if (entry.source === 'network' && matchesAnyPattern(entry.url || '', this.allowedErrorUrls)) {
        return;
      }
      this.consoleErrors.push({
        source: entry.source || 'log',
        text: entry.text || '',
        url: entry.url || '',
        timestamp: Date.now(),
      });
    });

    this.on('Network.requestWillBeSent', (params) => {
      const req = params.request || {};
      this.networkRequests.push({
        requestId: params.requestId,
        url: req.url || '',
        method: req.method || 'GET',
        type: params.type || '',
        timestamp: Date.now(),
      });
      if (this.networkRequests.length > 2000) this.networkRequests.shift();
    });

    this.on('Network.responseReceived', (params) => {
      const resp = params.response || {};
      const entry = {
        requestId: params.requestId,
        url: resp.url || '',
        status: resp.status ?? 0,
        statusText: resp.statusText || '',
        mimeType: resp.mimeType || '',
        type: params.type || '',
        timestamp: Date.now(),
      };
      this.networkResponses.push(entry);
      if (this.networkResponses.length > 2000) this.networkResponses.shift();

      if (entry.status >= 400 && !matchesAnyPattern(entry.url, this.allowedErrorUrls)) {
        this.httpErrors.push(entry);
      }
    });
  }

  on(method, handler) {
    let set = this.eventListeners.get(method);
    if (!set) {
      set = new Set();
      this.eventListeners.set(method, set);
    }
    set.add(handler);
    return () => set.delete(handler);
  }

  send(method, params = {}, timeoutMs = 15000) {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
      return Promise.reject(new Error(`CDP WebSocket not open for ${method}`));
    }
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`CDP command timed out (${timeoutMs}ms): ${method}`));
      }, timeoutMs);

      this.pending.set(id, { method, resolve, reject, timer });
      this.ws.send(JSON.stringify({ id, method, params }));
    });
  }

  async goto(url, { timeoutMs = 15000 } = {}) {
    let loaded = false;
    const off = this.on('Page.loadEventFired', () => {
      loaded = true;
    });

    try {
      const nav = await this.send('Page.navigate', { url }, timeoutMs);
      if (nav.errorText) {
        throw new Error(`Page.navigate failed for ${url}: ${nav.errorText}`);
      }
      const deadline = Date.now() + timeoutMs;
      while (!loaded && Date.now() < deadline) {
        const ready = await this.evalJs('document.readyState', { awaitPromise: false }).catch(
          () => '',
        );
        if (ready === 'complete' && loaded) break;
        await new Promise((r) => setTimeout(r, 40));
      }
      if (!loaded) {
        const ready = await this.evalJs('document.readyState', { awaitPromise: false }).catch(
          () => '',
        );
        if (ready !== 'complete' && ready !== 'interactive') {
          throw new Error(`Timed out waiting for Page.loadEventFired on ${url}`);
        }
      }
    } finally {
      off();
    }
  }

  async evalJs(expression, { awaitPromise = true, timeoutMs = 15000 } = {}) {
    const res = await this.send(
      'Runtime.evaluate',
      {
        expression,
        awaitPromise,
        returnByValue: true,
      },
      timeoutMs,
    );
    if (res.exceptionDetails) {
      const msg =
        res.exceptionDetails.exception?.description ||
        res.exceptionDetails.text ||
        'Runtime.evaluate exception';
      throw new Error(`evalJs failed: ${msg}`);
    }
    return res.result ? res.result.value : undefined;
  }

  async waitForFn(fnOrExpr, { timeoutMs = 10000, pollMs = 100, label = 'waitForFn' } = {}) {
    const expr =
      typeof fnOrExpr === 'function' ? `(${fnOrExpr.toString()})()` : `Boolean(${fnOrExpr})`;
    const deadline = Date.now() + timeoutMs;
    let lastError = null;
    while (Date.now() < deadline) {
      try {
        const val = await this.evalJs(expr, { awaitPromise: true });
        if (val) return val;
      } catch (err) {
        lastError = err;
      }
      await new Promise((r) => setTimeout(r, pollMs));
    }
    throw new Error(
      `Timed out after ${timeoutMs}ms in ${label}${lastError ? ` (last error: ${lastError.message})` : ''}`,
    );
  }

  async waitForSelector(selector, { timeoutMs = 10000, visible = false } = {}) {
    const selJson = JSON.stringify(selector);
    const expr = visible
      ? `(() => {
          const el = document.querySelector(${selJson});
          if (!el) return false;
          const st = getComputedStyle(el);
          if (st.display === 'none' || st.visibility === 'hidden' || st.opacity === '0') return false;
          const r = el.getBoundingClientRect();
          return r.width > 0 && r.height > 0;
        })()`
      : `Boolean(document.querySelector(${selJson}))`;
    await this.waitForFn(expr, {
      timeoutMs,
      pollMs: 50,
      label: `waitForSelector(${selector})`,
    });
  }

  async click(selector, { timeoutMs = 10000 } = {}) {
    await this.waitForSelector(selector, { timeoutMs, visible: true });
    const selJson = JSON.stringify(selector);
    const box = await this.evalJs(`(() => {
      const el = document.querySelector(${selJson});
      el.scrollIntoView({ block: 'center', inline: 'center' });
      const r = el.getBoundingClientRect();
      return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
    })()`);

    await this.send('Input.dispatchMouseEvent', {
      type: 'mousePressed',
      x: box.x,
      y: box.y,
      button: 'left',
      clickCount: 1,
    });
    await this.send('Input.dispatchMouseEvent', {
      type: 'mouseReleased',
      x: box.x,
      y: box.y,
      button: 'left',
      clickCount: 1,
    });
  }

  async type(selector, text, { clear = true, timeoutMs = 10000 } = {}) {
    await this.waitForSelector(selector, { timeoutMs, visible: true });
    const selJson = JSON.stringify(selector);
    const textJson = JSON.stringify(String(text));

    await this.evalJs(`(() => {
      const el = document.querySelector(${selJson});
      el.focus();
      if (${clear}) {
        const proto = el.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
        const setter = Object.getOwnPropertyDescriptor(proto, 'value')?.set;
        if (setter) setter.call(el, '');
        else el.value = '';
        el.dispatchEvent(new Event('input', { bubbles: true }));
      }
      return true;
    })()`);

    if (String(text).length > 0) {
      await this.send('Input.insertText', { text: String(text) });
      await this.evalJs(`(() => {
        const el = document.querySelector(${selJson});
        if (el && el.value !== ${textJson}) {
          const proto = el.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
          const setter = Object.getOwnPropertyDescriptor(proto, 'value')?.set;
          if (setter) setter.call(el, ${textJson});
          else el.value = ${textJson};
          el.dispatchEvent(new Event('input', { bubbles: true }));
        }
        el?.dispatchEvent(new Event('change', { bubbles: true }));
      })()`);
    }
  }

  async setCookie({
    name,
    value,
    url = undefined,
    domain = undefined,
    path: cookiePath = '/',
    httpOnly = false,
    secure = false,
    sameSite = 'Lax',
  }) {
    const res = await this.send('Network.setCookie', {
      name,
      value,
      ...(url ? { url } : {}),
      ...(domain ? { domain } : {}),
      path: cookiePath,
      httpOnly,
      secure,
      sameSite,
    });
    if (!res.success) {
      throw new Error(`Network.setCookie failed for ${name}`);
    }
  }

  async pressKey(key, { modifiers = 0 } = {}) {
    const keyMap = {
      Enter: { code: 'Enter', keyCode: 13 },
      Escape: { code: 'Escape', keyCode: 27 },
      Space: { key: ' ', code: 'Space', keyCode: 32 },
      ArrowLeft: { code: 'ArrowLeft', keyCode: 37 },
      ArrowRight: { code: 'ArrowRight', keyCode: 39 },
      ArrowUp: { code: 'ArrowUp', keyCode: 38 },
      ArrowDown: { code: 'ArrowDown', keyCode: 40 },
    };
    const info = keyMap[key] || {
      key,
      code: key.length === 1 ? `Key${key.toUpperCase()}` : key,
      keyCode: key.length === 1 ? key.toUpperCase().charCodeAt(0) : 0,
    };
    await this.send('Input.dispatchKeyEvent', {
      type: 'keyDown',
      key: info.key || key,
      code: info.code,
      windowsVirtualKeyCode: info.keyCode,
      modifiers,
    });
    await this.send('Input.dispatchKeyEvent', {
      type: 'keyUp',
      key: info.key || key,
      code: info.code,
      windowsVirtualKeyCode: info.keyCode,
      modifiers,
    });
  }

  clearErrors() {
    this.exceptions = [];
    this.consoleErrors = [];
    this.httpErrors = [];
  }

  getErrors() {
    return {
      exceptions: [...this.exceptions],
      consoleErrors: [...this.consoleErrors],
      httpErrors: [...this.httpErrors],
    };
  }

  assertNoErrors(label = 'CDP session') {
    const { exceptions, consoleErrors, httpErrors } = this.getErrors();
    const problems = [];
    if (exceptions.length > 0) {
      problems.push(`exceptions=${JSON.stringify(exceptions)}`);
    }
    if (consoleErrors.length > 0) {
      problems.push(`consoleErrors=${JSON.stringify(consoleErrors)}`);
    }
    if (httpErrors.length > 0) {
      problems.push(`httpErrors=${JSON.stringify(httpErrors)}`);
    }
    if (problems.length > 0) {
      throw new Error(`${label} recorded unexpected browser errors: ${problems.join('; ')}`);
    }
  }

  forceKillSync() {
    activeCdpClients.delete(this);
    if (this.proc && this.proc.exitCode === null && this.proc.signalCode === null) {
      try {
        process.kill(-this.proc.pid, 'SIGKILL');
      } catch {
        try {
          this.proc.kill('SIGKILL');
        } catch {
          // ignore
        }
      }
    }
    if (!this.keepProfile && this.userDataDir) {
      try {
        rmSync(this.userDataDir, { recursive: true, force: true, maxRetries: 5, retryDelay: 40 });
      } catch {
        // ignore
      }
      this.userDataDir = null;
    }
    if (activeCdpClients.size === 0) {
      detachCdpSignalHandlers();
    }
  }

  async close() {
    if (this.closed) return;
    this.closed = true;
    activeCdpClients.delete(this);

    if (this.ws) {
      try {
        if (this.ws.readyState === WebSocket.OPEN || this.ws.readyState === WebSocket.CONNECTING) {
          this.ws.close();
        }
      } catch {
        // ignore
      }
      this.ws = null;
    }

    if (this.proc) {
      const child = this.proc;
      if (child.exitCode === null && child.signalCode === null) {
        const exited = new Promise((resolve) => {
          child.once('exit', () => resolve());
        });
        try {
          process.kill(-child.pid, 'SIGTERM');
        } catch {
          child.kill('SIGTERM');
        }
        const timer = setTimeout(() => {
          if (child.exitCode === null && child.signalCode === null) {
            try {
              process.kill(-child.pid, 'SIGKILL');
            } catch {
              child.kill('SIGKILL');
            }
          }
        }, 3000);
        timer.unref();
        await exited;
        clearTimeout(timer);
      }
      this.proc = null;
    }

    if (!this.keepProfile && this.userDataDir) {
      rmSync(this.userDataDir, { recursive: true, force: true, maxRetries: 5, retryDelay: 40 });
      this.userDataDir = null;
    }

    if (activeCdpClients.size === 0) {
      detachCdpSignalHandlers();
    }
  }
}

export async function withCdpBrowser(fn, opts = {}) {
  const cdp = new CdpClient(opts);
  try {
    await cdp.launch();
    return await fn(cdp);
  } finally {
    await cdp.close();
  }
}
