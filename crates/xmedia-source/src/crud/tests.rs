use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use axum::Router;
use axum::extract::{FromRef, Path};
use axum::routing::post;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::application::auth::AuthService;
use xmedia_core::application::online_items::OnlineSourceDirectory;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::user::{User, UserRole};

use super::*;
use crate::model::{ExecutionDiagnostics, LoginUiItem, SourceCallOutput, default_true};

#[derive(Debug, Clone, Deserialize)]
struct SampleSpec {
    #[serde(default)]
    name: String,
    #[serde(default)]
    url: String,
    #[serde(default, rename = "sourceGroup")]
    source_group: Option<String>,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    weight: i64,
    #[serde(default)]
    comment: Option<String>,
}

impl SourceSpecAdapter for SampleSpec {
    const ENTITY_LABEL: &'static str = "测试源";
    const JSON_GROUP_KEY: &'static str = "sourceGroup";

    fn source_name(&self) -> &str {
        &self.name
    }

    fn effective_source_url(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.url)
    }

    fn group_name(&self) -> Option<&str> {
        self.source_group.as_deref()
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn weight(&self) -> i64 {
        self.weight
    }

    fn source_type_code(&self) -> i64 {
        0
    }

    fn summary_meta(&self) -> ParsedSourceSummaryMeta {
        ParsedSourceSummaryMeta {
            comment: self.comment.clone(),
            check_keyword: None,
            has_login_url: true,
            login_ui: vec![LoginUiItem {
                name: "pinCode".into(),
                item_type: "password".into(),
                action: None,
                style: None,
            }],
        }
    }
}

#[derive(Default)]
struct MemSourceRepo {
    rows: tokio::sync::Mutex<Vec<SourceRecord>>,
}

#[async_trait]
impl SourceRepository for MemSourceRepo {
    async fn list_all(&self) -> DomainResult<Vec<SourceRecord>> {
        Ok(self.rows.lock().await.clone())
    }

    async fn find_by_id(&self, id: Uuid) -> DomainResult<Option<SourceRecord>> {
        Ok(self.rows.lock().await.iter().find(|r| r.id == id).cloned())
    }

    async fn save(&self, record: &SourceRecord) -> DomainResult<()> {
        let mut g = self.rows.lock().await;
        if let Some(slot) = g.iter_mut().find(|r| r.id == record.id) {
            *slot = record.clone();
        } else {
            g.push(record.clone());
        }
        Ok(())
    }

    async fn update_metadata_if_exists(
        &self,
        id: Uuid,
        patch: SourceMetadataUpdate,
    ) -> DomainResult<Option<SourceRecord>> {
        let mut g = self.rows.lock().await;
        let Some(rec) = g.iter_mut().find(|r| r.id == id) else {
            return Ok(None);
        };
        rec.name = patch.name;
        rec.source_url = patch.source_url;
        rec.group_name = patch.group_name;
        rec.enabled = patch.enabled;
        rec.weight = patch.weight;
        rec.source_type = patch.source_type;
        rec.raw_json = patch.raw_json;
        if let Some(v) = patch.variable {
            rec.runtime_state.variable = v;
        }
        rec.updated_at = patch.updated_at;
        Ok(Some(rec.clone()))
    }

    async fn update_runtime_state(
        &self,
        id: Uuid,
        runtime_state: SourceRuntimeState,
        updated_at: Timestamp,
    ) -> DomainResult<()> {
        let mut g = self.rows.lock().await;
        if let Some(rec) = g.iter_mut().find(|r| r.id == id) {
            rec.runtime_state = runtime_state;
            rec.updated_at = updated_at;
        }
        Ok(())
    }

    async fn update_test_result(
        &self,
        id: Uuid,
        ok: bool,
        message: String,
        updated_at: Timestamp,
    ) -> DomainResult<()> {
        let mut g = self.rows.lock().await;
        if let Some(rec) = g.iter_mut().find(|r| r.id == id) {
            rec.last_test_ok = Some(ok);
            rec.last_test_msg = Some(message);
            rec.updated_at = updated_at;
        }
        Ok(())
    }

    async fn update_weight_if_exists(
        &self,
        id: Uuid,
        weight: i64,
        updated_at: Timestamp,
    ) -> DomainResult<Option<SourceRecord>> {
        let mut g = self.rows.lock().await;
        let Some(rec) = g.iter_mut().find(|r| r.id == id) else {
            return Ok(None);
        };
        rec.weight = weight;
        rec.raw_json = SourceRecord::sync_raw_json_fields(
            &rec.raw_json,
            None,
            Some(weight),
            None,
            "sourceGroup",
        );
        rec.updated_at = updated_at;
        Ok(Some(rec.clone()))
    }

    async fn update_batch_fields_if_exists(
        &self,
        id: Uuid,
        enabled: Option<bool>,
        group_name: Option<String>,
        updated_at: Timestamp,
    ) -> DomainResult<bool> {
        let mut g = self.rows.lock().await;
        let Some(rec) = g.iter_mut().find(|r| r.id == id) else {
            return Ok(false);
        };
        if let Some(en) = enabled {
            rec.enabled = en;
        }
        if let Some(ref grp) = group_name {
            rec.group_name = grp.trim().to_string();
        }
        rec.raw_json = SourceRecord::sync_raw_json_fields(
            &rec.raw_json,
            enabled,
            None,
            group_name.as_deref(),
            "sourceGroup",
        );
        rec.updated_at = updated_at;
        Ok(true)
    }

    async fn delete(&self, id: Uuid) -> DomainResult<()> {
        self.rows.lock().await.retain(|r| r.id != id);
        Ok(())
    }
}

#[test]
fn parse_json_batch_handles_bom_single_array_and_errors() {
    let bom_single = "\u{feff}  {\"name\":\"A\",\"url\":\"https://a.test\"}  ";
    let res = parse_json_batch::<SampleSpec, _>(bom_single, "数据源", "为空", |s| {
        s.name.trim().is_empty() && s.url.trim().is_empty()
    })
    .unwrap();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].0.name, "A");

    let array_with_blank = r#"[
        {"name":"","url":""},
        {"name":"B","url":"https://b.test"}
    ]"#;
    let res_arr = parse_json_batch::<SampleSpec, _>(array_with_blank, "数据源", "为空", |s| {
        s.name.trim().is_empty() && s.url.trim().is_empty()
    })
    .unwrap();
    assert_eq!(res_arr.len(), 1);
    assert_eq!(res_arr[0].0.name, "B");

    assert!(parse_json_batch::<SampleSpec, _>("  ", "数据源", "为空", |_| false).is_err());
    let bom_ws_err =
        parse_json_batch::<SampleSpec, _>("\u{feff}   ", "数据源", "为空", |_| false).unwrap_err();
    assert_eq!(bom_ws_err, "数据源 JSON 内容为空");
    assert!(parse_json_batch::<SampleSpec, _>("123", "数据源", "为空", |_| false).is_err());
    assert!(parse_json_batch::<SampleSpec, _>("{bad", "数据源", "为空", |_| false).is_err());
}

#[test]
fn sync_raw_json_fields_updates_and_clears_group_key() {
    let raw = r#"{"name":"X","enabled":true,"weight":1,"extra":42}"#;
    let updated = sync_raw_json_fields(raw, Some(false), Some(99), Some("  漫画 "), "sourceGroup");
    let val: serde_json::Value = serde_json::from_str(&updated).unwrap();
    assert_eq!(val["enabled"], false);
    assert_eq!(val["weight"], 99);
    assert_eq!(val["sourceGroup"], "漫画");
    assert_eq!(val["extra"], 42);

    let cleared = sync_raw_json_fields(&updated, None, None, Some("   "), "sourceGroup");
    let val2: serde_json::Value = serde_json::from_str(&cleared).unwrap();
    assert!(val2.get("sourceGroup").is_none());
    assert_eq!(val2["weight"], 99);
}

#[test]
fn masking_and_build_source_summary_for_admin_and_member() {
    let login_ui = vec![
        LoginUiItem {
            name: "用户名".into(),
            item_type: "text".into(),
            action: None,
            style: None,
        },
        LoginUiItem {
            name: "pinCode".into(),
            item_type: "PASSWORD".into(),
            action: None,
            style: None,
        },
    ];

    assert!(is_sensitive_login_field("pinCode", &login_ui));
    assert!(is_sensitive_login_field("apiToken", &login_ui));
    assert!(is_sensitive_login_field("登录密码", &login_ui));
    assert!(!is_sensitive_login_field("用户名", &login_ui));

    let mut rt = SourceRuntimeState {
        variable: "你".repeat(260),
        ..Default::default()
    };
    rt.login_info.insert("用户名".into(), "alice".into());
    rt.login_info.insert("pinCode".into(), "1234".into());
    rt.cookies.insert("b.example.com".into(), "x=1".into());
    rt.cookies.insert("a.example.com".into(), "y=2".into());

    let snap = SourceRecordSnapshot {
        id: Uuid::nil(),
        name: "Test",
        source_url: "https://example.com",
        group_name: "G1",
        enabled: true,
        weight: 10,
        source_type: 2,
        runtime_state: &rt,
        last_test_ok: Some(true),
        last_test_msg: Some("ok"),
        updated_at: Timestamp::UNIX_EPOCH,
    };
    let meta = ParsedSourceSummaryMeta {
        comment: Some("c".into()),
        check_keyword: Some("kw".into()),
        has_login_url: true,
        login_ui: login_ui.clone(),
    };

    let admin_dto = build_source_summary(
        SourceRecordSnapshot {
            id: snap.id,
            name: snap.name,
            source_url: snap.source_url,
            group_name: snap.group_name,
            enabled: snap.enabled,
            weight: snap.weight,
            source_type: snap.source_type,
            runtime_state: snap.runtime_state,
            last_test_ok: snap.last_test_ok,
            last_test_msg: snap.last_test_msg,
            updated_at: snap.updated_at,
        },
        meta.clone(),
        true,
    );
    assert_eq!(
        admin_dto.login_info.get("pinCode").map(String::as_str),
        Some(MASKED_SECRET_PLACEHOLDER)
    );
    assert_eq!(admin_dto.variable_preview.chars().count(), 240);
    assert_eq!(
        admin_dto.cookie_hosts,
        vec!["a.example.com", "b.example.com"]
    );

    let member_dto = build_source_summary(snap, meta, false);
    assert!(member_dto.login_ui.is_empty());
    assert!(member_dto.login_info.is_empty());
    assert!(member_dto.variable_preview.is_empty());
    assert!(member_dto.cookie_hosts.is_empty());
}

#[test]
fn test_report_diagnostics_reorder_dedup_and_runtime_merge() {
    let mut acc = ExecutionDiagnostics::default();
    let d1 = ExecutionDiagnostics {
        toasts: vec!["t1".into()],
        logs: vec!["l1".into()],
        browser_urls: vec![],
    };
    merge_diagnostics(&mut acc, &d1);
    assert_eq!(acc.toasts, vec!["t1"]);

    #[derive(Serialize)]
    #[serde(rename_all = "snake_case")]
    enum MockMode {
        Full,
    }
    #[derive(Serialize)]
    struct MockPayload {
        extra_count: usize,
    }
    let report = SourceTestReportDto {
        ok: true,
        mode: MockMode::Full,
        keyword: "kw".into(),
        total_elapsed_ms: 12,
        result_count: 1,
        sample_titles: vec!["T".into()],
        error: None,
        diagnostics: ExecutionDiagnostics::default(),
        steps: vec![],
        payload: MockPayload { extra_count: 7 },
    };
    assert_eq!(serde_json::to_value(&report).unwrap()["extra_count"], 7);

    let base = SourceRuntimeState {
        variable: "v1".into(),
        login_info: HashMap::from([("u".into(), "a".into())]),
        cookies: HashMap::from([("h.test".into(), "a=1; b=2".into())]),
        cache: HashMap::from([("k1".into(), "val1".into())]),
        android_id: "aid".into(),
    };
    let mut updated_clear = base.clone();
    updated_clear.variable.clear();
    updated_clear.cache.remove("k1");
    updated_clear.cookies.remove("h.test");
    let cleared = merge_runtime_state_with_base(&base, Some(base.clone()), updated_clear, true);
    assert!(cleared.variable.is_empty());
    assert!(!cleared.cache.contains_key("k1"));
    assert!(!cleared.cookies.contains_key("h.test"));
}

#[tokio::test]
async fn generic_source_crud_service_and_prefixed_router_work_end_to_end() {
    let repo = Arc::new(MemSourceRepo::default());
    let svc = Arc::new(SourceCrudService::<SampleSpec>::new(repo));

    let admin = User::new(
        "admin",
        "Admin",
        "hash",
        "enc",
        "xmk_admin",
        UserRole::Admin,
    )
    .unwrap();
    let member = User::new(
        "member",
        "Member",
        "hash",
        "enc",
        "xmk_member",
        UserRole::Member,
    )
    .unwrap();

    // 1. Import JSON array + idempotent upsert
    let raw = r#"[
        {"name":"Alpha","url":"https://alpha.example","sourceGroup":"G1","enabled":true,"weight":10},
        {"name":"Beta","url":"https://beta.example","enabled":false,"weight":5}
    ]"#;
    let report = svc.import_sources(&admin, raw).await.unwrap();
    assert_eq!(report.imported_count, 2);
    assert_eq!(report.updated_count, 0);

    let repeat = svc
        .import_sources(
            &admin,
            r#"{"name":"Alpha","url":"https://alpha.example","sourceGroup":"G2","enabled":true,"weight":20}"#,
        )
        .await
        .unwrap();
    assert_eq!(repeat.imported_count, 0);
    assert_eq!(repeat.updated_count, 1);

    // 2. Member sees only enabled sources; OnlineSourceDirectory lists both with enabled flag
    assert_eq!(svc.list_sources(&admin).await.unwrap().len(), 2);
    assert_eq!(svc.list_sources(&member).await.unwrap().len(), 1);
    let dir_map = OnlineSourceDirectory::list_sources(svc.as_ref())
        .await
        .unwrap();
    assert_eq!(dir_map.len(), 2);

    let beta_id = report.sources[1].id;
    let alpha_id = report.sources[0].id;

    // 3. Reorder, toggle enabled, update variable, login action preserving masked secret, export & delete
    let reordered = svc
        .reorder_sources(&admin, vec![beta_id, alpha_id])
        .await
        .unwrap();
    assert_eq!(reordered[0].id, beta_id);
    assert_eq!(reordered[0].weight, 20);

    let toggled = svc.set_source_enabled(&admin, beta_id, true).await.unwrap();
    assert!(toggled.enabled);

    let updated = svc
        .update_source(
            &admin,
            alpha_id,
            UpdateSourceInput {
                variable: Some("var-xyz".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(updated.has_variable);
    assert_eq!(
        svc.get_source_variable(&admin, alpha_id).await.unwrap(),
        "var-xyz"
    );

    let login_res = svc
        .run_login_action_with(
            &admin,
            alpha_id,
            HashMap::from([("pinCode".into(), "9988".into())]),
            Some("login()".into()),
            |_spec, mut state, submitted, _act| {
                state.login_info = submitted;
                Ok(SourceCallOutput {
                    data: "logged-in".into(),
                    state,
                    diagnostics: ExecutionDiagnostics::default(),
                })
            },
        )
        .await
        .unwrap();
    assert_eq!(login_res.result, "logged-in");
    assert_eq!(
        login_res
            .source
            .login_info
            .get("pinCode")
            .map(String::as_str),
        Some(MASKED_SECRET_PLACEHOLDER)
    );

    let batch_json = svc
        .export_sources_batch_json(&admin, vec![beta_id, alpha_id])
        .await
        .unwrap();
    assert!(batch_json.contains("Alpha"));
    assert!(batch_json.contains("Beta"));

    let del_report = svc
        .batch_sources(
            &admin,
            BatchSourcesInput {
                ids: vec![beta_id],
                action: BatchSourceAction::Delete,
            },
        )
        .await
        .unwrap();
    assert_eq!(del_report.affected_count, 1);
    assert_eq!(svc.list_sources(&admin).await.unwrap().len(), 1);

    // 4. Verify `sources_prefixed_crud_router` merges cleanly with `/sources/{id}/test` in Axum 0.8
    #[derive(Clone)]
    struct TestHttpState {
        auth: Arc<AuthService>,
        crud: Arc<SourceCrudService<SampleSpec>>,
    }
    impl FromRef<TestHttpState> for Arc<AuthService> {
        fn from_ref(s: &TestHttpState) -> Self {
            s.auth.clone()
        }
    }
    impl FromRef<TestHttpState> for Arc<SourceCrudService<SampleSpec>> {
        fn from_ref(s: &TestHttpState) -> Self {
            s.crud.clone()
        }
    }
    let _router: Router<TestHttpState> = Router::new()
        .merge(sources_prefixed_crud_router::<TestHttpState, SampleSpec>())
        .route(
            "/sources/{id}/test",
            post(|Path(_id): Path<Uuid>| async { "ok" }),
        );
}
