//! Placeholders in action texts (speech, HTTP URL, headers and body): `{control}`, `{state}`,
//! `{profile}` and `{value}`.
//!
//! The values come from the [`ActionRequest`](crate::ActionRequest) that runs the action.
//! `{value}` is the value of the control that emitted the action (e.g. a counter); controls
//! without a value leave it empty. Unknown placeholders stay as they are.

/// The values the placeholders stand for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Placeholders {
    pub control: String,
    pub state: String,
    pub profile: String,
    /// The emitting control's value, formatted, if it has one.
    pub value: Option<String>,
}

/// Placeholder names, for the editor's hint.
pub const NAMES: [&str; 4] = ["{control}", "{state}", "{profile}", "{value}"];

impl Placeholders {
    /// `text` with the placeholders replaced.
    pub fn fill(&self, text: &str) -> String {
        self.fill_with(text, |s| s.to_owned())
    }

    /// For URLs: the inserted values are percent-encoded, the template itself is kept.
    pub fn fill_url(&self, url: &str) -> String {
        self.fill_with(url, percent_encode)
    }

    fn fill_with(&self, text: &str, encode: impl Fn(&str) -> String) -> String {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(start) = rest.find('{') {
            out.push_str(&rest[..start]);
            let after = &rest[start..];
            let value = match after.find('}').map(|end| &after[..=end]) {
                Some("{control}") => Some(self.control.as_str()),
                Some("{state}") => Some(self.state.as_str()),
                Some("{profile}") => Some(self.profile.as_str()),
                Some("{value}") => Some(self.value.as_deref().unwrap_or("")),
                _ => None,
            };
            match value {
                Some(v) => {
                    out.push_str(&encode(v));
                    rest = &after[after.find('}').unwrap_or(0) + 1..];
                }
                None => {
                    out.push('{');
                    rest = &after[1..];
                }
            }
        }
        out.push_str(rest);
        out
    }
}

/// RFC 3986: everything but unreserved characters is encoded.
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p() -> Placeholders {
        Placeholders {
            control: "Comms".into(),
            state: "Guard 1".into(),
            profile: "DCS".into(),
            value: Some("7".into()),
        }
    }

    #[test]
    fn fills_known_placeholders_and_keeps_others() {
        assert_eq!(
            p().fill("{control}: {state} on {profile} = {value} {unknown} {"),
            "Comms: Guard 1 on DCS = 7 {unknown} {"
        );
        let no_value = Placeholders { value: None, ..p() };
        assert_eq!(no_value.fill("[{value}]"), "[]");
        assert_eq!(p().fill("{{state}}"), "{Guard 1}");
    }

    #[test]
    fn url_values_are_encoded() {
        assert_eq!(
            p().fill_url("https://x.test/hook?s={state}&p={profile}"),
            "https://x.test/hook?s=Guard%201&p=DCS"
        );
        assert_eq!(percent_encode("a/b&c=ä"), "a%2Fb%26c%3D%C3%A4");
    }
}
