use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::time::{Duration, Instant};

use crate::domain::error::{DomainError, DomainResult};
use crate::domain::user::User;
use crate::infra::stream::sha256_hex;

/// Computes a deterministic credential version fingerprint from a user's password hash, API key,
/// and role so password changes, admin password resets, API key rotations, and role changes
/// immediately invalidate all previously issued JWTs even across server restarts.
pub(super) fn compute_user_cred_ver(user: &User) -> String {
    let material = format!(
        "{}\0{}\0{}",
        user.password_hash,
        user.api_key,
        user.role.as_str()
    );
    let full = sha256_hex(material.as_bytes());
    full[..16].to_string()
}

/// Constant-time byte-slice comparison using XOR accumulation to avoid timing side-channels
/// when comparing API keys or secrets.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    std::hint::black_box(diff) == 0
}

pub(super) const MAX_REVOKED_JTIS: usize = 8192;
pub(super) const AUTH_RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60);
pub(crate) const AUTH_RATE_LIMIT_MAX_FAILURES: usize = 20;
pub(crate) const AUTH_RATE_LIMIT_MAX_KEYS: usize = 4096;

type RateLimitKey = (Option<IpAddr>, String);

struct RateLimiterState {
    entries: HashMap<RateLimitKey, VecDeque<Instant>>,
    overflow: VecDeque<Instant>,
}

pub(super) struct AuthRateLimiter {
    window: Duration,
    max_failures: usize,
    state: std::sync::Mutex<RateLimiterState>,
}

impl AuthRateLimiter {
    pub(super) fn new(window: Duration, max_failures: usize) -> Self {
        Self {
            window,
            max_failures: max_failures.max(1),
            state: std::sync::Mutex::new(RateLimiterState {
                entries: HashMap::new(),
                overflow: VecDeque::new(),
            }),
        }
    }

    fn normalize_key(client_ip: Option<IpAddr>, key: &str) -> RateLimitKey {
        let canonical_ip = client_ip.map(|ip| ip.to_canonical());
        let trimmed = key.trim();
        let norm_subject = if trimmed.is_empty() {
            "<anonymous>".to_string()
        } else {
            trimmed.to_ascii_lowercase()
        };
        (canonical_ip, norm_subject)
    }

    fn prune_hits(hits: &mut VecDeque<Instant>, now: Instant, window: Duration) {
        while hits
            .front()
            .is_some_and(|ts| now.saturating_duration_since(*ts) >= window)
        {
            hits.pop_front();
        }
    }

    pub(super) fn is_limited(&self, client_ip: Option<IpAddr>, key: &str) -> bool {
        let norm = Self::normalize_key(client_ip, key);
        let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let window = self.window;
        if let Some(hits) = guard.entries.get_mut(&norm) {
            Self::prune_hits(hits, now, window);
            let limited = hits.len() >= self.max_failures;
            if hits.is_empty() {
                guard.entries.remove(&norm);
            }
            return limited;
        }
        if guard.entries.len() >= AUTH_RATE_LIMIT_MAX_KEYS {
            Self::prune_hits(&mut guard.overflow, now, window);
            return guard.overflow.len() >= self.max_failures;
        }
        false
    }

    /// Records a failed authentication attempt for `(client_ip, key)` and returns
    /// `Err(TooManyRequests)` once the active failure window has reached `max_failures`.
    /// Never evicts active rate-limit windows.
    pub(super) fn record_failure_and_check(
        &self,
        client_ip: Option<IpAddr>,
        key: &str,
    ) -> DomainResult<()> {
        let norm = Self::normalize_key(client_ip, key);
        let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let window = self.window;

        if let Some(hits) = guard.entries.get_mut(&norm) {
            Self::prune_hits(hits, now, window);
            if hits.len() >= self.max_failures {
                return Err(DomainError::TooManyRequests(
                    "too many failed authentication attempts; please try again later".into(),
                ));
            }
            hits.push_back(now);
            return Ok(());
        }

        if guard.entries.len() >= AUTH_RATE_LIMIT_MAX_KEYS {
            guard.entries.retain(|_, hits| {
                Self::prune_hits(hits, now, window);
                !hits.is_empty()
            });
        }

        if guard.entries.len() < AUTH_RATE_LIMIT_MAX_KEYS {
            let mut hits = VecDeque::with_capacity(4);
            hits.push_back(now);
            guard.entries.insert(norm, hits);
            return Ok(());
        }

        Self::prune_hits(&mut guard.overflow, now, window);
        if guard.overflow.len() >= self.max_failures {
            return Err(DomainError::TooManyRequests(
                "too many failed authentication attempts; please try again later".into(),
            ));
        }
        guard.overflow.push_back(now);
        Ok(())
    }

    /// Clears minor pre-lockout typo counters upon valid authentication while preserving an
    /// active brute-force lockout window so concurrent attackers remain rate-limited.
    pub(super) fn clear_below_limit(&self, client_ip: Option<IpAddr>, key: &str) {
        let norm = Self::normalize_key(client_ip, key);
        let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let window = self.window;
        if let Some(hits) = guard.entries.get_mut(&norm) {
            Self::prune_hits(hits, now, window);
            if hits.len() < self.max_failures {
                guard.entries.remove(&norm);
            }
        }
    }
}
