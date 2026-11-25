use std::time::Duration;

use async_trait::async_trait;
use jiff::Timestamp;

use super::rate_limit::{AUTH_RATE_LIMIT_MAX_KEYS, AuthRateLimiter};
use super::*;

#[derive(Default)]
struct MemUserRepo {
    users: std::sync::Mutex<Vec<User>>,
}

#[async_trait]
impl UserRepository for MemUserRepo {
    async fn count(&self) -> DomainResult<u64> {
        Ok(self.users.lock().unwrap().len() as u64)
    }
    async fn has_any_user(&self) -> DomainResult<bool> {
        Ok(!self.users.lock().unwrap().is_empty())
    }
    async fn list_all(&self) -> DomainResult<Vec<User>> {
        Ok(self.users.lock().unwrap().clone())
    }
    async fn find_by_id(&self, id: UserId) -> DomainResult<Option<User>> {
        Ok(self
            .users
            .lock()
            .unwrap()
            .iter()
            .find(|u| u.id == id)
            .cloned())
    }
    async fn find_by_username(&self, username: &str) -> DomainResult<Option<User>> {
        let norm = username.trim().to_ascii_lowercase();
        Ok(self
            .users
            .lock()
            .unwrap()
            .iter()
            .find(|u| u.username == norm)
            .cloned())
    }
    async fn find_by_api_key(&self, api_key: &str) -> DomainResult<Option<User>> {
        Ok(self
            .users
            .lock()
            .unwrap()
            .iter()
            .find(|u| u.api_key == api_key)
            .cloned())
    }
    async fn save(&self, user: &User) -> DomainResult<()> {
        let mut g = self.users.lock().unwrap();
        if let Some(slot) = g.iter_mut().find(|u| u.id == user.id) {
            *slot = user.clone();
        } else {
            g.push(user.clone());
        }
        Ok(())
    }
    async fn update_preferences(
        &self,
        _id: UserId,
        _preferences: &UserPreferences,
        _updated_at: Timestamp,
    ) -> DomainResult<bool> {
        Ok(true)
    }
    async fn update_credentials(
        &self,
        _id: UserId,
        _password_hash: &str,
        _subsonic_secret_enc: &str,
        _updated_at: Timestamp,
    ) -> DomainResult<bool> {
        Ok(true)
    }
    async fn update_api_key(
        &self,
        _id: UserId,
        _api_key: &str,
        _updated_at: Timestamp,
    ) -> DomainResult<bool> {
        Ok(true)
    }
    async fn delete(&self, id: UserId) -> DomainResult<()> {
        self.users.lock().unwrap().retain(|u| u.id != id);
        Ok(())
    }
}

struct FastTestHasher;

#[async_trait]
impl PasswordHasherPort for FastTestHasher {
    async fn hash_password(&self, plain: &str) -> DomainResult<String> {
        Ok(format!("hash:{plain}"))
    }
    async fn verify_password(&self, plain: &str, hash: &str) -> DomainResult<bool> {
        Ok(hash == format!("hash:{plain}"))
    }
    fn encrypt_subsonic_secret(&self, plain: &str) -> DomainResult<String> {
        Ok(format!("enc:{plain}"))
    }
    fn verify_subsonic_token(
        &self,
        encrypted_secret: &str,
        _fallback_api_key: &str,
        salt: &str,
        expected_token_hex: &str,
    ) -> DomainResult<bool> {
        Ok(expected_token_hex == format!("{encrypted_secret}:{salt}"))
    }
    fn generate_api_key(&self) -> String {
        "xmk_valid_admin_key_123456".into()
    }
}

struct DummyTokenSvc;

impl TokenServicePort for DummyTokenSvc {
    fn issue_token(&self, claims: &AuthClaims) -> DomainResult<String> {
        Ok(format!("tok:{}:{}", claims.user_id, claims.jti))
    }
    fn verify_token(&self, _token: &str) -> DomainResult<AuthClaims> {
        Err(DomainError::Unauthorized("unused".into()))
    }
    fn ttl_seconds(&self) -> u64 {
        3600
    }
}

#[test]
fn constant_time_eq_compares_bytes_accurately() {
    assert!(constant_time_eq(b"xmk_secret_123", b"xmk_secret_123"));
    assert!(!constant_time_eq(b"xmk_secret_123", b"xmk_secret_124"));
    assert!(!constant_time_eq(b"short", b"longer"));
    assert!(constant_time_eq(b"", b""));
}

#[test]
fn rate_limiter_never_evicts_active_buckets_under_global_key_flood() {
    let limiter = AuthRateLimiter::new(Duration::from_secs(60), 3);
    let ip_a: IpAddr = "203.0.113.10".parse().unwrap();

    for _ in 0..3 {
        assert!(
            limiter
                .record_failure_and_check(Some(ip_a), "login:admin")
                .is_ok()
        );
    }
    assert!(limiter.is_limited(Some(ip_a), "login:admin"));
    assert!(matches!(
        limiter.record_failure_and_check(Some(ip_a), "login:admin"),
        Err(DomainError::TooManyRequests(_))
    ));

    // Flood past AUTH_RATE_LIMIT_MAX_KEYS with distinct unauthenticated keys.
    for i in 0..(AUTH_RATE_LIMIT_MAX_KEYS + 10) {
        let _ = limiter.record_failure_and_check(None, &format!("flood:{i}"));
    }

    // Active `(ip_a, login:admin)` bucket must NOT be evicted or reset.
    assert!(limiter.is_limited(Some(ip_a), "login:admin"));
    assert!(matches!(
        limiter.record_failure_and_check(Some(ip_a), "login:admin"),
        Err(DomainError::TooManyRequests(_))
    ));
    assert!(matches!(
        limiter.record_failure_and_check(None, "flood:extra"),
        Err(DomainError::TooManyRequests(_))
    ));
}

#[tokio::test]
async fn ip_scoped_rate_limiter_locks_out_attacker_ip_without_blocking_legitimate_ip() {
    let auth = AuthService::new(
        Arc::new(MemUserRepo::default()),
        Arc::new(FastTestHasher),
        Arc::new(DummyTokenSvc),
        Arc::new(RwLock::new(HashMap::new())),
    );
    let setup = auth
        .setup_initial_admin(SetupAdminInput {
            username: "admin".into(),
            display_name: Some("Admin".into()),
            password: "admin-good-pass".into(),
        })
        .await
        .unwrap();

    let ip_attacker: IpAddr = "203.0.113.99".parse().unwrap();
    let ip_attacker_v6_mapped: IpAddr = "::ffff:203.0.113.99".parse().unwrap();
    let ip_legit: IpAddr = "198.51.100.7".parse().unwrap();

    // Attacker on IP A spams failed `"admin"` logins up to the limit
    for i in 0..AUTH_RATE_LIMIT_MAX_FAILURES {
        assert!(matches!(
            auth.login_with_ip("admin", &format!("bad-{i}"), Some(ip_attacker))
                .await,
            Err(DomainError::Unauthorized(_))
        ));
    }
    // IP A (including IPv4-mapped IPv6 equivalent) is now rate-limited on `(IP_A, "admin")`
    assert!(matches!(
        auth.login_with_ip("admin", "bad-overflow", Some(ip_attacker))
            .await,
        Err(DomainError::TooManyRequests(_))
    ));
    assert!(matches!(
        auth.login_with_ip("admin", "admin-good-pass", Some(ip_attacker_v6_mapped))
            .await,
        Err(DomainError::TooManyRequests(_))
    ));

    // Legitimate `"admin"` on IP B is NOT locked out or throttled — even after a normal typo
    assert!(matches!(
        auth.login_with_ip("admin", "single-typo", Some(ip_legit))
            .await,
        Err(DomainError::Unauthorized(_))
    ));
    let legit_session = auth
        .login_with_ip("admin", "admin-good-pass", Some(ip_legit))
        .await
        .expect("legitimate admin on IP B must not be locked out by IP A");
    assert_eq!(legit_session.user.username, "admin");

    // Basic auth and API key failures on IP A also cannot lock out IP B
    for i in 0..AUTH_RATE_LIMIT_MAX_FAILURES {
        let _ = auth
            .verify_basic_credentials_with_ip("admin", &format!("bad-{i}"), Some(ip_attacker))
            .await;
        let _ = auth
            .authenticate_api_key_with_ip(&format!("xmk_bad_{i}"), Some(ip_attacker))
            .await;
    }
    assert!(matches!(
        auth.verify_basic_credentials_with_ip("admin", "bad-more", Some(ip_attacker))
            .await,
        Err(DomainError::TooManyRequests(_))
    ));
    assert!(matches!(
        auth.authenticate_api_key_with_ip("xmk_bad_more", Some(ip_attacker))
            .await,
        Err(DomainError::TooManyRequests(_))
    ));

    let basic_ok = auth
        .verify_basic_credentials_with_ip("admin", "admin-good-pass", Some(ip_legit))
        .await
        .unwrap();
    assert_eq!(basic_ok.username, "admin");
    let key_ok = auth
        .authenticate_api_key_with_ip(&setup.user.api_key, Some(ip_legit))
        .await
        .unwrap();
    assert_eq!(key_ok.username, "admin");
}

#[tokio::test]
async fn valid_admin_credentials_and_api_key_are_not_locked_out_by_failed_attempts() {
    use crate::application::activity::{ActivityHeatmapQuery, RecordActivityInput};
    use crate::domain::activity::{UserActivityDaily, UserActivityRepository};

    let auth = AuthService::new(
        Arc::new(MemUserRepo::default()),
        Arc::new(FastTestHasher),
        Arc::new(DummyTokenSvc),
        Arc::new(RwLock::new(HashMap::new())),
    );
    let setup = auth
        .setup_initial_admin(SetupAdminInput {
            username: "admin".into(),
            display_name: Some("Admin".into()),
            password: "admin-good-pass".into(),
        })
        .await
        .unwrap();

    for i in 0..AUTH_RATE_LIMIT_MAX_FAILURES {
        assert!(matches!(
            auth.login("admin", &format!("wrong-{i}")).await,
            Err(DomainError::Unauthorized(_))
        ));
    }
    assert!(matches!(
        auth.login("admin", "wrong-overflow").await,
        Err(DomainError::TooManyRequests(_))
    ));

    let valid_sess = auth.login("admin", "admin-good-pass").await.unwrap();
    assert_eq!(valid_sess.user.username, "admin");
    assert!(matches!(
        auth.login("admin", "still-wrong-after-valid").await,
        Err(DomainError::TooManyRequests(_))
    ));

    for i in 0..AUTH_RATE_LIMIT_MAX_FAILURES {
        assert!(matches!(
            auth.authenticate_api_key(&format!("xmk_bad_{i}")).await,
            Err(DomainError::Unauthorized(_))
        ));
    }
    assert!(matches!(
        auth.authenticate_api_key("xmk_bad_overflow").await,
        Err(DomainError::TooManyRequests(_))
    ));
    let api_user = auth
        .authenticate_api_key(&setup.user.api_key)
        .await
        .unwrap();
    assert_eq!(api_user.username, "admin");

    #[derive(Default)]
    struct MemActivityRepo {
        rows: tokio::sync::Mutex<Vec<UserActivityDaily>>,
    }

    #[async_trait]
    impl UserActivityRepository for MemActivityRepo {
        async fn list_by_user_and_domain(
            &self,
            user_id: UserId,
            media_kind: MediaKind,
        ) -> DomainResult<Vec<UserActivityDaily>> {
            let g = self.rows.lock().await;
            Ok(g.iter()
                .filter(|r| r.user_id == user_id && r.media_kind == media_kind)
                .cloned()
                .collect())
        }
        async fn find_by_key(
            &self,
            user_id: UserId,
            media_kind: MediaKind,
            activity_date: &str,
        ) -> DomainResult<Option<UserActivityDaily>> {
            let g = self.rows.lock().await;
            Ok(g.iter()
                .find(|r| {
                    r.user_id == user_id
                        && r.media_kind == media_kind
                        && r.activity_date == activity_date
                })
                .cloned())
        }
        async fn save(&self, record: &UserActivityDaily) -> DomainResult<()> {
            let mut g = self.rows.lock().await;
            if let Some(slot) = g.iter_mut().find(|r| r.id == record.id) {
                *slot = record.clone();
            } else {
                g.push(record.clone());
            }
            Ok(())
        }
        async fn delete_by_user(&self, user_id: UserId) -> DomainResult<()> {
            self.rows.lock().await.retain(|r| r.user_id != user_id);
            Ok(())
        }
    }

    let activity = Arc::new(ActivityService::new(Arc::new(MemActivityRepo::default())));
    let auth_with_act = auth.with_activity(activity.clone());
    let member = auth_with_act
        .create_user(
            &api_user,
            CreateUserInput {
                username: "member_to_delete".into(),
                display_name: None,
                password: "member-pass-123".into(),
                role: UserRole::Member,
            },
        )
        .await
        .unwrap();
    let today = Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::UTC)
        .date()
        .to_string();
    activity
        .record_heartbeat(
            member.id,
            RecordActivityInput {
                media_kind: MediaKind::Book,
                date: today.clone(),
                delta_secs: 90,
                new_session: true,
                item_title: Some("Sample".into()),
            },
        )
        .await
        .unwrap();
    activity
        .record_heartbeat(
            api_user.id,
            RecordActivityInput {
                media_kind: MediaKind::Book,
                date: today.clone(),
                delta_secs: 120,
                new_session: true,
                item_title: Some("AdminBook".into()),
            },
        )
        .await
        .unwrap();

    auth_with_act
        .delete_user(&api_user, member.id)
        .await
        .unwrap();

    let deleted_hm = activity
        .get_heatmap(
            member.id,
            ActivityHeatmapQuery {
                media_kind: MediaKind::Book,
                year: None,
                today: Some(today.clone()),
            },
        )
        .await
        .unwrap();
    assert_eq!(deleted_hm.summary.total_secs, 0);

    let admin_hm = activity
        .get_heatmap(
            api_user.id,
            ActivityHeatmapQuery {
                media_kind: MediaKind::Book,
                year: None,
                today: Some(today),
            },
        )
        .await
        .unwrap();
    assert_eq!(admin_hm.summary.total_secs, 120);
}
