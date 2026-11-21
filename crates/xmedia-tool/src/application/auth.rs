use sha2::{Digest, Sha256};
use xmedia_core::domain::error::{DomainError, DomainResult};

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

#[derive(Clone)]
pub struct ToolAuthService {
    expected_token: Option<String>,
}

impl ToolAuthService {
    pub fn new(password: Option<String>) -> Self {
        let clean = password
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty());
        let expected_token = clean.map(|pwd| {
            let mut hasher = Sha256::new();
            hasher.update(b"xmedia-tool-auth-v1:");
            hasher.update(pwd.as_bytes());
            hex::encode(hasher.finalize())
        });
        Self { expected_token }
    }

    pub fn requires_password(&self) -> bool {
        self.expected_token.is_some()
    }

    pub fn unlock(&self, password: &str) -> DomainResult<String> {
        let Some(ref expected) = self.expected_token else {
            return Ok("open-mode".to_string());
        };
        let mut hasher = Sha256::new();
        hasher.update(b"xmedia-tool-auth-v1:");
        hasher.update(password.trim().as_bytes());
        let candidate = hex::encode(hasher.finalize());
        if constant_time_eq(candidate.as_bytes(), expected.as_bytes()) {
            Ok(expected.clone())
        } else {
            Err(DomainError::Unauthorized("invalid access password".into()))
        }
    }

    pub fn verify_token(&self, provided: Option<&str>) -> DomainResult<()> {
        let Some(ref expected) = self.expected_token else {
            return Ok(());
        };
        match provided {
            Some(tok) if constant_time_eq(tok.trim().as_bytes(), expected.as_bytes()) => Ok(()),
            _ => Err(DomainError::Unauthorized(
                "access password token required".into(),
            )),
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
    fn auth_service_unlock_and_verify_token_work_as_expected() {
        let auth = ToolAuthService::new(Some("secret-pass".into()));
        assert!(auth.requires_password());
        assert!(auth.unlock("wrong").is_err());
        let token = auth.unlock("secret-pass").unwrap();
        assert!(auth.verify_token(Some(&token)).is_ok());
        assert!(auth.verify_token(Some("bad-token")).is_err());
        assert!(auth.verify_token(None).is_err());
    }
}
