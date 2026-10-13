//! Secrets in the Windows Credential Manager ("generic credentials", visible under Control
//! Panel, Credential Manager, Windows Credentials). Settings only hold a `SecretRef` naming the
//! entry.
//!
//! Values never go to the log; errors only name the entry.

use keyring::Entry;

const SERVICE: &str = "detent";

fn entry(target: &str) -> Result<Entry, String> {
    Entry::new_with_target(target, SERVICE, target)
        .map_err(|e| format!("credential store: cannot open {target}: {e}"))
}

/// The secret, or `None` when there is no entry.
pub fn get(target: &str) -> Result<Option<String>, String> {
    match entry(target)?.get_password() {
        Ok(s) => Ok(Some(s)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("credential store: cannot read {target}: {e}")),
    }
}

pub fn set(target: &str, value: &str) -> Result<(), String> {
    entry(target)?
        .set_password(value)
        .map_err(|e| format!("credential store: cannot write {target}: {e}"))
}

/// Removes the entry; a missing entry is fine.
pub fn delete(target: &str) -> Result<(), String> {
    match entry(target)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("credential store: cannot delete {target}: {e}")),
    }
}

pub fn exists(target: &str) -> bool {
    matches!(get(target), Ok(Some(_)))
}
