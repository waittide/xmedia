import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import {
  buildEdgeMockSources,
  buildMockBookSources,
  buildMockComicSources,
  buildMockMusicSources,
  buildMockVideoSources,
} from '../fixtures/mock_sources.mjs';
import { isDefaultAllowedErrorUrl, withCdpBrowser } from '../lib/cdp.mjs';
import { ApiClient, allocateFreePort, SutEndpointMissingError } from '../lib/client.mjs';
import {
  assertJpeg,
  assertM3u8Rewritten,
  assertMp3,
  assertMp4,
  assertMpegTs,
  assertPng,
  assertRasterImage,
  assertSecurityHeaders,
  decryptAes128Cbc,
  MOCK_AES_KEY_BYTES,
  MOCK_FMP4_INIT,
  MOCK_FMP4_SEG,
  MOCK_GBK_HTML_BYTES,
  MOCK_JPEG_BYTES,
  MOCK_MP3_BYTES,
  MOCK_MP4_BYTES,
  MOCK_PNG_BYTES,
  MOCK_TS_SEGMENT_1,
  MOCK_TS_SEGMENT_2,
} from '../lib/fixtures.mjs';
import { MockOriginServer } from '../lib/mock_origin.mjs';

export async function runSelfTests({ silent = false } = {}) {
  const mock = new MockOriginServer();
  const baseUrl = await mock.start();
  const cases = [];

  async function runCase(id, title, fn) {
    const start = performance.now();
    try {
      await fn();
      const durationMs = Math.round(performance.now() - start);
      cases.push({ id, title, status: 'pass', durationMs });
      if (!silent) {
        console.log(`  [PASS] ${id}: ${title} (${durationMs}ms)`);
      }
    } catch (err) {
      const durationMs = Math.round(performance.now() - start);
      cases.push({ id, title, status: 'fail', durationMs, error: err.stack || err.message });
      if (!silent) {
        console.error(`  [FAIL] ${id}: ${title} (${durationMs}ms)\n    ${err.message}`);
      }
    }
  }

  try {
    // ST-01: Mock health, reset, and request recorder
    await runCase('ST-01', 'Mock Origin health, reset & request recorder', async () => {
      const healthRes = await fetch(`${baseUrl}/__mock/health`);
      assert.equal(healthRes.status, 200);
      assert.deepEqual(await healthRes.json(), { status: 'ok' });

      const resetRes = await fetch(`${baseUrl}/__mock/reset`, { method: 'POST' });
      assert.equal(resetRes.status, 200);

      await fetch(`${baseUrl}/comic/xkcd/info.0.json`, {
        headers: { 'X-Probe-Header': 'probe-123' },
      });

      const reqsRes = await fetch(`${baseUrl}/__mock/requests?limit=10`);
      assert.equal(reqsRes.status, 200);
      const reqs = await reqsRes.json();
      assert.equal(reqs.length, 1);
      assert.equal(reqs[0].method, 'GET');
      assert.equal(reqs[0].path, '/comic/xkcd/info.0.json');
      assert.equal(reqs[0].headers['x-probe-header'], 'probe-123');

      await fetch(`${baseUrl}/__mock/reset`, { method: 'POST' });
      const afterReset = await (await fetch(`${baseUrl}/__mock/requests`)).json();
      assert.equal(afterReset.length, 0);
    });

    // ST-02: Magic-byte fixtures & sniffer assertions
    await runCase('ST-02', 'Deterministic binary fixtures & magic-byte sniffer', async () => {
      assert.ok(MOCK_JPEG_BYTES.length >= 125);
      assertJpeg(MOCK_JPEG_BYTES, 'MOCK_JPEG_BYTES');
      assertPng(MOCK_PNG_BYTES, 'MOCK_PNG_BYTES');
      assertRasterImage(MOCK_JPEG_BYTES, 'image/jpeg');
      assertRasterImage(MOCK_PNG_BYTES, 'image/png');

      assert.ok(MOCK_MP3_BYTES.length > 50000);
      assertMp3(MOCK_MP3_BYTES, 'MOCK_MP3_BYTES');

      assert.ok(MOCK_TS_SEGMENT_1.length >= 1880);
      assert.ok(MOCK_TS_SEGMENT_2.length >= 1880);
      assertMpegTs(MOCK_TS_SEGMENT_1, 'MOCK_TS_SEGMENT_1');
      assertMpegTs(MOCK_TS_SEGMENT_2, 'MOCK_TS_SEGMENT_2');

      assertMp4(MOCK_MP4_BYTES, 'MOCK_MP4_BYTES');
      assertMp4(MOCK_FMP4_INIT, 'MOCK_FMP4_INIT');
      assertMp4(MOCK_FMP4_SEG, 'MOCK_FMP4_SEG', { allowFragment: true });

      assert.equal(MOCK_AES_KEY_BYTES.length, 16);
      assert.equal(MOCK_AES_KEY_BYTES[0], 0x00);
      assert.equal(MOCK_AES_KEY_BYTES[15], 0x0f);

      const decodedGbk = new TextDecoder('gbk').decode(MOCK_GBK_HTML_BYTES);
      assert.ok(decodedGbk.includes('中文测试漫画'), `Unexpected GBK text: ${decodedGbk}`);
    });

    // ST-03: MangaDex mock chain + UA gate + HEAD 405
    await runCase('ST-03', 'MangaDex mock search -> detail -> feed -> at-home -> JPEG + HEAD 405', async () => {
      const blockedRes = await fetch(`${baseUrl}/comic/mangadex/manga?title=Naruto&strict_ua=1`, {
        headers: { 'User-Agent': 'Mozilla/5.0' },
      });
      assert.equal(blockedRes.status, 403);

      const searchRes = await fetch(`${baseUrl}/comic/mangadex/manga?title=Naruto&strict_ua=1`, {
        headers: { 'User-Agent': 'xmedia/0.1.0' },
      });
      assert.equal(searchRes.status, 200);
      const searchJson = await searchRes.json();
      assert.equal(searchJson.data[0].id, 'md-comic-1');
      assert.equal(searchJson.data[0].attributes.title.en, 'Mock Naruto');

      const feedJson = await (await fetch(`${baseUrl}/comic/mangadex/manga/md-comic-1/feed`)).json();
      assert.equal(feedJson.data.length, 2);

      const atHome = await (await fetch(`${baseUrl}/comic/mangadex/at-home/server/md-ch-1`)).json();
      const firstPageUrl = `${atHome.baseUrl}/data-saver/${atHome.chapter.hash}/${atHome.chapter.dataSaver[0]}`;

      const headRes = await fetch(firstPageUrl, { method: 'HEAD' });
      assert.equal(headRes.status, 405);

      const pageRes = await fetch(firstPageUrl);
      assert.equal(pageRes.status, 200);
      assertJpeg(Buffer.from(await pageRes.arrayBuffer()), 'MangaDex page 1');
    });

    // ST-04: WEBTOON mock HTML data-url + Referer 403 gate + paginated TOC
    await runCase('ST-04', 'WEBTOON mock HTML #_imageList data-url & Referer 403 gate', async () => {
      const searchHtml = await (await fetch(`${baseUrl}/comic/webtoon/search?keyword=tower`)).text();
      assert.ok(searchHtml.includes('/comic/webtoon/list?title_no=95'));

      const listPage1 = await (await fetch(`${baseUrl}/comic/webtoon/list?title_no=95`)).text();
      assert.ok(listPage1.includes('/comic/webtoon/list?title_no=95&page=2'));
      const listPage2 = await (await fetch(`${baseUrl}/comic/webtoon/list?title_no=95&page=2`)).text();
      assert.ok(listPage2.includes('episode_no=3'));

      const viewerHtml = await (
        await fetch(`${baseUrl}/comic/webtoon/viewer?title_no=95&episode_no=1`)
      ).text();
      const matches = [...viewerHtml.matchAll(/data-url="([^"]+)"/g)].map((m) => m[1]);
      assert.deepEqual(matches, ['/comic/webtoon/cdn/ep1_1.jpg', '/comic/webtoon/cdn/ep1_2.png']);

      const cdnUrl = `${baseUrl}${matches[0]}`;
      const noRefRes = await fetch(cdnUrl);
      assert.equal(noRefRes.status, 403);

      const okRes = await fetch(cdnUrl, {
        headers: { Referer: 'https://www.webtoons.com/' },
      });
      assert.equal(okRes.status, 200);
      assertJpeg(Buffer.from(await okRes.arrayBuffer()), 'WEBTOON page 1');

      const pngRes = await fetch(`${baseUrl}${matches[1]}`, {
        headers: { Referer: 'https://www.webtoons.com/' },
      });
      assert.equal(pngRes.status, 200);
      assertPng(Buffer.from(await pngRes.arrayBuffer()), 'WEBTOON page 2');
    });

    // ST-05: GDStudio (206 Range + 416) & Audius (200 ignoring Range)
    await runCase('ST-05', 'GDStudio Range 206/416 & Audius non-Range 200 MP3 mocks', async () => {
      const searchArr = await (
        await fetch(`${baseUrl}/music/gdstudio/api.php?types=search&source=netease&name=Mojito`)
      ).json();
      assert.equal(searchArr[0].name, 'Mock Mojito');

      const urlObj = await (
        await fetch(`${baseUrl}/music/gdstudio/api.php?types=url&source=netease&id=gd-101&br=320`)
      ).json();
      const lyrObj = await (
        await fetch(`${baseUrl}/music/gdstudio/api.php?types=lyric&source=netease&id=gd-101`)
      ).json();
      assert.ok(lyrObj.lyric.includes('[00:00.50]First synced line'));

      const rangeRes = await fetch(urlObj.url, { headers: { Range: 'bytes=0-4095' } });
      assert.equal(rangeRes.status, 206);
      assert.equal(rangeRes.headers.get('content-range'), `bytes 0-4095/${MOCK_MP3_BYTES.length}`);
      const sliceBuf = Buffer.from(await rangeRes.arrayBuffer());
      assert.equal(sliceBuf.length, 4096);
      assertMp3(sliceBuf, 'GDStudio MP3 Range slice');
      assert.ok(sliceBuf.equals(MOCK_MP3_BYTES.subarray(0, 4096)));

      const oobRes = await fetch(urlObj.url, { headers: { Range: 'bytes=99999999-' } });
      assert.equal(oobRes.status, 416);

      const audRes = await fetch(`${baseUrl}/music/audius/v1/tracks/aud-1/stream`, {
        headers: { Range: 'bytes=0-4095' },
      });
      assert.equal(audRes.status, 200);
      const audBuf = Buffer.from(await audRes.arrayBuffer());
      assert.equal(audBuf.length, MOCK_MP3_BYTES.length);
      assertMp3(audBuf, 'Audius non-Range full MP3');
    });

    // ST-06: AppleCMS v10 JSON + HLS master/sub/key/map/ts + require_header
    await runCase('ST-06', 'AppleCMS v10 JSON & nested HLS + AES key + fMP4 map + TS segments', async () => {
      const listJson = await (
        await fetch(`${baseUrl}/video/applecms/api.php/provide/vod/?ac=list&pg=1`)
      ).json();
      assert.equal(listJson.class.length, 2);

      const detailJson = await (
        await fetch(`${baseUrl}/video/applecms/api.php/provide/vod/?ac=detail&ids=1001`)
      ).json();
      const vod = detailJson.list[0];
      assert.ok(vod.vod_play_from.includes('sharehtml$$$ikm3u8$$$altm3u8'));
      assert.ok(vod.vod_play_url.includes('/video/applecms/share/1001'));

      const masterText = await (await fetch(`${baseUrl}/video/applecms/hls/master.m3u8`)).text();
      assert.ok(masterText.includes('5000kb/hls/index.m3u8'));

      const mediaText = await (
        await fetch(`${baseUrl}/video/applecms/hls/5000kb/hls/index.m3u8`)
      ).text();
      assert.ok(mediaText.includes('#EXT-X-KEY:METHOD=AES-128,URI="../keys/enc.key"'));
      assert.ok(mediaText.includes('#EXT-X-MAP:URI="init.mp4"'));

      const keyBuf = Buffer.from(
        await (await fetch(`${baseUrl}/video/applecms/hls/5000kb/keys/enc.key`)).arrayBuffer(),
      );
      assert.ok(keyBuf.equals(MOCK_AES_KEY_BYTES));

      const initBuf = Buffer.from(
        await (await fetch(`${baseUrl}/video/applecms/hls/5000kb/hls/init.mp4`)).arrayBuffer(),
      );
      assertMp4(initBuf, 'AppleCMS fMP4 init');

      const tsForbidden = await fetch(
        `${baseUrl}/video/applecms/hls/5000kb/hls/seg0.ts?require_header=X-Video-Token:secret123`,
      );
      assert.equal(tsForbidden.status, 403);

      const tsOk = await fetch(
        `${baseUrl}/video/applecms/hls/5000kb/hls/seg0.ts?require_header=X-Video-Token:secret123`,
        { headers: { 'X-Video-Token': 'secret123', Range: 'bytes=0-383' } },
      );
      assert.equal(tsOk.status, 206);
      const encPartial = Buffer.from(await tsOk.arrayBuffer());
      assert.equal(encPartial.length, 384);
      assert.notEqual(encPartial[0], 0x47, 'seg0.ts should be AES-128 encrypted');
      const decPartial = decryptAes128Cbc(encPartial, keyBuf, 0, { partialPrefixBytes: 376 });
      assertMpegTs(decPartial, 'AppleCMS TS partial');
    });

    // ST-07: PeerTube mock + Range MP4
    await runCase('ST-07', 'PeerTube JSON mock & Range MP4 fixture', async () => {
      const searchJson = await (
        await fetch(`${baseUrl}/video/peertube/api/v1/search/videos?search=blender`)
      ).json();
      assert.equal(searchJson.data[0].uuid, 'pt-vid-1');

      const detailJson = await (
        await fetch(`${baseUrl}/video/peertube/api/v1/videos/pt-vid-1`)
      ).json();
      const mp4Url = detailJson.files[0].fileUrl;

      const mp4RangeRes = await fetch(mp4Url, { headers: { Range: 'bytes=0-1023' } });
      assert.equal(mp4RangeRes.status, 206);
      const mp4Partial = Buffer.from(await mp4RangeRes.arrayBuffer());
      assert.equal(mp4Partial.length, 1024);
      assertMp4(mp4Partial, 'PeerTube MP4 Range');
    });

    // ST-08: Security / Edge endpoints
    await runCase('ST-08', 'Security, error, charset, redirect, cookie & oversized edge mocks', async () => {
      const svgRes = await fetch(`${baseUrl}/edge/svg-as-image`);
      assert.equal(svgRes.headers.get('content-type'), 'image/svg+xml');
      assert.ok((await svgRes.text()).includes('<svg'));

      const htmlImgRes = await fetch(`${baseUrl}/edge/html-as-image.jpg`);
      assert.ok((await htmlImgRes.text()).includes('<html>'));

      const wrongCtRes = await fetch(`${baseUrl}/edge/jpeg-wrong-ct`);
      assert.equal(wrongCtRes.headers.get('content-type'), 'application/octet-stream');
      assertJpeg(Buffer.from(await wrongCtRes.arrayBuffer()), 'JPEG wrong content-type');

      assert.equal((await fetch(`${baseUrl}/edge/error-502`)).status, 502);
      assert.equal((await fetch(`${baseUrl}/edge/error-500-html`)).status, 500);
      assert.throws(() => JSON.parse('not-json'));
      const badJsonText = await (await fetch(`${baseUrl}/edge/malformed-json`)).text();
      assert.throws(() => JSON.parse(badJsonText));

      const gbkBuf = Buffer.from(await (await fetch(`${baseUrl}/edge/gbk-html`)).arrayBuffer());
      assert.ok(new TextDecoder('gbk').decode(gbkBuf).includes('中文测试漫画'));

      const redirLoop = await fetch(`${baseUrl}/edge/redirect-loopback`, { redirect: 'manual' });
      assert.equal(redirLoop.status, 302);
      assert.ok(redirLoop.headers.get('location').includes('169.254.169.254'));

      const redirChain = await fetch(`${baseUrl}/edge/redirect-chain?n=8`, { redirect: 'manual' });
      assert.equal(redirChain.status, 302);

      // Cookie login flow
      assert.equal((await fetch(`${baseUrl}/edge/gated/comic-pages`)).status, 401);
      const loginRes = await fetch(`${baseUrl}/edge/auth/login`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ username: 'alice', password: 'secret' }),
      });
      assert.equal(loginRes.status, 200);
      const cookieHeader = loginRes.headers.get('set-cookie').split(';')[0];
      const gatedOk = await fetch(`${baseUrl}/edge/gated/comic-pages`, {
        headers: { Cookie: cookieHeader },
      });
      assert.equal(gatedOk.status, 200);

      // Oversized streaming header checks (abort body immediately after reading headers)
      const ac1 = new AbortController();
      const bigImg = await fetch(`${baseUrl}/edge/oversized-image`, { signal: ac1.signal });
      assert.equal(bigImg.headers.get('content-length'), String(17 * 1024 * 1024));
      ac1.abort();

      const ac2 = new AbortController();
      const bigM3u8 = await fetch(`${baseUrl}/edge/oversized-m3u8`, { signal: ac2.signal });
      assert.equal(bigM3u8.headers.get('content-length'), String(9 * 1024 * 1024));
      ac2.abort();

      const postEcho = await (
        await fetch(`${baseUrl}/edge/post-echo`, {
          method: 'POST',
          headers: { 'X-Custom': 'abc' },
          body: 'q=hello',
        })
      ).json();
      assert.equal(postEcho.ok, true);
      assert.equal(postEcho.headers['x-custom'], 'abc');
      assert.equal(postEcho.body, 'q=hello');
    });

    // ST-09: M3U8 rewrite verifier + mock source factories + client helpers
    await runCase('ST-09', 'M3U8 rewriter helper, mock source factories & client helpers', async () => {
      const sampleRewritten = [
        '#EXTM3U',
        '#EXT-X-VERSION:6',
        '#EXT-X-KEY:METHOD=AES-128,URI="/api/v1/video/online/hls-proxy?url=http%3A%2F%2F127.0.0.1%3A9000%2Fenc.key&source_id=src-1"',
        '#EXT-X-MAP:URI="/api/v1/video/online/hls-proxy?url=http%3A%2F%2F127.0.0.1%3A9000%2Finit.mp4&source_id=src-1"',
        '#EXTINF:1.0,',
        '/api/v1/video/online/hls-proxy?url=http%3A%2F%2F127.0.0.1%3A9000%2Fseg0.ts&source_id=src-1',
        '#EXT-X-ENDLIST',
      ].join('\n');

      const parsed = assertM3u8Rewritten(sampleRewritten, {
        expectedSourceId: 'src-1',
        requireKey: true,
        requireMap: true,
        minSegments: 1,
      });
      assert.equal(parsed.keys.length, 1);
      assert.equal(parsed.maps.length, 1);
      assert.equal(parsed.segments.length, 1);

      const comics = buildMockComicSources(baseUrl);
      const musics = buildMockMusicSources(baseUrl);
      const videos = buildMockVideoSources(baseUrl);
      const books = buildMockBookSources(baseUrl);
      const edges = buildEdgeMockSources(baseUrl);
      assert.equal(comics.all.length, 3);
      assert.equal(musics.all.length, 2);
      assert.equal(videos.all.length, 2);
      assert.equal(books.all.length, 1);
      assert.ok(edges.postEcho.searchUrl.includes('/edge/post-echo'));

      const freePort = await allocateFreePort();
      assert.ok(freePort > 0 && freePort <= 65535);

      const client = new ApiClient(baseUrl);
      await assert.rejects(
        () => client.failLoudlyRequest('GET', '/nonexistent-sut-route'),
        SutEndpointMissingError,
      );
    });

    // ST-10: Negative self-tests of assertion helpers
    await runCase('ST-10', 'Negative self-tests reject SVG/HTML images, bad status, missing headers & raw M3U8', async () => {
      const svgBuf = Buffer.from('<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>');
      assert.throws(() => assertRasterImage(svgBuf, 'image/jpeg', 'disguised svg'), /unexpected markup/i);
      assert.throws(() => assertJpeg(svgBuf, 'disguised svg'));

      // 2-byte BMP prefix followed by SVG payload (both short and >= 26 bytes) must be rejected
      assert.throws(
        () => assertRasterImage(Buffer.from('BM<svg xmlns="http://www.w3.org/2000/svg"/>'), null, 'BM+svg'),
        /unexpected markup|unrecognized raster/i,
      );
      assert.throws(
        () =>
          assertRasterImage(
            Buffer.from('BM<svg xmlns="http://www.w3.org/2000/svg">padded-to-over-26</svg>'),
            null,
            'BM+svg padded',
          ),
        /unexpected markup|unrecognized raster/i,
      );
      assert.throws(
        () => assertRasterImage(Buffer.alloc(20, 0x42), null, 'short BM header'),
        /unrecognized raster/i,
      );

      const htmlBuf = Buffer.from('<!doctype html><html><body>fake jpg</body></html>');
      assert.throws(() => assertRasterImage(htmlBuf, null, 'disguised html'), /unexpected markup/i);

      assert.throws(() => assertMp3(Buffer.from('{"not":"mp3"}')), /missing ID3 or MPEG sync/i);
      assert.throws(() => assertMpegTs(Buffer.alloc(376, 0)), /missing 0x47 sync byte/i);

      // Unaligned / truncated MPEG-TS buffers (even when 0x47 appears at packet starts) must be rejected
      const ts189 = Buffer.alloc(189, 0);
      ts189[0] = 0x47;
      ts189[188] = 0x47;
      assert.throws(() => assertMpegTs(ts189), /not a multiple of 188/i);

      const ts200 = Buffer.alloc(200, 0);
      ts200[0] = 0x47;
      ts200[188] = 0x47;
      assert.throws(() => assertMpegTs(ts200), /not a multiple of 188/i);

      assert.throws(() => assertMp4(Buffer.from('not-an-mp4-buffer-at-all')), /expected ftyp box/i);

      assert.throws(
        () => assertSecurityHeaders(new Headers({ 'content-type': 'image/jpeg' })),
        /nosniff/i,
      );
      assert.throws(
        () =>
          assertSecurityHeaders(
            new Headers({
              'x-content-type-options': 'nosniff',
              'content-security-policy': "default-src * 'unsafe-inline'",
            }),
          ),
        /unsafe-inline|wildcard/i,
      );
      assert.throws(
        () =>
          assertSecurityHeaders(
            new Headers({
              'x-content-type-options': 'nosniff',
              'content-security-policy': "default-src 'none'; img-src *",
            }),
          ),
        /wildcard/i,
      );

      const unrewrittenM3u8 = '#EXTM3U\n#EXTINF:1.0,\nhttp://127.0.0.1:9000/seg0.ts\n';
      assert.throws(() => assertM3u8Rewritten(unrewrittenM3u8), /expected rewritten path/i);

      const unrewrittenExtXMedia = [
        '#EXTM3U',
        '#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="g",NAME="en",URI="https://upstream.example/audio.m3u8"',
        '#EXT-X-STREAM-INF:BANDWIDTH=800000,AUDIO="g"',
        '/api/v1/video/online/hls-proxy?url=http%3A%2F%2F127.0.0.1%3A9000%2Fmain.m3u8&source_id=src-1',
      ].join('\n');
      assert.throws(
        () => assertM3u8Rewritten(unrewrittenExtXMedia, { expectedSourceId: 'src-1' }),
        /expected rewritten path/i,
      );

      // Verify failLoudlyRequest rejects HTTP 200 when 206 is expected
      const client = new ApiClient(baseUrl);
      await assert.rejects(
        () => client.failLoudlyRequest('GET', '/comic/xkcd/info.0.json', { expectedStatus: 206 }),
        /Expected HTTP 206.*got 200/,
      );
    });

    // ST-CDP: Real Chromium CDP WebSocket smoke test against Mock Origin HTML
    await runCase('ST-CDP', 'Real Chromium CDP WebSocket navigation, DOM interaction & clean teardown', async () => {
      await withCdpBrowser(async (cdp) => {
        await cdp.goto(`${baseUrl}/edge/cdp-smoke.html`);
        await cdp.waitForSelector('#heading', { visible: true });

        const imgReady = await cdp.waitForFn(
          () => {
            const img = document.getElementById('strip');
            return Boolean(img && img.complete && img.naturalWidth === 2 && img.naturalHeight === 2);
          },
          { timeoutMs: 5000, label: 'MOCK_PNG_BYTES decoded by Chromium' },
        );
        assert.equal(imgReady, true);

        await cdp.type('#kw', 'hello-cdp');
        await cdp.click('#btn');
        const outText = await cdp.evalJs("document.getElementById('out').textContent");
        assert.equal(outText, 'clicked:hello-cdp');

        cdp.assertNoErrors('CDP smoke page');
      });
    });

    // ST-11: parseArgs preserves `=` in flag values and importing run.mjs has no top-level side effects
    await runCase('ST-11', 'e2e/run.mjs parseArgs preserves `=` values and guards main() on import', async () => {
      const savedArgv = [...process.argv];
      try {
        process.argv = ['node', 'other-caller.mjs', '--unrelated-host-flag'];
        const { parseArgs } = await import('../run.mjs');
        const parsed = parseArgs([
          '--base-url=http://127.0.0.1:8080/?a=1&b=2',
          '--live-report=/tmp/dir=1',
          '--xmedia-bin=/tmp/bin=1',
          '--web-dir=/tmp/web=1',
          '--tier=1,2',
          '--domain=ui',
        ]);
        assert.equal(parsed.baseUrl, 'http://127.0.0.1:8080/?a=1&b=2');
        assert.equal(parsed.liveReport, '/tmp/dir=1');
        assert.equal(parsed.xmediaBin, '/tmp/bin=1');
        assert.equal(parsed.webDir, '/tmp/web=1');
        assert.deepEqual(parsed.tiers, [1, 2]);
        assert.equal(parsed.domain, 'ui');
      } finally {
        process.argv = savedArgv;
      }
    });

    // ST-12: Strict pathname checks in live.mjs and cdp.mjs reject query-string masking
    await runCase('ST-12', 'Error-filter allowlists reject proxy query-string masking payloads', async () => {
      const { isAllowedExternalThumbError } = await import('./live.mjs');
      assert.equal(
        isAllowedExternalThumbError(
          'http://127.0.0.1:8080/api/v1/music/online/cover-proxy?url=https://a.test/c.jpg',
        ),
        true,
      );
      assert.equal(
        isAllowedExternalThumbError(
          'http://127.0.0.1:8080/api/v1/video/online/poster-proxy?url=https://a.test/p.jpg',
        ),
        true,
      );
      assert.equal(
        isAllowedExternalThumbError(
          'http://127.0.0.1:8080/api/v1/comic/online/image-proxy?url=https://evil.test/api/v1/music/online/cover-proxy',
        ),
        false,
      );
      assert.equal(isDefaultAllowedErrorUrl('http://127.0.0.1:8080/favicon.ico'), true);
      assert.equal(isDefaultAllowedErrorUrl('/favicon.ico?v=1'), true);
      assert.equal(
        isDefaultAllowedErrorUrl(
          'http://127.0.0.1:8080/api/v1/comic/online/image-proxy?url=http://evil.test/favicon.ico',
        ),
        false,
      );
    });
  } finally {
    await mock.stop();
  }

  const passed = cases.filter((c) => c.status === 'pass').length;
  const failed = cases.filter((c) => c.status === 'fail').length;
  return {
    suite: 'self-test',
    total: cases.length,
    passed,
    failed,
    cases,
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const res = await runSelfTests();
  console.log(`\nSelf-test summary: ${res.passed}/${res.total} passed, ${res.failed} failed`);
  process.exitCode = res.failed > 0 ? 1 : 0;
}
