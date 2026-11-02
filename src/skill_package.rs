//! Local immutable skill snapshots. Metadata is declared provenance, not trust.
use crate::skills::{Skill, SkillPack};
use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

const MAX_STORE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_CONTENT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Args)]
pub struct PackageArgs {
    #[command(subcommand)]
    pub command: PackageCommand,
}

#[derive(Debug, Subcommand)]
pub enum PackageCommand {
    /// Установить снимок явно выбранных навыков без выполнения scripts/hooks
    Install {
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        store: PathBuf,
        #[arg(long)]
        id: String,
        #[arg(long)]
        version: String,
        #[arg(long)]
        origin: String,
        #[arg(long = "skill", required = true)]
        selected: Vec<String>,
    },
    /// Показать metadata текущего и предыдущего снимка
    Show {
        #[arg(long)]
        store: PathBuf,
    },
    /// Поменять текущий и предыдущий снимки местами
    Rollback {
        #[arg(long)]
        store: PathBuf,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub id: String,
    pub version: String,
    pub origin: String,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Revision {
    pub metadata: Metadata,
    pub pack: SkillPack,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Store {
    pub schema_version: u32,
    pub current: Revision,
    pub previous: Option<Revision>,
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

pub fn is_store(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.to_string_lossy().ends_with(".lyra-skills.json"))
}

fn valid_label(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

fn digest(pack: &SkillPack) -> io::Result<String> {
    let bytes = serde_json::to_vec(pack).map_err(io::Error::other)?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

impl Revision {
    fn validate(&self) -> io::Result<()> {
        self.pack.validate()?;
        let content_bytes = self.pack.skills.iter().fold(0usize, |sum, skill| {
            sum.saturating_add(skill.id.len())
                .saturating_add(skill.description.len())
                .saturating_add(skill.instructions.len())
                .saturating_add(skill.tags.iter().map(String::len).sum::<usize>())
        });
        if !valid_id(&self.metadata.id)
            || !valid_label(&self.metadata.version, 64)
            || !valid_label(&self.metadata.origin, 1024)
            || self.pack.skills.is_empty()
            || content_bytes > MAX_CONTENT_BYTES
            || self.metadata.sha256 != digest(&self.pack)?
        {
            return Err(io::Error::other(
                "Invalid skill package metadata, size or SHA256",
            ));
        }
        Ok(())
    }
}

impl Store {
    pub fn load(path: &Path) -> io::Result<Self> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(MAX_STORE_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_STORE_BYTES {
            return Err(io::Error::other("Skill package exceeds 16 MiB"));
        }
        let store: Self = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        store.validate()?;
        Ok(store)
    }

    fn validate(&self) -> io::Result<()> {
        if self.schema_version != 1 {
            return Err(io::Error::other("Unsupported skill package schema"));
        }
        self.current.validate()?;
        if let Some(previous) = &self.previous {
            previous.validate()?;
            if previous.metadata.id != self.current.metadata.id
                || previous.metadata.version == self.current.metadata.version
            {
                return Err(io::Error::other("Conflicting skill package revisions"));
            }
        }
        Ok(())
    }

    fn save(&self, path: &Path) -> io::Result<()> {
        self.validate()?;
        let bytes = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        if bytes.len() as u64 > MAX_STORE_BYTES {
            return Err(io::Error::other("Skill package exceeds 16 MiB"));
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.write_all(&bytes)?;
        file.as_file().sync_all()?;
        file.persist(path).map_err(|error| error.error)?;
        Ok(())
    }

    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"current": self.current.metadata,
            "skills": self.current.pack.skills.iter().map(|skill| &skill.id).collect::<Vec<_>>(),
            "previous": self.previous.as_ref().map(|revision| &revision.metadata)})
    }
}

struct Lock {
    path: PathBuf,
    _file: std::fs::File,
}
impl Lock {
    fn acquire(path: &Path) -> io::Result<Self> {
        if !is_store(path) {
            return Err(io::Error::other(
                "Store filename must end with .lyra-skills.json",
            ));
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent)?;
        let mut lock_name = path.as_os_str().to_os_string();
        lock_name.push(".lock");
        let lock_path = PathBuf::from(lock_name);
        let file = std::fs::OpenOptions::new().write(true).create_new(true).open(&lock_path).map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                io::Error::new(io::ErrorKind::AlreadyExists, "Skill package is locked by another operation; a stale lock requires manual recovery")
            } else { error }
        })?;
        Ok(Self {
            path: lock_path,
            _file: file,
        })
    }
}
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn install(
    source: &Path,
    path: &Path,
    id: &str,
    version: &str,
    origin: &str,
    selected: &[String],
) -> io::Result<Store> {
    if !valid_id(id)
        || !valid_label(version, 64)
        || !valid_label(origin, 1024)
        || selected.is_empty()
    {
        return Err(io::Error::other(
            "Package id, version, origin and selected skills are required",
        ));
    }
    let _lock = Lock::acquire(path)?;
    let source = SkillPack::load_source(source)?;
    let mut seen = std::collections::HashSet::new();
    let skills: Vec<Skill> = selected
        .iter()
        .map(|id| {
            if !seen.insert(id) {
                return Err(io::Error::other("Duplicate selected skill"));
            }
            source
                .find(id)
                .cloned()
                .ok_or_else(|| io::Error::other("Selected skill not found"))
        })
        .collect::<io::Result<_>>()?;
    let pack = SkillPack {
        schema_version: 1,
        skills,
    };
    let revision = Revision {
        metadata: Metadata {
            id: id.into(),
            version: version.into(),
            origin: origin.into(),
            sha256: digest(&pack)?,
        },
        pack,
    };
    revision.validate()?;
    let previous =
        if path.try_exists()? {
            let old = Store::load(path)?;
            if old.current.metadata.id != id {
                return Err(io::Error::other("Store belongs to another package"));
            }
            if old.current.metadata.version == version {
                if old.current == revision {
                    return Ok(old);
                }
                return Err(io::Error::other(
                    "Version already exists with different content or origin",
                ));
            }
            if old.previous.as_ref().is_some_and(|previous| {
                previous.metadata.version == version && previous != &revision
            }) {
                return Err(io::Error::other(
                    "Previous version already exists with different content or origin",
                ));
            }
            Some(old.current)
        } else {
            None
        };
    let store = Store {
        schema_version: 1,
        current: revision,
        previous,
    };
    store.save(path)?;
    Ok(store)
}

pub fn rollback(path: &Path) -> io::Result<Store> {
    let _lock = Lock::acquire(path)?;
    let mut store = Store::load(path)?;
    let previous = store
        .previous
        .as_mut()
        .ok_or_else(|| io::Error::other("No previous skill package version"))?;
    std::mem::swap(&mut store.current, previous);
    store.save(path)?;
    Ok(store)
}

pub fn run(args: PackageArgs, output: &mut impl Write) -> io::Result<()> {
    let store = match args.command {
        PackageCommand::Install {
            source,
            store,
            id,
            version,
            origin,
            selected,
        } => install(&source, &store, &id, &version, &origin, &selected)?,
        PackageCommand::Show { store } => Store::load(&store)?,
        PackageCommand::Rollback { store } => rollback(&store)?,
    };
    serde_json::to_writer_pretty(&mut *output, &store.summary()).map_err(io::Error::other)?;
    writeln!(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(path: &Path, instructions: &str) {
        let pack = SkillPack {
            schema_version: 1,
            skills: vec![
                Skill {
                    id: "review".into(),
                    description: "Review".into(),
                    instructions: instructions.into(),
                    tags: vec![],
                },
                Skill {
                    id: "other".into(),
                    description: "Not selected".into(),
                    instructions: "Do not include".into(),
                    tags: vec![],
                },
            ],
        };
        std::fs::write(path, serde_json::to_vec(&pack).unwrap()).unwrap();
    }

    #[test]
    fn selective_snapshot_survives_source_changes_and_rolls_both_ways() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.json");
        let path = dir.path().join("store.lyra-skills.json");
        let selected = vec!["review".into()];
        source(&input, "version one\r\n");
        let first = install(
            &input,
            &path,
            "ecc-local",
            "v1",
            "ECC pinned revision",
            &selected,
        )
        .unwrap();
        assert_eq!(first.current.pack.skills.len(), 1);
        assert_eq!(first.current.metadata.sha256.len(), 64);
        source(&input, "version two\r\n");
        assert_eq!(SkillPack::load_source(&path).unwrap(), first.current.pack);
        assert!(
            install(
                &input,
                &path,
                "ecc-local",
                "v1",
                "ECC pinned revision",
                &selected
            )
            .is_err()
        );
        assert_eq!(Store::load(&path).unwrap(), first);
        let second = install(
            &input,
            &path,
            "ecc-local",
            "v2",
            "ECC revision two",
            &selected,
        )
        .unwrap();
        assert_eq!(second.previous.as_ref().unwrap(), &first.current);
        assert_eq!(
            install(
                &input,
                &path,
                "ecc-local",
                "v2",
                "ECC revision two",
                &selected
            )
            .unwrap(),
            second
        );
        let reverted = rollback(&path).unwrap();
        assert_eq!(reverted.current, first.current);
        assert_eq!(rollback(&path).unwrap(), second);
        assert!(!dir.path().join("store.lyra-skills.json.lock").exists());
    }

    #[test]
    fn corruption_lock_and_invalid_install_preserve_working_store() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.json");
        let path = dir.path().join("store.lyra-skills.json");
        source(&input, "Original");
        let selected = vec!["review".into()];
        install(&input, &path, "ecc", "1", "Local fixture", &selected).unwrap();
        let original = std::fs::read(&path).unwrap();
        assert!(rollback(&path).is_err());
        assert!(install(&input, &path, "other", "2", "Local fixture", &selected).is_err());
        assert!(
            install(
                &input,
                &path,
                "ecc",
                "2",
                "Local fixture",
                &["missing".into()]
            )
            .is_err()
        );
        assert!(
            install(
                &input,
                &path,
                "ecc",
                "2",
                "Local fixture",
                &["review".into(), "review".into()]
            )
            .is_err()
        );
        {
            let _lock = Lock::acquire(&path).unwrap();
            assert_eq!(
                rollback(&path).unwrap_err().kind(),
                io::ErrorKind::AlreadyExists
            );
            assert!(install(&input, &path, "ecc", "2", "Local fixture", &selected).is_err());
        }
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let mut tampered: Store = serde_json::from_slice(&original).unwrap();
        tampered.current.pack.skills[0].instructions = "Corrupted".into();
        let damaged = serde_json::to_vec(&tampered).unwrap();
        std::fs::write(&path, &damaged).unwrap();
        assert!(SkillPack::load_source(&path).is_err());
        assert!(install(&input, &path, "ecc", "2", "Local fixture", &selected).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), damaged);
    }

    #[test]
    fn invalid_metadata_cannot_escape_package_directory() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.json");
        let path = dir.path().join("new/store.lyra-skills.json");
        source(&input, "Original");
        for (id, version, origin) in [
            ("../escape", "v1", "Local"),
            ("ecc", "", "Local"),
            ("ecc", "v1", "\n"),
        ] {
            assert!(install(&input, &path, id, version, origin, &["review".into()]).is_err());
        }
        assert!(!path.parent().unwrap().exists());
        assert!(
            install(
                &input,
                &dir.path().join("source.json"),
                "ecc",
                "1",
                "Local",
                &["review".into()]
            )
            .is_err()
        );
    }
}
