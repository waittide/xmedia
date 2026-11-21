use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::routing::get;

use crate::http::{HttpClientConfig, LegadoHttpClient};
use crate::model::{ExecutionDiagnostics, SourceMetadata, SourceRuntimeState};
use crate::rule::RuleValue;

use super::{JsExecutionContext, JsSandboxConfig, ReusableSourceSession, now_mono_ms};

fn sample_metadata() -> SourceMetadata {
    SourceMetadata {
        source_name: "TestComic".into(),
        source_url: "https://comic.example.com".into(),
        header: Some(r#"{"Referer":"https://comic.example.com"}"#.into()),
        login_url: Some("function ping(){ return 'pong'; }".into()),
        login_ui: Some(r#"[{"name":"user","type":"text"}]"#.into()),
        js_lib: Some("function helperFn(x){ return 'lib:' + x; }".into()),
        enabled_cookie_jar: true,
        raw_json: Some(r#"{"name":"TestComic"}"#.into()),
    }
}

fn make_test_http(
    allow_private_ips: bool,
) -> (LegadoHttpClient, Arc<Mutex<HashMap<String, String>>>) {
    let cookies = Arc::new(Mutex::new(HashMap::new()));
    let http = LegadoHttpClient::new(
        HttpClientConfig {
            allow_private_ips,
            ..Default::default()
        },
        cookies.clone(),
        None,
        true,
    )
    .unwrap();
    (http, cookies)
}

#[test]
fn sandbox_memory_and_stack_limits() {
    let meta = sample_metadata();
    let (http, _) = make_test_http(true);
    let mut state = SourceRuntimeState::default();
    state.ensure_android_id(&meta.source_url);
    let mut ctx = JsExecutionContext::new(
        &meta,
        http,
        Arc::new(Mutex::new(state)),
        Arc::new(Mutex::new(ExecutionDiagnostics::default())),
        JsSandboxConfig {
            memory_limit_bytes: 2 * 1024 * 1024,
            max_stack_bytes: 128 * 1024,
            eval_timeout: Duration::from_secs(5),
            ..Default::default()
        },
    )
    .unwrap();

    let stack_err = ctx
        .eval_rule_js(
            "function rec(n){ return rec(n+1) + 1; } rec(0);",
            &RuleValue::Text(String::new()),
            "",
        )
        .unwrap_err();
    assert!(
        !stack_err.is_empty(),
        "deep recursion should raise catchable JS error"
    );

    let mem_err = ctx
        .eval_rule_js(
            "var arr = []; for (var i = 0; i < 100000; i++) { arr.push('x'.repeat(1024)); } arr.length;",
            &RuleValue::Text(String::new()),
            "",
        )
        .unwrap_err();
    assert!(
        !mem_err.is_empty(),
        "excessive allocation should hit memory limit cleanly"
    );
}

#[test]
fn sandbox_deadline_and_cancel_flag() {
    let meta = sample_metadata();
    let (http, _) = make_test_http(true);
    let diag = Arc::new(Mutex::new(ExecutionDiagnostics::default()));
    let mut ctx = JsExecutionContext::new(
        &meta,
        http.clone(),
        Arc::new(Mutex::new(SourceRuntimeState::default())),
        diag.clone(),
        JsSandboxConfig::with_timeout(Duration::from_millis(120)),
    )
    .unwrap();

    let err = ctx
        .eval_rule_js("while(true){}", &RuleValue::Text(String::new()), "")
        .unwrap_err();
    assert!(
        err.contains("interrupted") || err.contains("失败"),
        "expected timeout interruption, got {err}"
    );

    // External cancel flag preserves prior java.log entries
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_bg = cancel.clone();
    let op_dl = Arc::new(AtomicU64::new(now_mono_ms().saturating_add(10_000)));
    let diag2 = Arc::new(Mutex::new(ExecutionDiagnostics::default()));
    let mut ctx2 = JsExecutionContext::new_with_control(
        &meta,
        http,
        Arc::new(Mutex::new(SourceRuntimeState::default())),
        diag2.clone(),
        JsSandboxConfig::with_timeout(Duration::from_secs(10)),
        op_dl,
        cancel,
    )
    .unwrap();

    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(60));
        cancel_bg.store(true, Ordering::Relaxed);
    });
    let started = Instant::now();
    let cancel_err = ctx2
        .eval_rule_js(
            "java.log('kept-log'); while(true){}",
            &RuleValue::Text(String::new()),
            "",
        )
        .unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(!cancel_err.is_empty());
    let logs = &diag2.lock().unwrap().logs;
    assert!(logs.iter().any(|l| l == "kept-log"), "logs: {logs:?}");
}

#[tokio::test]
async fn sandbox_http_request_quota() {
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_c = hits.clone();
    let app = Router::new().route(
        "/ping",
        get(move || {
            let hits = hits_c.clone();
            async move {
                hits.fetch_add(1, Ordering::SeqCst);
                "ok"
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let meta = SourceMetadata {
        source_name: "QuotaSource".into(),
        source_url: format!("http://{addr}"),
        ..Default::default()
    };
    let diag = Arc::new(Mutex::new(ExecutionDiagnostics::default()));
    let cfg = JsSandboxConfig {
        max_http_requests: 3,
        eval_timeout: Duration::from_secs(10),
        ..Default::default()
    };

    let out = tokio::task::spawn_blocking(move || {
        let (http, _) = make_test_http(true);
        let mut ctx = JsExecutionContext::new(
            &meta,
            http,
            Arc::new(Mutex::new(SourceRuntimeState::default())),
            diag.clone(),
            cfg,
        )
        .unwrap();
        let code = format!(
            "var ok = 0; for (var i = 0; i < 6; i++) {{ if (java.ajax('http://{addr}/ping') === 'ok') ok++; }} String(ok);"
        );
        let rv = ctx
            .eval_rule_js(&code, &RuleValue::Text(String::new()), "")
            .unwrap();
        (rv.as_text(), diag.lock().unwrap().logs.clone())
    })
    .await
    .unwrap();

    assert_eq!(out.0, "3");
    assert_eq!(hits.load(Ordering::SeqCst), 3);
    assert!(out.1.iter().any(|l| l.contains("上限")));
    server.abort();
}

#[test]
fn domain_globals_injection_and_merge() {
    let meta = sample_metadata();
    let (http, cookies) = make_test_http(true);
    let state_arc = Arc::new(Mutex::new(SourceRuntimeState::default()));
    let mut session = ReusableSourceSession::new(
        meta,
        http,
        state_arc,
        cookies,
        JsSandboxConfig::with_timeout(Duration::from_secs(5)),
        Duration::from_secs(5),
        Arc::new(AtomicBool::new(false)),
    );

    // Pure non-JS rule does not initialize QuickJS lazily
    let plain = session
        .eval_string(
            &RuleValue::Text(r#"<div class="t">Hello</div>"#.into()),
            "class.t@text",
            "",
        )
        .unwrap();
    assert_eq!(plain, "Hello");
    assert!(session.js.is_none(), "VM must stay lazy for non-JS rules");

    session.add_bootstrap_snippet(
        r#"
        globalThis.book = {
            name: '',
            _vars: { custom: 'init' },
            getVariable: function(k) { return this._vars[String(k)] || ''; },
            putVariable: function(k, v) { this._vars[String(k)] = String(v); }
        };
        "#,
    );
    session.set_search_vars("tower", 2).unwrap();
    session
        .set_domain_globals([
            ("comic", serde_json::json!({"title": "Tower", "id": 95})),
            ("chapter", serde_json::json!({"ep": 1})),
        ])
        .unwrap();
    session
        .set_domain_global("book", serde_json::json!({"name": "MergedBook"}), true)
        .unwrap();

    let res = session
        .eval_string(
            &RuleValue::Text(String::new()),
            "@js: [key, page, comic.title, comic.id, chapter.ep, book.name, book.getVariable('custom'), helperFn('ok')].join('|')",
            "",
        )
        .unwrap();
    assert_eq!(res, "tower|2|Tower|95|1|MergedBook|init|lib:ok");
}

#[test]
fn bridge_java_cookie_cache_source_methods() {
    let meta = sample_metadata();
    let (http, cookies) = make_test_http(true);
    let mut initial = SourceRuntimeState::default();
    initial.ensure_android_id(&meta.source_url);
    let expected_aid = initial.android_id.clone();

    let state_arc = Arc::new(Mutex::new(initial));
    let diag_arc = Arc::new(Mutex::new(ExecutionDiagnostics::default()));
    let mut ctx = JsExecutionContext::new(
        &meta,
        http,
        state_arc.clone(),
        diag_arc.clone(),
        JsSandboxConfig::with_timeout(Duration::from_secs(5)),
    )
    .unwrap();

    let script = r#"
        var md5 = java.md5Encode('abc');
        var md5_16 = java.md5Encode16('abc');
        var b64 = java.base64Encode('hello');
        var b64_d = java.base64Decode(b64);
        var hx = java.hexEncodeToString('hi');
        var hx_d = java.hexDecodeToString(hx);
        var enc = java.encodeURI('a b', 'utf-8');
        var tf = java.timeFormat(0);
        var aid = java.androidId();

        java.put('tk', 'tv');
        var tv = java.get('tk');
        java.toast('toast-msg');
        java.log('log-msg');

        cookie.setCookie('https://example.com', 'a=1; b=2');
        var subCk = cookie.getCookie('https://sub.example.com', 'b');
        cookie.replaceCookie('https://example.com', 'a=9');
        var repCk = cookie.getCookie('https://example.com');
        cookie.removeCookie('https://example.com');

        cache.put('pk', 'pv');
        var pv = cache.get('pk');
        cache.putMemory('mk', 'mv');
        var mv = cache.getFromMemory('mk');
        cache.deleteMemory('mk');
        var mvAfter = cache.getFromMemory('mk');

        source.setVariable('v123');
        source.putLoginInfo({ user: 'alice' });
        var srcSummary = [
            source.sourceName,
            source.bookSourceName,
            source.sourceUrl,
            source.bookSourceUrl,
            source.getKey(),
            source.getVariable(),
            source.getLoginInfoMap().user,
            JSON.parse(source.loginUi())[0].name
        ].join(',');

        var gStr = java.getString('$.item');
        var gEls = java.getElements('$.list');

        JSON.stringify({
            md5: md5,
            md5_16: md5_16,
            b64_d: b64_d,
            hx_d: hx_d,
            enc: enc,
            tf_nonempty: tf.length > 10,
            aid: aid,
            tv: tv,
            subCk: subCk,
            repCk: repCk,
            pv: pv,
            mv: mv,
            mvAfter: mvAfter,
            srcSummary: srcSummary,
            gStr: gStr,
            gElsLen: gEls.length
        });
    "#;

    let input = RuleValue::Json(serde_json::json!({"item": "val1", "list": [1, 2, 3]}));
    let out = ctx.eval_rule_js(script, &input, "").unwrap().as_text();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();

    assert_eq!(v["md5"].as_str(), Some("900150983cd24fb0d6963f7d28e17f72"));
    assert_eq!(v["md5_16"].as_str(), Some("3cd24fb0d6963f7d"));
    assert_eq!(v["b64_d"].as_str(), Some("hello"));
    assert_eq!(v["hx_d"].as_str(), Some("hi"));
    assert_eq!(v["enc"].as_str(), Some("a%20b"));
    assert_eq!(v["tf_nonempty"].as_bool(), Some(true));
    assert_eq!(v["aid"].as_str(), Some(expected_aid.as_str()));
    assert_eq!(v["tv"].as_str(), Some("tv"));
    assert_eq!(v["subCk"].as_str(), Some("2"));
    assert_eq!(v["repCk"].as_str(), Some("a=9"));
    assert_eq!(v["pv"].as_str(), Some("pv"));
    assert_eq!(v["mv"].as_str(), Some("mv"));
    assert!(v["mvAfter"].is_null());
    assert_eq!(
        v["srcSummary"].as_str(),
        Some(
            "TestComic,TestComic,https://comic.example.com,https://comic.example.com,https://comic.example.com,v123,alice,user"
        )
    );
    assert_eq!(v["gStr"].as_str(), Some("val1"));
    assert_eq!(v["gElsLen"].as_u64(), Some(3));

    assert!(cookies.lock().unwrap().is_empty());
    let d = diag_arc.lock().unwrap();
    assert!(d.toasts.contains(&"toast-msg".to_string()));
    assert!(d.logs.contains(&"log-msg".to_string()));
}

#[test]
fn mustache_template_propagates_sandbox_errors_and_allows_css_fallback() {
    let meta = sample_metadata();
    let (http, cookies) = make_test_http(true);

    // (a) Infinite loop inside {{ ... }} fails with timeout error (never swallowed as Ok)
    let mut timeout_session = ReusableSourceSession::new(
        meta.clone(),
        http.clone(),
        Arc::new(Mutex::new(SourceRuntimeState::default())),
        cookies.clone(),
        JsSandboxConfig::with_timeout(Duration::from_millis(80)),
        Duration::from_secs(5),
        Arc::new(AtomicBool::new(false)),
    );
    let timeout_err = timeout_session
        .eval_string(
            &RuleValue::Text(String::new()),
            "https://example.com/{{ while(true){} }}/path",
            "",
        )
        .unwrap_err();
    assert!(
        timeout_err.contains("interrupted") || timeout_err.contains("超时"),
        "expected timeout error, got: {timeout_err}"
    );

    // (b) cancel_flag inside {{ ... }} fails with cancel error and preserves prior java.log(...)
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_bg = cancel.clone();
    let mut cancel_session = ReusableSourceSession::new(
        meta.clone(),
        http.clone(),
        Arc::new(Mutex::new(SourceRuntimeState::default())),
        cookies.clone(),
        JsSandboxConfig::with_timeout(Duration::from_secs(10)),
        Duration::from_secs(10),
        cancel.clone(),
    );
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(60));
        cancel_bg.store(true, Ordering::Relaxed);
    });
    let cancel_err = cancel_session
        .eval_string(
            &RuleValue::Text(String::new()),
            "prefix/{{ java.log('kept-mustache'); while(true){} }}/suffix",
            "",
        )
        .unwrap_err();
    assert!(
        cancel_err.contains("取消") || cancel_err.contains("interrupted"),
        "expected cancel error, got: {cancel_err}"
    );
    let logs = cancel_session.snapshot_diagnostics().logs;
    assert!(
        logs.iter().any(|l| l == "kept-mustache"),
        "expected java.log preserved across cancel, got: {logs:?}"
    );

    // Multiple {{ ... }} blocks fail fast once cancel_flag is set
    let multi_err = cancel_session
        .eval_string(
            &RuleValue::Json(serde_json::json!({"a": "1", "b": "2"})),
            "{{$.a}}-{{$.b}}",
            "",
        )
        .unwrap_err();
    assert!(
        multi_err.contains("取消") || multi_err.contains("interrupted"),
        "expected pre-cancelled error on multi-block template, got: {multi_err}"
    );

    // (c) Stack overflow & OOM inside {{ ... }} return Err(...)
    let mut limit_session = ReusableSourceSession::new(
        meta.clone(),
        http.clone(),
        Arc::new(Mutex::new(SourceRuntimeState::default())),
        cookies.clone(),
        JsSandboxConfig {
            memory_limit_bytes: 2 * 1024 * 1024,
            max_stack_bytes: 128 * 1024,
            eval_timeout: Duration::from_secs(5),
            ..Default::default()
        },
        Duration::from_secs(5),
        Arc::new(AtomicBool::new(false)),
    );
    let stack_err = limit_session
        .eval_string(
            &RuleValue::Text(String::new()),
            "v={{ (function rec(n){ return rec(n+1)+1; })(0) }}",
            "",
        )
        .unwrap_err();
    assert!(
        !stack_err.is_empty(),
        "expected stack overflow error, got Ok"
    );

    let oom_err = limit_session
        .eval_string(
            &RuleValue::Text(String::new()),
            "v={{ (function(){ var a=[]; for(var i=0;i<100000;i++) a.push('x'.repeat(1024)); return a.length; })() }}",
            "",
        )
        .unwrap_err();
    assert!(!oom_err.is_empty(), "expected OOM error, got Ok");

    // (d) Legitimate CSS shorthand inside {{class.title@text}} when js is Some(...) still resolves cleanly
    let mut ok_session = ReusableSourceSession::new(
        meta,
        http,
        Arc::new(Mutex::new(SourceRuntimeState::default())),
        cookies,
        JsSandboxConfig::with_timeout(Duration::from_secs(5)),
        Duration::from_secs(5),
        Arc::new(AtomicBool::new(false)),
    );
    let html = RuleValue::Text(
        r#"<div><span class="title">Rust 书源</span><div class="name">作者<span>忽略</span></div><img data-url="https://cdn.example.com/cover.jpg"/></div>"#
            .into(),
    );
    let css_out = ok_session
        .eval_string(
            &html,
            "{{class.title@text}} / {{div.name@ownText}} / {{span.title}} / {{img[data-url^=\"https://\"]@data-url}} / {{helperFn('ok')}}",
            "",
        )
        .unwrap();
    assert_eq!(
        css_out,
        "Rust 书源 / 作者 / Rust 书源 / https://cdn.example.com/cover.jpg / lib:ok"
    );
}
