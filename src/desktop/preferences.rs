//! User preferences only: never persist a task, source code, model response or log.
use super::theme::{Language, Palette};
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

const LIMIT: u64 = 64 * 1024;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Preferences {
    pub schema_version: u32,
    pub palette: Palette,
    pub language: Language,
    pub animations: bool,
    pub check_updates: bool,
    pub preview_updates: bool,
    pub repo: Option<PathBuf>,
    pub model: Option<PathBuf>,
    pub provider_registry: Option<PathBuf>,
    pub documentation: Option<PathBuf>,
    pub files: String,
    pub subagents: bool,
    pub context: u32,
    pub tokens: u32,
    pub calls: u32,
    pub threads: u32,
    pub timeout: u64,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            schema_version: 1,
            palette: Palette::Violet,
            language: Language::Ru,
            animations: true,
            check_updates: cfg!(windows),
            preview_updates: env!("CARGO_PKG_VERSION").contains('-'),
            repo: None,
            model: None,
            provider_registry: None,
            documentation: None,
            files: String::new(),
            subagents: false,
            context: 4096,
            tokens: 512,
            calls: 12,
            threads: crate::inference::default_threads(),
            timeout: 600,
        }
    }
}
impl Preferences {
    fn valid(&self) -> bool {
        self.schema_version == 1
            && (128..=32768).contains(&self.context)
            && (1..=8192).contains(&self.tokens)
            && (1..=24).contains(&self.calls)
            && (!self.subagents || self.calls >= 3)
            && (1..=256).contains(&self.threads)
            && (1..=3600).contains(&self.timeout)
            && self.files.len() <= 32768
            && self.files.lines().filter(|l| !l.trim().is_empty()).count() <= 32
            && [
                &self.repo,
                &self.model,
                &self.documentation,
                &self.provider_registry,
            ]
            .into_iter()
            .all(|p| p.as_ref().is_none_or(|p| p.as_os_str().len() <= 4096))
    }
    pub fn load(path: &Path) -> io::Result<Self> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(LIMIT + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > LIMIT {
            return Err(io::Error::other("Preferences exceed 64 KiB"));
        }
        let prefs: Self = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        if !prefs.valid() {
            return Err(io::Error::other("Invalid preference values"));
        }
        Ok(prefs)
    }
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if !self.valid() {
            return Err(io::Error::other("Invalid preference values"));
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        if bytes.len() as u64 > LIMIT {
            return Err(io::Error::other("Preferences exceed 64 KiB"));
        }
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("No preferences directory"))?;
        std::fs::create_dir_all(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.write_all(&bytes)?;
        file.flush()?;
        file.persist(path).map_err(|e| e.error)?;
        Ok(())
    }
    pub fn reset_inference(&mut self) {
        let defaults = Self::default();
        self.context = defaults.context;
        self.tokens = defaults.tokens;
        self.calls = defaults.calls;
        self.threads = defaults.threads;
        self.timeout = defaults.timeout;
    }
}
pub fn config_path() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA")
            .filter(|p| !p.is_empty())
            .map(|p| PathBuf::from(p).join("Lyra/settings.json"))
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .filter(|p| !p.is_empty())
                    .map(|p| PathBuf::from(p).join(".config"))
            })
            .map(|p| p.join("lyra/settings.json"))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preferences_roundtrip_overwrite_and_reject_bad_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut p = Preferences {
            repo: Some(PathBuf::from("Проект с пробелом")),
            ..Default::default()
        };
        p.save(&path).unwrap();
        assert_eq!(Preferences::load(&path).unwrap(), p);
        p.palette = Palette::Light;
        p.language = Language::En;
        p.save(&path).unwrap();
        assert_eq!(Preferences::load(&path).unwrap(), p);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        let mut oversized = p.clone();
        oversized.files = "\"".repeat(32768);
        assert!(oversized.save(&path).is_err());
        assert_eq!(Preferences::load(&path).unwrap(), p);
        std::fs::write(&path, r#"{"threads":0}"#).unwrap();
        assert!(Preferences::load(&path).is_err());
        std::fs::write(&path, r#"{"task":"private code"}"#).unwrap();
        assert!(Preferences::load(&path).is_err());
        std::fs::write(&path, vec![b' '; LIMIT as usize + 1]).unwrap();
        assert!(Preferences::load(&path).is_err());
        let json = serde_json::to_value(p).unwrap();
        assert!(json.get("task").is_none());
        assert!(json.get("report").is_none());
        assert!(json.get("api_key").is_none());
    }
}
