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

    async fn find(&self, key: UserLookup<'_>) -> DomainResult<Option<User>> {
        let guard = self.users.lock().unwrap();
        let found = match key {
            UserLookup::Id(id) => guard.iter().find(|u| u.id == id),
            UserLookup::Username(username) => guard.iter().find(|u| u.username == username),
            UserLookup::ApiKey(api_key) => guard.iter().find(|u| u.api_key == api_key),
        };
        Ok(found.cloned())
    }

    async fn list_all(&self) -> DomainResult<Vec<User>> {
        Ok(self.users.lock().unwrap().clone())
    }

    async fn save(&self, user: &User) -> DomainResult<()> {
        let mut g = self.users.lock().unwrap();
        if let Some(existing) = g.iter_mut().find(|u| u.id == user.id) {
            *existing = user.clone();
        } else {
            g.push(user.clone());
        }
        Ok(())
    }

    async fn update(
        &self,
        id: UserId,
        patch: UserMutation<'_>,
        updated_at: Timestamp,
    ) -> DomainResult<bool> {
        let mut g = self.users.lock().unwrap();
        let Some(u) = g.iter_mut().find(|u| u.id == id) else {
            return Ok(false);
        };
        match patch {
            UserMutation::Preferences(preferences) => u.preferences = preferences.clone(),
            UserMutation::Credentials {
                password_hash,
                subsonic_secret_enc,
            } => {
                u.password_hash = password_hash.to_string();
                u.subsonic_secret_enc = subsonic_secret_enc.to_string();
            }
            UserMutation::ApiKey(api_key) => u.api_key = api_key.to_string(),
        }
        u.updated_at = updated_at;
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
        _salt: &str,
        expected_token_hex: &str,
    ) -> DomainResult<bool> {
        Ok(encrypted_secret == format!("enc:{expected_token_hex}"))
    }
    fn generate_api_key(&self) -> String {
        format!("xmk_{}", uuid::Uuid::new_v4().simple())
    }
}

struct DummyTokenSvc;

impl TokenServicePort for DummyTokenSvc {
    fn issue_token(&self, claims: &AuthClaims) -> DomainResult<String> {
        Ok(format!("tok:{}:{}", claims.user_id, claims.cred_ver))
    }
    fn verify_token(&self, _token: &str) -> DomainResult<AuthClaims> {
        Err(DomainError::Unauthorized("unused in unit test".into()))
    }
    fn ttl_seconds(&self) -> u64 {
        3600
    }
}

#[test]
fn rate_limiter_key_flood_never_evicts_active_bucket_and_applies_overflow_limit() {
    let limiter = AuthRateLimiter::new(Duration::from_secs(60), 3);
    let ip_a: IpAddr = "203.0.113.10".parse().unwrap();

    for _ in 0..3 {
        let _ = limiter.record_failure_and_check(Some(ip_a), "login:admin");
    }
    assert!(limiter.is_limited(Some(ip_a), "login:admin"));
    assert!(matches!(
        limiter.record_failure_and_check(Some(ip_a), "login:admin"),
        Err(DomainError::TooManyRequests(_))
    ));

    for i in 0..(AUTH_RATE_LIMIT_MAX_KEYS + 10) {
        let _ = limiter.record_failure_and_check(None, &format!("flood:{i}"));
    }

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

    for i in 0..AUTH_RATE_LIMIT_MAX_FAILURES {
        assert!(matches!(
            auth.login("admin", &format!("bad-{i}"), Some(ip_attacker))
                .await,
            Err(DomainError::Unauthorized(_))
        ));
    }
    assert!(matches!(
        auth.login("admin", "bad-overflow", Some(ip_attacker)).await,
        Err(DomainError::TooManyRequests(_))
    ));
    assert!(matches!(
        auth.login("admin", "admin-good-pass", Some(ip_attacker_v6_mapped))
            .await,
        Err(DomainError::TooManyRequests(_))
    ));

    assert!(matches!(
        auth.login("admin", "single-typo", Some(ip_legit)).await,
        Err(DomainError::Unauthorized(_))
    ));
    let legit_session = auth
        .login("admin", "admin-good-pass", Some(ip_legit))
        .await
        .expect("legitimate admin on IP B must not be locked out by IP A");
    assert_eq!(legit_session.user.username, "admin");

    for i in 0..AUTH_RATE_LIMIT_MAX_FAILURES {
        let _ = auth
            .verify_basic("admin", &format!("bad-{i}"), Some(ip_attacker))
            .await;
        let _ = auth
            .verify_api_key(&format!("xmk_bad_{i}"), Some(ip_attacker))
            .await;
    }
    assert!(matches!(
        auth.verify_basic("admin", "bad-more", Some(ip_attacker))
            .await,
        Err(DomainError::TooManyRequests(_))
    ));
    assert!(matches!(
        auth.verify_api_key("xmk_bad_more", Some(ip_attacker)).await,
        Err(DomainError::TooManyRequests(_))
    ));

    let legit_basic = auth
        .verify_basic("admin", "admin-good-pass", Some(ip_legit))
        .await
        .expect("Basic auth on IP B must succeed");
    assert_eq!(legit_basic.username, "admin");
    let legit_api = auth
        .verify_api_key(&setup.user.api_key, Some(ip_legit))
        .await
        .expect("API key auth on IP B must succeed");
    assert_eq!(legit_api.username, "admin");

    let ip_probe: IpAddr = "203.0.113.200".parse().unwrap();
    for i in 0..AUTH_RATE_LIMIT_MAX_FAILURES {
        assert!(matches!(
            auth.login(&format!("ghost_{i}"), "nope", Some(ip_probe))
                .await,
            Err(DomainError::Unauthorized(_))
        ));
    }
    assert!(matches!(
        auth.login("ghost_overflow", "nope", Some(ip_probe)).await,
        Err(DomainError::TooManyRequests(_))
    ));
    assert!(
        auth.login("admin", "admin-good-pass", Some(ip_legit))
            .await
            .is_ok()
    );

    let ip_sub: IpAddr = "203.0.113.201".parse().unwrap();
    for i in 0..AUTH_RATE_LIMIT_MAX_FAILURES {
        assert!(matches!(
            auth.verify_subsonic(
                SubsonicCredentials::TokenSalt {
                    username: "admin".into(),
                    token: format!("bad-tok-{i}"),
                    salt: "s".into(),
                },
                Some(ip_sub),
            )
            .await,
            Err(DomainError::Unauthorized(_))
        ));
    }
    assert!(matches!(
        auth.verify_subsonic(
            SubsonicCredentials::TokenSalt {
                username: "admin".into(),
                token: "admin-good-pass".into(),
                salt: "s".into(),
            },
            Some(ip_sub),
        )
        .await,
        Err(DomainError::TooManyRequests(_))
    ));
    assert!(
        auth.verify_subsonic(
            SubsonicCredentials::TokenSalt {
                username: "admin".into(),
                token: "admin-good-pass".into(),
                salt: "s".into(),
            },
            Some(ip_legit),
        )
        .await
        .is_ok()
    );

    // Without client IP (None), valid API keys still work while invalid guesses are rate-limited
    for i in 0..AUTH_RATE_LIMIT_MAX_FAILURES {
        assert!(matches!(
            auth.verify_api_key(&format!("xmk_bad_{i}"), None).await,
            Err(DomainError::Unauthorized(_))
        ));
    }
    assert!(matches!(
        auth.verify_api_key("xmk_bad_overflow", None).await,
        Err(DomainError::TooManyRequests(_))
    ));
    assert!(auth.verify_api_key(&setup.user.api_key, None).await.is_ok());
}
