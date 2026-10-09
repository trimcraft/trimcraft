//! JSON files in the app config directory, written atomically.

use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Storage {
    pub dir: PathBuf,
}

impl Storage {
    pub fn new(dir: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    /// Reads `name`. A missing file gives `None`; an unreadable one is moved aside as
    /// `<name>.broken` so the user's data is kept, and `None` is returned.
    pub fn load<T>(&self, name: &str, parse: impl FnOnce(&str) -> Result<T, String>) -> Option<T> {
        let path = self.path(name);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return None,
            Err(e) => {
                log::error!("cannot read {}: {e}", path.display());
                return None;
            }
        };
        match parse(&text) {
            Ok(value) => Some(value),
            Err(e) => {
                let aside = self.path(&format!("{name}.broken"));
                log::error!(
                    "{} is invalid ({e}); moved to {}",
                    path.display(),
                    aside.display()
                );
                if let Err(e) = fs::rename(&path, &aside) {
                    log::error!("could not move invalid file aside: {e}");
                }
                None
            }
        }
    }

    pub fn load_json<T: DeserializeOwned>(&self, name: &str) -> Option<T> {
        self.load(name, |t| serde_json::from_str(t).map_err(|e| e.to_string()))
    }

    pub fn save_json<T: Serialize>(&self, name: &str, value: &T) {
        let text = serde_json::to_string_pretty(value).expect("serialisable");
        self.save_text(name, &text);
    }

    pub fn save_text(&self, name: &str, text: &str) {
        let path = self.path(name);
        if let Err(e) = write_atomic(&path, text) {
            log::error!("could not save {}: {e}", path.display());
        }
    }
}

fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}
