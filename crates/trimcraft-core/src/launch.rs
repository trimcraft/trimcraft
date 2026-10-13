//! Launch actions: start a program, open a web page, open a file or folder with its default
//! app. The app starts programs with `CreateProcess` (never through a shell) and opens URLs
//! and files with `ShellExecuteW`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum LaunchTarget {
    Program {
        /// Full path of the executable.
        path: String,
        /// The command line after the program name, passed on unchanged (Windows programs
        /// parse their own command line), e.g. `--profile "My Game"`.
        #[serde(default)]
        arguments: String,
        /// Empty: the program's own folder.
        #[serde(default)]
        working_dir: String,
        /// Skip when a process with the same executable name is running.
        #[serde(default)]
        only_if_not_running: bool,
    },
    /// A web page, http or https only.
    Url { url: String },
    /// A file or folder, opened with its default app.
    Open { path: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Launch {
    pub target: LaunchTarget,
    /// The action queue waits until the program has exited (programs only; URLs and files
    /// return once they are handed to their app).
    #[serde(default)]
    pub wait: bool,
}

/// Whether `url` is an absolute http or https URL with a host.
pub fn web_url(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"));
    rest.is_some_and(|r| {
        let host = r.split(['/', '?', '#']).next().unwrap_or("");
        !host.is_empty() && !host.contains(char::is_whitespace)
    })
}

/// The executable's file name, compared case-insensitively for "only if not running".
pub fn exe_name(path: &str) -> &str {
    crate::audio::file_name(path.trim().trim_matches('"'))
}

impl LaunchTarget {
    pub fn describe(&self) -> String {
        match self {
            LaunchTarget::Program {
                path, arguments, ..
            } => {
                if arguments.is_empty() {
                    format!("start {}", exe_name(path))
                } else {
                    format!(
                        "start {} {}",
                        exe_name(path),
                        crate::audio::shorten(arguments, 40)
                    )
                }
            }
            LaunchTarget::Url { url } => format!("open {}", crate::webhook::redact_url(url)),
            LaunchTarget::Open { path } => format!("open {path}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_web_urls() {
        assert!(web_url("https://example.com"));
        assert!(web_url("HTTP://example.com/a?b=c"));
        assert!(!web_url("https://"));
        assert!(!web_url("file:///C:/Windows/notepad.exe"));
        assert!(!web_url("javascript:alert(1)"));
        assert!(!web_url("ms-settings:sound"));
        assert!(!web_url("example.com"));
    }

    #[test]
    fn program_names() {
        assert_eq!(exe_name(r#""C:\Games\DCS\bin\DCS.exe""#), "DCS.exe");
        let t: LaunchTarget =
            serde_json::from_str(r#"{"type":"program","path":"C:\\a\\b.exe"}"#).unwrap();
        assert_eq!(t.describe(), "start b.exe");
        let LaunchTarget::Program {
            only_if_not_running,
            working_dir,
            ..
        } = t
        else {
            panic!()
        };
        assert!(!only_if_not_running && working_dir.is_empty());
    }
}
