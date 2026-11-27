use rquickjs::{CatchResultExt, Ctx};

use crate::http::DEFAULT_WEBVIEW_UA;

use super::SharedBridgeState;
use super::convert::sloppy_eval_options;

pub(super) fn install_bootstrap_globals(
    ctx: &Ctx<'_>,
    b: &SharedBridgeState,
) -> Result<(), String> {
    let bootstrap = format!(
        r#"
        var __srcName = {src_name};
        var __srcUrl = {src_url};
        var __srcHeader = {src_header};
        var __loginUrl = {login_url};
        var __loginUiRaw = {login_ui};
        var __rawJson = {raw_json};
        var __defaultUA = {ua};

        globalThis.cookie = {{
            getCookie: function(url, key) {{
                var raw = __x_cookie_get(String(url || ''));
                if (!key) return raw;
                var parts = raw.split(';');
                for (var i = 0; i < parts.length; i++) {{
                    var p = parts[i].trim();
                    var eq = p.indexOf('=');
                    if (eq > 0 && p.slice(0, eq).trim() === String(key)) {{
                        return p.slice(eq + 1).trim();
                    }}
                }}
                return '';
            }},
            setCookie: function(url, val) {{
                __x_cookie_set(String(url || ''), String(val || ''));
            }},
            removeCookie: function(url) {{
                __x_cookie_del(String(url || ''));
            }},
            replaceCookie: function(url, val) {{
                __x_cookie_del(String(url || ''));
                __x_cookie_set(String(url || ''), String(val || ''));
            }}
        }};

        globalThis.cache = {{
            get: function(k) {{
                var v = __x_cache_get(String(k));
                return v === undefined ? null : v;
            }},
            put: function(k, v) {{
                __x_cache_put(String(k), v == null ? '' : String(v));
            }},
            delete: function(k) {{
                __x_cache_del(String(k));
            }},
            deleteMemory: function(k) {{
                __x_mem_put(String(k), '');
            }},
            getFromMemory: function(k) {{
                var v = __x_mem_get(String(k));
                return v === undefined ? null : v;
            }},
            putMemory: function(k, v) {{
                __x_mem_put(String(k), v == null ? '' : String(v));
            }}
        }};

        globalThis.source = {{
            sourceName: __srcName,
            sourceUrl: __srcUrl,
            header: __srcHeader,
            loginUrl: __loginUrl,
            rawJson: __rawJson,
            getKey: function() {{ return __srcUrl; }},
            getVariable: function() {{ return __x_src_get_var(); }},
            setVariable: function(v) {{ __x_src_set_var(v == null ? '' : String(v)); }},
            getLoginInfo: function() {{ return __x_src_get_login(); }},
            getLoginInfoMap: function() {{
                try {{ return JSON.parse(__x_src_get_login() || '{{}}'); }} catch (e) {{ return {{}}; }}
            }},
            putLoginInfo: function(v) {{
                var s = typeof v === 'string' ? v : JSON.stringify(v || {{}});
                __x_src_put_login(s);
            }},
            loginUi: function() {{ return __loginUiRaw; }}
        }};

        globalThis.java = {{
            ajax: function(url) {{
                return __x_ajax(String(url || ''));
            }},
            ajaxBase64: function(url) {{
                return __x_ajax_b64(String(url || ''));
            }},
            imageDataUrl: function(url) {{
                return __x_image_data_url(String(url || ''));
            }},
            connect: function(url) {{
                var resp = JSON.parse(__x_connect(String(url || '')));
                return {{
                    body: function() {{ return resp.body || ''; }},
                    bodyBase64: function() {{ return resp.bodyBase64 || ''; }},
                    dataUrl: function() {{ return resp.dataUrl || ''; }},
                    code: function() {{ return resp.code || 0; }},
                    url: function() {{ return resp.url || ''; }},
                    headers: function() {{ return resp.headers || {{}}; }}
                }};
            }},
            get: function(url, headers) {{
                var u = String(url || '');
                if (headers && typeof headers === 'object') {{
                    u = u + ',' + JSON.stringify({{ method: 'GET', headers: headers }});
                }}
                return this.connect(u);
            }},
            post: function(url, body, headers) {{
                var opt = {{ method: 'POST', body: body == null ? '' : (typeof body === 'string' ? body : JSON.stringify(body)) }};
                if (headers && typeof headers === 'object') opt.headers = headers;
                return this.connect(String(url || '') + ',' + JSON.stringify(opt));
            }},
            base64Encode: function(s) {{ return __x_b64_enc(s == null ? '' : String(s)); }},
            base64Decode: function(s) {{ return __x_b64_dec(s == null ? '' : String(s)); }},
            hexDecodeToString: function(s) {{ return __x_hex_dec(s == null ? '' : String(s)); }},
            hexEncodeToString: function(s) {{ return __x_hex_enc(s == null ? '' : String(s)); }},
            md5Encode: function(s) {{ return __x_md5(s == null ? '' : String(s)); }},
            md5Encode16: function(s) {{ return __x_md5(s == null ? '' : String(s)).slice(8, 24); }},
            sha1Encode: function(s) {{ return __x_crypto_hash('sha1', s == null ? '' : String(s), 'utf8', 'hex'); }},
            sha256Encode: function(s) {{ return __x_crypto_hash('sha256', s == null ? '' : String(s), 'utf8', 'hex'); }},
            hmacSha256: function(key, data, outEnc) {{
                return __x_crypto_hmac('sha256', String(key || ''), String(data || ''), JSON.stringify({{ outputEncoding: outEnc || 'hex' }}));
            }},
            aesEncrypt: function(opts) {{ return __x_crypto_aes_enc(typeof opts === 'string' ? opts : JSON.stringify(opts || {{}})); }},
            aesDecrypt: function(opts) {{ return __x_crypto_aes_dec(typeof opts === 'string' ? opts : JSON.stringify(opts || {{}})); }},
            rsaEncrypt: function(opts) {{ return __x_crypto_rsa_enc(typeof opts === 'string' ? opts : JSON.stringify(opts || {{}})); }},
            encodeURI: function(s, enc) {{ return __x_encode_uri(s == null ? '' : String(s), enc ? String(enc) : 'utf-8'); }},
            timeFormat: function(ts) {{
                var ms = ts instanceof Date ? ts.getTime() : Number(ts);
                if (!isFinite(ms)) ms = Date.now();
                return __x_time_fmt(ms);
            }},
            getCookie: function(url, key) {{ return globalThis.cookie.getCookie(url, key); }},
            getWebViewUA: function() {{ return __defaultUA; }},
            androidId: function() {{ return __x_android_id(); }},
            toast: function(msg) {{ __x_toast(msg == null ? '' : String(msg)); }},
            longToast: function(msg) {{ __x_toast(msg == null ? '' : String(msg)); }},
            log: function(msg) {{ __x_log(msg == null ? '' : (typeof msg === 'object' ? JSON.stringify(msg) : String(msg))); }},
            put: function(k, v) {{ return __x_put_temp(String(k), v == null ? '' : String(v)); }},
            get: function(k) {{ return __x_get_temp(String(k)); }},
            getString: function(rule) {{ return __x_get_string(String(rule || '')); }},
            getElements: function(rule) {{
                try {{ return JSON.parse(__x_get_elements(String(rule || ''))); }} catch (e) {{ return []; }}
            }},
            refreshExplore: function() {{}},
            startBrowser: function(url, title) {{
                __x_browser(String(url || ''), String(title || ''));
            }},
            startBrowserAwait: function(url, title) {{
                var u = String(url || '');
                if (u.indexOf('data:text/html') === 0) {{
                    throw new Error('Web 端不支持内置 HTML 弹窗设置，请使用下方备用设置按钮');
                }}
                __x_browser(u, String(title || ''));
                return {{ body: function() {{ return ''; }} }};
            }}
        }};

        globalThis.console = {{
            log: function() {{
                var parts = [];
                for (var i = 0; i < arguments.length; i++) parts.push(String(arguments[i]));
                globalThis.java.log(parts.join(' '));
            }},
            warn: function() {{ globalThis.console.log.apply(null, arguments); }},
            error: function() {{ globalThis.console.log.apply(null, arguments); }}
        }};

        globalThis.Packages = {{
            java: {{
                lang: {{
                    Thread: {{
                        sleep: function() {{}}
                    }}
                }}
            }}
        }};

        globalThis.result = '';
        globalThis.baseUrl = __srcUrl;
        globalThis.key = '';
        globalThis.page = 1;
        globalThis.__x_eval_rule = function(__x_code) {{
            return (function() {{ return eval(__x_code); }}).call(globalThis);
        }};
        "#,
        src_name = serde_json::to_string(&b.source_name).unwrap_or_else(|_| "\"\"".into()),
        src_url = serde_json::to_string(&b.source_url).unwrap_or_else(|_| "\"\"".into()),
        src_header = serde_json::to_string(&b.header_raw).unwrap_or_else(|_| "\"\"".into()),
        login_url = serde_json::to_string(&b.login_url_raw).unwrap_or_else(|_| "\"\"".into()),
        login_ui = serde_json::to_string(&b.login_ui_raw).unwrap_or_else(|_| "\"\"".into()),
        raw_json = serde_json::to_string(&b.raw_json_str).unwrap_or_else(|_| "\"\"".into()),
        ua = serde_json::to_string(DEFAULT_WEBVIEW_UA).unwrap_or_else(|_| "\"\"".into()),
    );

    ctx.eval_with_options::<(), _>(bootstrap.as_str(), sloppy_eval_options())
        .catch(ctx)
        .map_err(|e| format!("初始化 Legado JS 桥接环境失败: {e}"))?;
    Ok(())
}
