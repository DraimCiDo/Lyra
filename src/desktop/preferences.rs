//! User preferences only: never persist a task, source code, model response or log.
use super::theme::{Language, Palette};
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

const LIMIT: u64 = 64 * 1024;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Optimization {
    Safe,
    #[default]
    Balanced,
    Aggressive,
}
impl Optimization {
    pub fn from_slider(value: u32) -> Self {
        match value {
            0 => Self::Safe,
            2 => Self::Aggressive,
            _ => Self::Balanced,
        }
    }
    pub fn slider(self) -> u32 {
        match self {
            Self::Safe => 0,
            Self::Balanced => 1,
            Self::Aggressive => 2,
        }
    }
}
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
    pub skill_manifest: Option<PathBuf>,
    pub active_skill: String,
    pub cloud_chat: bool,
    pub cloud_task: bool,
    pub cloud_provider: String,
    pub cloud_model: String,
    pub documentation: Option<PathBuf>,
    pub files: String,
    pub subagents: bool,
    pub optimization: Optimization,
    pub context: u32,
    pub tokens: u32,
    pub calls: u32,
    pub threads: u32,
    /// Number of model layers offloaded to CUDA; 0 keeps inference on CPU.
    pub gpu_layers: u32,
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
            skill_manifest: None,
            active_skill: String::new(),
            cloud_chat: false,
            cloud_task: false,
            cloud_provider: String::new(),
            cloud_model: String::new(),
            documentation: None,
            files: String::new(),
            subagents: false,
            optimization: Optimization::Balanced,
            context: 4096,
            tokens: 512,
            calls: 12,
            threads: crate::inference::default_threads(),
            gpu_layers: 0,
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
            && self.gpu_layers <= 1024
            && (1..=3600).contains(&self.timeout)
            && self.active_skill.len() <= 64
            && self.cloud_provider.len() <= 128
            && self.cloud_model.len() <= 256
            && self.files.len() <= 32768
            && self.files.lines().filter(|l| !l.trim().is_empty()).count() <= 32
            && [
                &self.repo,
                &self.model,
                &self.documentation,
                &self.provider_registry,
                &self.skill_manifest,
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
        self.gpu_layers = defaults.gpu_layers;
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
    fn provider_registry_path_roundtrip_and_legacy_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let prefs = Preferences {
            provider_registry: Some(PathBuf::from("C:/Lyra/providers.json")),
            ..Preferences::default()
        };
        prefs.save(&path).unwrap();
        assert_eq!(Preferences::load(&path).unwrap(), prefs);
        let mut legacy = serde_json::to_value(&prefs).unwrap();
        legacy.as_object_mut().unwrap().remove("provider_registry");
        std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        assert_eq!(Preferences::load(&path).unwrap().provider_registry, None);
    }

    #[test]
    fn gpu_settings_roundtrip_legacy_and_safe_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut prefs = Preferences {
            gpu_layers: 99,
            ..Preferences::default()
        };
        prefs.save(&path).unwrap();
        assert_eq!(Preferences::load(&path).unwrap().gpu_layers, 99);
        let mut legacy = serde_json::to_value(&prefs).unwrap();
        legacy.as_object_mut().unwrap().remove("gpu_layers");
        std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        assert_eq!(Preferences::load(&path).unwrap().gpu_layers, 0);
        prefs.gpu_layers = 1025;
        assert!(prefs.save(&path).is_err());
        prefs.reset_inference();
        assert_eq!(prefs.gpu_layers, 0);
    }

    #[test]
    fn optimization_modes_persist_and_legacy_defaults_to_balanced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        for mode in [
            Optimization::Safe,
            Optimization::Balanced,
            Optimization::Aggressive,
        ] {
            let prefs = Preferences {
                optimization: mode,
                ..Preferences::default()
            };
            prefs.save(&path).unwrap();
            assert_eq!(Preferences::load(&path).unwrap().optimization, mode);
            assert_eq!(Optimization::from_slider(mode.slider()), mode);
        }
        let mut legacy = serde_json::to_value(Preferences::default()).unwrap();
        legacy.as_object_mut().unwrap().remove("optimization");
        std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        assert_eq!(
            Preferences::load(&path).unwrap().optimization,
            Optimization::Balanced
        );
    }

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
        assert_eq!(json["optimization"], "balanced");
        assert_eq!(Optimization::from_slider(0), Optimization::Safe);
        assert_eq!(Optimization::from_slider(2), Optimization::Aggressive);
        assert!(json.get("task").is_none());
        assert!(json.get("report").is_none());
        assert!(json.get("api_key").is_none());
    }
}
