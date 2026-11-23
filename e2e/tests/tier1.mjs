import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import {
  buildEdgeMockSources,
  buildMockBookSources,
  buildMockComicSources,
  buildMockMusicSources,
  buildMockVideoSources,
} from '../fixtures/mock_sources.mjs';
import { CdpClient } from '../lib/cdp.mjs';
import {
  ApiClient,
  REPO_ROOT,
  SutEndpointMissingError,
  XMediaServerManager,
  resolveWebDir,
} from '../lib/client.mjs';
import {
  MOCK_AES_KEY_BYTES,
  MOCK_JPEG_BYTES,
  MOCK_MP3_BYTES,
  MOCK_MP4_BYTES,
  MOCK_PNG_BYTES,
  assertJpeg,
  assertM3u8Rewritten,
  assertMp3,
  assertMp4,
  assertMpegTs,
  assertPng,
  assertRasterImage,
  assertSecurityHeaders,
  decryptAes128Cbc,
} from '../lib/fixtures.mjs';
import { MockOriginServer } from '../lib/mock_origin.mjs';

const execFileAsync = promisify(execFile);

// ---------------------------------------------------------------------------
// Shared helpers for Tier 1 opaque-box assertions
// ---------------------------------------------------------------------------

function crc32Bytes(buf) {
  let c = 0xffffffff;
  for (let i = 0; i < buf.length; i += 1) {
    c ^= buf[i];
    for (let k = 0; k < 8; k += 1) {
      c = (c & 1) !== 0 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    }
  }
  return (c ^ 0xffffffff) >>> 0;
}

function buildMinimalZip(entries) {
  const localParts = [];
  const centralParts = [];
  let offset = 0;

  for (const { name, data } of entries) {
    const nameBuf = Buffer.from(name, 'utf8');
    const dataBuf = Buffer.isBuffer(data) ? data : Buffer.from(data);
    const crc = crc32Bytes(dataBuf);

    const localHeader = Buffer.alloc(30);
    localHeader.writeUInt32LE(0x04034b50, 0);
    localHeader.writeUInt16LE(20, 4);
    localHeader.writeUInt16LE(0, 6);
    localHeader.writeUInt16LE(0, 8); // STORE
    localHeader.writeUInt16LE(0, 10);
    localHeader.writeUInt16LE(0, 12);
    localHeader.writeUInt32LE(crc, 14);
    localHeader.writeUInt32LE(dataBuf.length, 18);
    localHeader.writeUInt32LE(dataBuf.length, 22);
    localHeader.writeUInt16LE(nameBuf.length, 26);
    localHeader.writeUInt16LE(0, 28);

    const localRecord = Buffer.concat([localHeader, nameBuf, dataBuf]);
    localParts.push(localRecord);

    const centralHeader = Buffer.alloc(46);
    centralHeader.writeUInt32LE(0x02014b50, 0);
    centralHeader.writeUInt16LE(20, 4);
    centralHeader.writeUInt16LE(20, 6);
    centralHeader.writeUInt16LE(0, 8);
    centralHeader.writeUInt16LE(0, 10);
    centralHeader.writeUInt16LE(0, 12);
    centralHeader.writeUInt16LE(0, 14);
    centralHeader.writeUInt32LE(crc, 16);
    centralHeader.writeUInt32LE(dataBuf.length, 20);
    centralHeader.writeUInt32LE(dataBuf.length, 24);
    centralHeader.writeUInt16LE(nameBuf.length, 28);
    centralHeader.writeUInt16LE(0, 30);
    centralHeader.writeUInt16LE(0, 32);
    centralHeader.writeUInt16LE(0, 34);
    centralHeader.writeUInt16LE(0, 36);
    centralHeader.writeUInt32LE(0, 38);
    centralHeader.writeUInt32LE(offset, 42);

    centralParts.push(Buffer.concat([centralHeader, nameBuf]));
    offset += localRecord.length;
  }

  const centralDir = Buffer.concat(centralParts);
  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0);
  eocd.writeUInt16LE(0, 4);
  eocd.writeUInt16LE(0, 6);
  eocd.writeUInt16LE(entries.length, 8);
  eocd.writeUInt16LE(entries.length, 10);
  eocd.writeUInt32LE(centralDir.length, 12);
  eocd.writeUInt32LE(offset, 16);
  eocd.writeUInt16LE(0, 20);

  return Buffer.concat([...localParts, centralDir, eocd]);
}

function buildMultipartForm(files, textFields = {}) {
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

async function bootstrapClients(baseUrl, ApiClientCtor = ApiClient) {
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

async function resetMockRequests(mockBaseUrl) {
  const res = await fetch(`${mockBaseUrl}/__mock/reset`, { method: 'POST' });
  assert.equal(res.status, 200);
}

async function getMockRequests(mockBaseUrl, limit = 200) {
  const res = await fetch(`${mockBaseUrl}/__mock/requests?limit=${limit}`);
  assert.equal(res.status, 200);
  return res.json();
}

async function importSources(admin, domain, payload) {
  const rawJson = typeof payload === 'string' ? payload : JSON.stringify(payload);
  const res = await admin.failLoudlyRequest('POST', `/api/v1/${domain}/sources/import`, {
    body: { raw_json: rawJson },
    expectedStatus: 200,
  });
  return res.json;
}

async function clearDomainSources(admin, domain) {
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

async function ensureComicMocks(admin, mockBaseUrl, opts = {}) {
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

async function ensureMusicMocks(admin, mockBaseUrl) {
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

async function ensureVideoMocks(admin, mockBaseUrl) {
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

async function openAuthenticatedPage(CdpCtor, baseUrl, client, targetPath = '/') {
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

async function clickButtonMatching(cdp, patterns, { rootSelector = 'body', timeoutMs = 10000 } = {}) {
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

// ---------------------------------------------------------------------------
// Tier 1 Case Registry (90 cases: T1-F01-01 .. T1-F18-05)
// ---------------------------------------------------------------------------

function buildTier1Cases() {
  return [
    // =======================================================================
    // F1: WebAuth, Admin RBAC & Secret Masking (core)
    // =======================================================================
    {
      id: 'T1-F01-01',
      feature: 'F1',
      domain: 'core',
      title: 'Admin GET /api/v1/comic/sources via Cookie auth returns full metadata',
      async run(ctx) {
        await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const cookieClient = ctx.admin.withAuthMode('cookie');
        const res = await cookieClient.failLoudlyRequest('GET', '/api/v1/comic/sources', {
          expectedStatus: 200,
        });
        assert.ok(Array.isArray(res.json) && res.json.length >= 1, 'Expected non-empty comic sources array');
        const first = res.json[0];
        assert.ok(first.id && first.name && first.source_url, 'Source summary missing id/name/source_url');
        assert.equal(typeof first.enabled, 'boolean');
        assert.equal(typeof first.weight, 'number');
        assert.ok('login_info' in first && 'login_ui' in first && 'cookie_hosts' in first);
      },
    },
    {
      id: 'T1-F01-02',
      feature: 'F1',
      domain: 'core',
      title: 'Admin GET /api/v1/music/sources via Authorization Bearer JWT returns sources',
      async run(ctx) {
        await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        const bearerClient = ctx.admin.withAuthMode('bearer');
        const res = await bearerClient.failLoudlyRequest('GET', '/api/v1/music/sources', {
          expectedStatus: 200,
        });
        assert.ok(Array.isArray(res.json) && res.json.length >= 1);
        assert.ok(res.json.some((s) => s.name === 'Mock GDStudio'));
      },
    },
    {
      id: 'T1-F01-03',
      feature: 'F1',
      domain: 'core',
      title: 'Admin GET /api/v1/video/sources via x-api-key returns sources',
      async run(ctx) {
        await ensureVideoMocks(ctx.admin, ctx.mockBaseUrl);
        const keyClient = ctx.admin.withAuthMode('api_key');
        const res = await keyClient.failLoudlyRequest('GET', '/api/v1/video/sources', {
          expectedStatus: 200,
        });
        assert.ok(Array.isArray(res.json) && res.json.length >= 1);
        assert.ok(res.json.some((s) => s.name === 'Mock AppleCMS'));
      },
    },
    {
      id: 'T1-F01-04',
      feature: 'F1',
      domain: 'core',
      title: 'GET /api/v1/comic/online/image-proxy accepts ?token=<jwt> on GET',
      async run(ctx) {
        const mocks = await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const queryClient = ctx.member.withAuthMode('query_token');
        const res = await queryClient.failLoudlyRequest('GET', '/api/v1/comic/online/image-proxy', {
          query: {
            url: `${ctx.mockBaseUrl}/comic/xkcd/strip.png`,
            source_id: mocks.xkcd.id,
          },
          expectedStatus: 200,
        });
        assertPng(res.buffer, 'image-proxy via ?token=');
      },
    },
    {
      id: 'T1-F01-05',
      feature: 'F1',
      domain: 'core',
      title: 'Non-admin GET /sources returns enabled only and strips secret metadata across all 3 domains',
      async run(ctx) {
        for (const domain of ['comic', 'music', 'video']) {
          await clearDomainSources(ctx.admin, domain);
          const secretSrc = {
            sourceName: `Secret ${domain} Enabled`,
            sourceUrl: `${ctx.mockBaseUrl}/${domain}/secret-enabled`,
            enabled: true,
            loginUrl: `${ctx.mockBaseUrl}/edge/auth/login`,
            loginUi: JSON.stringify([{ name: 'username', type: 'text' }]),
            searchUrl: `${ctx.mockBaseUrl}/comic/xkcd/info.0.json`,
            apiUrl: `${ctx.mockBaseUrl}/video/applecms/api.php/provide/vod/`,
            sourceFormat: domain === 'video' ? 'applecms_v10' : undefined,
          };
          const disabledSrc = {
            sourceName: `Disabled ${domain}`,
            sourceUrl: `${ctx.mockBaseUrl}/${domain}/disabled-hidden`,
            enabled: false,
            searchUrl: `${ctx.mockBaseUrl}/comic/xkcd/info.0.json`,
            apiUrl: `${ctx.mockBaseUrl}/video/applecms/api.php/provide/vod/`,
            sourceFormat: domain === 'video' ? 'applecms_v10' : undefined,
          };
          const imp = await importSources(ctx.admin, domain, [secretSrc, disabledSrc]);
          const enabledId = imp.sources.find((s) => s.name.includes('Enabled')).id;
          await ctx.admin.failLoudlyRequest('PATCH', `/api/v1/${domain}/sources/${enabledId}`, {
            body: { variable: '{"secret_api_key":"top_secret_val"}' },
            expectedStatus: 200,
          });

          const memberRes = await ctx.member.failLoudlyRequest('GET', `/api/v1/${domain}/sources`, {
            expectedStatus: 200,
          });
          assert.equal(memberRes.json.length, 1, `${domain}: non-admin should only see enabled source`);
          const item = memberRes.json[0];
          assert.equal(item.enabled, true);
          assert.deepEqual(item.login_info || {}, {});
          assert.deepEqual(item.login_ui || [], []);
          assert.equal(item.variable_preview || '', '');
          assert.deepEqual(item.cookie_hosts || [], []);
        }
      },
    },

    // =======================================================================
    // F2: SSRF Guard, Redirect Policy & Sandbox Limits (core)
    // =======================================================================
    {
      id: 'T1-F02-01',
      feature: 'F2',
      domain: 'core',
      sut: 'hardened',
      title: 'Hardened SUT blocks 127.0.0.1 upstream in comic online search without contacting mock',
      async run(ctx) {
        const mocks = await ensureComicMocks(ctx.hardenedAdmin, ctx.mockBaseUrl);
        await resetMockRequests(ctx.mockBaseUrl);
        const searchRes = await ctx.hardenedAdmin.failLoudlyRequest(
          'POST',
          '/api/v1/comic/online/search',
          {
            body: { keyword: 'Naruto', source_ids: [mocks.xkcd.id] },
            expectedStatus: 200,
          },
        );
        const items = searchRes.json.results ?? searchRes.json.items ?? [];
        const errors = searchRes.json.errors ?? [];
        assert.equal(items.length, 0, 'Hardened SUT must not return results from 127.0.0.1');
        assert.ok(errors.length >= 1, 'Hardened SUT must report SSRF error for 127.0.0.1');
        const logged = await getMockRequests(ctx.mockBaseUrl);
        assert.equal(logged.length, 0, 'Hardened SUT must never open TCP connection to 127.0.0.1');
      },
    },
    {
      id: 'T1-F02-02',
      feature: 'F2',
      domain: 'core',
      sut: 'hardened',
      title: 'Hardened SUT blocks cloud metadata IP 169.254.169.254 in image-proxy',
      async run(ctx) {
        const res = await ctx.hardenedAdmin.failLoudlyRequest(
          'GET',
          '/api/v1/comic/online/image-proxy',
          {
            query: { url: 'http://169.254.169.254/latest/meta-data/' },
            expectedStatus: [400, 403],
          },
        );
        assert.ok(res.json?.error?.code, 'Expected structured JSON error envelope');
      },
    },
    {
      id: 'T1-F02-03',
      feature: 'F2',
      domain: 'core',
      sut: 'hardened',
      title: 'Hardened SUT blocks localhost hostname in audio-proxy',
      async run(ctx) {
        const res = await ctx.hardenedAdmin.failLoudlyRequest(
          'GET',
          '/api/v1/music/online/audio-proxy',
          {
            query: { url: 'http://localhost:8090/api/v1/auth/me' },
            expectedStatus: [400, 403],
          },
        );
        assert.ok(res.json?.error?.code);
      },
    },
    {
      id: 'T1-F02-04',
      feature: 'F2',
      domain: 'core',
      sut: 'hardened',
      title: 'Hardened SUT blocks IPv6 loopback [::1] in hls-proxy',
      async run(ctx) {
        const res = await ctx.hardenedAdmin.failLoudlyRequest(
          'GET',
          '/api/v1/video/online/hls-proxy',
          {
            query: { url: 'http://[::1]:8090/test.m3u8' },
            expectedStatus: [400, 403],
          },
        );
        assert.ok(res.json?.error?.code);
      },
    },
    {
      id: 'T1-F02-05',
      feature: 'F2',
      domain: 'core',
      title: 'Mock SUT allows valid same-origin 302 redirect on image-proxy',
      async run(ctx) {
        const mocks = await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('GET', '/api/v1/comic/online/image-proxy', {
          query: {
            url: `${ctx.mockBaseUrl}/edge/redirect-valid?to=/comic/xkcd/strip.png`,
            source_id: mocks.xkcd.id,
          },
          expectedStatus: 200,
        });
        assertPng(res.buffer, 'redirect-valid image-proxy');
      },
    },

    // =======================================================================
    // F3: Source Lifecycle CRUD (core)
    // =======================================================================
    {
      id: 'T1-F03-01',
      feature: 'F3',
      domain: 'core',
      title: 'Admin imports single JSON source object across comic, music, video',
      async run(ctx) {
        const comics = buildMockComicSources(ctx.mockBaseUrl);
        const musics = buildMockMusicSources(ctx.mockBaseUrl);
        const videos = buildMockVideoSources(ctx.mockBaseUrl);
        for (const [domain, src] of [
          ['comic', comics.xkcd],
          ['music', musics.gdstudio],
          ['video', videos.applecms],
        ]) {
          await clearDomainSources(ctx.admin, domain);
          const rep = await importSources(ctx.admin, domain, src);
          assert.equal(rep.imported_count, 1, `${domain}: imported_count`);
          assert.equal(rep.updated_count, 0, `${domain}: updated_count`);
          assert.equal(rep.sources.length, 1);
        }
      },
    },
    {
      id: 'T1-F03-02',
      feature: 'F3',
      domain: 'core',
      title: 'Admin imports JSON array of sources across comic, music, video',
      async run(ctx) {
        const comics = buildMockComicSources(ctx.mockBaseUrl);
        const musics = buildMockMusicSources(ctx.mockBaseUrl);
        const videos = buildMockVideoSources(ctx.mockBaseUrl);
        for (const [domain, arr] of [
          ['comic', [comics.mangadex, comics.webtoon]],
          ['music', [musics.gdstudio, musics.audius]],
          ['video', [videos.applecms, videos.peertube]],
        ]) {
          await clearDomainSources(ctx.admin, domain);
          const rep = await importSources(ctx.admin, domain, arr);
          assert.equal(rep.imported_count, 2, `${domain}: imported_count`);
          const listRes = await ctx.admin.failLoudlyRequest('GET', `/api/v1/${domain}/sources`, {
            expectedStatus: 200,
          });
          assert.equal(listRes.json.length, 2);
        }
      },
    },
    {
      id: 'T1-F03-03',
      feature: 'F3',
      domain: 'core',
      title: 'Admin PATCH updates enabled/weight/group_name and export reflects raw_json',
      async run(ctx) {
        const comics = buildMockComicSources(ctx.mockBaseUrl);
        const musics = buildMockMusicSources(ctx.mockBaseUrl);
        const videos = buildMockVideoSources(ctx.mockBaseUrl);
        for (const [domain, src] of [
          ['comic', comics.xkcd],
          ['music', musics.gdstudio],
          ['video', videos.applecms],
        ]) {
          await clearDomainSources(ctx.admin, domain);
          const imp = await importSources(ctx.admin, domain, src);
          const id = imp.sources[0].id;
          const patched = await ctx.admin.failLoudlyRequest(
            'PATCH',
            `/api/v1/${domain}/sources/${id}`,
            {
              body: { enabled: false, weight: 77, group_name: 'PatchedGroup' },
              expectedStatus: 200,
            },
          );
          assert.equal(patched.json.enabled, false);
          assert.equal(patched.json.weight, 77);
          assert.equal(patched.json.group_name, 'PatchedGroup');

          const exp = await ctx.admin.failLoudlyRequest(
            'GET',
            `/api/v1/${domain}/sources/${id}/export`,
            { expectedStatus: 200 },
          );
          const parsed = JSON.parse(exp.text);
          assert.equal(parsed.enabled, false);
          assert.equal(parsed.weight, 77);
          assert.equal(parsed.sourceGroup, 'PatchedGroup');
        }
      },
    },
    {
      id: 'T1-F03-04',
      feature: 'F3',
      domain: 'core',
      title: 'Admin reorder and batch enable/disable/set_group/delete across all 3 domains',
      async run(ctx) {
        const comics = buildMockComicSources(ctx.mockBaseUrl);
        const musics = buildMockMusicSources(ctx.mockBaseUrl);
        const videos = buildMockVideoSources(ctx.mockBaseUrl);
        for (const [domain, arr] of [
          ['comic', [comics.mangadex, comics.webtoon]],
          ['music', [musics.gdstudio, musics.audius]],
          ['video', [videos.applecms, videos.peertube]],
        ]) {
          await clearDomainSources(ctx.admin, domain);
          const imp = await importSources(ctx.admin, domain, arr);
          const ids = imp.sources.map((s) => s.id);
          const reversed = [...ids].reverse();
          const reord = await ctx.admin.failLoudlyRequest('POST', `/api/v1/${domain}/sources/reorder`, {
            body: { ordered_ids: reversed },
            expectedStatus: 200,
          });
          assert.deepEqual(
            reord.json.map((s) => s.id),
            reversed,
          );

          await ctx.admin.failLoudlyRequest('POST', `/api/v1/${domain}/sources/batch`, {
            body: { ids: [ids[0]], action: 'disable' },
            expectedStatus: 200,
          });
          await ctx.admin.failLoudlyRequest('POST', `/api/v1/${domain}/sources/batch`, {
            body: { ids: [ids[0]], action: 'enable' },
            expectedStatus: 200,
          });
          await ctx.admin.failLoudlyRequest('POST', `/api/v1/${domain}/sources/batch`, {
            body: { ids, action: 'set_group', group_name: 'BatchGrp' },
            expectedStatus: 200,
          });
          const afterSet = await ctx.admin.failLoudlyRequest('GET', `/api/v1/${domain}/sources`, {
            expectedStatus: 200,
          });
          assert.ok(afterSet.json.every((s) => s.group_name === 'BatchGrp' && s.enabled === true));

          await ctx.admin.failLoudlyRequest('POST', `/api/v1/${domain}/sources/batch`, {
            body: { ids: [ids[1]], action: 'delete' },
            expectedStatus: 200,
          });
          const afterDel = await ctx.admin.failLoudlyRequest('GET', `/api/v1/${domain}/sources`, {
            expectedStatus: 200,
          });
          assert.equal(afterDel.json.length, 1);
        }
      },
    },
    {
      id: 'T1-F03-05',
      feature: 'F3',
      domain: 'core',
      title: 'Admin export-batch returns JSON array and DELETE /sources/{id} removes source',
      async run(ctx) {
        const comics = buildMockComicSources(ctx.mockBaseUrl);
        await clearDomainSources(ctx.admin, 'comic');
        const imp = await importSources(ctx.admin, 'comic', [comics.xkcd, comics.mangadex]);
        const ids = imp.sources.map((s) => s.id);

        const exp = await ctx.admin.failLoudlyRequest('POST', '/api/v1/comic/sources/export-batch', {
          body: { ids },
          expectedStatus: 200,
        });
        const exportedArr = JSON.parse(exp.text);
        assert.ok(Array.isArray(exportedArr) && exportedArr.length === 2);

        const delRes = await ctx.admin.failLoudlyRequest(
          'DELETE',
          `/api/v1/comic/sources/${ids[0]}`,
          { expectedStatus: [200, 204] },
        );
        assert.ok([200, 204].includes(delRes.status));
        const remaining = await ctx.admin.failLoudlyRequest('GET', '/api/v1/comic/sources', {
          expectedStatus: 200,
        });
        assert.equal(remaining.json.length, 1);
        assert.equal(remaining.json[0].id, ids[1]);
      },
    },

    // =======================================================================
    // F4: Rule Engine, URL Options, Charset & QuickJS (core)
    // =======================================================================
    {
      id: 'T1-F04-01',
      feature: 'F4',
      domain: 'core',
      title: 'JSONPath rules and POST /sources/{id}/test mode=search return ok and extracted items',
      async run(ctx) {
        const mocks = await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.admin.failLoudlyRequest(
          'POST',
          `/api/v1/comic/sources/${mocks.mangadex.id}/test`,
          {
            body: { mode: 'search', keyword: 'Naruto' },
            expectedStatus: 200,
          },
        );
        assert.equal(res.json.ok, true);
        assert.ok(Array.isArray(res.json.steps) && res.json.steps.length >= 1);
        assert.ok(res.json.steps[0].ok === true);
        const items = res.json.search_items || res.json.items || [];
        assert.ok(items.length >= 1);
        assert.equal(items[0].name || items[0].title, 'Mock Naruto');
      },
    },
    {
      id: 'T1-F04-02',
      feature: 'F4',
      domain: 'core',
      title: 'CSS/Legado HTML shorthand resolves relative img@data-url to absolute http:// URLs',
      async run(ctx) {
        const mocks = await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const pagesRes = await ctx.admin.failLoudlyRequest('POST', '/api/v1/comic/online/pages', {
          body: {
            source_id: mocks.webtoon.id,
            chapter_url: `${ctx.mockBaseUrl}/comic/webtoon/viewer?title_no=95&episode_no=1`,
          },
          expectedStatus: 200,
        });
        assert.equal(pagesRes.json.pages.length, 2);
        assert.equal(
          pagesRes.json.pages[0].image_url,
          `${ctx.mockBaseUrl}/comic/webtoon/cdn/ep1_1.jpg`,
        );
        assert.equal(
          pagesRes.json.pages[1].image_url,
          `${ctx.mockBaseUrl}/comic/webtoon/cdn/ep1_2.png`,
        );
      },
    },
    {
      id: 'T1-F04-03',
      feature: 'F4',
      domain: 'core',
      title: 'Regex ##pattern##rep, Mustache {{key}}/{{page}}, and || fallback combinator work accurately',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'comic');
        const ruleSrc = {
          sourceName: 'Rule Combo Source',
          sourceUrl: `${ctx.mockBaseUrl}/comic/rule-combo`,
          sourceType: 2,
          enabled: true,
          searchUrl: `${ctx.mockBaseUrl}/comic/mangadex/manga?title={{key}}&limit={{page}}`,
          ruleSearch: {
            bookList: '$.data[*]',
            name: '$.attributes.title.missing||$.attributes.title.en##^Mock\\s+##Live-',
            author: "$.relationships[?(@.type=='author')].attributes.name",
            bookUrl: `${ctx.mockBaseUrl}/comic/mangadex/manga/{{$.id}}`,
          },
        };
        const imp = await importSources(ctx.admin, 'comic', ruleSrc);
        const res = await ctx.admin.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
          body: { keyword: 'Naruto', source_ids: [imp.sources[0].id], page: 3 },
          expectedStatus: 200,
        });
        const items = res.json.results ?? res.json.items ?? [];
        assert.equal(items.length, 1);
        assert.equal(items[0].title ?? items[0].name, 'Live-Naruto');
      },
    },
    {
      id: 'T1-F04-04',
      feature: 'F4',
      domain: 'core',
      title: 'url,{method:"POST",headers,body} sends POST request with custom headers and Mustache body',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'comic');
        const edges = buildEdgeMockSources(ctx.mockBaseUrl);
        const imp = await importSources(ctx.admin, 'comic', edges.postEcho);
        await resetMockRequests(ctx.mockBaseUrl);

        const res = await ctx.admin.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
          body: { keyword: 'hello-post', source_ids: [imp.sources[0].id] },
          expectedStatus: 200,
        });
        const items = res.json.results ?? res.json.items ?? [];
        assert.equal(items.length, 1);
        assert.equal(items[0].title ?? items[0].name, 'Echo Result');
        assert.equal(items[0].author, 'abc');

        const logged = await getMockRequests(ctx.mockBaseUrl);
        const postReq = logged.find((r) => r.path === '/edge/post-echo');
        assert.ok(postReq, 'Expected /edge/post-echo request in mock log');
        assert.equal(postReq.method, 'POST');
        assert.equal(postReq.headers['x-custom'], 'abc');
        assert.equal(postReq.body, 'q=hello-post');
      },
    },
    {
      id: 'T1-F04-05',
      feature: 'F4',
      domain: 'core',
      title: 'QuickJS jsLib + <js> rule transforms results and reads source.getVariable()',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'comic');
        const jsSource = {
          sourceName: 'QuickJS Transform Source',
          sourceUrl: `${ctx.mockBaseUrl}/comic/qjs-lib`,
          sourceType: 2,
          enabled: true,
          jsLib: 'function tagPrefix(v, s){ return "[" + (v || "none") + "] " + s; }',
          searchUrl: `${ctx.mockBaseUrl}/comic/xkcd/info.0.json`,
          ruleSearch: {
            bookList: '$',
            name: '<js>const d = typeof result === "string" ? JSON.parse(result) : result; tagPrefix(source.getVariable(), d.title)</js>',
            bookUrl: `@js:${JSON.stringify(`${ctx.mockBaseUrl}/comic/xkcd/info.0.json`)}`,
          },
        };
        const imp = await importSources(ctx.admin, 'comic', jsSource);
        const srcId = imp.sources[0].id;
        await ctx.admin.failLoudlyRequest('PATCH', `/api/v1/comic/sources/${srcId}`, {
          body: { variable: 'QJS-VAR' },
          expectedStatus: 200,
        });
        const testRes = await ctx.admin.failLoudlyRequest(
          'POST',
          `/api/v1/comic/sources/${srcId}/test`,
          { body: { mode: 'search', keyword: 'xkcd' }, expectedStatus: 200 },
        );
        assert.equal(testRes.json.ok, true);
        const items = testRes.json.search_items || testRes.json.items || [];
        assert.equal(items[0].name || items[0].title, '[QJS-VAR] Mock XKCD Strip');
        assert.ok(testRes.json.diagnostics && Array.isArray(testRes.json.diagnostics.logs));
      },
    },

    // =======================================================================
    // F5: Comic Source Parsing & Browsing (comic)
    // =======================================================================
    {
      id: 'T1-F05-01',
      feature: 'F5',
      domain: 'comic',
      title: 'MangaDex mock search returns Mock Naruto with cover_url and author',
      async run(ctx) {
        const mocks = await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
          body: { keyword: 'Naruto', source_ids: [mocks.mangadex.id] },
          expectedStatus: 200,
        });
        const results = res.json.results ?? res.json.items;
        assert.ok(Array.isArray(results) && results.length === 1);
        assert.equal(results[0].title ?? results[0].name, 'Mock Naruto');
        assert.equal(results[0].author, 'Kishimoto Mock');
        assert.ok(results[0].cover_url?.includes('cover1.jpg'));
      },
    },
    {
      id: 'T1-F05-02',
      feature: 'F5',
      domain: 'comic',
      title: 'MangaDex mock detail returns title, intro, and chapters md-ch-1 & md-ch-2',
      async run(ctx) {
        const mocks = await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/detail', {
          body: {
            source_id: mocks.mangadex.id,
            comic_url: `${ctx.mockBaseUrl}/comic/mangadex/manga/md-comic-1`,
          },
          expectedStatus: 200,
        });
        assert.equal(res.json.detail.title ?? res.json.detail.name, 'Mock Naruto');
        assert.ok((res.json.detail.intro || '').includes('Mock MangaDex synopsis'));
        assert.ok(Array.isArray(res.json.chapters) && res.json.chapters.length === 2);
        assert.ok((res.json.chapters[0].url ?? res.json.chapters[0].chapter_url).includes('md-ch-1'));
        assert.ok((res.json.chapters[1].url ?? res.json.chapters[1].chapter_url).includes('md-ch-2'));
      },
    },
    {
      id: 'T1-F05-03',
      feature: 'F5',
      domain: 'comic',
      title: 'MangaDex mock pages returns 3 pages with image_url and image-proxy proxy_url',
      async run(ctx) {
        const mocks = await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/pages', {
          body: {
            source_id: mocks.mangadex.id,
            chapter_url: `${ctx.mockBaseUrl}/comic/mangadex/at-home/server/md-ch-1`,
          },
          expectedStatus: 200,
        });
        assert.equal(res.json.pages.length, 3);
        for (let i = 0; i < 3; i += 1) {
          const p = res.json.pages[i];
          assert.equal(p.index, i);
          assert.ok(p.image_url.startsWith(`${ctx.mockBaseUrl}/comic/mangadex/cdn/`));
          assert.ok(p.proxy_url.includes('/api/v1/comic/online/image-proxy'));
        }
      },
    },
    {
      id: 'T1-F05-04',
      feature: 'F5',
      domain: 'comic',
      title: 'WEBTOON mock search -> detail -> pages extracts #_imageList data-url and reading_direction=webtoon',
      async run(ctx) {
        const mocks = await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const searchRes = await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
          body: { keyword: 'Tower', source_ids: [mocks.webtoon.id] },
          expectedStatus: 200,
        });
        const items = searchRes.json.results ?? searchRes.json.items;
        assert.equal(items[0].title ?? items[0].name, 'Tower of Mock');

        const detailRes = await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/detail', {
          body: {
            source_id: mocks.webtoon.id,
            comic_url: items[0].comic_url ?? items[0].url,
          },
          expectedStatus: 200,
        });
        assert.ok(detailRes.json.chapters.length >= 2);
        const ep1Url = detailRes.json.chapters[0].url ?? detailRes.json.chapters[0].chapter_url;

        const pagesRes = await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/pages', {
          body: { source_id: mocks.webtoon.id, chapter_url: ep1Url },
          expectedStatus: 200,
        });
        assert.equal(pagesRes.json.pages.length, 2);
        assert.equal(pagesRes.json.reading_direction, 'webtoon');
        assert.ok(pagesRes.json.pages[0].image_url.includes('/comic/webtoon/cdn/ep1_1.jpg'));
      },
    },
    {
      id: 'T1-F05-05',
      feature: 'F5',
      domain: 'comic',
      title: 'XKCD mock explore + pages returns Mock XKCD Strip and PNG proxy_url',
      async run(ctx) {
        const mocks = await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const expRes = await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/explore', {
          body: {
            source_id: mocks.xkcd.id,
            explore_url: `${ctx.mockBaseUrl}/comic/xkcd/info.0.json`,
          },
          expectedStatus: 200,
        });
        const items = expRes.json.items ?? expRes.json.results;
        assert.ok(items.length >= 1);
        assert.equal(items[0].title ?? items[0].name, 'Mock XKCD Strip');

        const pagesRes = await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/pages', {
          body: {
            source_id: mocks.xkcd.id,
            chapter_url: `${ctx.mockBaseUrl}/comic/xkcd/info.0.json`,
          },
          expectedStatus: 200,
        });
        assert.equal(pagesRes.json.pages.length, 1);
        assert.ok(pagesRes.json.pages[0].proxy_url.includes('/api/v1/comic/online/image-proxy'));
      },
    },

    // =======================================================================
    // F6: Comic Image Proxy (comic)
    // =======================================================================
    {
      id: 'T1-F06-01',
      feature: 'F6',
      domain: 'comic',
      title: 'image-proxy serves MangaDex JPEG with valid magic bytes and image/jpeg Content-Type',
      async run(ctx) {
        const mocks = await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('GET', '/api/v1/comic/online/image-proxy', {
          query: {
            url: `${ctx.mockBaseUrl}/comic/mangadex/cdn/data-saver/hash123/s1.jpg`,
            source_id: mocks.mangadex.id,
          },
          expectedStatus: 200,
        });
        assert.equal(res.headers.get('content-type'), 'image/jpeg');
        assertJpeg(res.buffer, 'MangaDex image-proxy JPEG');
      },
    },
    {
      id: 'T1-F06-02',
      feature: 'F6',
      domain: 'comic',
      title: 'image-proxy injects WEBTOON imageHeader Referer: https://www.webtoons.com/',
      async run(ctx) {
        const mocks = await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        await resetMockRequests(ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('GET', '/api/v1/comic/online/image-proxy', {
          query: {
            url: `${ctx.mockBaseUrl}/comic/webtoon/cdn/ep1_1.jpg`,
            source_id: mocks.webtoon.id,
          },
          expectedStatus: 200,
        });
        assertJpeg(res.buffer, 'WEBTOON image-proxy JPEG');
        const reqs = await getMockRequests(ctx.mockBaseUrl);
        const cdnReq = reqs.find((r) => r.path === '/comic/webtoon/cdn/ep1_1.jpg');
        assert.ok(cdnReq, 'Expected upstream CDN request in mock log');
        assert.equal(cdnReq.headers.referer, 'https://www.webtoons.com/');
      },
    },
    {
      id: 'T1-F06-03',
      feature: 'F6',
      domain: 'comic',
      title: 'image-proxy serves XKCD PNG with image/png Content-Type and PNG magic bytes',
      async run(ctx) {
        const mocks = await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('GET', '/api/v1/comic/online/image-proxy', {
          query: {
            url: `${ctx.mockBaseUrl}/comic/xkcd/strip.png`,
            source_id: mocks.xkcd.id,
          },
          expectedStatus: 200,
        });
        assert.equal(res.headers.get('content-type'), 'image/png');
        assertPng(res.buffer, 'XKCD image-proxy PNG');
      },
    },
    {
      id: 'T1-F06-04',
      feature: 'F6',
      domain: 'comic',
      title: 'image-proxy sets X-Content-Type-Options: nosniff and restrictive CSP',
      async run(ctx) {
        const mocks = await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('GET', '/api/v1/comic/online/image-proxy', {
          query: {
            url: `${ctx.mockBaseUrl}/comic/xkcd/strip.png`,
            source_id: mocks.xkcd.id,
          },
          expectedStatus: 200,
        });
        assertSecurityHeaders(res.headers, { requireCsp: true, label: 'comic image-proxy' });
      },
    },
    {
      id: 'T1-F06-05',
      feature: 'F6',
      domain: 'comic',
      title: 'image-proxy sniffs magic bytes to serve image/jpeg when upstream sends application/octet-stream',
      async run(ctx) {
        const mocks = await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('GET', '/api/v1/comic/online/image-proxy', {
          query: {
            url: `${ctx.mockBaseUrl}/edge/jpeg-wrong-ct`,
            source_id: mocks.xkcd.id,
          },
          expectedStatus: 200,
        });
        assert.equal(res.headers.get('content-type'), 'image/jpeg');
        assertJpeg(res.buffer, 'magic-sniffed jpeg-wrong-ct');
      },
    },

    // =======================================================================
    // F7: Music Source Parsing, Stream & Lyrics (music)
    // =======================================================================
    {
      id: 'T1-F07-01',
      feature: 'F7',
      domain: 'music',
      title: 'GDStudio mock search returns Mock Mojito with normalized artist and album',
      async run(ctx) {
        const mocks = await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('POST', '/api/v1/music/online/search', {
          body: { keyword: 'Mojito', source_ids: [mocks.gdstudio.id] },
          expectedStatus: 200,
        });
        const results = res.json.results ?? res.json.items;
        assert.equal(results.length, 1);
        assert.equal(results[0].title, 'Mock Mojito');
        assert.ok(results[0].artist.includes('Jay Mock'));
        assert.equal(results[0].album, 'Mock Album');
      },
    },
    {
      id: 'T1-F07-02',
      feature: 'F7',
      domain: 'music',
      title: 'GDStudio mock track-info resolves stream, cover proxy URLs and synced LRC lyrics',
      async run(ctx) {
        const mocks = await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('POST', '/api/v1/music/online/track-info', {
          body: {
            source_id: mocks.gdstudio.id,
            track_id: 'gd-101',
            url_id: 'gd-101',
            cover_id: 'pic-101',
            lyric_id: 'gd-101',
            quality: '320',
          },
          expectedStatus: 200,
        });
        assert.ok(res.json.stream_url.includes('gd-101.mp3'));
        assert.ok(res.json.proxy_stream_url.includes('/api/v1/music/online/audio-proxy'));
        assert.ok(res.json.proxy_cover_url.includes('/api/v1/music/online/cover-proxy'));
        assert.equal(res.json.lyrics?.synced, true);
      },
    },
    {
      id: 'T1-F07-03',
      feature: 'F7',
      domain: 'music',
      title: 'GDStudio mock /online/stream returns stream_url and audio-proxy URL',
      async run(ctx) {
        const mocks = await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('POST', '/api/v1/music/online/stream', {
          body: {
            source_id: mocks.gdstudio.id,
            track_id: 'gd-101',
            url_id: 'gd-101',
            quality: '320',
          },
          expectedStatus: 200,
        });
        assert.ok(res.json.stream_url.includes('gd-101.mp3'));
        assert.ok(res.json.proxy_stream_url.includes('/api/v1/music/online/audio-proxy'));
      },
    },
    {
      id: 'T1-F07-04',
      feature: 'F7',
      domain: 'music',
      title: 'GDStudio mock /online/lyrics parses LRC timestamps into StructuredLyricsDto',
      async run(ctx) {
        const mocks = await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('POST', '/api/v1/music/online/lyrics', {
          body: { source_id: mocks.gdstudio.id, track_id: 'gd-101', lyric_id: 'gd-101' },
          expectedStatus: 200,
        });
        assert.equal(res.json.synced, true);
        assert.ok(Array.isArray(res.json.lines) && res.json.lines.length >= 2);
        assert.equal(res.json.lines[0].start_ms, 500);
        assert.equal(res.json.lines[0].text, 'First synced line');
      },
    },
    {
      id: 'T1-F07-05',
      feature: 'F7',
      domain: 'music',
      title: 'Audius mock explore + search + stream returns Fragments Mock and audio-proxy URL',
      async run(ctx) {
        const mocks = await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        const exp = await ctx.member.failLoudlyRequest('POST', '/api/v1/music/online/explore', {
          body: {
            source_id: mocks.audius.id,
            explore_url: `${ctx.mockBaseUrl}/music/audius/v1/tracks/trending`,
          },
          expectedStatus: 200,
        });
        const expItems = exp.json.results ?? exp.json.items;
        assert.equal(expItems[0].title, 'Fragments Mock');
        assert.equal(expItems[0].duration_secs ?? expItems[0].duration, 212);

        const search = await ctx.member.failLoudlyRequest('POST', '/api/v1/music/online/search', {
          body: { keyword: 'Fragments', source_ids: [mocks.audius.id] },
          expectedStatus: 200,
        });
        const track = (search.json.results ?? search.json.items)[0];
        const stream = await ctx.member.failLoudlyRequest('POST', '/api/v1/music/online/stream', {
          body: {
            source_id: mocks.audius.id,
            track_id: track.track_id ?? track.id,
            play_url: track.play_url,
          },
          expectedStatus: 200,
        });
        assert.ok(stream.json.proxy_stream_url.includes('/api/v1/music/online/audio-proxy'));
      },
    },

    // =======================================================================
    // F8: Music Cover & Streaming Audio Proxy (music)
    // =======================================================================
    {
      id: 'T1-F08-01',
      feature: 'F8',
      domain: 'music',
      title: 'cover-proxy serves GDStudio JPEG cover with nosniff and CSP headers',
      async run(ctx) {
        const mocks = await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('GET', '/api/v1/music/online/cover-proxy', {
          query: {
            url: `${ctx.mockBaseUrl}/music/gdstudio/cdn/cover-101.jpg`,
            source_id: mocks.gdstudio.id,
          },
          expectedStatus: 200,
        });
        assertJpeg(res.buffer, 'music cover-proxy JPEG');
        assertSecurityHeaders(res.headers, { requireCsp: true, label: 'music cover-proxy' });
      },
    },
    {
      id: 'T1-F08-02',
      feature: 'F8',
      domain: 'music',
      title: 'audio-proxy serves full MP3 on non-Range GET with audio/mpeg and ID3 magic',
      async run(ctx) {
        const mocks = await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('GET', '/api/v1/music/online/audio-proxy', {
          query: {
            url: `${ctx.mockBaseUrl}/music/gdstudio/cdn/gd-101.mp3?br=320`,
            source_id: mocks.gdstudio.id,
          },
          expectedStatus: 200,
        });
        assert.ok((res.headers.get('content-type') || '').includes('audio/mpeg'));
        assertMp3(res.buffer, 'audio-proxy full MP3');
        assert.ok(res.buffer.equals(MOCK_MP3_BYTES));
      },
    },
    {
      id: 'T1-F08-03',
      feature: 'F8',
      domain: 'music',
      title: 'audio-proxy forwards Range: bytes=0-4095 and returns 206 Partial Content',
      async run(ctx) {
        const mocks = await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('GET', '/api/v1/music/online/audio-proxy', {
          query: {
            url: `${ctx.mockBaseUrl}/music/gdstudio/cdn/gd-101.mp3?br=320`,
            source_id: mocks.gdstudio.id,
          },
          headers: { Range: 'bytes=0-4095' },
          expectedStatus: 206,
        });
        assert.equal(
          res.headers.get('content-range'),
          `bytes 0-4095/${MOCK_MP3_BYTES.length}`,
        );
        assert.equal(res.buffer.length, 4096);
        assert.ok(res.buffer.equals(MOCK_MP3_BYTES.subarray(0, 4096)));
      },
    },
    {
      id: 'T1-F08-04',
      feature: 'F8',
      domain: 'music',
      title: 'audio-proxy forwards mid-stream Range: bytes=4096-8191 accurately',
      async run(ctx) {
        const mocks = await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('GET', '/api/v1/music/online/audio-proxy', {
          query: {
            url: `${ctx.mockBaseUrl}/music/gdstudio/cdn/gd-101.mp3?br=320`,
            source_id: mocks.gdstudio.id,
          },
          headers: { Range: 'bytes=4096-8191' },
          expectedStatus: 206,
        });
        assert.equal(
          res.headers.get('content-range'),
          `bytes 4096-8191/${MOCK_MP3_BYTES.length}`,
        );
        assert.equal(res.buffer.length, 4096);
        assert.ok(res.buffer.equals(MOCK_MP3_BYTES.subarray(4096, 8192)));
      },
    },
    {
      id: 'T1-F08-05',
      feature: 'F8',
      domain: 'music',
      title: 'audio-proxy handles non-Range upstream (Audius 200 OK) when client sends Range',
      async run(ctx) {
        const mocks = await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('GET', '/api/v1/music/online/audio-proxy', {
          query: {
            url: `${ctx.mockBaseUrl}/music/audius/v1/tracks/aud-1/stream`,
            source_id: mocks.audius.id,
          },
          headers: { Range: 'bytes=0-4095' },
          expectedStatus: 200,
        });
        assertMp3(res.buffer, 'Audius 200 fallback MP3');
        assert.ok(res.buffer.equals(MOCK_MP3_BYTES));
      },
    },

    // =======================================================================
    // F9: Video Source Parsing (video)
    // =======================================================================
    {
      id: 'T1-F09-01',
      feature: 'F9',
      domain: 'video',
      title: 'AppleCMS v10 /online/categories returns 电影 (1) and 动漫 (2)',
      async run(ctx) {
        const mocks = await ensureVideoMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('POST', '/api/v1/video/online/categories', {
          body: { source_id: mocks.applecms.id },
          expectedStatus: 200,
        });
        const cats = res.json.categories;
        assert.ok(Array.isArray(cats) && cats.length >= 2);
        const names = cats.map((c) => c.name ?? c.type_name);
        assert.ok(names.includes('电影') && names.includes('动漫'));
      },
    },
    {
      id: 'T1-F09-02',
      feature: 'F9',
      domain: 'video',
      title: 'AppleCMS v10 /online/search and /online/browse return Mock Wandering Earth',
      async run(ctx) {
        const mocks = await ensureVideoMocks(ctx.admin, ctx.mockBaseUrl);
        const searchRes = await ctx.member.failLoudlyRequest('POST', '/api/v1/video/online/search', {
          body: { keyword: 'Earth', source_ids: [mocks.applecms.id] },
          expectedStatus: 200,
        });
        const sList = searchRes.json.results ?? searchRes.json.items;
        assert.equal(sList.length, 1);
        assert.equal(sList[0].title ?? sList[0].name, 'Mock Wandering Earth');
        assert.equal(sList[0].remarks, 'HD');

        const browseRes = await ctx.member.failLoudlyRequest('POST', '/api/v1/video/online/browse', {
          body: { source_id: mocks.applecms.id, category_id: '1', page: 1 },
          expectedStatus: 200,
        });
        const bList = browseRes.json.results ?? browseRes.json.items;
        assert.ok(bList.length >= 1);
        assert.equal(bList[0].title ?? bList[0].name, 'Mock Wandering Earth');
      },
    },
    {
      id: 'T1-F09-03',
      feature: 'F9',
      domain: 'video',
      title: 'AppleCMS v10 /online/detail parses $$$/#/$ groups, filters /share/, orders ikm3u8 first',
      async run(ctx) {
        const mocks = await ensureVideoMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('POST', '/api/v1/video/online/detail', {
          body: { source_id: mocks.applecms.id, video_id: '1001' },
          expectedStatus: 200,
        });
        const groups = res.json.play_groups;
        assert.ok(Array.isArray(groups) && groups.length >= 2);
        assert.ok(!groups.some((g) => g.name === 'sharehtml'), 'sharehtml non-media group should be filtered');
        assert.equal(groups[0].name, 'ikm3u8');
        assert.equal(groups[0].episodes.length, 2);
        assert.equal(groups[0].episodes[0].name, '第1集');
        assert.equal(groups[0].episodes[1].name, '第2集');
      },
    },
    {
      id: 'T1-F09-04',
      feature: 'F9',
      domain: 'video',
      title: 'AppleCMS v10 /online/play returns stream_type=hls and hls-proxy URL',
      async run(ctx) {
        const mocks = await ensureVideoMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('POST', '/api/v1/video/online/play', {
          body: {
            source_id: mocks.applecms.id,
            play_url: `${ctx.mockBaseUrl}/video/applecms/hls/master.m3u8`,
            group_name: 'ikm3u8',
          },
          expectedStatus: 200,
        });
        assert.equal(res.json.stream_type, 'hls');
        assert.ok(res.json.proxy_stream_url.includes('/api/v1/video/online/hls-proxy'));
      },
    },
    {
      id: 'T1-F09-05',
      feature: 'F9',
      domain: 'video',
      title: 'PeerTube rule mode search -> detail -> play returns stream_type=mp4 and stream-proxy URL',
      async run(ctx) {
        const mocks = await ensureVideoMocks(ctx.admin, ctx.mockBaseUrl);
        const searchRes = await ctx.member.failLoudlyRequest('POST', '/api/v1/video/online/search', {
          body: { keyword: 'Blender', source_ids: [mocks.peertube.id] },
          expectedStatus: 200,
        });
        const item = (searchRes.json.results ?? searchRes.json.items)[0];
        assert.equal(item.title ?? item.name, 'Mock Blender Open Movie');

        const detailRes = await ctx.member.failLoudlyRequest('POST', '/api/v1/video/online/detail', {
          body: {
            source_id: mocks.peertube.id,
            video_id: item.video_id ?? item.id,
            detail_url: item.detail_url,
          },
          expectedStatus: 200,
        });
        const ep = detailRes.json.play_groups[0].episodes[0];

        const playRes = await ctx.member.failLoudlyRequest('POST', '/api/v1/video/online/play', {
          body: { source_id: mocks.peertube.id, play_url: ep.play_url },
          expectedStatus: 200,
        });
        assert.equal(playRes.json.stream_type, 'mp4');
        assert.ok(playRes.json.proxy_stream_url.includes('/api/v1/video/online/stream-proxy'));
      },
    },

    // =======================================================================
    // F10: Video Poster, Stream & HLS Rewriting Proxies (video)
    // =======================================================================
    {
      id: 'T1-F10-01',
      feature: 'F10',
      domain: 'video',
      title: 'poster-proxy serves AppleCMS JPEG poster with nosniff and CSP headers',
      async run(ctx) {
        const mocks = await ensureVideoMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('GET', '/api/v1/video/online/poster-proxy', {
          query: {
            url: `${ctx.mockBaseUrl}/video/applecms/poster/1001.jpg`,
            source_id: mocks.applecms.id,
          },
          expectedStatus: 200,
        });
        assertJpeg(res.buffer, 'video poster-proxy JPEG');
        assertSecurityHeaders(res.headers, { requireCsp: true, label: 'video poster-proxy' });
      },
    },
    {
      id: 'T1-F10-02',
      feature: 'F10',
      domain: 'video',
      title: 'stream-proxy serves PeerTube MP4 full (200) and Range bytes=0-1023 (206)',
      async run(ctx) {
        const mocks = await ensureVideoMocks(ctx.admin, ctx.mockBaseUrl);
        const fullRes = await ctx.member.failLoudlyRequest('GET', '/api/v1/video/online/stream-proxy', {
          query: {
            url: `${ctx.mockBaseUrl}/video/peertube/media/pt-vid-1.mp4`,
            source_id: mocks.peertube.id,
          },
          expectedStatus: 200,
        });
        assertMp4(fullRes.buffer, 'stream-proxy full MP4');
        assert.ok(fullRes.buffer.equals(MOCK_MP4_BYTES));

        const rangeRes = await ctx.member.failLoudlyRequest('GET', '/api/v1/video/online/stream-proxy', {
          query: {
            url: `${ctx.mockBaseUrl}/video/peertube/media/pt-vid-1.mp4`,
            source_id: mocks.peertube.id,
          },
          headers: { Range: 'bytes=0-1023' },
          expectedStatus: 206,
        });
        assert.equal(rangeRes.headers.get('content-range'), `bytes 0-1023/${MOCK_MP4_BYTES.length}`);
        assert.equal(rangeRes.buffer.length, 1024);
        assertMp4(rangeRes.buffer, 'stream-proxy partial MP4');
      },
    },
    {
      id: 'T1-F10-03',
      feature: 'F10',
      domain: 'video',
      title: 'hls-proxy rewrites master.m3u8 sub-playlist URI to /api/v1/video/online/hls-proxy',
      async run(ctx) {
        const mocks = await ensureVideoMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('GET', '/api/v1/video/online/hls-proxy', {
          query: {
            url: `${ctx.mockBaseUrl}/video/applecms/hls/master.m3u8`,
            source_id: mocks.applecms.id,
          },
          expectedStatus: 200,
        });
        const parsed = assertM3u8Rewritten(res.text, {
          expectedSourceId: mocks.applecms.id,
          minSegments: 1,
          label: 'master.m3u8',
        });
        const decodedTarget = new URL(parsed.segments[0], ctx.baseUrl).searchParams.get('url');
        assert.ok(decodedTarget.endsWith('/video/applecms/hls/5000kb/hls/index.m3u8'));
      },
    },
    {
      id: 'T1-F10-04',
      feature: 'F10',
      domain: 'video',
      title: 'hls-proxy rewrites #EXT-X-KEY, #EXT-X-MAP, relative and absolute .ts segments',
      async run(ctx) {
        const mocks = await ensureVideoMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('GET', '/api/v1/video/online/hls-proxy', {
          query: {
            url: `${ctx.mockBaseUrl}/video/applecms/hls/5000kb/hls/index.m3u8`,
            source_id: mocks.applecms.id,
          },
          expectedStatus: 200,
        });
        const parsed = assertM3u8Rewritten(res.text, {
          expectedSourceId: mocks.applecms.id,
          requireKey: true,
          requireMap: true,
          minSegments: 2,
          label: 'media index.m3u8',
        });
        assert.ok(parsed.keys[0].includes('enc.key'));
        assert.ok(parsed.maps[0].includes('init.mp4'));
        assert.ok(parsed.segments[0].includes('seg0.ts'));
        assert.ok(parsed.segments[1].includes('seg1.ts'));
      },
    },
    {
      id: 'T1-F10-05',
      feature: 'F10',
      domain: 'video',
      title: 'hls-proxy fetches Range TS segment & AES key while injecting source mediaHeader',
      async run(ctx) {
        const mocks = await ensureVideoMocks(ctx.admin, ctx.mockBaseUrl);
        await resetMockRequests(ctx.mockBaseUrl);
        const playlistRes = await ctx.member.failLoudlyRequest(
          'GET',
          '/api/v1/video/online/hls-proxy',
          {
            query: {
              url: `${ctx.mockBaseUrl}/video/applecms/hls/5000kb/hls/index.m3u8?require_header=X-Video-Token:secret123`,
              source_id: mocks.applecmsToken.id,
            },
            expectedStatus: 200,
          },
        );
        const parsed = assertM3u8Rewritten(playlistRes.text, {
          expectedSourceId: mocks.applecmsToken.id,
          requireKey: true,
          minSegments: 2,
        });

        const keyRes = await ctx.member.failLoudlyRequest('GET', parsed.keys[0], {
          expectedStatus: 200,
        });
        assert.ok(keyRes.buffer.equals(MOCK_AES_KEY_BYTES));

        const segRes = await ctx.member.failLoudlyRequest('GET', parsed.segments[0], {
          headers: { Range: 'bytes=0-383' },
          expectedStatus: 206,
        });
        assert.equal(segRes.buffer.length, 384);
        assert.notEqual(segRes.buffer[0], 0x47, 'seg0.ts should be AES-128 encrypted');
        const decSeg = decryptAes128Cbc(segRes.buffer, keyRes.buffer, 0, { partialPrefixBytes: 376 });
        assertMpegTs(decSeg, 'rewritten TS segment');

        const reqs = await getMockRequests(ctx.mockBaseUrl);
        const segReq = reqs.find((r) => r.path.endsWith('/seg0.ts'));
        assert.ok(segReq, 'Expected seg0.ts request in mock log');
        assert.equal(segReq.headers['x-video-token'], 'secret123');
      },
    },

    // =======================================================================
    // F11: Restart Persistence (core)
    // =======================================================================
    {
      id: 'T1-F11-04',
      feature: 'F11',
      domain: 'core',
      title: 'Source enabled/weight/group_name mutations persist across xmedia server restart on SQLite',
      async run(ctx) {
        if (ctx.externalBaseUrl) {
          throw new Error('T1-F11-04 requires managed SUT to test server restart');
        }
        const mgr = new ctx.XMediaServerManager({
          xmediaBin: ctx.xmediaBin,
          webDir: ctx.webDir,
          keepTmp: ctx.keepTmp,
        });
        try {
          const url1 = await mgr.start({ mode: 'mock' });
          const { admin } = await bootstrapClients(url1, ctx.ApiClient);
          const mocks = await ensureComicMocks(admin, ctx.mockBaseUrl);
          const targetId = mocks.xkcd.id;
          await admin.failLoudlyRequest('PATCH', `/api/v1/comic/sources/${targetId}`, {
            body: { enabled: false, weight: 99, group_name: 'CustomPersist' },
            expectedStatus: 200,
          });

          const url2 = await mgr.restart({ mode: 'mock' });
          const admin2 = new ctx.ApiClient(url2, { authMode: 'bearer' });
          await admin2.login({ username: 'admin', password: 'AdminPass123!' });
          const list = (
            await admin2.failLoudlyRequest('GET', '/api/v1/comic/sources', { expectedStatus: 200 })
          ).json;
          const found = list.find((s) => s.id === targetId);
          assert.ok(found, 'Source missing after restart');
          assert.equal(found.enabled, false);
          assert.equal(found.weight, 99);
          assert.equal(found.group_name, 'CustomPersist');
        } finally {
          await mgr.cleanup();
        }
      },
    },
    {
      id: 'T1-F11-05',
      feature: 'F11',
      domain: 'core',
      title: 'Local library data and online source tables coexist cleanly across DB restart',
      async run(ctx) {
        if (ctx.externalBaseUrl) {
          throw new Error('T1-F11-05 requires managed SUT to test DB coexistence across restart');
        }
        const mgr = new ctx.XMediaServerManager({
          xmediaBin: ctx.xmediaBin,
          webDir: ctx.webDir,
          keepTmp: ctx.keepTmp,
        });
        try {
          const url1 = await mgr.start({ mode: 'mock' });
          const { admin } = await bootstrapClients(url1, ctx.ApiClient);
          const beforeLibs = (
            await admin.failLoudlyRequest('GET', '/api/v1/libraries', { expectedStatus: 200 })
          ).json;
          assert.ok(beforeLibs.length >= 4);
          await ensureComicMocks(admin, ctx.mockBaseUrl);
          await ensureMusicMocks(admin, ctx.mockBaseUrl);
          await ensureVideoMocks(admin, ctx.mockBaseUrl);

          const url2 = await mgr.restart({ mode: 'mock' });
          const admin2 = new ctx.ApiClient(url2, { authMode: 'bearer' });
          await admin2.login({ username: 'admin', password: 'AdminPass123!' });
          const afterLibs = (
            await admin2.failLoudlyRequest('GET', '/api/v1/libraries', { expectedStatus: 200 })
          ).json;
          assert.equal(afterLibs.length, beforeLibs.length);
          for (const domain of ['comic', 'music', 'video']) {
            const srcs = (
              await admin2.failLoudlyRequest('GET', `/api/v1/${domain}/sources`, {
                expectedStatus: 200,
              })
            ).json;
            assert.ok(srcs.length >= 2);
          }
        } finally {
          await mgr.cleanup();
        }
      },
    },

    // =======================================================================
    // F12: Web UI Settings Source Management (ui)
    // =======================================================================
    {
      id: 'T1-F12-01',
      feature: 'F12',
      domain: 'ui',
      title: 'Browser: Admin opens Settings Data Sources comic tab & imports JSON source',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'comic');
        const comics = buildMockComicSources(ctx.mockBaseUrl);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.admin,
          '/settings?tab=domain-datasources&domain=comic',
        );
        try {
          await cdp.waitForSelector('[data-settings-tab="sources"]', { visible: true });
          await clickButtonMatching(cdp, [/导入|import/i]);
          await cdp.waitForSelector('textarea', { visible: true });
          await cdp.type('textarea', JSON.stringify(comics.xkcd));
          await clickButtonMatching(cdp, [/确认导入|导入|submit|import/i], {
            rootSelector: '[role="dialog"], form, .space-y-4',
          });
          await cdp.waitForFn(
            `document.body.innerText.includes('Mock XKCD')`,
            { timeoutMs: 10000, label: 'Mock XKCD source card rendered' },
          );
          cdp.assertNoErrors('T1-F12-01');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T1-F12-02',
      feature: 'F12',
      domain: 'ui',
      title: 'Browser: Admin switches to music & video source tabs with filters and toggles',
      async run(ctx) {
        await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        await ensureVideoMocks(ctx.admin, ctx.mockBaseUrl);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.admin,
          '/settings?tab=domain-datasources&domain=music',
        );
        try {
          await cdp.waitForFn(
            `document.body.innerText.includes('Mock GDStudio')`,
            { timeoutMs: 10000, label: 'music source card rendered' },
          );
          await cdp.waitForSelector('main input[type="text"], main input[type="search"]', { visible: true });
          await cdp.goto(`${ctx.baseUrl}/settings?tab=domain-datasources&domain=video`);
          await cdp.waitForFn(
            `document.body.innerText.includes('Mock AppleCMS')`,
            { timeoutMs: 10000, label: 'video source card rendered' },
          );
          cdp.assertNoErrors('T1-F12-02');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T1-F12-03',
      feature: 'F12',
      domain: 'ui',
      title: 'Browser: Admin edits source group_name in Edit Sheet and saves',
      async run(ctx) {
        await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.admin,
          '/settings?tab=domain-datasources&domain=comic',
        );
        try {
          await cdp.waitForFn(`document.body.innerText.includes('Mock MangaDex')`, {
            timeoutMs: 10000,
          });
          await clickButtonMatching(cdp, [/编辑|edit/i]);
          await cdp.waitForSelector('[role="dialog"]', { visible: true });
          await cdp.waitForSelector('[role="dialog"] input[name="group_name"]', {
            visible: true,
            timeoutMs: 8000,
          });
          await cdp.evalJs(`(() => {
            const dlg = document.querySelector('[role="dialog"]');
            const inputs = [...dlg.querySelectorAll('input')];
            const grpInput = inputs.find((i) => /group|分组/i.test(i.placeholder || i.name || i.getAttribute('aria-label') || '') || i.value === 'Mock-Comic');
            if (!grpInput) throw new Error('Group input not found in Edit Sheet');
            const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
            setter.call(grpInput, 'EditedUiGroup');
            grpInput.dispatchEvent(new Event('input', { bubbles: true }));
            grpInput.dispatchEvent(new Event('change', { bubbles: true }));
          })()`);
          await clickButtonMatching(cdp, [/保存|save/i], { rootSelector: '[role="dialog"]' });
          await cdp.waitForFn(`document.body.innerText.includes('EditedUiGroup')`, {
            timeoutMs: 10000,
            label: 'EditedUiGroup chip rendered',
          });
          cdp.assertNoErrors('T1-F12-03');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T1-F12-04',
      feature: 'F12',
      domain: 'ui',
      title: 'Browser: Admin opens Debug Sheet and runs full test on mock comic source',
      async run(ctx) {
        await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.admin,
          '/settings?tab=domain-datasources&domain=comic',
        );
        try {
          await cdp.waitForFn(`document.body.innerText.includes('Mock MangaDex')`, {
            timeoutMs: 10000,
          });
          await clickButtonMatching(cdp, [/更多|more|菜单|menu|⋮/i]);
          await clickButtonMatching(cdp, [/调试|测试|debug|test/i]);
          await cdp.waitForSelector('[role="dialog"]', { visible: true });
          await clickButtonMatching(cdp, [/开始测试|运行|run|test/i], {
            rootSelector: '[role="dialog"]',
          });
          await cdp.waitForFn(
            `document.querySelector('[role="dialog"]')?.innerText.includes('Mock Naruto')`,
            { timeoutMs: 15000, label: 'debug preview shows Mock Naruto' },
          );
          cdp.assertNoErrors('T1-F12-04');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T1-F12-05',
      feature: 'F12',
      domain: 'ui',
      title: 'Browser: Admin uses batch selection toolbar to disable and enable sources',
      async run(ctx) {
        await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.admin,
          '/settings?tab=domain-datasources&domain=comic',
        );
        try {
          await cdp.waitForFn(`document.body.innerText.includes('Mock MangaDex')`, {
            timeoutMs: 10000,
          });
          await clickButtonMatching(cdp, [/批量|batch|多选|select/i]);
          await clickButtonMatching(cdp, [/全选|select all/i]);
          await clickButtonMatching(cdp, [/禁用|disable/i]);
          await cdp.waitForFn(
            `document.body.innerText.includes('已禁用') || document.body.innerText.match(/disabled/i)`,
            { timeoutMs: 8000, label: 'batch disable reflected' },
          );
          await clickButtonMatching(cdp, [/启用|enable/i]);
          cdp.assertNoErrors('T1-F12-05');
        } finally {
          await cdp.close();
        }
      },
    },

    // =======================================================================
    // F13: Web UI Domain Switchers & GlobalSearchModal (ui)
    // =======================================================================
    {
      id: 'T1-F13-01',
      feature: 'F13',
      domain: 'ui',
      title: 'Browser: /comic/series local|online switcher updates scope=online and searches mock comics',
      async run(ctx) {
        await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.member,
          '/comic/series',
        );
        try {
          await clickButtonMatching(cdp, [/在线|online/i]);
          await cdp.waitForFn(`location.search.includes('scope=online')`, { timeoutMs: 8000 });
          await cdp.type('main input[type="text"], main input[type="search"]', 'Naruto');
          await cdp.pressKey('Enter');
          await cdp.waitForFn(`document.body.innerText.includes('Mock Naruto')`, {
            timeoutMs: 10000,
            label: 'online comic search results rendered',
          });
          cdp.assertNoErrors('T1-F13-01');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T1-F13-02',
      feature: 'F13',
      domain: 'ui',
      title: 'Browser: /music/songs local|online switcher searches and renders mock online tracks',
      async run(ctx) {
        await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.member,
          '/music/songs',
        );
        try {
          await clickButtonMatching(cdp, [/在线|online/i]);
          await cdp.waitForSelector('main input[type="text"], main input[type="search"]', { visible: true });
          await cdp.type('main input[type="text"], main input[type="search"]', 'Mojito');
          await cdp.pressKey('Enter');
          await cdp.waitForFn(`document.body.innerText.includes('Mock Mojito')`, {
            timeoutMs: 10000,
            label: 'online music track rendered',
          });
          cdp.assertNoErrors('T1-F13-02');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T1-F13-03',
      feature: 'F13',
      domain: 'ui',
      title: 'Browser: /video/catalog local|online switcher renders AppleCMS categories and cards',
      async run(ctx) {
        await ensureVideoMocks(ctx.admin, ctx.mockBaseUrl);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.member,
          '/video/catalog',
        );
        try {
          await clickButtonMatching(cdp, [/在线|online/i]);
          await cdp.waitForFn(
            `document.body.innerText.includes('电影') && document.body.innerText.includes('动漫')`,
            { timeoutMs: 10000, label: 'AppleCMS categories rendered' },
          );
          await cdp.waitForFn(`document.body.innerText.includes('Mock Wandering Earth')`, {
            timeoutMs: 10000,
            label: 'AppleCMS browse card rendered',
          });
          cdp.assertNoErrors('T1-F13-03');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T1-F13-04',
      feature: 'F13',
      domain: 'ui',
      title: 'Browser: GlobalSearchModal (Ctrl+K) shows online quick actions for all 4 domains',
      async run(ctx) {
        const cdp = await openAuthenticatedPage(ctx.CdpClient, ctx.baseUrl, ctx.member, '/comic/series');
        try {
          await cdp.pressKey('k', { modifiers: 2 }); // Ctrl+K
          await cdp.waitForSelector('[role="dialog"] input', { visible: true });
          await cdp.type('[role="dialog"] input', 'Mock');
          await cdp.waitForFn(
            `(() => {
              const txt = document.querySelector('[role="dialog"]')?.innerText || '';
              return /book|书|图书/i.test(txt) && /comic|漫画/i.test(txt) && /music|音乐/i.test(txt) && /video|影视|视频/i.test(txt);
            })()`,
            { timeoutMs: 8000, label: 'all 4 domain online quick actions shown' },
          );
          cdp.assertNoErrors('T1-F13-04');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T1-F13-05',
      feature: 'F13',
      domain: 'ui',
      title: 'Browser: Clicking Comic online quick action in GlobalSearchModal routes to /comic/series?q=Mock&scope=online',
      async run(ctx) {
        await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const cdp = await openAuthenticatedPage(ctx.CdpClient, ctx.baseUrl, ctx.member, '/music/songs');
        try {
          await cdp.pressKey('k', { modifiers: 2 });
          await cdp.waitForSelector('[role="dialog"] input', { visible: true });
          await cdp.type('[role="dialog"] input', 'Naruto');
          await clickButtonMatching(cdp, [/在线漫画|online comic/i], {
            rootSelector: '[role="dialog"]',
          });
          await cdp.waitForFn(
            `location.pathname.includes('/comic/library') && location.search.includes('scope=online') && document.body.innerText.includes('Mock Naruto')`,
            { timeoutMs: 12000, label: 'navigated and auto-searched online comic' },
          );
          cdp.assertNoErrors('T1-F13-05');
        } finally {
          await cdp.close();
        }
      },
    },

    // =======================================================================
    // F14: Web UI Comic Online Detail & Reader (ui)
    // =======================================================================
    {
      id: 'T1-F14-01',
      feature: 'F14',
      domain: 'ui',
      title: 'Browser: Online comic search -> click card -> online detail shows proxied cover & chapters',
      async run(ctx) {
        await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.member,
          '/comic/series?scope=online&q=Naruto',
        );
        try {
          await cdp.waitForFn(`document.body.innerText.includes('Mock Naruto')`, { timeoutMs: 10000 });
          await clickButtonMatching(cdp, [/Mock Naruto/]);
          await cdp.waitForFn(
            `document.body.innerText.includes('Arrival') && document.body.innerText.includes('Departure')`,
            { timeoutMs: 10000, label: 'comic detail chapters rendered' },
          );
          const coverLoaded = await cdp.waitForFn(
            `[...document.querySelectorAll('img')].some((img) => img.src.includes('image-proxy') && img.complete && img.naturalWidth > 0)`,
            { timeoutMs: 10000, label: 'proxied cover image loaded' },
          );
          assert.equal(coverLoaded, true);
          cdp.assertNoErrors('T1-F14-01');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T1-F14-02',
      feature: 'F14',
      domain: 'ui',
      title: 'Browser: Online comic reader renders all chapter pages via image-proxy with naturalWidth > 0',
      async run(ctx) {
        await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.member,
          '/comic/series?scope=online&q=Naruto',
        );
        try {
          await cdp.waitForFn(`document.body.innerText.includes('Mock Naruto')`, { timeoutMs: 10000 });
          await clickButtonMatching(cdp, [/Mock Naruto/]);
          await clickButtonMatching(cdp, [/Arrival|Ch\.1/]);
          await cdp.waitForFn(
            `(() => {
              if (!location.pathname.includes('/comic/play/online')) return false;
              const imgs = [...document.querySelectorAll('img[alt^="Page "][src*="image-proxy"]')];
              return imgs.length >= 1 && imgs.every((i) => i.complete && i.naturalWidth > 0);
            })()`,
            { timeoutMs: 12000, label: 'comic reader proxied pages decoded' },
          );
          cdp.assertNoErrors('T1-F14-02');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T1-F14-03',
      feature: 'F14',
      domain: 'ui',
      title: 'Browser: Switching reading modes in online comic reader updates layout without broken images',
      async run(ctx) {
        await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.member,
          '/comic/series?scope=online&q=Naruto',
        );
        try {
          await cdp.waitForFn(`document.body.innerText.includes('Mock Naruto')`, { timeoutMs: 10000 });
          await clickButtonMatching(cdp, [/Mock Naruto/]);
          await clickButtonMatching(cdp, [/Arrival|Ch\.1/]);
          await cdp.waitForFn(
            `location.pathname.includes('/comic/play/online') && Boolean(document.querySelector('img[alt^="Page "][src*="image-proxy"]'))`,
            { timeoutMs: 12000, label: 'comic reader mounted' },
          );
          await clickButtonMatching(cdp, [/阅读设置|settings/i]);
          for (const pat of [/条漫|webtoon/i, /双页|double/i, /单页|single/i]) {
            await clickButtonMatching(cdp, [pat], { rootSelector: '[role="dialog"]' });
          }
          const ok = await cdp.evalJs(
            `[...document.querySelectorAll('img[alt^="Page "][src*="image-proxy"]')].every((i) => i.complete && i.naturalWidth > 0)`,
          );
          assert.equal(ok, true);
          cdp.assertNoErrors('T1-F14-03');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T1-F14-04',
      feature: 'F14',
      domain: 'ui',
      title: 'Browser: Adjusting zoom and clicking Next Chapter loads chapter 2 via image-proxy',
      async run(ctx) {
        await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.member,
          '/comic/series?scope=online&q=Naruto',
        );
        try {
          await cdp.waitForFn(`document.body.innerText.includes('Mock Naruto')`, { timeoutMs: 10000 });
          await clickButtonMatching(cdp, [/Mock Naruto/]);
          await clickButtonMatching(cdp, [/Arrival|Ch\.1/]);
          await cdp.waitForFn(
            `location.pathname.includes('/comic/play/online') && Boolean(document.querySelector('img[alt^="Page "][src*="image-proxy"]'))`,
            { timeoutMs: 12000, label: 'comic reader chapter 1 mounted' },
          );
          await clickButtonMatching(cdp, [/下一话|下一章|next/i]);
          await cdp.waitForFn(
            `document.body.innerText.includes('Departure') || [...document.querySelectorAll('img[alt^="Page "][src*="image-proxy"]')].some((i) => i.src.includes('hash456') && i.naturalWidth > 0)`,
            { timeoutMs: 10000, label: 'chapter 2 loaded' },
          );
          cdp.assertNoErrors('T1-F14-04');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T1-F14-05',
      feature: 'F14',
      domain: 'ui',
      title: 'Browser: WEBTOON mock chapter renders data-url images in webtoon mode with Referer proxy',
      async run(ctx) {
        await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.member,
          '/comic/series?scope=online&q=Tower',
        );
        try {
          await cdp.waitForFn(`document.body.innerText.includes('Tower of Mock')`, { timeoutMs: 10000 });
          await clickButtonMatching(cdp, [/Tower of Mock/]);
          await clickButtonMatching(cdp, [/Ep\.1 Prologue/]);
          await cdp.waitForFn(
            `(() => {
              if (!location.pathname.includes('/comic/play/online')) return false;
              const imgs = [...document.querySelectorAll('img[alt^="Page "][src*="image-proxy"]')];
              return imgs.length >= 2 && imgs.every((i) => i.complete && i.naturalWidth > 0);
            })()`,
            { timeoutMs: 12000, label: 'WEBTOON 2 pages loaded via image-proxy' },
          );
          cdp.assertNoErrors('T1-F14-05');
        } finally {
          await cdp.close();
        }
      },
    },

    // =======================================================================
    // F15: Web UI Music Online Player, Lyrics & Queue (ui)
    // =======================================================================
    {
      id: 'T1-F15-01',
      feature: 'F15',
      domain: 'ui',
      title: 'Browser: Playing online GDStudio track mounts PlayerBar with audio-proxy src and readyState>=2',
      async run(ctx) {
        await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.member,
          '/music/songs?scope=online&q=Mojito',
        );
        try {
          await cdp.waitForFn(`document.body.innerText.includes('Mock Mojito')`, { timeoutMs: 10000 });
          await clickButtonMatching(cdp, [/Mock Mojito|播放|play/i]);
          await cdp.waitForFn(
            `(() => {
              const audio = document.querySelector('audio');
              return Boolean(audio && audio.src.includes('audio-proxy') && audio.readyState >= 2 && !audio.error);
            })()`,
            { timeoutMs: 12000, label: 'PlayerBar audio-proxy readyState>=2' },
          );
          cdp.assertNoErrors('T1-F15-01');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T1-F15-02',
      feature: 'F15',
      domain: 'ui',
      title: 'Browser: PlayerBar artwork loads via cover-proxy with naturalWidth > 0',
      async run(ctx) {
        await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.member,
          '/music/songs?scope=online&q=Mojito',
        );
        try {
          await cdp.waitForFn(`document.body.innerText.includes('Mock Mojito')`, { timeoutMs: 10000 });
          await clickButtonMatching(cdp, [/Mock Mojito|播放|play/i]);
          await cdp.waitForFn(
            `[...document.querySelectorAll('img')].some((img) => img.src.includes('cover-proxy') && img.complete && img.naturalWidth > 0)`,
            { timeoutMs: 12000, label: 'PlayerBar cover-proxy loaded' },
          );
          cdp.assertNoErrors('T1-F15-02');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T1-F15-03',
      feature: 'F15',
      domain: 'ui',
      title: 'Browser: LyricsOverlay displays synced LRC lines and clicking a line seeks audio',
      async run(ctx) {
        await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.member,
          '/music/songs?scope=online&q=Mojito',
        );
        try {
          await cdp.waitForFn(`document.body.innerText.includes('Mock Mojito')`, { timeoutMs: 10000 });
          await clickButtonMatching(cdp, [/Mock Mojito|播放|play/i]);
          await cdp.waitForFn(`Boolean(document.querySelector('audio')?.src.includes('audio-proxy'))`, {
            timeoutMs: 10000,
          });
          await clickButtonMatching(cdp, [/歌词|lyrics/i]);
          await cdp.waitForFn(
            `document.body.innerText.includes('First synced line') && document.body.innerText.includes('Second synced line')`,
            { timeoutMs: 10000, label: 'LyricsOverlay rendered synced lines' },
          );
          await clickButtonMatching(cdp, [/Second synced line/]);
          await cdp.waitForFn(`(document.querySelector('audio')?.currentTime || 0) >= 1.5`, {
            timeoutMs: 8000,
            label: 'clicking lyric line seeks audio',
          });
          cdp.assertNoErrors('T1-F15-03');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T1-F15-04',
      feature: 'F15',
      domain: 'ui',
      title: 'Browser: QueueDrawer enqueues second online track and switches playback',
      async run(ctx) {
        await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.member,
          '/music/songs?scope=online&q=Mojito',
        );
        try {
          await cdp.waitForFn(`document.body.innerText.includes('Mock Mojito')`, { timeoutMs: 10000 });
          await clickButtonMatching(cdp, [/Mock Mojito|播放|play/i]);
          await cdp.type('main input[type="text"], main input[type="search"]', 'Fragments');
          await cdp.pressKey('Enter');
          await cdp.waitForFn(`document.body.innerText.includes('Fragments Mock')`, { timeoutMs: 10000 });
          await clickButtonMatching(cdp, [/加入队列|加入|enqueue/i]);
          await clickButtonMatching(cdp, [/播放队列|队列|queue/i]);
          await cdp.waitForSelector('[role="dialog"]', { visible: true, timeoutMs: 8000 });
          await cdp.waitForFn(
            `Boolean(document.querySelector('[role="dialog"]')?.innerText.includes('Fragments Mock'))`,
            { timeoutMs: 8000, label: 'Fragments Mock rendered inside QueueDrawer' },
          );
          await clickButtonMatching(cdp, [/Fragments Mock/], { rootSelector: '[role="dialog"]' });
          await cdp.waitForFn(
            `Boolean(document.querySelector('footer')?.innerText.includes('Fragments Mock')) && Boolean(document.querySelector('audio')?.src.includes('audio-proxy'))`,
            { timeoutMs: 10000, label: 'QueueDrawer track click switched PlayerBar playback to Fragments Mock' },
          );
          cdp.assertNoErrors('T1-F15-04');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T1-F15-05',
      feature: 'F15',
      domain: 'ui',
      title: 'Browser: PlayerBar persists across Workspace route navigation (/music/songs -> /comic/series)',
      async run(ctx) {
        await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.member,
          '/music/songs?scope=online&q=Mojito',
        );
        try {
          await cdp.waitForFn(`document.body.innerText.includes('Mock Mojito')`, { timeoutMs: 10000 });
          await clickButtonMatching(cdp, [/Mock Mojito|播放|play/i]);
          await cdp.waitForFn(`Boolean(document.querySelector('audio')?.src.includes('audio-proxy'))`, {
            timeoutMs: 10000,
          });
          await clickButtonMatching(cdp, [/漫画|comic/i]);
          await cdp.waitForFn(
            `location.pathname.includes('/comic') && Boolean(document.querySelector('audio')?.src.includes('audio-proxy'))`,
            { timeoutMs: 10000, label: 'PlayerBar persisted across routes' },
          );
          cdp.assertNoErrors('T1-F15-05');
        } finally {
          await cdp.close();
        }
      },
    },

    // =======================================================================
    // F16: Web UI Video Online Catalog & Player (ui)
    // =======================================================================
    {
      id: 'T1-F16-01',
      feature: 'F16',
      domain: 'ui',
      title: 'Browser: Online video detail renders poster-proxy, play-line tabs, and episodes',
      async run(ctx) {
        await ensureVideoMocks(ctx.admin, ctx.mockBaseUrl);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.member,
          '/video/catalog?scope=online&q=Earth',
        );
        try {
          await cdp.waitForFn(`document.body.innerText.includes('Mock Wandering Earth')`, {
            timeoutMs: 10000,
          });
          await clickButtonMatching(cdp, [/Mock Wandering Earth/]);
          await cdp.waitForFn(
            `document.body.innerText.includes('ikm3u8') && document.body.innerText.includes('第1集') && document.body.innerText.includes('第2集')`,
            { timeoutMs: 10000, label: 'video detail lines and episodes rendered' },
          );
          const posterOk = await cdp.waitForFn(
            `[...document.querySelectorAll('img')].some((i) => i.src.includes('poster-proxy') && i.complete && i.naturalWidth > 0)`,
            { timeoutMs: 10000, label: 'poster-proxy loaded' },
          );
          assert.equal(posterOk, true);
          cdp.assertNoErrors('T1-F16-01');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T1-F16-02',
      feature: 'F16',
      domain: 'ui',
      title: 'Browser: Playing 第1集 uses hls.js + hls-proxy for master/sub m3u8 and TS segments',
      async run(ctx) {
        await ensureVideoMocks(ctx.admin, ctx.mockBaseUrl);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.member,
          '/video/catalog?scope=online&q=Earth',
        );
        try {
          await cdp.waitForFn(`document.body.innerText.includes('Mock Wandering Earth')`, {
            timeoutMs: 10000,
          });
          await clickButtonMatching(cdp, [/Mock Wandering Earth/]);
          await clickButtonMatching(cdp, [/第1集/]);
          await cdp.waitForSelector('video', { visible: true });
          await cdp.waitForFn(
            `Boolean(document.querySelector('video') && !document.querySelector('video').error)`,
            { timeoutMs: 10000 },
          );
          const hlsSeen = cdp.networkRequests.some((r) => r.url.includes('/api/v1/video/online/hls-proxy'));
          assert.ok(hlsSeen, 'Expected hls-proxy requests in browser network log');
          cdp.assertNoErrors('T1-F16-02');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T1-F16-03',
      feature: 'F16',
      domain: 'ui',
      title: 'Browser: Switching to 第2集 in online video player fetches new m3u8 via hls-proxy',
      async run(ctx) {
        await ensureVideoMocks(ctx.admin, ctx.mockBaseUrl);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.member,
          '/video/catalog?scope=online&q=Earth',
        );
        try {
          await cdp.waitForFn(`document.body.innerText.includes('Mock Wandering Earth')`, {
            timeoutMs: 10000,
          });
          await clickButtonMatching(cdp, [/Mock Wandering Earth/]);
          await clickButtonMatching(cdp, [/第1集/]);
          await cdp.waitForSelector('video', { visible: true });
          await clickButtonMatching(cdp, [/选集|episodes/i]);
          await clickButtonMatching(cdp, [/第2集/], { rootSelector: '[role="dialog"]' });
          await cdp.waitForFn(
            `Boolean(document.querySelector('video') && !document.querySelector('video').error)`,
            { timeoutMs: 10000 },
          );
          const ep2Seen = cdp.networkRequests.some((r) => r.url.includes('ep2'));
          assert.ok(ep2Seen, 'Expected ep2 hls-proxy request after episode switch');
          cdp.assertNoErrors('T1-F16-03');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T1-F16-04',
      feature: 'F16',
      domain: 'ui',
      title: 'Browser: Playing PeerTube mock video loads MP4 via stream-proxy with readyState >= 1',
      async run(ctx) {
        await ensureVideoMocks(ctx.admin, ctx.mockBaseUrl);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.member,
          '/video/catalog?scope=online&q=Blender',
        );
        try {
          await cdp.waitForFn(`document.body.innerText.includes('Mock Blender Open Movie')`, {
            timeoutMs: 10000,
          });
          await clickButtonMatching(cdp, [/Mock Blender Open Movie/]);
          await clickButtonMatching(cdp, [/播放|play|360|正片/i]);
          await cdp.waitForFn(
            `(() => {
              const v = document.querySelector('video');
              return Boolean(v && v.src.includes('stream-proxy') && v.readyState >= 1 && !v.error);
            })()`,
            { timeoutMs: 12000, label: 'PeerTube stream-proxy MP4 readyState>=1' },
          );
          cdp.assertNoErrors('T1-F16-04');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T1-F16-05',
      feature: 'F16',
      domain: 'ui',
      title: 'Browser: Online video player HUD controls (mute, speed, back) work without exceptions',
      async run(ctx) {
        await ensureVideoMocks(ctx.admin, ctx.mockBaseUrl);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.member,
          '/video/catalog?scope=online&q=Blender',
        );
        try {
          await cdp.waitForFn(`document.body.innerText.includes('Mock Blender Open Movie')`, {
            timeoutMs: 10000,
          });
          await clickButtonMatching(cdp, [/Mock Blender Open Movie/]);
          await clickButtonMatching(cdp, [/播放|play|360|正片/i]);
          await cdp.waitForSelector('video', { visible: true });
          const wasMuted = await cdp.evalJs(`Boolean(document.querySelector('video')?.muted)`);
          await clickButtonMatching(cdp, [/静音|mute/i]);
          await cdp.waitForFn(
            `Boolean(document.querySelector('video')?.muted) !== ${JSON.stringify(wasMuted)}`,
            { timeoutMs: 5000, label: 'video mute state toggled' },
          );
          await clickButtonMatching(cdp, [/返回|back/i]);
          await cdp.waitForFn(`!location.pathname.includes('/play/')`, { timeoutMs: 8000 });
          cdp.assertNoErrors('T1-F16-05');
        } finally {
          await cdp.close();
        }
      },
    },

    // =======================================================================
    // F17: Upstream Resilience (core)
    // =======================================================================
    {
      id: 'T1-F17-01',
      feature: 'F17',
      domain: 'core',
      title: 'Upstream 502 error isolates failing source while returning healthy source results',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'comic');
        const comics = buildMockComicSources(ctx.mockBaseUrl);
        const edges = buildEdgeMockSources(ctx.mockBaseUrl);
        const imp = await importSources(ctx.admin, 'comic', [comics.xkcd, edges.error502]);
        const ids = imp.sources.map((s) => s.id);

        const res = await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
          body: { keyword: 'Strip', source_ids: ids },
          expectedStatus: 200,
        });
        const items = res.json.results ?? res.json.items ?? [];
        const errors = res.json.errors ?? [];
        assert.ok(items.some((i) => (i.title ?? i.name) === 'Mock XKCD Strip'));
        assert.ok(errors.length >= 1, 'Expected 502 source error reported in errors[]');
      },
    },
    {
      id: 'T1-F17-02',
      feature: 'F17',
      domain: 'core',
      title: 'Upstream delay/timeout returns bounded response with error while server stays responsive',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'comic');
        const edges = buildEdgeMockSources(ctx.mockBaseUrl);
        const imp = await importSources(ctx.admin, 'comic', edges.delayTimeout);
        const start = performance.now();
        const res = await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
          body: { keyword: 'slow', source_ids: [imp.sources[0].id] },
          timeoutMs: 40000,
          expectedStatus: 200,
        });
        const elapsed = performance.now() - start;
        assert.ok(elapsed < 35000, `Expected bounded timeout, took ${Math.round(elapsed)}ms`);
        assert.ok((res.json.errors ?? []).length >= 1, 'Expected timeout error reported');

        const health = await ctx.member.failLoudlyRequest('GET', '/api/v1/system/capabilities', {
          expectedStatus: 200,
        });
        assert.ok(Array.isArray(health.json.modules));
      },
    },
    {
      id: 'T1-F17-03',
      feature: 'F17',
      domain: 'core',
      title: 'Malformed JSON upstream returns 200 with error captured in errors[] and no 500 panic',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'comic');
        const edges = buildEdgeMockSources(ctx.mockBaseUrl);
        const imp = await importSources(ctx.admin, 'comic', edges.malformedJson);
        const res = await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
          body: { keyword: 'broken', source_ids: [imp.sources[0].id] },
          expectedStatus: 200,
        });
        assert.equal((res.json.results ?? res.json.items ?? []).length, 0);
        assert.ok((res.json.errors ?? []).length >= 1);
      },
    },
    {
      id: 'T1-F17-04',
      feature: 'F17',
      domain: 'core',
      title: 'Search returning empty result set yields results: [] and errors: []',
      async run(ctx) {
        const comics = await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
          body: { keyword: 'NonExistentXYZ', source_ids: [comics.mangadex.id] },
          expectedStatus: 200,
        });
        assert.deepEqual(res.json.results ?? res.json.items, []);
        assert.deepEqual(res.json.errors, []);
      },
    },
    {
      id: 'T1-F17-05',
      feature: 'F17',
      domain: 'core',
      title: 'Cookie-gated login-action persists session cookie in runtime_state for /online/pages',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'comic');
        const edges = buildEdgeMockSources(ctx.mockBaseUrl);
        const imp = await importSources(ctx.admin, 'comic', edges.cookieGated);
        const srcId = imp.sources[0].id;

        const loginRes = await ctx.admin.failLoudlyRequest(
          'POST',
          `/api/v1/comic/sources/${srcId}/login-action`,
          {
            body: { login_info: { username: 'alice', password: 'secret' } },
            expectedStatus: 200,
          },
        );
        assert.ok(loginRes.json.source.cookie_hosts.length >= 1);

        await resetMockRequests(ctx.mockBaseUrl);
        const pagesRes = await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/pages', {
          body: {
            source_id: srcId,
            chapter_url: `${ctx.mockBaseUrl}/edge/gated/comic-pages`,
          },
          expectedStatus: 200,
        });
        assert.equal(pagesRes.json.pages.length, 1);
        assert.ok(pagesRes.json.pages[0].image_url.includes('/comic/xkcd/strip.png'));

        const reqs = await getMockRequests(ctx.mockBaseUrl);
        const gatedReq = reqs.find((r) => r.path === '/edge/gated/comic-pages');
        assert.ok(gatedReq?.headers.cookie?.includes('mock_sess=valid_cookie_42'));
      },
    },

    // =======================================================================
    // F18: Non-Regression (nonreg)
    // =======================================================================
    {
      id: 'T1-F18-01',
      feature: 'F18',
      domain: 'nonreg',
      title: 'Local library non-regression: upload, list, and stream/read across comic, music, video, book',
      async run(ctx) {
        // 1. Comic CBZ upload & read
        const cbzBuf = buildMinimalZip([{ name: '001.png', data: MOCK_PNG_BYTES }]);
        const comicForm = buildMultipartForm([
          { filename: 'nonreg_comic.cbz', contentType: 'application/zip', data: cbzBuf },
        ]);
        const upComic = await ctx.admin.failLoudlyRequest('POST', '/api/v1/comic/upload', {
          headers: { 'Content-Type': comicForm.contentType },
          body: comicForm.body,
          expectedStatus: 201,
        });
        assert.ok(upComic.json.uploaded.length === 1);
        const comicSeries = (
          await ctx.member.failLoudlyRequest('GET', '/api/v1/comic/series', { expectedStatus: 200 })
        ).json;
        assert.ok(comicSeries.length >= 1);

        // 2. Music MP3 upload & stream
        const musicForm = buildMultipartForm([
          { filename: 'nonreg_track.mp3', contentType: 'audio/mpeg', data: MOCK_MP3_BYTES },
        ]);
        await ctx.admin.failLoudlyRequest('POST', '/api/v1/music/upload', {
          headers: { 'Content-Type': musicForm.contentType },
          body: musicForm.body,
          expectedStatus: 201,
        });
        const tracksRes = (
          await ctx.member.failLoudlyRequest('GET', '/api/v1/music/tracks', { expectedStatus: 200 })
        ).json;
        assert.ok(tracksRes.items.length >= 1);
        const streamTrack = await ctx.member.failLoudlyRequest(
          'GET',
          `/api/v1/music/tracks/${tracksRes.items[0].id}/stream`,
          { expectedStatus: 200 },
        );
        assertMp3(streamTrack.buffer, 'local music stream');

        // 3. Video MP4 upload & stream
        const videoForm = buildMultipartForm([
          { filename: 'nonreg_video.mp4', contentType: 'video/mp4', data: MOCK_MP4_BYTES },
        ]);
        await ctx.admin.failLoudlyRequest('POST', '/api/v1/video/upload', {
          headers: { 'Content-Type': videoForm.contentType },
          body: videoForm.body,
          expectedStatus: 201,
        });
        const videos = (
          await ctx.member.failLoudlyRequest('GET', '/api/v1/video/videos', { expectedStatus: 200 })
        ).json;
        assert.ok(videos.length >= 1);
        const streamVid = await ctx.member.failLoudlyRequest(
          'GET',
          `/api/v1/video/videos/${videos[0].id}/stream`,
          { expectedStatus: 200 },
        );
        assertMp4(streamVid.buffer, 'local video stream');

        // 4. Book TXT upload & read chapter
        const txtContent = Buffer.from('Chapter 1\n\nHello xmedia non-regression book.', 'utf8');
        const bookForm = buildMultipartForm([
          { filename: 'nonreg_book.txt', contentType: 'text/plain', data: txtContent },
        ]);
        await ctx.admin.failLoudlyRequest('POST', '/api/v1/book/books/upload', {
          headers: { 'Content-Type': bookForm.contentType },
          body: bookForm.body,
          expectedStatus: 201,
        });
        const booksRes = (
          await ctx.member.failLoudlyRequest('GET', '/api/v1/book/books', { expectedStatus: 200 })
        ).json;
        assert.ok(booksRes.items.length >= 1);
      },
    },
    {
      id: 'T1-F18-02',
      feature: 'F18',
      domain: 'nonreg',
      title: 'xmedia-book online non-regression: import, search, book-info, toc, content, cover-proxy',
      async run(ctx) {
        const books = buildMockBookSources(ctx.mockBaseUrl);
        const imp = await importSources(ctx.admin, 'book', books.basicBook);
        const srcId = imp.sources[0].id;

        const searchRes = await ctx.member.failLoudlyRequest('POST', '/api/v1/book/online/search', {
          body: { keyword: 'Tower', source_ids: [srcId], page: 1 },
          expectedStatus: 200,
        });
        assert.ok(searchRes.json.items.length >= 1);
        const bookUrl = searchRes.json.items[0].book.book_url;

        const infoRes = await ctx.member.failLoudlyRequest('POST', '/api/v1/book/online/book-info', {
          body: { source_id: srcId, book_url: bookUrl },
          expectedStatus: 200,
        });
        assert.equal(infoRes.json.detail.name, 'Tower of Mock');

        const tocRes = await ctx.member.failLoudlyRequest('POST', '/api/v1/book/online/toc', {
          body: { source_id: srcId, toc_url: bookUrl },
          expectedStatus: 200,
        });
        assert.ok(tocRes.json.chapters.length >= 2);

        const contentRes = await ctx.member.failLoudlyRequest('POST', '/api/v1/book/online/content', {
          body: { source_id: srcId, chapter_url: tocRes.json.chapters[0].url ?? tocRes.json.chapters[0].chapter_url },
          expectedStatus: 200,
        });
        const contentText = contentRes.json.content.content ?? contentRes.json.content.text;
        assert.equal(typeof contentText, 'string');

        const coverRes = await ctx.member.failLoudlyRequest('GET', '/api/v1/book/online/cover-proxy', {
          query: { url: `${ctx.mockBaseUrl}/comic/xkcd/strip.png` },
          expectedStatus: 200,
        });
        assertPng(coverRes.buffer, 'xmedia-book cover-proxy');
      },
    },
    {
      id: 'T1-F18-03',
      feature: 'F18',
      domain: 'nonreg',
      title: 'Legacy OPDS (/opds/book/v1.2, /opds/comic/v1.2) and OpenSubsonic (/rest/ping.view) stay green',
      async run(ctx) {
        const keyClient = ctx.admin.withAuthMode('api_key');
        const opdsBook = await keyClient.failLoudlyRequest('GET', '/opds/book/v1.2', {
          expectedStatus: 200,
        });
        assert.ok(opdsBook.text.includes('<feed') && opdsBook.text.includes('xmlns'));

        const opdsComic = await keyClient.failLoudlyRequest('GET', '/opds/comic/v1.2', {
          expectedStatus: 200,
        });
        assert.ok(opdsComic.text.includes('<feed'));

        const ping = await ctx.admin.failLoudlyRequest('GET', '/rest/ping.view', {
          authMode: 'none',
          query: { apiKey: ctx.admin.apiKey, f: 'json' },
          expectedStatus: 200,
        });
        assert.equal(ping.json['subsonic-response']?.status, 'ok');
      },
    },
    {
      id: 'T1-F18-04',
      feature: 'F18',
      domain: 'nonreg',
      title: 'Gateway system capabilities & overview expose all 4 domains; optional cargo test gate',
      async run(ctx) {
        const cap = await ctx.member.failLoudlyRequest('GET', '/api/v1/system/capabilities', {
          authMode: 'none',
          expectedStatus: 200,
        });
        assert.deepEqual([...cap.json.modules].sort(), ['book', 'comic', 'music', 'video']);

        const overview = await ctx.admin.failLoudlyRequest('GET', '/api/v1/system/overview', {
          expectedStatus: 200,
        });
        assert.equal(overview.json.domains.length, 4);

        if (process.env.XMEDIA_E2E_RUN_BUILD_GATES === '1') {
          await execFileAsync('cargo', ['test', '--workspace'], {
            cwd: REPO_ROOT,
            timeout: 300000,
          });
        }
      },
    },
    {
      id: 'T1-F18-05',
      feature: 'F18',
      domain: 'nonreg',
      title: 'SPA static HTML, hashed immutable assets, deep-link fallback, and API/OPDS 404 exclusions',
      async run(ctx) {
        const rootRes = await ctx.admin.failLoudlyRequest('GET', '/', {
          authMode: 'none',
          expectedStatus: 200,
        });
        assert.ok((rootRes.headers.get('content-type') || '').includes('text/html'));
        assert.ok(rootRes.text.includes('<div id="app">'));
        assert.ok(rootRes.headers.get('content-security-policy'));

        const scriptMatch = rootRes.text.match(/src="(\/assets\/[^"]+\.js)"/);
        assert.ok(scriptMatch, 'Missing Vite JS bundle script tag in index.html');
        const assetRes = await ctx.admin.failLoudlyRequest('GET', scriptMatch[1], {
          authMode: 'none',
          expectedStatus: 200,
        });
        assert.ok((assetRes.headers.get('cache-control') || '').includes('immutable'));

        const spaDeep = await ctx.admin.failLoudlyRequest('GET', '/comic/series', {
          authMode: 'none',
          expectedStatus: 200,
        });
        assert.ok(spaDeep.text.includes('<div id="app">'));

        for (const missingPath of ['/_app/health', '/api/nonexistent', '/opds/nonexistent', '/rest/nonexistent/deep']) {
          const miss = await ctx.admin.request('GET', missingPath, { authMode: 'none' });
          assert.equal(miss.status, 404, `${missingPath} must return 404, not SPA HTML`);
        }
        const restMiss = await ctx.admin.request('GET', '/rest/nonexistent', { authMode: 'none' });
        assert.ok(
          !restMiss.text.includes('<div id="app">'),
          '/rest/nonexistent must return OpenSubsonic error response, not SPA HTML',
        );

        if (process.env.XMEDIA_E2E_RUN_BUILD_GATES === '1') {
          await execFileAsync('pnpm', ['--dir', 'web', 'run', 'typecheck'], {
            cwd: REPO_ROOT,
            timeout: 180000,
          });
          await execFileAsync('pnpm', ['--dir', 'web', 'run', 'build'], {
            cwd: REPO_ROOT,
            timeout: 180000,
          });
        }
      },
    },
  ];
}

// ---------------------------------------------------------------------------
// Public Runner Entrypoint
// ---------------------------------------------------------------------------

export async function runTier1(context = {}) {
  const {
    baseUrl: externalBaseUrl = null,
    xmediaBin = null,
    webDir = resolveWebDir(),
    keepTmp = false,
    domain = null,
    silent = false,
    MockOriginServer: MockCtor = MockOriginServer,
    XMediaServerManager: ManagerCtor = XMediaServerManager,
    ApiClient: ApiCtor = ApiClient,
    CdpClient: CdpCtor = CdpClient,
  } = context;

  const allCases = buildTier1Cases();
  const selectedCases = domain ? allCases.filter((c) => c.domain === domain) : allCases;
  const results = [];

  if (selectedCases.length === 0) {
    return { name: 'tier1', suite: 'tier1', total: 0, passed: 0, failed: 0, cases: [] };
  }

  const mockOrigin = new MockCtor();
  const mockBaseUrl = await mockOrigin.start();

  const needsMockSut = selectedCases.some((c) => c.sut !== 'hardened');
  const needsHardenedSut = selectedCases.some((c) => c.sut === 'hardened');

  let mockManager = null;
  let hardenedManager = null;

  try {
    let baseUrl = externalBaseUrl;
    let admin = null;
    let member = null;
    let hardenedAdmin = null;
    let hardenedMember = null;

    try {
      if (externalBaseUrl) {
        ({ admin, member } = await bootstrapClients(externalBaseUrl, ApiCtor));
        hardenedAdmin = admin;
        hardenedMember = member;
      } else {
        if (needsMockSut) {
          mockManager = new ManagerCtor({ xmediaBin, webDir, keepTmp });
          baseUrl = await mockManager.start({ mode: 'mock' });
          ({ admin, member } = await bootstrapClients(baseUrl, ApiCtor));
        }
        if (needsHardenedSut) {
          hardenedManager = new ManagerCtor({ xmediaBin, webDir, keepTmp });
          const hardenedBaseUrl = await hardenedManager.start({ mode: 'hardened' });
          ({ admin: hardenedAdmin, member: hardenedMember } = await bootstrapClients(
            hardenedBaseUrl,
            ApiCtor,
          ));
        }
      }
    } catch (bootErr) {
      results.push({
        id: 'T1-BOOTSTRAP',
        feature: 'BOOTSTRAP',
        domain: domain || 'core',
        title: 'Tier 1 SUT startup & auth bootstrap',
        status: 'fail',
        durationMs: 0,
        error: bootErr.stack || bootErr.message,
      });
      if (!silent) {
        console.error(`  [FAIL] T1-BOOTSTRAP: ${bootErr.message}`);
      }
      return {
        name: 'tier1',
        suite: 'tier1',
        total: 1,
        passed: 0,
        failed: 1,
        cases: results,
      };
    }

    const ctx = {
      externalBaseUrl,
      baseUrl,
      mockBaseUrl,
      admin,
      member,
      hardenedAdmin,
      hardenedMember,
      xmediaBin,
      webDir,
      keepTmp,
      XMediaServerManager: ManagerCtor,
      ApiClient: ApiCtor,
      CdpClient: CdpCtor,
    };

    for (const tc of selectedCases) {
      const start = performance.now();
      try {
        await tc.run(ctx);
        const durationMs = Math.round(performance.now() - start);
        results.push({
          id: tc.id,
          feature: tc.feature,
          domain: tc.domain,
          title: tc.title,
          status: 'pass',
          durationMs,
        });
        if (!silent) {
          console.log(`  [PASS] ${tc.id}: ${tc.title} (${durationMs}ms)`);
        }
      } catch (err) {
        const durationMs = Math.round(performance.now() - start);
        results.push({
          id: tc.id,
          feature: tc.feature,
          domain: tc.domain,
          title: tc.title,
          status: 'fail',
          durationMs,
          error: err.stack || err.message,
        });
        if (!silent) {
          console.error(`  [FAIL] ${tc.id}: ${tc.title} (${durationMs}ms)\n    ${err.message}`);
        }
        if (err instanceof SutEndpointMissingError && !silent) {
          console.error(`    ${err.message}`);
        }
      }
    }
  } finally {
    if (mockManager) await mockManager.cleanup();
    if (hardenedManager) await hardenedManager.cleanup();
    await mockOrigin.stop();
  }

  const passed = results.filter((r) => r.status === 'pass').length;
  const failed = results.filter((r) => r.status === 'fail').length;
  return {
    name: 'tier1',
    suite: 'tier1',
    total: results.length,
    passed,
    failed,
    cases: results,
  };
}

export const runSuite = runTier1;
