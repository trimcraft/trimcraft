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
    let agent = agent(Duration::from_millis(u64::from(request.timeout_ms)));
    let mut call = ureq::http::Request::builder()
        .method(request.method.as_str())
        .uri(&request.url);
    for (name, value) in &request.headers {
        let value = match value {
            HeaderValue::Plain(v) => v.clone(),
            HeaderValue::Secret(secret) => secret_value(name, secret)?,
        };
        call = call.header(name, value);
    }
    let result = match &request.body {
        Some(body) => call.body(body.as_str()).map(|r| agent.run(r)),
        None => call.body(()).map(|r| agent.run(r)),
    }
    .map_err(|e| format!("{} (invalid request: {e})", redact_url(&request.url)))?;
    match result {
        Ok(response) => {
            let status = response.status();
            if status.is_client_error() || status.is_server_error() {
                Err(format!(
                    "HTTP {} {}",
                    status.as_u16(),
                    status.canonical_reason().unwrap_or_default()
                ))
            } else {
                Ok(format!("HTTP {}", status.as_u16()))
            }
        }
        Err(e) => Err(format!("{} ({})", redact_url(&request.url), error_text(&e))),
    }
}

/// An agent that returns 4xx and 5xx responses instead of turning them into errors.
pub(crate) fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .build()
        .into()
}

/// Describes a failed request without the URL: some ureq errors repeat it, query included.
pub(crate) fn error_text(error: &ureq::Error) -> String {
    match error {
        ureq::Error::BadUri(_) => "invalid URL".into(),
        ureq::Error::RequireHttpsOnly(_) => "HTTPS required".into(),
        other => other.to_string(),
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
