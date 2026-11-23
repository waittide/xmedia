import assert from 'node:assert/strict';
import {
  buildMockComicSources,
  buildMockMusicSources,
  buildMockVideoSources,
} from '../fixtures/mock_sources.mjs';
import { ApiClient, SutEndpointMissingError } from './client.mjs';
import { crc32 } from './fixtures.mjs';

export function buildMinimalZip(entries) {
  const localParts = [];
  const centralParts = [];
  let offset = 0;

  for (const { name, data } of entries) {
    const nameBuf = Buffer.from(name, 'utf8');
    const dataBuf = Buffer.isBuffer(data) ? data : Buffer.from(data);
    const crc = crc32(dataBuf);

    const localHdr = Buffer.alloc(30);
    localHdr.writeUInt32LE(0x04034b50, 0);
    localHdr.writeUInt16LE(20, 4);
    localHdr.writeUInt16LE(0x0800, 6); // UTF-8 filename flag
    localHdr.writeUInt16LE(0, 8); // STORE
    localHdr.writeUInt32LE(crc, 14);
    localHdr.writeUInt32LE(dataBuf.length, 18);
    localHdr.writeUInt32LE(dataBuf.length, 22);
    localHdr.writeUInt16LE(nameBuf.length, 26);
    localParts.push(localHdr, nameBuf, dataBuf);

    const centralHdr = Buffer.alloc(46);
    centralHdr.writeUInt32LE(0x02014b50, 0);
    centralHdr.writeUInt16LE(20, 4);
    centralHdr.writeUInt16LE(20, 6);
    centralHdr.writeUInt16LE(0x0800, 8);
    centralHdr.writeUInt32LE(crc, 16);
    centralHdr.writeUInt32LE(dataBuf.length, 20);
    centralHdr.writeUInt32LE(dataBuf.length, 24);
    centralHdr.writeUInt16LE(nameBuf.length, 28);
    centralHdr.writeUInt32LE(offset, 42);
    centralParts.push(centralHdr, nameBuf);

    offset += localHdr.length + nameBuf.length + dataBuf.length;
  }

  const centralDir = Buffer.concat(centralParts);
  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0);
  eocd.writeUInt16LE(entries.length, 8);
  eocd.writeUInt16LE(entries.length, 10);
  eocd.writeUInt32LE(centralDir.length, 12);
  eocd.writeUInt32LE(offset, 16);

  return Buffer.concat([...localParts, centralDir, eocd]);
}

export function buildMultipartForm(files, textFields = {}) {
  const boundary = `----XMediaE2E${Date.now().toString(16)}${Math.random().toString(16).slice(2)}`;
  const chunks = [];

  for (const [k, v] of Object.entries(textFields)) {
    chunks.push(
      Buffer.from(
        `--${boundary}\r\nContent-Disposition: form-data; name="${k}"\r\n\r\n${String(v)}\r\n`,
        'utf8',
      ),
    );
  }

  for (const { fieldName = 'file', filename, contentType, data } of files) {
    chunks.push(
      Buffer.from(
        `--${boundary}\r\nContent-Disposition: form-data; name="${fieldName}"; filename="${filename}"\r\nContent-Type: ${contentType}\r\n\r\n`,
        'utf8',
      ),
    );
    chunks.push(Buffer.isBuffer(data) ? data : Buffer.from(data));
    chunks.push(Buffer.from('\r\n', 'utf8'));
  }

  chunks.push(Buffer.from(`--${boundary}--\r\n`, 'utf8'));
  return {
    contentType: `multipart/form-data; boundary=${boundary}`,
    body: Buffer.concat(chunks),
  };
}

export async function bootstrapClients(baseUrl, ApiClientCtor = ApiClient) {
  const admin = new ApiClientCtor(baseUrl, { authMode: 'bearer' });
  const status = await admin.getSetupStatus();
  if (status.needs_setup) {
    await admin.setupAdmin({
      username: 'admin',
      display_name: 'E2E Admin',
      password: 'AdminPass123!',
    });
    await admin.createMemberUser({
      username: 'member',
      display_name: 'E2E Member',
      password: 'UserPass123!',
      role: 'member',
    });
  } else {
    await admin.login({ username: 'admin', password: 'AdminPass123!' });
    const createRes = await admin.request('POST', '/api/v1/users', {
      body: {
        username: 'member',
        display_name: 'E2E Member',
        password: 'UserPass123!',
        role: 'member',
      },
    });
    if (createRes.status === 404 || createRes.status === 405) {
      throw new SutEndpointMissingError('POST', '/api/v1/users', createRes.status, createRes.text);
    }
  }
  const member = await admin.loginAs({
    username: 'member',
    password: 'UserPass123!',
    authMode: 'bearer',
  });
  return { admin, member };
}

export async function ensureBootstrapped(baseUrl, adminCreds, memberCreds) {
  const probe = new ApiClient(baseUrl, { authMode: 'none' });
  const status = await probe.getSetupStatus();
  const admin = new ApiClient(baseUrl, { authMode: 'bearer' });
  if (status.needs_setup) {
    await admin.setupAdmin(adminCreds);
  } else {
    await admin.login({ username: adminCreds.username, password: adminCreds.password });
  }

  const member = new ApiClient(baseUrl, { authMode: 'bearer' });
  const createRes = await admin.request('POST', '/api/v1/users', { body: memberCreds });
  if (createRes.status === 404 || createRes.status === 405) {
    throw new SutEndpointMissingError('POST', '/api/v1/users', createRes.status, createRes.text);
  }
  await member.login({ username: memberCreds.username, password: memberCreds.password });
  return { admin, member };
}

export async function resetMockRequests(mockBaseUrl) {
  const res = await fetch(`${mockBaseUrl}/__mock/reset`, { method: 'POST' });
  assert.equal(res.status, 200);
}

export async function getMockRequests(mockBaseUrl, limit = 200) {
  const res = await fetch(`${mockBaseUrl}/__mock/requests?limit=${limit}`);
  assert.equal(res.status, 200);
  return res.json();
}

export async function importSources(admin, domain, payload) {
  const rawJson = typeof payload === 'string' ? payload : JSON.stringify(payload);
  const res = await admin.failLoudlyRequest('POST', `/api/v1/${domain}/sources/import`, {
    body: { raw_json: rawJson },
    expectedStatus: 200,
  });
  return res.json;
}

export async function importSource(admin, domain, sourceObj) {
  const imported = await importSources(admin, domain, sourceObj);
  const list = imported?.sources || [];
  assert.ok(list.length >= 1, `Expected imported source in response for ${domain}`);
  const targetName = typeof sourceObj === 'object' ? (sourceObj.sourceName || sourceObj.name) : null;
  return (targetName && list.find((s) => s.name === targetName)) || list[0];
}

export async function deleteSourceSafe(admin, domain, id) {
  if (!id) return;
  await admin.request('DELETE', `/api/v1/${domain}/sources/${id}`).catch(() => {});
}

export async function clearDomainSources(admin, domain) {
  const res = await admin.failLoudlyRequest('GET', `/api/v1/${domain}/sources`, {
    expectedStatus: 200,
  });
  const list = Array.isArray(res.json) ? res.json : [];
  if (list.length > 0) {
    await admin.failLoudlyRequest('POST', `/api/v1/${domain}/sources/batch`, {
      body: { ids: list.map((s) => s.id), action: 'delete' },
      expectedStatus: 200,
    });
  }
}

export async function ensureComicMocks(admin, mockBaseUrl, opts = {}) {
  await clearDomainSources(admin, 'comic');
  const sources = buildMockComicSources(mockBaseUrl, opts);
  const imported = await importSources(admin, 'comic', sources.all);
  const byName = Object.fromEntries(imported.sources.map((s) => [s.name, s]));
  return {
    mangadex: byName['Mock MangaDex'],
    webtoon: byName['Mock WEBTOON'],
    xkcd: byName['Mock XKCD'],
    all: imported.sources,
  };
}

export async function ensureMusicMocks(admin, mockBaseUrl) {
  await clearDomainSources(admin, 'music');
  const sources = buildMockMusicSources(mockBaseUrl);
  const imported = await importSources(admin, 'music', sources.all);
  const byName = Object.fromEntries(imported.sources.map((s) => [s.name, s]));
  return {
    gdstudio: byName['Mock GDStudio'],
    audius: byName['Mock Audius'],
    all: imported.sources,
  };
}

export async function ensureVideoMocks(admin, mockBaseUrl) {
  await clearDomainSources(admin, 'video');
  const sources = buildMockVideoSources(mockBaseUrl);
  const imported = await importSources(admin, 'video', [
    sources.applecms,
    sources.peertube,
    sources.applecmsToken,
  ]);
  const byName = Object.fromEntries(imported.sources.map((s) => [s.name, s]));
  return {
    applecms: byName['Mock AppleCMS'],
    applecmsToken: byName['Mock AppleCMS Token'],
    peertube: byName['Mock PeerTube'],
    all: imported.sources,
  };
}

export function toRelativeApiPath(urlOrPath) {
  if (!urlOrPath) return '';
  if (urlOrPath.startsWith('http://') || urlOrPath.startsWith('https://')) {
    const u = new URL(urlOrPath);
    return `${u.pathname}${u.search}`;
  }
  return urlOrPath.startsWith('/') ? urlOrPath : `/${urlOrPath}`;
}

export async function openAuthenticatedPage(CdpCtor, baseUrl, client, targetPath = '/') {
  const cdp = new CdpCtor();
  try {
    await cdp.launch();
    const cookieVal = client.cookie || client.token;
    assert.ok(cookieVal, 'Client must have cookie or token for browser session');
    await cdp.setCookie({
      name: 'xmedia_token',
      value: cookieVal,
      url: baseUrl,
      httpOnly: true,
      sameSite: 'Lax',
    });
    await cdp.goto(`${baseUrl}${targetPath}`);
    const isFullscreen = /^\/(?:login|setup|(?:comic|book|video)\/(?:(?:online\/)?(?:read|watch)|play\/))(?=[/?#]|$)/.test(
      targetPath,
    );
    await cdp.waitForSelector(isFullscreen ? '#app > *' : '[data-workspace-shell]', {
      visible: true,
      timeoutMs: 12000,
    });
    return cdp;
  } catch (err) {
    await cdp.close().catch(() => {});
    throw err;
  }
}

export async function clickButtonMatching(cdp, patterns, { rootSelector = 'body', timeoutMs = 10000 } = {}) {
  const patStrings = patterns.map((p) => (p instanceof RegExp ? p.source : String(p)));
  await cdp.waitForFn(
    `(() => {
      const root = document.querySelector(${JSON.stringify(rootSelector)}) || document;
      const regs = ${JSON.stringify(patStrings)}.map((s) => new RegExp(s, 'i'));
      const isVisible = (el) => {
        const st = getComputedStyle(el);
        if (st.display === 'none' || st.visibility === 'hidden') return false;
        const r = el.getBoundingClientRect();
        return r.width > 0 && r.height > 0;
      };
      const matchesEl = (el) => {
        const candidates = [
          el.textContent || '',
          el.getAttribute('aria-label') || '',
          el.getAttribute('title') || '',
        ].map((s) => s.trim()).filter(Boolean);
        return candidates.some((txt) => regs.some((r) => r.test(txt)));
      };
      const primary = [...root.querySelectorAll('button, a, [role="link"], [role="button"], [role="menuitem"], [role="tab"], [role="option"], [role="checkbox"], label')];
      let match = primary.find((el) => isVisible(el) && matchesEl(el));
      if (!match) {
        const fallback = [...root.querySelectorAll('tr, td')];
        match = fallback.find((el) => isVisible(el) && matchesEl(el));
      }
      if (!match) return false;
      match.scrollIntoView({ block: 'center' });
      match.click();
      return true;
    })()`,
    { timeoutMs, label: `clickButtonMatching(${patStrings.join('|')})` },
  );
}

export async function browserLogin(cdp, baseUrl, creds) {
  await cdp.goto(`${baseUrl}/login`);
  await cdp.waitForSelector('input[autocomplete="username"]', { visible: true });
  assert.equal(
    cdp.exceptions.length,
    0,
    `Unexpected JS exceptions before login: ${JSON.stringify(cdp.exceptions)}`,
  );
  const unexpectedPreHttp = cdp.httpErrors.filter(
    (e) => !(e.status === 401 && String(e.url || '').includes('/api/v1/auth/me')),
  );
  assert.equal(
    unexpectedPreHttp.length,
    0,
    `Unexpected HTTP errors before login: ${JSON.stringify(unexpectedPreHttp)}`,
  );
  cdp.clearErrors();
  await cdp.type('input[autocomplete="username"]', creds.username);
  await cdp.type('input[autocomplete="current-password"]', creds.password);
  await cdp.click('button[type="submit"]');
  await cdp.waitForFn(() => !window.location.pathname.startsWith('/login'), {
    timeoutMs: 10000,
    label: 'browser redirect after login',
  });
  await cdp.waitForSelector('[data-workspace-shell]', { visible: true, timeoutMs: 12000 });
}

export async function clickButtonByText(cdp, texts, { timeoutMs = 8000 } = {}) {
  const list = Array.isArray(texts) ? texts : [texts];
  const clicked = await cdp.waitForFn(
    `(() => {
      const targets = ${JSON.stringify(list)}.map(s => s.toLowerCase());
      const isVisible = (el) => {
        if (el.disabled || el.getAttribute('aria-disabled') === 'true') return false;
        const st = getComputedStyle(el);
        if (st.display === 'none' || st.visibility === 'hidden') return false;
        const r = el.getBoundingClientRect();
        return r.width > 0 && r.height > 0;
      };
      const getCandidates = (el) =>
        [
          el.textContent || '',
          el.getAttribute('aria-label') || '',
          el.getAttribute('title') || '',
        ]
          .map((s) => s.trim().toLowerCase())
          .filter(Boolean);
      const matchesExact = (el) => {
        if (getCandidates(el).some((c) => targets.includes(c))) return true;
        return [...el.querySelectorAll('h1, h2, h3, h4, p, span, strong')].some((child) =>
          targets.includes((child.textContent || '').trim().toLowerCase()),
        );
      };
      const matchesIncludes = (el) =>
        getCandidates(el).some((c) => targets.some((t) => c.includes(t)));
      const primary = [...document.querySelectorAll('button, [role="link"], [role="button"], [role="tab"], [role="menuitem"], a')];
      for (const matcher of [matchesExact, matchesIncludes]) {
        for (const el of primary) {
          if (isVisible(el) && matcher(el)) {
            el.scrollIntoView({ block: 'center' });
            el.click();
            return true;
          }
        }
      }
      const fallback = [...document.querySelectorAll('tr, td')];
      for (const matcher of [matchesExact, matchesIncludes]) {
        for (const el of fallback) {
          if (isVisible(el) && matcher(el)) {
            el.scrollIntoView({ block: 'center' });
            el.click();
            return true;
          }
        }
      }
      return false;
    })()`,
    { timeoutMs, pollMs: 100, label: `clickButtonByText(${list.join('|')})` },
  );
  assert.equal(clicked, true);
}
