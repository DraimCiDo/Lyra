//! Permission-scoped, declarative skill registry.
//! Skills are data, not executable scripts. Hooks and shell commands require
//! separate explicit grants and are never executed by this module.
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io;
use std::path::Path;

const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_SKILLS: usize = 256;
const MAX_CATALOG_BYTES: usize = 4 * 1024 * 1024;
const MAX_CATALOG_ENTRIES: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Skill {
    pub id: String,
    pub description: String,
    pub instructions: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillPack {
    pub schema_version: u32,
    pub skills: Vec<Skill>,
}

impl SkillPack {
    /// A JSON pack, one SKILL.md, or an explicitly selected directory containing
    /// SKILL.md and/or immediate child directories with SKILL.md. No recursion,
    /// resource execution, network access, or implicit skill activation.
    pub fn load_source(path: &Path) -> io::Result<Self> {
        if !path.is_dir() {
            if crate::skill_package::is_store(path) {
                return Ok(crate::skill_package::Store::load(path)?.current.pack);
            }
            return if path.file_name().is_some_and(|name| name == "SKILL.md") {
                Self::load_markdown(path)
            } else {
                Self::load(path)
            };
        }
        let root = path.canonicalize()?;
        let mut files = Vec::new();
        let root_skill = root.join("SKILL.md");
        if root_skill.try_exists()? {
            files.push(root_skill);
        }
        for (index, entry) in std::fs::read_dir(&root)?.enumerate() {
            if index >= MAX_CATALOG_ENTRIES {
                return Err(io::Error::other("Skill catalog exceeds 512 entries"));
            }
            let entry = entry?;
            let kind = entry.file_type()?;
            // Junctions and symlink directories are never discovery roots.
            if kind.is_dir() && !kind.is_symlink() {
                let child = entry.path().canonicalize()?;
                if !child.starts_with(&root) {
                    return Err(io::Error::other("Skill directory escapes catalog"));
                }
                let candidate = child.join("SKILL.md");
                if candidate.try_exists()? {
                    files.push(candidate);
                }
            }
        }
        files.sort();
        let mut pack = Self {
            schema_version: 1,
            skills: Vec::new(),
        };
        let mut total = 0usize;
        for file in files {
            let resolved = file.canonicalize()?;
            if !resolved.starts_with(&root) || !resolved.is_file() {
                return Err(io::Error::other(
                    "SKILL.md escapes catalog or is not a file",
                ));
            }
            let imported = Self::load_markdown(&resolved)?;
            // Account for the actual bounded content, not potentially stale metadata.
            for skill in imported.skills {
                total = total
                    .saturating_add(skill.instructions.len())
                    .saturating_add(skill.description.len())
                    .saturating_add(skill.id.len());
                if total > MAX_CATALOG_BYTES {
                    return Err(io::Error::other("Skill catalog exceeds 4 MiB"));
                }
                pack.skills.push(skill);
                pack.validate()?;
            }
        }
        if pack.skills.is_empty() {
            return Err(io::Error::other("No SKILL.md files found in catalog"));
        }
        pack.skills.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(pack)
    }

    pub fn load(path: &Path) -> io::Result<Self> {
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(MAX_MANIFEST_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Skill manifest exceeds 64 KiB",
            ));
        }
        let pack: Self = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        pack.validate()?;
        Ok(pack)
    }

    /// Import one Agent Skills / ECC SKILL.md as declarative instructions.
    /// No scripts, shell commands, or hooks are executed during import.
    pub fn load_markdown(path: &Path) -> io::Result<Self> {
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(MAX_MANIFEST_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "SKILL.md exceeds 64 KiB",
            ));
        }
        let source = std::str::from_utf8(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let normalized = source.replace("\r\n", "\n");
        let rest = normalized.strip_prefix("---\n").ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "SKILL.md must start with YAML frontmatter",
            )
        })?;
        let (frontmatter, instructions) = rest.split_once("\n---\n").ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Unterminated SKILL.md frontmatter",
            )
        })?;
        let mut id = None;
        let mut description = None;
        for line in frontmatter.lines() {
            if line.starts_with(char::is_whitespace) {
                continue;
            }
            if let Some((key, value)) = line.split_once(':') {
                let value = value.trim().trim_matches('"').trim_matches('\'');
                match key.trim() {
                    "name" if id.is_none() => id = Some(value.to_owned()),
                    "description" if description.is_none() => description = Some(value.to_owned()),
                    "name" | "description" => {
                        return Err(io::Error::other("Duplicate SKILL.md field"));
                    }
                    _ => {}
                }
            }
        }
        let pack = Self {
            schema_version: 1,
            skills: vec![Skill {
                id: id.ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "Missing skill name")
                })?,
                description: description.ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "Missing skill description")
                })?,
                instructions: instructions.trim().to_owned(),
                tags: Vec::new(),
            }],
        };
        pack.validate()?;
        Ok(pack)
    }

    pub fn validate(&self) -> io::Result<()> {
        if self.schema_version != 1 || self.skills.len() > MAX_SKILLS {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid skill pack version or size",
            ));
        }
        let mut seen = HashSet::new();
        for skill in &self.skills {
            let valid_id = !skill.id.is_empty()
                && skill.id.len() <= 64
                && skill
                    .id
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-');
            if !valid_id
                || !seen.insert(&skill.id)
                || skill.description.len() > 1024
                || skill.instructions.len() > 16384
                || skill.tags.len() > 32
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Invalid or duplicate skill",
                ));
            }
        }
        Ok(())
    }

    pub fn find(&self, id: &str) -> Option<&Skill> {
        self.skills.iter().find(|skill| skill.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_manifest_loads_and_rejects_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("skills.json");
        std::fs::write(
            &path,
            r#"{"schema_version":1,"skills":[{"id":"rust-review","description":"Review Rust","instructions":"Check ownership and tests"}]}"#,
        )
        .unwrap();
        let pack = SkillPack::load(&path).unwrap();
        assert_eq!(pack.find("rust-review").unwrap().description, "Review Rust");
        let mut invalid = pack.clone();
        invalid.skills.push(invalid.skills[0].clone());
        assert!(invalid.validate().is_err());
        invalid.skills[1].id = "../escape".into();
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn imports_ecc_style_skill_markdown_without_execution() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("SKILL.md");
        std::fs::write(
            &path,
            "---\nname: rust-review\ndescription: Review Rust changes\n---\n# Steps\nCheck tests.\n",
        )
        .unwrap();
        let pack = SkillPack::load_markdown(&path).unwrap();
        assert_eq!(
            pack.find("rust-review").unwrap().instructions,
            "# Steps\nCheck tests."
        );
        std::fs::write(&path, "---\nname: ../bad\ndescription: Unsafe\n---\nhello").unwrap();
        assert!(SkillPack::load_markdown(&path).is_err());
    }

    #[test]
    fn imports_crlf_skill_without_losing_body() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("SKILL.md");
        std::fs::write(
            &path,
            "---\r\nname: rust-review\r\ndescription: Review Rust\r\n---\r\n# Steps\r\nCheck tests.",
        )
        .unwrap();
        let pack = SkillPack::load_markdown(&path).unwrap();
        assert_eq!(pack.skills[0].instructions, "# Steps\nCheck tests.");
    }

    #[test]
    fn unknown_manifest_fields_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("skills.json");
        std::fs::write(&path, r#"{"schema_version":1,"skills":[],"execute":true}"#).unwrap();
        assert!(SkillPack::load(&path).is_err());
    }

    fn write_skill(root: &Path, directory: &str, id: &str, body: &str) {
        let path = root.join(directory);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(
            path.join("SKILL.md"),
            format!(
                "---\nname: {id}\ndescription: Review code\nmetadata:\n  name: ignored\n---\n{body}"
            ),
        )
        .unwrap();
    }

    #[test]
    fn catalog_is_sorted_bounded_and_never_executes_resources() {
        let dir = tempfile::tempdir().unwrap();
        write_skill(dir.path(), "z", "rust", "Check ownership.");
        write_skill(
            dir.path(),
            "a",
            "review",
            "Run scripts/hook.cmd only if authorized.",
        );
        // Non-skill resources and nested directories are not loaded or executed.
        std::fs::write(dir.path().join("a/hook.cmd"), "exit 99").unwrap();
        write_skill(dir.path(), "a/nested", "nested", "Not discovered");
        let pack = SkillPack::load_source(dir.path()).unwrap();
        assert_eq!(
            pack.skills
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
            ["review", "rust"]
        );
        assert_eq!(
            SkillPack::load_source(&dir.path().join("z"))
                .unwrap()
                .skills
                .len(),
            1
        );
        write_skill(dir.path(), "duplicate", "rust", "Conflicting duplicate");
        assert!(SkillPack::load_source(dir.path()).is_err());
        let empty = tempfile::tempdir().unwrap();
        assert!(SkillPack::load_source(empty.path()).is_err());
        for index in 0..=MAX_CATALOG_ENTRIES {
            std::fs::write(empty.path().join(index.to_string()), "").unwrap();
        }
        assert!(
            SkillPack::load_source(empty.path())
                .unwrap_err()
                .to_string()
                .contains("512")
        );
    }

    #[test]
    fn invalid_catalog_member_rejects_entire_selection() {
        let dir = tempfile::tempdir().unwrap();
        write_skill(dir.path(), "good", "good", "Review");
        write_skill(dir.path(), "bad", "bad", &"x".repeat(16385));
        assert!(SkillPack::load_source(dir.path()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn catalog_rejects_skill_link_outside_root_and_ignores_directory_links() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        write_skill(outside.path(), "", "external", "Do not read");
        std::os::unix::fs::symlink(outside.path(), dir.path().join("linked")).unwrap();
        assert!(SkillPack::load_source(dir.path()).is_err()); // no discovered skills
        write_skill(dir.path(), "local", "local", "Local instructions");
        assert_eq!(SkillPack::load_source(dir.path()).unwrap().skills.len(), 1);
        std::os::unix::fs::symlink(outside.path().join("SKILL.md"), dir.path().join("SKILL.md"))
            .unwrap();
        assert!(SkillPack::load_source(dir.path()).is_err());
    }
}
