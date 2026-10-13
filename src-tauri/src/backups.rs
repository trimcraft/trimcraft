//! Automatic backups of setup.json in the `backups` folder of the config directory.
//!
//! Every successful save of the setup asks for a backup; they are written at most once every
//! five minutes (a change that arrives sooner is written when the time is up). Backups are also
//! written before an import and before a restore, and on request. Old ones are pruned by the
//! rules in `detent_core::backup`.

use detent_core::backup::{self, BackupInfo};
use detent_core::{Category, Diagnostics, Record};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DIR: &str = "backups";
/// How often the thread looks for a change that is waiting for its turn.
const TICK: Duration = Duration::from_secs(30);

#[derive(Default)]
struct State {
    /// When a backup was last written by this run.
    last: Option<Instant>,
    /// Setup text of a save that came too soon after the last backup.
    pending: Option<String>,
    /// Text of the last backup, so identical content is not written twice.
    last_text: Option<String>,
}

pub struct Backups {
    dir: PathBuf,
    state: Mutex<State>,
    diagnostics: Diagnostics,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Backups {
    /// `config_dir` is where setup.json lives. Starts the thread that writes waiting changes.
    pub fn spawn(config_dir: &std::path::Path, diagnostics: Diagnostics) -> Arc<Self> {
        let backups = Arc::new(Self {
            dir: config_dir.join(DIR),
            state: Mutex::new(State::default()),
            diagnostics,
        });
        let weak = Arc::downgrade(&backups);
        let spawned = std::thread::Builder::new()
            .name("detent-backups".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(TICK);
                    let Some(backups) = weak.upgrade() else {
                        return;
                    };
                    backups.write_pending();
                }
            });
        if let Err(e) = spawned {
            log::error!("could not start the backup thread: {e}");
        }
        backups
    }

    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The setup was saved. Writes a backup unless one was written less than five minutes ago;
    /// then the text waits.
    pub fn note_saved(&self, text: &str) {
        let due = {
            let mut state = self.state();
            let due = state
                .last
                .is_none_or(|t| t.elapsed().as_secs() >= backup::MIN_INTERVAL_SECS);
            if !due {
                state.pending = Some(text.to_owned());
            }
            due
        };
        if due {
            self.write(text, "automatic");
        }
    }

    fn write_pending(&self) {
        let text = {
            let mut state = self.state();
            let due = state
                .last
                .is_none_or(|t| t.elapsed().as_secs() >= backup::MIN_INTERVAL_SECS);
            if !due {
                return;
            }
            state.pending.take()
        };
        if let Some(text) = text {
            self.write(&text, "automatic");
        }
    }

    /// Writes a backup now, whatever the interval. `reason` goes into the event log.
    pub fn create(&self, text: &str, reason: &str) -> Result<BackupInfo, String> {
        if let Err(e) = fs::create_dir_all(&self.dir) {
            return Err(self.failed(format!("cannot create the backup folder: {e}")));
        }
        // Two backups within a second get consecutive names.
        let mut secs = now_secs();
        while self.dir.join(backup::file_name(secs)).exists() {
            secs += 1;
        }
        let name = backup::file_name(secs);
        let path = self.dir.join(&name);
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, text)
            .and_then(|()| fs::rename(&tmp, &path))
            .map_err(|e| self.failed(format!("cannot write the backup {name}: {e}")))?;
        {
            let mut state = self.state();
            state.last = Some(Instant::now());
            state.pending = None;
            state.last_text = Some(text.to_owned());
        }
        self.diagnostics.record(
            Record::new(Category::Log, format!("Backup written ({reason}): {name}"))
                .detail(path.display().to_string()),
        );
        self.prune();
        Ok(BackupInfo {
            name,
            unix_secs: secs,
            size: text.len() as u64,
            profiles: backup::profile_count(text),
        })
    }

    fn write(&self, text: &str, reason: &str) {
        if self.state().last_text.as_deref() == Some(text) {
            return;
        }
        // Errors are reported by `create`.
        let _ = self.create(text, reason);
    }

    fn failed(&self, message: String) -> String {
        // The warning reaches the event log through the log bridge.
        log::warn!("backup failed: {message}");
        message
    }

    /// Newest first.
    pub fn list(&self) -> Vec<BackupInfo> {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut list: Vec<BackupInfo> = entries
            .filter_map(Result::ok)
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                let unix_secs = backup::parse_name(&name)?;
                let size = e.metadata().ok()?.len();
                let profiles = fs::read_to_string(e.path())
                    .ok()
                    .and_then(|t| backup::profile_count(&t));
                Some(BackupInfo {
                    name,
                    unix_secs,
                    size,
                    profiles,
                })
            })
            .collect();
        list.sort_by(|a, b| {
            b.unix_secs
                .cmp(&a.unix_secs)
                .then_with(|| b.name.cmp(&a.name))
        });
        list
    }

    /// The text of the backup `name` (only names of this folder's backups are accepted).
    pub fn read(&self, name: &str) -> Result<String, String> {
        if backup::parse_name(name).is_none() {
            return Err(format!("{name} is not a backup"));
        }
        fs::read_to_string(self.dir.join(name)).map_err(|e| format!("cannot read {name}: {e}"))
    }

    fn prune(&self) {
        let list: Vec<(String, u64)> = self
            .list()
            .into_iter()
            .map(|b| (b.name, b.unix_secs))
            .collect();
        for name in backup::to_prune(&list, now_secs()) {
            if let Err(e) = fs::remove_file(self.dir.join(&name)) {
                log::warn!("could not delete the old backup {name}: {e}");
            }
        }
    }
}
