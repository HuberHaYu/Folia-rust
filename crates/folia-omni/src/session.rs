// Platform sessions kept in the system keyring only, one entry per provider; every call blocks.

use crate::OmniError;

fn entry(service: &str, provider: &str) -> Result<keyring::Entry, OmniError> {
    Ok(keyring::Entry::new(service, &format!("{provider}-session"))?)
}

pub(crate) fn load(service: &str, provider: &str) -> Result<Option<String>, OmniError> {
    match entry(service, provider)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

pub(crate) fn save(service: &str, provider: &str, value: &str) -> Result<(), OmniError> {
    entry(service, provider)?.set_password(value)?;
    tracing::info!(provider, "session kept in the system keyring");
    Ok(())
}

pub(crate) fn delete(service: &str, provider: &str) -> Result<(), OmniError> {
    match entry(service, provider)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(err.into()),
    }
}
