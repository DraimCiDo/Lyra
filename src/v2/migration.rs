//! Read-only v1 conversion. A v2 import is an independent, versioned snapshot.
use crate::{
    desktop::{history, preferences::Preferences},
    model_gateway::Message,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

const MAX_IMPORT_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportBundle {
    pub schema_version: u32,
    pub source: PathBuf,
    pub preferences: Option<ImportedPreferences>,
    pub conversations: Vec<Vec<Message>>,
    pub observations: Vec<String>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportedPreferences {
    pub model: Option<PathBuf>,
    pub project: Option<PathBuf>,
}

impl ImportBundle {
    pub fn preview(source: &Path) -> io::Result<Self> {
        let source = source.canonicalize()?;
        if !source.is_dir() {
            return Err(io::Error::other(
                "Select the v1 settings and conversation directory",
            ));
        }
        let mut observations = Vec::new();
        let preferences = match regular_file(&source, "settings.json")
            .and_then(|path| Preferences::load(&path))
        {
            Ok(preferences) => Some(ImportedPreferences {
                model: preferences.model,
                project: preferences.repo,
            }),
            Err(error) => {
                observations.push(format!("Settings: {error}"));
                None
            }
        };
        let conversations = match regular_file(&source, "conversations.json")
            .and_then(|path| history::export_conversations(&path))
        {
            Ok(conversations) => conversations,
            Err(error) => {
                observations.push(format!("Conversations: {error}"));
                Vec::new()
            }
        };
        Ok(Self {
            schema_version: 1,
            source,
            preferences,
            conversations,
            observations,
        })
    }

    pub fn load(path: &Path) -> io::Result<Self> {
        if !fs::symlink_metadata(path)?.is_file() {
            return Err(io::Error::other("An ordinary v2 import file is required"));
        }
        let mut bytes = Vec::new();
        fs::File::open(path)?
            .take(MAX_IMPORT_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_IMPORT_BYTES {
            return Err(io::Error::other("Import exceeds the size limit"));
        }
        let bundle: Self = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        if bundle.schema_version != 1 {
            return Err(io::Error::other("Unsupported v2 import schema"));
        }
        Ok(bundle)
    }

    pub fn install(&self, path: &Path) -> io::Result<()> {
        if self.preferences.is_none() && self.conversations.is_empty() {
            return Err(io::Error::other("No supported v1 data to import"));
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .canonicalize()?;
        if parent.starts_with(&self.source) {
            return Err(io::Error::other(
                "Choose a destination outside the v1 data directory",
            ));
        }
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        let bytes = serde_json::to_vec(self).map_err(io::Error::other)?;
        if bytes.len() as u64 > MAX_IMPORT_BYTES {
            return Err(io::Error::other("Import exceeds the size limit"));
        }
        temporary.write_all(&bytes)?;
        temporary.as_file().sync_all()?;
        match temporary.persist_noclobber(path) {
            Ok(_) => Ok(()),
            Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => {
                if Self::load(path)? == *self {
                    Ok(())
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "Choose a new path for a different v1 import",
                    ))
                }
            }
            Err(error) => Err(error.error),
        }
    }
}

fn regular_file(folder: &Path, name: &str) -> io::Result<PathBuf> {
    let path = folder.join(name);
    if fs::symlink_metadata(&path)?.is_file() {
        Ok(path)
    } else {
        Err(io::Error::other("Expected an ordinary v1 data file"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_gateway::Role;

    #[test]
    fn import_is_read_only_idempotent_and_omits_unsupported_data() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("v1");
        fs::create_dir(&source).unwrap();
        let settings = br#"{"repo":"project","model":"model.gguf","cloud_task":true,"cloud_provider":"unused"}"#;
        fs::write(source.join("settings.json"), settings).unwrap();
        fs::write(source.join("conversations.json"), br#"{"sessions":[{"chat":{"messages":[{"user":true,"text":"hello"},{"user":false,"text":"world"}]},"mode":"Chat","repo":null,"files":"","draft":""}],"active":0}"#).unwrap();
        let preview = ImportBundle::preview(&source).unwrap();
        assert!(
            preview.observations.is_empty(),
            "{:?}",
            preview.observations
        );
        assert_eq!(preview.conversations[0][0].role, Role::User);
        assert_eq!(preview.conversations[0][1].content, "world");
        let destination = root.path().join("import.json");
        assert!(!destination.exists());
        preview.install(&destination).unwrap();
        preview.install(&destination).unwrap();
        assert_eq!(ImportBundle::load(&destination).unwrap(), preview);
        assert_eq!(fs::read(source.join("settings.json")).unwrap(), settings);
        assert!(
            !String::from_utf8(fs::read(destination).unwrap())
                .unwrap()
                .contains("cloud_provider")
        );
    }

    #[test]
    fn preview_reports_invalid_history_but_retains_supported_settings() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("settings.json"), b"{}").unwrap();
        fs::write(root.path().join("conversations.json"), b"broken JSON").unwrap();
        let preview = ImportBundle::preview(root.path()).unwrap();
        assert!(preview.preferences.is_some());
        assert!(preview.conversations.is_empty());
        assert_eq!(preview.observations.len(), 1);
        assert!(preview.install(&root.path().join("import.json")).is_err());
    }
}
