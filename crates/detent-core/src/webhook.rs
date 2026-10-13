//! HTTP request actions (webhooks): method, URL, headers and body, with placeholders.
//!
//! A header value can be secret: it then lives in the operating system's credential store and
//! the setup only holds a [`SecretRef`]. Secret values never appear in descriptions or logs.

use crate::connection::SecretRef;
use crate::placeholder::Placeholders;
use serde::{Deserialize, Serialize};

pub const DEFAULT_TIMEOUT_MS: u32 = 5_000;
pub const MIN_TIMEOUT_MS: u32 = 100;
pub const MAX_TIMEOUT_MS: u32 = 60_000;
/// Credential store entries for secret header values start with this.
pub const SECRET_PREFIX: &str = "detent/http/";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    #[default]
    Post,
    Put,
    Patch,
    Delete,
}

impl HttpMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Patch => "PATCH",
            HttpMethod::Delete => "DELETE",
        }
    }

    /// Whether a body is sent.
    pub fn has_body(self) -> bool {
        !matches!(self, HttpMethod::Get | HttpMethod::Delete)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpHeader {
    pub name: String,
    /// Empty when the value is secret.
    #[serde(default)]
    pub value: String,
    /// Where the secret value is stored; `None` for a plain value.
    #[serde(default)]
    pub secret: Option<SecretRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HttpRequest {
    pub method: HttpMethod,
    /// May contain placeholders; their values are URL-encoded.
    pub url: String,
    pub headers: Vec<HttpHeader>,
    /// May contain placeholders. Not sent for GET and DELETE.
    pub body: String,
    pub timeout_ms: u32,
    /// The action queue waits for the response.
    pub wait: bool,
}

impl Default for HttpRequest {
    fn default() -> Self {
        Self {
            method: HttpMethod::Post,
            url: String::new(),
            headers: Vec::new(),
            body: String::new(),
            timeout_ms: DEFAULT_TIMEOUT_MS,
            wait: false,
        }
    }
}

/// A request ready to send: placeholders filled, secrets still to be looked up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    pub method: HttpMethod,
    pub url: String,
    /// Name and either a plain value or the secret to look up.
    pub headers: Vec<(String, HeaderValue)>,
    pub body: Option<String>,
    pub timeout_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderValue {
    Plain(String),
    Secret(SecretRef),
}

impl HttpRequest {
    /// Fills the placeholders (not in secret values).
    pub fn prepare(&self, p: &Placeholders) -> Prepared {
        Prepared {
            method: self.method,
            url: p.fill_url(self.url.trim()),
            headers: self
                .headers
                .iter()
                .map(|h| {
                    let value = match &h.secret {
                        Some(secret) => HeaderValue::Secret(secret.clone()),
                        None => HeaderValue::Plain(p.fill(&h.value)),
                    };
                    (h.name.trim().to_owned(), value)
                })
                .collect(),
            body: self.method.has_body().then(|| p.fill(&self.body)),
            timeout_ms: self.timeout_ms,
        }
    }

    pub fn describe(&self) -> String {
        format!("{} {}", self.method.as_str(), redact_url(&self.url))
    }
}

/// A header name: an HTTP token (RFC 9110).
pub fn valid_header_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

/// A header value: no line breaks or other control characters.
pub fn valid_header_value(value: &str) -> bool {
    !value.chars().any(|c| c.is_control() && c != '\t')
}

/// The URL without query and user info, for logs (tokens often sit there).
pub fn redact_url(url: &str) -> String {
    let url = url.trim();
    let base = url.split(['?', '#']).next().unwrap_or(url);
    let mut redacted = match base.split_once("://") {
        Some((scheme, rest)) => {
            let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
            match authority.rsplit_once('@') {
                Some((_, host)) => format!("{scheme}://…@{host}{path}"),
                None => base.to_owned(),
            }
        }
        None => base.to_owned(),
    };
    if url[base.len()..].starts_with('?') {
        redacted.push_str("?…");
    }
    redacted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p() -> Placeholders {
        Placeholders {
            control: "Comms".into(),
            state: "Guard 1".into(),
            profile: "DCS".into(),
            value: None,
        }
    }

    #[test]
    fn prepare_fills_placeholders_but_not_secrets() {
        let secret = SecretRef::WindowsCredentialManager {
            target: format!("{SECRET_PREFIX}1"),
        };
        let r = HttpRequest {
            method: HttpMethod::Post,
            url: "https://hooks.test/{profile}?s={state}".into(),
            headers: vec![
                HttpHeader {
                    name: " X-State ".into(),
                    value: "{state}".into(),
                    secret: None,
                },
                HttpHeader {
                    name: "Authorization".into(),
                    value: String::new(),
                    secret: Some(secret.clone()),
                },
            ],
            body: r#"{"text":"{control}: {state}"}"#.into(),
            ..HttpRequest::default()
        };
        let prepared = r.prepare(&p());
        assert_eq!(prepared.url, "https://hooks.test/DCS?s=Guard%201");
        assert_eq!(
            prepared.headers[0],
            ("X-State".into(), HeaderValue::Plain("Guard 1".into()))
        );
        assert_eq!(prepared.headers[1].1, HeaderValue::Secret(secret));
        assert_eq!(
            prepared.body.as_deref(),
            Some(r#"{"text":"Comms: Guard 1"}"#)
        );
        let get = HttpRequest {
            method: HttpMethod::Get,
            ..r
        };
        assert_eq!(get.prepare(&p()).body, None);
    }

    #[test]
    fn descriptions_hide_queries_and_credentials() {
        assert_eq!(
            redact_url("https://hooks.test/abc?token=secret"),
            "https://hooks.test/abc?…"
        );
        assert_eq!(
            redact_url("https://user:pw@hooks.test/abc"),
            "https://…@hooks.test/abc"
        );
        assert_eq!(
            redact_url("https://hooks.test/a@b"),
            "https://hooks.test/a@b"
        );
        let r = HttpRequest {
            url: "https://h.test/x?k=1".into(),
            ..HttpRequest::default()
        };
        assert_eq!(r.describe(), "POST https://h.test/x?…");
    }

    #[test]
    fn header_checks() {
        assert!(valid_header_name("X-Api-Key"));
        assert!(!valid_header_name("Bad Name"));
        assert!(!valid_header_name(""));
        assert!(valid_header_value("Bearer abc"));
        assert!(!valid_header_value("a\r\nX-Evil: 1"));
    }

    #[test]
    fn serialises_for_the_ui() {
        let r: HttpRequest =
            serde_json::from_str(r#"{"method":"GET","url":"https://x.test"}"#).unwrap();
        assert_eq!(r.timeout_ms, DEFAULT_TIMEOUT_MS);
        assert_eq!(r.method, HttpMethod::Get);
        assert!(!r.wait);
    }
}
