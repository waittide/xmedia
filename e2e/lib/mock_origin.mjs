import http from 'node:http';
import {
  encryptAes128Cbc,
  MOCK_AES_KEY_BYTES,
  MOCK_FMP4_INIT,
  MOCK_GBK_HTML_BYTES,
  MOCK_JPEG_BYTES,
  MOCK_MP3_BYTES,
  MOCK_MP4_BYTES,
  MOCK_PNG_BYTES,
  MOCK_TS_SEGMENT_1,
  MOCK_TS_SEGMENT_2,
  parseByteRange,
} from './fixtures.mjs';

const MOCK_ENC_TS_SEGMENT_1 = encryptAes128Cbc(MOCK_TS_SEGMENT_1, MOCK_AES_KEY_BYTES, 0);
const MOCK_ENC_TS_SEGMENT_2 = encryptAes128Cbc(MOCK_TS_SEGMENT_2, MOCK_AES_KEY_BYTES, 1);

function sendJson(res, status, data, extraHeaders = {}) {
  const body = JSON.stringify(data);
  res.writeHead(status, {
    'Content-Type': 'application/json; charset=utf-8',
    'Content-Length': Buffer.byteLength(body),
    ...extraHeaders,
  });
  res.end(body);
}

function sendText(res, status, text, contentType = 'text/plain; charset=utf-8', extraHeaders = {}) {
  const buf = Buffer.from(text, 'utf8');
  res.writeHead(status, {
    'Content-Type': contentType,
    'Content-Length': buf.length,
    ...extraHeaders,
  });
  res.end(buf);
}

function sendBuffer(res, status, buf, contentType, extraHeaders = {}) {
  res.writeHead(status, {
    'Content-Type': contentType,
    'Content-Length': buf.length,
    ...extraHeaders,
  });
  res.end(buf);
}

function sendRangeBuffer(req, res, buf, contentType, { ignoreRange = false, extraHeaders = {} } = {}) {
  const rangeHeader = ignoreRange ? null : req.headers.range;
  const parsed = parseByteRange(rangeHeader, buf.length);
  if (!parsed.present) {
    res.writeHead(200, {
      'Content-Type': contentType,
      'Content-Length': buf.length,
      ...(ignoreRange ? {} : { 'Accept-Ranges': 'bytes' }),
      ...extraHeaders,
    });
    res.end(buf);
    return;
  }
  if (!parsed.valid) {
    if (parsed.status === 416) {
      res.writeHead(416, {
        'Content-Range': `bytes */${buf.length}`,
        'Content-Length': 0,
      });
      res.end();
      return;
    }
    sendText(res, 400, 'Invalid Range header');
    return;
  }
  const slice = buf.subarray(parsed.start, parsed.end + 1);
  res.writeHead(206, {
    'Content-Type': contentType,
    'Accept-Ranges': 'bytes',
    'Content-Range': `bytes ${parsed.start}-${parsed.end}/${buf.length}`,
    'Content-Length': slice.length,
    ...extraHeaders,
  });
  res.end(slice);
}

function readRequestBody(req) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    let total = 0;
    req.on('data', (chunk) => {
      total += chunk.length;
      if (total <= 1024 * 1024) {
        chunks.push(chunk);
      }
    });
    req.on('end', () => resolve(Buffer.concat(chunks).toString('utf8')));
    req.on('error', reject);
  });
}

function checkRequiredHeaderParam(req, urlObj) {
  const reqHeader = urlObj.searchParams.get('require_header');
  if (!reqHeader) return true;
  const idx = reqHeader.indexOf(':');
  if (idx === -1) return true;
  const key = reqHeader.slice(0, idx).trim().toLowerCase();
  const expectedVal = reqHeader.slice(idx + 1).trim();
  const actualVal = req.headers[key];
  return actualVal === expectedVal;
}

export class MockOriginServer {
  constructor({ host = '127.0.0.1', port = 0 } = {}) {
    this.host = host;
    this.port = port;
    this.server = null;
    this.baseUrl = '';
    this.requests = [];
    this.sockets = new Set();
    this.activeTimers = new Set();
  }

  async start() {
    if (this.server) return this.baseUrl;

    this.server = http.createServer((req, res) => {
      this.#handleRequest(req, res).catch((err) => {
        if (!res.headersSent) {
          sendJson(res, 500, { error: 'mock_origin_internal', message: err.message });
        } else {
          res.destroy();
        }
      });
    });

    this.server.on('connection', (socket) => {
      this.sockets.add(socket);
      socket.on('close', () => this.sockets.delete(socket));
    });

    await new Promise((resolve, reject) => {
      this.server.once('error', reject);
      this.server.listen(this.port, this.host, () => {
        this.server.removeListener('error', reject);
        resolve();
      });
    });

    const addr = this.server.address();
    this.port = addr.port;
    this.baseUrl = `http://${this.host}:${this.port}`;
    return this.baseUrl;
  }

  async stop() {
    for (const timer of this.activeTimers) {
      clearTimeout(timer);
    }
    this.activeTimers.clear();

    for (const socket of this.sockets) {
      socket.destroy();
    }
    this.sockets.clear();

    if (!this.server) return;
    const srv = this.server;
    this.server = null;
    await new Promise((resolve) => srv.close(() => resolve()));
  }

  reset() {
    this.requests = [];
  }

  async #handleRequest(req, res) {
    const urlObj = new URL(req.url || '/', this.baseUrl || `http://${this.host}`);
    const pathname = urlObj.pathname;
    const query = Object.fromEntries(urlObj.searchParams.entries());
    const bodyText =
      req.method === 'POST' || req.method === 'PUT' || req.method === 'PATCH'
        ? await readRequestBody(req)
        : '';

    // Control endpoints are excluded from inbound audit log so assertions stay clean
    if (pathname === '/__mock/health') {
      return sendJson(res, 200, { status: 'ok' });
    }
    if (pathname === '/__mock/requests') {
      const limit = Number.parseInt(urlObj.searchParams.get('limit') || '100', 10);
      const safeLimit = Number.isFinite(limit) && limit > 0 ? limit : 100;
      return sendJson(res, 200, this.requests.slice(-safeLimit));
    }
    if (pathname === '/__mock/reset' && req.method === 'POST') {
      this.reset();
      return sendJson(res, 200, { ok: true });
    }

    // Record non-control request
    const normalizedHeaders = {};
    for (const [k, v] of Object.entries(req.headers)) {
      normalizedHeaders[k.toLowerCase()] = Array.isArray(v) ? v.join(', ') : v;
    }
    this.requests.push({
      method: req.method,
      path: pathname,
      query,
      headers: normalizedHeaders,
      body: bodyText,
      timestamp_ms: Date.now(),
    });
    if (this.requests.length > 2000) {
      this.requests.shift();
    }

    // -----------------------------------------------------------------------
    // A. Comic Mocks (/comic/...)
    // -----------------------------------------------------------------------
    if (pathname === '/comic/mangadex/manga') {
      const strictUa = urlObj.searchParams.get('strict_ua') === '1';
      const ua = normalizedHeaders['user-agent'] || '';
      if (strictUa && (!ua || ua.startsWith('Mozilla/5.0'))) {
        return sendText(
          res,
          403,
          '<html><head><title>Bot Challenge</title></head><body>403 Forbidden</body></html>',
          'text/html; charset=utf-8',
        );
      }
      const titleQuery = (urlObj.searchParams.get('title') || '').trim();
      if (titleQuery.toLowerCase().includes('nonexistentxyz')) {
        return sendJson(res, 200, { result: 'ok', data: [] });
      }
      return sendJson(res, 200, {
        result: 'ok',
        data: [
          {
            id: 'md-comic-1',
            type: 'manga',
            attributes: {
              title: { en: 'Mock Naruto', zh: '模拟火影' },
              description: { en: 'Mock MangaDex synopsis' },
              status: 'ongoing',
              tags: [{ attributes: { name: { en: 'Action' } } }],
            },
            relationships: [
              { type: 'cover_art', attributes: { fileName: 'cover1.jpg' } },
              { type: 'author', attributes: { name: 'Kishimoto Mock' } },
            ],
          },
        ],
      });
    }

    if (pathname === '/comic/mangadex/manga/md-comic-1') {
      return sendJson(res, 200, {
        result: 'ok',
        data: {
          id: 'md-comic-1',
          type: 'manga',
          attributes: {
            title: { en: 'Mock Naruto', zh: '模拟火影' },
            description: { en: 'Mock MangaDex synopsis' },
            status: 'ongoing',
            tags: [{ attributes: { name: { en: 'Action' } } }],
          },
          relationships: [
            { type: 'cover_art', attributes: { fileName: 'cover1.jpg' } },
            { type: 'author', attributes: { name: 'Kishimoto Mock' } },
          ],
        },
      });
    }

    if (pathname === '/comic/mangadex/manga/md-comic-1/feed') {
      const includeEmpty = urlObj.searchParams.get('include_empty') === '1';
      const chapters = [
        { id: 'md-ch-1', attributes: { chapter: '1', title: 'Arrival', pages: 3 } },
        { id: 'md-ch-2', attributes: { chapter: '2', title: 'Departure', pages: 2 } },
      ];
      if (includeEmpty) {
        chapters.push({ id: 'md-ch-empty', attributes: { chapter: '3', title: 'Empty', pages: 0 } });
      }
      return sendJson(res, 200, { result: 'ok', data: chapters });
    }

    if (pathname === '/comic/mangadex/manga/md-comic-empty/feed') {
      return sendJson(res, 200, { result: 'ok', data: [] });
    }

    if (pathname === '/comic/mangadex/at-home/server/md-ch-1') {
      return sendJson(res, 200, {
        result: 'ok',
        baseUrl: `${this.baseUrl}/comic/mangadex/cdn`,
        chapter: {
          hash: 'hash123',
          data: ['p1.jpg', 'p2.png', 'p3.jpg'],
          dataSaver: ['s1.jpg', 's2.png', 's3.jpg'],
        },
      });
    }

    if (pathname === '/comic/mangadex/at-home/server/md-ch-2') {
      return sendJson(res, 200, {
        result: 'ok',
        baseUrl: `${this.baseUrl}/comic/mangadex/cdn`,
        chapter: {
          hash: 'hash456',
          data: ['p1.jpg', 'p2.png'],
          dataSaver: ['s1.jpg', 's2.png'],
        },
      });
    }

    if (pathname === '/comic/mangadex/at-home/server/md-ch-empty') {
      return sendJson(res, 200, {
        result: 'ok',
        baseUrl: `${this.baseUrl}/comic/mangadex/cdn`,
        chapter: {
          hash: 'hashempty',
          data: [],
          dataSaver: [],
        },
      });
    }

    if (
      pathname.startsWith('/comic/mangadex/cdn/') ||
      pathname.startsWith('/comic/mangadex/covers/')
    ) {
      if (req.method === 'HEAD') {
        res.writeHead(405, { 'Content-Length': 0 });
        res.end();
        return;
      }
      if (pathname.endsWith('.png')) {
        return sendBuffer(res, 200, MOCK_PNG_BYTES, 'image/png');
      }
      return sendBuffer(res, 200, MOCK_JPEG_BYTES, 'image/jpeg');
    }

    // WEBTOON HTML Mock
    if (pathname === '/comic/webtoon/search') {
      const kw = (urlObj.searchParams.get('keyword') || '').trim();
      if (kw.toLowerCase().includes('nonexistentxyz')) {
        return sendText(
          res,
          200,
          '<!doctype html><html><body><ul class="card_lst"></ul></body></html>',
          'text/html; charset=utf-8',
        );
      }
      return sendText(
        res,
        200,
        `<!doctype html><html><body>
          <ul class="card_lst">
            <li>
              <a href="/comic/webtoon/list?title_no=95">
                <p class="subj">Tower of Mock</p>
                <p class="author">SIU Mock</p>
                <img src="/comic/webtoon/cdn/thumb.jpg">
              </a>
            </li>
          </ul>
        </body></html>`,
        'text/html; charset=utf-8',
      );
    }

    if (pathname === '/comic/webtoon/list') {
      const titleNo = urlObj.searchParams.get('title_no') || '95';
      const page = urlObj.searchParams.get('page') || '1';
      if (titleNo === '0') {
        return sendText(
          res,
          200,
          `<!doctype html><html><body>
            <h1 class="subj">Empty Webtoon</h1>
            <span class="author">Nobody</span>
            <p class="summary">No chapters yet</p>
            <ul id="_listUl"></ul>
          </body></html>`,
          'text/html; charset=utf-8',
        );
      }
      if (page === '2') {
        return sendText(
          res,
          200,
          `<!doctype html><html><body>
            <ul id="_listUl">
              <li><a href="/comic/webtoon/viewer?title_no=95&episode_no=3"><span class="subj">Ep.3 Page2</span></a></li>
            </ul>
          </body></html>`,
          'text/html; charset=utf-8',
        );
      }
      return sendText(
        res,
        200,
        `<!doctype html><html><body>
          <h1 class="subj">Tower of Mock</h1>
          <span class="author">SIU Mock</span>
          <h2 class="genre">Fantasy</h2>
          <p class="summary">Climbing the mock tower</p>
          <img class="detail_cover" src="/comic/webtoon/cdn/thumb.jpg">
          <ul id="_listUl">
            <li><a href="/comic/webtoon/viewer?title_no=95&episode_no=1"><span class="subj">Ep.1 Prologue</span></a></li>
            <li><a href="/comic/webtoon/viewer?title_no=95&episode_no=2"><span class="subj">Ep.2 Test</span></a></li>
          </ul>
          <div class="paginate"><a class="pg_next" href="/comic/webtoon/list?title_no=95&page=2">Next</a></div>
        </body></html>`,
        'text/html; charset=utf-8',
      );
    }

    if (pathname === '/comic/webtoon/viewer') {
      const ep = urlObj.searchParams.get('episode_no') || '1';
      return sendText(
        res,
        200,
        `<!doctype html><html><body>
          <div id="_imageList">
            <img data-url="/comic/webtoon/cdn/ep${ep}_1.jpg" src="/static/blank.gif">
            <img data-url="/comic/webtoon/cdn/ep${ep}_2.png" src="/static/blank.gif">
          </div>
        </body></html>`,
        'text/html; charset=utf-8',
      );
    }

    if (pathname.startsWith('/comic/webtoon/cdn/')) {
      const referer = normalizedHeaders.referer || '';
      if (!referer.startsWith('https://www.webtoons.com/')) {
        return sendText(
          res,
          403,
          '<html><body>403 Forbidden: Missing valid Referer</body></html>',
          'text/html; charset=utf-8',
        );
      }
      if (pathname.endsWith('.png')) {
        return sendBuffer(res, 200, MOCK_PNG_BYTES, 'image/png');
      }
      return sendBuffer(res, 200, MOCK_JPEG_BYTES, 'image/jpeg');
    }

    // XKCD JSON Mock
    if (pathname === '/comic/xkcd/info.0.json') {
      return sendJson(res, 200, {
        num: 999,
        title: 'Mock XKCD Strip',
        alt: 'Mouseover mock text',
        img: `${this.baseUrl}/comic/xkcd/strip.png`,
      });
    }
    if (pathname === '/comic/xkcd/strip.png') {
      return sendBuffer(res, 200, MOCK_PNG_BYTES, 'image/png');
    }

    // -----------------------------------------------------------------------
    // B. Music Mocks (/music/...)
    // -----------------------------------------------------------------------
    if (pathname === '/music/gdstudio/api.php') {
      const types = urlObj.searchParams.get('types') || 'search';
      if (types === 'search') {
        const name = (urlObj.searchParams.get('name') || '').trim();
        if (name.toLowerCase().includes('nonexistentxyz')) {
          return sendJson(res, 200, []);
        }
        return sendJson(res, 200, [
          {
            id: 'gd-101',
            name: 'Mock Mojito',
            artist: ['Jay Mock', 'Feat Guest'],
            album: 'Mock Album',
            pic_id: 'pic-101',
            url_id: 'gd-101',
            lyric_id: 'gd-101',
            source: 'netease',
          },
        ]);
      }
      if (types === 'url') {
        const id = urlObj.searchParams.get('id') || 'gd-101';
        const br = urlObj.searchParams.get('br') || '320';
        return sendJson(res, 200, {
          url: `${this.baseUrl}/music/gdstudio/cdn/${encodeURIComponent(id)}.mp3?br=${encodeURIComponent(br)}`,
          br: Number.parseInt(br, 10) || 320,
          size: MOCK_MP3_BYTES.length,
        });
      }
      if (types === 'pic') {
        return sendJson(res, 200, {
          url: `${this.baseUrl}/music/gdstudio/cdn/cover-101.jpg`,
        });
      }
      if (types === 'lyric') {
        const id = urlObj.searchParams.get('id') || 'gd-101';
        if (id === 'gd-plain') {
          return sendJson(res, 200, {
            lyric: 'Line one plain text\nLine two plain text\n',
            tlyric: '',
          });
        }
        if (id === 'gd-offset') {
          return sendJson(res, 200, {
            lyric: '[offset:-500]\n[00:01.00]Offset line\n',
            tlyric: '',
          });
        }
        return sendJson(res, 200, {
          lyric:
            '[ti:Mock Mojito]\n[ar:Jay Mock]\n[00:00.50]First synced line\n[00:02.00]Second synced line\n',
          tlyric: '[00:00.50]第一行翻译\n[00:02.00]第二行翻译\n',
        });
      }
      return sendJson(res, 400, { error: `unsupported types=${types}` });
    }

    if (pathname.startsWith('/music/gdstudio/cdn/') && pathname.endsWith('.mp3')) {
      if (urlObj.searchParams.get('require_referer') === '1' && !normalizedHeaders.referer) {
        return sendText(res, 403, 'Missing Referer');
      }
      return sendRangeBuffer(req, res, MOCK_MP3_BYTES, 'audio/mpeg');
    }

    if (pathname === '/music/gdstudio/cdn/cover-101.jpg') {
      return sendBuffer(res, 200, MOCK_JPEG_BYTES, 'image/jpeg');
    }

    if (pathname === '/music/audius/v1/tracks/trending' || pathname === '/music/audius/v1/tracks/search') {
      const q = (urlObj.searchParams.get('query') || '').trim();
      if (q.toLowerCase().includes('nonexistentxyz')) {
        return sendJson(res, 200, { data: [] });
      }
      return sendJson(res, 200, {
        data: [
          {
            id: 'aud-1',
            title: 'Fragments Mock',
            duration: 212,
            user: { name: 'Xsetra Mock' },
            artwork: { '150x150': `${this.baseUrl}/music/audius/art/aud-1.jpg` },
          },
        ],
      });
    }

    if (pathname === '/music/audius/v1/tracks/aud-1/stream') {
      // Non-Range upstream: always ignores Range and streams 200 full MP3
      return sendRangeBuffer(req, res, MOCK_MP3_BYTES, 'audio/mpeg', { ignoreRange: true });
    }

    if (pathname === '/music/audius/art/aud-1.jpg') {
      return sendBuffer(res, 200, MOCK_JPEG_BYTES, 'image/jpeg');
    }

    // -----------------------------------------------------------------------
    // C. Video Mocks (/video/...)
    // -----------------------------------------------------------------------
    if (pathname === '/video/applecms/api.php/provide/vod/' || pathname === '/video/applecms/api.php/provide/vod') {
      const ac = urlObj.searchParams.get('ac') || 'list';
      const pg = Number.parseInt(urlObj.searchParams.get('pg') || '1', 10) || 1;
      const wd = (urlObj.searchParams.get('wd') || '').trim();

      if (ac === 'list') {
        return sendJson(res, 200, {
          code: 1,
          msg: '数据列表',
          page: pg,
          pagecount: 2,
          limit: '20',
          total: 2,
          class: [
            { type_id: 1, type_name: '电影' },
            { type_id: 2, type_name: '动漫' },
          ],
          list:
            pg > 2
              ? []
              : [
                  {
                    vod_id: 1001,
                    vod_name: 'Mock Wandering Earth',
                    type_id: 1,
                    type_name: '电影',
                    vod_remarks: 'HD',
                    vod_pic: `${this.baseUrl}/video/applecms/poster/1001.jpg`,
                    vod_time: '2026-11-21 10:00:00',
                  },
                ],
        });
      }

      if (wd.toLowerCase().includes('nonexistentxyz') || pg > 2) {
        return sendJson(res, 200, {
          code: 1,
          msg: '数据列表',
          page: pg,
          pagecount: 2,
          limit: '20',
          total: 0,
          list: [],
        });
      }

      return sendJson(res, 200, {
        code: 1,
        msg: '数据列表',
        page: pg,
        pagecount: 2,
        limit: '20',
        total: 1,
        class: [
          { type_id: 1, type_name: '电影' },
          { type_id: 2, type_name: '动漫' },
        ],
        list: [
          {
            vod_id: 1001,
            vod_name: 'Mock Wandering Earth',
            type_id: 1,
            type_name: '电影',
            vod_pic: `${this.baseUrl}/video/applecms/poster/1001.jpg`,
            vod_remarks: 'HD',
            vod_year: '2026',
            vod_area: 'CN',
            vod_actor: 'Mock Actor',
            vod_director: 'Mock Director',
            vod_content: '<p>Mock sci-fi intro</p>',
            vod_play_from: 'sharehtml$$$ikm3u8$$$altm3u8',
            vod_play_url: [
              `正片$${this.baseUrl}/video/applecms/share/1001`,
              `第1集$${this.baseUrl}/video/applecms/hls/master.m3u8#第2集$${this.baseUrl}/video/applecms/hls/ep2/index.m3u8`,
              `备用$${this.baseUrl}/video/applecms/hls/alt/index.m3u8`,
            ].join('$$$'),
          },
        ],
      });
    }

    if (pathname === '/video/applecms/poster/1001.jpg') {
      return sendBuffer(res, 200, MOCK_JPEG_BYTES, 'image/jpeg');
    }

    if (pathname === '/video/applecms/share/1001') {
      return sendText(res, 200, '<html><body>Share HTML page</body></html>', 'text/html; charset=utf-8');
    }

    if (pathname === '/video/applecms/hls/master.m3u8') {
      const qs = urlObj.search || '';
      const playlist = [
        '#EXTM3U',
        '#EXT-X-VERSION:3',
        '#EXT-X-STREAM-INF:BANDWIDTH=1200000,RESOLUTION=640x360',
        `5000kb/hls/index.m3u8${qs}`,
        '',
      ].join('\n');
      return sendText(res, 200, playlist, 'application/vnd.apple.mpegurl');
    }

    if (pathname === '/video/applecms/hls/5000kb/hls/index.m3u8') {
      const withQuery = urlObj.searchParams.get('with_query') === '1';
      const reqHeader = urlObj.searchParams.get('require_header');
      const headerQs = reqHeader ? `require_header=${encodeURIComponent(reqHeader)}` : '';
      const keyUri = withQuery
        ? `../keys/enc.key?k=1${headerQs ? `&${headerQs}` : ''}`
        : `../keys/enc.key${headerQs ? `?${headerQs}` : ''}`;
      const seg0Uri = withQuery
        ? `seg0.ts?token=a&b=1${headerQs ? `&${headerQs}` : ''}`
        : `seg0.ts${headerQs ? `?${headerQs}` : ''}`;
      const seg1Uri = `${this.baseUrl}/video/applecms/hls/5000kb/hls/seg1.ts${headerQs ? `?${headerQs}` : ''}`;
      const initUri = `init.mp4${headerQs ? `?${headerQs}` : ''}`;

      const playlist = [
        '#EXTM3U',
        '#EXT-X-VERSION:6',
        '#EXT-X-TARGETDURATION:2',
        '#EXT-X-MEDIA-SEQUENCE:0',
        `#EXT-X-MAP:URI="${initUri}"`,
        `#EXT-X-KEY:METHOD=AES-128,URI="${keyUri}"`,
        '#EXTINF:1.0,',
        seg0Uri,
        '#EXTINF:1.0,',
        seg1Uri,
        '#EXT-X-ENDLIST',
        '',
      ].join('\n');
      return sendText(res, 200, playlist, 'application/vnd.apple.mpegurl');
    }

    if (pathname === '/video/applecms/hls/ep2/index.m3u8' || pathname === '/video/applecms/hls/alt/index.m3u8') {
      const playlist = [
        '#EXTM3U',
        '#EXT-X-VERSION:3',
        '#EXT-X-TARGETDURATION:2',
        '#EXT-X-MEDIA-SEQUENCE:0',
        '#EXTINF:1.0,',
        'seg0.ts',
        '#EXT-X-ENDLIST',
        '',
      ].join('\n');
      return sendText(res, 200, playlist, 'application/vnd.apple.mpegurl');
    }

    if (pathname === '/video/applecms/hls/5000kb/keys/enc.key') {
      if (!checkRequiredHeaderParam(req, urlObj)) {
        return sendText(res, 403, 'Forbidden: missing required header');
      }
      return sendBuffer(res, 200, MOCK_AES_KEY_BYTES, 'application/octet-stream');
    }

    if (pathname === '/video/applecms/hls/5000kb/hls/init.mp4') {
      if (!checkRequiredHeaderParam(req, urlObj)) {
        return sendText(res, 403, 'Forbidden: missing required header');
      }
      return sendRangeBuffer(req, res, MOCK_FMP4_INIT, 'video/mp4');
    }

    if (pathname === '/video/applecms/hls/5000kb/hls/seg0.ts') {
      if (!checkRequiredHeaderParam(req, urlObj)) {
        return sendText(res, 403, 'Forbidden: missing required header');
      }
      return sendRangeBuffer(req, res, MOCK_ENC_TS_SEGMENT_1, 'video/mp2t');
    }

    if (
      pathname === '/video/applecms/hls/ep2/seg0.ts' ||
      pathname === '/video/applecms/hls/alt/seg0.ts'
    ) {
      if (!checkRequiredHeaderParam(req, urlObj)) {
        return sendText(res, 403, 'Forbidden: missing required header');
      }
      return sendRangeBuffer(req, res, MOCK_TS_SEGMENT_1, 'video/mp2t');
    }

    if (pathname === '/video/applecms/hls/5000kb/hls/seg1.ts') {
      if (!checkRequiredHeaderParam(req, urlObj)) {
        return sendText(res, 403, 'Forbidden: missing required header');
      }
      return sendRangeBuffer(req, res, MOCK_ENC_TS_SEGMENT_2, 'video/mp2t');
    }

    // PeerTube JSON Mock
    if (pathname === '/video/peertube/api/v1/search/videos') {
      const search = (urlObj.searchParams.get('search') || '').trim();
      if (search.toLowerCase().includes('nonexistentxyz')) {
        return sendJson(res, 200, { total: 0, data: [] });
      }
      return sendJson(res, 200, {
        total: 1,
        data: [
          {
            uuid: 'pt-vid-1',
            name: 'Mock Blender Open Movie',
            thumbnailPath: '/video/peertube/thumb/pt-vid-1.jpg',
            duration: 10,
          },
        ],
      });
    }

    if (pathname === '/video/peertube/api/v1/videos/pt-vid-1') {
      return sendJson(res, 200, {
        uuid: 'pt-vid-1',
        name: 'Mock Blender Open Movie',
        description: 'PeerTube MP4 mock',
        files: [
          {
            resolution: { id: 360 },
            fileUrl: `${this.baseUrl}/video/peertube/media/pt-vid-1.mp4`,
          },
        ],
      });
    }

    if (pathname === '/video/peertube/thumb/pt-vid-1.jpg') {
      return sendBuffer(res, 200, MOCK_JPEG_BYTES, 'image/jpeg');
    }

    if (pathname === '/video/peertube/media/pt-vid-1.mp4') {
      return sendRangeBuffer(req, res, MOCK_MP4_BYTES, 'video/mp4');
    }

    if (pathname === '/video/subs/wildcard-cors.vtt') {
      res.writeHead(200, {
        'Content-Type': 'text/vtt; charset=utf-8',
        'Access-Control-Allow-Origin': '*',
      });
      res.end('WEBVTT\n\n00:00:00.000 --> 00:00:10.000\nMock Wildcard CORS Cue\n');
      return;
    }

    if (pathname === '/video/subs/referer-gated.srt') {
      const referer = normalizedHeaders.referer || '';
      if (!referer.startsWith('https://sub.mock.test/')) {
        return sendText(res, 403, 'Forbidden: missing subtitle Referer');
      }
      return sendText(
        res,
        200,
        '1\n00:00:00,000 --> 00:00:10,000\nMock Referer SRT Cue\n',
        'text/plain; charset=utf-8',
      );
    }

    if (pathname === '/video/subtitled/play') {
      return sendJson(res, 200, {
        stream: `${this.baseUrl}/video/peertube/media/pt-vid-1.mp4`,
        subs: [
          {
            label: 'Wildcard VTT',
            lang: 'en',
            url: `${this.baseUrl}/video/subs/wildcard-cors.vtt`,
          },
          {
            label: 'Referer SRT',
            lang: 'zh',
            url: `${this.baseUrl}/video/subs/referer-gated.srt`,
          },
        ],
      });
    }

    // -----------------------------------------------------------------------
    // D. Security, Error, Encoding & Edge Mocks (/edge/...)
    // -----------------------------------------------------------------------
    if (pathname === '/edge/svg-as-image') {
      return sendText(
        res,
        200,
        '<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>',
        'image/svg+xml',
      );
    }

    if (pathname === '/edge/html-as-image' || pathname === '/edge/html-as-image.jpg') {
      return sendText(res, 200, '<html><body>Not an image</body></html>', 'text/html; charset=utf-8');
    }

    if (pathname === '/edge/jpeg-wrong-ct') {
      return sendBuffer(res, 200, MOCK_JPEG_BYTES, 'application/octet-stream');
    }

    if (pathname === '/edge/delay') {
      const ms = Math.max(0, Math.min(Number.parseInt(urlObj.searchParams.get('ms') || '1000', 10) || 0, 60000));
      let closed = false;
      const timer = setTimeout(() => {
        this.activeTimers.delete(timer);
        if (!closed && !res.writableEnded) {
          sendJson(res, 200, { delayed_ms: ms, ok: true });
        }
      }, ms);
      this.activeTimers.add(timer);
      req.on('close', () => {
        closed = true;
        clearTimeout(timer);
        this.activeTimers.delete(timer);
      });
      return;
    }

    if (pathname === '/edge/error-502') {
      return sendJson(res, 502, { error: 'upstream_down' });
    }

    if (pathname === '/edge/error-500-html') {
      return sendText(
        res,
        500,
        '<html><body><h1>500 Internal Server Error</h1></body></html>',
        'text/html; charset=utf-8',
      );
    }

    if (pathname === '/edge/malformed-json') {
      return sendText(res, 200, '{ "broken": [1, 2, ', 'application/json');
    }

    if (pathname === '/edge/malformed-html') {
      return sendText(
        res,
        200,
        '<div><span>unclosed broken html<p class="item">Partial</p><img src="/bad"',
        'text/html; charset=utf-8',
      );
    }

    if (pathname === '/edge/malformed-m3u8') {
      return sendText(res, 200, 'not an m3u8 playlist', 'application/vnd.apple.mpegurl');
    }

    if (pathname === '/edge/gbk-html') {
      return sendBuffer(res, 200, MOCK_GBK_HTML_BYTES, 'text/html; charset=GBK');
    }

    if (pathname === '/edge/redirect-valid') {
      const to = urlObj.searchParams.get('to') || '/comic/xkcd/strip.png';
      const safeTo = to.startsWith('/') ? to : '/comic/xkcd/strip.png';
      res.writeHead(302, { Location: safeTo, 'Content-Length': 0 });
      res.end();
      return;
    }

    if (pathname === '/edge/redirect-loopback') {
      const target =
        urlObj.searchParams.get('target') === 'loopback'
          ? 'http://127.0.0.1:9/secret'
          : 'http://169.254.169.254/latest/meta-data/';
      res.writeHead(302, { Location: target, 'Content-Length': 0 });
      res.end();
      return;
    }

    if (pathname === '/edge/redirect-chain') {
      const maxHops = Math.max(1, Math.min(Number.parseInt(urlObj.searchParams.get('n') || '8', 10) || 8, 32));
      const step = Number.parseInt(urlObj.searchParams.get('step') || '1', 10) || 1;
      if (step <= maxHops) {
        res.writeHead(302, {
          Location: `/edge/redirect-chain?step=${step + 1}&n=${maxHops}`,
          'Content-Length': 0,
        });
        res.end();
        return;
      }
      return sendJson(res, 200, { ok: true, hops: maxHops });
    }

    if (pathname === '/edge/auth/login') {
      let username = '';
      let password = '';
      try {
        const parsed = JSON.parse(bodyText);
        username = parsed.username || '';
        password = parsed.password || '';
      } catch {
        const params = new URLSearchParams(bodyText);
        username = params.get('username') || '';
        password = params.get('password') || '';
      }
      if (username === 'alice' && password === 'secret') {
        return sendJson(
          res,
          200,
          { ok: true, user: 'alice' },
          { 'Set-Cookie': 'mock_sess=valid_cookie_42; Path=/edge/gated; HttpOnly' },
        );
      }
      return sendJson(res, 401, { ok: false, error: 'invalid_credentials' });
    }

    if (pathname === '/edge/gated/comic-pages') {
      const cookie = normalizedHeaders.cookie || '';
      if (!cookie.includes('mock_sess=valid_cookie_42')) {
        return sendJson(res, 401, { error: 'auth_required' });
      }
      return sendJson(res, 200, {
        pages: [`${this.baseUrl}/comic/xkcd/strip.png`],
      });
    }

    if (pathname === '/edge/oversized-image') {
      const totalBytes = 17 * 1024 * 1024; // 17 MiB > 16 MiB cap
      await this.#streamOversized(req, res, totalBytes, 'image/jpeg', MOCK_JPEG_BYTES);
      return;
    }

    if (pathname === '/edge/oversized-m3u8') {
      const totalBytes = 9 * 1024 * 1024; // 9 MiB > 8 MiB cap
      const header = Buffer.from('#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:2\n', 'utf8');
      const unit = Buffer.from('#EXTINF:1.0,\nseg.ts\n'.repeat(2048), 'utf8');
      await this.#streamOversized(req, res, totalBytes, 'application/vnd.apple.mpegurl', header, unit);
      return;
    }

    if (pathname === '/edge/post-echo') {
      return sendJson(res, 200, {
        ok: true,
        method: req.method,
        headers: normalizedHeaders,
        body: bodyText,
        items: [
          {
            title: 'Echo Result',
            author: normalizedHeaders['x-custom'] || 'none',
            url: `${this.baseUrl}/comic/xkcd/strip.png`,
            echo: bodyText,
          },
        ],
      });
    }

    if (pathname === '/edge/cdp-smoke.html') {
      return sendText(
        res,
        200,
        `<!doctype html>
        <html lang="en">
          <head><meta charset="utf-8"><title>CDP Smoke</title></head>
          <body>
            <h1 id="heading">Mock Origin CDP Smoke</h1>
            <img id="strip" src="/comic/xkcd/strip.png" alt="strip">
            <input id="kw" type="text" value="">
            <button id="btn" type="button">Go</button>
            <div id="out">idle</div>
            <script>
              document.getElementById('btn').addEventListener('click', () => {
                const v = document.getElementById('kw').value;
                document.getElementById('out').textContent = 'clicked:' + v;
              });
            </script>
          </body>
        </html>`,
        'text/html; charset=utf-8',
      );
    }

    return sendJson(res, 404, { error: 'not_found', path: pathname });
  }

  async #streamOversized(req, res, totalBytes, contentType, prefixBuf, repeatChunk = null) {
    let aborted = false;
    req.on('close', () => {
      aborted = true;
    });

    res.writeHead(200, {
      'Content-Type': contentType,
      'Content-Length': totalBytes,
    });

    const chunkTemplate = repeatChunk || Buffer.alloc(64 * 1024, 0);
    let sent = 0;

    const writeChunk = async (buf) => {
      if (aborted || res.destroyed) return false;
      const canContinue = res.write(buf);
      if (!canContinue) {
        await new Promise((resolve) => {
          const onDone = () => {
            res.off('drain', onDone);
            res.off('close', onDone);
            resolve();
          };
          res.once('drain', onDone);
          res.once('close', onDone);
        });
      }
      return !aborted && !res.destroyed;
    };

    const firstSlice = prefixBuf.subarray(0, Math.min(prefixBuf.length, totalBytes));
    if (!(await writeChunk(firstSlice))) return;
    sent += firstSlice.length;

    while (sent < totalBytes) {
      const remaining = totalBytes - sent;
      const piece =
        remaining >= chunkTemplate.length ? chunkTemplate : chunkTemplate.subarray(0, remaining);
      if (!(await writeChunk(piece))) return;
      sent += piece.length;
    }
    if (!aborted && !res.destroyed) {
      res.end();
    }
  }
}
