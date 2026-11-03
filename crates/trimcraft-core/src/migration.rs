//! Taking over the folders of an earlier installation on the first start, plus a Detent
//! installation's (Trimcraft's former name) Credential Manager entries, secret references and
//! autostart. Only the decisions live here; `src-tauri/src/migration.rs` does the file system,
//! credential and registry work.
//!
//! Folders come from the newest earlier location that has something: Trimcraft up to 0.1.3
//! (`cloud.breu.trimcraft`), then Detent (`cloud.breu.detent`). A machine that only has Detent
//! moves straight to the current folders.
//!
//! Every step is idempotent: the old folders are copied, never moved, and get a note file
//! afterwards; a credential is deleted only after its copy was written and read back.

use serde_json::Value;

/// Tauri identifier since 0.1.4; its folders are `%APPDATA%\io.github.trimcraft.trimcraft` and
/// `%LOCALAPPDATA%\io.github.trimcraft.trimcraft`. Must match `tauri.conf.json` (a test in the
/// app crate checks it).
pub const IDENTIFIER: &str = "io.github.trimcraft.trimcraft";
/// Tauri identifier of Trimcraft up to 0.1.3.
pub const OLD_TRIMCRAFT_IDENTIFIER: &str = "cloud.breu.trimcraft";
/// Tauri identifier of Detent; its folders are `%APPDATA%\cloud.breu.detent` and
/// `%LOCALAPPDATA%\cloud.breu.detent`.
pub const OLD_IDENTIFIER: &str = "cloud.breu.detent";
/// Prefix of Detent's Credential Manager entries (`detent/discord/client-secret`, ...).
pub const OLD_SECRET_PREFIX: &str = "detent/";
pub const SECRET_PREFIX: &str = "trimcraft/";
/// Name of Detent's per-user `Run` registry value.
pub const OLD_RUN_VALUE: &str = "Detent";
/// Left in a Detent folder once its contents were copied. Its presence ends the folder's
/// migration for good.
pub const NOTE_FILE: &str = "MIGRATED_TO_TRIMCRAFT.txt";
/// Left in a `cloud.breu.trimcraft` folder once its contents were copied.
pub const MOVED_NOTE_FILE: &str = "MOVED_TO_io.github.trimcraft.trimcraft.txt";
/// The file whose presence in the new config folder means Trimcraft has a setup of its own.
pub const SETUP_FILE: &str = "setup.json";
/// Store name of credential references in the setup (`SecretRef::store`).
const CREDENTIAL_STORE: &str = "windowsCredentialManager";

/// Detent's elevated autostart task for a Windows user.
pub fn old_task_name(user: &str) -> String {
    format!("Detent autostart {user}")
}

// --- Folders ---------------------------------------------------------------------------------

/// An earlier location of Trimcraft's folders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Source {
    /// The Tauri identifier, the folder name below `%APPDATA%` and `%LOCALAPPDATA%`.
    pub identifier: &'static str,
    /// For log messages.
    pub name: &'static str,
    /// Written into the old folder once it was copied.
    pub note_file: &'static str,
}

/// Earlier locations, newest first. The newest one that has something decides.
pub const SOURCES: [Source; 2] = [
    Source {
        identifier: OLD_TRIMCRAFT_IDENTIFIER,
        name: "Trimcraft 0.1.3 or older",
        note_file: MOVED_NOTE_FILE,
    },
    Source {
        identifier: OLD_IDENTIFIER,
        name: "Detent",
        note_file: NOTE_FILE,
    },
];

impl Source {
    pub fn note_text(&self, new_dir: &str) -> String {
        if self.identifier == OLD_IDENTIFIER {
            format!(
                "Detent is now called Trimcraft.\r\n\
                 Trimcraft copied the contents of this folder to {new_dir} on its first start \
                 and uses that copy from now on. This folder is no longer read; you can delete \
                 it once Trimcraft works as expected.\r\n"
            )
        } else {
            format!(
                "Trimcraft keeps its files in {new_dir} since version 0.1.4.\r\n\
                 It copied the contents of this folder there on its first start and uses that \
                 copy from now on. This folder is no longer read; you can delete it once \
                 Trimcraft works as expected.\r\n"
            )
        }
    }
}

/// Which of the two folders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderKind {
    /// `%APPDATA%\<identifier>`: setup, devices, values, control states, backups.
    Config,
    /// `%LOCALAPPDATA%\<identifier>`: logs and the WebView2 profile.
    Local,
}

/// What is known about one earlier folder.
#[derive(Debug, Clone, Default)]
pub struct FolderState {
    pub exists: bool,
    /// Files in the folder, relative, with `/` as separator.
    pub files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderPlan {
    /// Nothing to do, and why.
    Skip(&'static str),
    /// Copy these files from `SOURCES[source]` (relative paths, the same in both folders), in
    /// this order, then write the source's note into the old folder. `skipped` were left out
    /// on purpose.
    Copy {
        source: usize,
        files: Vec<String>,
        skipped: Vec<String>,
    },
}

/// What to copy into a new folder. `sources` holds the state of each of [`SOURCES`], in that
/// order; `new_has_setup` whether the new config folder has a setup.json (only looked at for
/// [`FolderKind::Config`]).
///
/// The newest source with something to take over decides: if it was taken over before (it has
/// its note), nothing older is looked at either. The setup goes last, so a copy that was
/// interrupted is done again at the next start (a config folder counts as taken over once it
/// has a setup).
pub fn plan_folder(kind: FolderKind, sources: &[FolderState], new_has_setup: bool) -> FolderPlan {
    if kind == FolderKind::Config && new_has_setup {
        return FolderPlan::Skip("Trimcraft has a setup of its own");
    }
    let Some((source, state)) = sources
        .iter()
        .enumerate()
        .take(SOURCES.len())
        .find(|(_, s)| s.exists && s.files.iter().any(|f| copies(kind, f)))
    else {
        return FolderPlan::Skip("no earlier folder");
    };
    if state.files.iter().any(|f| f == SOURCES[source].note_file) {
        return FolderPlan::Skip("taken over before");
    }
    let (mut files, mut skipped): (Vec<String>, Vec<String>) =
        state.files.iter().cloned().partition(|f| copies(kind, f));
    files.sort_by_key(|f| (f == SETUP_FILE, f.clone()));
    skipped.sort();
    FolderPlan::Copy {
        source,
        files,
        skipped,
    }
}

fn copies(kind: FolderKind, file: &str) -> bool {
    // Leftovers of an atomic write (storage.rs writes `<name>.tmp`, then renames), and the
    // notes of earlier migrations.
    if file.ends_with(".tmp") || SOURCES.iter().any(|s| s.note_file == file) {
        return false;
    }
    match kind {
        FolderKind::Config => true,
        // The WebView2 profile is mostly cache, locked while the old version runs, and the one
        // value the UI keeps there (the last opened section) is only a convenience; only the
        // logs move.
        FolderKind::Local => file.starts_with("logs/"),
    }
}

// --- Credentials -----------------------------------------------------------------------------

/// The Trimcraft target of a Detent credential, `None` for anything else.
pub fn new_secret_target(old: &str) -> Option<String> {
    old.strip_prefix(OLD_SECRET_PREFIX)
        .filter(|rest| !rest.is_empty())
        .map(|rest| format!("{SECRET_PREFIX}{rest}"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialStep {
    /// Write the secret under `new`, read it back, then delete `old`.
    Copy { old: String, new: String },
    /// `new` exists already (an earlier start stopped after writing it, or it was entered in
    /// Trimcraft): delete `old` if both hold the same secret, otherwise keep both.
    Reconcile { old: String, new: String },
}

/// One step per Detent entry in `old_targets` (as listed by the credential store), given the
/// Trimcraft entries that exist.
pub fn plan_credentials(old_targets: &[String], new_targets: &[String]) -> Vec<CredentialStep> {
    let mut steps: Vec<CredentialStep> = old_targets
        .iter()
        .filter_map(|old| {
            let new = new_secret_target(old)?;
            let old = old.clone();
            Some(if new_targets.contains(&new) {
                CredentialStep::Reconcile { old, new }
            } else {
                CredentialStep::Copy { old, new }
            })
        })
        .collect();
    steps.sort_by(|a, b| a.old().cmp(b.old()));
    steps.dedup();
    steps
}

impl CredentialStep {
    pub fn old(&self) -> &str {
        match self {
            Self::Copy { old, .. } | Self::Reconcile { old, .. } => old,
        }
    }
}

/// Rewrites the credential references in a setup (or any JSON inside it) from Detent's prefix
/// to Trimcraft's. Returns how many were changed. Used when loading setups of schema 4 and
/// older, so setup.json, backups and imported files all point at the migrated entries.
pub fn rename_secret_refs(value: &mut Value) -> usize {
    match value {
        Value::Object(obj) => {
            let mut changed = 0;
            if obj.get("store").and_then(Value::as_str) == Some(CREDENTIAL_STORE)
                && let Some(new) = obj
                    .get("target")
                    .and_then(Value::as_str)
                    .and_then(new_secret_target)
            {
                obj.insert("target".into(), new.into());
                changed += 1;
            }
            changed + obj.values_mut().map(rename_secret_refs).sum::<usize>()
        }
        Value::Array(list) => list.iter_mut().map(rename_secret_refs).sum(),
        _ => 0,
    }
}

// --- Autostart -------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutostartStep {
    RemoveOldRunKey,
    RemoveOldTask,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AutostartMigration {
    pub steps: Vec<AutostartStep>,
    /// Why something stays for now.
    pub note: Option<String>,
}

/// Removes Detent's autostart entries; Trimcraft registers its own from the (taken over)
/// settings as usual. Detent's task was created elevated and can only be removed elevated;
/// with "Always run as administrator" on, Trimcraft restarts elevated and removes it then.
pub fn plan_autostart(run_key: bool, task: bool, elevated: bool) -> AutostartMigration {
    let mut out = AutostartMigration::default();
    if run_key {
        out.steps.push(AutostartStep::RemoveOldRunKey);
    }
    if task {
        if elevated {
            out.steps.push(AutostartStep::RemoveOldTask);
        } else {
            out.note = Some(
                "Detent's elevated autostart task can only be removed while Trimcraft runs as \
                 administrator; it is removed at the next start as administrator"
                    .into(),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn files(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn folder(list: &[&str]) -> FolderState {
        FolderState {
            exists: true,
            files: files(list),
        }
    }

    const NONE: FolderState = FolderState {
        exists: false,
        files: Vec::new(),
    };

    /// Old Trimcraft only, Detent only, both.
    fn only_trimcraft(state: FolderState) -> [FolderState; 2] {
        [state, NONE]
    }

    fn only_detent(state: FolderState) -> [FolderState; 2] {
        [NONE, state]
    }

    #[test]
    fn sources_are_newest_first_and_not_the_current_identifier() {
        assert_eq!(SOURCES[0].identifier, "cloud.breu.trimcraft");
        assert_eq!(SOURCES[1].identifier, "cloud.breu.detent");
        assert!(SOURCES.iter().all(|s| s.identifier != IDENTIFIER));
        assert_eq!(MOVED_NOTE_FILE, format!("MOVED_TO_{IDENTIFIER}.txt"));
        let note = SOURCES[0].note_text(r"C:\x\io.github.trimcraft.trimcraft");
        assert!(note.contains(r"C:\x\io.github.trimcraft.trimcraft"));
        assert!(!note.contains("Detent"));
        assert!(
            SOURCES[1]
                .note_text("d")
                .starts_with("Detent is now called Trimcraft.")
        );
    }

    #[test]
    fn config_folder_is_copied_with_the_setup_last() {
        let plan = plan_folder(
            FolderKind::Config,
            &only_trimcraft(folder(&[
                "setup.json",
                "devices.json",
                "backups/setup-20260101-120000.json",
                "values.json",
                "control-states.json",
                "setup.tmp",
            ])),
            false,
        );
        assert_eq!(
            plan,
            FolderPlan::Copy {
                source: 0,
                files: files(&[
                    "backups/setup-20260101-120000.json",
                    "control-states.json",
                    "devices.json",
                    "values.json",
                    "setup.json",
                ]),
                skipped: files(&["setup.tmp"]),
            }
        );
    }

    #[test]
    fn config_folder_is_left_alone_when_done_or_not_needed() {
        assert_eq!(
            plan_folder(FolderKind::Config, &[NONE, NONE], false),
            FolderPlan::Skip("no earlier folder")
        );
        assert_eq!(
            plan_folder(FolderKind::Config, &[], false),
            FolderPlan::Skip("no earlier folder")
        );
        let done = only_trimcraft(folder(&["setup.json", MOVED_NOTE_FILE]));
        assert_eq!(
            plan_folder(FolderKind::Config, &done, false),
            FolderPlan::Skip("taken over before")
        );
        assert_eq!(
            plan_folder(
                FolderKind::Config,
                &only_trimcraft(folder(&["setup.json"])),
                true
            ),
            FolderPlan::Skip("Trimcraft has a setup of its own")
        );
        assert_eq!(
            plan_folder(FolderKind::Config, &only_trimcraft(folder(&[])), false),
            FolderPlan::Skip("no earlier folder")
        );
    }

    #[test]
    fn an_interrupted_copy_is_repeated() {
        // The first start copied some files but not the setup and wrote no note.
        let plan = plan_folder(
            FolderKind::Config,
            &only_trimcraft(folder(&["devices.json", "setup.json"])),
            false,
        );
        assert!(matches!(plan, FolderPlan::Copy { ref files, .. } if files.len() == 2));
    }

    #[test]
    fn a_detent_only_machine_moves_straight_to_the_new_folder() {
        let plan = plan_folder(
            FolderKind::Config,
            &only_detent(folder(&["setup.json", "devices.json"])),
            false,
        );
        assert_eq!(
            plan,
            FolderPlan::Copy {
                source: 1,
                files: files(&["devices.json", "setup.json"]),
                skipped: vec![],
            }
        );
        let done = only_detent(folder(&["setup.json", NOTE_FILE]));
        assert_eq!(
            plan_folder(FolderKind::Config, &done, false),
            FolderPlan::Skip("taken over before")
        );
    }

    #[test]
    fn the_newest_source_wins() {
        // Detent was taken over by Trimcraft 0.1.3 before: copy 0.1.3's folder.
        let both = [
            folder(&["setup.json", "values.json"]),
            folder(&["setup.json", NOTE_FILE]),
        ];
        assert!(matches!(
            plan_folder(FolderKind::Config, &both, false),
            FolderPlan::Copy { source: 0, .. }
        ));
        // 0.1.3 started with a setup of its own and left Detent alone: still 0.1.3's.
        let both = [folder(&["setup.json"]), folder(&["setup.json"])];
        assert!(matches!(
            plan_folder(FolderKind::Config, &both, false),
            FolderPlan::Copy { source: 0, .. }
        ));
        // Once 0.1.3's folder was taken over, Detent's is never looked at.
        let both = [
            folder(&["setup.json", MOVED_NOTE_FILE]),
            folder(&["setup.json"]),
        ];
        assert_eq!(
            plan_folder(FolderKind::Config, &both, false),
            FolderPlan::Skip("taken over before")
        );
        // An old Trimcraft folder with nothing in it does not hide Detent's.
        let both = [folder(&["setup.tmp"]), folder(&["setup.json"])];
        assert!(matches!(
            plan_folder(FolderKind::Config, &both, false),
            FolderPlan::Copy { source: 1, .. }
        ));
    }

    #[test]
    fn notes_of_earlier_migrations_are_not_copied() {
        // 0.1.3's own folder never has Detent's note, but a copy must not carry one along.
        let plan = plan_folder(
            FolderKind::Config,
            &only_trimcraft(folder(&["setup.json", NOTE_FILE])),
            false,
        );
        assert_eq!(
            plan,
            FolderPlan::Copy {
                source: 0,
                files: files(&["setup.json"]),
                skipped: files(&[NOTE_FILE]),
            }
        );
    }

    #[test]
    fn local_folder_takes_only_the_logs() {
        let state = folder(&[
            "logs/trimcraft.log",
            "logs/trimcraft_2026-10-01.log",
            "EBWebView/Default/Local Storage/leveldb/000003.log",
            "EBWebView/Default/Cache/data_0",
        ]);
        // The new setup is not looked at for the local folder.
        assert_eq!(
            plan_folder(FolderKind::Local, &only_trimcraft(state), true),
            FolderPlan::Copy {
                source: 0,
                files: files(&["logs/trimcraft.log", "logs/trimcraft_2026-10-01.log"]),
                skipped: files(&[
                    "EBWebView/Default/Cache/data_0",
                    "EBWebView/Default/Local Storage/leveldb/000003.log",
                ]),
            }
        );
        let webview_only = folder(&["EBWebView/Default/Cache/data_0"]);
        assert_eq!(
            plan_folder(FolderKind::Local, &only_trimcraft(webview_only), false),
            FolderPlan::Skip("no earlier folder")
        );
        // Detent's logs when 0.1.3 has none; none when 0.1.3's were taken over.
        let detent_logs = folder(&["logs/detent.log"]);
        assert!(matches!(
            plan_folder(
                FolderKind::Local,
                &[folder(&["EBWebView/x"]), detent_logs.clone()],
                false
            ),
            FolderPlan::Copy { source: 1, .. }
        ));
        assert_eq!(
            plan_folder(
                FolderKind::Local,
                &[
                    folder(&["logs/trimcraft.log", MOVED_NOTE_FILE]),
                    detent_logs
                ],
                false
            ),
            FolderPlan::Skip("taken over before")
        );
    }

    #[test]
    fn credential_targets_get_the_new_prefix() {
        assert_eq!(
            new_secret_target("detent/discord/client-secret").as_deref(),
            Some("trimcraft/discord/client-secret")
        );
        assert_eq!(
            new_secret_target("detent/http/18f3a").as_deref(),
            Some("trimcraft/http/18f3a")
        );
        assert_eq!(new_secret_target("detent/"), None);
        assert_eq!(new_secret_target("other/detent/x"), None);
        assert_eq!(new_secret_target("trimcraft/http/1"), None);
    }

    #[test]
    fn credentials_are_copied_or_reconciled() {
        let old = files(&[
            "detent/http/2",
            "detent/discord/oauth-tokens",
            "detent/discord/client-secret",
            "something-else",
        ]);
        let existing = files(&["trimcraft/discord/client-secret"]);
        let copy = |o: &str, n: &str| CredentialStep::Copy {
            old: o.into(),
            new: n.into(),
        };
        assert_eq!(
            plan_credentials(&old, &existing),
            vec![
                CredentialStep::Reconcile {
                    old: "detent/discord/client-secret".into(),
                    new: "trimcraft/discord/client-secret".into(),
                },
                copy(
                    "detent/discord/oauth-tokens",
                    "trimcraft/discord/oauth-tokens"
                ),
                copy("detent/http/2", "trimcraft/http/2"),
            ]
        );
        assert!(plan_credentials(&[], &existing).is_empty());
    }

    #[test]
    fn secret_references_are_renamed_wherever_they_are() {
        let mut setup = json!({
            "connections": {"discord": {"clientId": "1", "clientSecret":
                {"store": "windowsCredentialManager", "target": "detent/discord/client-secret"}}},
            "profiles": [{"virtualControls": [{"actions": [{"type": "http", "headers": [
                {"name": "Authorization", "value": "",
                 "secret": {"store": "windowsCredentialManager", "target": "detent/http/1"}},
                {"name": "X-Plain", "value": "detent/http/1"},
                {"name": "X-New", "value": "",
                 "secret": {"store": "windowsCredentialManager", "target": "trimcraft/http/2"}}
            ]}]}]}],
        });
        assert_eq!(rename_secret_refs(&mut setup), 2);
        assert_eq!(
            setup["connections"]["discord"]["clientSecret"]["target"],
            "trimcraft/discord/client-secret"
        );
        let headers = &setup["profiles"][0]["virtualControls"][0]["actions"][0]["headers"];
        assert_eq!(headers[0]["secret"]["target"], "trimcraft/http/1");
        // Only references are touched, not texts that happen to look like one.
        assert_eq!(headers[1]["value"], "detent/http/1");
        assert_eq!(headers[2]["secret"]["target"], "trimcraft/http/2");
        // A second pass changes nothing.
        assert_eq!(rename_secret_refs(&mut setup), 0);
    }

    #[test]
    fn old_autostart_is_removed_the_task_only_elevated() {
        assert_eq!(
            plan_autostart(false, false, false),
            AutostartMigration::default()
        );
        assert_eq!(
            plan_autostart(true, false, false).steps,
            vec![AutostartStep::RemoveOldRunKey]
        );
        let normal = plan_autostart(true, true, false);
        assert_eq!(normal.steps, vec![AutostartStep::RemoveOldRunKey]);
        assert!(normal.note.is_some());
        let elevated = plan_autostart(false, true, true);
        assert_eq!(elevated.steps, vec![AutostartStep::RemoveOldTask]);
        assert_eq!(elevated.note, None);
        assert_eq!(old_task_name("sam"), "Detent autostart sam");
    }
}
