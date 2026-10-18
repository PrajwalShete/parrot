//! `~/.parrot/config.json`: the Gemini API key, the data folder and a few preferences.
//!
//! The file is written atomically with owner-only permissions, because it holds the API key.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const DEFAULT_PORT: u16 = 4747;
pub const DEFAULT_VOICE: &str = "Kore";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub api_key: Option<String>,
    pub data_dir: PathBuf,
    pub port: u16,
    /// Prebuilt Gemini voice for the assistant.
    pub voice: String,
    /// BCP-47 hints for transcription, e.g. ["mr-IN", "hi-IN", "en-IN"]. Empty = auto-detect.
    pub language_codes: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_key: None,
            data_dir: default_data_dir(),
            port: DEFAULT_PORT,
            voice: DEFAULT_VOICE.into(),
            language_codes: Vec::new(),
        }
    }
}

/// `~/.parrot`, or `$PARROT_HOME` (used by tests and for portable installs).
pub fn home() -> PathBuf {
    if let Some(dir) = std::env::var_os("PARROT_HOME") {
        return PathBuf::from(dir);
    }
    dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".parrot")
}

pub fn path() -> PathBuf {
    home().join("config.json")
}

/// `~/Documents/Parrot`, falling back to `~/Parrot` where there is no Documents folder.
pub fn default_data_dir() -> PathBuf {
    dirs::document_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Parrot")
}

impl Config {
    pub fn load() -> Result<Option<Self>> {
        Self::load_from(&path())
    }

    pub fn load_from(path: &Path) -> Result<Option<Self>> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(Some(
                serde_json::from_str(&text).with_context(|| format!("{} is not valid JSON", path.display()))?,
            )),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    pub fn load_or_default() -> Result<Self> {
        Ok(Self::load()?.unwrap_or_default())
    }

    /// True once setup has stored a key.
    pub fn is_ready(&self) -> bool {
        self.api_key.as_deref().is_some_and(|k| !k.trim().is_empty())
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&path())
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        let dir = path.parent().context("config path has no parent")?;
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        restrict(dir, 0o700);
        let tmp = dir.join(".config.json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)? + "\n")?;
        restrict(&tmp, 0o600);
        std::fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    /// The key with everything but the last four characters hidden.
    pub fn masked_key(&self) -> String {
        mask(self.api_key.as_deref().unwrap_or(""))
    }
}

pub fn mask(key: &str) -> String {
    let n = key.chars().count();
    if n <= 4 {
        return "•".repeat(n);
    }
    let tail: String = key.chars().skip(n - 4).collect();
    format!("{}{tail}", "•".repeat(8))
}

#[cfg(unix)]
fn restrict(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
fn restrict(_: &Path, _: u32) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_is_private() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p/config.json");
        assert_eq!(Config::load_from(&path).unwrap(), None);
        let cfg = Config {
            api_key: Some("AIzaSyTEST1234".into()),
            ..Config::default()
        };
        cfg.save_to(&path).unwrap();
        assert_eq!(Config::load_from(&path).unwrap(), Some(cfg));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn missing_fields_take_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, r#"{"api_key":"k"}"#).unwrap();
        let cfg = Config::load_from(&path).unwrap().unwrap();
        assert_eq!(cfg.port, DEFAULT_PORT);
        assert_eq!(cfg.voice, DEFAULT_VOICE);
        assert!(cfg.is_ready());
    }

    #[test]
    fn masks_keys() {
        assert_eq!(mask("AIzaSyABCDEFGH1234"), "••••••••1234");
        assert_eq!(mask("abc"), "•••");
    }
}
