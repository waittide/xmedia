import { spawn } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { existsSync, mkdtempSync, rmSync } from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { assertM3u8Rewritten } from './fixtures.mjs';

export { assertM3u8Rewritten };

const __dirname = path.dirname(fileURLToPath(import.meta.url));
export const REPO_ROOT = path.resolve(__dirname, '..', '..');

const activeServerManagers = new Set();
let serverSignalHandlersInstalled = false;

function cleanupAllActiveServersSync() {
  for (const mgr of [...activeServerManagers]) {
    mgr.forceCleanupSync();
  }
}

const onServerExit = () => {
  cleanupAllActiveServersSync();
};
const onServerSigint = () => {
  cleanupAllActiveServersSync();
  detachServerSignalHandlers();
  if (process.listenerCount('SIGINT') === 0) {
    process.kill(process.pid, 'SIGINT');
  }
};
const onServerSigterm = () => {
  cleanupAllActiveServersSync();
  detachServerSignalHandlers();
  if (process.listenerCount('SIGTERM') === 0) {
    process.kill(process.pid, 'SIGTERM');
  }
};

function attachServerSignalHandlers() {
  if (serverSignalHandlersInstalled) return;
  serverSignalHandlersInstalled = true;
  process.on('exit', onServerExit);
  process.on('SIGINT', onServerSigint);
  process.on('SIGTERM', onServerSigterm);
}

function detachServerSignalHandlers() {
  if (!serverSignalHandlersInstalled) return;
  serverSignalHandlersInstalled = false;
  process.off('exit', onServerExit);
  process.off('SIGINT', onServerSigint);
  process.off('SIGTERM', onServerSigterm);
}

export class SutEndpointMissingError extends Error {
  constructor(method, reqPath, status, bodyPreview = '') {
    super(
      `SUT_ENDPOINT_MISSING: ${method} ${reqPath} returned ${status}${
        bodyPreview ? ` — ${bodyPreview}` : ''
      }`,
    );
    this.name = 'SutEndpointMissingError';
    this.method = method;
    this.path = reqPath;
    this.status = status;
  }
}

export function allocateFreePort(host = '127.0.0.1') {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.unref();
    server.on('error', reject);
    server.listen(0, host, () => {
      const addr = server.address();
      const port = typeof addr === 'object' && addr ? addr.port : 0;
      server.close((err) => {
        if (err) reject(err);
        else resolve(port);
      });
    });
  });
}

export function resolveXMediaBinary(explicitPath = null) {
  const candidates = [
    explicitPath,
    process.env.XMEDIA_BIN,
    path.join(REPO_ROOT, 'target/debug/xmedia'),
    path.join(REPO_ROOT, 'target/release/xmedia'),
  ].filter(Boolean);

  for (const candidate of candidates) {
    if (existsSync(candidate)) {
      return path.resolve(candidate);
    }
  }

  throw new Error(
    `xmedia binary not found. Checked: ${candidates.join(', ')}. ` +
      'Build it with `cargo build --bin xmedia` or pass --xmedia-bin <path>.',
  );
}

export function resolveWebDir(explicitWebDir = null) {
  const candidates = [
    explicitWebDir,
    process.env.XMEDIA_WEB_DIR,
    path.join(REPO_ROOT, 'web/dist'),
  ].filter(Boolean);

  for (const dir of candidates) {
    if (existsSync(path.join(dir, 'index.html'))) {
      return path.resolve(dir);
    }
  }
  return null;
}

function extractCookieValue(headers, cookieName) {
  const setCookies =
    typeof headers.getSetCookie === 'function'
      ? headers.getSetCookie()
      : headers.get('set-cookie')
        ? [headers.get('set-cookie')]
        : [];
  for (const raw of setCookies) {
    const parts = raw.split(';');
    const first = parts[0]?.trim();
    if (!first) continue;
    const eqIdx = first.indexOf('=');
    if (eqIdx === -1) continue;
    const name = first.slice(0, eqIdx).trim();
    const val = first.slice(eqIdx + 1).trim();
    if (name === cookieName) {
      return val;
    }
  }
  return null;
}

export class XMediaServerManager {
  constructor({
    xmediaBin = null,
    webDir = null,
    dataDir = null,
    secret = null,
    ffmpegBin = null,
    keepTmp = false,
    startupTimeoutMs = 20000,
    stopTimeoutMs = 5000,
  } = {}) {
    this.xmediaBin = xmediaBin;
    this.webDir = webDir !== undefined ? webDir : resolveWebDir();
    this.ownsTempDir = !dataDir;
    this.dataDir = dataDir || mkdtempSync(path.join(os.tmpdir(), 'xmedia-e2e-'));
    this.secret = secret || randomBytes(32).toString('hex');
    this.ffmpegBin = ffmpegBin;
    this.keepTmp = keepTmp;
    this.startupTimeoutMs = startupTimeoutMs;
    this.stopTimeoutMs = stopTimeoutMs;

    this.proc = null;
    this.port = null;
    this.baseUrl = null;
    this.mode = 'mock';
    this.logs = [];
    this.exitInfo = null;
  }

  async start({ mode = 'mock' } = {}) {
    if (this.proc) {
      throw new Error('XMediaServerManager is already running');
    }
    this.mode = mode;
    const binPath = resolveXMediaBinary(this.xmediaBin);
    this.port = await allocateFreePort('127.0.0.1');
    this.baseUrl = `http://127.0.0.1:${this.port}`;
    this.logs = [];
    this.exitInfo = null;

    const args = [
      '--bind',
      `127.0.0.1:${this.port}`,
      '--data-dir',
      this.dataDir,
      '--secret',
      this.secret,
    ];
    if (this.webDir) {
      args.push('--web-dir', this.webDir);
    }
    if (this.ffmpegBin) {
      args.push('--ffmpeg-bin', this.ffmpegBin);
    }

    const env = { ...process.env };
    if (mode === 'mock') {
      env.XMEDIA_ALLOW_PRIVATE_IPS_FOR_TESTING = '1';
    } else {
      delete env.XMEDIA_ALLOW_PRIVATE_IPS_FOR_TESTING;
    }

    this.proc = spawn(binPath, args, {
      cwd: REPO_ROOT,
      env,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    activeServerManagers.add(this);
    attachServerSignalHandlers();

    const pushLog = (stream, chunk) => {
      const lines = chunk.toString('utf8').split(/\r?\n/).filter(Boolean);
      for (const line of lines) {
        this.logs.push(`[${stream}] ${line}`);
        if (this.logs.length > 200) this.logs.shift();
      }
    };

    this.proc.stdout.on('data', (c) => pushLog('stdout', c));
    this.proc.stderr.on('data', (c) => pushLog('stderr', c));
    this.proc.on('exit', (code, signal) => {
      this.exitInfo = { code, signal };
      this.proc = null;
    });

    await this.#waitUntilReady();
    return this.baseUrl;
  }

  async #waitUntilReady() {
    const deadline = Date.now() + this.startupTimeoutMs;
    const probeUrl = `${this.baseUrl}/api/v1/auth/setup-status`;
    let lastErr = '';

    while (Date.now() < deadline) {
      if (this.exitInfo) {
        throw new Error(
          `xmedia exited prematurely (code=${this.exitInfo.code}, signal=${this.exitInfo.signal}):\n${this.logs.join('\n')}`,
        );
      }
      try {
        const res = await fetch(probeUrl, { signal: AbortSignal.timeout(1500) });
        if (res.status === 200) {
          return;
        }
        lastErr = `HTTP ${res.status}`;
      } catch (err) {
        lastErr = err.message;
      }
      await new Promise((r) => setTimeout(r, 100));
    }

    const logsTail = this.logs.slice(-40).join('\n');
    await this.stop();
    throw new Error(
      `Timed out waiting for xmedia at ${probeUrl} (${lastErr}). Recent logs:\n${logsTail}`,
    );
  }

  async stop() {
    if (!this.proc) {
      return this.exitInfo || { code: 0, signal: null };
    }
    const child = this.proc;
    const exited = new Promise((resolve) => {
      child.once('exit', (code, signal) => resolve({ code, signal }));
    });
    child.kill('SIGTERM');

    const killTimer = setTimeout(() => {
      if (child.exitCode === null && child.signalCode === null) {
        child.kill('SIGKILL');
      }
    }, this.stopTimeoutMs);
    killTimer.unref();

    const result = await exited;
    clearTimeout(killTimer);
    this.proc = null;
    return result;
  }

  async restart(opts = undefined) {
    const targetMode = opts?.mode || this.mode;
    await this.stop();
    return this.start({ mode: targetMode });
  }

  forceCleanupSync() {
    activeServerManagers.delete(this);
    if (this.proc && this.proc.exitCode === null && this.proc.signalCode === null) {
      try {
        this.proc.kill('SIGKILL');
      } catch {
        // ignore
      }
      this.proc = null;
    }
    if (this.ownsTempDir && !this.keepTmp && this.dataDir) {
      try {
        rmSync(this.dataDir, { recursive: true, force: true, maxRetries: 5, retryDelay: 40 });
      } catch {
        // ignore
      }
    }
    if (activeServerManagers.size === 0) {
      detachServerSignalHandlers();
    }
  }

  async cleanup() {
    await this.stop();
    activeServerManagers.delete(this);
    if (this.ownsTempDir && !this.keepTmp && this.dataDir) {
      rmSync(this.dataDir, { recursive: true, force: true, maxRetries: 5, retryDelay: 40 });
    }
    if (activeServerManagers.size === 0) {
      detachServerSignalHandlers();
    }
  }
}

export class ApiClient {
  constructor(
    baseUrl,
    { authMode = 'bearer', token = null, cookie = null, apiKey = null, user = null } = {},
  ) {
    this.baseUrl = baseUrl.replace(/\/+$/, '');
    this.authMode = authMode;
    this.token = token;
    this.cookie = cookie;
    this.apiKey = apiKey;
    this.user = user;
  }

  withAuthMode(authMode) {
    return new ApiClient(this.baseUrl, {
      authMode,
      token: this.token,
      cookie: this.cookie,
      apiKey: this.apiKey,
      user: this.user,
    });
  }

  async request(
    method,
    reqPath,
    {
      body = undefined,
      headers = {},
      query = null,
      authMode = this.authMode,
      timeoutMs = 30000,
    } = {},
  ) {
    const normalizedPath = reqPath.startsWith('/') ? reqPath : `/${reqPath}`;
    const url = new URL(`${this.baseUrl}${normalizedPath}`);
    if (query && typeof query === 'object') {
      for (const [k, v] of Object.entries(query)) {
        if (v !== undefined && v !== null) {
          url.searchParams.set(k, String(v));
        }
      }
    }

    const reqHeaders = new Headers(headers);
    if (authMode === 'bearer' && this.token) {
      reqHeaders.set('Authorization', `Bearer ${this.token}`);
    } else if (authMode === 'cookie' && (this.cookie || this.token)) {
      reqHeaders.set('Cookie', `xmedia_token=${this.cookie || this.token}`);
    } else if (authMode === 'api_key' && this.apiKey) {
      reqHeaders.set('x-api-key', this.apiKey);
    } else if (authMode === 'query_token' && this.token) {
      url.searchParams.set('token', this.token);
    }

    let reqBody;
    if (body !== undefined) {
      if (
        typeof body === 'string' ||
        Buffer.isBuffer(body) ||
        body instanceof Uint8Array ||
        (typeof FormData !== 'undefined' && body instanceof FormData)
      ) {
        reqBody = body;
      } else {
        reqBody = JSON.stringify(body);
        if (!reqHeaders.has('Content-Type')) {
          reqHeaders.set('Content-Type', 'application/json');
        }
      }
    }

    const res = await fetch(url, {
      method,
      headers: reqHeaders,
      body: reqBody,
      redirect: 'manual',
      signal: AbortSignal.timeout(timeoutMs),
    });

    const setCookie = extractCookieValue(res.headers, 'xmedia_token');
    if (setCookie && authMode !== 'none') {
      this.cookie = setCookie;
    }

    const arrBuf = await res.arrayBuffer();
    const buffer = Buffer.from(arrBuf);
    const contentType = res.headers.get('content-type') || '';
    const isTextLike =
      contentType.includes('json') ||
      contentType.includes('text') ||
      contentType.includes('xml') ||
      contentType.includes('mpegurl') ||
      contentType.includes('javascript') ||
      buffer.length < 65536;
    const text = isTextLike ? buffer.toString('utf8') : '';
    let json = null;
    if (text && (contentType.includes('json') || text.trimStart().startsWith('{') || text.trimStart().startsWith('['))) {
      try {
        json = JSON.parse(text);
      } catch {
        json = null;
      }
    }

    return {
      status: res.status,
      ok: res.ok,
      headers: res.headers,
      buffer,
      text,
      json,
    };
  }

  async failLoudlyRequest(method, reqPath, opts = {}) {
    const { expectedStatus = null, allow404 = false, ...reqOpts } = opts;
    const res = await this.request(method, reqPath, reqOpts);

    const expectedList = Array.isArray(expectedStatus)
      ? expectedStatus
      : expectedStatus !== null
        ? [expectedStatus]
        : null;

    const is404Allowed =
      allow404 || (expectedList && (expectedList.includes(404) || expectedList.includes(405)));

    if ((res.status === 404 || res.status === 405) && !is404Allowed) {
      throw new SutEndpointMissingError(method, reqPath, res.status, res.text.slice(0, 200));
    }

    if (expectedList && !expectedList.includes(res.status)) {
      throw new Error(
        `Expected HTTP ${expectedList.join('|')} for ${method} ${reqPath}, got ${res.status}: ${res.text.slice(0, 400)}`,
      );
    }

    return res;
  }

  async getSetupStatus() {
    const res = await this.failLoudlyRequest('GET', '/api/v1/auth/setup-status', {
      authMode: 'none',
      expectedStatus: 200,
    });
    return res.json;
  }

  async setupAdmin({
    username = 'admin',
    display_name = 'E2E Admin',
    password = 'AdminPass123!',
  } = {}) {
    const res = await this.failLoudlyRequest('POST', '/api/v1/auth/setup', {
      authMode: 'none',
      body: { username, display_name, password },
      expectedStatus: 201,
    });
    this.token = res.json.token;
    this.cookie = extractCookieValue(res.headers, 'xmedia_token') || res.json.token;
    this.apiKey = res.json.user?.api_key || null;
    this.user = res.json.user || null;
    return res.json;
  }

  async createMemberUser({
    username = 'member',
    display_name = 'E2E Member',
    password = 'UserPass123!',
    role = 'member',
  } = {}) {
    const res = await this.failLoudlyRequest('POST', '/api/v1/users', {
      body: { username, display_name, password, role },
      expectedStatus: 201,
    });
    return res.json;
  }

  async login({ username, password }) {
    const res = await this.failLoudlyRequest('POST', '/api/v1/auth/login', {
      authMode: 'none',
      body: { username, password },
      expectedStatus: 200,
    });
    this.token = res.json.token;
    this.cookie = extractCookieValue(res.headers, 'xmedia_token') || res.json.token;
    this.apiKey = res.json.user?.api_key || null;
    this.user = res.json.user || null;
    return res.json;
  }

  async loginAs({ username, password, authMode = this.authMode }) {
    const client = new ApiClient(this.baseUrl, { authMode });
    await client.login({ username, password });
    return client;
  }
}
