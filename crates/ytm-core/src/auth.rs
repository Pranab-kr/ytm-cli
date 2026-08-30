//! Token persistence. Tokens go to the OS keyring, never to a file (NFR-6).

use std::sync::Mutex;

const SERVICE: &str = "ytm-cli";
const USER: &str = "default";
/// Treat a token as expired this many seconds early, so a request never races expiry.
const EXPIRY_HEADROOM_SECS: i64 = 30;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredToken {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds.
    pub expires_at: i64,
    pub token_type: String,
}

/// Hand-written so token values can never reach a log line.
impl std::fmt::Debug for StoredToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredToken")
            .field("access_token", &"<redacted>")
            .field("refresh_token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .field("token_type", &self.token_type)
            .finish()
    }
}

impl StoredToken {
    pub fn is_expired_at(&self, now_unix: i64) -> bool {
        self.expires_at - EXPIRY_HEADROOM_SECS <= now_unix
    }
    pub fn is_expired(&self) -> bool {
        self.is_expired_at(chrono::Utc::now().timestamp())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TokenStoreError {
    #[error("could not reach the OS keyring: {0}")]
    Keyring(String),
    #[error("stored credentials are corrupt — run `ytm login` again")]
    Corrupt,
}

pub trait TokenStore: Send + Sync {
    fn load(&self) -> Result<Option<StoredToken>, TokenStoreError>;
    fn save(&self, t: &StoredToken) -> Result<(), TokenStoreError>;
    /// Idempotent: clearing when nothing is stored succeeds.
    fn clear(&self) -> Result<(), TokenStoreError>;
}

pub struct KeyringStore {
    service: String,
}

impl KeyringStore {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }
    pub fn default_store() -> Self {
        Self::new(SERVICE)
    }

    fn entry(&self) -> Result<keyring::Entry, TokenStoreError> {
        keyring::Entry::new(&self.service, USER)
            .map_err(|e| TokenStoreError::Keyring(e.to_string()))
    }
}

impl TokenStore for KeyringStore {
    fn load(&self) -> Result<Option<StoredToken>, TokenStoreError> {
        match self.entry()?.get_password() {
            Ok(json) => serde_json::from_str(&json)
                .map(Some)
                .map_err(|_| TokenStoreError::Corrupt),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(TokenStoreError::Keyring(e.to_string())),
        }
    }

    fn save(&self, t: &StoredToken) -> Result<(), TokenStoreError> {
        let json = serde_json::to_string(t).map_err(|_| TokenStoreError::Corrupt)?;
        self.entry()?
            .set_password(&json)
            .map_err(|e| TokenStoreError::Keyring(e.to_string()))
    }

    fn clear(&self) -> Result<(), TokenStoreError> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(TokenStoreError::Keyring(e.to_string())),
        }
    }
}

#[derive(Default)]
pub struct MemoryStore {
    inner: Mutex<Option<StoredToken>>,
}

impl TokenStore for MemoryStore {
    fn load(&self) -> Result<Option<StoredToken>, TokenStoreError> {
        Ok(self.inner.lock().unwrap().clone())
    }
    fn save(&self, t: &StoredToken) -> Result<(), TokenStoreError> {
        *self.inner.lock().unwrap() = Some(t.clone());
        Ok(())
    }
    fn clear(&self) -> Result<(), TokenStoreError> {
        *self.inner.lock().unwrap() = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> StoredToken {
        StoredToken {
            access_token: "at".into(),
            refresh_token: "rt".into(),
            expires_at: 1_800_000_000,
            token_type: "Bearer".into(),
        }
    }

    #[test]
    fn memory_store_round_trips() {
        let s = MemoryStore::default();
        assert!(s.load().unwrap().is_none());
        s.save(&sample()).unwrap();
        assert_eq!(s.load().unwrap().unwrap().access_token, "at");
        s.clear().unwrap();
        assert!(s.load().unwrap().is_none());
    }

    #[test]
    fn clearing_an_absent_token_is_not_an_error() {
        // `ytm logout` must succeed even when already logged out (FR-A4).
        let s = MemoryStore::default();
        assert!(s.clear().is_ok());
    }

    #[test]
    fn token_is_expired_within_the_safety_window() {
        let now = 1_000_000;
        // 30s of headroom: a token expiring in 10s counts as expired.
        let t = StoredToken {
            expires_at: now + 10,
            ..sample()
        };
        assert!(t.is_expired_at(now));
        let t = StoredToken {
            expires_at: now + 600,
            ..sample()
        };
        assert!(!t.is_expired_at(now));
    }

    #[test]
    fn debug_impl_does_not_leak_token_values() {
        // NFR-6: secrets must never reach a log line. The secret values here are
        // deliberately distinctive — "at"/"rt" would also match the field name
        // `expires_at`, so a passing assertion would prove nothing.
        let t = StoredToken {
            access_token: "ACCESS-SECRET".into(),
            refresh_token: "REFRESH-SECRET".into(),
            ..sample()
        };
        let d = format!("{t:?}");
        assert!(
            !d.contains("ACCESS-SECRET"),
            "access token leaked into Debug output: {d}"
        );
        assert!(
            !d.contains("REFRESH-SECRET"),
            "refresh token leaked into Debug output: {d}"
        );
        assert!(d.contains("<redacted>"), "expected redaction markers: {d}");
    }

    #[test]
    #[ignore = "requires an OS keyring; run manually with --ignored"]
    fn keyring_store_round_trips() {
        let s = KeyringStore::new("ytm-cli-test");
        s.save(&sample()).unwrap();
        assert_eq!(s.load().unwrap().unwrap().refresh_token, "rt");
        s.clear().unwrap();
    }
}
