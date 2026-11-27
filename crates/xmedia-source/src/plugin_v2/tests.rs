use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use xmedia_core::domain::library::MediaKind;

use super::*;
use crate::http::HttpClientConfig;
use crate::model::SourceRuntimeState;

const SAMPLE_V2_JS: &str = r#"
export default definePlugin({
  id: "netease-mock",
  name: "网易云官方测试",
  version: "2.1.0",
  domain: "music",
  baseUrl: "https://music.163.com",
  group: "无损",
  description: "ES Module + 原生加密 + 契约化认证测试",
  checkKeyword: "晴天",
  rateLimit: "5/1000",
  headers: {
    "Referer": "https://music.163.com/"
  },
  qualities: ["standard", "exhigh", "lossless", "hires"],
  exploreCategories: [
    { title: "飙升榜", id: "19723756" },
    { title: "热歌榜", id: "3778678" }
  ],
  settings: [
    {
      key: "preferredLevel",
      label: "默认音质",
      type: "select",
      default: "lossless",
      options: [
        { label: "极高 (320k)", value: "exhigh" },
        { label: "无损 (FLAC)", value: "lossless" }
      ]
    },
    {
      key: "apiSecret",
      label: "自定义密钥",
      type: "password",
      default: ""
    }
  ],
  auth: {
    description: "支持网易云 App 扫码与短信验证码登录",
    qrCode: {
      async create(ctx) {
        const rand = ctx.crypto.randomBytes(8, "hex");
        ctx.storage.set("last_unikey", "uk_" + rand);
        return {
          key: "uk_" + rand,
          qrUrl: "https://music.163.com/login?codekey=uk_" + rand
        };
      },
      async poll(ctx, { key }) {
        if (key === "confirm_now") {
          ctx.cookies.set("https://music.163.com", "MUSIC_U=vip_token_123");
          ctx.storage.setJson("profile", { nickname: "AliceVIP", userId: "10001", vipLabel: "黑胶VIP" });
          return { status: "confirmed", message: "登录成功" };
        }
        return { status: "waiting", message: "等待扫码" };
      }
    },
    otp: {
      async send(ctx, { countryCode, phone }) {
        ctx.storage.set("otp_target", countryCode + "-" + phone);
        return { ok: true, cooldownSecs: 60, message: "验证码已发送" };
      },
      async verify(ctx, { phone, code }) {
        if (code !== "8888") {
          return { ok: false, message: "验证码错误" };
        }
        ctx.cookies.set("https://music.163.com", "MUSIC_U=otp_token_" + phone);
        ctx.storage.setJson("profile", { nickname: "PhoneUser", userId: "20002", vipLabel: "畅听VIP" });
        return { ok: true };
      }
    },
    cookie: {
      hint: "粘贴包含 MUSIC_U 的 Cookie"
    },
    async getProfile(ctx) {
      const musicU = ctx.cookies.get("https://music.163.com", "MUSIC_U");
      const saved = ctx.storage.getJson("profile");
      if (!musicU) {
        return { loggedIn: false };
      }
      return {
        loggedIn: true,
        userId: saved ? saved.userId : "999",
        nickname: saved ? saved.nickname : "CookieUser",
        vipLabel: saved ? saved.vipLabel : null
      };
    },
    async logout(ctx) {
      ctx.cookies.clear();
      ctx.storage.clear();
      return { ok: true };
    }
  },
  async search(ctx, { keyword, page, limit }) {
    const firstEnc = ctx.crypto.aesEncrypt({
      mode: "cbc",
      key: "0CoJUm6Qyw8W8jud",
      iv: "0102030405060708",
      data: JSON.stringify({ s: keyword, page, limit }),
      outputEncoding: "base64"
    });
    const roundtrip = JSON.parse(ctx.crypto.aesDecrypt({
      mode: "cbc",
      key: "0CoJUm6Qyw8W8jud",
      iv: "0102030405060708",
      data: firstEnc,
      inputEncoding: "base64"
    }));
    const encSecKey = ctx.crypto.rsaEncrypt({
      padding: "none",
      modulusHex: "00e0b509f6259df8642dbc35662901477df22677ec152b5ff68ace615bb7b725152b3ab17a876aea8a5aa76d2e417629ec4ee341f56135fccf695280104e0312ecbda92557c93870114af6c9d05c4f7f0c3685b7a46bee255932575cce10b424d813cfe4875d3e82047b97ddef52741d546b8e289dc6935b3ece0462db0a22b8e7",
      exponentHex: "010001",
      data: "0123456789abcdef",
      outputEncoding: "hex"
    });
    const sig = ctx.crypto.hmacSha256("sec", roundtrip.s, "hex");
    return [
      {
        id: "186016",
        title: roundtrip.s + "-" + ctx.settings.preferredLevel,
        artist: "周杰伦",
        album: "叶惠美",
        durationSecs: 269,
        format: encSecKey.length === 256 && sig.length === 64 ? "flac" : "mp3"
      }
    ];
  }
});
"#;

#[test]
fn inspect_manifest_and_execute_v2_crypto_async_search_and_contract_auth() {
    assert!(is_js_plugin_source(SAMPLE_V2_JS));
    assert!(!is_js_plugin_source(
        r#"{"sourceName":"Rule","sourceUrl":"https://a.test"}"#
    ));

    let manifest = inspect_js_plugin_manifest(SAMPLE_V2_JS).expect("manifest should parse");
    assert_eq!(manifest.id, "netease-mock");
    assert_eq!(manifest.name, "网易云官方测试");
    assert_eq!(manifest.version, "2.1.0");
    assert_eq!(manifest.domain, MediaKind::Music);
    assert_eq!(manifest.qualities.len(), 4);
    assert_eq!(manifest.explore_categories.len(), 2);
    assert!(manifest.has_search);
    assert!(manifest.auth.qr_code);
    assert!(manifest.auth.otp);
    assert!(manifest.auth.cookie);
    assert!(manifest.auth.has_profile);
    assert!(manifest.auth.has_logout);
    assert_eq!(manifest.settings.len(), 2);

    // 1. Execute async search exercising AES-128-CBC + unpadded NetEase weapi RSA + HMAC-SHA256 + default settings
    let search_out = PluginV2Runner::call_method_sync(
        SAMPLE_V2_JS,
        &manifest,
        SourceRuntimeState::default(),
        &HttpClientConfig::default(),
        Duration::from_secs(5),
        Duration::from_secs(5),
        Arc::new(AtomicBool::new(false)),
        &["search"],
        &[serde_json::json!({ "keyword": "晴天", "page": 1, "limit": 20 })],
    )
    .expect("v2 search should succeed");

    let arr = search_out.data.as_array().expect("array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["title"], "晴天-lossless");
    assert_eq!(arr[0]["format"], "flac");

    // 2. Execute QR create -> QR poll (confirm) -> Status -> Logout lifecycle
    let qr_created = PluginV2Runner::run_auth_action_sync(
        SAMPLE_V2_JS,
        &manifest,
        SourceRuntimeState::default(),
        PluginAuthActionRequest::QrCreate,
        &HttpClientConfig::default(),
        Duration::from_secs(5),
    )
    .expect("qr_create should succeed");
    assert!(qr_created.data.ok);
    assert!(
        qr_created
            .data
            .qr_key
            .as_deref()
            .is_some_and(|k| k.starts_with("uk_"))
    );

    let qr_confirmed = PluginV2Runner::run_auth_action_sync(
        SAMPLE_V2_JS,
        &manifest,
        qr_created.state,
        PluginAuthActionRequest::QrPoll {
            key: "confirm_now".into(),
        },
        &HttpClientConfig::default(),
        Duration::from_secs(5),
    )
    .expect("qr_poll should succeed");
    assert_eq!(qr_confirmed.data.status.as_deref(), Some("confirmed"));
    let profile = qr_confirmed.data.profile.expect("auto-fetched profile");
    assert!(profile.logged_in);
    assert_eq!(profile.nickname.as_deref(), Some("AliceVIP"));
    assert_eq!(profile.vip_label.as_deref(), Some("黑胶VIP"));

    // 3. Logout clears cookies & storage
    let logged_out = PluginV2Runner::run_auth_action_sync(
        SAMPLE_V2_JS,
        &manifest,
        qr_confirmed.state,
        PluginAuthActionRequest::Logout,
        &HttpClientConfig::default(),
        Duration::from_secs(5),
    )
    .expect("logout should succeed");
    assert!(logged_out.state.cookies.is_empty());
    assert!(logged_out.state.cache.is_empty());

    // 4. OTP send + verify
    let otp_sent = PluginV2Runner::run_auth_action_sync(
        SAMPLE_V2_JS,
        &manifest,
        logged_out.state,
        PluginAuthActionRequest::OtpSend {
            country_code: "86".into(),
            phone: "13800138000".into(),
        },
        &HttpClientConfig::default(),
        Duration::from_secs(5),
    )
    .expect("otp_send should succeed");
    assert!(otp_sent.data.ok);
    assert_eq!(otp_sent.data.cooldown_secs, Some(60));

    let otp_verified = PluginV2Runner::run_auth_action_sync(
        SAMPLE_V2_JS,
        &manifest,
        otp_sent.state,
        PluginAuthActionRequest::OtpVerify {
            country_code: "86".into(),
            phone: "13800138000".into(),
            code: "8888".into(),
        },
        &HttpClientConfig::default(),
        Duration::from_secs(5),
    )
    .expect("otp_verify should succeed");
    assert!(otp_verified.data.ok);
    assert_eq!(
        otp_verified
            .data
            .profile
            .as_ref()
            .and_then(|p| p.nickname.as_deref()),
        Some("PhoneUser")
    );
}
