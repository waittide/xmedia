// XMedia Plugin v2 TypeScript SDK Ambient Declarations
// Compatible with ES2023 modules evaluated in XMedia's QuickJS runtime.

export type PluginDomain = "book" | "comic" | "music" | "video";

export type ByteEncoding = "utf8" | "hex" | "base64";

export interface PluginHttpRequestOptions {
  url: string;
  method?: "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD";
  query?: Record<string, string | number | boolean | null | undefined>;
  headers?: Record<string, string>;
  form?: Record<string, string | number | boolean | null | undefined>;
  json?: unknown;
  body?: string;
}

export interface PluginHttpResponse {
  readonly status: number;
  readonly ok: boolean;
  readonly url: string;
  readonly headers: Record<string, string>;
  readonly text: string;
  readonly bodyBase64: string;
  readonly dataUrl: string;
  json<T = any>(): T;
}

export interface PluginHttpClient {
  request(options: PluginHttpRequestOptions): PluginHttpResponse;
  get(
    url: string,
    options?: Omit<PluginHttpRequestOptions, "url" | "method">
  ): PluginHttpResponse;
  post(
    url: string,
    bodyOrOptions?: unknown,
    options?: Omit<PluginHttpRequestOptions, "url" | "method">
  ): PluginHttpResponse;
}

export interface HmacOptions {
  keyEncoding?: ByteEncoding;
  inputEncoding?: ByteEncoding;
  outputEncoding?: ByteEncoding;
}

export interface AesCipherOptions {
  mode?: "cbc" | "ecb" | "gcm";
  padding?: "pkcs7" | "zero" | "none";
  key: string;
  keyEncoding?: ByteEncoding;
  iv?: string;
  ivEncoding?: ByteEncoding;
  aad?: string;
  aadEncoding?: ByteEncoding;
  data: string;
  inputEncoding?: ByteEncoding;
  outputEncoding?: ByteEncoding;
}

export interface RsaEncryptOptions {
  padding?: "pkcs1v15" | "oaep-sha256" | "none";
  publicKeyPem?: string;
  modulusHex?: string;
  exponentHex?: string;
  data: string;
  inputEncoding?: ByteEncoding;
  outputEncoding?: ByteEncoding;
}

export interface PluginCrypto {
  md5(data: string, inputEncoding?: ByteEncoding, outputEncoding?: ByteEncoding): string;
  sha1(data: string, inputEncoding?: ByteEncoding, outputEncoding?: ByteEncoding): string;
  sha256(data: string, inputEncoding?: ByteEncoding, outputEncoding?: ByteEncoding): string;
  hmacSha1(key: string, data: string, options?: HmacOptions | ByteEncoding): string;
  hmacSha256(key: string, data: string, options?: HmacOptions | ByteEncoding): string;
  aesEncrypt(options: AesCipherOptions): string;
  aesDecrypt(options: AesCipherOptions): string;
  rsaEncrypt(options: RsaEncryptOptions): string;
  randomBytes(byteLength?: number, outputEncoding?: ByteEncoding): string;
  base64Encode(text: string): string;
  base64Decode(base64: string): string;
  hexEncode(text: string): string;
  hexDecode(hex: string): string;
}

export interface PluginCookies {
  get(url?: string, name?: string): string;
  getAll(url?: string): Record<string, string>;
  set(url: string, cookieHeaderValue: string): void;
  remove(url?: string): void;
  clear(): void;
}

export interface PluginStorage {
  get(key: string): string;
  set(key: string, value: string): void;
  remove(key: string): void;
  getJson<T = any>(key: string): T | null;
  setJson(key: string, value: unknown): void;
  clear(): void;
}

export interface PluginHtml {
  selectText(html: string, rule: string, baseUrl?: string): string;
  selectList(html: string, rule: string): string[];
}

export interface PluginContext<S extends Record<string, any> = Record<string, any>> {
  readonly http: PluginHttpClient;
  readonly crypto: PluginCrypto;
  readonly cookies: PluginCookies;
  readonly storage: PluginStorage;
  readonly settings: Readonly<S>;
  readonly html: PluginHtml;
  log(...args: unknown[]): void;
  toast(message: string): void;
}

export interface PluginSettingOption {
  label: string;
  value: string;
}

export interface PluginSettingField {
  key: string;
  label: string;
  type: "select" | "switch" | "text" | "password";
  default?: string | boolean | number | null;
  options?: PluginSettingOption[];
  description?: string;
  placeholder?: string;
}

export interface PluginExploreCategory {
  title: string;
  id: string;
}

export interface PluginProfile {
  loggedIn: boolean;
  userId?: string;
  nickname?: string;
  avatarUrl?: string;
  vipLabel?: string;
  message?: string;
}

export interface PluginQrCreateResult {
  key: string;
  qrUrl?: string;
  qrImage?: string;
  message?: string;
}

export interface PluginQrPollResult {
  status: "waiting" | "scanned" | "confirmed" | "expired" | "failed";
  message?: string;
  profile?: PluginProfile;
}

export interface PluginOtpSendResult {
  ok?: boolean;
  cooldownSecs?: number;
  message?: string;
}

export interface PluginLoginVerifyResult {
  ok?: boolean;
  message?: string;
  profile?: PluginProfile;
}

export interface PluginAuthSpec<S extends Record<string, any> = Record<string, any>> {
  description?: string;
  cookieHint?: string;
  qrCode?: {
    create(ctx: PluginContext<S>): PluginQrCreateResult | Promise<PluginQrCreateResult>;
    poll(
      ctx: PluginContext<S>,
      params: { key: string }
    ): PluginQrPollResult | Promise<PluginQrPollResult>;
  };
  otp?: {
    send(
      ctx: PluginContext<S>,
      params: { countryCode: string; phone: string }
    ): PluginOtpSendResult | Promise<PluginOtpSendResult>;
    verify(
      ctx: PluginContext<S>,
      params: { countryCode: string; phone: string; code: string }
    ): PluginLoginVerifyResult | Promise<PluginLoginVerifyResult>;
  };
  password?:
    | ((
        ctx: PluginContext<S>,
        params: { username: string; password: string }
      ) => PluginLoginVerifyResult | Promise<PluginLoginVerifyResult>)
    | {
        login(
          ctx: PluginContext<S>,
          params: { username: string; password: string }
        ): PluginLoginVerifyResult | Promise<PluginLoginVerifyResult>;
      };
  cookie?:
    | boolean
    | {
        hint?: string;
        login?(
          ctx: PluginContext<S>,
          params: { cookie: string }
        ): PluginLoginVerifyResult | Promise<PluginLoginVerifyResult>;
      };
  getProfile?(ctx: PluginContext<S>): PluginProfile | Promise<PluginProfile>;
  logout?(ctx: PluginContext<S>): { ok?: boolean } | void | Promise<{ ok?: boolean } | void>;
}

export interface BasePluginManifest<S extends Record<string, any> = Record<string, any>> {
  id?: string;
  name: string;
  version?: string;
  domain: PluginDomain;
  baseUrl: string;
  group?: string;
  description?: string;
  checkKeyword?: string;
  rateLimit?: string;
  headers?: Record<string, string>;
  mediaHeaders?: Record<string, string>;
  exploreCategories?: PluginExploreCategory[];
  settings?: PluginSettingField[];
  auth?: PluginAuthSpec<S>;
  enabled?: boolean;
  weight?: number;
}

// Music Domain Contracts

export interface MusicTrackInput {
  id: string;
  trackId: string;
  title?: string;
  artist?: string;
  album?: string | null;
  durationSecs?: number | null;
  coverUrl?: string | null;
  coverId?: string | null;
  urlId?: string | null;
  lyricId?: string | null;
  playUrl?: string | null;
  detailUrl?: string | null;
  quality?: string | null;
}

export interface MusicTrackResult {
  id: string;
  title: string;
  artist?: string;
  artists?: Array<string | { name: string }>;
  album?: string | { name: string } | null;
  durationSecs?: number;
  durationMs?: number;
  duration?: string;
  coverUrl?: string | null;
  coverId?: string | null;
  urlId?: string | null;
  lyricId?: string | null;
  playUrl?: string | null;
  detailUrl?: string | null;
  format?: string | null;
}

export interface MusicStreamResult {
  url: string;
  format?: string;
  bitrate?: number;
  headers?: Record<string, string>;
  expiresInSecs?: number;
}

export interface MusicLyricsResult {
  lyric: string;
  translatedLyric?: string | null;
}

export interface MusicPluginDefinition<S extends Record<string, any> = Record<string, any>>
  extends BasePluginManifest<S> {
  domain: "music";
  qualities?: string[];
  search(
    ctx: PluginContext<S>,
    params: { keyword: string; page: number; limit: number }
  ): MusicTrackResult[] | { tracks: MusicTrackResult[] } | Promise<MusicTrackResult[] | { tracks: MusicTrackResult[] }>;
  explore?(
    ctx: PluginContext<S>,
    params: { categoryId: string; page: number; limit: number }
  ):
    | MusicTrackResult[]
    | { tracks: MusicTrackResult[]; categories?: PluginExploreCategory[] }
    | Promise<MusicTrackResult[] | { tracks: MusicTrackResult[]; categories?: PluginExploreCategory[] }>;
  getTrackInfo?(
    ctx: PluginContext<S>,
    params: { id: string; trackId: string; detailUrl?: string | null; track: MusicTrackInput }
  ): MusicTrackResult | Promise<MusicTrackResult>;
  getStream(
    ctx: PluginContext<S>,
    params: {
      id: string;
      trackId: string;
      urlId?: string | null;
      playUrl?: string | null;
      quality: string;
      track: MusicTrackInput;
    }
  ): MusicStreamResult | string | null | Promise<MusicStreamResult | string | null>;
  getCover?(
    ctx: PluginContext<S>,
    params: { id: string; trackId: string; coverId?: string | null; track: MusicTrackInput }
  ): string | { coverUrl: string } | null | Promise<string | { coverUrl: string } | null>;
  getLyrics?(
    ctx: PluginContext<S>,
    params: { id: string; trackId: string; lyricId?: string | null; track: MusicTrackInput }
  ): MusicLyricsResult | string | null | Promise<MusicLyricsResult | string | null>;
}

// Book Domain Contracts

export interface BookSearchItem {
  name: string;
  bookUrl: string;
  author?: string;
  coverUrl?: string;
  intro?: string;
  kind?: string;
  lastChapter?: string;
  wordCount?: string;
}

export interface BookDetailResult extends BookSearchItem {
  tocUrl?: string;
}

export interface BookChapterItem {
  title: string;
  url: string;
  updateTime?: string;
  isVolume?: boolean;
}

export interface BookPluginDefinition<S extends Record<string, any> = Record<string, any>>
  extends BasePluginManifest<S> {
  domain: "book";
  search(
    ctx: PluginContext<S>,
    params: { keyword: string; page: number }
  ): BookSearchItem[] | Promise<BookSearchItem[]>;
  getBookInfo?(
    ctx: PluginContext<S>,
    params: { bookUrl: string; book: Record<string, any> }
  ): BookDetailResult | Promise<BookDetailResult>;
  getToc(
    ctx: PluginContext<S>,
    params: { tocUrl: string; book: Record<string, any> }
  ): BookChapterItem[] | Promise<BookChapterItem[]>;
  getChapterContent(
    ctx: PluginContext<S>,
    params: {
      chapterUrl: string;
      book: Record<string, any>;
      chapter: { index: number; title: string; url: string };
    }
  ): string | { title?: string; content: string; rawHtml?: string } | Promise<string | { title?: string; content: string; rawHtml?: string }>;
}

// Comic Domain Contracts

export interface ComicSearchItem {
  title: string;
  comicUrl: string;
  author?: string;
  coverUrl?: string;
  intro?: string;
  kind?: string;
  lastChapter?: string;
  wordCount?: string;
}

export interface ComicDetailResult extends ComicSearchItem {
  tocUrl?: string;
  status?: string;
  readingDirection?: "ltr" | "rtl" | "vertical";
}

export interface ComicChapterItem {
  title: string;
  url: string;
  updateTime?: string;
  isVolume?: boolean;
}

export interface ComicPluginDefinition<S extends Record<string, any> = Record<string, any>>
  extends BasePluginManifest<S> {
  domain: "comic";
  search(
    ctx: PluginContext<S>,
    params: { keyword: string; page: number }
  ): ComicSearchItem[] | Promise<ComicSearchItem[]>;
  explore?(
    ctx: PluginContext<S>,
    params: { categoryId: string; page: number }
  ): ComicSearchItem[] | Promise<ComicSearchItem[]>;
  getDetail?(
    ctx: PluginContext<S>,
    params: { comicUrl: string; comic: Record<string, any> }
  ): ComicDetailResult | Promise<ComicDetailResult>;
  getChapters(
    ctx: PluginContext<S>,
    params: { tocUrl: string; comic: Record<string, any> }
  ): ComicChapterItem[] | Promise<ComicChapterItem[]>;
  getPages(
    ctx: PluginContext<S>,
    params: {
      chapterUrl: string;
      comic: Record<string, any>;
      chapter: { index: number; title: string; url: string };
    }
  ): Array<string | { url: string }> | Promise<Array<string | { url: string }>>;
}

// Video Domain Contracts

export interface VideoCategoryItem {
  id: string;
  name: string;
  parentId?: string;
}

export interface VideoSearchItem {
  vodId: string;
  title: string;
  detailUrl?: string;
  coverUrl?: string;
  remarks?: string;
  year?: string;
  area?: string;
  typeName?: string;
}

export interface VideoDetailResult {
  id: string;
  title: string;
  coverUrl?: string;
  intro?: string;
  year?: string;
  area?: string;
  actors?: string;
  director?: string;
  remarks?: string;
  playGroups: Array<{
    name: string;
    episodes: Array<{ name: string; playUrl: string }>;
  }>;
}

export interface VideoPlayResult {
  streamUrl: string;
  streamType?: "hls" | "mp4" | "mkv" | "webm";
  subtitles?: Array<{ label: string; lang?: string; url: string }>;
}

export interface VideoPluginDefinition<S extends Record<string, any> = Record<string, any>>
  extends BasePluginManifest<S> {
  domain: "video";
  getCategories?(ctx: PluginContext<S>): VideoCategoryItem[] | Promise<VideoCategoryItem[]>;
  search(
    ctx: PluginContext<S>,
    params: { keyword: string; page: number }
  ): VideoSearchItem[] | Promise<VideoSearchItem[]>;
  browse?(
    ctx: PluginContext<S>,
    params: { categoryId?: string | null; page: number }
  ):
    | VideoSearchItem[]
    | { items: VideoSearchItem[]; total?: number; pageCount?: number }
    | Promise<VideoSearchItem[] | { items: VideoSearchItem[]; total?: number; pageCount?: number }>;
  getDetail(
    ctx: PluginContext<S>,
    params: { vodId?: string | null; detailUrl?: string | null }
  ): VideoDetailResult | Promise<VideoDetailResult>;
  getPlayInfo?(
    ctx: PluginContext<S>,
    params: { playUrl: string; groupName?: string | null }
  ): VideoPlayResult | string | Promise<VideoPlayResult | string>;
}

export type AnyPluginDefinition<S extends Record<string, any> = Record<string, any>> =
  | MusicPluginDefinition<S>
  | BookPluginDefinition<S>
  | ComicPluginDefinition<S>
  | VideoPluginDefinition<S>;

declare global {
  function definePlugin<S extends Record<string, any> = Record<string, any>>(
    definition: AnyPluginDefinition<S>
  ): AnyPluginDefinition<S>;
}
