use std::fmt;

use lspotify_core::{AppError, CredentialStore, ErrorKind, Result};

const SERVICE_NAME: &str = "lspotify";
const ACCOUNT_NAME: &str = "primary-account-refresh-token";

/// The operating system's native secure credential store. No plaintext fallback is permitted.
pub struct SystemCredentialStore {
    entry: keyring::Entry,
}

impl SystemCredentialStore {
    pub fn open() -> Result<Self> {
        keyring::Entry::new(SERVICE_NAME, ACCOUNT_NAME).map(|entry| Self { entry }).map_err(error)
    }
}
impl fmt::Debug for SystemCredentialStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("SystemCredentialStore").finish_non_exhaustive()
    }
}
impl CredentialStore for SystemCredentialStore {
    fn load_refresh_token(&self) -> Result<Option<String>> {
        match self.entry.get_password() {
            Ok(token) => Ok(Some(token)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(value) => Err(error(value)),
        }
    }
    fn store_refresh_token(&self, token: &str) -> Result<()> {
        if token.is_empty() {
            return Err(AppError::new(ErrorKind::InvalidInput, "refresh token cannot be empty"));
        }
        self.entry.set_password(token).map_err(error)
    }
    fn clear_refresh_token(&self) -> Result<()> {
        match self.entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(value) => Err(error(value)),
        }
    }
}
fn error(value: keyring::Error) -> AppError {
    AppError::new(ErrorKind::Storage, format!("native credential storage is unavailable: {value}"))
}
