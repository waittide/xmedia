use sha2::{Digest, Sha256};
use xmedia_core::domain::error::{DomainError, DomainResult};

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
        if candidate == *expected {
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
            Some(tok) if tok.trim() == expected => Ok(()),
            _ => Err(DomainError::Unauthorized(
                "access password token required".into(),
            )),
        }
    }
}
