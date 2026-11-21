use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use aes_gcm::Aes256Gcm;
use aes_gcm::aead::{Aead, KeyInit};
use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use md5::{Digest as Md5DigestTrait, Md5};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use uuid::Uuid;

use crate::application::ports::{AuthClaims, PasswordHasherPort, TokenServicePort};
use crate::domain::error::{DomainError, DomainResult};
use crate::domain::user::{UserId, UserRole};

/// Generates a 256-bit (64 hex character) cryptographically secure random secret using the OS RNG.
pub fn generate_random_secret_hex() -> String {
    let mut bytes = [0u8; 32];
    fill_os_random(&mut bytes);
    hex::encode(bytes)
}

fn fill_os_random(dest: &mut [u8]) {
    // Combine two CSPRNG sources: `Uuid::new_v4` is backed by OS getrandom, and hashing full
    // 128-bit UUID outputs with SHA-256 eliminates fixed version/variant bits so every output
    // bit has full uniform entropy.
    let mut offset = 0usize;
    let mut counter = 0u64;
    while offset < dest.len() {
        let u1 = Uuid::new_v4();
        let u2 = Uuid::new_v4();
        let mut sha = Sha256::new();
        sha.update(u1.as_bytes());
        sha.update(u2.as_bytes());
        sha.update(counter.to_le_bytes());
        let block = sha.finalize();
        let take = (dest.len() - offset).min(block.len());
        dest[offset..offset + take].copy_from_slice(&block[..take]);
        offset += take;
        counter += 1;
    }
}

pub const DEFAULT_PASSWORD_CACHE_TTL: Duration = Duration::from_secs(60);
pub const DEFAULT_PASSWORD_CACHE_CAPACITY: usize = 512;

fn sha256_bytes(input: &[u8]) -> [u8; 32] {
    let mut sha = Sha256::new();
    sha.update(input);
    let digest = sha.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

fn password_hash_prefix(hash: &str) -> String {
    hash.chars().take(48).collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct VerifyCacheKey {
    scope: String,
    password_hash_prefix: String,
    plain_sha256: [u8; 32],
}

/// Short-TTL, bounded-capacity in-memory cache for verified password hashes.
/// Never stores plaintext passwords; keys only contain scope, hash prefix, and SHA-256 digests.
pub struct PasswordVerifyCache {
    ttl: Duration,
    max_entries: usize,
    entries: Mutex<HashMap<VerifyCacheKey, Instant>>,
}

impl PasswordVerifyCache {
    pub fn new(ttl: Duration, max_entries: usize) -> Self {
        Self {
            ttl,
            max_entries: max_entries.max(1),
            entries: Mutex::new(HashMap::new()),
        }
    }

    pub fn check_hash(&self, hash: &str, plain: &str) -> bool {
        let key = VerifyCacheKey {
            scope: hex::encode(sha256_bytes(hash.as_bytes())),
            password_hash_prefix: password_hash_prefix(hash),
            plain_sha256: sha256_bytes(plain.as_bytes()),
        };
        self.contains_valid(&key)
    }

    pub fn record_hash(&self, hash: &str, plain: &str) {
        let key = VerifyCacheKey {
            scope: hex::encode(sha256_bytes(hash.as_bytes())),
            password_hash_prefix: password_hash_prefix(hash),
            plain_sha256: sha256_bytes(plain.as_bytes()),
        };
        self.insert_valid(key);
    }

    pub fn check_user_basic(&self, user_id: UserId, password_hash: &str, plain: &str) -> bool {
        let key = VerifyCacheKey {
            scope: user_id.0.to_string(),
            password_hash_prefix: password_hash_prefix(password_hash),
            plain_sha256: sha256_bytes(plain.as_bytes()),
        };
        self.contains_valid(&key)
    }

    pub fn record_user_basic(&self, user_id: UserId, password_hash: &str, plain: &str) {
        let key = VerifyCacheKey {
            scope: user_id.0.to_string(),
            password_hash_prefix: password_hash_prefix(password_hash),
            plain_sha256: sha256_bytes(plain.as_bytes()),
        };
        self.insert_valid(key);
    }

    pub fn len(&self) -> usize {
        self.entries.lock().map(|g| g.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn contains_valid(&self, key: &VerifyCacheKey) -> bool {
        let Ok(mut guard) = self.entries.lock() else {
            return false;
        };
        let now = Instant::now();
        if let Some(&expires_at) = guard.get(key) {
            if now < expires_at {
                return true;
            }
            guard.remove(key);
        }
        false
    }

    fn insert_valid(&self, key: VerifyCacheKey) {
        let Ok(mut guard) = self.entries.lock() else {
            return;
        };
        let now = Instant::now();
        if guard.len() >= self.max_entries {
            guard.retain(|_, &mut exp| now < exp);
        }
        if guard.len() >= self.max_entries
            && let Some(oldest_key) = guard
                .iter()
                .min_by_key(|(_, exp)| **exp)
                .map(|(k, _)| k.clone())
        {
            guard.remove(&oldest_key);
        }
        guard.insert(key, now + self.ttl);
    }
}

impl Default for PasswordVerifyCache {
    fn default() -> Self {
        Self::new(DEFAULT_PASSWORD_CACHE_TTL, DEFAULT_PASSWORD_CACHE_CAPACITY)
    }
}

fn verify_argon2_raw(plain: &str, hash: &str) -> DomainResult<bool> {
    let parsed = PasswordHash::new(hash)
        .map_err(|e| DomainError::Persistence(format!("invalid password hash: {e}")))?;
    Ok(Argon2::default()
        .verify_password(plain.as_bytes(), &parsed)
        .is_ok())
}

fn hash_argon2_raw(plain: &str) -> DomainResult<String> {
    Argon2::default()
        .hash_password(plain.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| DomainError::Persistence(format!("password hashing failed: {e}")))
}

fn run_cpu_offloaded<T>(f: impl FnOnce() -> T) -> T {
    if tokio::runtime::Handle::try_current()
        .ok()
        .is_some_and(|h| h.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread)
    {
        tokio::task::block_in_place(f)
    } else {
        f()
    }
}

pub struct SecurityManager {
    cipher_key: [u8; 32],
    verify_cache: Arc<PasswordVerifyCache>,
}

impl SecurityManager {
    pub fn new(server_secret: &str) -> Self {
        let mut sha = Sha256::new();
        sha.update(b"xmedia-subsonic-secret-v1:");
        sha.update(server_secret.as_bytes());
        let digest = sha.finalize();
        let mut cipher_key = [0u8; 32];
        cipher_key.copy_from_slice(&digest);
        Self {
            cipher_key,
            verify_cache: Arc::new(PasswordVerifyCache::default()),
        }
    }

    pub fn verify_cache(&self) -> &PasswordVerifyCache {
        &self.verify_cache
    }

    /// Verifies a password against an Argon2 PHC hash without blocking Tokio worker threads.
    /// Uses the bounded short-TTL in-memory cache on repeated requests (e.g. concurrent OPDS reads).
    pub async fn verify_password_async(&self, plain: &str, hash: &str) -> DomainResult<bool> {
        if self.verify_cache.check_hash(hash, plain) {
            return Ok(true);
        }
        let plain_owned = plain.to_owned();
        let hash_owned = hash.to_owned();
        let matched =
            tokio::task::spawn_blocking(move || verify_argon2_raw(&plain_owned, &hash_owned))
                .await
                .map_err(|e| {
                    DomainError::Persistence(format!("password verification task failed: {e}"))
                })??;
        if matched {
            self.verify_cache.record_hash(hash, plain);
        }
        Ok(matched)
    }

    pub fn decrypt_subsonic_secret(&self, encoded: &str) -> DomainResult<String> {
        let (nonce_hex, ct_hex) = encoded
            .split_once(':')
            .ok_or_else(|| DomainError::Persistence("corrupted subsonic secret format".into()))?;
        let nonce_bytes = hex::decode(nonce_hex)
            .map_err(|e| DomainError::Persistence(format!("invalid nonce hex: {e}")))?;
        if nonce_bytes.len() != 12 {
            return Err(DomainError::Persistence(
                "invalid subsonic secret nonce length".into(),
            ));
        }
        let ciphertext = hex::decode(ct_hex)
            .map_err(|e| DomainError::Persistence(format!("invalid ciphertext hex: {e}")))?;

        let cipher = Aes256Gcm::new_from_slice(&self.cipher_key)
            .map_err(|e| DomainError::Persistence(format!("cipher init error: {e}")))?;
        let nonce = nonce_bytes
            .as_slice()
            .try_into()
            .map_err(|_| DomainError::Persistence("invalid subsonic secret nonce".into()))?;
        let plain = cipher
            .decrypt(&nonce, ciphertext.as_ref())
            .map_err(|_| DomainError::Unauthorized("failed to decrypt subsonic secret".into()))?;
        String::from_utf8(plain)
            .map_err(|e| DomainError::Persistence(format!("invalid UTF-8 secret: {e}")))
    }

    fn compute_md5_hex(secret: &str, salt: &str) -> String {
        let mut md5 = Md5::new();
        md5.update(secret.as_bytes());
        md5.update(salt.as_bytes());
        hex::encode(md5.finalize())
    }

    fn constant_time_eq_hex(a: &str, b: &str) -> bool {
        let a = a.trim().as_bytes();
        let b = b.trim().as_bytes();
        if a.len() != b.len() {
            return false;
        }
        let mut diff = 0u8;
        for (&x, &y) in a.iter().zip(b.iter()) {
            diff |= x.to_ascii_lowercase() ^ y.to_ascii_lowercase();
        }
        diff == 0
    }
}

impl PasswordHasherPort for SecurityManager {
    fn hash_password(&self, plain: &str) -> DomainResult<String> {
        run_cpu_offloaded(|| hash_argon2_raw(plain))
    }

    fn verify_password(&self, plain: &str, hash: &str) -> DomainResult<bool> {
        if self.verify_cache.check_hash(hash, plain) {
            return Ok(true);
        }
        let matched = run_cpu_offloaded(|| verify_argon2_raw(plain, hash))?;
        if matched {
            self.verify_cache.record_hash(hash, plain);
        }
        Ok(matched)
    }

    fn encrypt_subsonic_secret(&self, plain: &str) -> DomainResult<String> {
        let cipher = Aes256Gcm::new_from_slice(&self.cipher_key)
            .map_err(|e| DomainError::Persistence(format!("cipher init error: {e}")))?;
        let mut nonce_bytes = [0u8; 12];
        fill_os_random(&mut nonce_bytes);
        let nonce = nonce_bytes.into();
        let ciphertext = cipher
            .encrypt(&nonce, plain.as_bytes())
            .map_err(|e| DomainError::Persistence(format!("encryption failed: {e}")))?;
        Ok(format!(
            "{}:{}",
            hex::encode(nonce_bytes),
            hex::encode(ciphertext)
        ))
    }

    fn verify_subsonic_token(
        &self,
        encrypted_secret: &str,
        fallback_api_key: &str,
        salt: &str,
        expected_token_hex: &str,
    ) -> DomainResult<bool> {
        if let Ok(secret) = self.decrypt_subsonic_secret(encrypted_secret) {
            let candidate = Self::compute_md5_hex(&secret, salt);
            if Self::constant_time_eq_hex(&candidate, expected_token_hex) {
                return Ok(true);
            }
        }
        if !fallback_api_key.is_empty() {
            let fallback = Self::compute_md5_hex(fallback_api_key, salt);
            if Self::constant_time_eq_hex(&fallback, expected_token_hex) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn generate_api_key(&self) -> String {
        format!("xmk_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct JwtPayload {
    sub: String,
    username: String,
    role: String,
    #[serde(default)]
    ver: String,
    #[serde(default)]
    jti: String,
    exp: u64,
    iat: u64,
}

pub struct JwtTokenService {
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    ttl_seconds: u64,
}

impl JwtTokenService {
    pub fn new(secret: &str, ttl_seconds: u64) -> Self {
        Self {
            encoding_key: EncodingKey::from_secret(secret.as_bytes())
                .expect("HMAC secret key construction must succeed"),
            decoding_key: DecodingKey::from_secret(secret.as_bytes())
                .expect("HMAC secret key construction must succeed"),
            ttl_seconds,
        }
    }
}

impl TokenServicePort for JwtTokenService {
    fn issue_token(&self, claims: &AuthClaims) -> DomainResult<String> {
        let now = jsonwebtoken::get_current_timestamp();
        let jti = if claims.jti.is_empty() {
            Uuid::new_v4().to_string()
        } else {
            claims.jti.clone()
        };
        let payload = JwtPayload {
            sub: claims.user_id.0.to_string(),
            username: claims.username.clone(),
            role: claims.role.as_str().to_string(),
            ver: claims.cred_ver.clone(),
            jti,
            iat: now,
            exp: now.saturating_add(self.ttl_seconds),
        };
        encode(&Header::new(Algorithm::HS256), &payload, &self.encoding_key)
            .map_err(|e| DomainError::Unauthorized(format!("failed to encode token: {e}")))
    }

    fn verify_token(&self, token: &str) -> DomainResult<AuthClaims> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = true;
        let decoded = decode::<JwtPayload>(token, &self.decoding_key, &validation)
            .map_err(|e| DomainError::Unauthorized(format!("invalid or expired token: {e}")))?;

        let user_uuid = Uuid::parse_str(&decoded.claims.sub)
            .map_err(|e| DomainError::Unauthorized(format!("invalid subject uuid: {e}")))?;
        let role = UserRole::parse(&decoded.claims.role)?;

        Ok(AuthClaims {
            user_id: UserId(user_uuid),
            username: decoded.claims.username,
            role,
            cred_ver: decoded.claims.ver,
            jti: decoded.claims.jti,
            exp: decoded.claims.exp,
        })
    }

    fn ttl_seconds(&self) -> u64 {
        self.ttl_seconds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn verify_password_async_caches_positive_result_and_invalidates_on_hash_change() {
        let mgr = SecurityManager::new("test-secret");
        let hash1 = mgr.hash_password("super-secret-1").unwrap();
        let hash2 = mgr.hash_password("super-secret-2").unwrap();

        assert!(
            mgr.verify_password_async("super-secret-1", &hash1)
                .await
                .unwrap()
        );
        assert_eq!(mgr.verify_cache().len(), 1);
        assert!(mgr.verify_cache().check_hash(&hash1, "super-secret-1"));

        // Second call hits cache directly.
        assert!(
            mgr.verify_password_async("super-secret-1", &hash1)
                .await
                .unwrap()
        );

        // Wrong password must fail and never be cached.
        assert!(
            !mgr.verify_password_async("wrong-pass", &hash1)
                .await
                .unwrap()
        );
        assert_eq!(mgr.verify_cache().len(), 1);

        // Changed hash must miss cache for old password.
        assert!(!mgr.verify_cache().check_hash(&hash2, "super-secret-1"));
        assert!(
            !mgr.verify_password_async("super-secret-1", &hash2)
                .await
                .unwrap()
        );
        assert!(
            mgr.verify_password_async("super-secret-2", &hash2)
                .await
                .unwrap()
        );
        assert_eq!(mgr.verify_cache().len(), 2);
    }

    #[test]
    fn password_verify_cache_respects_ttl_and_capacity_eviction() {
        let cache = PasswordVerifyCache::new(Duration::from_millis(25), 2);
        let u1 = UserId(Uuid::new_v4());
        let u2 = UserId(Uuid::new_v4());
        let u3 = UserId(Uuid::new_v4());

        cache.record_user_basic(u1, "$argon2id$v=19$hash1", "pw1");
        std::thread::sleep(Duration::from_millis(5));
        cache.record_user_basic(u2, "$argon2id$v=19$hash2", "pw2");
        assert!(cache.check_user_basic(u1, "$argon2id$v=19$hash1", "pw1"));
        assert!(cache.check_user_basic(u2, "$argon2id$v=19$hash2", "pw2"));

        // Third insert evicts oldest entry (u1) due to max_entries=2.
        cache.record_user_basic(u3, "$argon2id$v=19$hash3", "pw3");
        assert_eq!(cache.len(), 2);
        assert!(!cache.check_user_basic(u1, "$argon2id$v=19$hash1", "pw1"));
        assert!(cache.check_user_basic(u3, "$argon2id$v=19$hash3", "pw3"));

        // Wait past TTL -> expired entries miss.
        std::thread::sleep(Duration::from_millis(30));
        assert!(!cache.check_user_basic(u2, "$argon2id$v=19$hash2", "pw2"));
        assert!(!cache.check_user_basic(u3, "$argon2id$v=19$hash3", "pw3"));
    }
}
