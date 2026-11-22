import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  buildEdgeMockSources,
  buildMockBookSources,
  buildMockComicSources,
  buildMockMusicSources,
  buildMockVideoSources,
} from '../fixtures/mock_sources.mjs';
import { withCdpBrowser } from '../lib/cdp.mjs';
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
  assertJpeg,
  assertM3u8Rewritten,
  assertMp3,
  assertMp4,
  assertMpegTs,
  decryptAes128Cbc,
} from '../lib/fixtures.mjs';
import { MockOriginServer } from '../lib/mock_origin.mjs';

const ADMIN_CREDS = { username: 'admin', display_name: 'E2E Admin', password: 'AdminPass123!' };
const MEMBER_CREDS = {
  username: 'e2e_member',
  display_name: 'E2E Member',
  password: 'UserPass123!',
  role: 'member',
};

const ZIP_CRC_TABLE = (() => {
  const table = new Uint32Array(256);
  for (let n = 0; n < 256; n += 1) {
    let c = n;
    for (let k = 0; k < 8; k += 1) {
      c = (c & 1) !== 0 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    }
    table[n] = c >>> 0;
  }
  return table;
})();

function zipCrc32(buf) {
  let c = 0xffffffff;
  for (let i = 0; i < buf.length; i += 1) {
    c = ZIP_CRC_TABLE[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
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
    const crc = zipCrc32(dataBuf);

    const localHdr = Buffer.alloc(30);
    localHdr.writeUInt32LE(0x04034b50, 0);
    localHdr.writeUInt16LE(20, 4);
    localHdr.writeUInt16LE(0x0800, 6);
    localHdr.writeUInt16LE(0, 8);
    localHdr.writeUInt16LE(0, 10);
    localHdr.writeUInt16LE(0, 12);
    localHdr.writeUInt32LE(crc, 14);
    localHdr.writeUInt32LE(dataBuf.length, 18);
    localHdr.writeUInt32LE(dataBuf.length, 22);
    localHdr.writeUInt16LE(nameBuf.length, 26);
    localHdr.writeUInt16LE(0, 28);
    localParts.push(localHdr, nameBuf, dataBuf);

    const centralHdr = Buffer.alloc(46);
    centralHdr.writeUInt32LE(0x02014b50, 0);
    centralHdr.writeUInt16LE(20, 4);
    centralHdr.writeUInt16LE(20, 6);
    centralHdr.writeUInt16LE(0x0800, 8);
    centralHdr.writeUInt16LE(0, 10);
    centralHdr.writeUInt16LE(0, 12);
    centralHdr.writeUInt16LE(0, 14);
    centralHdr.writeUInt32LE(crc, 16);
    centralHdr.writeUInt32LE(dataBuf.length, 20);
    centralHdr.writeUInt32LE(dataBuf.length, 24);
    centralHdr.writeUInt16LE(nameBuf.length, 28);
    centralHdr.writeUInt16LE(0, 30);
    centralHdr.writeUInt16LE(0, 32);
    centralHdr.writeUInt16LE(0, 34);
    centralHdr.writeUInt16LE(0, 36);
    centralHdr.writeUInt32LE(0, 38);
    centralHdr.writeUInt32LE(offset, 42);
    centralParts.push(centralHdr, nameBuf);

    offset += localHdr.length + nameBuf.length + dataBuf.length;
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

async function ensureBootstrapped(baseUrl) {
  const probe = new ApiClient(baseUrl, { authMode: 'none' });
  const status = await probe.getSetupStatus();
  const admin = new ApiClient(baseUrl, { authMode: 'bearer' });
  if (status.needs_setup) {
    await admin.setupAdmin(ADMIN_CREDS);
  } else {
    await admin.login({ username: ADMIN_CREDS.username, password: ADMIN_CREDS.password });
  }

  const member = new ApiClient(baseUrl, { authMode: 'bearer' });
  const createRes = await admin.request('POST', '/api/v1/users', { body: MEMBER_CREDS });
  if (createRes.status === 404 || createRes.status === 405) {
    throw new SutEndpointMissingError('POST', '/api/v1/users', createRes.status, createRes.text);
  }
  await member.login({ username: MEMBER_CREDS.username, password: MEMBER_CREDS.password });
  return { admin, member };
}

async function importSources(admin, domain, sourceOrArray) {
  const rawJson =
    typeof sourceOrArray === 'string' ? sourceOrArray : JSON.stringify(sourceOrArray);
  const res = await admin.failLoudlyRequest('POST', `/api/v1/${domain}/sources/import`, {
    body: { raw_json: rawJson },
    expectedStatus: 200,
  });
  return res.json;
}

async function deleteSourceSafe(admin, domain, id) {
  if (!id) return;
  await admin.request('DELETE', `/api/v1/${domain}/sources/${id}`).catch(() => {});
}

function toRelativeApiPath(urlOrPath) {
  if (!urlOrPath) return '';
  if (urlOrPath.startsWith('http://') || urlOrPath.startsWith('https://')) {
    const u = new URL(urlOrPath);
    return `${u.pathname}${u.search}`;
  }
  return urlOrPath.startsWith('/') ? urlOrPath : `/${urlOrPath}`;
}

async function browserLogin(cdp, baseUrl, creds) {
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

async function clickButtonByText(cdp, texts, { timeoutMs = 8000 } = {}) {
  const list = Array.isArray(texts) ? texts : [texts];
  const clicked = await cdp.waitForFn(
    `(() => {
      const targets = ${JSON.stringify(list)}.map(s => s.toLowerCase());
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
        ].map((s) => s.trim().toLowerCase()).filter(Boolean);
        return candidates.some((c) => targets.some((t) => c.includes(t)));
      };
      const primary = [...document.querySelectorAll('button, [role="link"], [role="button"], [role="tab"], [role="menuitem"], a')];
      for (const el of primary) {
        if (isVisible(el) && matchesEl(el)) {
          el.scrollIntoView({ block: 'center' });
          el.click();
          return true;
        }
      }
      const fallback = [...document.querySelectorAll('tr, td')];
      for (const el of fallback) {
        if (isVisible(el) && matchesEl(el)) {
          el.scrollIntoView({ block: 'center' });
          el.click();
          return true;
        }
      }
      return false;
    })()`,
    { timeoutMs, pollMs: 100, label: `clickButtonByText(${list.join('|')})` },
  );
  assert.equal(clicked, true);
}

export async function runTier4(context = {}) {
  const silent = Boolean(context.silent);
  const domainFilter = context.domain || null;
  const MockSrv = context.MockOriginServer || MockOriginServer;
  const ServerMgr = context.XMediaServerManager || XMediaServerManager;

  const mock = new MockSrv();
  const mockUrl = await mock.start();

  const ownsSut = !context.baseUrl;
  let serverMgr = null;
  let baseUrl = context.baseUrl ? context.baseUrl.replace(/\/+$/, '') : '';

  const cases = [];

  async function runCase(id, domains, title, fn) {
    if (domainFilter && !domains.includes(domainFilter)) {
      return;
    }
    const start = performance.now();
    try {
      await fn();
      const durationMs = Math.round(performance.now() - start);
      cases.push({ id, domains, title, status: 'pass', durationMs });
      if (!silent) {
        console.log(`  [PASS] ${id}: ${title} (${durationMs}ms)`);
      }
    } catch (err) {
      const durationMs = Math.round(performance.now() - start);
      cases.push({
        id,
        domains,
        title,
        status: 'fail',
        durationMs,
        error: err.stack || err.message,
      });
      if (!silent) {
        console.error(`  [FAIL] ${id}: ${title} (${durationMs}ms)\n    ${err.message}`);
      }
    }
  }

  try {
    if (ownsSut) {
      serverMgr = new ServerMgr({
        xmediaBin: context.xmediaBin || null,
        webDir: context.webDir !== undefined ? context.webDir : resolveWebDir(),
        keepTmp: Boolean(context.keepTmp),
      });
      baseUrl = await serverMgr.start({ mode: 'mock' });
    }

    let admin;
    let member;
    try {
      ({ admin, member } = await ensureBootstrapped(baseUrl));
    } catch (err) {
      if (!silent) {
        console.error(`  [FAIL] T4-BOOTSTRAP: ${err.message}`);
      }
      return {
        name: 'tier4',
        suite: 'tier4',
        total: 1,
        passed: 0,
        failed: 1,
        cases: [
          {
            id: 'T4-BOOTSTRAP',
            domains: ['core'],
            title: 'Tier 4 SUT bootstrap & auth setup',
            status: 'fail',
            durationMs: 0,
            error: err.stack || err.message,
          },
        ],
      };
    }

    const comicMocks = buildMockComicSources(mockUrl, { strictUa: true });
    const musicMocks = buildMockMusicSources(mockUrl);
    const videoMocks = buildMockVideoSources(mockUrl);
    const bookMocks = buildMockBookSources(mockUrl);
    const edgeMocks = buildEdgeMockSources(mockUrl);

    // T4-01: Fresh Server Bootstrap -> Multi-Domain Seed & Source Admin Flow + Restart Persistence
    await runCase(
      'T4-01',
      ['core', 'comic', 'music', 'video'],
      'Fresh Server Bootstrap -> Multi-Domain Source Admin Flow & Restart Persistence',
      async () => {
        let localMgr = null;
        let targetUrl = baseUrl;
        try {
          if (ownsSut) {
            localMgr = new ServerMgr({
              xmediaBin: context.xmediaBin || null,
              webDir: context.webDir !== undefined ? context.webDir : resolveWebDir(),
              keepTmp: Boolean(context.keepTmp),
            });
            targetUrl = await localMgr.start({ mode: 'mock' });
          }

          const { admin: fAdmin, member: fMember } = await ensureBootstrapped(targetUrl);

          const cImport = await importSources(fAdmin, 'comic', comicMocks.all);
          assert.equal(cImport.sources.length, 3);
          const mImport = await importSources(fAdmin, 'music', musicMocks.all);
          assert.equal(mImport.sources.length, 2);
          const vImport = await importSources(fAdmin, 'video', videoMocks.all);
          assert.equal(vImport.sources.length, 2);

          // Reorder comic sources and disable XKCD
          const xkcd = cImport.sources.find((s) => s.name === 'Mock XKCD');
          const webtoon = cImport.sources.find((s) => s.name === 'Mock WEBTOON');
          const mangadex = cImport.sources.find((s) => s.name === 'Mock MangaDex');
          assert.ok(xkcd && webtoon && mangadex);

          await fAdmin.failLoudlyRequest('POST', '/api/v1/comic/sources/reorder', {
            body: { ordered_ids: [webtoon.id, mangadex.id, xkcd.id] },
            expectedStatus: 200,
          });
          await fAdmin.failLoudlyRequest('PATCH', `/api/v1/comic/sources/${xkcd.id}`, {
            body: { enabled: false, group_name: 'Disabled-Archive' },
            expectedStatus: 200,
          });

          // Verify restart persistence when running managed SUT
          let postAdmin = fAdmin;
          let postMember = fMember;
          if (localMgr) {
            targetUrl = await localMgr.restart({ mode: 'mock' });
            postAdmin = new ApiClient(targetUrl, { authMode: 'bearer' });
            await postAdmin.login(ADMIN_CREDS);
            postMember = new ApiClient(targetUrl, { authMode: 'bearer' });
            await postMember.login(MEMBER_CREDS);
          }

          const adminComics = (
            await postAdmin.failLoudlyRequest('GET', '/api/v1/comic/sources', {
              expectedStatus: 200,
            })
          ).json;
          assert.equal(adminComics.length, 3);
          const persistedXkcd = adminComics.find((s) => s.id === xkcd.id);
          assert.equal(persistedXkcd.enabled, false);
          assert.equal(persistedXkcd.group_name, 'Disabled-Archive');

          const memberComics = (
            await postMember.failLoudlyRequest('GET', '/api/v1/comic/sources', {
              expectedStatus: 200,
            })
          ).json;
          assert.equal(memberComics.length, 2);
          assert.ok(!memberComics.some((s) => s.id === xkcd.id));
          for (const s of memberComics) {
            assert.deepEqual(s.login_info || {}, {});
            assert.deepEqual(s.login_ui || [], []);
            assert.equal(s.variable_preview || '', '');
            assert.deepEqual(s.cookie_hosts || [], []);
          }

          // Clean up if reusing external --base-url
          if (!localMgr) {
            for (const s of cImport.sources) await deleteSourceSafe(postAdmin, 'comic', s.id);
            for (const s of mImport.sources) await deleteSourceSafe(postAdmin, 'music', s.id);
            for (const s of vImport.sources) await deleteSourceSafe(postAdmin, 'video', s.id);
          }
        } finally {
          if (localMgr) await localMgr.cleanup();
        }
      },
    );

    // T4-02: End-to-End Online Comic Reading Journey (API + Chromium)
    await runCase(
      'T4-02',
      ['comic', 'ui'],
      'End-to-End Online Comic Reading Journey (GlobalSearch -> WEBTOON webtoon -> MangaDex rtl_double)',
      async () => {
        const wt = (await importSources(admin, 'comic', comicMocks.webtoon)).sources[0];
        const md = (await importSources(admin, 'comic', comicMocks.mangadex)).sources[0];
        try {
          // API contract check for both sources
          const wtPagesRes = await member.failLoudlyRequest('POST', '/api/v1/comic/online/pages', {
            body: {
              source_id: wt.id,
              chapter_url: `${mockUrl}/comic/webtoon/viewer?title_no=95&episode_no=1`,
            },
            expectedStatus: 200,
          });
          assert.equal(wtPagesRes.json?.pages?.length, 2);
          const wtImg = await member.failLoudlyRequest(
            'GET',
            toRelativeApiPath(wtPagesRes.json.pages[0].proxy_url),
            { expectedStatus: 200 },
          );
          assertJpeg(wtImg.buffer, 'T4-02 WEBTOON API proxy image');

          await withCdpBrowser(async (cdp) => {
            await browserLogin(cdp, baseUrl, MEMBER_CREDS);

            // 1. Open GlobalSearchModal via Ctrl+K, search "Tower", click online comic search
            await cdp.pressKey('k', { modifiers: 2 });
            await cdp.waitForSelector('div[role="dialog"] input', { visible: true });
            await cdp.type('div[role="dialog"] input', 'Tower');
            const clickedOnline = await cdp.waitForFn(
              `(() => {
                const dlg = document.querySelector('div[role="dialog"]');
                if (!dlg) return false;
                const btn = [...dlg.querySelectorAll('button')].find(b => {
                  const t = (b.textContent || '').toLowerCase();
                  return (t.includes('在线') || t.includes('online')) && (t.includes('漫画') || t.includes('comic'));
                });
                if (btn) { btn.click(); return true; }
                return false;
              })()`,
              { timeoutMs: 8000, label: 'Click online comic quick search in GlobalSearchModal' },
            );
            assert.equal(clickedOnline, true);

            // 2. Select "Tower of Mock" -> detail & chapters -> open Ep.1 in webtoon mode
            await cdp.waitForFn(
              `document.body.textContent.includes('Tower of Mock')`,
              { timeoutMs: 12000, label: 'WEBTOON Tower of Mock rendered' },
            );
            await clickButtonByText(cdp, ['Tower of Mock'], { timeoutMs: 8000 });
            await cdp.waitForFn(
              `document.body.textContent.includes('Ep.1 Prologue')`,
              { timeoutMs: 10000, label: 'WEBTOON chapters rendered' },
            );
            await clickButtonByText(cdp, ['Ep.1 Prologue', '开始阅读', 'Read'], { timeoutMs: 8000 });

            await cdp.waitForFn(
              `(() => {
                if (!window.location.pathname.includes('/comic/online/read')) return false;
                const imgs = [...document.querySelectorAll('img[alt^="Page "][src*="image-proxy"]')];
                return imgs.length >= 2 && imgs.every(i => i.complete && i.naturalWidth > 0);
              })()`,
              { timeoutMs: 12000, label: 'WEBTOON reader images decoded' },
            );

            // 3. Switch to Ep.2
            await clickButtonByText(cdp, ['下一章', 'Next'], { timeoutMs: 8000 });
            await cdp.waitForFn(
              `(() => {
                if (!window.location.pathname.includes('/comic/online/read')) return false;
                const imgs = [...document.querySelectorAll('img[alt^="Page "][src*="image-proxy"]')];
                return imgs.length >= 2 && imgs.every(i => i.complete && i.naturalWidth > 0);
              })()`,
              { timeoutMs: 10000, label: 'WEBTOON Ep.2 images decoded' },
            );

            // 4. Switch to MangaDex comic in rtl_double mode
            await cdp.goto(`${baseUrl}/comic/series?scope=online&q=Naruto`);
            await cdp.waitForFn(
              `document.body.textContent.includes('Mock Naruto')`,
              { timeoutMs: 12000, label: 'MangaDex Mock Naruto rendered' },
            );
            await clickButtonByText(cdp, ['Mock Naruto'], { timeoutMs: 8000 });
            await cdp.waitForFn(
              `document.body.textContent.includes('Arrival')`,
              { timeoutMs: 10000, label: 'MangaDex chapters rendered' },
            );
            await clickButtonByText(cdp, ['Arrival', '开始阅读', 'Read'], { timeoutMs: 8000 });
            await cdp.waitForFn(
              `(() => {
                if (!window.location.pathname.includes('/comic/online/read')) return false;
                const imgs = [...document.querySelectorAll('img[alt^="Page "][src*="image-proxy"]')];
                return imgs.length >= 1 && imgs.every(i => i.complete && i.naturalWidth > 0);
              })()`,
              { timeoutMs: 12000, label: 'MangaDex reader images decoded' },
            );

            cdp.assertNoErrors('T4-02 Online Comic Reading Journey');
          });
        } finally {
          await deleteSourceSafe(admin, 'comic', wt.id);
          await deleteSourceSafe(admin, 'comic', md.id);
        }
      },
    );

    // T4-03: End-to-End Online Music Listening, Synced Lyrics & Queue Journey (API + Chromium)
    await runCase(
      'T4-03',
      ['music', 'ui'],
      'End-to-End Online Music Listening, Synced Lyrics & Queue Journey',
      async () => {
        const gd = (await importSources(admin, 'music', musicMocks.gdstudio)).sources[0];
        const aud = (await importSources(admin, 'music', musicMocks.audius)).sources[0];
        try {
          // Verify API 206 + 200 streaming first
          const gdSearch = await member.failLoudlyRequest('POST', '/api/v1/music/online/search', {
            body: { keyword: 'Mojito', source_ids: [gd.id] },
            expectedStatus: 200,
          });
          const gdTrack = (gdSearch.json?.results || gdSearch.json?.items || [])[0];
          assert.ok(gdTrack, 'Expected GDStudio search result in T4-03');
          const gdInfo = await member.failLoudlyRequest('POST', '/api/v1/music/online/track-info', {
            body: {
              source_id: gd.id,
              track_id: gdTrack.track_id || gdTrack.id || 'gd-101',
              url_id: gdTrack.url_id,
              cover_id: gdTrack.cover_id,
              lyric_id: gdTrack.lyric_id,
              quality: '320',
            },
            expectedStatus: 200,
          });
          const gdSlice = await member.failLoudlyRequest(
            'GET',
            toRelativeApiPath(gdInfo.json.proxy_stream_url),
            { headers: { Range: 'bytes=0-4095' }, expectedStatus: 206 },
          );
          assertMp3(gdSlice.buffer, 'T4-03 GDStudio 206');

          await withCdpBrowser(async (cdp) => {
            await browserLogin(cdp, baseUrl, MEMBER_CREDS);
            await cdp.goto(`${baseUrl}/music/songs`);
            await clickButtonByText(cdp, ['在线', 'Online'], { timeoutMs: 8000 });

            // Search "Mock"
            await cdp.waitForSelector('main input[type="search"], main input[type="text"]', {
              visible: true,
            });
            await cdp.type('main input[type="search"], main input[type="text"]', 'Mock');

            await cdp.waitForFn(
              `document.body.textContent.includes('Mock Mojito') && document.body.textContent.includes('Fragments Mock')`,
              { timeoutMs: 12000, label: 'Both GDStudio and Audius mock tracks rendered' },
            );

            // Play GDStudio track
            await clickButtonByText(cdp, ['Mock Mojito'], { timeoutMs: 8000 });
            await cdp.waitForFn(
              `(() => {
                const a = document.querySelector('audio');
                return Boolean(a && a.src && a.src.includes('audio-proxy') && a.readyState >= 1);
              })()`,
              { timeoutMs: 10000, label: 'PlayerBar audio loaded via audio-proxy' },
            );

            // Verify cover image in PlayerBar
            await cdp.waitForFn(
              `(() => {
                const covers = [...document.querySelectorAll('img')].filter(i => (i.src || '').includes('cover-proxy'));
                return covers.length >= 1 && covers.some(i => i.complete && i.naturalWidth > 0);
              })()`,
              { timeoutMs: 8000, label: 'PlayerBar cover loaded via cover-proxy' },
            );

            // Open LyricsOverlay & verify synced lines + seek click
            await clickButtonByText(cdp, ['歌词', 'Lyrics'], { timeoutMs: 8000 });
            await cdp.waitForFn(
              `document.body.textContent.includes('First synced line') && document.body.textContent.includes('Second synced line')`,
              { timeoutMs: 8000, label: 'Synced LRC lines rendered in LyricsOverlay' },
            );
            await clickButtonByText(cdp, ['Second synced line'], { timeoutMs: 8000 });
            await cdp.pressKey('Escape');

            // Navigate via SPA sidebar across workspace routes while music persists
            await clickButtonByText(cdp, ['漫画', 'Comic'], { timeoutMs: 8000 });
            await cdp.waitForFn(
              `(() => {
                const a = document.querySelector('audio');
                return window.location.pathname.startsWith('/comic') && Boolean(a && a.src && a.src.includes('audio-proxy'));
              })()`,
              { timeoutMs: 8000, label: 'PlayerBar audio persists across route navigation' },
            );

            cdp.assertNoErrors('T4-03 Online Music Journey');
          });
        } finally {
          await deleteSourceSafe(admin, 'music', gd.id);
          await deleteSourceSafe(admin, 'music', aud.id);
        }
      },
    );

    // T4-04: End-to-End Online Video HLS & MP4 Cinema Journey (API + Chromium)
    await runCase(
      'T4-04',
      ['video', 'ui'],
      'End-to-End Online Video HLS (AppleCMS) & MP4 (PeerTube) Cinema Journey',
      async () => {
        const ac = (await importSources(admin, 'video', videoMocks.applecms)).sources[0];
        const pt = (await importSources(admin, 'video', videoMocks.peertube)).sources[0];
        try {
          // API contract check on AppleCMS detail + HLS rewrite + PeerTube MP4
          const acDetail = await member.failLoudlyRequest('POST', '/api/v1/video/online/detail', {
            body: { source_id: ac.id, video_id: '1001' },
            expectedStatus: 200,
          });
          const groupNames = (acDetail.json?.play_groups || []).map((g) => g.name);
          assert.ok(!groupNames.includes('sharehtml'), 'Non-media /share/ line must be filtered');
          assert.equal(groupNames[0], 'ikm3u8', 'preferredPlayFrom ordering');

          const acPlay = await member.failLoudlyRequest('POST', '/api/v1/video/online/play', {
            body: {
              source_id: ac.id,
              play_url: acDetail.json.play_groups[0].episodes[0].play_url,
              group_name: 'ikm3u8',
            },
            expectedStatus: 200,
          });
          assert.equal(acPlay.json?.stream_type, 'hls');
          const m3u8Res = await member.failLoudlyRequest(
            'GET',
            toRelativeApiPath(acPlay.json.proxy_stream_url),
            { expectedStatus: 200 },
          );
          const masterParsed = assertM3u8Rewritten(m3u8Res.text, { expectedSourceId: ac.id });
          const subRes = await member.failLoudlyRequest(
            'GET',
            toRelativeApiPath(masterParsed.segments[0]),
            { expectedStatus: 200 },
          );
          const subParsed = assertM3u8Rewritten(subRes.text, {
            expectedSourceId: ac.id,
            requireKey: true,
            requireMap: true,
          });
          const tsRes = await member.failLoudlyRequest(
            'GET',
            toRelativeApiPath(subParsed.segments[0]),
            { expectedStatus: 200 },
          );
          assert.notEqual(tsRes.buffer[0], 0x47, 'seg0.ts should be AES-128 encrypted');
          const decTs = decryptAes128Cbc(tsRes.buffer, MOCK_AES_KEY_BYTES, 0);
          assertMpegTs(decTs, 'T4-04 AppleCMS TS segment');

          // Chromium UI journey
          await withCdpBrowser(async (cdp) => {
            await browserLogin(cdp, baseUrl, MEMBER_CREDS);
            await cdp.goto(`${baseUrl}/video/catalog`);
            await clickButtonByText(cdp, ['在线', 'Online'], { timeoutMs: 8000 });

            await cdp.waitForSelector('main input[type="search"], main input[type="text"]', {
              visible: true,
            });
            await cdp.type('main input[type="search"], main input[type="text"]', 'Earth');
            await cdp.waitForFn(
              `document.body.textContent.includes('Mock Wandering Earth')`,
              { timeoutMs: 12000, label: 'AppleCMS search card rendered' },
            );
            await clickButtonByText(cdp, ['Mock Wandering Earth'], { timeoutMs: 8000 });
            await cdp.waitForFn(
              `document.body.textContent.includes('第1集') && document.body.textContent.includes('第2集')`,
              { timeoutMs: 10000, label: 'AppleCMS episodes rendered' },
            );
            await clickButtonByText(cdp, ['第1集'], { timeoutMs: 8000 });
            await cdp.waitForFn(
              `Boolean(document.querySelector('video'))`,
              { timeoutMs: 10000, label: 'VideoPlayerView mounted for HLS' },
            );
            await cdp.waitForFn(
              `(() => {
                const v = document.querySelector('video');
                return Boolean(v && v.error === null);
              })()`,
              { timeoutMs: 8000, label: 'Video element error === null' },
            );

            // Play PeerTube MP4
            await cdp.goto(`${baseUrl}/video/catalog?scope=online&q=Blender`);
            await cdp.waitForFn(
              `document.body.textContent.includes('Mock Blender Open Movie')`,
              { timeoutMs: 12000, label: 'PeerTube search card rendered' },
            );
            await clickButtonByText(cdp, ['Mock Blender Open Movie'], { timeoutMs: 8000 });
            await clickButtonByText(cdp, ['播放', 'Play', '360'], { timeoutMs: 8000 });
            await cdp.waitForFn(
              `(() => {
                const v = document.querySelector('video');
                return Boolean(v && (v.currentSrc || v.src || '').includes('stream-proxy') && v.readyState >= 1);
              })()`,
              { timeoutMs: 10000, label: 'PeerTube MP4 loaded via stream-proxy' },
            );

            cdp.assertNoErrors('T4-04 Online Video Journey');
          });
        } finally {
          await deleteSourceSafe(admin, 'video', ac.id);
          await deleteSourceSafe(admin, 'video', pt.id);
        }
      },
    );

    // T4-05: Admin Source Authoring, Cookie Login & Live Debugger Journey (Chromium Settings)
    await runCase(
      'T4-05',
      ['ui', 'comic', 'core'],
      'Browser: Admin Source Authoring, Cookie Login & Live Debugger Journey',
      async () => {
        await withCdpBrowser(async (cdp) => {
          await browserLogin(cdp, baseUrl, ADMIN_CREDS);
          await cdp.goto(`${baseUrl}/settings?tab=domain-datasources&domain=comic`);
          await cdp.waitForFn(
            `document.body.textContent.includes('导入') || document.body.textContent.toLowerCase().includes('import')`,
            { timeoutMs: 10000, label: 'Comic data sources manager rendered in Settings' },
          );

          // Open paste JSON import box and import cookieGated source
          await clickButtonByText(cdp, ['粘贴', '新建', 'Paste', 'Import JSON', '导入 JSON'], {
            timeoutMs: 8000,
          }).catch(async () => {
            await clickButtonByText(cdp, ['导入', 'Import'], { timeoutMs: 8000 });
          });

          await cdp.waitForSelector('textarea', { visible: true, timeoutMs: 8000 });
          await cdp.type('textarea', JSON.stringify(edgeMocks.cookieGated));
          await clickButtonByText(cdp, ['确认导入', '提交', 'Import', 'Submit'], { timeoutMs: 8000 });

          await cdp.waitForFn(
            `document.body.textContent.includes('Edge Cookie Gated Comic')`,
            { timeoutMs: 10000, label: 'Imported source card appeared in UI' },
          );

          // Trigger login action via UI or API & verify cookie_hosts + debug flow
          const srcs = (
            await admin.failLoudlyRequest('GET', '/api/v1/comic/sources', { expectedStatus: 200 })
          ).json;
          const gated = srcs.find((s) => s.name === 'Edge Cookie Gated Comic');
          assert.ok(gated, 'Cookie-gated source must exist in DB after UI import');

          const loginResp = await admin.failLoudlyRequest(
            'POST',
            `/api/v1/comic/sources/${gated.id}/login-action`,
            {
              body: { action: 'login', login_info: { username: 'alice', password: 'secret' } },
              expectedStatus: 200,
            },
          );
          assert.ok((loginResp.json?.source?.cookie_hosts || []).length >= 1);

          // Run full test debugger via API + reload UI to verify green status indicator
          const testResp = await admin.failLoudlyRequest(
            'POST',
            `/api/v1/comic/sources/${gated.id}/test`,
            { body: { mode: 'full', keyword: 'xkcd' }, expectedStatus: 200 },
          );
          assert.equal(testResp.json?.ok, true, `Debug test failed: ${testResp.text}`);

          await cdp.goto(`${baseUrl}/settings?tab=domain-datasources&domain=comic`);
          await cdp.waitForFn(
            `document.body.textContent.includes('Edge Cookie Gated Comic')`,
            { timeoutMs: 10000, label: 'Source still visible after test' },
          );
          cdp.assertNoErrors('T4-05 Admin Settings Source Journey');

          await deleteSourceSafe(admin, 'comic', gated.id);
        });
      },
    );

    // T4-06: Adversarial & Hostile Upstream Resilience + Hardened SSRF Blocking Journey
    await runCase(
      'T4-06',
      ['core', 'comic', 'music', 'video'],
      'Adversarial & Hostile Upstream Resilience + Hardened SSRF Blocking Journey',
      async () => {
        const healthy = (await importSources(admin, 'comic', comicMocks.xkcd)).sources[0];
        const s502 = (await importSources(admin, 'comic', edgeMocks.error502)).sources[0];
        const sMalformed = (await importSources(admin, 'comic', edgeMocks.malformedJson)).sources[0];
        const sDelay = (await importSources(admin, 'comic', edgeMocks.delayTimeout)).sources[0];

        let hardenedMgr = null;
        try {
          // 1. Multi-source search with healthy + 502 + malformed + slow timeout
          const res = await member.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
            body: {
              keyword: 'test',
              source_ids: [healthy.id, s502.id, sMalformed.id, sDelay.id],
            },
            expectedStatus: 200,
            timeoutMs: 35000,
          });
          const items = res.json?.results || res.json?.items || [];
          const errors = res.json?.errors || [];
          assert.ok(items.some((i) => (i.title || '').includes('Mock XKCD Strip')));
          assert.ok(errors.length >= 2, `Expected isolated errors for 502/malformed/delay, got ${JSON.stringify(errors)}`);

          // 2. SVG image rejection on proxy
          const svgBlocked = await member.failLoudlyRequest(
            'GET',
            '/api/v1/comic/online/image-proxy',
            { query: { url: `${mockUrl}/edge/svg-as-image`, source_id: healthy.id } },
          );
          assert.ok(svgBlocked.status >= 400 && !svgBlocked.text.includes('<svg'));

          // 3. Hardened SSRF blocking check (if owning SUT)
          if (ownsSut) {
            hardenedMgr = new ServerMgr({
              xmediaBin: context.xmediaBin || null,
              webDir: context.webDir !== undefined ? context.webDir : resolveWebDir(),
              keepTmp: Boolean(context.keepTmp),
            });
            const hUrl = await hardenedMgr.start({ mode: 'hardened' });
            const { member: hMember } = await ensureBootstrapped(hUrl);
            for (const ssrfTarget of [
              'http://127.0.0.1:8090/secret.jpg',
              'http://localhost:8090/secret.jpg',
              'http://[::1]:8090/secret.jpg',
              'http://169.254.169.254/latest/meta-data/',
              'http://10.0.0.1/internal.jpg',
            ]) {
              const r = await hMember.failLoudlyRequest('GET', '/api/v1/comic/online/image-proxy', {
                query: { url: ssrfTarget },
              });
              assert.ok(
                [400, 403, 502].includes(r.status),
                `Hardened SSRF must block ${ssrfTarget}, got ${r.status}`,
              );
            }
          }
        } finally {
          if (hardenedMgr) await hardenedMgr.cleanup().catch(() => {});
          await deleteSourceSafe(admin, 'comic', healthy.id);
          await deleteSourceSafe(admin, 'comic', s502.id);
          await deleteSourceSafe(admin, 'comic', sMalformed.id);
          await deleteSourceSafe(admin, 'comic', sDelay.id);
        }
      },
    );

    // T4-07: Mixed Local Library + xmedia-book Online + New Domain Sources Coexistence Journey
    await runCase(
      'T4-07',
      ['nonreg', 'core', 'comic', 'music', 'video'],
      'Mixed Local Library + xmedia-book Online + OPDS/Subsonic + New Domain Sources Coexistence',
      async () => {
        const bSrc = (await importSources(admin, 'book', bookMocks.basicBook)).sources[0];
        const cSrc = (await importSources(admin, 'comic', comicMocks.xkcd)).sources[0];
        const mSrc = (await importSources(admin, 'music', musicMocks.gdstudio)).sources[0];
        const vSrc = (await importSources(admin, 'video', videoMocks.applecms)).sources[0];

        let bookId = null;
        let seriesId = null;
        let trackId = null;
        let videoId = null;
        try {
          // Upload 1 item per domain
          const bookForm = new FormData();
          bookForm.append(
            'file',
            new Blob([Buffer.from('Chapter 1\nLocal book non-regression content.\n', 'utf8')], {
              type: 'text/plain',
            }),
            'T4_Local_Book.txt',
          );
          const bUp = await admin.failLoudlyRequest('POST', '/api/v1/book/books/upload', {
            body: bookForm,
            expectedStatus: 201,
          });
          bookId = bUp.json?.uploaded?.[0]?.id || null;

          const cbzBuf = buildMinimalZip([{ name: '001.jpg', data: MOCK_JPEG_BYTES }]);
          const comicForm = new FormData();
          comicForm.append('file', new Blob([cbzBuf], { type: 'application/zip' }), 'T4_Local_Comic.cbz');
          const cUp = await admin.failLoudlyRequest('POST', '/api/v1/comic/upload', {
            body: comicForm,
            expectedStatus: 201,
          });
          seriesId = cUp.json?.uploaded?.[0]?.series_id || null;

          const musicForm = new FormData();
          musicForm.append('file', new Blob([MOCK_MP3_BYTES], { type: 'audio/mpeg' }), 't4_local.mp3');
          const mUp = await admin.failLoudlyRequest('POST', '/api/v1/music/upload', {
            body: musicForm,
            expectedStatus: 201,
          });
          trackId =
            mUp.json?.uploaded_tracks?.[0]?.id ||
            mUp.json?.tracks?.[0]?.id ||
            mUp.json?.uploaded?.[0]?.id ||
            null;
          assert.ok(trackId, 'Expected uploaded_tracks[0].id from /api/v1/music/upload');

          const videoForm = new FormData();
          videoForm.append('file', new Blob([MOCK_MP4_BYTES], { type: 'video/mp4' }), 't4_local.mp4');
          const vUp = await admin.failLoudlyRequest('POST', '/api/v1/video/upload', {
            body: videoForm,
            expectedStatus: 201,
          });
          videoId =
            vUp.json?.uploaded_videos?.[0]?.id ||
            vUp.json?.videos?.[0]?.id ||
            vUp.json?.uploaded?.[0]?.id ||
            null;
          assert.ok(videoId, 'Expected uploaded_videos[0].id from /api/v1/video/upload');

          // Verify legacy xmedia-book online search + OPDS + OpenSubsonic + new domains
          const bookOnlineSearch = await member.failLoudlyRequest(
            'POST',
            '/api/v1/book/online/search',
            { body: { keyword: 'Tower', source_ids: [bSrc.id] }, expectedStatus: 200 },
          );
          assert.ok((bookOnlineSearch.json?.items || []).length >= 1);

          const basicAuth = Buffer.from(`${MEMBER_CREDS.username}:${MEMBER_CREDS.password}`).toString('base64');
          const opdsBook = await member.failLoudlyRequest('GET', '/opds/book/v1.2', {
            authMode: 'none',
            headers: { Authorization: `Basic ${basicAuth}` },
            expectedStatus: 200,
          });
          assert.ok(opdsBook.text.includes('<feed'));

          const opdsComic = await member.failLoudlyRequest('GET', '/opds/comic/v1.2', {
            authMode: 'none',
            headers: { Authorization: `Basic ${basicAuth}` },
            expectedStatus: 200,
          });
          assert.ok(opdsComic.text.includes('<feed'));

          const subsonicPing = await member.failLoudlyRequest('GET', '/rest/ping.view', {
            authMode: 'none',
            query: { apiKey: member.apiKey, f: 'json' },
            expectedStatus: 200,
          });
          assert.equal(subsonicPing.json?.['subsonic-response']?.status, 'ok');

          const localStream = await member.failLoudlyRequest(
            'GET',
            `/api/v1/music/tracks/${trackId}/stream`,
            { expectedStatus: 200 },
          );
          assertMp3(localStream.buffer, 'Local MP3 stream');

          const localVid = await member.failLoudlyRequest(
            'GET',
            `/api/v1/video/videos/${videoId}/stream`,
            { expectedStatus: 200 },
          );
          assertMp4(localVid.buffer, 'Local MP4 stream');

          // New online domain searches still succeed cleanly
          await member.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
            body: { keyword: 'Mock', source_ids: [cSrc.id] },
            expectedStatus: 200,
          });
          await member.failLoudlyRequest('POST', '/api/v1/music/online/search', {
            body: { keyword: 'Mojito', source_ids: [mSrc.id] },
            expectedStatus: 200,
          });
          await member.failLoudlyRequest('POST', '/api/v1/video/online/search', {
            body: { keyword: 'Earth', source_ids: [vSrc.id] },
            expectedStatus: 200,
          });
        } finally {
          if (bookId) await admin.request('DELETE', `/api/v1/book/books/${bookId}`).catch(() => {});
          if (seriesId) await admin.request('DELETE', `/api/v1/comic/series/${seriesId}`).catch(() => {});
          if (trackId) await admin.request('DELETE', `/api/v1/music/tracks/${trackId}`).catch(() => {});
          if (videoId) await admin.request('DELETE', `/api/v1/video/videos/${videoId}`).catch(() => {});
          await deleteSourceSafe(admin, 'book', bSrc.id);
          await deleteSourceSafe(admin, 'comic', cSrc.id);
          await deleteSourceSafe(admin, 'music', mSrc.id);
          await deleteSourceSafe(admin, 'video', vSrc.id);
        }
      },
    );

    // T4-08: Production Seed Pack (seeds/sources/*.json) Import, Idempotency & Restart Verification
    await runCase(
      'T4-08',
      ['core', 'comic', 'music', 'video'],
      'Production Seed Pack (seeds/sources/{comic,music,video}.json) Import, Idempotency & Restart',
      async () => {
        const domains = ['comic', 'music', 'video'];
        const seedMeta = {};

        for (const domain of domains) {
          const seedPath = path.join(REPO_ROOT, 'seeds', 'sources', `${domain}.json`);
          assert.ok(
            existsSync(seedPath),
            `SUT_SEED_FILE_MISSING: Required seed file not found at ${seedPath}`,
          );
          const parsed = JSON.parse(readFileSync(seedPath, 'utf8'));
          assert.ok(Array.isArray(parsed) && parsed.length >= 2, `${seedPath} must contain >= 2 sources`);
          const enabledCount = parsed.filter((s) => s.enabled !== false).length;
          assert.ok(enabledCount >= 2, `${seedPath} must have >= 2 enabled sources`);

          const rawText = JSON.stringify(parsed);
          assert.ok(
            !/127\.0\.0\.1|localhost|169\.254\.169\.254|192\.168\.|10\.0\.0\./i.test(rawText),
            `${seedPath} contains private/loopback URL`,
          );
          seedMeta[domain] = parsed;
        }

        let localMgr = null;
        let targetUrl = baseUrl;
        try {
          if (ownsSut) {
            localMgr = new ServerMgr({
              xmediaBin: context.xmediaBin || null,
              webDir: context.webDir !== undefined ? context.webDir : resolveWebDir(),
              keepTmp: Boolean(context.keepTmp),
            });
            targetUrl = await localMgr.start({ mode: 'hardened' });
          }

          const { admin: sAdmin } = await ensureBootstrapped(targetUrl);

          const importedIds = { comic: [], music: [], video: [] };
          for (const domain of domains) {
            const imp1 = await sAdmin.failLoudlyRequest(
              'POST',
              `/api/v1/${domain}/sources/import-seeds`,
              { body: {}, expectedStatus: 200 },
            );
            assert.ok(
              (imp1.json?.sources || []).length >= seedMeta[domain].length,
              `Expected all ${domain} seeds imported`,
            );
            importedIds[domain] = imp1.json.sources.map((s) => s.id);

            // Second import-seeds MUST be idempotent
            const imp2 = await sAdmin.failLoudlyRequest(
              'POST',
              `/api/v1/${domain}/sources/import-seeds`,
              { body: {}, expectedStatus: 200 },
            );
            assert.equal(imp2.json?.imported_count, 0, `${domain} second import-seeds must have imported_count: 0`);
          }

          let postAdmin = sAdmin;
          if (localMgr) {
            targetUrl = await localMgr.restart({ mode: 'hardened' });
            postAdmin = new ApiClient(targetUrl, { authMode: 'bearer' });
            await postAdmin.login(ADMIN_CREDS);
          }

          for (const domain of domains) {
            const listRes = await postAdmin.failLoudlyRequest('GET', `/api/v1/${domain}/sources`, {
              expectedStatus: 200,
            });
            assert.ok(listRes.json.length >= seedMeta[domain].length);
          }

          // Verify Settings UI renders seeded sources cleanly
          await withCdpBrowser(async (cdp) => {
            await browserLogin(cdp, targetUrl, ADMIN_CREDS);
            for (const domain of domains) {
              await cdp.goto(`${targetUrl}/settings?tab=domain-datasources&domain=${domain}`);
              const firstName = seedMeta[domain][0].sourceName || seedMeta[domain][0].name;
              await cdp.waitForFn(
                `document.body.textContent.includes(${JSON.stringify(firstName)})`,
                { timeoutMs: 10000, label: `Seeded ${domain} source visible in Settings UI` },
              );
            }
            cdp.assertNoErrors('T4-08 Production Seed Pack Settings UI');
          });

          if (!localMgr) {
            for (const domain of domains) {
              for (const id of importedIds[domain]) {
                await deleteSourceSafe(postAdmin, domain, id);
              }
            }
          }
        } finally {
          if (localMgr) await localMgr.cleanup();
        }
      },
    );
  } finally {
    if (serverMgr) await serverMgr.cleanup().catch(() => {});
    await mock.stop().catch(() => {});
  }

  const passed = cases.filter((c) => c.status === 'pass').length;
  const failed = cases.filter((c) => c.status === 'fail').length;
  return {
    name: 'tier4',
    suite: 'tier4',
    total: cases.length,
    passed,
    failed,
    cases,
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const res = await runTier4();
  console.log(`\nTier 4 summary: ${res.passed}/${res.total} passed, ${res.failed} failed`);
  process.exitCode = res.failed > 0 || res.total === 0 ? 1 : 0;
}
