use std::fmt;

use mellowdeck_core::{AppError, CredentialStore, ErrorKind, Result};

const SERVICE_NAME: &str = "Mellowdeck Spotify";
const ACCOUNT_NAME: &str = "primary-account-refresh-token";

pub struct WindowsCredentialStore {
    entry: keyring::Entry,
}

impl WindowsCredentialStore {
    /// Opens Mellowdeck's single-account entry in Windows Credential Manager.
    ///
    /// # Errors
    ///
    /// Returns an error when Windows Credential Manager is unavailable or rejects the entry name.
    pub fn open() -> Result<Self> {
        let entry = keyring::Entry::new(SERVICE_NAME, ACCOUNT_NAME).map_err(credential_error)?;
        Ok(Self { entry })
    }
}

impl fmt::Debug for WindowsCredentialStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("WindowsCredentialStore").finish_non_exhaustive()
    }
}

impl CredentialStore for WindowsCredentialStore {
    fn load_refresh_token(&self) -> Result<Option<String>> {
        match self.entry.get_password() {
            Ok(token) => Ok(Some(token)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(credential_error(error)),
        }
    }

    fn store_refresh_token(&self, refresh_token: &str) -> Result<()> {
        if refresh_token.is_empty() {
            return Err(AppError::new(ErrorKind::InvalidInput, "refresh token cannot be empty"));
        }
        self.entry.set_password(refresh_token).map_err(credential_error)
    }

    fn clear_refresh_token(&self) -> Result<()> {
        match self.entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(credential_error(error)),
        }
    }
}

fn credential_error(error: keyring::Error) -> AppError {
    let message = error.to_string();
    drop(error);
    AppError::new(ErrorKind::Storage, format!("Windows credential operation failed: {message}"))
}
