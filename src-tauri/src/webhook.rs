//! "HTTP request" actions (webhooks), sent with ureq over HTTPS (rustls).
//!
//! Secret header values are read from the Credential Manager right before sending and never
//! logged. Error texts name the host and path, never the query (tokens often sit there).

use crate::secrets;
use std::time::Duration;
use trimcraft_core::connection::SecretRef;
use trimcraft_core::webhook::{HeaderValue, Prepared, SECRET_PREFIX, redact_url};

/// Sends the request. Returns the outcome for the event log, e.g. "HTTP 204"; a 4xx or 5xx
/// status is an error.
pub fn send(request: &Prepared) -> Result<String, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_millis(u64::from(request.timeout_ms)))
        .build();
    let mut call = agent.request(request.method.as_str(), &request.url);
    for (name, value) in &request.headers {
        let value = match value {
            HeaderValue::Plain(v) => v.clone(),
            HeaderValue::Secret(secret) => secret_value(name, secret)?,
        };
        call = call.set(name, &value);
    }
    let result = match &request.body {
        Some(body) => call.send_string(body),
        None => call.call(),
    };
    match result {
        Ok(response) => Ok(format!("HTTP {}", response.status())),
        Err(ureq::Error::Status(code, response)) => {
            Err(format!("HTTP {code} {}", response.status_text()))
        }
        Err(ureq::Error::Transport(t)) => Err(format!(
            "{} ({}{})",
            redact_url(&request.url),
            t.kind(),
            t.message().map(|m| format!(": {m}")).unwrap_or_default()
        )),
    }
}

fn secret_value(name: &str, secret: &SecretRef) -> Result<String, String> {
    let SecretRef::WindowsCredentialManager { target } = secret;
    match secrets::get(target)? {
        Some(value) => Ok(value),
        None => Err(format!(
            "the secret value of header {name} is missing from the Credential Manager; enter it again"
        )),
    }
}

/// Stores a secret header value and returns where it lives. Every value gets an entry of its
/// own, so editing one action never changes another.
pub fn store_secret(value: &str) -> Result<SecretRef, String> {
    if value.is_empty() {
        return Err("enter the secret value".into());
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let target = format!("{SECRET_PREFIX}{nanos:x}");
    secrets::set(&target, value)?;
    Ok(SecretRef::WindowsCredentialManager { target })
}

/// Removes a secret header value that is no longer used.
pub fn delete_secret(secret: &SecretRef) -> Result<(), String> {
    let SecretRef::WindowsCredentialManager { target } = secret;
    if !target.starts_with(SECRET_PREFIX) {
        return Err("not an HTTP header secret".into());
    }
    secrets::delete(target)
}
