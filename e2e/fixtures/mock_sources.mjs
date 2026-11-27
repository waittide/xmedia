export function buildMockComicSources(mockBaseUrl, { strictUa = false } = {}) {
  const uaSuffix = strictUa ? '&strict_ua=1' : '';
  const mangadex = {
    sourceName: 'Mock MangaDex',
    sourceUrl: `${mockBaseUrl}/comic/mangadex`,
    sourceGroup: 'Mock-Comic',
    sourceType: 2,
    enabled: true,
    weight: 100,
    readingDirection: 'rtl',
    header: JSON.stringify({ 'User-Agent': 'xmedia/0.1.0' }),
    searchUrl: `${mockBaseUrl}/comic/mangadex/manga?title={{key}}&limit=10${uaSuffix}`,
    ruleSearch: {
      bookList: '$.data[*]',
      name: '$.attributes.title.en||$.attributes.title.zh',
      author: "$.relationships[?(@.type=='author')].attributes.name",
      bookUrl: `${mockBaseUrl}/comic/mangadex/manga/{{$.id}}`,
      coverUrl: `${mockBaseUrl}/comic/mangadex/covers/{{$.relationships[?(@.type=='cover_art')].attributes.fileName}}`,
      intro: '$.attributes.description.en',
      kind: '$.attributes.status',
    },
    ruleDetail: {
      name: '$.data.attributes.title.en||$.data.attributes.title.zh',
      author: "$.data.relationships[?(@.type=='author')].attributes.name",
      intro: '$.data.attributes.description.en',
      coverUrl: `${mockBaseUrl}/comic/mangadex/covers/{{$.data.relationships[?(@.type=='cover_art')].attributes.fileName}}`,
      tocUrl: `${mockBaseUrl}/comic/mangadex/manga/{{$.data.id}}/feed`,
      readingDirection: 'rtl',
    },
    ruleToc: {
      chapterList: '$.data[*]',
      chapterName: 'Ch.{{$.attributes.chapter}} {{$.attributes.title}}',
      chapterUrl: `${mockBaseUrl}/comic/mangadex/at-home/server/{{$.id}}`,
    },
    rulePages: {
      content: `<js>
const d = typeof result === 'string' ? JSON.parse(result) : result;
const base = d.baseUrl;
const h = d.chapter.hash;
const list = (d.chapter.dataSaver && d.chapter.dataSaver.length) ? d.chapter.dataSaver : (d.chapter.data || []);
list.map(f => base + '/data-saver/' + h + '/' + f).join('\\n');
</js>`,
    },
  };

  const webtoon = {
    sourceName: 'Mock WEBTOON',
    sourceUrl: `${mockBaseUrl}/comic/webtoon`,
    sourceGroup: 'Mock-Comic',
    sourceType: 2,
    enabled: true,
    weight: 90,
    readingDirection: 'webtoon',
    imageHeader: JSON.stringify({ Referer: 'https://www.webtoons.com/' }),
    searchUrl: `${mockBaseUrl}/comic/webtoon/search?keyword={{key}}`,
    ruleSearch: {
      bookList: '.card_lst li',
      name: '.subj@text',
      author: '.author@text',
      bookUrl: 'a@href',
      coverUrl: 'img@src',
    },
    ruleDetail: {
      name: 'h1.subj@text',
      author: '.author@text',
      intro: '.summary@text',
      kind: '.genre@text',
      coverUrl: 'img.detail_cover@src',
      readingDirection: 'webtoon',
    },
    ruleToc: {
      chapterList: '#_listUl li',
      chapterName: '.subj@text',
      chapterUrl: 'a@href',
      nextTocUrl: '.paginate a.pg_next@href',
    },
    rulePages: {
      content: '#_imageList img@data-url',
    },
  };

  const xkcd = {
    sourceName: 'Mock XKCD',
    sourceUrl: `${mockBaseUrl}/comic/xkcd`,
    sourceGroup: 'Mock-Comic',
    sourceType: 2,
    enabled: true,
    weight: 80,
    readingDirection: 'ltr',
    searchUrl: `${mockBaseUrl}/comic/xkcd/info.0.json`,
    exploreUrl: `Latest::${mockBaseUrl}/comic/xkcd/info.0.json`,
    ruleSearch: {
      bookList: '$',
      name: '$.title',
      bookUrl: `@js:${JSON.stringify(`${mockBaseUrl}/comic/xkcd/info.0.json`)}`,
      coverUrl: '$.img',
      intro: '$.alt',
    },
    ruleExplore: {
      bookList: '$',
      name: '$.title',
      bookUrl: `@js:${JSON.stringify(`${mockBaseUrl}/comic/xkcd/info.0.json`)}`,
      coverUrl: '$.img',
      intro: '$.alt',
    },
    ruleDetail: {
      name: '$.title',
      coverUrl: '$.img',
      intro: '$.alt',
      tocUrl: `@js:${JSON.stringify(`${mockBaseUrl}/comic/xkcd/info.0.json`)}`,
      readingDirection: 'ltr',
    },
    ruleToc: {
      chapterList: '$',
      chapterName: '$.title',
      chapterUrl: `@js:${JSON.stringify(`${mockBaseUrl}/comic/xkcd/info.0.json`)}`,
    },
    rulePages: {
      content: '$.img',
    },
  };

  return {
    mangadex,
    webtoon,
    xkcd,
    all: [mangadex, webtoon, xkcd],
  };
}

export function buildMockMusicSources(mockBaseUrl) {
  const gdstudio = {
    sourceName: 'Mock GDStudio',
    sourceUrl: `${mockBaseUrl}/music/gdstudio`,
    sourceGroup: 'Mock-Music',
    enabled: true,
    weight: 100,
    supportedQualities: ['128', '320', 'flac'],
    searchUrl: `${mockBaseUrl}/music/gdstudio/api.php?types=search&source=netease&name={{key}}&count={{limit}}&pages={{page}}`,
    ruleSearch: {
      trackList: '$[*]',
      trackId: '$.id',
      title: '$.name',
      artist: '$.artist',
      album: '$.album',
      coverId: '$.pic_id',
      urlId: '$.url_id',
      lyricId: '$.lyric_id',
    },
    ruleStream: {
      requestUrl: `${mockBaseUrl}/music/gdstudio/api.php?types=url&source=netease&id={{urlId}}&br={{quality}}`,
      streamUrl: '$.url',
      bitrate: '$.br',
    },
    ruleCover: {
      requestUrl: `${mockBaseUrl}/music/gdstudio/api.php?types=pic&source=netease&id={{coverId}}&size=300`,
      coverUrl: '$.url',
    },
    ruleLyric: {
      requestUrl: `${mockBaseUrl}/music/gdstudio/api.php?types=lyric&source=netease&id={{lyricId}}`,
      lyric: '$.lyric',
      translatedLyric: '$.tlyric',
    },
  };

  const audius = {
    sourceName: 'Mock Audius',
    sourceUrl: `${mockBaseUrl}/music/audius`,
    sourceGroup: 'Mock-Music',
    enabled: true,
    weight: 90,
    supportedQualities: ['320'],
    searchUrl: `${mockBaseUrl}/music/audius/v1/tracks/search?query={{key}}`,
    exploreUrl: `Trending::${mockBaseUrl}/music/audius/v1/tracks/trending`,
    ruleSearch: {
      trackList: '$.data[*]',
      trackId: '$.id',
      title: '$.title',
      artist: '$.user.name',
      duration: '$.duration',
      coverUrl: '$.artwork.150x150',
      playUrl: `${mockBaseUrl}/music/audius/v1/tracks/{{$.id}}/stream`,
    },
    ruleExplore: {
      trackList: '$.data[*]',
      trackId: '$.id',
      title: '$.title',
      artist: '$.user.name',
      duration: '$.duration',
      coverUrl: '$.artwork.150x150',
      playUrl: `${mockBaseUrl}/music/audius/v1/tracks/{{$.id}}/stream`,
    },
    ruleStream: {
      streamUrl: '{{playUrl}}',
    },
  };

  return {
    gdstudio,
    audius,
    all: [gdstudio, audius],
  };
}

export function buildMockVideoSources(mockBaseUrl) {
  const applecms = {
    sourceName: 'Mock AppleCMS',
    sourceUrl: `${mockBaseUrl}/video/applecms`,
    sourceGroup: 'Mock-Video',
    sourceFormat: 'applecms_v10',
    apiUrl: `${mockBaseUrl}/video/applecms/api.php/provide/vod/`,
    enabled: true,
    weight: 100,
    preferredPlayFrom: ['ikm3u8', 'altm3u8'],
    playUrlFilterRegex: '(?i)\\.(m3u8|mp4|mkv|webm)(\\?|$)',
  };

  const applecmsToken = {
    sourceName: 'Mock AppleCMS Token',
    sourceUrl: `${mockBaseUrl}/video/applecms-token`,
    sourceGroup: 'Mock-Video',
    sourceFormat: 'applecms_v10',
    apiUrl: `${mockBaseUrl}/video/applecms/api.php/provide/vod/`,
    enabled: true,
    weight: 85,
    mediaHeader: JSON.stringify({ 'X-Video-Token': 'secret123' }),
    preferredPlayFrom: ['ikm3u8', 'altm3u8'],
    playUrlFilterRegex: '(?i)\\.(m3u8|mp4|mkv|webm)(\\?|$)',
  };

  const peertube = {
    sourceName: 'Mock PeerTube',
    sourceUrl: `${mockBaseUrl}/video/peertube`,
    sourceGroup: 'Mock-Video',
    sourceFormat: 'rule',
    enabled: true,
    weight: 90,
    searchUrl: `${mockBaseUrl}/video/peertube/api/v1/search/videos?search={{key}}`,
    ruleSearch: {
      list: '$.data[*]',
      vodId: '$.uuid',
      name: '$.name',
      coverUrl: `${mockBaseUrl}{{$.thumbnailPath}}`,
      detailUrl: `${mockBaseUrl}/video/peertube/api/v1/videos/{{$.uuid}}`,
    },
    ruleDetail: {
      name: '$.name',
      intro: '$.description',
      episodes: '$.files[*]',
      episodeName: '{{$.resolution.id}}p',
      episodeUrl: '$.fileUrl',
    },
    rulePlay: {
      streamUrl: '{{playUrl}}',
      streamType: 'mp4',
    },
  };

  return {
    applecms,
    applecmsToken,
    peertube,
    all: [applecms, peertube],
  };
}

export function buildMockBookSources(mockBaseUrl) {
  const basicBook = {
    bookSourceName: 'Mock Legado Book',
    bookSourceUrl: `${mockBaseUrl}/edge/book`,
    bookSourceGroup: 'Mock-Book',
    bookSourceType: 0,
    enabled: true,
    weight: 100,
    searchUrl: `${mockBaseUrl}/comic/webtoon/search?keyword={{key}}`,
    ruleSearch: {
      bookList: '.card_lst li',
      name: '.subj@text',
      author: '.author@text',
      bookUrl: 'a@href',
      coverUrl: 'img@src',
    },
    ruleBookInfo: {
      name: 'h1.subj@text',
      author: '.author@text',
      intro: '.summary@text',
    },
    ruleToc: {
      chapterList: '#_listUl li',
      chapterName: '.subj@text',
      chapterUrl: 'a@href',
    },
    ruleContent: {
      content: 'body@text',
    },
  };
  return { basicBook, all: [basicBook] };
}

export function buildEdgeMockSources(mockBaseUrl) {
  return {
    error502: {
      sourceName: 'Edge 502 Source',
      sourceUrl: `${mockBaseUrl}/edge/error-502`,
      sourceGroup: 'Edge',
      sourceType: 2,
      enabled: true,
      searchUrl: `${mockBaseUrl}/edge/error-502`,
      ruleSearch: { bookList: '$.items[*]', name: '$.title', bookUrl: '$.url' },
    },
    delayTimeout: {
      sourceName: 'Edge Slow Timeout Source',
      sourceUrl: `${mockBaseUrl}/edge/delay`,
      sourceGroup: 'Edge',
      sourceType: 2,
      enabled: true,
      searchUrl: `${mockBaseUrl}/edge/delay?ms=25000`,
      ruleSearch: { bookList: '$.items[*]', name: '$.title', bookUrl: '$.url' },
    },
    malformedJson: {
      sourceName: 'Edge Malformed JSON',
      sourceUrl: `${mockBaseUrl}/edge/malformed-json`,
      sourceGroup: 'Edge',
      sourceType: 2,
      enabled: true,
      searchUrl: `${mockBaseUrl}/edge/malformed-json`,
      ruleSearch: { bookList: '$.items[*]', name: '$.title', bookUrl: '$.url' },
    },
    malformedHtml: {
      sourceName: 'Edge Malformed HTML',
      sourceUrl: `${mockBaseUrl}/edge/malformed-html`,
      sourceGroup: 'Edge',
      sourceType: 2,
      enabled: true,
      searchUrl: `${mockBaseUrl}/edge/malformed-html`,
      ruleSearch: {
        bookList: '.item',
        name: '@text',
        bookUrl: `@js:${JSON.stringify(`${mockBaseUrl}/comic/xkcd/info.0.json`)}`,
      },
    },
    gbkHtml: {
      sourceName: 'Edge GBK HTML',
      sourceUrl: `${mockBaseUrl}/edge/gbk-html`,
      sourceGroup: 'Edge',
      sourceType: 2,
      enabled: true,
      searchUrl: `${mockBaseUrl}/edge/gbk-html`,
      ruleSearch: {
        bookList: 'h1.title',
        name: '@text',
        bookUrl: `@js:${JSON.stringify(`${mockBaseUrl}/comic/xkcd/info.0.json`)}`,
      },
    },
    postEcho: {
      sourceName: 'Edge POST Echo',
      sourceUrl: `${mockBaseUrl}/edge/post-echo`,
      sourceGroup: 'Edge',
      sourceType: 2,
      enabled: true,
      searchUrl: `${mockBaseUrl}/edge/post-echo,${JSON.stringify({
        method: 'POST',
        headers: { 'X-Custom': 'abc', 'Content-Type': 'application/x-www-form-urlencoded' },
        body: 'q={{key}}',
      })}`,
      ruleSearch: {
        bookList: '$.items[*]',
        name: '$.title',
        author: '$.author',
        bookUrl: '$.url',
      },
    },
    cookieGated: {
      sourceName: 'Edge Cookie Gated Comic',
      sourceUrl: `${mockBaseUrl}/edge/gated`,
      sourceGroup: 'Edge',
      sourceType: 2,
      enabled: true,
      loginUrl: `@js:function login() {
const info = source.getLoginInfoMap();
const payload = JSON.stringify({ username: info.username || '', password: info.password || '' });
java.post('${mockBaseUrl}/edge/auth/login', payload, { 'Content-Type': 'application/json' });
}
login();`,
      loginUi: JSON.stringify([
        { name: 'username', type: 'text' },
        { name: 'password', type: 'password' },
      ]),
      loginCheckJs: `<js>
const payload = JSON.stringify({ username: source.getLoginInfoMap().username || '', password: source.getLoginInfoMap().password || '' });
java.post('${mockBaseUrl}/edge/auth/login', payload, { 'Content-Type': 'application/json' });
</js>`,
      searchUrl: `${mockBaseUrl}/comic/xkcd/info.0.json`,
      ruleSearch: {
        bookList: '$',
        name: '$.title',
        bookUrl: `@js:${JSON.stringify(`${mockBaseUrl}/edge/gated/comic-pages`)}`,
      },
      ruleDetail: {
        name: '@js:"Gated Comic"',
        tocUrl: `@js:${JSON.stringify(`${mockBaseUrl}/edge/gated/comic-pages`)}`,
      },
      ruleToc: {
        chapterList: '$',
        chapterName: '@js:"Gated Chapter 1"',
        chapterUrl: `@js:${JSON.stringify(`${mockBaseUrl}/edge/gated/comic-pages`)}`,
      },
      rulePages: {
        content: '$.pages[*]',
      },
    },
    quickJsInfiniteLoop: {
      sourceName: 'Edge QuickJS Infinite Loop',
      sourceUrl: `${mockBaseUrl}/edge/qjs-loop`,
      sourceGroup: 'Edge',
      sourceType: 2,
      enabled: true,
      searchUrl: `${mockBaseUrl}/comic/xkcd/info.0.json`,
      ruleSearch: {
        bookList: '<js>while(true){}</js>',
        name: '$.title',
        bookUrl: '$.img',
      },
    },
    quickJsMemoryHog: {
      sourceName: 'Edge QuickJS Memory Hog',
      sourceUrl: `${mockBaseUrl}/edge/qjs-oom`,
      sourceGroup: 'Edge',
      sourceType: 2,
      enabled: true,
      searchUrl: `${mockBaseUrl}/comic/xkcd/info.0.json`,
      ruleSearch: {
        bookList: '<js>const arr = new Array(100000000).fill(1); arr.length;</js>',
        name: '$.title',
        bookUrl: '$.img',
      },
    },
    quickJsHttpFlood: {
      sourceName: 'Edge QuickJS HTTP Flood',
      sourceUrl: `${mockBaseUrl}/edge/qjs-http`,
      sourceGroup: 'Edge',
      sourceType: 2,
      enabled: true,
      searchUrl: `${mockBaseUrl}/comic/xkcd/info.0.json`,
      ruleSearch: {
        bookList: `<js>for (let i = 0; i < 40; i++) { java.ajax('${mockBaseUrl}/comic/xkcd/info.0.json'); } '[]';</js>`,
        name: '$.title',
        bookUrl: '$.img',
      },
    },
  };
}
