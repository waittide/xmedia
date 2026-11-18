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

pub struct SecurityManager {
    cipher_key: [u8; 32],
}

impl SecurityManager {
    pub fn new(server_secret: &str) -> Self {
        let mut sha = Sha256::new();
        sha.update(b"xmedia-subsonic-secret-v1:");
        sha.update(server_secret.as_bytes());
        let digest = sha.finalize();
        let mut cipher_key = [0u8; 32];
        cipher_key.copy_from_slice(&digest);
        Self { cipher_key }
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
        Argon2::default()
            .hash_password(plain.as_bytes())
            .map(|h| h.to_string())
            .map_err(|e| DomainError::Persistence(format!("password hashing failed: {e}")))
    }

    fn verify_password(&self, plain: &str, hash: &str) -> DomainResult<bool> {
        let parsed = PasswordHash::new(hash)
            .map_err(|e| DomainError::Persistence(format!("invalid password hash: {e}")))?;
        Ok(Argon2::default()
            .verify_password(plain.as_bytes(), &parsed)
            .is_ok())
    }

    fn encrypt_subsonic_secret(&self, plain: &str) -> DomainResult<String> {
        let cipher = Aes256Gcm::new_from_slice(&self.cipher_key)
            .map_err(|e| DomainError::Persistence(format!("cipher init error: {e}")))?;
        let random_uuid = Uuid::new_v4();
        let nonce_slice = &random_uuid.as_bytes()[..12];
        let nonce = nonce_slice
            .try_into()
            .expect("12-byte slice must fit AES-GCM 96-bit nonce");
        let ciphertext = cipher
            .encrypt(&nonce, plain.as_bytes())
            .map_err(|e| DomainError::Persistence(format!("encryption failed: {e}")))?;
        Ok(format!(
            "{}:{}",
            hex::encode(nonce_slice),
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
        let payload = JwtPayload {
            sub: claims.user_id.0.to_string(),
            username: claims.username.clone(),
            role: claims.role.as_str().to_string(),
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
        })
    }

    fn ttl_seconds(&self) -> u64 {
        self.ttl_seconds
    }
}
