use std::fmt;
use std::sync::atomic::Ordering;

use aes_gcm::Aes256Gcm;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::SourceStoreError;

const ENCRYPTED_SECRET_PREFIX: &str = "enc:v1:";

#[derive(Clone)]
pub struct SourceSecretCipher {
    key: [u8; 32],
}

impl fmt::Debug for SourceSecretCipher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SourceSecretCipher([REDACTED])")
    }
}

impl Drop for SourceSecretCipher {
    fn drop(&mut self) {
        for b in &mut self.key {
            // SAFETY: `b` is a valid, aligned mutable byte owned by `self`.
            unsafe {
                std::ptr::write_volatile(b, 0);
            }
        }
        std::sync::atomic::compiler_fence(Ordering::SeqCst);
    }
}

pub(super) enum DecryptedSecret {
    Empty,
    Decrypted(String),
    MigratedPlaintext(String),
    Undecryptable,
}

impl SourceSecretCipher {
    pub fn new(key: [u8; 32]) -> Self {
        Self { key }
    }

    fn aad_for(source_id: &str) -> String {
        format!("xmedia-remote-source:{source_id}")
    }

    fn generate_nonce() -> [u8; 12] {
        let mut sha = Sha256::new();
        sha.update(Uuid::new_v4().as_bytes());
        sha.update(Uuid::new_v4().as_bytes());
        let digest = sha.finalize();
        let mut nonce = [0u8; 12];
        nonce.copy_from_slice(&digest[..12]);
        nonce
    }

    pub fn encrypt(&self, source_id: &str, plain: &str) -> Result<String, SourceStoreError> {
        if plain.is_empty() {
            return Ok(String::new());
        }
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|e| SourceStoreError::Io(format!("Cipher initialization failed: {e}")))?;
        let nonce_bytes = Self::generate_nonce();
        let nonce = nonce_bytes.into();
        let aad = Self::aad_for(source_id);
        let ciphertext = cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plain.as_bytes(),
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|e| SourceStoreError::Io(format!("Secret encryption failed: {e}")))?;
        Ok(format!(
            "{ENCRYPTED_SECRET_PREFIX}{}:{}",
            hex::encode(nonce_bytes),
            hex::encode(ciphertext)
        ))
    }

    pub(super) fn decrypt_or_migrate(&self, source_id: &str, raw: &str) -> DecryptedSecret {
        if raw.is_empty() {
            return DecryptedSecret::Empty;
        }
        let Some(encoded) = raw.strip_prefix(ENCRYPTED_SECRET_PREFIX) else {
            return DecryptedSecret::MigratedPlaintext(raw.to_string());
        };
        let Some((nonce_hex, ct_hex)) = encoded.split_once(':') else {
            return DecryptedSecret::Undecryptable;
        };
        let (Ok(nonce_bytes), Ok(ct_bytes)) = (hex::decode(nonce_hex), hex::decode(ct_hex)) else {
            return DecryptedSecret::Undecryptable;
        };
        if nonce_bytes.len() != 12 {
            return DecryptedSecret::Undecryptable;
        }
        let Ok(cipher) = Aes256Gcm::new_from_slice(&self.key) else {
            return DecryptedSecret::Undecryptable;
        };
        let Ok(nonce_arr): Result<[u8; 12], _> = nonce_bytes.as_slice().try_into() else {
            return DecryptedSecret::Undecryptable;
        };
        let nonce = nonce_arr.into();
        let aad = Self::aad_for(source_id);
        match cipher.decrypt(
            &nonce,
            Payload {
                msg: ct_bytes.as_slice(),
                aad: aad.as_bytes(),
            },
        ) {
            Ok(plain_bytes) => match String::from_utf8(plain_bytes) {
                Ok(plain) => DecryptedSecret::Decrypted(plain),
                Err(_) => DecryptedSecret::Undecryptable,
            },
            Err(_) => DecryptedSecret::Undecryptable,
        }
    }
}
