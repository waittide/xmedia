import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import {
  buildEdgeMockSources,
  buildMockComicSources,
  buildMockMusicSources,
  buildMockVideoSources,
} from '../fixtures/mock_sources.mjs';
import { withCdpBrowser } from '../lib/cdp.mjs';
import {
  ApiClient,
  SutEndpointMissingError,
  XMediaServerManager,
  resolveWebDir,
} from '../lib/client.mjs';
import {
  MOCK_AES_KEY_BYTES,
  MOCK_FMP4_INIT,
  MOCK_JPEG_BYTES,
  MOCK_MP3_BYTES,
  MOCK_MP4_BYTES,
  assertJpeg,
  assertM3u8Rewritten,
  assertMp3,
  assertMp4,
  assertMpegTs,
  assertPng,
  assertSecurityHeaders,
  decryptAes128Cbc,
} from '../lib/fixtures.mjs';
import {
  browserLogin,
  buildMinimalZip,
  clickButtonByText,
  clickButtonMatching,
  deleteSourceSafe,
  ensureBootstrapped as sharedEnsureBootstrapped,
  getMockRequests,
  importSource,
  resetMockRequests,
  toRelativeApiPath,
} from '../lib/helpers.mjs';
import { MockOriginServer } from '../lib/mock_origin.mjs';

const ADMIN_CREDS = { username: 'admin', display_name: 'E2E Admin', password: 'AdminPass123!' };
const MEMBER_CREDS = { username: 'e2e_member', display_name: 'E2E Member', password: 'UserPass123!', role: 'member' };

function ensureBootstrapped(baseUrl) {
  return sharedEnsureBootstrapped(baseUrl, ADMIN_CREDS, MEMBER_CREDS);
}

export async function runTier3(context = {}) {
  const silent = Boolean(context.silent);
  const domainFilter = context.domain || null;
  const MockSrv = context.MockOriginServer || MockOriginServer;
  const ServerMgr = context.XMediaServerManager || XMediaServerManager;

  const mock = new MockSrv();
  const mockUrl = await mock.start();

  const ownsSut = !context.baseUrl;
  let serverMgr = null;
  let hardenedMgr = null;
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
      const durationMs = 0;
      const errMsg = err.stack || err.message;
      if (!silent) {
        console.error(`  [FAIL] T3-BOOTSTRAP: ${err.message}`);
      }
      return {
        name: 'tier3',
        suite: 'tier3',
        total: 1,
        passed: 0,
        failed: 1,
        cases: [
          {
            id: 'T3-BOOTSTRAP',
            domains: ['core'],
            title: 'Tier 3 SUT bootstrap & auth setup',
            status: 'fail',
            durationMs,
            error: errMsg,
          },
        ],
      };
    }

    const comicMocks = buildMockComicSources(mockUrl, { strictUa: true });
    const musicMocks = buildMockMusicSources(mockUrl);
    const videoMocks = buildMockVideoSources(mockUrl);
    const edgeMocks = buildEdgeMockSources(mockUrl);

    // T3-01: Cookie x User/Admin x comic plugins/import strict isolation
    await runCase(
      'T3-01',
      ['core', 'comic'],
      'Cookie x User/Admin x comic plugins/import per-user isolation',
      async () => {
        const userCookie = member.withAuthMode('cookie');
        const adminCookie = admin.withAuthMode('cookie');

        const adminPlugin = await importSource(adminCookie, 'comic', comicMocks.xkcd);
        let memberPlugin = null;
        try {
          assert.equal(adminPlugin.name, 'Mock XKCD');
          await userCookie.failLoudlyRequest('GET', `/api/v1/plugins/${adminPlugin.id}`, {
            expectedStatus: 404,
          });
          memberPlugin = await importSource(userCookie, 'comic', comicMocks.xkcd);
          assert.notEqual(memberPlugin.id, adminPlugin.id, 'User and Admin plugins must be isolated');
        } finally {
          await deleteSourceSafe(admin, 'comic', adminPlugin.id);
          if (memberPlugin) await deleteSourceSafe(member, 'comic', memberPlugin.id);
        }
      },
    );

    // T3-02: Bearer x User x music x online/search (enabled + disabled sources)
    await runCase(
      'T3-02',
      ['core', 'music'],
      'Bearer x User x music online/search excludes disabled plugins',
      async () => {
        const disabledAudius = {
          ...musicMocks.audius,
          sourceName: 'Disabled Audius T3',
          sourceUrl: `${mockUrl}/music/audius-disabled-t3`,
          enabled: false,
        };
        const sEnabled = await importSource(member, 'music', musicMocks.gdstudio);
        const sDisabled = await importSource(member, 'music', disabledAudius);
        try {
          await resetMockRequests(mockUrl);
          const userBearer = member.withAuthMode('bearer');
          const res = await userBearer.failLoudlyRequest('POST', '/api/v1/music/online/search', {
            body: { keyword: 'Mojito' },
            expectedStatus: 200,
          });
          const items = res.json?.results || res.json?.items || [];
          assert.ok(items.some((t) => (t.title || t.name || '').includes('Mock Mojito')));

          const reqs = await getMockRequests(mockUrl);
          assert.ok(
            !reqs.some((r) => r.path.includes('/music/audius/')),
            'Disabled music source MUST NOT be queried',
          );

          const listRes = await userBearer.failLoudlyRequest('GET', '/api/v1/plugins?domain=music', {
            expectedStatus: 200,
          });
          const visibleIds = (listRes.json || []).map((s) => s.id);
          assert.ok(visibleIds.includes(sEnabled.id));
          assert.ok(visibleIds.includes(sDisabled.id));
          const adminList = await admin.failLoudlyRequest('GET', '/api/v1/plugins?domain=music', {
            expectedStatus: 200,
          });
          assert.ok(!(adminList.json || []).some((s) => s.id === sEnabled.id));
        } finally {
          await deleteSourceSafe(member, 'music', sEnabled.id);
          await deleteSourceSafe(member, 'music', sDisabled.id);
        }
      },
    );

    // T3-03: x-api-key x Admin x video x plugins/{id}/test
    await runCase(
      'T3-03',
      ['core', 'video'],
      'x-api-key x Admin x video AppleCMS full /test diagnostics',
      async () => {
        const adminKey = admin.withAuthMode('api_key');
        assert.ok(adminKey.apiKey, 'Admin must have api_key from setup/login');
        const src = await importSource(adminKey, 'video', videoMocks.applecms);
        try {
          const testRes = await adminKey.failLoudlyRequest(
            'POST',
            `/api/v1/plugins/${src.id}/test`,
            {
              body: { mode: 'full', keyword: 'Earth' },
              expectedStatus: 200,
            },
          );
          assert.equal(testRes.json?.ok, true, `Expected /test ok:true, got ${testRes.text}`);
          assert.ok(Array.isArray(testRes.json?.steps) && testRes.json.steps.length >= 2);
          assert.ok(testRes.json.steps.every((st) => st.ok === true));
        } finally {
          await deleteSourceSafe(admin, 'video', src.id);
        }
      },
    );

    // T3-04: ?token= x User x comic x image-proxy (GET allowed, POST rejected)
    await runCase(
      'T3-04',
      ['core', 'comic'],
      '?token= x User x comic image-proxy GET 200 vs POST /online/search 401',
      async () => {
        const src = await importSource(member, 'comic', comicMocks.xkcd);
        try {
          const userQuery = member.withAuthMode('query_token');
          const imgRes = await userQuery.failLoudlyRequest('GET', '/api/v1/comic/online/image-proxy', {
            query: { url: `${mockUrl}/comic/xkcd/strip.png`, source_id: src.id },
            expectedStatus: 200,
          });
          assertPng(imgRes.buffer, 'T3-04 ?token= image-proxy');
          assertSecurityHeaders(imgRes.headers, { label: 'T3-04 image-proxy' });

          await userQuery.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
            body: { keyword: 'xkcd' },
            expectedStatus: 401,
          });
        } finally {
          await deleteSourceSafe(member, 'comic', src.id);
        }
      },
    );

    // T3-05: ?token= x Admin x music x audio-proxy + Range
    await runCase(
      'T3-05',
      ['core', 'music'],
      '?token= x Admin x music audio-proxy Range 206 Partial Content',
      async () => {
        const src = await importSource(admin, 'music', musicMocks.gdstudio);
        try {
          const adminQuery = admin.withAuthMode('query_token');
          const res = await adminQuery.failLoudlyRequest('GET', '/api/v1/music/online/audio-proxy', {
            query: {
              url: `${mockUrl}/music/gdstudio/cdn/gd-101.mp3?br=320`,
              source_id: src.id,
            },
            headers: { Range: 'bytes=0-1023' },
            expectedStatus: 206,
          });
          assert.equal(res.buffer.length, 1024);
          assert.ok(res.buffer.equals(MOCK_MP3_BYTES.subarray(0, 1024)));
          assertMp3(res.buffer, 'T3-05 audio-proxy 206 slice');
          assert.ok((res.headers.get('content-range') || '').startsWith('bytes 0-1023/'));
        } finally {
          await deleteSourceSafe(admin, 'music', src.id);
        }
      },
    );

    // T3-06: ?token= x User x video x hls-proxy
    await runCase(
      'T3-06',
      ['core', 'video'],
      '?token= x User x video hls-proxy master rewrite & segment fetch',
      async () => {
        const src = await importSource(member, 'video', videoMocks.applecms);
        try {
          const userQuery = member.withAuthMode('query_token');
          const masterRes = await userQuery.failLoudlyRequest(
            'GET',
            '/api/v1/video/online/hls-proxy',
            {
              query: {
                url: `${mockUrl}/video/applecms/hls/master.m3u8`,
                source_id: src.id,
              },
              expectedStatus: 200,
            },
          );
          const masterParsed = assertM3u8Rewritten(masterRes.text, {
            expectedSourceId: src.id,
            minSegments: 1,
            label: 'T3-06 master.m3u8',
          });

          const subRel = toRelativeApiPath(masterParsed.segments[0]);
          const subRes = await userQuery.failLoudlyRequest('GET', subRel, { expectedStatus: 200 });
          const subParsed = assertM3u8Rewritten(subRes.text, {
            expectedSourceId: src.id,
            minSegments: 2,
            label: 'T3-06 sub index.m3u8',
          });

          const segRel = toRelativeApiPath(subParsed.segments[0]);
          const segRes = await userQuery.failLoudlyRequest('GET', segRel, { expectedStatus: 200 });
          assert.notEqual(segRes.buffer[0], 0x47, 'seg0.ts should be AES-128 encrypted');
          const decSeg = decryptAes128Cbc(segRes.buffer, MOCK_AES_KEY_BYTES, 0);
          assertMpegTs(decSeg, 'T3-06 rewritten TS segment');
        } finally {
          await deleteSourceSafe(member, 'video', src.id);
        }
      },
    );

    // T3-07: WEBTOON HTML x Referer-gated x image-proxy
    await runCase(
      'T3-07',
      ['comic'],
      'WEBTOON HTML data-url resolution x Referer-gated image-proxy',
      async () => {
        const src = await importSource(member, 'comic', comicMocks.webtoon);
        try {
          await resetMockRequests(mockUrl);
          const searchRes = await member.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
            body: { keyword: 'Tower', source_ids: [src.id] },
            expectedStatus: 200,
          });
          const item = (searchRes.json?.results || searchRes.json?.items || [])[0];
          assert.ok(item, 'Expected WEBTOON search result');
          const detailRes = await member.failLoudlyRequest('POST', '/api/v1/comic/online/detail', {
            body: { source_id: src.id, comic_url: item.comic_url || item.book_url || item.url },
            expectedStatus: 200,
          });
          const ch = (detailRes.json?.chapters || [])[0];
          assert.ok(ch, 'Expected WEBTOON chapter');

          const pagesRes = await member.failLoudlyRequest('POST', '/api/v1/comic/online/pages', {
            body: { source_id: src.id, chapter_url: ch.chapter_url || ch.url },
            expectedStatus: 200,
          });
          const pages = pagesRes.json?.pages || [];
          assert.equal(pages.length, 2);
          assert.equal(pagesRes.json?.reading_direction || detailRes.json?.detail?.reading_direction, 'webtoon');

          const p0Res = await member.failLoudlyRequest(
            'GET',
            toRelativeApiPath(pages[0].proxy_url),
            { expectedStatus: 200 },
          );
          assertJpeg(p0Res.buffer, 'WEBTOON page 1');
          const p1Res = await member.failLoudlyRequest(
            'GET',
            toRelativeApiPath(pages[1].proxy_url),
            { expectedStatus: 200 },
          );
          assertPng(p1Res.buffer, 'WEBTOON page 2');

          const reqs = await getMockRequests(mockUrl);
          const cdnHits = reqs.filter((r) => r.path.startsWith('/comic/webtoon/cdn/'));
          assert.ok(cdnHits.length >= 2);
          assert.ok(
            cdnHits.every((r) => (r.headers.referer || '').startsWith('https://www.webtoons.com/')),
            'image-proxy must inject source imageHeader Referer',
          );
        } finally {
          await deleteSourceSafe(member, 'comic', src.id);
        }
      },
    );

    // T3-08: MangaDex JSON x Custom UA + 405 HEAD x image-proxy
    await runCase(
      'T3-08',
      ['comic'],
      'MangaDex JSON strict UA gate + 405 HEAD x image-proxy',
      async () => {
        const src = await importSource(member, 'comic', comicMocks.mangadex);
        try {
          await resetMockRequests(mockUrl);
          const searchRes = await member.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
            body: { keyword: 'Naruto', source_ids: [src.id] },
            expectedStatus: 200,
          });
          const item = (searchRes.json?.results || searchRes.json?.items || [])[0];
          assert.ok(item && item.title.includes('Mock Naruto'));

          const detailRes = await member.failLoudlyRequest('POST', '/api/v1/comic/online/detail', {
            body: { source_id: src.id, comic_url: item.comic_url || item.book_url || item.url },
            expectedStatus: 200,
          });
          const ch = (detailRes.json?.chapters || [])[0];
          assert.ok(ch);

          const pagesRes = await member.failLoudlyRequest('POST', '/api/v1/comic/online/pages', {
            body: { source_id: src.id, chapter_url: ch.chapter_url || ch.url },
            expectedStatus: 200,
          });
          const pages = pagesRes.json?.pages || [];
          assert.equal(pages.length, 3);

          const imgRes = await member.failLoudlyRequest(
            'GET',
            toRelativeApiPath(pages[0].proxy_url),
            { expectedStatus: 200 },
          );
          assertJpeg(imgRes.buffer, 'MangaDex page 1 JPEG');

          const reqs = await getMockRequests(mockUrl);
          const searchHit = reqs.find((r) => r.path === '/comic/mangadex/manga');
          assert.ok(searchHit?.headers['user-agent']?.includes('xmedia'));
          assert.ok(!reqs.some((r) => r.method === 'HEAD' && r.path.startsWith('/comic/mangadex/cdn/')));
        } finally {
          await deleteSourceSafe(member, 'comic', src.id);
        }
      },
    );

    // T3-09: GDStudio JSON x 206 Range x audio-proxy + cover-proxy
    await runCase(
      'T3-09',
      ['music'],
      'GDStudio multi-step resolve x 206 Range audio-proxy + cover-proxy',
      async () => {
        const src = await importSource(member, 'music', musicMocks.gdstudio);
        try {
          const searchRes = await member.failLoudlyRequest('POST', '/api/v1/music/online/search', {
            body: { keyword: 'Mojito', source_ids: [src.id] },
            expectedStatus: 200,
          });
          const track = (searchRes.json?.results || searchRes.json?.items || [])[0];
          assert.ok(track);

          const infoRes = await member.failLoudlyRequest('POST', '/api/v1/music/online/track-info', {
            body: {
              source_id: src.id,
              track_id: track.track_id || track.id,
              url_id: track.url_id,
              cover_id: track.cover_id,
              lyric_id: track.lyric_id,
              quality: '320',
            },
            expectedStatus: 200,
          });
          assert.ok(infoRes.json?.proxy_stream_url);
          assert.ok(infoRes.json?.proxy_cover_url);
          assert.equal(infoRes.json?.lyrics?.synced, true);

          const coverRes = await member.failLoudlyRequest(
            'GET',
            toRelativeApiPath(infoRes.json.proxy_cover_url),
            { expectedStatus: 200 },
          );
          assertJpeg(coverRes.buffer, 'GDStudio cover');
          assertSecurityHeaders(coverRes.headers, { label: 'GDStudio cover-proxy' });

          const audioRes = await member.failLoudlyRequest(
            'GET',
            toRelativeApiPath(infoRes.json.proxy_stream_url),
            { headers: { Range: 'bytes=4096-8191' }, expectedStatus: 206 },
          );
          assert.equal(audioRes.buffer.length, 4096);
          assert.ok(audioRes.buffer.equals(MOCK_MP3_BYTES.subarray(4096, 8192)));
        } finally {
          await deleteSourceSafe(member, 'music', src.id);
        }
      },
    );

    // T3-10: Audius JSON x 200 Full (ignores Range) x audio-proxy
    await runCase(
      'T3-10',
      ['music'],
      'Audius non-Range 200 upstream x Range-request audio-proxy',
      async () => {
        const src = await importSource(member, 'music', musicMocks.audius);
        try {
          const exploreRes = await member.failLoudlyRequest('POST', '/api/v1/music/online/explore', {
            body: { source_id: src.id },
            expectedStatus: 200,
          });
          const track = (exploreRes.json?.results || exploreRes.json?.items || [])[0];
          assert.ok(track);

          const streamRes = await member.failLoudlyRequest('POST', '/api/v1/music/online/stream', {
            body: {
              source_id: src.id,
              track_id: track.track_id || track.id,
              play_url: track.play_url,
            },
            expectedStatus: 200,
          });
          const audioRes = await member.failLoudlyRequest(
            'GET',
            toRelativeApiPath(streamRes.json.proxy_stream_url),
            { headers: { Range: 'bytes=0-4095' }, expectedStatus: 200 },
          );
          assert.equal(audioRes.buffer.length, MOCK_MP3_BYTES.length);
          assertMp3(audioRes.buffer, 'Audius full 200 audio-proxy');
        } finally {
          await deleteSourceSafe(member, 'music', src.id);
        }
      },
    );

    // T3-11: AppleCMS v10 x Relative HLS + AES Key + fMP4 MAP x hls-proxy
    await runCase(
      'T3-11',
      ['video'],
      'AppleCMS v10 relative HLS + AES-128 key + fMP4 init + TS rewrite via hls-proxy',
      async () => {
        const src = await importSource(member, 'video', videoMocks.applecmsToken);
        try {
          await resetMockRequests(mockUrl);
          const masterRes = await member.failLoudlyRequest(
            'GET',
            '/api/v1/video/online/hls-proxy',
            {
              query: {
                url: `${mockUrl}/video/applecms/hls/master.m3u8?with_query=1&require_header=X-Video-Token:secret123`,
                source_id: src.id,
              },
              expectedStatus: 200,
            },
          );
          const masterParsed = assertM3u8Rewritten(masterRes.text, {
            expectedSourceId: src.id,
            minSegments: 1,
          });

          const subRes = await member.failLoudlyRequest(
            'GET',
            toRelativeApiPath(masterParsed.segments[0]),
            { expectedStatus: 200 },
          );
          const subParsed = assertM3u8Rewritten(subRes.text, {
            expectedSourceId: src.id,
            requireKey: true,
            requireMap: true,
            minSegments: 2,
          });

          const keyRes = await member.failLoudlyRequest(
            'GET',
            toRelativeApiPath(subParsed.keys[0]),
            { expectedStatus: 200 },
          );
          assert.ok(keyRes.buffer.equals(MOCK_AES_KEY_BYTES), 'AES-128 key bytes mismatch');

          const initRes = await member.failLoudlyRequest(
            'GET',
            toRelativeApiPath(subParsed.maps[0]),
            { expectedStatus: 200 },
          );
          assert.ok(initRes.buffer.equals(MOCK_FMP4_INIT), 'fMP4 init segment bytes mismatch');

          const seg0Res = await member.failLoudlyRequest(
            'GET',
            toRelativeApiPath(subParsed.segments[0]),
            { headers: { Range: 'bytes=0-383' }, expectedStatus: 206 },
          );
          assert.equal(seg0Res.buffer.length, 384);
          assert.notEqual(seg0Res.buffer[0], 0x47, 'seg0.ts should be AES-128 encrypted');
          const decSeg0 = decryptAes128Cbc(seg0Res.buffer, keyRes.buffer, 0, { partialPrefixBytes: 376 });
          assertMpegTs(decSeg0, 'AppleCMS TS seg0');

          const seg1Res = await member.failLoudlyRequest(
            'GET',
            toRelativeApiPath(subParsed.segments[1]),
            { expectedStatus: 200 },
          );
          assert.notEqual(seg1Res.buffer[0], 0x47, 'seg1.ts should be AES-128 encrypted');
          const decSeg1 = decryptAes128Cbc(seg1Res.buffer, keyRes.buffer, 1);
          assertMpegTs(decSeg1, 'AppleCMS TS seg1');

          const reqs = await getMockRequests(mockUrl);
          const segHit = reqs.find((r) => r.path.endsWith('/seg0.ts'));
          assert.equal(segHit?.headers['x-video-token'], 'secret123');
        } finally {
          await deleteSourceSafe(member, 'video', src.id);
        }
      },
    );

    // T3-12: PeerTube Rule x 206 Range MP4 x stream-proxy
    await runCase(
      'T3-12',
      ['video'],
      'PeerTube rule mode x 206 Range MP4 via stream-proxy',
      async () => {
        const src = await importSource(member, 'video', videoMocks.peertube);
        try {
          const searchRes = await member.failLoudlyRequest('POST', '/api/v1/video/online/search', {
            body: { keyword: 'Blender', source_ids: [src.id] },
            expectedStatus: 200,
          });
          const item = (searchRes.json?.results || searchRes.json?.items || [])[0];
          assert.ok(item);

          const detailRes = await member.failLoudlyRequest('POST', '/api/v1/video/online/detail', {
            body: {
              source_id: src.id,
              video_id: item.video_id || item.id,
              detail_url: item.detail_url || item.url,
            },
            expectedStatus: 200,
          });
          const ep = detailRes.json?.play_groups?.[0]?.episodes?.[0];
          assert.ok(ep, 'Expected PeerTube episode/file');

          const playRes = await member.failLoudlyRequest('POST', '/api/v1/video/online/play', {
            body: { source_id: src.id, play_url: ep.play_url },
            expectedStatus: 200,
          });
          assert.equal(playRes.json?.stream_type, 'mp4');
          assert.ok(playRes.json?.proxy_stream_url?.includes('stream-proxy'));

          const mp4Res = await member.failLoudlyRequest(
            'GET',
            toRelativeApiPath(playRes.json.proxy_stream_url),
            { headers: { Range: 'bytes=0-1023' }, expectedStatus: 206 },
          );
          assert.equal(mp4Res.buffer.length, 1024);
          assertMp4(mp4Res.buffer, 'PeerTube Range MP4');
          assert.ok(mp4Res.buffer.equals(MOCK_MP4_BYTES.subarray(0, 1024)));
        } finally {
          await deleteSourceSafe(member, 'video', src.id);
        }
      },
    );

    // T3-13: Cookie-session Login x Protected Pages x image-proxy
    await runCase(
      'T3-13',
      ['core', 'comic'],
      'Cookie-session login-action x gated comic pages x image-proxy',
      async () => {
        const src = await importSource(admin, 'comic', edgeMocks.cookieGated);
        try {
          // Before login: should fail (upstream 401)
          const beforeRes = await admin.failLoudlyRequest('POST', '/api/v1/comic/online/pages', {
            body: { source_id: src.id, chapter_url: `${mockUrl}/edge/gated/comic-pages` },
          });
          assert.ok(
            beforeRes.status >= 400 || (beforeRes.json?.pages || []).length === 0,
            'Gated pages must fail before login-action',
          );

          const loginRes = await admin.failLoudlyRequest(
            'POST',
            `/api/v1/plugins/${src.id}/login-action`,
            {
              body: { action: 'login', login_info: { username: 'alice', password: 'secret' } },
              expectedStatus: 200,
            },
          );
          assert.ok(
            (loginRes.json?.source?.cookie_hosts || []).length >= 1,
            'Expected cookie_hosts populated after login-action',
          );
          // Cross-user isolation: member cannot use admin's authenticated plugin
          await member.failLoudlyRequest('POST', '/api/v1/comic/online/pages', {
            body: { source_id: src.id, chapter_url: `${mockUrl}/edge/gated/comic-pages` },
            expectedStatus: 404,
          });

          const afterRes = await admin.failLoudlyRequest('POST', '/api/v1/comic/online/pages', {
            body: { source_id: src.id, chapter_url: `${mockUrl}/edge/gated/comic-pages` },
            expectedStatus: 200,
          });
          const pages = afterRes.json?.pages || [];
          assert.equal(pages.length, 1);

          const imgRes = await admin.failLoudlyRequest(
            'GET',
            toRelativeApiPath(pages[0].proxy_url),
            { expectedStatus: 200 },
          );
          assertPng(imgRes.buffer, 'Cookie-gated page PNG');
        } finally {
          await deleteSourceSafe(admin, 'comic', src.id);
        }
      },
    );

    // T3-14: GBK Charset x CSS Shorthand x Regex ##
    await runCase(
      'T3-14',
      ['comic'],
      'GBK HTML charset decode x CSS shorthand x ## regex cleanup',
      async () => {
        const gbkComic = {
          ...edgeMocks.gbkHtml,
          sourceName: 'GBK Regex T3',
          sourceUrl: `${mockUrl}/edge/gbk-regex-t3`,
          ruleSearch: {
            bookList: 'h1.title',
            name: '@text##测试##',
            bookUrl: `@js:${JSON.stringify(`${mockUrl}/comic/xkcd/info.0.json`)}`,
          },
        };
        const src = await importSource(member, 'comic', gbkComic);
        try {
          const res = await member.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
            body: { keyword: 'test', source_ids: [src.id] },
            expectedStatus: 200,
          });
          const items = res.json?.results || res.json?.items || [];
          assert.equal(items.length, 1);
          assert.equal(items[0].title, '中文漫画', 'Expected GBK decoded + ## regex stripped title');
        } finally {
          await deleteSourceSafe(member, 'comic', src.id);
        }
      },
    );

    // T3-15: QuickJS jsLib x Music Rule x LRC Lyrics
    await runCase(
      'T3-15',
      ['music'],
      'QuickJS jsLib + <js> rule x Music search & LRC lyrics parsing',
      async () => {
        const jsMusic = {
          ...musicMocks.gdstudio,
          sourceName: 'QuickJS Music T3',
          sourceUrl: `${mockUrl}/music/qjs-t3`,
          jsLib: 'function tagTitle(t) { return "[JS] " + t; }',
          ruleSearch: {
            ...musicMocks.gdstudio.ruleSearch,
            title: '$.name<js>tagTitle(result)</js>',
          },
        };
        const src = await importSource(member, 'music', jsMusic);
        try {
          const searchRes = await member.failLoudlyRequest('POST', '/api/v1/music/online/search', {
            body: { keyword: 'Mojito', source_ids: [src.id] },
            expectedStatus: 200,
          });
          const tr = (searchRes.json?.results || searchRes.json?.items || [])[0];
          assert.ok(tr);
          assert.equal(tr.title, '[JS] Mock Mojito');

          const lyrRes = await member.failLoudlyRequest('POST', '/api/v1/music/online/lyrics', {
            body: { source_id: src.id, lyric_id: tr.lyric_id || tr.track_id || 'gd-101' },
            expectedStatus: 200,
          });
          assert.equal(lyrRes.json?.synced, true);
          assert.equal(lyrRes.json?.lines?.[0]?.start_ms, 500);
          assert.equal(lyrRes.json?.lines?.[0]?.text, 'First synced line');
        } finally {
          await deleteSourceSafe(member, 'music', src.id);
        }
      },
    );

    // T3-16: SSRF Hardened x Redirect-to-Loopback x hls-proxy
    await runCase(
      'T3-16',
      ['core', 'video'],
      'Hardened SSRF blocks loopback & redirect-to-loopback on hls-proxy',
      async () => {
        if (!ownsSut) {
          // When testing an external --base-url, still probe loopback SSRF on hls-proxy
          const res = await member.failLoudlyRequest('GET', '/api/v1/video/online/hls-proxy', {
            query: { url: 'http://127.0.0.1:9/secret.m3u8' },
          });
          assert.ok(res.status === 400 || res.status === 403 || res.status === 502);
          return;
        }

        hardenedMgr = new ServerMgr({
          xmediaBin: context.xmediaBin || null,
          webDir: context.webDir !== undefined ? context.webDir : resolveWebDir(),
          keepTmp: Boolean(context.keepTmp),
        });
        const hardenedUrl = await hardenedMgr.start({ mode: 'hardened' });
        try {
          const { admin: hAdmin, member: hMember } = await ensureBootstrapped(hardenedUrl);
          const hSrc = await importSource(hMember, 'video', videoMocks.applecms);

          const directLoop = await hMember.failLoudlyRequest('GET', '/api/v1/video/online/hls-proxy', {
            query: { url: `${mockUrl}/video/applecms/hls/master.m3u8`, source_id: hSrc.id },
          });
          assert.ok(
            [400, 403, 502].includes(directLoop.status),
            `Hardened hls-proxy must block 127.0.0.1 target, got ${directLoop.status}`,
          );

          const metaBlocked = await hMember.failLoudlyRequest('GET', '/api/v1/video/online/hls-proxy', {
            query: { url: 'http://169.254.169.254/latest/meta-data/', source_id: hSrc.id },
          });
          assert.ok([400, 403, 502].includes(metaBlocked.status));

          const redirectBlocked = await hMember.failLoudlyRequest(
            'GET',
            '/api/v1/video/online/hls-proxy',
            {
              query: {
                url: `${mockUrl}/edge/redirect-loopback?target=loopback`,
                source_id: hSrc.id,
              },
            },
          );
          assert.ok([400, 403, 502].includes(redirectBlocked.status));
        } finally {
          await hardenedMgr.cleanup();
          hardenedMgr = null;
        }
      },
    );

    // T3-17: SVG Payload x All 3 Image Proxies
    await runCase(
      'T3-17',
      ['comic', 'music', 'video'],
      'SVG XSS payload rejected across comic/image-proxy, music/cover-proxy, video/poster-proxy',
      async () => {
        const cSrc = await importSource(member, 'comic', comicMocks.xkcd);
        const mSrc = await importSource(member, 'music', musicMocks.gdstudio);
        const vSrc = await importSource(member, 'video', videoMocks.applecms);
        try {
          for (const [ep, sid] of [
            ['/api/v1/comic/online/image-proxy', cSrc.id],
            ['/api/v1/music/online/cover-proxy', mSrc.id],
            ['/api/v1/video/online/poster-proxy', vSrc.id],
          ]) {
            const res = await member.failLoudlyRequest('GET', ep, {
              query: { url: `${mockUrl}/edge/svg-as-image`, source_id: sid },
            });
            assert.ok(
              res.status >= 400 && res.status !== 404,
              `${ep} must reject SVG payload, got ${res.status}`,
            );
            assert.ok(!res.text.includes('<svg'), `${ep} leaked raw SVG bytes`);
          }
        } finally {
          await deleteSourceSafe(member, 'comic', cSrc.id);
          await deleteSourceSafe(member, 'music', mSrc.id);
          await deleteSourceSafe(member, 'video', vSrc.id);
        }
      },
    );

    // T3-18: Seed Persistence x Admin Modify x Restart x Cross-User Isolation
    await runCase(
      'T3-18',
      ['core', 'comic', 'music', 'video'],
      'Seed persistence x admin disable x server restart x strict cross-user isolation',
      async () => {
        const c1 = await importSource(admin, 'comic', comicMocks.mangadex);
        const c2 = await importSource(admin, 'comic', comicMocks.xkcd);
        try {
          await admin.failLoudlyRequest('PATCH', `/api/v1/plugins/${c2.id}`, {
            body: { enabled: false, weight: 77, group_name: 'Pinned-T3' },
            expectedStatus: 200,
          });

          if (ownsSut && serverMgr) {
            baseUrl = await serverMgr.restart({ mode: 'mock' });
            admin = new ApiClient(baseUrl, { authMode: 'bearer' });
            await admin.login(ADMIN_CREDS);
            member = new ApiClient(baseUrl, { authMode: 'bearer' });
            await member.login(MEMBER_CREDS);
          }

          const adminList = await admin.failLoudlyRequest('GET', '/api/v1/plugins?domain=comic', {
            expectedStatus: 200,
          });
          const adminC2 = adminList.json.find((s) => s.id === c2.id);
          assert.ok(adminC2);
          assert.equal(adminC2.enabled, false);
          assert.equal(adminC2.weight, 77);
          assert.equal(adminC2.group_name, 'Pinned-T3');

          const memberList = await member.failLoudlyRequest('GET', '/api/v1/plugins?domain=comic', {
            expectedStatus: 200,
          });
          assert.ok(!memberList.json.some((s) => s.id === c1.id || s.id === c2.id));
        } finally {
          await deleteSourceSafe(admin, 'comic', c1.id);
          await deleteSourceSafe(admin, 'comic', c2.id);
        }
      },
    );

    // T3-19: Re-Import Dedup x Cookie Jar x Export
    await runCase(
      'T3-19',
      ['core', 'comic'],
      'Re-import dedup preserves cookie_hosts while updating searchUrl in export',
      async () => {
        const src = await importSource(admin, 'comic', edgeMocks.cookieGated);
        try {
          await admin.failLoudlyRequest('POST', `/api/v1/plugins/${src.id}/login-action`, {
            body: { action: 'login', login_info: { username: 'alice', password: 'secret' } },
            expectedStatus: 200,
          });

          const updatedSpec = {
            ...edgeMocks.cookieGated,
            searchUrl: `${mockUrl}/comic/xkcd/info.0.json?v=2`,
          };
          const reimport = await admin.failLoudlyRequest('POST', '/api/v1/plugins/import?domain=comic', {
            body: { domain: 'comic', raw_json: JSON.stringify(updatedSpec) },
            expectedStatus: 200,
          });
          assert.equal(reimport.json?.updated_count, 1);
          assert.equal(reimport.json?.imported_count, 0);

          const matched = reimport.json.sources.find((s) => s.id === src.id);
          assert.ok(matched && (matched.cookie_hosts || []).length >= 1, 'cookie_hosts must survive re-import');

          const expRes = await admin.failLoudlyRequest('GET', `/api/v1/plugins/${src.id}/export`, {
            expectedStatus: 200,
          });
          const expStr = typeof expRes.json === 'string' ? expRes.json : JSON.stringify(expRes.json || expRes.text);
          assert.ok(expStr.includes('?v=2'), 'Export must reflect updated searchUrl');
        } finally {
          await deleteSourceSafe(admin, 'comic', src.id);
        }
      },
    );

    // T3-20: Web UI x Settings Debug x Online Search
    await runCase(
      'T3-20',
      ['ui', 'comic'],
      'Browser: Settings source edit/group update reflects in Online Search',
      async () => {
        const src = await importSource(admin, 'comic', comicMocks.xkcd);
        try {
          await withCdpBrowser(async (cdp) => {
            await browserLogin(cdp, baseUrl, ADMIN_CREDS);
            await cdp.goto(`${baseUrl}/comic/settings?tab=sources&advanced=1`);
            await cdp.waitForFn(
              `document.body.textContent.includes('Mock XKCD')`,
              { timeoutMs: 10000, label: 'Mock XKCD visible in Settings comic datasources' },
            );
            cdp.clearErrors();

            // Update group via API or edit sheet and verify Settings & Online Search remain healthy
            await admin.failLoudlyRequest('PATCH', `/api/v1/plugins/${src.id}`, {
              body: { group_name: 'UI-Group-T3', weight: 150 },
              expectedStatus: 200,
            });
            await cdp.goto(`${baseUrl}/comic/settings?tab=sources&advanced=1`);
            await cdp.waitForFn(
              `document.body.textContent.includes('UI-Group-T3')`,
              { timeoutMs: 10000, label: 'Updated group chip visible in Settings' },
            );

            await cdp.goto(`${baseUrl}/comic/library?scope=online&q=Mock`);
            await cdp.waitForFn(
              `document.body.textContent.includes('Mock XKCD Strip')`,
              { timeoutMs: 12000, label: 'Online comic search renders Mock XKCD Strip' },
            );
            cdp.assertNoErrors('T3-20 Settings + Online Search');
          });
        } finally {
          await deleteSourceSafe(admin, 'comic', src.id);
        }
      },
    );

    // T3-21: Web UI x GlobalSearchModal x All 3 Domains
    await runCase(
      'T3-21',
      ['ui', 'comic', 'music', 'video'],
      'Browser: GlobalSearchModal routes cleanly to comic/music/video online search',
      async () => {
        const cSrc = await importSource(member, 'comic', comicMocks.xkcd);
        const mSrc = await importSource(member, 'music', musicMocks.gdstudio);
        const vSrc = await importSource(member, 'video', videoMocks.applecms);
        try {
          await withCdpBrowser(async (cdp) => {
            await browserLogin(cdp, baseUrl, MEMBER_CREDS);

            // Rapid Ctrl+K reopen immediately after Escape (<= 30ms, without waiting for unmount)
            await cdp.pressKey('k', { modifiers: 2 });
            await cdp.waitForSelector('div[role="dialog"][aria-modal="true"] input', {
              visible: true,
              timeoutMs: 8000,
            });
            await new Promise((r) => setTimeout(r, 250));
            await cdp.pressKey('Escape');
            await new Promise((r) => setTimeout(r, 15));
            const modalAttrDuringLeave = await cdp.evalJs(
              `Boolean(document.querySelector('[aria-modal="true"]'))`,
            );
            assert.equal(
              modalAttrDuringLeave,
              false,
              'aria-modal="true" must be removed immediately when leave transition starts',
            );
            await cdp.pressKey('k', { modifiers: 2 });
            await cdp.waitForFn(
              `(() => {
                const dlg = document.querySelector('div[role="dialog"][aria-modal="true"]');
                const inp = dlg?.querySelector('input');
                if (!dlg || !inp) return false;
                const rect = inp.getBoundingClientRect();
                return rect.width > 0 && rect.height > 0 && document.activeElement === inp;
              })()`,
              { timeoutMs: 5000, label: 'GlobalSearchModal rapidly reopens and focuses input' },
            );
            await cdp.pressKey('Escape');
            await cdp.waitForFn(`!document.querySelector('[role="dialog"]')`, {
              timeoutMs: 5000,
              label: 'GlobalSearchModal closed after rapid reopen test',
            });

            for (const { labelHints, expectedPath } of [
              { labelHints: ['comic', '漫画'], expectedPath: '/comic' },
              { labelHints: ['music', '音乐'], expectedPath: '/music' },
              { labelHints: ['video', '影视', '视频'], expectedPath: '/video' },
            ]) {
              await cdp.pressKey('k', { modifiers: 2 }); // Ctrl+K
              await cdp.waitForSelector('div[role="dialog"] input', { visible: true, timeoutMs: 8000 });
              await cdp.type('div[role="dialog"] input', 'Mock');

              const clicked = await cdp.waitForFn(
                `(() => {
                  const dlg = document.querySelector('div[role="dialog"]');
                  if (!dlg) return false;
                  const hints = ${JSON.stringify(labelHints)};
                  const btns = [...dlg.querySelectorAll('button')];
                  for (const b of btns) {
                    const t = (b.textContent || '').toLowerCase();
                    if ((t.includes('在线') || t.includes('online')) && hints.some(h => t.includes(h))) {
                      b.click();
                      return true;
                    }
                  }
                  return false;
                })()`,
                { timeoutMs: 8000, label: `GlobalSearchModal online action (${labelHints.join('/')})` },
              );
              assert.equal(clicked, true);

              await cdp.waitForFn(
                `window.location.pathname.startsWith(${JSON.stringify(expectedPath)}) && window.location.search.includes('scope=online')`,
                { timeoutMs: 8000, label: `Navigated to ${expectedPath} scope=online` },
              );
              await cdp.waitForFn(`!document.querySelector('[role="dialog"]')`, {
                timeoutMs: 5000,
                label: 'GlobalSearchModal closed',
              });
            }
            cdp.assertNoErrors('T3-21 GlobalSearchModal cross-domain');
          });
        } finally {
          await deleteSourceSafe(member, 'comic', cSrc.id);
          await deleteSourceSafe(member, 'music', mSrc.id);
          await deleteSourceSafe(member, 'video', vSrc.id);
        }
      },
    );

    // T3-22: Web UI x Music Online x Video Online (Video pauses Music)
    await runCase(
      'T3-22',
      ['ui', 'music', 'video'],
      'Browser: Launching online Video automatically pauses active online Music playback',
      async () => {
        const mSrc = await importSource(member, 'music', musicMocks.gdstudio);
        const vSrc = await importSource(member, 'video', videoMocks.peertube);
        try {
          await withCdpBrowser(async (cdp) => {
            await browserLogin(cdp, baseUrl, MEMBER_CREDS);
            await cdp.goto(`${baseUrl}/music/library?scope=online&q=Mojito`);
            await cdp.waitForFn(
              `document.body.textContent.includes('Mock Mojito')`,
              { timeoutMs: 12000, label: 'Online music track rendered' },
            );

            // Play the track via UI click
            await clickButtonByText(cdp, ['Mock Mojito', '播放', 'Play'], { timeoutMs: 8000 });
            await cdp.waitForFn(
              `(() => {
                const a = document.querySelector('audio');
                return Boolean(a && a.src && a.src.includes('audio-proxy'));
              })()`,
              { timeoutMs: 10000, label: 'Audio element bound to audio-proxy' },
            );

            // Navigate via SPA sidebar to online video so Pinia playerStore & <audio> stay alive
            await clickButtonByText(cdp, ['影视', '视频', 'Video'], { timeoutMs: 8000 });
            await cdp.waitForFn(`window.location.pathname.startsWith('/video')`, {
              timeoutMs: 8000,
              label: 'Navigated to /video domain',
            });
            await cdp.click('[data-nav="library"]');
            await cdp.waitForFn(`window.location.pathname.startsWith('/video/library')`, {
              timeoutMs: 8000,
              label: 'Navigated to /video/library',
            });
            await clickButtonByText(cdp, ['在线', 'Online'], { timeoutMs: 8000 });
            await cdp.waitForSelector('main input[type="search"], main input[type="text"]', {
              visible: true,
              timeoutMs: 8000,
            });
            await cdp.type('main input[type="search"], main input[type="text"]', 'Blender');
            await cdp.pressKey('Enter');
            await cdp.waitForFn(
              `document.body.textContent.includes('Mock Blender Open Movie')`,
              { timeoutMs: 12000, label: 'Online video card rendered' },
            );
            const stillPlayingBeforeVideo = await cdp.evalJs(
              `(() => { const a = document.querySelector('audio'); return Boolean(a && a.src.includes('audio-proxy') && !a.paused); })()`,
            );
            assert.equal(stillPlayingBeforeVideo, true, 'Music audio must still be playing before video starts');
            await clickButtonMatching(cdp, [/Mock Blender Open Movie/], {
              rootSelector: 'main',
              timeoutMs: 8000,
            });
            await cdp.waitForFn(`window.location.pathname.startsWith('/video/item/online')`, {
              timeoutMs: 8000,
              label: 'Navigated to /video/item/online',
            });
            await clickButtonMatching(cdp, [/^立即播放$|^播放$|^play now$|^play$|360p?/i], {
              rootSelector: 'main',
              timeoutMs: 8000,
            });

            await cdp.waitForFn(
              `(() => {
                const v = document.querySelector('video');
                return Boolean(v && v.error === null && v.readyState >= 1);
              })()`,
              { timeoutMs: 12000, label: 'Online video playing (readyState >= 1)' },
            );

            // Navigate back to /video/item/online so <PlayerBar> re-attaches <audio>
            await clickButtonByText(cdp, ['返回', 'Back'], { timeoutMs: 8000 });
            await cdp.waitForFn(`window.location.pathname.startsWith('/video/item/online')`, {
              timeoutMs: 8000,
              label: 'Returned to /video/item/online',
            });
            await cdp.waitForFn(
              `(() => {
                const a = document.querySelector('audio');
                return Boolean(a && a.src.includes('audio-proxy') && a.paused === true);
              })()`,
              { timeoutMs: 8000, label: 'Re-attached <audio> exists and is paused' },
            );
            cdp.assertNoErrors('T3-22 Music->Video handoff');
          });
        } finally {
          await deleteSourceSafe(member, 'music', mSrc.id);
          await deleteSourceSafe(member, 'video', vSrc.id);
        }
      },
    );

    // T3-23: Web UI x Comic Webtoon x Zoom + Dim
    await runCase(
      'T3-23',
      ['ui', 'comic'],
      'Browser: Online WEBTOON reader with zoom & dimImages loads proxied data-url images',
      async () => {
        const src = await importSource(member, 'comic', comicMocks.webtoon);
        try {
          await withCdpBrowser(async (cdp) => {
            await browserLogin(cdp, baseUrl, MEMBER_CREDS);
            await cdp.goto(`${baseUrl}/comic/library?scope=online&q=Tower`);
            await cdp.waitForFn(
              `document.body.textContent.includes('Tower of Mock')`,
              { timeoutMs: 12000, label: 'WEBTOON search card rendered' },
            );
            await clickButtonByText(cdp, ['Tower of Mock'], { timeoutMs: 8000 });
            await cdp.waitForFn(
              `document.body.textContent.includes('Ep.1 Prologue')`,
              { timeoutMs: 10000, label: 'WEBTOON chapter list rendered' },
            );
            await clickButtonMatching(cdp, [/Ep\.1 Prologue|^开始阅读$|^阅读$|^Start Reading$|^Read$/i], {
              rootSelector: 'main',
              timeoutMs: 8000,
            });

            await cdp.waitForFn(
              `(() => {
                if (!window.location.pathname.includes('/comic/play/online')) return false;
                const imgs = [...document.querySelectorAll('img[alt^="Page "][src*="image-proxy"]')];
                return imgs.length >= 2 && imgs.every(i => i.complete && i.naturalWidth > 0);
              })()`,
              { timeoutMs: 12000, label: 'WEBTOON reader images loaded via image-proxy' },
            );
            cdp.assertNoErrors('T3-23 WEBTOON online reader');
          });
        } finally {
          await deleteSourceSafe(member, 'comic', src.id);
        }
      },
    );

    // T3-24: Non-Regression x Local + Online x Same Domain
    await runCase(
      'T3-24',
      ['nonreg', 'comic', 'music', 'video'],
      'Local upload/browse/stream interleaved with online sources across all 3 domains',
      async () => {
        const cSrc = await importSource(member, 'comic', comicMocks.xkcd);
        const mSrc = await importSource(member, 'music', musicMocks.gdstudio);
        const vSrc = await importSource(member, 'video', videoMocks.peertube);

        let localSeriesId = null;
        let localTrackId = null;
        let localVideoId = null;
        try {
          // 1. Upload minimal local CBZ, MP3, MP4
          const cbzBuf = buildMinimalZip([{ name: '001.jpg', data: MOCK_JPEG_BYTES }]);
          const comicForm = new FormData();
          comicForm.append('file', new Blob([cbzBuf], { type: 'application/zip' }), 'T3_Local_Comic.cbz');
          const cUp = await admin.failLoudlyRequest('POST', '/api/v1/comic/upload', {
            body: comicForm,
            expectedStatus: 201,
          });
          localSeriesId = cUp.json?.uploaded?.[0]?.series_id || null;

          const musicForm = new FormData();
          musicForm.append('file', new Blob([MOCK_MP3_BYTES], { type: 'audio/mpeg' }), 't3_local_track.mp3');
          const mUp = await admin.failLoudlyRequest('POST', '/api/v1/music/upload', {
            body: musicForm,
            expectedStatus: 201,
          });
          localTrackId =
            mUp.json?.uploaded_tracks?.[0]?.id ||
            mUp.json?.tracks?.[0]?.id ||
            mUp.json?.uploaded?.[0]?.id ||
            null;
          assert.ok(localTrackId, 'Expected uploaded_tracks[0].id from /api/v1/music/upload');

          const videoForm = new FormData();
          videoForm.append('file', new Blob([MOCK_MP4_BYTES], { type: 'video/mp4' }), 't3_local_video.mp4');
          const vUp = await admin.failLoudlyRequest('POST', '/api/v1/video/upload', {
            body: videoForm,
            expectedStatus: 201,
          });
          localVideoId =
            vUp.json?.uploaded_videos?.[0]?.id ||
            vUp.json?.videos?.[0]?.id ||
            vUp.json?.uploaded?.[0]?.id ||
            null;
          assert.ok(localVideoId, 'Expected uploaded_videos[0].id from /api/v1/video/upload');

          // Bind pooled uploads into member's personal library
          await member.failLoudlyRequest('POST', '/api/v1/comic/bindings', {
            body: { item_ids: [localSeriesId] },
            expectedStatus: 200,
          });
          await member.failLoudlyRequest('POST', '/api/v1/music/bindings', {
            body: { item_ids: [localTrackId] },
            expectedStatus: 200,
          });
          await member.failLoudlyRequest('POST', '/api/v1/video/bindings', {
            body: { item_ids: [localVideoId] },
            expectedStatus: 200,
          });

          // 2. Interleave local reads & online searches
          const [localComicList, onlineComicRes] = await Promise.all([
            member.failLoudlyRequest('GET', '/api/v1/comic/series', { expectedStatus: 200 }),
            member.failLoudlyRequest('POST', '/api/v1/comic/online/search', {
              body: { keyword: 'Mock', source_ids: [cSrc.id] },
              expectedStatus: 200,
            }),
          ]);
          assert.ok(Array.isArray(localComicList.json) && localComicList.json.length >= 1);
          assert.ok((onlineComicRes.json?.results || onlineComicRes.json?.items || []).length >= 1);

          const [localMusicList, onlineMusicRes] = await Promise.all([
            member.failLoudlyRequest('GET', '/api/v1/music/tracks', { expectedStatus: 200 }),
            member.failLoudlyRequest('POST', '/api/v1/music/online/search', {
              body: { keyword: 'Mojito', source_ids: [mSrc.id] },
              expectedStatus: 200,
            }),
          ]);
          assert.ok((localMusicList.json?.items || localMusicList.json || []).length >= 1);
          assert.ok((onlineMusicRes.json?.results || onlineMusicRes.json?.items || []).length >= 1);

          const [localVideoList, onlineVideoRes] = await Promise.all([
            member.failLoudlyRequest('GET', '/api/v1/video/videos', { expectedStatus: 200 }),
            member.failLoudlyRequest('POST', '/api/v1/video/online/search', {
              body: { keyword: 'Blender', source_ids: [vSrc.id] },
              expectedStatus: 200,
            }),
          ]);
          assert.ok(Array.isArray(localVideoList.json) && localVideoList.json.length >= 1);
          assert.ok((onlineVideoRes.json?.results || onlineVideoRes.json?.items || []).length >= 1);
        } finally {
          if (localSeriesId) await admin.request('DELETE', `/api/v1/comic/series/${localSeriesId}`).catch(() => {});
          if (localTrackId) await admin.request('DELETE', `/api/v1/music/tracks/${localTrackId}`).catch(() => {});
          if (localVideoId) await admin.request('DELETE', `/api/v1/video/videos/${localVideoId}`).catch(() => {});
          await deleteSourceSafe(member, 'comic', cSrc.id);
          await deleteSourceSafe(member, 'music', mSrc.id);
          await deleteSourceSafe(member, 'video', vSrc.id);
        }
      },
    );

    // T3-25: bfcache + /api/v1/events/stream SSE cleanup across 8 sequential full-page navigations
    await runCase(
      'T3-25',
      ['core', 'ui'],
      'Browser: 8 sequential full-page HTTP/1.1 navigations & history.back() do not exhaust SSE connection pool',
      async () => {
        await withCdpBrowser(async (cdp) => {
          await browserLogin(cdp, baseUrl, MEMBER_CREDS);
          const routes = [
            '/comic/library',
            '/music/library',
            '/video/library',
            '/settings',
            '/comic/library?page=1',
            '/music/library?page=2',
            '/video/library?page=3',
            '/book/library',
          ];
          for (const route of routes) {
            await cdp.goto(`${baseUrl}${route}`);
            await cdp.waitForSelector('#app', { visible: true, timeoutMs: 5000 });
          }
          const capStatus = await cdp.evalJs(
            `fetch('/api/v1/system/capabilities', { credentials: 'include' }).then((r) => r.status)`,
          );
          assert.equal(capStatus, 200, 'HTTP/1.1 connection pool must remain responsive after 8 navigations');

          await cdp.evalJs(`history.back()`);
          await new Promise((r) => setTimeout(r, 400));
          const backCapStatus = await cdp.evalJs(
            `fetch('/api/v1/system/capabilities', { credentials: 'include' }).then((r) => r.status)`,
          );
          assert.equal(backCapStatus, 200, 'capabilities fetch must succeed after history.back() + pageshow');
          cdp.assertNoErrors('T3-25 SSE bfcache multi-navigation');
        });
      },
    );
  } finally {
    if (hardenedMgr) await hardenedMgr.cleanup().catch(() => {});
    if (serverMgr) await serverMgr.cleanup().catch(() => {});
    await mock.stop().catch(() => {});
  }

  const passed = cases.filter((c) => c.status === 'pass').length;
  const failed = cases.filter((c) => c.status === 'fail').length;
  return {
    name: 'tier3',
    suite: 'tier3',
    total: cases.length,
    passed,
    failed,
    cases,
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const res = await runTier3();
  console.log(`\nTier 3 summary: ${res.passed}/${res.total} passed, ${res.failed} failed`);
  process.exitCode = res.failed > 0 || res.total === 0 ? 1 : 0;
}
