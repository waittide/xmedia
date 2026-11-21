import assert from 'node:assert/strict';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { withCdpBrowser } from '../lib/cdp.mjs';
import {
  ApiClient,
  REPO_ROOT,
  SutEndpointMissingError,
  XMediaServerManager,
  resolveWebDir,
} from '../lib/client.mjs';
import {
  assertM3u8Rewritten,
  assertMp3,
  assertMp4,
  assertMpegTs,
  assertRasterImage,
  assertSecurityHeaders,
} from '../lib/fixtures.mjs';

const ADMIN_CREDS = { username: 'admin', display_name: 'E2E Live Admin', password: 'AdminPass123!' };
const MEMBER_CREDS = {
  username: 'live_member',
  display_name: 'E2E Live Member',
  password: 'UserPass123!',
  role: 'member',
};

async function withRetries(fn, { attempts = 2, delayMs = 800 } = {}) {
  let lastErr;
  for (let i = 0; i < attempts; i += 1) {
    try {
      return await fn(i + 1);
    } catch (err) {
      if (err instanceof SutEndpointMissingError) throw err;
      lastErr = err;
      if (i + 1 < attempts) {
        await new Promise((r) => setTimeout(r, delayMs));
      }
    }
  }
  throw lastErr;
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

function toRelativeApiPath(urlOrPath) {
  if (!urlOrPath) return '';
  if (urlOrPath.startsWith('http://') || urlOrPath.startsWith('https://')) {
    const u = new URL(urlOrPath);
    return `${u.pathname}${u.search}`;
  }
  return urlOrPath.startsWith('/') ? urlOrPath : `/${urlOrPath}`;
}

async function fetchStreamHeadBytes(baseUrl, relPath, token, maxBytes = 8192, timeoutMs = 20000) {
  const url = new URL(`${baseUrl.replace(/\/+$/, '')}${toRelativeApiPath(relPath)}`);
  const ac = new AbortController();
  const timer = setTimeout(() => ac.abort(), timeoutMs);
  try {
    const res = await fetch(url, {
      headers: { Authorization: `Bearer ${token}` },
      signal: ac.signal,
    });
    if (res.status === 404 || res.status === 405) {
      throw new SutEndpointMissingError('GET', url.pathname, res.status);
    }
    assert.ok(res.status === 200 || res.status === 206, `Expected 200/206, got ${res.status}`);
    const reader = res.body?.getReader();
    const chunks = [];
    let total = 0;
    if (reader) {
      while (total < maxBytes) {
        const { done, value } = await reader.read();
        if (done || !value) break;
        chunks.push(Buffer.from(value));
        total += value.length;
      }
      await reader.cancel().catch(() => {});
    }
    return {
      status: res.status,
      headers: res.headers,
      buffer: Buffer.concat(chunks).subarray(0, maxBytes),
    };
  } finally {
    clearTimeout(timer);
    ac.abort();
  }
}

function findMatchingSources(sources, namePatterns) {
  const pats = namePatterns.map((p) => p.toLowerCase());
  return sources.filter((s) => {
    const hay = `${s.name || ''} ${s.source_url || ''} ${s.group_name || ''}`.toLowerCase();
    return pats.some((p) => hay.includes(p));
  });
}

async function browserLogin(cdp, baseUrl, creds) {
  await cdp.goto(`${baseUrl}/login`);
  await cdp.waitForSelector('input[autocomplete="username"]', { visible: true });
  await cdp.type('input[autocomplete="username"]', creds.username);
  await cdp.type('input[autocomplete="current-password"]', creds.password);
  await cdp.click('button[type="submit"]');
  await cdp.waitForFn(() => !window.location.pathname.startsWith('/login'), {
    timeoutMs: 10000,
    label: 'browser redirect after login',
  });
}

async function clickButtonByText(cdp, texts, { timeoutMs = 10000 } = {}) {
  const list = Array.isArray(texts) ? texts : [texts];
  const clicked = await cdp.waitForFn(
    `(() => {
      const targets = ${JSON.stringify(list)}.map(s => s.toLowerCase());
      const els = [...document.querySelectorAll('button, [role="tab"], a')];
      for (const el of els) {
        const st = getComputedStyle(el);
        if (st.display === 'none' || st.visibility === 'hidden') continue;
        const r = el.getBoundingClientRect();
        if (r.width === 0 || r.height === 0) continue;
        const txt = (el.textContent || '').trim().toLowerCase();
        if (targets.some(t => txt.includes(t))) {
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

export async function runLiveTests(context = {}) {
  const silent = Boolean(context.silent);
  const domainFilter = context.domain || null;
  const ServerMgr = context.XMediaServerManager || XMediaServerManager;

  const ownsSut = !context.baseUrl;
  let serverMgr = null;
  let baseUrl = context.baseUrl ? context.baseUrl.replace(/\/+$/, '') : '';

  const reportDir =
    process.env.XMEDIA_E2E_REPORT_DIR || path.join(os.tmpdir(), 'xmedia-e2e-live-report');

  const cases = [];
  const evidence = {
    generated_at: new Date().toISOString(),
    mode: 'hardened (allow_private_ips=false)',
    report_dir: reportDir,
    domains: {
      comic: { passed: 0, results: [] },
      music: { passed: 0, results: [] },
      video: { passed: 0, results: [] },
    },
    browser_smoke: null,
    gate: null,
  };

  async function runSourceCase(id, domain, title, fn) {
    if (domainFilter && domainFilter !== domain && domainFilter !== 'ui') {
      return null;
    }
    const start = performance.now();
    try {
      const detail = await fn();
      const durationMs = Math.round(performance.now() - start);
      const entry = { id, domain, title, status: 'pass', durationMs, detail };
      cases.push(entry);
      if (evidence.domains[domain]) {
        evidence.domains[domain].passed += 1;
        evidence.domains[domain].results.push(entry);
      }
      if (!silent) {
        console.log(`  [PASS] ${id}: ${title} (${durationMs}ms) — ${JSON.stringify(detail)}`);
      }
      return entry;
    } catch (err) {
      if (err instanceof SutEndpointMissingError) {
        const durationMs = Math.round(performance.now() - start);
        const entry = { id, domain, title, status: 'fail', durationMs, error: err.message };
        cases.push(entry);
        throw err;
      }
      const durationMs = Math.round(performance.now() - start);
      const entry = {
        id,
        domain,
        title,
        status: 'warn',
        durationMs,
        error: err.stack || err.message,
      };
      cases.push(entry);
      if (evidence.domains[domain]) {
        evidence.domains[domain].results.push(entry);
      }
      if (!silent) {
        console.warn(`  [WARN] ${id}: ${title} (${durationMs}ms)\n    ${err.message}`);
      }
      return entry;
    }
  }

  try {
    if (ownsSut) {
      serverMgr = new ServerMgr({
        xmediaBin: context.xmediaBin || null,
        webDir: context.webDir !== undefined ? context.webDir : resolveWebDir(),
        keepTmp: Boolean(context.keepTmp),
      });
      baseUrl = await serverMgr.start({ mode: 'hardened' });
    }

    const { admin, member } = await ensureBootstrapped(baseUrl);

    // Import production seed packs from seeds/sources/{comic,music,video}.json
    const seeded = {};
    for (const domain of ['comic', 'music', 'video']) {
      const seedPath = path.join(REPO_ROOT, 'seeds', 'sources', `${domain}.json`);
      assert.ok(existsSync(seedPath), `Required seed file missing: ${seedPath}`);
      JSON.parse(readFileSync(seedPath, 'utf8'));

      await admin.failLoudlyRequest('POST', `/api/v1/${domain}/sources/import-seeds`, {
        body: {},
        expectedStatus: 200,
      });
      const listRes = await admin.failLoudlyRequest('GET', `/api/v1/${domain}/sources`, {
        expectedStatus: 200,
      });
      seeded[domain] = listRes.json || [];
      assert.ok(seeded[domain].length >= 1, `No ${domain} sources after import-seeds`);
    }

    let liveComicSample = null;
    let liveMusicSample = null;
    let liveVideoSample = null;

    // LIVE-01: Comic — MangaDex
    await runSourceCase('LIVE-01', 'comic', 'Comic — MangaDex live search -> pages -> image-proxy', async () =>
      withRetries(async () => {
        const candidates = findMatchingSources(seeded.comic, ['mangadex']);
        assert.ok(candidates.length >= 1, 'No MangaDex seed found');
        let lastErr = null;
        for (const src of candidates) {
          try {
            const search = await member.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
              body: { keyword: 'Naruto', source_ids: [src.id] },
              expectedStatus: 200,
              timeoutMs: 25000,
            });
            const item = (search.json?.results || search.json?.items || [])[0];
            assert.ok(item, 'Empty MangaDex search results');
            const detail = await member.failLoudlyRequest('POST', '/api/v1/comic/online/detail', {
              body: { source_id: src.id, comic_url: item.comic_url || item.book_url || item.url },
              expectedStatus: 200,
              timeoutMs: 25000,
            });
            const ch = (detail.json?.chapters || []).find((c) => c.chapter_url || c.url);
            assert.ok(ch, 'No chapters in MangaDex detail');
            const pages = await member.failLoudlyRequest('POST', '/api/v1/comic/online/pages', {
              body: { source_id: src.id, chapter_url: ch.chapter_url || ch.url },
              expectedStatus: 200,
              timeoutMs: 25000,
            });
            const firstPage = (pages.json?.pages || [])[0];
            assert.ok(firstPage?.proxy_url, 'No pages returned from MangaDex');
            const imgRes = await member.failLoudlyRequest(
              'GET',
              toRelativeApiPath(firstPage.proxy_url),
              { expectedStatus: 200, timeoutMs: 25000 },
            );
            const mime = assertRasterImage(imgRes.buffer, null, 'LIVE-01 MangaDex page');
            assert.ok(imgRes.buffer.length > 1024, `Image too small: ${imgRes.buffer.length}`);
            assertSecurityHeaders(imgRes.headers, { label: 'LIVE-01 image-proxy' });
            liveComicSample = liveComicSample || { keyword: 'Naruto', title: item.title };
            return { source: src.name, mime, bytes: imgRes.buffer.length, title: item.title };
          } catch (err) {
            if (err instanceof SutEndpointMissingError) throw err;
            lastErr = err;
          }
        }
        throw lastErr;
      }),
    );

    // LIVE-02: Comic — WEBTOON
    await runSourceCase('LIVE-02', 'comic', 'Comic — WEBTOON live search -> pages -> Referer image-proxy', async () =>
      withRetries(async () => {
        const candidates = findMatchingSources(seeded.comic, ['webtoon']);
        assert.ok(candidates.length >= 1, 'No WEBTOON seed found');
        let lastErr = null;
        for (const src of candidates) {
          try {
            const search = await member.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
              body: { keyword: 'Tower of God', source_ids: [src.id] },
              expectedStatus: 200,
              timeoutMs: 25000,
            });
            const item = (search.json?.results || search.json?.items || [])[0];
            assert.ok(item, 'Empty WEBTOON search results');
            const detail = await member.failLoudlyRequest('POST', '/api/v1/comic/online/detail', {
              body: { source_id: src.id, comic_url: item.comic_url || item.book_url || item.url },
              expectedStatus: 200,
              timeoutMs: 25000,
            });
            const ch = (detail.json?.chapters || [])[0];
            assert.ok(ch, 'No WEBTOON chapters');
            const pages = await member.failLoudlyRequest('POST', '/api/v1/comic/online/pages', {
              body: { source_id: src.id, chapter_url: ch.chapter_url || ch.url },
              expectedStatus: 200,
              timeoutMs: 25000,
            });
            const firstPage = (pages.json?.pages || [])[0];
            assert.ok(firstPage?.proxy_url, 'No WEBTOON pages');
            const imgRes = await member.failLoudlyRequest(
              'GET',
              toRelativeApiPath(firstPage.proxy_url),
              { expectedStatus: 200, timeoutMs: 25000 },
            );
            const mime = assertRasterImage(imgRes.buffer, null, 'LIVE-02 WEBTOON page');
            assert.ok(imgRes.buffer.length > 1024);
            liveComicSample = liveComicSample || { keyword: 'Tower of God', title: item.title };
            return { source: src.name, mime, bytes: imgRes.buffer.length, title: item.title };
          } catch (err) {
            if (err instanceof SutEndpointMissingError) throw err;
            lastErr = err;
          }
        }
        throw lastErr;
      }),
    );

    // LIVE-03: Comic — XKCD
    await runSourceCase('LIVE-03', 'comic', 'Comic — XKCD live explore -> image-proxy PNG', async () =>
      withRetries(async () => {
        const candidates = findMatchingSources(seeded.comic, ['xkcd']);
        assert.ok(candidates.length >= 1, 'No XKCD seed found');
        const src = candidates[0];
        const explore = await member.failLoudlyRequest('POST', '/api/v1/comic/online/explore', {
          body: { source_id: src.id },
          expectedStatus: 200,
          timeoutMs: 25000,
        });
        const item = (explore.json?.items || explore.json?.results || [])[0];
        assert.ok(item, 'Empty XKCD explore items');
        const detail = await member.failLoudlyRequest('POST', '/api/v1/comic/online/detail', {
          body: { source_id: src.id, comic_url: item.comic_url || item.book_url || item.url },
          expectedStatus: 200,
          timeoutMs: 25000,
        });
        const ch = (detail.json?.chapters || [])[0];
        assert.ok(ch, 'No XKCD chapter');
        const pages = await member.failLoudlyRequest('POST', '/api/v1/comic/online/pages', {
          body: { source_id: src.id, chapter_url: ch.chapter_url || ch.url },
          expectedStatus: 200,
          timeoutMs: 25000,
        });
        const firstPage = (pages.json?.pages || [])[0];
        assert.ok(firstPage?.proxy_url);
        const imgRes = await member.failLoudlyRequest(
          'GET',
          toRelativeApiPath(firstPage.proxy_url),
          { expectedStatus: 200, timeoutMs: 25000 },
        );
        const mime = assertRasterImage(imgRes.buffer, null, 'LIVE-03 XKCD strip');
        liveComicSample = liveComicSample || { keyword: 'xkcd', title: item.title };
        return { source: src.name, mime, bytes: imgRes.buffer.length, title: item.title };
      }),
    );

    // LIVE-04: Music — GDStudio netease
    await runSourceCase('LIVE-04', 'music', 'Music — GDStudio live track-info + 206 audio-proxy + cover-proxy + lyrics', async () =>
      withRetries(async () => {
        const candidates = findMatchingSources(seeded.music, ['gdstudio']);
        assert.ok(candidates.length >= 1, 'No GDStudio seed found');
        const src = candidates[0];
        const search = await member.failLoudlyRequest('POST', '/api/v1/music/online/search', {
          body: { keyword: 'Mojito', source_ids: [src.id] },
          expectedStatus: 200,
          timeoutMs: 25000,
        });
        const tr = (search.json?.results || search.json?.items || [])[0];
        assert.ok(tr, 'Empty GDStudio search results');
        const info = await member.failLoudlyRequest('POST', '/api/v1/music/online/track-info', {
          body: {
            source_id: src.id,
            track_id: tr.track_id || tr.id,
            url_id: tr.url_id,
            lyric_id: tr.lyric_id,
            cover_id: tr.cover_id,
            quality: '320',
          },
          expectedStatus: 200,
          timeoutMs: 25000,
        });
        assert.ok(info.json?.proxy_stream_url, 'No proxy_stream_url');
        const audRes = await member.failLoudlyRequest(
          'GET',
          toRelativeApiPath(info.json.proxy_stream_url),
          { headers: { Range: 'bytes=0-4095' }, expectedStatus: [200, 206], timeoutMs: 25000 },
        );
        assertMp3(audRes.buffer, 'LIVE-04 GDStudio audio');

        let coverMime = null;
        if (info.json?.proxy_cover_url) {
          const covRes = await member.failLoudlyRequest(
            'GET',
            toRelativeApiPath(info.json.proxy_cover_url),
            { expectedStatus: 200, timeoutMs: 25000 },
          );
          coverMime = assertRasterImage(covRes.buffer, null, 'LIVE-04 GDStudio cover');
        }
        assert.ok(
          info.json?.lyrics && ((info.json.lyrics.lines || []).length > 0 || info.json.lyrics.plain_text),
          'Expected non-empty lyrics from GDStudio',
        );
        liveMusicSample = liveMusicSample || { keyword: 'Mojito', title: tr.title || tr.name };
        return {
          source: src.name,
          status: audRes.status,
          audioBytes: audRes.buffer.length,
          coverMime,
          lyricLines: info.json.lyrics.lines?.length || 0,
        };
      }),
    );

    // LIVE-05: Music — Audius
    await runSourceCase('LIVE-05', 'music', 'Music — Audius trending -> audio-proxy stream chunk', async () =>
      withRetries(async () => {
        const candidates = findMatchingSources(seeded.music, ['audius']);
        assert.ok(candidates.length >= 1, 'No Audius seed found');
        const src = candidates[0];
        const explore = await member.failLoudlyRequest('POST', '/api/v1/music/online/explore', {
          body: { source_id: src.id },
          expectedStatus: 200,
          timeoutMs: 25000,
        });
        const tr = (explore.json?.results || explore.json?.items || [])[0];
        assert.ok(tr, 'Empty Audius trending tracks');
        const stream = await member.failLoudlyRequest('POST', '/api/v1/music/online/stream', {
          body: { source_id: src.id, track_id: tr.track_id || tr.id, play_url: tr.play_url },
          expectedStatus: 200,
          timeoutMs: 25000,
        });
        assert.ok(stream.json?.proxy_stream_url);
        const head = await fetchStreamHeadBytes(
          baseUrl,
          stream.json.proxy_stream_url,
          member.token,
          8192,
          25000,
        );
        assertMp3(head.buffer, 'LIVE-05 Audius stream head');
        liveMusicSample = liveMusicSample || { keyword: tr.title || 'trending', title: tr.title };
        return { source: src.name, status: head.status, bytesRead: head.buffer.length, title: tr.title };
      }),
    );

    // LIVE-06: Music — Internet Archive / iTunes+LRCLIB
    await runSourceCase('LIVE-06', 'music', 'Music — Internet Archive / iTunes+LRCLIB live stream & lyrics', async () =>
      withRetries(async () => {
        const candidates = findMatchingSources(seeded.music, ['archive', 'itunes', 'lrclib']);
        assert.ok(candidates.length >= 1, 'No Internet Archive / iTunes seed found');
        let lastErr = null;
        for (const src of candidates) {
          try {
            const kw = src.name.toLowerCase().includes('itunes') ? 'bohemian rhapsody' : 'classical';
            const search = await member.failLoudlyRequest('POST', '/api/v1/music/online/search', {
              body: { keyword: kw, source_ids: [src.id] },
              expectedStatus: 200,
              timeoutMs: 25000,
            });
            const tr = (search.json?.results || search.json?.items || [])[0];
            assert.ok(tr, `No search results for ${src.name}`);
            const info = await member.failLoudlyRequest('POST', '/api/v1/music/online/track-info', {
              body: {
                source_id: src.id,
                track_id: tr.track_id || tr.id,
                detail_url: tr.detail_url,
                play_url: tr.play_url,
              },
              expectedStatus: 200,
              timeoutMs: 25000,
            });
            assert.ok(info.json?.proxy_stream_url);
            const head = await fetchStreamHeadBytes(
              baseUrl,
              info.json.proxy_stream_url,
              member.token,
              4096,
              25000,
            );
            assert.ok(head.buffer.length > 512, 'Expected non-empty audio stream');
            liveMusicSample = liveMusicSample || { keyword: kw, title: tr.title };
            return { source: src.name, status: head.status, bytesRead: head.buffer.length, title: tr.title };
          } catch (err) {
            if (err instanceof SutEndpointMissingError) throw err;
            lastErr = err;
          }
        }
        throw lastErr;
      }),
    );

    // LIVE-07: Video — AppleCMS v10 multi-source fallback (at least 1 node must yield 0x47 MPEG-TS bytes)
    await runSourceCase('LIVE-07', 'video', 'Video — AppleCMS v10 multi-node HLS -> hls-proxy 0x47 MPEG-TS', async () => {
      const candidates = findMatchingSources(seeded.video, ['ikun', 'suoni', 'yhzy', 'modu', 'applecms']);
      assert.ok(candidates.length >= 1, 'No AppleCMS video seeds found');
      const nodeErrors = [];
      for (const src of candidates) {
        try {
          const browse = await member.failLoudlyRequest('POST', '/api/v1/video/online/browse', {
            body: { source_id: src.id, page: 1 },
            expectedStatus: 200,
            timeoutMs: 25000,
          });
          const vod = (browse.json?.results || browse.json?.items || [])[0];
          assert.ok(vod, 'Empty AppleCMS browse list');
          const detail = await member.failLoudlyRequest('POST', '/api/v1/video/online/detail', {
            body: { source_id: src.id, video_id: vod.video_id || vod.id, detail_url: vod.detail_url },
            expectedStatus: 200,
            timeoutMs: 25000,
          });
          const ep = detail.json?.play_groups?.[0]?.episodes?.[0];
          assert.ok(ep?.play_url, 'No playable AppleCMS episode');
          const play = await member.failLoudlyRequest('POST', '/api/v1/video/online/play', {
            body: { source_id: src.id, play_url: ep.play_url },
            expectedStatus: 200,
            timeoutMs: 25000,
          });
          assert.ok(play.json?.proxy_stream_url, 'No proxy_stream_url');

          // Follow master playlist -> media playlist -> first .ts segment
          let playlistUrl = toRelativeApiPath(play.json.proxy_stream_url);
          let m3u8Res = await member.failLoudlyRequest('GET', playlistUrl, {
            expectedStatus: 200,
            timeoutMs: 25000,
          });
          let parsed = assertM3u8Rewritten(m3u8Res.text, { expectedSourceId: src.id });
          if (m3u8Res.text.includes('#EXT-X-STREAM-INF')) {
            m3u8Res = await member.failLoudlyRequest(
              'GET',
              toRelativeApiPath(parsed.segments[0]),
              { expectedStatus: 200, timeoutMs: 25000 },
            );
            parsed = assertM3u8Rewritten(m3u8Res.text, { expectedSourceId: src.id });
          }

          const tsRes = await member.failLoudlyRequest(
            'GET',
            toRelativeApiPath(parsed.segments[0]),
            { headers: { Range: 'bytes=0-2047' }, expectedStatus: [200, 206], timeoutMs: 25000 },
          );
          const alignedLen = Math.floor(tsRes.buffer.length / 188) * 188;
          assert.ok(alignedLen >= 188, `TS segment too small: ${tsRes.buffer.length}`);
          assertMpegTs(tsRes.buffer.subarray(0, alignedLen), `LIVE-07 ${src.name} TS`);
          liveVideoSample = liveVideoSample || { keyword: vod.title, title: vod.title };
          return {
            source: src.name,
            title: vod.title,
            tsBytes: tsRes.buffer.length,
            nodeWarnings: nodeErrors,
          };
        } catch (err) {
          if (err instanceof SutEndpointMissingError) throw err;
          nodeErrors.push({ source: src.name, error: err.message });
        }
      }
      throw new Error(`All AppleCMS nodes failed: ${JSON.stringify(nodeErrors)}`);
    });

    // LIVE-08: Video — PeerTube framatube.org
    await runSourceCase('LIVE-08', 'video', 'Video — PeerTube framatube.org live MP4 stream-proxy Range', async () =>
      withRetries(async () => {
        const candidates = findMatchingSources(seeded.video, ['peertube', 'framatube']);
        assert.ok(candidates.length >= 1, 'No PeerTube seed found');
        const src = candidates[0];
        const search = await member.failLoudlyRequest('POST', '/api/v1/video/online/search', {
          body: { keyword: 'blender', source_ids: [src.id] },
          expectedStatus: 200,
          timeoutMs: 25000,
        });
        const vod = (search.json?.results || search.json?.items || [])[0];
        assert.ok(vod, 'Empty PeerTube search results');
        const detail = await member.failLoudlyRequest('POST', '/api/v1/video/online/detail', {
          body: { source_id: src.id, video_id: vod.video_id || vod.id, detail_url: vod.detail_url },
          expectedStatus: 200,
          timeoutMs: 25000,
        });
        const ep = detail.json?.play_groups?.[0]?.episodes?.[0];
        assert.ok(ep?.play_url, 'No PeerTube playable file');
        const play = await member.failLoudlyRequest('POST', '/api/v1/video/online/play', {
          body: { source_id: src.id, play_url: ep.play_url },
          expectedStatus: 200,
          timeoutMs: 25000,
        });
        const mp4Res = await member.failLoudlyRequest(
          'GET',
          toRelativeApiPath(play.json.proxy_stream_url),
          { headers: { Range: 'bytes=0-4095' }, expectedStatus: [200, 206], timeoutMs: 25000 },
        );
        assertMp4(mp4Res.buffer, 'LIVE-08 PeerTube MP4');
        liveVideoSample = liveVideoSample || { keyword: 'blender', title: vod.title };
        return { source: src.name, title: vod.title, status: mp4Res.status, bytes: mp4Res.buffer.length };
      }),
    );

    // Evaluate domain gates before LIVE-09 / LIVE-10
    const requiredDomains = domainFilter && ['comic', 'music', 'video'].includes(domainFilter)
      ? [domainFilter]
      : ['comic', 'music', 'video'];
    for (const d of requiredDomains) {
      if (evidence.domains[d].passed === 0) {
        for (const c of cases) {
          if (c.domain === d && c.status === 'warn') {
            c.status = 'fail';
          }
        }
      }
    }

    // LIVE-09: Live Browser Smoke
    {
      const start = performance.now();
      try {
        assert.ok(
          liveComicSample || liveMusicSample || liveVideoSample,
          'LIVE-09 requires at least one verified live comic/music/video sample',
        );
        await withCdpBrowser(async (cdp) => {
          await browserLogin(cdp, baseUrl, MEMBER_CREDS);
          const summary = {};

          if (liveComicSample) {
            await cdp.goto(
              `${baseUrl}/comic/series?scope=online&q=${encodeURIComponent(liveComicSample.keyword)}`,
            );
            await cdp.waitForFn(
              `document.body.textContent.includes(${JSON.stringify(liveComicSample.title)})`,
              { timeoutMs: 25000, label: 'LIVE-09 comic search result' },
            );
            await clickButtonByText(cdp, [liveComicSample.title], { timeoutMs: 10000 });
            await cdp.waitForFn(
              `document.body.textContent.includes('阅读') || document.body.textContent.toLowerCase().includes('read') || document.querySelectorAll('a, button').length > 0`,
              { timeoutMs: 15000, label: 'LIVE-09 comic detail loaded' },
            );
            await clickButtonByText(cdp, ['开始阅读', 'Read', 'Ch.', 'Ep.', '第'], {
              timeoutMs: 10000,
            }).catch(() => {});
            await cdp.waitForFn(
              `(() => {
                const imgs = [...document.querySelectorAll('img')].filter(i => (i.src || '').includes('image-proxy'));
                return imgs.length >= 1 && imgs.some(i => i.complete && i.naturalWidth > 0);
              })()`,
              { timeoutMs: 25000, label: 'LIVE-09 live comic image-proxy loaded' },
            );
            summary.comic = liveComicSample.title;
          }

          if (liveMusicSample) {
            await cdp.goto(
              `${baseUrl}/music/songs?scope=online&q=${encodeURIComponent(liveMusicSample.keyword)}`,
            );
            await cdp.waitForFn(
              `document.body.textContent.includes(${JSON.stringify(liveMusicSample.title)})`,
              { timeoutMs: 25000, label: 'LIVE-09 music search result' },
            );
            await clickButtonByText(cdp, [liveMusicSample.title], { timeoutMs: 10000 });
            await cdp.waitForFn(
              `(() => {
                const a = document.querySelector('audio');
                return Boolean(a && a.src && a.src.includes('audio-proxy'));
              })()`,
              { timeoutMs: 20000, label: 'LIVE-09 live audio-proxy loaded' },
            );
            summary.music = liveMusicSample.title;
          }

          if (liveVideoSample) {
            await cdp.goto(
              `${baseUrl}/video/catalog?scope=online&q=${encodeURIComponent(liveVideoSample.keyword)}`,
            );
            await cdp.waitForFn(
              `document.body.textContent.includes(${JSON.stringify(liveVideoSample.title)})`,
              { timeoutMs: 25000, label: 'LIVE-09 video search result' },
            );
            await clickButtonByText(cdp, [liveVideoSample.title], { timeoutMs: 10000 });
            await clickButtonByText(cdp, ['播放', 'Play', '第1集', '正片', 'HD', '360'], {
              timeoutMs: 10000,
            }).catch(() => {});
            await cdp.waitForFn(
              `(() => {
                const v = document.querySelector('video');
                return Boolean(v && v.error === null);
              })()`,
              { timeoutMs: 20000, label: 'LIVE-09 live video element loaded' },
            );
            summary.video = liveVideoSample.title;
          }

          cdp.assertNoErrors('LIVE-09 Live Browser Smoke');
          evidence.browser_smoke = summary;
        });

        const durationMs = Math.round(performance.now() - start);
        cases.push({
          id: 'LIVE-09',
          domain: 'ui',
          title: 'Live Browser Smoke (Chromium comic/music/video online UI)',
          status: 'pass',
          durationMs,
          detail: evidence.browser_smoke,
        });
        if (!silent) console.log(`  [PASS] LIVE-09: Live Browser Smoke (${durationMs}ms)`);
      } catch (err) {
        const durationMs = Math.round(performance.now() - start);
        cases.push({
          id: 'LIVE-09',
          domain: 'ui',
          title: 'Live Browser Smoke (Chromium comic/music/video online UI)',
          status: 'fail',
          durationMs,
          error: err.stack || err.message,
        });
        if (!silent) console.error(`  [FAIL] LIVE-09: ${err.message}`);
      }
    }

    // LIVE-10: Evidence Artifact Generation
    {
      const start = performance.now();
      try {
        evidence.gate = {
          required_domains: requiredDomains,
          comic_passed: evidence.domains.comic.passed,
          music_passed: evidence.domains.music.passed,
          video_passed: evidence.domains.video.passed,
          domain_gate_ok: requiredDomains.every((d) => evidence.domains[d].passed >= 1),
        };
        mkdirSync(reportDir, { recursive: true });
        const jsonPath = path.join(reportDir, 'live_verification_report.json');
        const mdPath = path.join(reportDir, 'live_verification_report.md');

        writeFileSync(jsonPath, JSON.stringify({ ...evidence, cases }, null, 2), 'utf8');

        const mdLines = [
          '# xmedia Live Source Verification Report',
          '',
          `- Generated at: \`${evidence.generated_at}\``,
          `- Mode: \`${evidence.mode}\``,
          `- Domain gate: **${evidence.gate.domain_gate_ok ? 'PASS' : 'FAIL'}** (Comic=${evidence.domains.comic.passed}, Music=${evidence.domains.music.passed}, Video=${evidence.domains.video.passed})`,
          '',
          '## Case Summary',
          '',
          '| ID | Domain | Status | Duration (ms) | Detail / Error |',
          '|---|---|---|---|---|',
          ...cases.map(
            (c) =>
              `| ${c.id} | ${c.domain} | ${c.status.toUpperCase()} | ${c.durationMs} | ${
                c.detail ? JSON.stringify(c.detail) : (c.error || '').split('\n')[0]
              } |`,
          ),
          '',
        ];
        writeFileSync(mdPath, mdLines.join('\n'), 'utf8');

        assert.ok(existsSync(jsonPath) && existsSync(mdPath));
        const durationMs = Math.round(performance.now() - start);
        cases.push({
          id: 'LIVE-10',
          domain: 'core',
          title: `Evidence Artifact Generation (${reportDir})`,
          status: 'pass',
          durationMs,
          detail: { jsonPath, mdPath },
        });
        if (!silent) {
          console.log(`  [PASS] LIVE-10: Wrote ${jsonPath} and ${mdPath} (${durationMs}ms)`);
        }
      } catch (err) {
        const durationMs = Math.round(performance.now() - start);
        cases.push({
          id: 'LIVE-10',
          domain: 'core',
          title: 'Evidence Artifact Generation',
          status: 'fail',
          durationMs,
          error: err.stack || err.message,
        });
        if (!silent) console.error(`  [FAIL] LIVE-10: ${err.message}`);
      }
    }
  } finally {
    if (serverMgr) await serverMgr.cleanup().catch(() => {});
  }

  const passed = cases.filter((c) => c.status === 'pass').length;
  const failed = cases.filter((c) => c.status === 'fail').length;
  return {
    name: 'live',
    total: cases.length,
    passed,
    failed,
    cases,
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const res = await runLiveTests();
  console.log(`\nLive summary: ${res.passed}/${res.total} passed, ${res.failed} failed`);
  process.exitCode = res.failed > 0 || res.total === 0 ? 1 : 0;
}
