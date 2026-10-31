//! Permission-scoped, declarative skill registry.
//! Skills are data, not executable scripts. Hooks and shell commands require
//! separate explicit grants and are never executed by this module.
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io;
use std::path::Path;

const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_SKILLS: usize = 256;

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
            io::Error::new(io::ErrorKind::InvalidData, "SKILL.md must start with YAML frontmatter")
        })?;
        let (frontmatter, instructions) = rest.split_once("\n---\n").ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "Unterminated SKILL.md frontmatter")
        })?;
        let mut id = None;
        let mut description = None;
        for line in frontmatter.lines() {
            if let Some((key, value)) = line.split_once(':') {
                let value = value.trim().trim_matches('"').trim_matches('\'');
                match key.trim() {
                    "name" => id = Some(value.to_owned()),
                    "description" => description = Some(value.to_owned()),
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
        assert_eq!(pack.find("rust-review").unwrap().instructions, "# Steps\nCheck tests.");
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
}
