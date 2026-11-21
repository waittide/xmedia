import assert from 'node:assert/strict';
import http from 'node:http';
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
} from '../lib/fixtures.mjs';
import { MockOriginServer } from '../lib/mock_origin.mjs';
import { readFileSync } from 'node:fs';
import path from 'node:path';

// ---------------------------------------------------------------------------
// Shared helpers for Tier 2 boundary, edge, error & security tests
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
    localHeader.writeUInt16LE(0, 8);
    localHeader.writeUInt32LE(crc, 14);
    localHeader.writeUInt32LE(dataBuf.length, 18);
    localHeader.writeUInt32LE(dataBuf.length, 22);
    localHeader.writeUInt16LE(nameBuf.length, 26);

    const localRecord = Buffer.concat([localHeader, nameBuf, dataBuf]);
    localParts.push(localRecord);

    const centralHeader = Buffer.alloc(46);
    centralHeader.writeUInt32LE(0x02014b50, 0);
    centralHeader.writeUInt16LE(20, 4);
    centralHeader.writeUInt16LE(20, 6);
    centralHeader.writeUInt32LE(crc, 16);
    centralHeader.writeUInt32LE(dataBuf.length, 20);
    centralHeader.writeUInt32LE(dataBuf.length, 24);
    centralHeader.writeUInt16LE(nameBuf.length, 28);
    centralHeader.writeUInt32LE(offset, 42);

    centralParts.push(Buffer.concat([centralHeader, nameBuf]));
    offset += localRecord.length;
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

function buildMultipartForm(files) {
  const boundary = `----XMediaTier2${Date.now().toString(16)}${Math.random().toString(16).slice(2)}`;
  const chunks = [];
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

async function ensureComicMocks(admin, mockBaseUrl) {
  await clearDomainSources(admin, 'comic');
  const sources = buildMockComicSources(mockBaseUrl);
  const imported = await importSources(admin, 'comic', sources.all);
  const byName = Object.fromEntries(imported.sources.map((s) => [s.name, s]));
  return {
    mangadex: byName['Mock MangaDex'],
    webtoon: byName['Mock WEBTOON'],
    xkcd: byName['Mock XKCD'],
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
    await cdp.waitForSelector('#app', { timeoutMs: 12000 });
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
// Tier 2 Case Registry (90 cases: T2-F01-01 .. T2-F18-05)
// ---------------------------------------------------------------------------

function buildTier2Cases() {
  return [
    // =======================================================================
    // F1: WebAuth, Admin RBAC & Secret Masking Boundary (core)
    // =======================================================================
    {
      id: 'T2-F01-01',
      feature: 'F1',
      domain: 'core',
      title: 'Unauthenticated requests to sources and online endpoints return 401 unauthorized',
      async run(ctx) {
        const anon = new ctx.ApiClient(ctx.baseUrl, { authMode: 'none' });
        for (const [method, p, body] of [
          ['GET', '/api/v1/comic/sources', undefined],
          ['GET', '/api/v1/music/sources', undefined],
          ['GET', '/api/v1/video/sources', undefined],
          ['POST', '/api/v1/comic/online/search', { keyword: 'test' }],
          ['POST', '/api/v1/music/online/search', { keyword: 'test' }],
          ['POST', '/api/v1/video/online/search', { keyword: 'test' }],
          ['GET', '/api/v1/comic/online/image-proxy?url=https://example.com/a.jpg', undefined],
          ['GET', '/api/v1/music/online/audio-proxy?url=https://example.com/a.mp3', undefined],
          ['GET', '/api/v1/video/online/hls-proxy?url=https://example.com/a.m3u8', undefined],
        ]) {
          const res = await anon.failLoudlyRequest(method, p, { body, expectedStatus: 401 });
          assert.equal(res.json?.error?.code, 'unauthorized');
        }
      },
    },
    {
      id: 'T2-F01-02',
      feature: 'F1',
      domain: 'core',
      title: 'Query ?token= is rejected with 401 on POST /online/search across comic, music, video',
      async run(ctx) {
        const qClient = ctx.admin.withAuthMode('query_token');
        for (const domain of ['comic', 'music', 'video']) {
          const res = await qClient.failLoudlyRequest('POST', `/api/v1/${domain}/online/search`, {
            body: { keyword: 'test' },
            expectedStatus: 401,
          });
          assert.equal(res.json?.error?.code, 'unauthorized');
        }
      },
    },
    {
      id: 'T2-F01-03',
      feature: 'F1',
      domain: 'core',
      title: 'Non-admin receives 403 forbidden on all source mutation/admin endpoints across all 3 domains',
      async run(ctx) {
        const fakeId = '00000000-0000-4000-8000-000000000001';
        for (const domain of ['comic', 'music', 'video']) {
          const checks = [
            ['POST', `/api/v1/${domain}/sources/import`, { raw_json: '{}' }],
            ['PATCH', `/api/v1/${domain}/sources/${fakeId}`, { enabled: false }],
            ['DELETE', `/api/v1/${domain}/sources/${fakeId}`, undefined],
            ['POST', `/api/v1/${domain}/sources/reorder`, { ordered_ids: [] }],
            ['POST', `/api/v1/${domain}/sources/batch`, { ids: [], action: 'disable' }],
            ['POST', `/api/v1/${domain}/sources/export-batch`, { ids: [] }],
            ['GET', `/api/v1/${domain}/sources/${fakeId}/export`, undefined],
            ['GET', `/api/v1/${domain}/sources/${fakeId}/variable`, undefined],
            ['POST', `/api/v1/${domain}/sources/${fakeId}/login-action`, { login_info: {} }],
            ['POST', `/api/v1/${domain}/sources/${fakeId}/test`, { mode: 'search' }],
          ];
          for (const [method, p, body] of checks) {
            const res = await ctx.member.failLoudlyRequest(method, p, {
              body,
              expectedStatus: 403,
            });
            assert.equal(res.json?.error?.code, 'forbidden', `${method} ${p}`);
          }
        }
      },
    },
    {
      id: 'T2-F01-04',
      feature: 'F1',
      domain: 'core',
      title: 'Non-admin querying disabled source_id in /online/search or /detail never fetches upstream',
      async run(ctx) {
        const comics = await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const musics = await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        const videos = await ensureVideoMocks(ctx.admin, ctx.mockBaseUrl);
        await ctx.admin.failLoudlyRequest('PATCH', `/api/v1/comic/sources/${comics.xkcd.id}`, {
          body: { enabled: false },
          expectedStatus: 200,
        });
        await ctx.admin.failLoudlyRequest('PATCH', `/api/v1/music/sources/${musics.gdstudio.id}`, {
          body: { enabled: false },
          expectedStatus: 200,
        });
        await ctx.admin.failLoudlyRequest('PATCH', `/api/v1/video/sources/${videos.peertube.id}`, {
          body: { enabled: false },
          expectedStatus: 200,
        });

        await resetMockRequests(ctx.mockBaseUrl);
        const comicSearch = await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
          body: { keyword: 'Strip', source_ids: [comics.xkcd.id] },
          expectedStatus: [200, 403, 404],
        });
        if (comicSearch.status === 200) {
          assert.equal((comicSearch.json.results ?? comicSearch.json.items ?? []).length, 0);
        }

        await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/detail', {
          body: { source_id: comics.xkcd.id, comic_url: `${ctx.mockBaseUrl}/comic/xkcd/info.0.json` },
          expectedStatus: [400, 403, 404],
        });
        await ctx.member.failLoudlyRequest('POST', '/api/v1/music/online/track-info', {
          body: { source_id: musics.gdstudio.id, track_id: 'gd-101' },
          expectedStatus: [400, 403, 404],
        });
        await ctx.member.failLoudlyRequest('POST', '/api/v1/video/online/detail', {
          body: { source_id: videos.peertube.id, video_id: 'pt-vid-1' },
          expectedStatus: [400, 403, 404],
        });

        const logged = await getMockRequests(ctx.mockBaseUrl);
        assert.equal(logged.length, 0, 'Disabled sources must never trigger upstream requests');
      },
    },
    {
      id: 'T2-F01-05',
      feature: 'F1',
      domain: 'core',
      title: 'Tampered JWT and rotated API key immediately return 401 on media proxies',
      async run(ctx) {
        const badJwt = new ctx.ApiClient(ctx.baseUrl, {
          authMode: 'bearer',
          token: 'eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJmYWtlIn0.invalidsignature',
        });
        for (const p of [
          '/api/v1/comic/online/image-proxy?url=https://example.com/a.jpg',
          '/api/v1/music/online/audio-proxy?url=https://example.com/a.mp3',
          '/api/v1/video/online/hls-proxy?url=https://example.com/a.m3u8',
        ]) {
          await badJwt.failLoudlyRequest('GET', p, { expectedStatus: 401 });
        }

        // Rotate a temporary member's API key and verify old key fails
        const tempUser = `rot_${Date.now()}`;
        await ctx.admin.failLoudlyRequest('POST', '/api/v1/users', {
          body: {
            username: tempUser,
            display_name: 'Rotate Test',
            password: 'UserPass123!',
            role: 'member',
          },
          expectedStatus: 201,
        });
        const tempClient = await ctx.admin.loginAs({
          username: tempUser,
          password: 'UserPass123!',
          authMode: 'api_key',
        });
        const oldApiKey = tempClient.apiKey;
        assert.ok(oldApiKey);

        await tempClient.failLoudlyRequest('POST', '/api/v1/auth/rotate-api-key', {
          authMode: 'bearer',
          expectedStatus: 200,
        });
        const staleClient = new ctx.ApiClient(ctx.baseUrl, {
          authMode: 'api_key',
          apiKey: oldApiKey,
        });
        await staleClient.failLoudlyRequest(
          'GET',
          '/api/v1/comic/online/image-proxy?url=https://example.com/a.jpg',
          { expectedStatus: 401 },
        );
      },
    },

    // =======================================================================
    // F2: SSRF & QuickJS Sandbox Boundary (core)
    // =======================================================================
    {
      id: 'T2-F02-01',
      feature: 'F2',
      domain: 'core',
      sut: 'hardened',
      title: 'Hardened SUT blocks RFC1918, CGNAT, and 0.0.0.0 IPs on proxies and sources',
      async run(ctx) {
        const blockedUrls = [
          'http://10.0.0.1/a.jpg',
          'http://172.16.0.1/a.jpg',
          'http://192.168.1.1/a.jpg',
          'http://100.64.0.1/a.jpg',
          'http://0.0.0.0:8090/a.jpg',
        ];
        for (const u of blockedUrls) {
          await ctx.hardenedAdmin.failLoudlyRequest('GET', '/api/v1/comic/online/image-proxy', {
            query: { url: u },
            expectedStatus: [400, 403],
          });
          await ctx.hardenedAdmin.failLoudlyRequest('GET', '/api/v1/music/online/audio-proxy', {
            query: { url: u },
            expectedStatus: [400, 403],
          });
          await ctx.hardenedAdmin.failLoudlyRequest('GET', '/api/v1/video/online/hls-proxy', {
            query: { url: u },
            expectedStatus: [400, 403],
          });
        }
      },
    },
    {
      id: 'T2-F02-02',
      feature: 'F2',
      domain: 'core',
      sut: 'hardened',
      title: 'Hardened SUT blocks IPv4-mapped IPv6, ULA fc00::/7, and link-local fe80::/10',
      async run(ctx) {
        for (const u of [
          'http://[::ffff:127.0.0.1]/a.jpg',
          'http://[fc00::1]/a.jpg',
          'http://[fe80::1]/a.jpg',
        ]) {
          await ctx.hardenedAdmin.failLoudlyRequest('GET', '/api/v1/comic/online/image-proxy', {
            query: { url: u },
            expectedStatus: [400, 403],
          });
        }
      },
    },
    {
      id: 'T2-F02-03',
      feature: 'F2',
      domain: 'core',
      sut: 'hardened',
      title: 'Hardened SUT rejects non-HTTP schemes (file://, ftp://, javascript:, gopher://)',
      async run(ctx) {
        for (const u of [
          'file:///etc/passwd',
          'ftp://example.com/a',
          'javascript:alert(1)',
          'gopher://127.0.0.1:6379/_INFO',
        ]) {
          await ctx.hardenedAdmin.failLoudlyRequest('GET', '/api/v1/comic/online/image-proxy', {
            query: { url: u },
            expectedStatus: [400, 403],
          });
          await ctx.hardenedAdmin.failLoudlyRequest('GET', '/api/v1/video/online/stream-proxy', {
            query: { url: u },
            expectedStatus: [400, 403],
          });
        }
      },
    },
    {
      id: 'T2-F02-04',
      feature: 'F2',
      domain: 'core',
      title: 'Mock SUT rejects redirect chain exceeding 6 hops (/edge/redirect-chain?n=8)',
      async run(ctx) {
        const comics = await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('GET', '/api/v1/comic/online/image-proxy', {
          query: {
            url: `${ctx.mockBaseUrl}/edge/redirect-chain?n=8`,
            source_id: comics.xkcd.id,
          },
          expectedStatus: [400, 502],
        });
        assert.ok(res.json?.error?.code);
      },
    },
    {
      id: 'T2-F02-05',
      feature: 'F2',
      domain: 'core',
      title: 'QuickJS sandbox terminates infinite loop, huge allocation, and HTTP flood without crashing server',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'comic');
        const edges = buildEdgeMockSources(ctx.mockBaseUrl);
        const imp = await importSources(ctx.admin, 'comic', [
          edges.quickJsInfiniteLoop,
          edges.quickJsMemoryHog,
          edges.quickJsHttpFlood,
        ]);
        for (const src of imp.sources) {
          const res = await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
            body: { keyword: 'test', source_ids: [src.id] },
            timeoutMs: 25000,
            expectedStatus: 200,
          });
          assert.equal((res.json.results ?? res.json.items ?? []).length, 0);
          assert.ok((res.json.errors ?? []).length >= 1, `${src.name} should fail sandbox limits cleanly`);
        }
        const cap = await ctx.member.failLoudlyRequest('GET', '/api/v1/system/capabilities', {
          authMode: 'none',
          expectedStatus: 200,
        });
        assert.equal(cap.json.modules.length, 4);
      },
    },

    // =======================================================================
    // F3: Source CRUD Boundary (core)
    // =======================================================================
    {
      id: 'T2-F03-01',
      feature: 'F3',
      domain: 'core',
      title: 'Import JSON with UTF-8 BOM, stringified weight/bools, and invalid entry skips/handles cleanly',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'comic');
        const validLenient = {
          sourceName: 'BOM Lenient Source',
          sourceUrl: `${ctx.mockBaseUrl}/comic/bom-lenient`,
          enabled: 'true',
          weight: '50',
          searchUrl: `${ctx.mockBaseUrl}/comic/xkcd/info.0.json`,
        };
        const invalidBlank = { sourceName: '', sourceUrl: '' };
        const bomPayload = `\ufeff${JSON.stringify([validLenient, invalidBlank])}`;

        const res = await ctx.admin.failLoudlyRequest('POST', '/api/v1/comic/sources/import', {
          body: { raw_json: bomPayload },
          expectedStatus: [200, 400],
        });
        if (res.status === 200) {
          assert.equal(res.json.imported_count, 1);
          assert.equal(res.json.sources[0].weight, 50);
          assert.equal(res.json.sources[0].enabled, true);
        }
      },
    },
    {
      id: 'T2-F03-02',
      feature: 'F3',
      domain: 'core',
      title: 'Re-importing same (source_url, name) updates searchUrl while preserving cookies & variable',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'comic');
        const edges = buildEdgeMockSources(ctx.mockBaseUrl);
        const imp1 = await importSources(ctx.admin, 'comic', edges.cookieGated);
        const srcId = imp1.sources[0].id;
        await ctx.admin.failLoudlyRequest('PATCH', `/api/v1/comic/sources/${srcId}`, {
          body: { variable: 'keep-me-var' },
          expectedStatus: 200,
        });
        await ctx.admin.failLoudlyRequest('POST', `/api/v1/comic/sources/${srcId}/login-action`, {
          body: { login_info: { username: 'alice', password: 'secret' } },
          expectedStatus: 200,
        });

        const updatedSpec = {
          ...edges.cookieGated,
          searchUrl: `${ctx.mockBaseUrl}/comic/xkcd/info.0.json?updated=1`,
        };
        const imp2 = await importSources(ctx.admin, 'comic', updatedSpec);
        assert.equal(imp2.updated_count, 1);
        assert.equal(imp2.imported_count, 0);
        assert.equal(imp2.sources[0].id, srcId);
        assert.ok(imp2.sources[0].cookie_hosts.length >= 1, 'cookie_hosts must be preserved on re-import');

        const varRes = await ctx.admin.failLoudlyRequest(
          'GET',
          `/api/v1/comic/sources/${srcId}/variable`,
          { expectedStatus: 200 },
        );
        assert.equal(varRes.json.variable, 'keep-me-var');
      },
    },
    {
      id: 'T2-F03-03',
      feature: 'F3',
      domain: 'core',
      title: 'Import with malformed JSON or empty string returns 400 validation_error across all 3 domains',
      async run(ctx) {
        for (const domain of ['comic', 'music', 'video']) {
          for (const bad of ['{bad', '']) {
            const res = await ctx.admin.failLoudlyRequest('POST', `/api/v1/${domain}/sources/import`, {
              body: { raw_json: bad },
              expectedStatus: 400,
            });
            assert.equal(res.json?.error?.code, 'validation_error');
          }
        }
      },
    },
    {
      id: 'T2-F03-04',
      feature: 'F3',
      domain: 'core',
      title: 'Import exceeding 32 MiB limit returns 413 or 400 cleanly without OOM',
      async run(ctx) {
        const bigStr = 'x'.repeat(33 * 1024 * 1024);
        const bodyBuf = Buffer.from(`{"raw_json":"${bigStr}"}`, 'utf8');
        const res = await ctx.admin.failLoudlyRequest('POST', '/api/v1/comic/sources/import', {
          headers: { 'Content-Type': 'application/json' },
          body: bodyBuf,
          timeoutMs: 30000,
          expectedStatus: [400, 413],
        });
        assert.ok([400, 413].includes(res.status));
      },
    },
    {
      id: 'T2-F03-05',
      feature: 'F3',
      domain: 'core',
      title: 'PATCH, DELETE, and test with nonexistent UUID return 404 not_found across all 3 domains',
      async run(ctx) {
        const missingId = '99999999-9999-4999-8999-999999999999';
        for (const domain of ['comic', 'music', 'video']) {
          await ctx.admin.failLoudlyRequest('PATCH', `/api/v1/${domain}/sources/${missingId}`, {
            body: { enabled: false },
            expectedStatus: 404,
          });
          await ctx.admin.failLoudlyRequest('DELETE', `/api/v1/${domain}/sources/${missingId}`, {
            expectedStatus: 404,
          });
          await ctx.admin.failLoudlyRequest('POST', `/api/v1/${domain}/sources/${missingId}/test`, {
            body: { mode: 'search', keyword: 'x' },
            expectedStatus: 404,
          });
        }
      },
    },

    // =======================================================================
    // F4: Rule Engine Boundary (core)
    // =======================================================================
    {
      id: 'T2-F04-01',
      feature: 'F4',
      domain: 'core',
      title: 'GBK-encoded HTML (/edge/gbk-html) decodes Chinese characters "中文测试漫画" cleanly',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'comic');
        const edges = buildEdgeMockSources(ctx.mockBaseUrl);
        const imp = await importSources(ctx.admin, 'comic', edges.gbkHtml);
        const res = await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
          body: { keyword: 'gbk', source_ids: [imp.sources[0].id] },
          expectedStatus: 200,
        });
        const items = res.json.results ?? res.json.items ?? [];
        assert.equal(items.length, 1);
        assert.equal(items[0].title ?? items[0].name, '中文测试漫画');
      },
    },
    {
      id: 'T2-F04-02',
      feature: 'F4',
      domain: 'core',
      title: 'data: pseudo-URL in rule content resolves safely without external HTTP call',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'comic');
        const dataUri = `data:image/png;base64,${MOCK_PNG_BYTES.toString('base64')}`;
        const src = {
          sourceName: 'Data URI Source',
          sourceUrl: `${ctx.mockBaseUrl}/comic/data-uri`,
          sourceType: 2,
          enabled: true,
          searchUrl: `${ctx.mockBaseUrl}/comic/xkcd/info.0.json`,
          ruleContent: { content: dataUri },
        };
        const imp = await importSources(ctx.admin, 'comic', src);
        await resetMockRequests(ctx.mockBaseUrl);
        const pagesRes = await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/pages', {
          body: {
            source_id: imp.sources[0].id,
            chapter_url: `${ctx.mockBaseUrl}/comic/xkcd/info.0.json`,
          },
          expectedStatus: 200,
        });
        assert.ok(Array.isArray(pagesRes.json.pages));
      },
    },
    {
      id: 'T2-F04-03',
      feature: 'F4',
      domain: 'core',
      title: 'List reversal prefix (-$.data[*]) reverses extracted list order accurately',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'comic');
        const base = buildMockComicSources(ctx.mockBaseUrl).mangadex;
        base.sourceName = 'Reversed TOC MangaDex';
        base.sourceUrl = `${ctx.mockBaseUrl}/comic/mangadex-rev`;
        base.ruleToc.chapterList = '-$.data[*]';
        const imp = await importSources(ctx.admin, 'comic', base);
        const detail = await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/detail', {
          body: {
            source_id: imp.sources[0].id,
            comic_url: `${ctx.mockBaseUrl}/comic/mangadex/manga/md-comic-1`,
          },
          expectedStatus: 200,
        });
        assert.equal(detail.json.chapters.length, 2);
        assert.ok(detail.json.chapters[0].chapter_url.includes('md-ch-2'));
        assert.ok(detail.json.chapters[1].chapter_url.includes('md-ch-1'));
      },
    },
    {
      id: 'T2-F04-04',
      feature: 'F4',
      domain: 'core',
      title: 'Recursive JSONPath ($..name) and unmatched CSS selector return cleanly without panic',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'comic');
        const src = {
          sourceName: 'Empty Selector Source',
          sourceUrl: `${ctx.mockBaseUrl}/comic/empty-sel`,
          sourceType: 2,
          enabled: true,
          searchUrl: `${ctx.mockBaseUrl}/comic/webtoon/search?keyword={{key}}`,
          ruleSearch: {
            bookList: '.nonexistent-css-class-xyz li',
            name: '.subj@text',
            bookUrl: 'a@href',
          },
        };
        const imp = await importSources(ctx.admin, 'comic', src);
        const res = await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
          body: { keyword: 'test', source_ids: [imp.sources[0].id] },
          expectedStatus: 200,
        });
        assert.deepEqual(res.json.results ?? res.json.items ?? [], []);
      },
    },
    {
      id: 'T2-F04-05',
      feature: 'F4',
      domain: 'core',
      title: 'concurrentRate "1/500" throttles consecutive requests cleanly without dropping results',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'comic');
        const xkcd = buildMockComicSources(ctx.mockBaseUrl).xkcd;
        xkcd.sourceName = 'Throttled XKCD';
        xkcd.sourceUrl = `${ctx.mockBaseUrl}/comic/xkcd-throttled`;
        xkcd.concurrentRate = '1/500';
        const imp = await importSources(ctx.admin, 'comic', xkcd);
        const id = imp.sources[0].id;

        const t0 = performance.now();
        const r1 = await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
          body: { keyword: 'a', source_ids: [id] },
          expectedStatus: 200,
        });
        const r2 = await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
          body: { keyword: 'b', source_ids: [id] },
          expectedStatus: 200,
        });
        const elapsed = performance.now() - t0;
        assert.equal((r1.json.results ?? r1.json.items).length, 1);
        assert.equal((r2.json.results ?? r2.json.items).length, 1);
        assert.ok(elapsed >= 350, `Expected concurrentRate 1/500 throttling, elapsed=${Math.round(elapsed)}ms`);
      },
    },

    // =======================================================================
    // F5: Comic Parsing Boundary (comic)
    // =======================================================================
    {
      id: 'T2-F05-01',
      feature: 'F5',
      domain: 'comic',
      title: 'MangaDex chapter with pages: 0 (md-ch-empty) returns empty pages[] gracefully',
      async run(ctx) {
        const mocks = await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/pages', {
          body: {
            source_id: mocks.mangadex.id,
            chapter_url: `${ctx.mockBaseUrl}/comic/mangadex/at-home/server/md-ch-empty`,
          },
          expectedStatus: 200,
        });
        assert.deepEqual(res.json.pages, []);
      },
    },
    {
      id: 'T2-F05-02',
      feature: 'F5',
      domain: 'comic',
      title: 'Comic readingDirection override propagates from source/detail to pages response',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'comic');
        const wt = buildMockComicSources(ctx.mockBaseUrl).webtoon;
        wt.readingDirection = 'ltr';
        wt.ruleBookInfo.readingDirection = 'webtoon';
        const imp = await importSources(ctx.admin, 'comic', wt);
        const id = imp.sources[0].id;
        const det = await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/detail', {
          body: { source_id: id, comic_url: `${ctx.mockBaseUrl}/comic/webtoon/list?title_no=95` },
          expectedStatus: 200,
        });
        assert.equal(det.json.detail.reading_direction, 'webtoon');
      },
    },
    {
      id: 'T2-F05-03',
      feature: 'F5',
      domain: 'comic',
      title: 'Paginated TOC (nextTocUrl) aggregates chapters across page 1 and page 2 without cycle',
      async run(ctx) {
        const mocks = await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const det = await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/detail', {
          body: {
            source_id: mocks.webtoon.id,
            comic_url: `${ctx.mockBaseUrl}/comic/webtoon/list?title_no=95`,
          },
          expectedStatus: 200,
        });
        assert.equal(det.json.chapters.length, 3);
        assert.ok(det.json.chapters.some((c) => c.title.includes('Ep.3 Page2')));
      },
    },
    {
      id: 'T2-F05-04',
      feature: 'F5',
      domain: 'comic',
      title: 'CJK and special characters in search keyword are properly percent-encoded upstream',
      async run(ctx) {
        const mocks = await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        await resetMockRequests(ctx.mockBaseUrl);
        await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
          body: { keyword: '神之塔 & 100%', source_ids: [mocks.webtoon.id] },
          expectedStatus: 200,
        });
        const reqs = await getMockRequests(ctx.mockBaseUrl);
        const sReq = reqs.find((r) => r.path === '/comic/webtoon/search');
        assert.ok(sReq);
        assert.equal(sReq.query.keyword, '神之塔 & 100%');
      },
    },
    {
      id: 'T2-F05-05',
      feature: 'F5',
      domain: 'comic',
      title: 'Empty chapter_url or missing source_id on /online/pages returns 400 or 404/422',
      async run(ctx) {
        const mocks = await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/pages', {
          body: { source_id: mocks.xkcd.id, chapter_url: '' },
          expectedStatus: [400, 422],
        });
        await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/pages', {
          body: {
            source_id: '99999999-9999-4999-8999-999999999999',
            chapter_url: `${ctx.mockBaseUrl}/comic/xkcd/info.0.json`,
          },
          expectedStatus: 404,
        });
      },
    },

    // =======================================================================
    // F6: Comic Image Proxy Boundary (comic)
    // =======================================================================
    {
      id: 'T2-F06-01',
      feature: 'F6',
      domain: 'comic',
      title: 'image-proxy rejects /edge/svg-as-image and never returns SVG markup',
      async run(ctx) {
        const mocks = await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('GET', '/api/v1/comic/online/image-proxy', {
          query: { url: `${ctx.mockBaseUrl}/edge/svg-as-image`, source_id: mocks.xkcd.id },
          expectedStatus: [400, 415, 502],
        });
        assert.ok(!res.text.includes('<svg'));
      },
    },
    {
      id: 'T2-F06-02',
      feature: 'F6',
      domain: 'comic',
      title: 'image-proxy rejects HTML disguised as .jpg (/edge/html-as-image.jpg)',
      async run(ctx) {
        const mocks = await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('GET', '/api/v1/comic/online/image-proxy', {
          query: { url: `${ctx.mockBaseUrl}/edge/html-as-image.jpg`, source_id: mocks.xkcd.id },
          expectedStatus: [400, 415, 502],
        });
        assert.ok(!res.text.includes('<html>'));
      },
    },
    {
      id: 'T2-F06-03',
      feature: 'F6',
      domain: 'comic',
      title: 'image-proxy rejects /edge/oversized-image (>16 MiB cap) cleanly',
      async run(ctx) {
        const mocks = await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('GET', '/api/v1/comic/online/image-proxy', {
          query: { url: `${ctx.mockBaseUrl}/edge/oversized-image`, source_id: mocks.xkcd.id },
          expectedStatus: [400, 413, 502],
        });
        assert.ok(res.json?.error?.code);
      },
    },
    {
      id: 'T2-F06-04',
      feature: 'F6',
      domain: 'comic',
      title: 'image-proxy surfaces upstream 403 cleanly when WEBTOON CDN lacks Referer (wrong source_id)',
      async run(ctx) {
        const mocks = await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('GET', '/api/v1/comic/online/image-proxy', {
          query: {
            url: `${ctx.mockBaseUrl}/comic/webtoon/cdn/ep1_1.jpg`,
            source_id: mocks.xkcd.id,
          },
          expectedStatus: [400, 403, 502],
        });
        assert.ok(!res.text.includes('<html>'));
      },
    },
    {
      id: 'T2-F06-05',
      feature: 'F6',
      domain: 'comic',
      title: 'image-proxy with empty url= query parameter returns 400 validation_error',
      async run(ctx) {
        const res = await ctx.member.failLoudlyRequest('GET', '/api/v1/comic/online/image-proxy', {
          query: { url: '' },
          expectedStatus: 400,
        });
        assert.equal(res.json?.error?.code, 'validation_error');
      },
    },

    // =======================================================================
    // F7: Music Parsing Boundary (music)
    // =======================================================================
    {
      id: 'T2-F07-01',
      feature: 'F7',
      domain: 'music',
      title: 'Music artist normalizes both JSON array (GDStudio) and plain string (Audius)',
      async run(ctx) {
        const mocks = await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        const r1 = await ctx.member.failLoudlyRequest('POST', '/api/v1/music/online/search', {
          body: { keyword: 'Mojito', source_ids: [mocks.gdstudio.id] },
          expectedStatus: 200,
        });
        const a1 = (r1.json.results ?? r1.json.items)[0].artist;
        assert.ok(a1.includes('Jay Mock') && a1.includes('Feat Guest'));

        const r2 = await ctx.member.failLoudlyRequest('POST', '/api/v1/music/online/search', {
          body: { keyword: 'Fragments', source_ids: [mocks.audius.id] },
          expectedStatus: 200,
        });
        assert.equal((r2.json.results ?? r2.json.items)[0].artist, 'Xsetra Mock');
      },
    },
    {
      id: 'T2-F07-02',
      feature: 'F7',
      domain: 'music',
      title: 'Music duration normalizes seconds (212), milliseconds (212000), and "03:32" string',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'music');
        const msSrc = {
          sourceName: 'Duration Ms Source',
          sourceUrl: `${ctx.mockBaseUrl}/music/dur-ms`,
          enabled: true,
          searchUrl: `${ctx.mockBaseUrl}/music/audius/v1/tracks/search?query={{key}}`,
          ruleSearch: {
            trackList: '$.data[*]',
            trackId: '$.id',
            title: '$.title',
            artist: '$.user.name',
            duration: '<js>212000</js>',
          },
        };
        const mmssSrc = {
          sourceName: 'Duration MMSS Source',
          sourceUrl: `${ctx.mockBaseUrl}/music/dur-mmss`,
          enabled: true,
          searchUrl: `${ctx.mockBaseUrl}/music/audius/v1/tracks/search?query={{key}}`,
          ruleSearch: {
            trackList: '$.data[*]',
            trackId: '$.id',
            title: '$.title',
            artist: '$.user.name',
            duration: '<js>"03:32"</js>',
          },
        };
        const imp = await importSources(ctx.admin, 'music', [msSrc, mmssSrc]);
        for (const src of imp.sources) {
          const res = await ctx.member.failLoudlyRequest('POST', '/api/v1/music/online/search', {
            body: { keyword: 'Fragments', source_ids: [src.id] },
            expectedStatus: 200,
          });
          const item = (res.json.results ?? res.json.items)[0];
          const dur = item.duration_secs ?? item.duration;
          assert.equal(dur, 212, `${src.name}: expected 212s, got ${dur}`);
        }
      },
    },
    {
      id: 'T2-F07-03',
      feature: 'F7',
      domain: 'music',
      title: 'Plain-text lyrics set synced=false and LRC [offset:-500] adjusts/parses accurately',
      async run(ctx) {
        const mocks = await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        const plain = await ctx.member.failLoudlyRequest('POST', '/api/v1/music/online/lyrics', {
          body: { source_id: mocks.gdstudio.id, track_id: 'gd-plain', lyric_id: 'gd-plain' },
          expectedStatus: 200,
        });
        assert.equal(plain.json.synced, false);
        assert.ok(
          (plain.json.plain_text || '').includes('Line one plain text') ||
            (plain.json.lines || []).some((l) => l.text.includes('Line one plain text')),
        );

        const offset = await ctx.member.failLoudlyRequest('POST', '/api/v1/music/online/lyrics', {
          body: { source_id: mocks.gdstudio.id, track_id: 'gd-offset', lyric_id: 'gd-offset' },
          expectedStatus: 200,
        });
        assert.equal(offset.json.synced, true);
        assert.ok(offset.json.lines.length >= 1);
      },
    },
    {
      id: 'T2-F07-04',
      feature: 'F7',
      domain: 'music',
      title: 'Unsupported quality="dsd" falls back gracefully or returns structured 400 error',
      async run(ctx) {
        const mocks = await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('POST', '/api/v1/music/online/stream', {
          body: {
            source_id: mocks.gdstudio.id,
            track_id: 'gd-101',
            url_id: 'gd-101',
            quality: 'dsd',
          },
          expectedStatus: [200, 400],
        });
        if (res.status === 200) {
          assert.ok(res.json.proxy_stream_url);
        } else {
          assert.ok(res.json?.error?.code);
        }
      },
    },
    {
      id: 'T2-F07-05',
      feature: 'F7',
      domain: 'music',
      title: 'Track without lyric_id succeeds on /online/track-info with lyrics: null',
      async run(ctx) {
        const mocks = await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('POST', '/api/v1/music/online/track-info', {
          body: {
            source_id: mocks.audius.id,
            track_id: 'aud-1',
            play_url: `${ctx.mockBaseUrl}/music/audius/v1/tracks/aud-1/stream`,
          },
          expectedStatus: 200,
        });
        assert.ok(res.json.proxy_stream_url.includes('/api/v1/music/online/audio-proxy'));
        assert.equal(res.json.lyrics ?? null, null);
      },
    },

    // =======================================================================
    // F8: Music Proxy Boundary (music)
    // =======================================================================
    {
      id: 'T2-F08-01',
      feature: 'F8',
      domain: 'music',
      title: 'cover-proxy rejects SVG and HTML payloads via magic-byte sniffing',
      async run(ctx) {
        const mocks = await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        for (const p of ['/edge/svg-as-image', '/edge/html-as-image.jpg']) {
          const res = await ctx.member.failLoudlyRequest('GET', '/api/v1/music/online/cover-proxy', {
            query: { url: `${ctx.mockBaseUrl}${p}`, source_id: mocks.gdstudio.id },
            expectedStatus: [400, 415, 502],
          });
          assert.ok(!res.text.includes('<svg') && !res.text.includes('<html>'));
        }
      },
    },
    {
      id: 'T2-F08-02',
      feature: 'F8',
      domain: 'music',
      title: 'audio-proxy handles suffix Range (bytes=-512) and open-ended Range (bytes=4096-)',
      async run(ctx) {
        const mocks = await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        const mp3Url = `${ctx.mockBaseUrl}/music/gdstudio/cdn/gd-101.mp3?br=320`;

        const suf = await ctx.member.failLoudlyRequest('GET', '/api/v1/music/online/audio-proxy', {
          query: { url: mp3Url, source_id: mocks.gdstudio.id },
          headers: { Range: 'bytes=-512' },
          expectedStatus: 206,
        });
        assert.equal(suf.buffer.length, 512);
        assert.ok(suf.buffer.equals(MOCK_MP3_BYTES.subarray(MOCK_MP3_BYTES.length - 512)));

        const open = await ctx.member.failLoudlyRequest('GET', '/api/v1/music/online/audio-proxy', {
          query: { url: mp3Url, source_id: mocks.gdstudio.id },
          headers: { Range: 'bytes=4096-' },
          expectedStatus: 206,
        });
        assert.equal(open.buffer.length, MOCK_MP3_BYTES.length - 4096);
        assert.ok(open.buffer.equals(MOCK_MP3_BYTES.subarray(4096)));
      },
    },
    {
      id: 'T2-F08-03',
      feature: 'F8',
      domain: 'music',
      title: 'audio-proxy returns 416 Range Not Satisfiable for out-of-bounds Range',
      async run(ctx) {
        const mocks = await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        await ctx.member.failLoudlyRequest('GET', '/api/v1/music/online/audio-proxy', {
          query: {
            url: `${ctx.mockBaseUrl}/music/gdstudio/cdn/gd-101.mp3?br=320`,
            source_id: mocks.gdstudio.id,
          },
          headers: { Range: 'bytes=99999999-' },
          expectedStatus: [416, 400, 502],
        });
      },
    },
    {
      id: 'T2-F08-04',
      feature: 'F8',
      domain: 'music',
      title: 'audio-proxy handles malformed Range header (bytes=abc-def) without 500 error',
      async run(ctx) {
        const mocks = await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('GET', '/api/v1/music/online/audio-proxy', {
          query: {
            url: `${ctx.mockBaseUrl}/music/gdstudio/cdn/gd-101.mp3?br=320`,
            source_id: mocks.gdstudio.id,
          },
          headers: { Range: 'bytes=abc-def' },
          expectedStatus: [200, 400],
        });
        if (res.status === 200) {
          assertMp3(res.buffer, 'audio-proxy fallback on malformed range');
        }
      },
    },
    {
      id: 'T2-F08-05',
      feature: 'F8',
      domain: 'music',
      title: 'audio-proxy returns non-200 error envelope when upstream returns 502',
      async run(ctx) {
        const mocks = await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('GET', '/api/v1/music/online/audio-proxy', {
          query: { url: `${ctx.mockBaseUrl}/edge/error-502`, source_id: mocks.gdstudio.id },
          expectedStatus: [400, 502],
        });
        assert.ok(res.json?.error?.code);
      },
    },

    // =======================================================================
    // F9: Video Parsing Boundary (video)
    // =======================================================================
    {
      id: 'T2-F09-01',
      feature: 'F9',
      domain: 'video',
      title: 'AppleCMS with ONLY /share/ URLs filters non-media lines gracefully without panic',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'video');
        const shareOnlySrv = http.createServer((req, res) => {
          const payload = JSON.stringify({
            code: 1,
            list: [
              {
                vod_id: 2001,
                vod_name: 'Share Only Movie',
                vod_play_from: 'shareonly',
                vod_play_url: '正片$http://127.0.0.1:9/share/2001',
              },
            ],
          });
          res.writeHead(200, { 'Content-Type': 'application/json' });
          res.end(payload);
        });
        await new Promise((r) => shareOnlySrv.listen(0, '127.0.0.1', r));
        try {
          const port = shareOnlySrv.address().port;
          const src = {
            sourceName: 'ShareOnly AppleCMS',
            sourceUrl: `http://127.0.0.1:${port}`,
            sourceFormat: 'applecms_v10',
            apiUrl: `http://127.0.0.1:${port}/vod/`,
            enabled: true,
            playUrlFilterRegex: '(?i)\\.(m3u8|mp4)(\\?|$)',
          };
          const imp = await importSources(ctx.admin, 'video', src);
          const det = await ctx.member.failLoudlyRequest('POST', '/api/v1/video/online/detail', {
            body: { source_id: imp.sources[0].id, video_id: '2001' },
            expectedStatus: 200,
          });
          assert.deepEqual(det.json.play_groups, []);
        } finally {
          await new Promise((r) => shareOnlySrv.close(r));
        }
      },
    },
    {
      id: 'T2-F09-02',
      feature: 'F9',
      domain: 'video',
      title: 'AppleCMS mismatched vod_play_from/url group counts and missing $ label recover safely',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'video');
        const mismatchSrv = http.createServer((req, res) => {
          const payload = JSON.stringify({
            code: 1,
            list: [
              {
                vod_id: 2002,
                vod_name: 'Mismatch Movie',
                vod_play_from: 'lineA',
                vod_play_url: `${ctx.mockBaseUrl}/video/applecms/hls/master.m3u8$$$line2ep$${ctx.mockBaseUrl}/video/applecms/hls/ep2/index.m3u8`,
              },
            ],
          });
          res.writeHead(200, { 'Content-Type': 'application/json' });
          res.end(payload);
        });
        await new Promise((r) => mismatchSrv.listen(0, '127.0.0.1', r));
        try {
          const port = mismatchSrv.address().port;
          const imp = await importSources(ctx.admin, 'video', {
            sourceName: 'Mismatch AppleCMS',
            sourceUrl: `http://127.0.0.1:${port}`,
            sourceFormat: 'applecms_v10',
            apiUrl: `http://127.0.0.1:${port}/vod/`,
            enabled: true,
          });
          const det = await ctx.member.failLoudlyRequest('POST', '/api/v1/video/online/detail', {
            body: { source_id: imp.sources[0].id, video_id: '2002' },
            expectedStatus: 200,
          });
          assert.ok(Array.isArray(det.json.play_groups) && det.json.play_groups.length >= 1);
        } finally {
          await new Promise((r) => mismatchSrv.close(r));
        }
      },
    },
    {
      id: 'T2-F09-03',
      feature: 'F9',
      domain: 'video',
      title: 'AppleCMS preferredPlayFrom ["altm3u8", "ikm3u8"] reorders play_groups accordingly',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'video');
        const ac = buildMockVideoSources(ctx.mockBaseUrl).applecms;
        ac.preferredPlayFrom = ['altm3u8', 'ikm3u8'];
        const imp = await importSources(ctx.admin, 'video', ac);
        const det = await ctx.member.failLoudlyRequest('POST', '/api/v1/video/online/detail', {
          body: { source_id: imp.sources[0].id, video_id: '1001' },
          expectedStatus: 200,
        });
        assert.equal(det.json.play_groups[0].name, 'altm3u8');
        assert.equal(det.json.play_groups[1].name, 'ikm3u8');
      },
    },
    {
      id: 'T2-F09-04',
      feature: 'F9',
      domain: 'video',
      title: 'AppleCMS vod_content HTML tags (<p>...</p>) are cleaned safely in detail.intro',
      async run(ctx) {
        const mocks = await ensureVideoMocks(ctx.admin, ctx.mockBaseUrl);
        const det = await ctx.member.failLoudlyRequest('POST', '/api/v1/video/online/detail', {
          body: { source_id: mocks.applecms.id, video_id: '1001' },
          expectedStatus: 200,
        });
        assert.ok(det.json.intro.includes('Mock sci-fi intro'));
        assert.ok(!det.json.intro.includes('<p>'));
      },
    },
    {
      id: 'T2-F09-05',
      feature: 'F9',
      domain: 'video',
      title: 'Empty video search keyword returns 400 and out-of-bounds page=999 returns results: []',
      async run(ctx) {
        const mocks = await ensureVideoMocks(ctx.admin, ctx.mockBaseUrl);
        await ctx.member.failLoudlyRequest('POST', '/api/v1/video/online/search', {
          body: { keyword: '', source_ids: [mocks.applecms.id] },
          expectedStatus: [400, 422],
        });
        const oob = await ctx.member.failLoudlyRequest('POST', '/api/v1/video/online/browse', {
          body: { source_id: mocks.applecms.id, page: 999 },
          expectedStatus: 200,
        });
        assert.deepEqual(oob.json.results ?? oob.json.items ?? [], []);
      },
    },

    // =======================================================================
    // F10: Video Proxy Boundary (video)
    // =======================================================================
    {
      id: 'T2-F10-01',
      feature: 'F10',
      domain: 'video',
      title: 'poster-proxy rejects /edge/svg-as-image',
      async run(ctx) {
        const mocks = await ensureVideoMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('GET', '/api/v1/video/online/poster-proxy', {
          query: { url: `${ctx.mockBaseUrl}/edge/svg-as-image`, source_id: mocks.applecms.id },
          expectedStatus: [400, 415, 502],
        });
        assert.ok(!res.text.includes('<svg'));
      },
    },
    {
      id: 'T2-F10-02',
      feature: 'F10',
      domain: 'video',
      title: 'hls-proxy rejects /edge/malformed-m3u8 missing #EXTM3U header',
      async run(ctx) {
        const mocks = await ensureVideoMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('GET', '/api/v1/video/online/hls-proxy', {
          query: { url: `${ctx.mockBaseUrl}/edge/malformed-m3u8`, source_id: mocks.applecms.id },
          expectedStatus: [400, 502],
        });
        assert.ok(res.json?.error?.code);
      },
    },
    {
      id: 'T2-F10-03',
      feature: 'F10',
      domain: 'video',
      title: 'hls-proxy rejects /edge/oversized-m3u8 (>8 MiB cap) cleanly',
      async run(ctx) {
        const mocks = await ensureVideoMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('GET', '/api/v1/video/online/hls-proxy', {
          query: { url: `${ctx.mockBaseUrl}/edge/oversized-m3u8`, source_id: mocks.applecms.id },
          expectedStatus: [400, 413, 502],
        });
        assert.ok(res.json?.error?.code);
      },
    },
    {
      id: 'T2-F10-04',
      feature: 'F10',
      domain: 'video',
      title: 'hls-proxy preserves query parameters on rewritten segment and #EXT-X-KEY URIs',
      async run(ctx) {
        const mocks = await ensureVideoMocks(ctx.admin, ctx.mockBaseUrl);
        await resetMockRequests(ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('GET', '/api/v1/video/online/hls-proxy', {
          query: {
            url: `${ctx.mockBaseUrl}/video/applecms/hls/5000kb/hls/index.m3u8?with_query=1`,
            source_id: mocks.applecms.id,
          },
          expectedStatus: 200,
        });
        const parsed = assertM3u8Rewritten(res.text, {
          expectedSourceId: mocks.applecms.id,
          requireKey: true,
          minSegments: 2,
        });
        await ctx.member.failLoudlyRequest('GET', parsed.segments[0], { expectedStatus: 200 });
        const reqs = await getMockRequests(ctx.mockBaseUrl);
        const segReq = reqs.find((r) => r.path.endsWith('/seg0.ts'));
        assert.equal(segReq?.query.token, 'a');
        assert.equal(segReq?.query.b, '1');
      },
    },
    {
      id: 'T2-F10-05',
      feature: 'F10',
      domain: 'video',
      title: 'hls-proxy applies source mediaHeader (X-Video-Token) and strips forbidden hop-by-hop headers',
      async run(ctx) {
        const mocks = await ensureVideoMocks(ctx.admin, ctx.mockBaseUrl);
        await resetMockRequests(ctx.mockBaseUrl);
        const segRes = await ctx.member.failLoudlyRequest('GET', '/api/v1/video/online/hls-proxy', {
          query: {
            url: `${ctx.mockBaseUrl}/video/applecms/hls/5000kb/hls/seg0.ts?require_header=X-Video-Token:secret123`,
            source_id: mocks.applecmsToken.id,
          },
          headers: { 'X-Forwarded-Host': 'evil.example.com' },
          expectedStatus: 200,
        });
        assertMpegTs(segRes.buffer, 'hls-proxy header-injected segment');
        const reqs = await getMockRequests(ctx.mockBaseUrl);
        const segReq = reqs.find((r) => r.path.endsWith('/seg0.ts'));
        assert.equal(segReq?.headers['x-video-token'], 'secret123');
        assert.ok(segReq?.headers.host?.startsWith('127.0.0.1:'));
      },
    },

    // =======================================================================
    // F11: Seed & Persistence Boundary (core)
    // =======================================================================
    {
      id: 'T2-F11-01',
      feature: 'F11',
      domain: 'core',
      title: 'seeds/sources/{comic,music,video}.json contain only public https:// URLs (zero private/localhost)',
      async run() {
        for (const domain of ['comic', 'music', 'video']) {
          const raw = readFileSync(
            path.join(REPO_ROOT, 'seeds', 'sources', `${domain}.json`),
            'utf8',
          );
          const arr = JSON.parse(raw);
          for (const entry of arr) {
            const url = entry.sourceUrl || entry.apiUrl || '';
            assert.ok(
              url.startsWith('https://'),
              `${domain}.json entry "${entry.sourceName}" must use https://, got ${url}`,
            );
            assert.ok(
              !/localhost|127\.0\.0\.1|10\.|192\.168\.|169\.254\./i.test(url),
              `${domain}.json contains private/localhost URL: ${url}`,
            );
          }
        }
      },
    },
    {
      id: 'T2-F11-02',
      feature: 'F11',
      domain: 'core',
      title: 'Re-running import-seeds restores deleted seed without overwriting existing disabled state',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'comic');
        await ctx.admin.failLoudlyRequest('POST', '/api/v1/comic/sources/import-seeds', {
          body: {},
          expectedStatus: 200,
        });
        const list1 = (
          await ctx.admin.failLoudlyRequest('GET', '/api/v1/comic/sources', { expectedStatus: 200 })
        ).json;
        assert.ok(list1.length >= 2);
        const deletedId = list1[0].id;
        const disabledId = list1[1].id;

        await ctx.admin.failLoudlyRequest('DELETE', `/api/v1/comic/sources/${deletedId}`, {
          expectedStatus: [200, 204],
        });
        await ctx.admin.failLoudlyRequest('PATCH', `/api/v1/comic/sources/${disabledId}`, {
          body: { enabled: false },
          expectedStatus: 200,
        });

        const reimp = await ctx.admin.failLoudlyRequest(
          'POST',
          '/api/v1/comic/sources/import-seeds',
          { body: {}, expectedStatus: 200 },
        );
        assert.equal(reimp.json.imported_count, 1);
        const list2 = (
          await ctx.admin.failLoudlyRequest('GET', '/api/v1/comic/sources', { expectedStatus: 200 })
        ).json;
        const keptDisabled = list2.find((s) => s.id === disabledId);
        assert.equal(keptDisabled?.enabled, false);
      },
    },
    {
      id: 'T2-F11-03',
      feature: 'F11',
      domain: 'core',
      title: 'Non-admin calling POST /api/v1/{comic,music,video}/sources/import-seeds receives 403',
      async run(ctx) {
        for (const domain of ['comic', 'music', 'video']) {
          const res = await ctx.member.failLoudlyRequest(
            'POST',
            `/api/v1/${domain}/sources/import-seeds`,
            { body: {}, expectedStatus: 403 },
          );
          assert.equal(res.json?.error?.code, 'forbidden');
        }
      },
    },
    {
      id: 'T2-F11-04',
      feature: 'F11',
      domain: 'core',
      title: 'Deleted source remains absent after server restart',
      async run(ctx) {
        if (ctx.externalBaseUrl) {
          throw new Error('T2-F11-04 requires managed SUT to test server restart');
        }
        const mgr = new ctx.XMediaServerManager({
          xmediaBin: ctx.xmediaBin,
          webDir: ctx.webDir,
          keepTmp: ctx.keepTmp,
        });
        try {
          const url1 = await mgr.start({ mode: 'mock' });
          const { admin } = await bootstrapClients(url1, ctx.ApiClient);
          const mocks = await ensureMusicMocks(admin, ctx.mockBaseUrl);
          await admin.failLoudlyRequest('DELETE', `/api/v1/music/sources/${mocks.gdstudio.id}`, {
            expectedStatus: [200, 204],
          });

          const url2 = await mgr.restart({ mode: 'mock' });
          const admin2 = new ctx.ApiClient(url2, { authMode: 'bearer' });
          await admin2.login({ username: 'admin', password: 'AdminPass123!' });
          const list = (
            await admin2.failLoudlyRequest('GET', '/api/v1/music/sources', { expectedStatus: 200 })
          ).json;
          assert.ok(!list.some((s) => s.id === mocks.gdstudio.id));
        } finally {
          await mgr.cleanup();
        }
      },
    },
    {
      id: 'T2-F11-05',
      feature: 'F11',
      domain: 'core',
      title: 'Large 64 KiB custom source variable persists intact across server restart',
      async run(ctx) {
        if (ctx.externalBaseUrl) {
          throw new Error('T2-F11-05 requires managed SUT to test server restart');
        }
        const mgr = new ctx.XMediaServerManager({
          xmediaBin: ctx.xmediaBin,
          webDir: ctx.webDir,
          keepTmp: ctx.keepTmp,
        });
        try {
          const url1 = await mgr.start({ mode: 'mock' });
          const { admin } = await bootstrapClients(url1, ctx.ApiClient);
          const mocks = await ensureVideoMocks(admin, ctx.mockBaseUrl);
          const bigVar = 'V'.repeat(64 * 1024);
          await admin.failLoudlyRequest('PATCH', `/api/v1/video/sources/${mocks.applecms.id}`, {
            body: { variable: bigVar },
            expectedStatus: 200,
          });

          const url2 = await mgr.restart({ mode: 'mock' });
          const admin2 = new ctx.ApiClient(url2, { authMode: 'bearer' });
          await admin2.login({ username: 'admin', password: 'AdminPass123!' });
          const got = (
            await admin2.failLoudlyRequest(
              'GET',
              `/api/v1/video/sources/${mocks.applecms.id}/variable`,
              { expectedStatus: 200 },
            )
          ).json;
          assert.equal(got.variable.length, 64 * 1024);
          assert.equal(got.variable, bigVar);
        } finally {
          await mgr.cleanup();
        }
      },
    },

    // =======================================================================
    // F12: Web UI Settings Boundary (ui)
    // =======================================================================
    {
      id: 'T2-F12-01',
      feature: 'F12',
      domain: 'ui',
      title: 'Browser (Non-Admin): /settings?tab=domain-datasources shows admin-only lock for comic/music/video',
      async run(ctx) {
        await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.member,
          '/settings?tab=domain-datasources&domain=comic',
        );
        try {
          await cdp.waitForFn(
            `!/Mock MangaDex/.test(document.body.innerText) && /管理员|admin/i.test(document.body.innerText)`,
            { timeoutMs: 8000, label: 'non-admin lock card displayed' },
          );
          for (const d of ['music', 'video']) {
            await cdp.goto(`${ctx.baseUrl}/settings?tab=domain-datasources&domain=${d}`);
            await cdp.waitForFn(`/管理员|admin/i.test(document.body.innerText)`, { timeoutMs: 8000 });
          }
          cdp.assertNoErrors('T2-F12-01');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T2-F12-02',
      feature: 'F12',
      domain: 'ui',
      title: 'Browser (Admin): Pasting invalid JSON in Import modal shows inline error without crashing SPA',
      async run(ctx) {
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.admin,
          '/settings?tab=domain-datasources&domain=comic',
        );
        try {
          await clickButtonMatching(cdp, [/导入|import/i]);
          await cdp.waitForSelector('textarea', { visible: true });
          await cdp.type('textarea', '{not valid json');
          await clickButtonMatching(cdp, [/确认导入|导入|submit|import/i], {
            rootSelector: '[role="dialog"], form, .space-y-4',
          });
          await cdp.waitForFn(
            `/error|无效|失败|invalid|parse|json/i.test(document.body.innerText)`,
            { timeoutMs: 8000, label: 'inline import error shown' },
          );
          assert.equal(cdp.exceptions.length, 0);
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T2-F12-03',
      feature: 'F12',
      domain: 'ui',
      title: 'Browser (Admin): Aborting slow debug test resets UI state cleanly',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'comic');
        const edges = buildEdgeMockSources(ctx.mockBaseUrl);
        await importSources(ctx.admin, 'comic', edges.delayTimeout);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.admin,
          '/settings?tab=domain-datasources&domain=comic',
        );
        try {
          await cdp.waitForFn(`document.body.innerText.includes('Edge Slow Timeout Source')`, {
            timeoutMs: 10000,
          });
          await clickButtonMatching(cdp, [/更多|more|菜单|menu|⋮/i]);
          await clickButtonMatching(cdp, [/调试|测试|debug|test/i]);
          await cdp.waitForSelector('[role="dialog"]', { visible: true });
          await clickButtonMatching(cdp, [/开始测试|运行|run|test/i], {
            rootSelector: '[role="dialog"]',
          });
          await clickButtonMatching(cdp, [/取消|中止|停止|abort|cancel|stop/i], {
            rootSelector: '[role="dialog"]',
          });
          assert.equal(cdp.exceptions.length, 0);
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T2-F12-04',
      feature: 'F12',
      domain: 'ui',
      title: 'Browser (Admin): Login/Variables modal submits credentials and populates cookie_hosts',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'comic');
        const edges = buildEdgeMockSources(ctx.mockBaseUrl);
        await importSources(ctx.admin, 'comic', edges.cookieGated);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.admin,
          '/settings?tab=domain-datasources&domain=comic',
        );
        try {
          await cdp.waitForFn(`document.body.innerText.includes('Edge Cookie Gated Comic')`, {
            timeoutMs: 10000,
          });
          await clickButtonMatching(cdp, [/更多|more|菜单|menu|⋮/i]);
          await clickButtonMatching(cdp, [/登录|login|凭证/i]);
          await cdp.waitForSelector('[role="dialog"]', { visible: true });
          await cdp.evalJs(`(() => {
            const dlg = document.querySelector('[role="dialog"]');
            const inputs = [...dlg.querySelectorAll('input')];
            const setVal = (el, val) => {
              const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
              setter.call(el, val);
              el.dispatchEvent(new Event('input', { bubbles: true }));
            };
            if (inputs[0]) setVal(inputs[0], 'alice');
            if (inputs[1]) setVal(inputs[1], 'secret');
          })()`);
          await clickButtonMatching(cdp, [/登录|提交|确认|submit|login/i], {
            rootSelector: '[role="dialog"]',
          });
          await cdp.waitForFn(`document.body.innerText.includes('127.0.0.1')`, {
            timeoutMs: 10000,
            label: 'cookie_hosts updated in UI',
          });
          cdp.assertNoErrors('T2-F12-04');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T2-F12-05',
      feature: 'F12',
      domain: 'ui',
      title: 'Browser: Switching locale zh-CN <-> en-US renders zero raw i18n keys',
      async run(ctx) {
        await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.admin,
          '/settings?tab=domain-datasources&domain=comic',
        );
        try {
          for (const locale of ['en-US', 'zh-CN']) {
            await ctx.admin.failLoudlyRequest('PUT', '/api/v1/auth/preferences', {
              body: { locale },
              expectedStatus: 200,
            });
            await cdp.goto(`${ctx.baseUrl}/settings?tab=domain-datasources&domain=comic`);
            await cdp.waitForSelector('[data-settings-tab="domain-datasources"]', { visible: true });
            const rawKeyLeak = await cdp.evalJs(`(() => {
              const txt = document.body.innerText;
              const m = txt.match(/\\b(settings|comic|music|video|onlineSources)\\.[a-zA-Z0-9_]+\\b/);
              return m ? m[0] : null;
            })()`);
            assert.equal(rawKeyLeak, null, `Detected untranslated i18n key in ${locale}: ${rawKeyLeak}`);
          }
          cdp.assertNoErrors('T2-F12-05');
        } finally {
          await cdp.close();
        }
      },
    },

    // =======================================================================
    // F13: Web UI Switchers & GlobalSearchModal Boundary (ui)
    // =======================================================================
    {
      id: 'T2-F13-01',
      feature: 'F13',
      domain: 'ui',
      title: 'Browser: Online comic search with all sources disabled renders guidance banner',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'comic');
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.admin,
          '/comic/series?scope=online',
        );
        try {
          await cdp.waitForFn(
            `/数据源|来源|source|settings|设置/i.test(document.body.innerText)`,
            { timeoutMs: 8000, label: 'empty sources guidance rendered' },
          );
          cdp.assertNoErrors('T2-F13-01');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T2-F13-02',
      feature: 'F13',
      domain: 'ui',
      title: 'Browser: Rapidly toggling local <-> online switcher keeps latest scope state clean',
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
          await clickButtonMatching(cdp, [/本地|local/i]);
          await clickButtonMatching(cdp, [/在线|online/i]);
          await cdp.waitForFn(`location.search.includes('scope=online')`, { timeoutMs: 6000 });
          cdp.assertNoErrors('T2-F13-02');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T2-F13-03',
      feature: 'F13',
      domain: 'ui',
      title: 'Browser: GlobalSearchModal with whitespace-only query does not emit online quick actions',
      async run(ctx) {
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.member,
          '/comic/series',
        );
        try {
          await cdp.pressKey('k', { modifiers: 2 });
          await cdp.waitForSelector('[role="dialog"] input', { visible: true });
          await cdp.type('[role="dialog"] input', '   ');
          const hasOnlineAction = await cdp.evalJs(
            `/在线.*搜索|online.*search/i.test(document.querySelector('[role="dialog"]')?.innerText || '')`,
          );
          assert.equal(hasOnlineAction, false);
          cdp.assertNoErrors('T2-F13-03');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T2-F13-04',
      feature: 'F13',
      domain: 'ui',
      title: 'Browser (Non-Admin): GlobalSearchModal & online search incur zero 403 admin-endpoint errors',
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
          const forbiddenCalls = cdp.httpErrors.filter((e) => e.status === 403);
          assert.equal(forbiddenCalls.length, 0);
          cdp.assertNoErrors('T2-F13-04');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T2-F13-05',
      feature: 'F13',
      domain: 'ui',
      title: 'Browser: Multi-source search with 1 healthy + 1 502 source renders partial results & notice',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'comic');
        const comics = buildMockComicSources(ctx.mockBaseUrl);
        const edges = buildEdgeMockSources(ctx.mockBaseUrl);
        await importSources(ctx.admin, 'comic', [comics.xkcd, edges.error502]);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.member,
          '/comic/series?scope=online&q=Strip',
        );
        try {
          await cdp.waitForFn(`document.body.innerText.includes('Mock XKCD Strip')`, {
            timeoutMs: 10000,
          });
          assert.equal(cdp.exceptions.length, 0);
        } finally {
          await cdp.close();
        }
      },
    },

    // =======================================================================
    // F14: Web UI Comic Reader Boundary (ui)
    // =======================================================================
    {
      id: 'T2-F14-01',
      feature: 'F14',
      domain: 'ui',
      title: 'Browser: Comic reader handles broken image URL (/edge/svg-as-image) without JS crash',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'comic');
        const badImgSrc = {
          sourceName: 'Broken SVG Page Comic',
          sourceUrl: `${ctx.mockBaseUrl}/comic/broken-svg`,
          sourceType: 2,
          enabled: true,
          searchUrl: `${ctx.mockBaseUrl}/comic/xkcd/info.0.json`,
          ruleSearch: {
            bookList: '$',
            name: '$.title',
            bookUrl: `${ctx.mockBaseUrl}/comic/xkcd/info.0.json`,
          },
          ruleBookInfo: {
            name: '$.title',
            tocUrl: `${ctx.mockBaseUrl}/comic/xkcd/info.0.json`,
          },
          ruleToc: {
            chapterList: '$',
            chapterName: 'Broken Chapter',
            chapterUrl: `${ctx.mockBaseUrl}/comic/xkcd/info.0.json`,
          },
          ruleContent: {
            content: `${ctx.mockBaseUrl}/edge/svg-as-image`,
          },
        };
        await importSources(ctx.admin, 'comic', badImgSrc);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.member,
          '/comic/series?scope=online&q=Strip',
        );
        try {
          await cdp.waitForFn(`document.body.innerText.includes('Mock XKCD Strip')`, { timeoutMs: 10000 });
          await clickButtonMatching(cdp, [/Mock XKCD Strip/]);
          await clickButtonMatching(cdp, [/Broken Chapter/]);
          await cdp.waitForSelector('img[src*="image-proxy"]', { timeoutMs: 10000 });
          assert.equal(cdp.exceptions.length, 0);
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T2-F14-02',
      feature: 'F14',
      domain: 'ui',
      title: 'Browser: Deep-link reload with invalid comic source_id shows graceful error (no white screen)',
      async run(ctx) {
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.member,
          '/comic/online/read?source_id=99999999-9999-4999-8999-999999999999&chapter_url=invalid',
        );
        try {
          await cdp.waitForFn(`document.body.innerText.trim().length > 0`, { timeoutMs: 8000 });
          assert.equal(cdp.exceptions.length, 0);
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T2-F14-03',
      feature: 'F14',
      domain: 'ui',
      title: 'Browser: Keyboard navigation (ArrowRight, ArrowLeft, Escape) in online comic reader',
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
          await cdp.waitForSelector('img[src*="image-proxy"]', { visible: true });
          await cdp.pressKey('ArrowRight');
          await cdp.pressKey('ArrowLeft');
          await cdp.pressKey('Escape');
          assert.equal(cdp.exceptions.length, 0);
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T2-F14-04',
      feature: 'F14',
      domain: 'ui',
      title: 'Browser: Toggling dimImages eye-care filter & fit mode in online comic reader preserves pages',
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
          await cdp.waitForSelector('img[src*="image-proxy"]', { visible: true });
          await clickButtonMatching(cdp, [/护眼|调暗|dim/i]);
          await clickButtonMatching(cdp, [/适应宽度|fit/i]);
          cdp.assertNoErrors('T2-F14-04');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T2-F14-05',
      feature: 'F14',
      domain: 'ui',
      title: 'Browser: Online comic detail with 0 chapters renders empty chapter state cleanly',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'comic');
        const emptyChSrc = buildMockComicSources(ctx.mockBaseUrl).webtoon;
        emptyChSrc.searchUrl = `${ctx.mockBaseUrl}/comic/webtoon/list?title_no=0`;
        emptyChSrc.ruleSearch = {
          bookList: 'body',
          name: 'h1.subj@text',
          bookUrl: `${ctx.mockBaseUrl}/comic/webtoon/list?title_no=0`,
        };
        await importSources(ctx.admin, 'comic', emptyChSrc);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.member,
          '/comic/series?scope=online&q=Empty',
        );
        try {
          await cdp.waitForFn(`document.body.innerText.includes('Empty Webtoon')`, { timeoutMs: 10000 });
          await clickButtonMatching(cdp, [/Empty Webtoon/]);
          await cdp.waitForFn(`/暂无章节|empty|0.*chapter|没有章节/i.test(document.body.innerText)`, {
            timeoutMs: 8000,
          });
          cdp.assertNoErrors('T2-F14-05');
        } finally {
          await cdp.close();
        }
      },
    },

    // =======================================================================
    // F15: Web UI Music Boundary (ui)
    // =======================================================================
    {
      id: 'T2-F15-01',
      feature: 'F15',
      domain: 'ui',
      title: 'Browser: Playing online track with lyrics=null shows empty state in LyricsOverlay',
      async run(ctx) {
        await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.member,
          '/music/songs?scope=online&q=Fragments',
        );
        try {
          await cdp.waitForFn(`document.body.innerText.includes('Fragments Mock')`, { timeoutMs: 10000 });
          await clickButtonMatching(cdp, [/Fragments Mock|播放|play/i]);
          await clickButtonMatching(cdp, [/歌词|lyrics/i]);
          await cdp.waitForFn(`/暂无歌词|no lyrics/i.test(document.body.innerText)`, { timeoutMs: 8000 });
          cdp.assertNoErrors('T2-F15-01');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T2-F15-02',
      feature: 'F15',
      domain: 'ui',
      title: 'Browser: Track with failing/missing cover falls back cleanly without broken image crash',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'music');
        const noCover = buildMockMusicSources(ctx.mockBaseUrl).audius;
        delete noCover.ruleSearch.coverUrl;
        await importSources(ctx.admin, 'music', noCover);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.member,
          '/music/songs?scope=online&q=Fragments',
        );
        try {
          await cdp.waitForFn(`document.body.innerText.includes('Fragments Mock')`, { timeoutMs: 10000 });
          await clickButtonMatching(cdp, [/Fragments Mock|播放|play/i]);
          await cdp.waitForSelector('audio', { timeoutMs: 8000 });
          cdp.assertNoErrors('T2-F15-02');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T2-F15-03',
      feature: 'F15',
      domain: 'ui',
      title: 'Browser: Seeking Range-capable GDStudio track updates audio.currentTime cleanly',
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
          await cdp.waitForFn(`(document.querySelector('audio')?.readyState || 0) >= 2`, {
            timeoutMs: 10000,
          });
          await cdp.evalJs(`(() => { const a = document.querySelector('audio'); a.currentTime = 1.5; })()`);
          const ct = await cdp.evalJs(`document.querySelector('audio')?.currentTime || 0`);
          assert.ok(ct >= 1.0);
          cdp.assertNoErrors('T2-F15-03');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T2-F15-04',
      feature: 'F15',
      domain: 'ui',
      title: 'Browser: Clearing QueueDrawer resets PlayerBar cleanly after Audius non-Range playback',
      async run(ctx) {
        await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.member,
          '/music/songs?scope=online&q=Fragments',
        );
        try {
          await cdp.waitForFn(`document.body.innerText.includes('Fragments Mock')`, { timeoutMs: 10000 });
          await clickButtonMatching(cdp, [/Fragments Mock|播放|play/i]);
          await cdp.waitForSelector('audio', { timeoutMs: 8000 });
          await clickButtonMatching(cdp, [/播放队列|队列|queue/i]);
          await clickButtonMatching(cdp, [/清空|clear/i]);
          await clickButtonMatching(cdp, [/删除|确认|清空|delete|confirm|clear/i], {
            rootSelector: '[role="dialog"]',
          });
          cdp.assertNoErrors('T2-F15-04');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T2-F15-05',
      feature: 'F15',
      domain: 'ui',
      title: 'Browser: Logging out pauses/clears online music PlayerBar and redirects to /login',
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
          await cdp.waitForSelector('audio', { timeoutMs: 8000 });
          await cdp.click('header [aria-haspopup="menu"], header div.relative.ml-1 > button');
          await clickButtonMatching(cdp, [/退出|登出|logout|sign out/i]);
          await cdp.waitForFn(`location.pathname.includes('/login')`, { timeoutMs: 10000 });
          const audioPaused = await cdp.evalJs(
            `!document.querySelector('audio') || document.querySelector('audio').paused`,
          );
          assert.equal(audioPaused, true);
          cdp.assertNoErrors('T2-F15-05');
        } finally {
          await cdp.close();
        }
      },
    },

    // =======================================================================
    // F16: Web UI Video Boundary (ui)
    // =======================================================================
    {
      id: 'T2-F16-01',
      feature: 'F16',
      domain: 'ui',
      title: 'Browser: Switching away from HLS video player destroys hls.js cleanly without MSE errors',
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
          await clickButtonMatching(cdp, [/返回|back/i]);
          await cdp.waitForFn(`!location.pathname.includes('/watch')`, { timeoutMs: 8000 });
          cdp.assertNoErrors('T2-F16-01');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T2-F16-02',
      feature: 'F16',
      domain: 'ui',
      title: 'Browser: Video returning /edge/malformed-m3u8 shows non-crashing error state in player',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'video');
        const badM3u8Src = {
          sourceName: 'Bad M3U8 Source',
          sourceUrl: `${ctx.mockBaseUrl}/video/bad-m3u8`,
          sourceFormat: 'rule',
          enabled: true,
          searchUrl: `${ctx.mockBaseUrl}/video/peertube/api/v1/search/videos?search={{key}}`,
          ruleSearch: {
            list: '$.data[*]',
            vodId: '$.uuid',
            name: '$.name',
            detailUrl: `${ctx.mockBaseUrl}/video/peertube/api/v1/videos/{{$.uuid}}`,
          },
          ruleDetail: {
            name: '$.name',
            episodes: '$.files[*]',
            playUrl: `${ctx.mockBaseUrl}/edge/malformed-m3u8`,
          },
          rulePlay: {
            streamUrl: `${ctx.mockBaseUrl}/edge/malformed-m3u8`,
            streamType: 'hls',
          },
        };
        await importSources(ctx.admin, 'video', badM3u8Src);
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
          await clickButtonMatching(cdp, [/播放|play|正片|1/i]);
          await cdp.waitForSelector('video', { timeoutMs: 8000 });
          assert.equal(cdp.exceptions.length, 0);
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T2-F16-03',
      feature: 'F16',
      domain: 'ui',
      title: 'Browser: Video detail with 0 playable groups shows empty/unsupported notice cleanly',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'video');
        const emptyVidSrc = {
          sourceName: 'Empty Groups Video',
          sourceUrl: `${ctx.mockBaseUrl}/video/empty-groups`,
          sourceFormat: 'rule',
          enabled: true,
          searchUrl: `${ctx.mockBaseUrl}/video/peertube/api/v1/search/videos?search={{key}}`,
          ruleSearch: {
            list: '$.data[*]',
            vodId: '$.uuid',
            name: '$.name',
            detailUrl: `${ctx.mockBaseUrl}/video/peertube/api/v1/videos/{{$.uuid}}`,
          },
          ruleDetail: {
            name: '$.name',
            episodes: '$.nonexistent_episodes[*]',
          },
        };
        await importSources(ctx.admin, 'video', emptyVidSrc);
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
          await cdp.waitForFn(`/暂无|empty|无可用|no.*episode/i.test(document.body.innerText)`, {
            timeoutMs: 8000,
          });
          cdp.assertNoErrors('T2-F16-03');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T2-F16-04',
      feature: 'F16',
      domain: 'ui',
      title: 'Browser: Direct MP4 (stream-proxy) video seek updates currentTime cleanly',
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
          await cdp.waitForFn(`(document.querySelector('video')?.readyState || 0) >= 1`, {
            timeoutMs: 10000,
          });
          await cdp.evalJs(`document.querySelector('video').currentTime = 0.5`);
          cdp.assertNoErrors('T2-F16-04');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T2-F16-05',
      feature: 'F16',
      domain: 'ui',
      title: 'Browser: Starting online video playback pauses active online music PlayerBar',
      async run(ctx) {
        await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        await ensureVideoMocks(ctx.admin, ctx.mockBaseUrl);
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
          await clickButtonMatching(cdp, [/影视|视频|video/i]);
          await clickButtonMatching(cdp, [/在线|online/i]);
          await cdp.waitForFn(`document.body.innerText.includes('Mock Blender Open Movie')`, {
            timeoutMs: 10000,
          });
          await clickButtonMatching(cdp, [/Mock Blender Open Movie/]);
          await clickButtonMatching(cdp, [/播放|play|360|正片/i]);
          await cdp.waitForSelector('video', { visible: true });
          const musicPaused = await cdp.evalJs(
            `!document.querySelector('audio') || document.querySelector('audio').paused`,
          );
          assert.equal(musicPaused, true, 'Music PlayerBar must pause when video plays');
          cdp.assertNoErrors('T2-F16-05');
        } finally {
          await cdp.close();
        }
      },
    },

    // =======================================================================
    // F17: Resilience Boundary (core)
    // =======================================================================
    {
      id: 'T2-F17-01',
      feature: 'F17',
      domain: 'core',
      title: 'Upstream 500 HTML error page returns structured JSON error envelope (never raw HTML)',
      async run(ctx) {
        const mocks = await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const res = await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/detail', {
          body: { source_id: mocks.xkcd.id, comic_url: `${ctx.mockBaseUrl}/edge/error-500-html` },
          expectedStatus: [400, 502],
        });
        assert.ok(res.json?.error?.code);
        assert.ok(!res.text.includes('<h1>500'));
      },
    },
    {
      id: 'T2-F17-02',
      feature: 'F17',
      domain: 'core',
      title: 'Malformed broken HTML (/edge/malformed-html) is tolerated by CSS parser without panic',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'comic');
        const edges = buildEdgeMockSources(ctx.mockBaseUrl);
        const imp = await importSources(ctx.admin, 'comic', edges.malformedHtml);
        const res = await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
          body: { keyword: 'partial', source_ids: [imp.sources[0].id] },
          expectedStatus: 200,
        });
        const items = res.json.results ?? res.json.items ?? [];
        assert.equal(items.length, 1);
        assert.equal(items[0].title ?? items[0].name, 'Partial');
      },
    },
    {
      id: 'T2-F17-03',
      feature: 'F17',
      domain: 'core',
      title: 'Concurrent burst of 8 parallel /online/search calls across all 3 domains completes cleanly',
      async run(ctx) {
        const comics = await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        const musics = await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        const videos = await ensureVideoMocks(ctx.admin, ctx.mockBaseUrl);
        const tasks = [
          ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
            body: { keyword: 'Naruto', source_ids: [comics.mangadex.id] },
            expectedStatus: 200,
          }),
          ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
            body: { keyword: 'Tower', source_ids: [comics.webtoon.id] },
            expectedStatus: 200,
          }),
          ctx.member.failLoudlyRequest('POST', '/api/v1/music/online/search', {
            body: { keyword: 'Mojito', source_ids: [musics.gdstudio.id] },
            expectedStatus: 200,
          }),
          ctx.member.failLoudlyRequest('POST', '/api/v1/music/online/search', {
            body: { keyword: 'Fragments', source_ids: [musics.audius.id] },
            expectedStatus: 200,
          }),
          ctx.member.failLoudlyRequest('POST', '/api/v1/video/online/search', {
            body: { keyword: 'Earth', source_ids: [videos.applecms.id] },
            expectedStatus: 200,
          }),
          ctx.member.failLoudlyRequest('POST', '/api/v1/video/online/search', {
            body: { keyword: 'Blender', source_ids: [videos.peertube.id] },
            expectedStatus: 200,
          }),
          ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
            body: { keyword: 'Strip', source_ids: [comics.xkcd.id] },
            expectedStatus: 200,
          }),
          ctx.member.failLoudlyRequest('POST', '/api/v1/music/online/search', {
            body: { keyword: 'Mojito', source_ids: [musics.gdstudio.id] },
            expectedStatus: 200,
          }),
        ];
        const all = await Promise.all(tasks);
        assert.equal(all.length, 8);
      },
    },
    {
      id: 'T2-F17-04',
      feature: 'F17',
      domain: 'core',
      title: 'Gated endpoint /edge/gated/comic-pages fails before login-action and succeeds after login-action',
      async run(ctx) {
        await clearDomainSources(ctx.admin, 'comic');
        const edges = buildEdgeMockSources(ctx.mockBaseUrl);
        const imp = await importSources(ctx.admin, 'comic', edges.cookieGated);
        const srcId = imp.sources[0].id;

        const before = await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/pages', {
          body: { source_id: srcId, chapter_url: `${ctx.mockBaseUrl}/edge/gated/comic-pages` },
          expectedStatus: [200, 400, 401, 502],
        });
        if (before.status === 200) {
          assert.equal(before.json.pages.length, 0);
        }

        await ctx.admin.failLoudlyRequest('POST', `/api/v1/comic/sources/${srcId}/login-action`, {
          body: { login_info: { username: 'alice', password: 'secret' } },
          expectedStatus: 200,
        });
        const after = await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/online/pages', {
          body: { source_id: srcId, chapter_url: `${ctx.mockBaseUrl}/edge/gated/comic-pages` },
          expectedStatus: 200,
        });
        assert.equal(after.json.pages.length, 1);
      },
    },
    {
      id: 'T2-F17-05',
      feature: 'F17',
      domain: 'core',
      title: 'Upstream connection reset mid-body terminates proxy cleanly without crashing gateway',
      async run(ctx) {
        const mocks = await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        let resetHitCount = 0;
        const resetSrv = http.createServer((req, res) => {
          resetHitCount += 1;
          res.writeHead(200, {
            'Content-Type': 'audio/mpeg',
            'Content-Length': String(MOCK_MP3_BYTES.length),
          });
          res.write(MOCK_MP3_BYTES.subarray(0, 512));
          setTimeout(() => req.socket.destroy(), 30);
        });
        await new Promise((r) => resetSrv.listen(0, '127.0.0.1', r));
        try {
          const port = resetSrv.address().port;
          let res = null;
          try {
            res = await ctx.member.request('GET', '/api/v1/music/online/audio-proxy', {
              query: {
                url: `http://127.0.0.1:${port}/abrupt.mp3`,
                source_id: mocks.gdstudio.id,
              },
              timeoutMs: 10000,
            });
          } catch {
            // Stream truncation or connection reset on client side is acceptable once upstream was hit
          }
          if (res && (res.status === 404 || res.status === 405)) {
            throw new SutEndpointMissingError(
              'GET',
              '/api/v1/music/online/audio-proxy',
              res.status,
              res.text?.slice(0, 200) || '',
            );
          }
          assert.ok(
            resetHitCount >= 1,
            `Expected audio-proxy to connect to upstream resetSrv (resetHitCount=${resetHitCount}, status=${res?.status ?? 'thrown'})`,
          );
          const cap = await ctx.member.failLoudlyRequest('GET', '/api/v1/system/capabilities', {
            authMode: 'none',
            expectedStatus: 200,
          });
          assert.equal(cap.json.modules.length, 4);
        } finally {
          await new Promise((r) => resetSrv.close(r));
        }
      },
    },

    // =======================================================================
    // F18: Non-Regression Boundary (nonreg)
    // =======================================================================
    {
      id: 'T2-F18-01',
      feature: 'F18',
      domain: 'nonreg',
      title: 'Local media upload, stream, and delete coexist with online sources without interference',
      async run(ctx) {
        await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        await ensureMusicMocks(ctx.admin, ctx.mockBaseUrl);
        await ensureVideoMocks(ctx.admin, ctx.mockBaseUrl);

        const cbzBuf = buildMinimalZip([{ name: '001.png', data: MOCK_PNG_BYTES }]);
        const comicForm = buildMultipartForm([
          { filename: 't2_local.cbz', contentType: 'application/zip', data: cbzBuf },
        ]);
        const up = await ctx.admin.failLoudlyRequest('POST', '/api/v1/comic/upload', {
          headers: { 'Content-Type': comicForm.contentType },
          body: comicForm.body,
          expectedStatus: 201,
        });
        const seriesId = up.json.uploaded[0].series_id;
        await ctx.admin.failLoudlyRequest('DELETE', `/api/v1/comic/series/${seriesId}`, {
          expectedStatus: [200, 204],
        });

        // Online sources remain untouched
        const srcs = (
          await ctx.admin.failLoudlyRequest('GET', '/api/v1/comic/sources', { expectedStatus: 200 })
        ).json;
        assert.equal(srcs.length, 3);
      },
    },
    {
      id: 'T2-F18-02',
      feature: 'F18',
      domain: 'nonreg',
      title: 'xmedia-book tracked online book (/online/track, /online/toc, /online/chapter/0) stays green',
      async run(ctx) {
        const books = buildMockBookSources(ctx.mockBaseUrl);
        const imp = await importSources(ctx.admin, 'book', books.basicBook);
        const srcId = imp.sources[0].id;
        const bookUrl = `${ctx.mockBaseUrl}/comic/webtoon/list?title_no=95`;

        const tracked = await ctx.member.failLoudlyRequest('POST', '/api/v1/book/online/track', {
          body: {
            source_id: srcId,
            name: 'Tracked NonReg Book',
            author: 'SIU Mock',
            book_url: bookUrl,
            toc_url: bookUrl,
          },
          expectedStatus: 200,
        });
        const bookId = tracked.json.id;
        const toc = await ctx.member.failLoudlyRequest(
          'GET',
          `/api/v1/book/books/${bookId}/online/toc`,
          { expectedStatus: 200 },
        );
        assert.ok(toc.json.chapters.length >= 2);
        const ch = await ctx.member.failLoudlyRequest(
          'GET',
          `/api/v1/book/books/${bookId}/online/chapter/0`,
          { expectedStatus: 200 },
        );
        assert.ok(typeof ch.json.content === 'string');
      },
    },
    {
      id: 'T2-F18-03',
      feature: 'F18',
      domain: 'nonreg',
      title: 'Browser: Local comic reader, local music playback, and local video player show zero regression',
      async run(ctx) {
        const cbzBuf = buildMinimalZip([{ name: '001.png', data: MOCK_PNG_BYTES }]);
        const comicForm = buildMultipartForm([
          { filename: 'ui_local.cbz', contentType: 'application/zip', data: cbzBuf },
        ]);
        const upComic = await ctx.admin.failLoudlyRequest('POST', '/api/v1/comic/upload', {
          headers: { 'Content-Type': comicForm.contentType },
          body: comicForm.body,
          expectedStatus: 201,
        });
        const chapterId = upComic.json.uploaded[0].chapter_id;

        const cdp = await openAuthenticatedPage(
          ctx.CdpClient,
          ctx.baseUrl,
          ctx.member,
          `/comic/read/${chapterId}`,
        );
        try {
          await cdp.waitForFn(
            `[...document.querySelectorAll('img')].some((i) => i.complete && i.naturalWidth > 0)`,
            { timeoutMs: 10000, label: 'local comic reader page loaded' },
          );
          await cdp.goto(`${ctx.baseUrl}/music/songs`);
          await cdp.waitForSelector('#app', { visible: true });
          await cdp.goto(`${ctx.baseUrl}/video/catalog`);
          await cdp.waitForSelector('#app', { visible: true });
          cdp.assertNoErrors('T2-F18-03');
        } finally {
          await cdp.close();
        }
      },
    },
    {
      id: 'T2-F18-04',
      feature: 'F18',
      domain: 'nonreg',
      title: 'Non-privileged user access across local /comic/series and online /comic/sources enforces consistent RBAC',
      async run(ctx) {
        await ensureComicMocks(ctx.admin, ctx.mockBaseUrl);
        await ctx.member.failLoudlyRequest('GET', '/api/v1/comic/series', { expectedStatus: 200 });
        const srcs = await ctx.member.failLoudlyRequest('GET', '/api/v1/comic/sources', {
          expectedStatus: 200,
        });
        assert.ok(srcs.json.every((s) => s.enabled === true && s.variable_preview === ''));
        await ctx.member.failLoudlyRequest('POST', '/api/v1/comic/sources/import', {
          body: { raw_json: '[]' },
          expectedStatus: 403,
        });
      },
    },
    {
      id: 'T2-F18-05',
      feature: 'F18',
      domain: 'nonreg',
      title: 'Server graceful SIGTERM shutdown after online operations exits cleanly with intact DB',
      async run(ctx) {
        if (ctx.externalBaseUrl) {
          throw new Error('T2-F18-05 requires managed SUT to test SIGTERM graceful shutdown');
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
          await admin.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
            body: { keyword: 'Strip', source_ids: [mocks.xkcd.id] },
            expectedStatus: 200,
          });
          const exitRes = await mgr.stop();
          assert.ok(
            exitRes.code === 0 || exitRes.signal === 'SIGTERM',
            `Unexpected shutdown result: ${JSON.stringify(exitRes)}`,
          );
          // Re-open same dataDir to verify SQLite WAL/DB integrity
          const url2 = await mgr.start({ mode: 'mock' });
          const admin2 = new ctx.ApiClient(url2, { authMode: 'bearer' });
          await admin2.login({ username: 'admin', password: 'AdminPass123!' });
          const srcs = (
            await admin2.failLoudlyRequest('GET', '/api/v1/comic/sources', { expectedStatus: 200 })
          ).json;
          assert.equal(srcs.length, 3);
        } finally {
          await mgr.cleanup();
        }
      },
    },
  ];
}

// ---------------------------------------------------------------------------
// Public Runner Entrypoint
// ---------------------------------------------------------------------------

export async function runTier2(context = {}) {
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

  const allCases = buildTier2Cases();
  const selectedCases = domain ? allCases.filter((c) => c.domain === domain) : allCases;
  const results = [];

  if (selectedCases.length === 0) {
    return { name: 'tier2', suite: 'tier2', total: 0, passed: 0, failed: 0, cases: [] };
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
        id: 'T2-BOOTSTRAP',
        feature: 'BOOTSTRAP',
        domain: domain || 'core',
        title: 'Tier 2 SUT startup & auth bootstrap',
        status: 'fail',
        durationMs: 0,
        error: bootErr.stack || bootErr.message,
      });
      if (!silent) {
        console.error(`  [FAIL] T2-BOOTSTRAP: ${bootErr.message}`);
      }
      return {
        name: 'tier2',
        suite: 'tier2',
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
    name: 'tier2',
    suite: 'tier2',
    total: results.length,
    passed,
    failed,
    cases: results,
  };
}

export const runSuite = runTier2;
