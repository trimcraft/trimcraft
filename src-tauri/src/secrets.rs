//! Secrets in the Windows Credential Manager ("generic credentials", visible under Control
//! Panel, Credential Manager, Windows Credentials). Settings only hold a `SecretRef` naming the
//! entry.
//!
//! Values never go to the log; errors only name the entry.

use keyring::Entry;

const SERVICE: &str = "trimcraft";

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

/// The stored bytes as they are, `None` when there is no entry. For copying an entry without
/// depending on how its value was encoded.
pub fn get_raw(target: &str) -> Result<Option<Vec<u8>>, String> {
    match entry(target)?.get_secret() {
        Ok(s) => Ok(Some(s)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("credential store: cannot read {target}: {e}")),
    }
}

pub fn set_raw(target: &str, value: &[u8]) -> Result<(), String> {
    entry(target)?
        .set_secret(value)
        .map_err(|e| format!("credential store: cannot write {target}: {e}"))
}

/// Target names of this user's generic credentials that start with `prefix`.
pub fn list(prefix: &str) -> Result<Vec<String>, String> {
    use windows_sys::Win32::Foundation::{ERROR_NOT_FOUND, GetLastError};
    use windows_sys::Win32::Security::Credentials::{
        CRED_TYPE_GENERIC, CREDENTIALW, CredEnumerateW, CredFree,
    };
    let filter = crate::elevation::wide(format!("{prefix}*"));
    let mut count = 0u32;
    let mut list: *mut *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: NUL-terminated filter; on success `list` holds `count` entries until CredFree.
    if unsafe { CredEnumerateW(filter.as_ptr(), 0, &mut count, &mut list) } == 0 {
        // SAFETY: no preconditions.
        let error = unsafe { GetLastError() };
        return if error == ERROR_NOT_FOUND {
            Ok(Vec::new())
        } else {
            Err(format!(
                "credential store: cannot list {prefix}* (error {error})"
            ))
        };
    }
    let mut targets = Vec::new();
    for i in 0..count as usize {
        // SAFETY: `list` has `count` valid pointers to credentials, each with a NUL-terminated
        // target name.
        unsafe {
            let cred = &**list.add(i);
            if cred.Type == CRED_TYPE_GENERIC && !cred.TargetName.is_null() {
                let len = (0..).take_while(|&j| *cred.TargetName.add(j) != 0).count();
                let name = std::slice::from_raw_parts(cred.TargetName, len);
                targets.push(String::from_utf16_lossy(name));
            }
        }
    }
    // SAFETY: allocated by CredEnumerateW above.
    unsafe { CredFree(list.cast()) };
    Ok(targets)
}
