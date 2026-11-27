use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};

pub const DEFAULT_TOOL_SESSION_TTL: Duration = Duration::from_secs(7 * 24 * 3600);
const DEFAULT_UNLOCK_RATE_WINDOW: Duration = Duration::from_secs(60);
const DEFAULT_MAX_FAILED_UNLOCKS: usize = 5;
const MAX_ACTIVE_SESSIONS: usize = 256;
const MAX_TRACKED_UNLOCK_IPS: usize = 1024;

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let max_len = a.len().max(b.len());
    let len_diff = a.len() ^ b.len();
    let mut byte_diff: u8 = 0;
    for idx in 0..max_len {
        let ba = a.get(idx).copied().unwrap_or(0);
        let bb = b.get(idx).copied().unwrap_or(0);
        byte_diff |= ba ^ bb;
    }
    len_diff == 0 && byte_diff == 0
}

fn sha256_digest(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

fn salted_password_digest(salt: &[u8; 32], password: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"xmedia-tool-auth-v2:");
    hasher.update(salt);
    hasher.update(b":");
    hasher.update(password.as_bytes());
    hasher.finalize().into()
}

fn generate_csprng_salt() -> [u8; 32] {
    let mut salt = [0u8; 32];
    salt[..16].copy_from_slice(Uuid::new_v4().as_bytes());
    salt[16..].copy_from_slice(Uuid::new_v4().as_bytes());
    salt
}

fn generate_random_session_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

struct PasswordVerifier {
    salt: [u8; 32],
    digest: [u8; 32],
}

#[derive(Default)]
struct AuthState {
    sessions: HashMap<[u8; 32], Instant>,
    failed_unlocks: HashMap<Option<IpAddr>, VecDeque<Instant>>,
}

impl AuthState {
    fn prune_expired_sessions(&mut self, now: Instant) {
        self.sessions.retain(|_, expires_at| *expires_at > now);
    }

    fn prune_unlock_window(&mut self, now: Instant, window: Duration) {
        self.failed_unlocks.retain(|_, attempts| {
            while let Some(&front) = attempts.front() {
                if now.saturating_duration_since(front) >= window {
                    attempts.pop_front();
                } else {
                    break;
                }
            }
            !attempts.is_empty()
        });
    }
}

#[derive(Clone)]
pub struct ToolAuthService {
    verifier: Option<Arc<PasswordVerifier>>,
    state: Arc<Mutex<AuthState>>,
    session_ttl: Duration,
    unlock_window: Duration,
    max_failed_unlocks: usize,
}

impl ToolAuthService {
    pub fn new(password: Option<String>) -> Self {
        Self::with_options(
            password,
            DEFAULT_TOOL_SESSION_TTL,
            DEFAULT_UNLOCK_RATE_WINDOW,
            DEFAULT_MAX_FAILED_UNLOCKS,
        )
    }

    fn with_options(
        password: Option<String>,
        session_ttl: Duration,
        unlock_window: Duration,
        max_failed_unlocks: usize,
    ) -> Self {
        let clean = password
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty());
        let verifier = clean.map(|pwd| {
            let salt = generate_csprng_salt();
            let digest = salted_password_digest(&salt, &pwd);
            Arc::new(PasswordVerifier { salt, digest })
        });
        Self {
            verifier,
            state: Arc::new(Mutex::new(AuthState::default())),
            session_ttl,
            unlock_window,
            max_failed_unlocks: max_failed_unlocks.max(1),
        }
    }

    pub fn session_ttl_secs(&self) -> u64 {
        self.session_ttl.as_secs()
    }

    pub fn requires_password(&self) -> bool {
        self.verifier.is_some()
    }

    #[cfg(test)]
    fn unlock(&self, password: &str) -> DomainResult<String> {
        self.unlock_with_ip(password, None)
    }

    pub fn unlock_with_ip(
        &self,
        password: &str,
        client_ip: Option<IpAddr>,
    ) -> DomainResult<String> {
        let Some(ref verifier) = self.verifier else {
            return Ok("open-mode".to_string());
        };
        let now = Instant::now();
        let ip_key = client_ip.map(|ip| ip.to_canonical());
        let candidate = salted_password_digest(&verifier.salt, password.trim());

        let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        guard.prune_unlock_window(now, self.unlock_window);
        if guard
            .failed_unlocks
            .get(&ip_key)
            .is_some_and(|q| q.len() >= self.max_failed_unlocks)
        {
            return Err(DomainError::TooManyRequests(
                "too many failed unlock attempts; please wait before retrying".into(),
            ));
        }

        if !constant_time_eq(&candidate, &verifier.digest) {
            if guard.failed_unlocks.len() >= MAX_TRACKED_UNLOCK_IPS
                && !guard.failed_unlocks.contains_key(&ip_key)
            {
                guard.failed_unlocks.clear();
            }
            guard
                .failed_unlocks
                .entry(ip_key)
                .or_default()
                .push_back(now);
            return Err(DomainError::Unauthorized("invalid access password".into()));
        }

        guard.failed_unlocks.remove(&ip_key);

        guard.prune_expired_sessions(now);
        if guard.sessions.len() >= MAX_ACTIVE_SESSIONS
            && let Some(oldest_key) = guard
                .sessions
                .iter()
                .min_by_key(|(_, exp)| **exp)
                .map(|(k, _)| *k)
        {
            guard.sessions.remove(&oldest_key);
        }

        let token = generate_random_session_token();
        let token_hash = sha256_digest(token.as_bytes());
        guard.sessions.insert(token_hash, now + self.session_ttl);
        Ok(token)
    }

    pub fn verify_token(&self, provided: Option<&str>) -> DomainResult<()> {
        if self.verifier.is_none() {
            return Ok(());
        }
        let Some(tok) = provided.map(str::trim).filter(|s| !s.is_empty()) else {
            return Err(DomainError::Unauthorized(
                "access password token required".into(),
            ));
        };
        let token_hash = sha256_digest(tok.as_bytes());
        let now = Instant::now();
        let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        guard.prune_expired_sessions(now);
        if guard.sessions.contains_key(&token_hash) {
            Ok(())
        } else {
            Err(DomainError::Unauthorized(
                "access password token required".into(),
            ))
        }
    }

    #[cfg(test)]
    fn lock(&self, provided: Option<&str>) {
        self.lock_tokens(provided);
    }

    pub fn lock_tokens<'a>(&self, tokens: impl IntoIterator<Item = &'a str>) {
        if self.verifier.is_none() {
            return;
        }
        let now = Instant::now();
        let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        guard.prune_expired_sessions(now);
        for tok in tokens {
            let trimmed = tok.trim();
            if !trimmed.is_empty() {
                let token_hash = sha256_digest(trimmed.as_bytes());
                guard.sessions.remove(&token_hash);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq_handles_equal_different_and_mismatched_lengths() {
        assert!(constant_time_eq(b"abc123", b"abc123"));
        assert!(!constant_time_eq(b"abc123", b"abc124"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(!constant_time_eq(b"", b"a"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn auth_service_issues_distinct_random_sessions_and_invalidates_on_lock() {
        let auth = ToolAuthService::new(Some("secret-pass".into()));
        assert!(auth.requires_password());
        assert!(auth.unlock("wrong").is_err());

        let token1 = auth.unlock("secret-pass").unwrap();
        let token2 = auth.unlock("secret-pass").unwrap();
        assert_ne!(token1, token2);
        assert_eq!(token1.len(), 64);
        assert!(auth.verify_token(Some(&token1)).is_ok());
        assert!(auth.verify_token(Some(&token2)).is_ok());
        assert!(auth.verify_token(Some("bad-token")).is_err());
        assert!(auth.verify_token(None).is_err());

        auth.lock(Some(&token1));
        assert!(auth.verify_token(Some(&token1)).is_err());
        assert!(auth.verify_token(Some(&token2)).is_ok());
    }

    #[test]
    fn auth_service_enforces_session_expiry_and_unlock_rate_limiting() {
        let auth = ToolAuthService::with_options(
            Some("secret-pass".into()),
            Duration::from_millis(25),
            Duration::from_millis(60),
            3,
        );

        let tok = auth.unlock("secret-pass").unwrap();
        assert!(auth.verify_token(Some(&tok)).is_ok());
        std::thread::sleep(Duration::from_millis(35));
        assert!(auth.verify_token(Some(&tok)).is_err());

        assert!(matches!(
            auth.unlock("bad-1"),
            Err(DomainError::Unauthorized(_))
        ));
        assert!(matches!(
            auth.unlock("bad-2"),
            Err(DomainError::Unauthorized(_))
        ));
        assert!(matches!(
            auth.unlock("bad-3"),
            Err(DomainError::Unauthorized(_))
        ));
        assert!(matches!(
            auth.unlock("secret-pass"),
            Err(DomainError::TooManyRequests(_))
        ));

        std::thread::sleep(Duration::from_millis(70));
        assert!(auth.unlock("secret-pass").is_ok());
    }

    #[test]
    fn unlock_rate_limiting_is_isolated_per_client_ip() {
        let auth = ToolAuthService::with_options(
            Some("secret-pass".into()),
            Duration::from_secs(60),
            Duration::from_secs(60),
            2,
        );
        let ip_a: IpAddr = "192.168.1.10".parse().unwrap();
        let ip_b: IpAddr = "192.168.1.20".parse().unwrap();

        assert!(auth.unlock_with_ip("wrong", Some(ip_a)).is_err());
        assert!(auth.unlock_with_ip("wrong", Some(ip_a)).is_err());
        assert!(matches!(
            auth.unlock_with_ip("secret-pass", Some(ip_a)),
            Err(DomainError::TooManyRequests(_))
        ));
        assert!(auth.unlock_with_ip("secret-pass", Some(ip_b)).is_ok());
    }
}
