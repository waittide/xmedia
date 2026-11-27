# XMedia Plugin v2 开发指南

本文档面向插件作者与自动化开发 Agent，说明 XMedia 双轨插件体系中的 **Plugin v2 JavaScript 模块** 规范、内置运行时能力、契约化认证协议以及本地 CLI 调试流程。完整类型声明请参考 `docs/plugin-sdk.d.ts`。

## 1. 架构与隔离模型

XMedia 同时兼容两类插件格式：

1. **Plugin v2 (`.js` / `.mjs`)**：标准 ES2023 模块（`export default definePlugin({ ... })`），运行于受限的 QuickJS 沙箱，适用于官方接口签名、加密请求、扫码/短信验证码登录与多档音质或画质协商。
2. **传统 JSON 规则 (`.json`)**：声明式 CSS / XPath / JSONPath 规则对象或数组，适合静态站点抓取。

所有插件及其运行时状态（Cookie、键值存储、配置项、登录态）在服务端严格按用户隔离存储于 `user_plugins` 表，不同用户之间不共享任何插件定义或账号凭证。

## 2. 最小插件结构

```js
/// <reference path="./plugin-sdk.d.ts" />

export default definePlugin({
  id: "netease-official",
  name: "网易云音乐",
  version: "1.0.0",
  domain: "music",
  baseUrl: "https://music.163.com",
  group: "官方音源",
  checkKeyword: "晴天",
  rateLimit: "5/1000",
  headers: {
    "Referer": "https://music.163.com/",
    "User-Agent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36"
  },
  qualities: ["standard", "exhigh", "lossless", "hires"],
  exploreCategories: [
    { title: "飙升榜", id: "19723756" },
    { title: "新歌榜", id: "3779629" },
    { title: "原创榜", id: "2884035" },
    { title: "热歌榜", id: "3778678" }
  ],
  settings: [
    {
      key: "preferredLevel",
      label: "默认音质",
      type: "select",
      default: "exhigh",
      options: [
        { label: "标准 (128k)", value: "standard" },
        { label: "极高 (320k)", value: "exhigh" },
        { label: "无损 (FLAC)", value: "lossless" },
        { label: "Hi-Res", value: "hires" }
      ]
    }
  ],
  async search(ctx, { keyword, page, limit }) {
    const offset = (page - 1) * limit;
    const resp = ctx.http.get("https://music.163.com/api/search/get/web", {
      query: { csrf_token: "", s: keyword, type: 1, offset, limit }
    }).json();
    const songs = (resp.result && resp.result.songs) || [];
    return songs.map((s) => ({
      id: String(s.id),
      title: s.name,
      artists: s.artists,
      album: s.album,
      durationMs: s.duration,
      coverId: String(s.id)
    }));
  },
  async getStream(ctx, { id, quality }) {
    const level = quality || ctx.settings.preferredLevel || "exhigh";
    const resp = ctx.http.get("https://music.163.com/api/song/enhance/player/url/v1", {
      query: { ids: JSON.stringify([Number(id) || id]), level, encodeType: "flac" }
    }).json();
    const item = resp.data && resp.data[0];
    if (!item || !item.url) return null;
    return {
      url: item.url,
      format: item.type,
      bitrate: item.br,
      expiresInSecs: item.expi
    };
  }
});
```

导入插件时，服务端在零网络权限沙箱中静态读取元数据并自动识别 `domain`（`book` / `comic` / `music` / `video`），严禁在模块顶层发起网络请求。

## 3. 运行时上下文 `ctx`

每个业务与认证方法的第一参数均为只读 `ctx` 对象：

### 3.1 `ctx.http`

内置 SSRF 防护、超时控制、自动响应解码与当前用户的独立 Cookie Jar（上游返回 `Set-Cookie` 时自动按域名持久化）。

- `ctx.http.get(url, { query, headers })`
- `ctx.http.post(url, { form, json, body, query, headers })`
- `ctx.http.request({ url, method, query, headers, form, json, body })`

返回值包含 `status`、`ok`、`url`、`headers`、`text`、`bodyBase64`、`dataUrl` 与 `.json()` 方法。当传入 `form` 对象时自动按 `application/x-www-form-urlencoded; charset=UTF-8` 编码；传入 `json` 时自动序列化并附带 `application/json; charset=UTF-8`。

### 3.2 `ctx.crypto`

由 Rust 原生实现，无需在插件内打包第三方加密库：

- 摘要与消息认证码：`md5`、`sha1`、`sha256`、`hmacSha1`、`hmacSha256`
- 对称加解密：`aesEncrypt` / `aesDecrypt`（支持 AES-128/192/256，`mode: "cbc" | "ecb" | "gcm"`，`padding: "pkcs7" | "zero" | "none"`）
- 非对称加密：`rsaEncrypt`（支持 `padding: "pkcs1v15" | "oaep-sha256" | "none"`，可传 `publicKeyPem` 或十六进制 `modulusHex` + `exponentHex`）
- 随机数与编码：`randomBytes(len, "hex" | "base64")`、`base64Encode`、`base64Decode`、`hexEncode`、`hexDecode`

例如网易云 `weapi` 所需的双层 AES-128-CBC 加无填充原始模幂 RSA：

```js
function weapi(ctx, payload) {
  const presetKey = "0CoJUm6Qyw8W8jud";
  const iv = "0102030405060708";
  const modulusHex =
    "00e0b509f6259df8642dbc35662901477df22677ec152b5ff68ace615bb7b725" +
    "152b3ab17a876aea8a5aa76d2e417629ec4ee341f56135fccf695280104e0312" +
    "ecbda92557c93870114af6c9d05c4f7f0c3685b7a46bee255932575cce10b424" +
    "d813cfe4875d3e82047b97ddef52741d546b8e289dc6935b3ece0462db0a22b8e7";
  const secretKey = ctx.crypto.randomBytes(8, "hex"); // 16 ASCII hex chars
  const step1 = ctx.crypto.aesEncrypt({
    mode: "cbc",
    key: presetKey,
    iv,
    data: JSON.stringify(payload),
    outputEncoding: "base64"
  });
  const params = ctx.crypto.aesEncrypt({
    mode: "cbc",
    key: secretKey,
    iv,
    data: step1,
    outputEncoding: "base64"
  });
  const reversedKey = secretKey.split("").reverse().join("");
  const encSecKey = ctx.crypto.rsaEncrypt({
    padding: "none",
    modulusHex,
    exponentHex: "010001",
    data: reversedKey,
    outputEncoding: "hex"
  });
  return { params, encSecKey };
}
```

### 3.3 `ctx.cookies`、`ctx.storage`、`ctx.settings` 与 `ctx.html`

- `ctx.cookies`：`get(url, name)`、`getAll(url)`、`set(url, "K=V")`、`remove(url)`、`clear()`
- `ctx.storage`：当前用户的插件私有键值存储（`get`、`set`、`remove`、`getJson`、`setJson`、`clear`）
- `ctx.settings`：合并默认值与用户自定义值后的只读配置对象（`type: "password"` 字段在前端 API 返回时自动脱敏）
- `ctx.html`：`selectText(html, rule, baseUrl)` 与 `selectList(html, rule)`，复用 Rust HTML/CSS 选择器引擎

## 4. 契约化认证协议 (`plugin.auth`)

插件只需声明支持的认证分支，XMedia 前端会自动生成统一的多标签认证弹窗（含本地 SVG 二维码渲染、2 秒自动轮询、短信 60 秒倒计时、登录态资料展示与退出登录）：

| 分支 | 方法签名 | 返回格式 |
| --- | --- | --- |
| 扫码登录 | `auth.qrCode.create(ctx)` | `{ key, qrUrl?, qrImage?, message? }` |
| 扫码轮询 | `auth.qrCode.poll(ctx, { key })` | `{ status: "waiting" \| "scanned" \| "confirmed" \| "expired" \| "failed", message?, profile? }` |
| 发送验证码 | `auth.otp.send(ctx, { countryCode, phone })` | `{ ok, cooldownSecs?, message? }` |
| 校验验证码 | `auth.otp.verify(ctx, { countryCode, phone, code })` | `{ ok, message?, profile? }` |
| 密码登录 | `auth.password.login(ctx, { username, password })` | `{ ok, message?, profile? }` |
| Cookie 登录 | `auth.cookie: true` 或 `{ hint?, login?(ctx, { cookie }) }` | 自动写入 Cookie 并调用 `getProfile` 校验 |
| 登录态查询 | `auth.getProfile(ctx)` | `{ loggedIn, userId?, nickname?, avatarUrl?, vipLabel?, message? }` |
| 退出登录 | `auth.logout(ctx)` | 清理上游会话与本地 Cookie |

当 `poll`、`otp.verify`、`password` 或 `cookie` 返回成功且未内联携带 `profile` 时，宿主会自动在同一会话内调用 `auth.getProfile(ctx)` 并返回最新资料。

## 5. 各领域方法契约速览

- **音乐 (`domain: "music"`)**：`search`、`explore`（可选）、`getTrackInfo`（可选）、`getStream`、`getCover`（可选，无列表封面时按需触发）、`getLyrics`（可选）。
- **书籍 (`domain: "book"`)**：`search`、`getBookInfo`（可选）、`getToc`、`getChapterContent`。
- **漫画 (`domain: "comic"`)**：`search`、`explore`（可选）、`getDetail`（可选）、`getChapters`、`getPages`。
- **影视 (`domain: "video"`)**：`getCategories`（可选，未定义时回退至 `exploreCategories`）、`search`、`browse`（可选）、`getDetail`、`getPlayInfo`（可选）。

## 6. 使用 `xmedia-tool plugin` 本地调试

无需启动数据库或 Web 服务，即可直接通过 `xmedia-tool` 验证与调试外部插件仓库（例如 `xmedia-sources`）中的插件：

```bash
# 1. 零网络校验插件元数据与声明结构
cargo run -p xmedia-tool -- plugin check /path/to/netease.js

# 2. 携带持久化状态文件测试搜索、榜单、歌词与取流
cargo run -p xmedia-tool -- plugin search /path/to/netease.js "晴天" --state /tmp/ncm-state.json
cargo run -p xmedia-tool -- plugin explore /path/to/netease.js --category 19723756 --state /tmp/ncm-state.json
cargo run -p xmedia-tool -- plugin lyrics /path/to/netease.js --id 186016 --state /tmp/ncm-state.json
cargo run -p xmedia-tool -- plugin stream /path/to/netease.js --id 186016 --quality exhigh --state /tmp/ncm-state.json

# 3. 调试契约化登录（扫码 / 短信验证码 / Cookie）并保存登录态至 state.json
cargo run -p xmedia-tool -- plugin auth /path/to/netease.js qr-create --state /tmp/ncm-state.json
cargo run -p xmedia-tool -- plugin auth /path/to/netease.js qr-poll --key "<unikey>" --state /tmp/ncm-state.json
cargo run -p xmedia-tool -- plugin auth /path/to/netease.js status --state /tmp/ncm-state.json

# 4. 修改配置项并验证登录后的无损取流
cargo run -p xmedia-tool -- plugin set /path/to/netease.js preferredLevel=lossless --state /tmp/ncm-state.json
cargo run -p xmedia-tool -- plugin stream /path/to/netease.js --id 186016 --quality lossless --state /tmp/ncm-state.json
```
