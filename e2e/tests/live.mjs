import assert from 'node:assert/strict';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { withCdpBrowser } from '../lib/cdp.mjs';

export function isAllowedExternalThumbError(url) {
  if (!url) return false;
  try {
    const pathname = new URL(url, 'http://localhost').pathname;
    return (
      pathname === '/api/v1/music/online/cover-proxy' ||
      pathname === '/api/v1/video/online/poster-proxy'
    );
  } catch {
    return false;
  }
}

export function isComicImageProxyUrl(url) {
  if (!url) return false;
  try {
    return new URL(url, 'http://localhost').pathname === '/api/v1/comic/online/image-proxy';
  } catch {
    return false;
  }
}

export function isAllowedLiveThumbHttpError(entry, preReaderComicReqIds) {
  if (!entry || ![400, 502, 504].includes(entry.status)) return false;
  if (isAllowedExternalThumbError(entry.url)) return true;
  return (
    isComicImageProxyUrl(entry.url) &&
    preReaderComicReqIds instanceof Set &&
    preReaderComicReqIds.has(entry.requestId)
  );
}
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

async function clickButtonByText(cdp, texts, { timeoutMs = 10000 } = {}) {
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

export async function runLiveTests(context = {}) {
  const silent = Boolean(context.silent);
  const domainFilter = context.domain || null;
  const ServerMgr = context.XMediaServerManager || XMediaServerManager;

  const ownsSut = !context.baseUrl;
  let serverMgr = null;
  let baseUrl = context.baseUrl ? context.baseUrl.replace(/\/+$/, '') : '';

  const reportDir =
    context.liveReport ||
    process.env.XMEDIA_E2E_REPORT_DIR ||
    path.join(os.tmpdir(), 'xmedia-e2e-live-report');

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
        const errors = [];
        for (const src of candidates) {
          const kw = src.name.includes('中文') ? '一拳超人' : 'Yotsuba';
          try {
            const search = await member.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
              body: { keyword: kw, source_ids: [src.id] },
              expectedStatus: 200,
              timeoutMs: 25000,
            });
            const items = (search.json?.results || search.json?.items || []).slice(0, 5);
            assert.ok(items.length >= 1, `Empty MangaDex search results for ${src.name} (${kw})`);
            for (const item of items) {
              try {
                const detail = await member.failLoudlyRequest('POST', '/api/v1/comic/online/detail', {
                  body: { source_id: src.id, comic_url: item.comic_url || item.book_url || item.url },
                  expectedStatus: 200,
                  timeoutMs: 25000,
                });
                const ch = (detail.json?.chapters || []).find((c) => c.chapter_url || c.url);
                if (!ch) {
                  errors.push(`${src.name}[${item.title}]: No non-external chapters`);
                  continue;
                }
                const pages = await member.failLoudlyRequest('POST', '/api/v1/comic/online/pages', {
                  body: { source_id: src.id, chapter_url: ch.chapter_url || ch.url },
                  expectedStatus: 200,
                  timeoutMs: 25000,
                });
                const firstPage = (pages.json?.pages || []).find((p) => p?.proxy_url);
                if (!firstPage) {
                  errors.push(`${src.name}[${item.title}]: No pages returned`);
                  continue;
                }
                const imgRes = await member.failLoudlyRequest(
                  'GET',
                  toRelativeApiPath(firstPage.proxy_url),
                  { expectedStatus: 200, timeoutMs: 25000 },
                );
                const mime = assertRasterImage(imgRes.buffer, null, 'LIVE-01 MangaDex page');
                assert.ok(imgRes.buffer.length > 1024, `Image too small: ${imgRes.buffer.length}`);
                assertSecurityHeaders(imgRes.headers, { label: 'LIVE-01 image-proxy' });
                liveComicSample = liveComicSample || {
                  sourceId: src.id,
                  keyword: kw,
                  title: item.title,
                };
                return { source: src.name, keyword: kw, mime, bytes: imgRes.buffer.length, title: item.title };
              } catch (candErr) {
                if (candErr instanceof SutEndpointMissingError) throw candErr;
                errors.push(`${src.name}[${item.title}]: ${candErr.message}`);
              }
            }
          } catch (err) {
            if (err instanceof SutEndpointMissingError) throw err;
            errors.push(`${src.name}: ${err.message}`);
          }
        }
        throw new Error(`All MangaDex candidates failed: ${errors.join(' | ')}`);
      }),
    );

    // LIVE-02: Comic — WEBTOON
    await runSourceCase('LIVE-02', 'comic', 'Comic — WEBTOON live search -> pages -> Referer image-proxy', async () =>
      withRetries(async () => {
        const candidates = findMatchingSources(seeded.comic, ['webtoon']);
        assert.ok(candidates.length >= 1, 'No WEBTOON seed found');
        const errors = [];
        for (const src of candidates) {
          const kw = src.name.includes('中文') || src.name.includes('繁體') ? '神之塔' : 'Tower of God';
          try {
            const search = await member.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
              body: { keyword: kw, source_ids: [src.id] },
              expectedStatus: 200,
              timeoutMs: 25000,
            });
            const items = (search.json?.results || search.json?.items || []).slice(0, 3);
            assert.ok(items.length >= 1, `Empty WEBTOON search results for ${src.name} (${kw})`);
            for (const item of items) {
              try {
                const detail = await member.failLoudlyRequest('POST', '/api/v1/comic/online/detail', {
                  body: { source_id: src.id, comic_url: item.comic_url || item.book_url || item.url },
                  expectedStatus: 200,
                  timeoutMs: 25000,
                });
                const ch = (detail.json?.chapters || []).find((c) => c.chapter_url || c.url);
                if (!ch) {
                  errors.push(`${src.name}[${item.title}]: No WEBTOON chapters`);
                  continue;
                }
                const pages = await member.failLoudlyRequest('POST', '/api/v1/comic/online/pages', {
                  body: { source_id: src.id, chapter_url: ch.chapter_url || ch.url },
                  expectedStatus: 200,
                  timeoutMs: 25000,
                });
                const firstPage = (pages.json?.pages || []).find((p) => p?.proxy_url);
                if (!firstPage) {
                  errors.push(`${src.name}[${item.title}]: No WEBTOON pages`);
                  continue;
                }
                const imgRes = await member.failLoudlyRequest(
                  'GET',
                  toRelativeApiPath(firstPage.proxy_url),
                  { expectedStatus: 200, timeoutMs: 25000 },
                );
                const mime = assertRasterImage(imgRes.buffer, null, 'LIVE-02 WEBTOON page');
                assert.ok(imgRes.buffer.length > 512, `WEBTOON image too small: ${imgRes.buffer.length}`);
                assertSecurityHeaders(imgRes.headers, { label: 'LIVE-02 image-proxy' });
                liveComicSample = liveComicSample || {
                  sourceId: src.id,
                  keyword: kw,
                  title: item.title,
                };
                return { source: src.name, keyword: kw, mime, bytes: imgRes.buffer.length, title: item.title };
              } catch (candErr) {
                if (candErr instanceof SutEndpointMissingError) throw candErr;
                errors.push(`${src.name}[${item.title}]: ${candErr.message}`);
              }
            }
          } catch (err) {
            if (err instanceof SutEndpointMissingError) throw err;
            errors.push(`${src.name}: ${err.message}`);
          }
        }
        throw new Error(`All WEBTOON candidates failed: ${errors.join(' | ')}`);
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
        assert.ok(imgRes.buffer.length > 512, `XKCD image too small: ${imgRes.buffer.length}`);
        assertSecurityHeaders(imgRes.headers, { label: 'LIVE-03 image-proxy' });
        liveComicSample = liveComicSample || {
          sourceId: src.id,
          keyword: 'xkcd',
          title: item.title,
        };
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
        assertSecurityHeaders(audRes.headers, { requireCsp: false, label: 'LIVE-04 audio-proxy' });

        assert.ok(info.json?.proxy_cover_url, 'Expected proxy_cover_url from GDStudio');
        const covRes = await member.failLoudlyRequest(
          'GET',
          toRelativeApiPath(info.json.proxy_cover_url),
          { expectedStatus: 200, timeoutMs: 25000 },
        );
        const coverMime = assertRasterImage(covRes.buffer, null, 'LIVE-04 GDStudio cover');
        assertSecurityHeaders(covRes.headers, { label: 'LIVE-04 cover-proxy' });
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

    // LIVE-06: Music — Internet Archive Audio AND iTunes+LRCLIB
    await runSourceCase('LIVE-06', 'music', 'Music — Internet Archive Audio & iTunes+LRCLIB live stream & lyrics', async () => {
      const archiveCandidates = findMatchingSources(seeded.music, ['archive']);
      assert.ok(archiveCandidates.length >= 1, 'No Internet Archive Audio seed found');
      const archiveSrc = archiveCandidates[0];

      const itunesCandidates = findMatchingSources(seeded.music, ['itunes', 'lrclib']);
      assert.ok(itunesCandidates.length >= 1, 'No iTunes Search + LRCLIB seed found');
      const itunesSrc = itunesCandidates[0];

      const archiveResult = await withRetries(
        async () => {
          const search = await member.failLoudlyRequest('POST', '/api/v1/music/online/search', {
            body: { keyword: 'classical', source_ids: [archiveSrc.id] },
            expectedStatus: 200,
            timeoutMs: 25000,
          });
          const tracks = (search.json?.results || search.json?.items || []).slice(0, 5);
          assert.ok(tracks.length >= 1, `No search results for ${archiveSrc.name}`);

          const errors = [];
          for (const tr of tracks) {
            try {
              const info = await member.failLoudlyRequest('POST', '/api/v1/music/online/track-info', {
                body: {
                  source_id: archiveSrc.id,
                  track_id: tr.track_id || tr.id,
                  detail_url: tr.detail_url,
                  play_url: tr.play_url,
                },
                expectedStatus: 200,
                timeoutMs: 25000,
              });
              assert.ok(info.json?.proxy_stream_url, `No proxy_stream_url for ${tr.title}`);
              const head = await fetchStreamHeadBytes(
                baseUrl,
                info.json.proxy_stream_url,
                member.token,
                4096,
                25000,
              );
              assertMp3(head.buffer, `LIVE-06 ${archiveSrc.name}`);
              assertSecurityHeaders(head.headers, { requireCsp: false, label: 'LIVE-06 archive audio-proxy' });
              return { source: archiveSrc.name, status: head.status, bytesRead: head.buffer.length, title: tr.title };
            } catch (err) {
              if (err instanceof SutEndpointMissingError) throw err;
              errors.push(`${tr.title}: ${err.message}`);
            }
          }
          throw new Error(`Internet Archive Audio candidates failed: ${errors.join(' | ')}`);
        },
        { attempts: 3, delayMs: 1000 },
      );

      const itunesResult = await withRetries(
        async () => {
          const kw = 'bohemian rhapsody';
          const search = await member.failLoudlyRequest('POST', '/api/v1/music/online/search', {
            body: { keyword: kw, source_ids: [itunesSrc.id] },
            expectedStatus: 200,
            timeoutMs: 25000,
          });
          const tr = (search.json?.results || search.json?.items || [])[0];
          assert.ok(tr, `No search results for ${itunesSrc.name}`);
          const info = await member.failLoudlyRequest('POST', '/api/v1/music/online/track-info', {
            body: {
              source_id: itunesSrc.id,
              track_id: tr.track_id || tr.id,
              detail_url: tr.detail_url,
              play_url: tr.play_url,
            },
            expectedStatus: 200,
            timeoutMs: 25000,
          });
          assert.ok(info.json?.proxy_stream_url, `No proxy_stream_url for ${itunesSrc.name}`);
          const head = await fetchStreamHeadBytes(
            baseUrl,
            info.json.proxy_stream_url,
            member.token,
            4096,
            25000,
          );
          assertMp4(head.buffer, `LIVE-06 ${itunesSrc.name} M4A`);
          assertSecurityHeaders(head.headers, { requireCsp: false, label: 'LIVE-06 itunes audio-proxy' });
          assert.ok(
            info.json?.lyrics && ((info.json.lyrics.lines || []).length > 0 || info.json.lyrics.plain_text),
            `Expected non-empty lyrics from ${itunesSrc.name}`,
          );
          liveMusicSample = liveMusicSample || { keyword: kw, title: tr.title };
          return {
            source: itunesSrc.name,
            status: head.status,
            bytesRead: head.buffer.length,
            lyricLines: info.json.lyrics.lines?.length || 0,
            hasPlainLyrics: Boolean(info.json.lyrics.plain_text),
            title: tr.title,
          };
        },
        { attempts: 3, delayMs: 1000 },
      );

      return { archive: archiveResult, itunes: itunesResult };
    });

    // LIVE-07: Video — AppleCMS v10 multi-source fallback (at least 1 node must yield valid HLS segments)
    await runSourceCase('LIVE-07', 'video', 'Video — AppleCMS v10 multi-node HLS -> hls-proxy segments', async () => {
      const candidates = findMatchingSources(seeded.video, ['ikun', 'suoni', 'yhzy', 'modu', 'applecms']);
      assert.ok(candidates.length >= 1, 'No AppleCMS video seeds found');
      const nodeErrors = [];
      for (const src of candidates) {
        try {
          return await withRetries(
            async () => {
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
              assert.equal(play.json?.stream_type, 'hls', `Expected stream_type=hls for ${src.name}`);
              assert.ok(play.json?.proxy_stream_url, 'No proxy_stream_url');

              // Follow master playlist -> media playlist -> first segment
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

              let encrypted = false;
              if (parsed.keys.length > 0) {
                encrypted = true;
                const keyRes = await member.failLoudlyRequest(
                  'GET',
                  toRelativeApiPath(parsed.keys[0]),
                  { expectedStatus: 200, timeoutMs: 25000 },
                );
                assert.equal(keyRes.buffer.length, 16, `Expected 16-byte AES key, got ${keyRes.buffer.length}`);
              }

              const tsRes = await member.failLoudlyRequest(
                'GET',
                toRelativeApiPath(parsed.segments[0]),
                { headers: { Range: 'bytes=0-1879' }, expectedStatus: [200, 206], timeoutMs: 25000 },
              );
              assert.ok(tsRes.buffer.length >= 188, `TS segment too small: ${tsRes.buffer.length}`);
              if (!encrypted) {
                assertMpegTs(tsRes.buffer, `LIVE-07 ${src.name} TS`, {
                  allowPartialTail: tsRes.status === 206 && tsRes.buffer.length < 1880,
                });
              }

              if (src.searchable !== false && !liveVideoSample) {
                liveVideoSample = { keyword: vod.title, title: vod.title };
              }
              return {
                source: src.name,
                title: vod.title,
                tsBytes: tsRes.buffer.length,
                encrypted,
                nodeWarnings: nodeErrors,
              };
            },
            { attempts: 2, delayMs: 800 },
          );
        } catch (err) {
          if (err instanceof SutEndpointMissingError) throw err;
          nodeErrors.push({ source: src.name, error: err.message });
        }
      }
      throw new Error(`All AppleCMS nodes failed: ${JSON.stringify(nodeErrors)}`);
    });

    // LIVE-08: Video — PeerTube framatube.org (handles both HLS fMP4/TS and direct MP4)
    await runSourceCase('LIVE-08', 'video', 'Video — PeerTube framatube.org live HLS/MP4 proxy Range', async () =>
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
        assert.ok(play.json?.proxy_stream_url, 'No PeerTube proxy_stream_url');

        const isHls =
          play.json?.stream_type === 'hls' || String(play.json.proxy_stream_url).includes('hls-proxy');
        if (isHls) {
          let m3u8Res = await member.failLoudlyRequest(
            'GET',
            toRelativeApiPath(play.json.proxy_stream_url),
            { expectedStatus: 200, timeoutMs: 25000 },
          );
          let parsed = assertM3u8Rewritten(m3u8Res.text, { expectedSourceId: src.id });
          if (m3u8Res.text.includes('#EXT-X-STREAM-INF')) {
            m3u8Res = await member.failLoudlyRequest(
              'GET',
              toRelativeApiPath(parsed.segments[0]),
              { expectedStatus: 200, timeoutMs: 25000 },
            );
            parsed = assertM3u8Rewritten(m3u8Res.text, { expectedSourceId: src.id });
          }
          const segTarget = parsed.maps[0] || parsed.segments[0];
          assert.ok(segTarget, 'PeerTube HLS playlist contained no map or segment URI');
          const segRes = await member.failLoudlyRequest(
            'GET',
            toRelativeApiPath(segTarget),
            { headers: { Range: 'bytes=0-4095' }, expectedStatus: [200, 206], timeoutMs: 25000 },
          );
          if (parsed.maps.length > 0) {
            assertMp4(segRes.buffer, 'LIVE-08 PeerTube fMP4', { allowFragment: true });
          } else {
            assertMpegTs(segRes.buffer, 'LIVE-08 PeerTube TS', {
              allowPartialTail: segRes.status === 206,
            });
          }
          liveVideoSample = liveVideoSample || { keyword: 'blender', title: vod.title };
          return {
            source: src.name,
            title: vod.title,
            streamType: 'hls',
            status: segRes.status,
            bytes: segRes.buffer.length,
          };
        }

        const mp4Res = await member.failLoudlyRequest(
          'GET',
          toRelativeApiPath(play.json.proxy_stream_url),
          { headers: { Range: 'bytes=0-4095' }, expectedStatus: [200, 206], timeoutMs: 25000 },
        );
        assertMp4(mp4Res.buffer, 'LIVE-08 PeerTube MP4');
        liveVideoSample = liveVideoSample || { keyword: 'blender', title: vod.title };
        return {
          source: src.name,
          title: vod.title,
          streamType: 'mp4',
          status: mp4Res.status,
          bytes: mp4Res.buffer.length,
        };
      }),
    );

    // LIVE-08B: Video — Internet Archive Video MP4 stream-proxy Range
    await runSourceCase('LIVE-08B', 'video', 'Video — Internet Archive live MP4 stream-proxy Range', async () =>
      withRetries(
        async () => {
          const candidates = findMatchingSources(seeded.video, ['internet archive', 'archive.org']);
          assert.ok(candidates.length >= 1, 'No Internet Archive Video seed found');
          const src = candidates[0];
          const kw = 'Big Buck Bunny';
          const search = await member.failLoudlyRequest('POST', '/api/v1/video/online/search', {
            body: { keyword: kw, source_ids: [src.id] },
            expectedStatus: 200,
            timeoutMs: 25000,
          });
          const vod = (search.json?.results || search.json?.items || [])[0];
          assert.ok(vod, 'Empty Internet Archive Video search results');
          const detail = await member.failLoudlyRequest('POST', '/api/v1/video/online/detail', {
            body: { source_id: src.id, video_id: vod.video_id || vod.id, detail_url: vod.detail_url },
            expectedStatus: 200,
            timeoutMs: 25000,
          });
          const ep = detail.json?.play_groups?.[0]?.episodes?.[0];
          assert.ok(ep?.play_url, 'No playable Internet Archive MP4 episode');
          const play = await member.failLoudlyRequest('POST', '/api/v1/video/online/play', {
            body: { source_id: src.id, play_url: ep.play_url },
            expectedStatus: 200,
            timeoutMs: 25000,
          });
          assert.equal(play.json?.stream_type, 'mp4', 'Expected stream_type=mp4 from Internet Archive Video');
          assert.ok(
            play.json?.proxy_stream_url?.includes('stream-proxy'),
            `Expected stream-proxy URL, got ${play.json?.proxy_stream_url}`,
          );
          const mp4Res = await member.failLoudlyRequest(
            'GET',
            toRelativeApiPath(play.json.proxy_stream_url),
            { headers: { Range: 'bytes=0-4095' }, expectedStatus: [200, 206], timeoutMs: 25000 },
          );
          assertMp4(mp4Res.buffer, 'LIVE-08B Internet Archive MP4');
          liveVideoSample = liveVideoSample || { keyword: kw, title: vod.title };
          return {
            source: src.name,
            title: vod.title,
            streamType: 'mp4',
            status: mp4Res.status,
            bytes: mp4Res.buffer.length,
          };
        },
        { attempts: 3, delayMs: 1000 },
      ),
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
          let preReaderComicReqIds = new Set();

          if (liveComicSample) {
            const sourceQuery = liveComicSample.sourceId
              ? `&source=${encodeURIComponent(liveComicSample.sourceId)}`
              : '';
            await cdp.goto(
              `${baseUrl}/comic/series?scope=online&q=${encodeURIComponent(liveComicSample.keyword)}${sourceQuery}`,
            );
            const waitForComicSearch = async (timeoutMs = 30000) => {
              const deadline = Date.now() + timeoutMs;
              while (Date.now() < deadline) {
                const state = await cdp.evalJs(`(() => {
                  const txt = document.body.textContent || '';
                  if (txt.includes(${JSON.stringify(liveComicSample.title)})) return 'found';
                  if (document.querySelector('[role="alert"]')) return 'alert';
                  return 'pending';
                })()`);
                if (state === 'found' || state === 'alert') return state;
                await new Promise((r) => setTimeout(r, 150));
              }
              throw new Error(`Timed out after ${timeoutMs}ms in LIVE-09 comic search result`);
            };
            const comicSearchState = await waitForComicSearch(30000);
            if (comicSearchState === 'alert') {
              summary.comicSearchRetried = true;
              const hasRetryBtn = await cdp.evalJs(
                `Boolean([...document.querySelectorAll('button')].find(b => ['重试', 'Retry'].includes((b.textContent || '').trim())))`,
              );
              if (hasRetryBtn) {
                await clickButtonByText(cdp, ['重试', 'Retry'], { timeoutMs: 5000 });
              } else {
                await cdp.click('button[type="submit"]');
              }
              await cdp.waitForFn(
                `document.body.textContent.includes(${JSON.stringify(liveComicSample.title)})`,
                { timeoutMs: 30000, label: 'LIVE-09 comic search retry result' },
              );
            }
            await clickButtonByText(cdp, [liveComicSample.title], { timeoutMs: 10000 });
            let detailState = await cdp.waitForFn(
              () => {
                if (!window.location.pathname.includes('/comic/item/online')) return false;
                const txt = document.body.textContent || '';
                if (txt.includes('开始阅读') || txt.includes('Start Reading')) return 'ready';
                if (txt.includes('重试') || txt.includes('Retry')) return 'error';
                return false;
              },
              { timeoutMs: 45000, label: 'LIVE-09 comic detail loaded' },
            );
            if (detailState === 'error') {
              const preRetryHttpCount = cdp.httpErrors.length;
              const preRetryConsoleCount = cdp.consoleErrors.length;
              await clickButtonByText(cdp, ['重试', 'Retry'], { timeoutMs: 10000 });
              detailState = await cdp.waitForFn(
                () => {
                  if (!window.location.pathname.includes('/comic/item/online')) return false;
                  const txt = document.body.textContent || '';
                  if (txt.includes('开始阅读') || txt.includes('Start Reading')) return 'ready';
                  if (txt.includes('重试') || txt.includes('Retry')) return 'error';
                  return false;
                },
                { timeoutMs: 45000, label: 'LIVE-09 comic detail retry loaded' },
              );
              assert.equal(
                detailState,
                'ready',
                'LIVE-09 comic detail remained in error state after retry',
              );
              summary.comicDetailRetried = true;
              const isDetailApiErr = (e) =>
                String(e?.url || '').includes('/api/v1/comic/online/detail');
              for (let i = preRetryHttpCount - 1; i >= 0; i -= 1) {
                if (isDetailApiErr(cdp.httpErrors[i])) cdp.httpErrors.splice(i, 1);
              }
              for (let i = preRetryConsoleCount - 1; i >= 0; i -= 1) {
                if (cdp.consoleErrors[i]?.source === 'network' && isDetailApiErr(cdp.consoleErrors[i])) {
                  cdp.consoleErrors.splice(i, 1);
                }
              }
            }
            assert.equal(detailState, 'ready', 'LIVE-09 comic detail did not reach ready state');
            preReaderComicReqIds = new Set(
              cdp.networkRequests
                .filter((r) => isComicImageProxyUrl(r.url))
                .map((r) => r.requestId),
            );
            await clickButtonByText(cdp, ['开始阅读', 'Start Reading'], {
              timeoutMs: 20000,
            });
            await cdp.waitForFn(
              `(() => {
                if (!window.location.pathname.includes('/comic/play/online')) return false;
                const imgs = [...document.querySelectorAll('img')].filter(i => (i.src || '').includes('image-proxy'));
                return imgs.length >= 1 && imgs.some(i => i.complete && i.naturalWidth > 0);
              })()`,
              { timeoutMs: 25000, label: 'LIVE-09 live comic image-proxy loaded' },
            );
            const readerImageResponses = cdp.networkResponses.filter(
              (r) => isComicImageProxyUrl(r.url) && !preReaderComicReqIds.has(r.requestId),
            );
            assert.ok(
              readerImageResponses.length >= 1,
              'Expected at least one opened reader /api/v1/comic/online/image-proxy response in LIVE-09',
            );
            assert.ok(
              readerImageResponses.some((r) => r.status === 200),
              `Expected HTTP 200 for opened comic reader image-proxy page image, got: ${JSON.stringify(
                readerImageResponses.map((r) => ({ status: r.status, url: r.url })),
              )}`,
            );
            assert.ok(
              !readerImageResponses.some((r) => r.status >= 400),
              `Unexpected error status on opened comic reader image-proxy request(s): ${JSON.stringify(
                readerImageResponses.filter((r) => r.status >= 400),
              )}`,
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
                return Boolean(a && a.error === null && a.src && a.src.includes('audio-proxy') && a.readyState >= 1);
              })()`,
              { timeoutMs: 25000, label: 'LIVE-09 live audio-proxy loaded (readyState >= 1)' },
            );
            assert.ok(
              cdp.networkResponses.some(
                (r) =>
                  r.url.includes('/api/v1/music/online/audio-proxy') &&
                  (r.status === 200 || r.status === 206),
              ),
              'Expected 200/206 response for /api/v1/music/online/audio-proxy in LIVE-09',
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
            await cdp.waitForFn(
              `window.location.pathname.includes('/video/item/online') && (document.body.textContent.includes('立即播放') || document.body.textContent.includes('Play Now'))`,
              { timeoutMs: 30000, label: 'LIVE-09 video detail loaded' },
            );
            await clickButtonByText(
              cdp,
              ['立即播放', 'Play Now'],
              { timeoutMs: 20000 },
            );
            await cdp.waitForFn(
              `(() => {
                if (!window.location.pathname.includes('/video/play/online')) return false;
                const v = document.querySelector('video');
                return Boolean(v && v.error === null && v.readyState >= 1 && (v.currentSrc || v.src));
              })()`,
              { timeoutMs: 25000, label: 'LIVE-09 live video element loaded' },
            );
            assert.ok(
              cdp.networkResponses.some(
                (r) =>
                  (r.url.includes('/api/v1/video/online/hls-proxy') ||
                    r.url.includes('/api/v1/video/online/stream-proxy')) &&
                  (r.status === 200 || r.status === 206),
              ),
              'Expected 200/206 response for video proxy in LIVE-09',
            );
            summary.video = liveVideoSample.title;
          }

          const { exceptions, consoleErrors, httpErrors } = cdp.getErrors();
          const fatalHttpErrors = httpErrors.filter(
            (e) => !isAllowedLiveThumbHttpError(e, preReaderComicReqIds),
          );
          const fatalHttpUrls = new Set(fatalHttpErrors.map((e) => e.url));
          const ignoredThumbUrls = new Set(
            httpErrors
              .filter((e) => isAllowedLiveThumbHttpError(e, preReaderComicReqIds))
              .map((e) => e.url),
          );
          const fatalConsoleErrors = consoleErrors.filter(
            (e) => !(e.source === 'network' && ignoredThumbUrls.has(e.url) && !fatalHttpUrls.has(e.url)),
          );
          const problems = [];
          if (exceptions.length > 0) problems.push(`exceptions=${JSON.stringify(exceptions)}`);
          if (fatalConsoleErrors.length > 0) {
            problems.push(`consoleErrors=${JSON.stringify(fatalConsoleErrors)}`);
          }
          if (fatalHttpErrors.length > 0) problems.push(`httpErrors=${JSON.stringify(fatalHttpErrors)}`);
          if (problems.length > 0) {
            throw new Error(`LIVE-09 Live Browser Smoke recorded unexpected browser errors: ${problems.join('; ')}`);
          }
          if (ignoredThumbUrls.size > 0) {
            summary.externalThumbnailWarnings = [...ignoredThumbUrls];
          }
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
      const escapeMdCell = (val) =>
        String(val ?? '')
          .replace(/\|/g, '\\|')
          .replace(/\r?\n/g, ' ');

      try {
        evidence.gate = {
          required_domains: requiredDomains,
          comic_passed: evidence.domains.comic.passed,
          music_passed: evidence.domains.music.passed,
          video_passed: evidence.domains.video.passed,
          domain_gate_ok: requiredDomains.every((d) => evidence.domains[d].passed >= 1),
        };
        assert.equal(
          evidence.gate.domain_gate_ok,
          true,
          `Domain gate failed: ${JSON.stringify(evidence.gate)}`,
        );

        mkdirSync(reportDir, { recursive: true });
        const jsonPath = path.join(reportDir, 'live_verification_report.json');
        const mdPath = path.join(reportDir, 'live_verification_report.md');
        const durationMs = Math.round(performance.now() - start);

        cases.push({
          id: 'LIVE-10',
          domain: 'core',
          title: `Evidence Artifact Generation (${reportDir})`,
          status: 'pass',
          durationMs,
          detail: { jsonPath, mdPath },
        });

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
          ...cases.map((c) => {
            const detailText = c.detail
              ? JSON.stringify(c.detail)
              : (c.error || '').split('\n')[0];
            return `| ${escapeMdCell(c.id)} | ${escapeMdCell(c.domain)} | ${escapeMdCell(c.status.toUpperCase())} | ${escapeMdCell(c.durationMs)} | ${escapeMdCell(detailText)} |`;
          }),
          '',
        ];
        writeFileSync(mdPath, mdLines.join('\n'), 'utf8');

        assert.ok(existsSync(jsonPath) && existsSync(mdPath), 'Missing live report output file');
        const writtenJson = JSON.parse(readFileSync(jsonPath, 'utf8'));
        assert.ok(
          Array.isArray(writtenJson.cases) && writtenJson.cases.some((c) => c.id === 'LIVE-10'),
          'Written JSON report missing LIVE-10 entry',
        );
        const writtenMd = readFileSync(mdPath, 'utf8');
        assert.ok(writtenMd.includes('| LIVE-10 |'), 'Written Markdown report missing LIVE-10 row');

        if (!silent) {
          console.log(`  [PASS] LIVE-10: Wrote ${jsonPath} and ${mdPath} (${durationMs}ms)`);
        }
      } catch (err) {
        const durationMs = Math.round(performance.now() - start);
        const existingIdx = cases.findIndex((c) => c.id === 'LIVE-10');
        const failEntry = {
          id: 'LIVE-10',
          domain: 'core',
          title: 'Evidence Artifact Generation',
          status: 'fail',
          durationMs,
          error: err.stack || err.message,
        };
        if (existingIdx >= 0) cases[existingIdx] = failEntry;
        else cases.push(failEntry);
        if (!silent) console.error(`  [FAIL] LIVE-10: ${err.message}`);
      }
    }
  } finally {
    if (serverMgr) await serverMgr.cleanup().catch(() => {});
  }

  const passed = cases.filter((c) => c.status === 'pass').length;
  const failed = cases.filter((c) => c.status === 'fail').length;
  const warned = cases.filter((c) => c.status === 'warn').length;
  return {
    name: 'live',
    suite: 'live',
    total: cases.length,
    passed,
    failed,
    warned,
    cases,
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const res = await runLiveTests();
  console.log(`\nLive summary: ${res.passed}/${res.total} passed, ${res.failed} failed, ${res.warned} warned`);
  process.exitCode = res.failed > 0 || res.total === 0 ? 1 : 0;
}
