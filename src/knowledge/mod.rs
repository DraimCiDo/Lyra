//! Bounded document packs and deterministic retrieval; no network during inference.
#[cfg(feature = "online-docs")]
pub mod online;
use crate::project::profile::{self, Profile};
use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};
pub const DOCUMENT_LIMIT: usize = 256 * 1024;
pub const PACK_LIMIT: usize = 3 * 1024 * 1024;
pub const TEXT_BUDGET: usize = 2 * 1024 * 1024;
fn err(s: impl std::fmt::Display) -> io::Error {
    io::Error::other(s.to_string())
}
pub fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn read_text(path: &Path, limit: usize) -> io::Result<String> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_file() || m.file_type().is_symlink() {
        return Err(err("Ожидается обычный файл, не symlink."));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return Err(err("Reparse file запрещён."));
        }
    }
    let mut text = String::new();
    fs::File::open(path)?
        .take(limit as u64 + 1)
        .read_to_string(&mut text)?;
    if text.len() > limit || text.contains('\0') {
        return Err(err("Файл превышает лимит или содержит двоичные данные."));
    }
    Ok(text)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub id: String,
    pub title: String,
    pub origin: String,
    pub version_scope: String,
    pub sha256: String,
    pub retrieved_unix: u64,
}
impl Source {
    pub fn validate(&self) -> io::Result<()> {
        if self.id.is_empty()
            || self.id.len() > 64
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
            || self.title.is_empty()
            || self.title.len() > 256
            || self.origin.len() > 2048
            || self.origin.chars().any(char::is_control)
            || !(self.origin.starts_with("https://") || self.origin.starts_with("local:"))
            || self.version_scope.len() > 128
            || self.sha256.len() != 64
            || !self.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(err("Некорректные сведения об источнике."));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub source: Source,
    pub text: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pack {
    pub schema_version: u32,
    pub profile: Profile,
    pub target_version: String,
    pub documents: Vec<Document>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snippet {
    pub source: Source,
    pub line: usize,
    pub text: String,
}
impl Pack {
    pub fn empty() -> Self {
        Self {
            schema_version: 1,
            profile: Profile::Generic,
            target_version: String::new(),
            documents: Vec::new(),
        }
    }
    pub fn validate(&self) -> io::Result<()> {
        if self.schema_version != 1 || self.documents.len() > 16 {
            return Err(err("Неподдерживаемый document pack."));
        }
        profile::version_text(&self.target_version)?;
        let mut ids = BTreeSet::new();
        let mut size = 0;
        for d in &self.documents {
            d.source.validate()?;
            size += d.text.len();
            if !ids.insert(&d.source.id)
                || d.text.is_empty()
                || d.text.len() > DOCUMENT_LIMIT
                || size > TEXT_BUDGET
                || d.text.contains('\0')
                || digest(d.text.as_bytes()) != d.source.sha256.to_ascii_lowercase()
            {
                return Err(err(
                    "Повтор ID, неверный hash или превышен бюджет документации.",
                ));
            }
        }
        Ok(())
    }
    pub fn load(path: &Path) -> io::Result<Self> {
        let p: Self = serde_json::from_str(&read_text(path, PACK_LIMIT)?).map_err(err)?;
        p.validate()?;
        Ok(p)
    }
    pub fn save(&self, path: &Path) -> io::Result<()> {
        self.validate()?;
        let bytes = serde_json::to_vec_pretty(self).map_err(err)?;
        if bytes.len() > PACK_LIMIT {
            return Err(err("Сериализованный pack превышает лимит."));
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        temp.write_all(&bytes)?;
        temp.as_file().sync_all()?;
        temp.persist_noclobber(path).map_err(err)?;
        Ok(())
    }
    pub fn catalog(&self) -> Vec<&Source> {
        self.documents.iter().map(|d| &d.source).collect()
    }
    pub fn search(&self, query: &str, limit: usize, budget: usize) -> io::Result<Vec<Snippet>> {
        if query.trim().is_empty() || query.len() > 256 || limit == 0 || limit > 8 || budget > 8192
        {
            return Err(err(
                "Поиск документации: запрос 1–256 байт, 1–8 результатов, до 8 KiB.",
            ));
        }
        let terms: BTreeSet<_> = query
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .map(str::to_lowercase)
            .filter(|s| {
                s.len() > 1
                    && !matches!(
                        s.as_str(),
                        "the"
                            | "and"
                            | "for"
                            | "with"
                            | "from"
                            | "this"
                            | "that"
                            | "using"
                            | "create"
                            | "make"
                            | "build"
                            | "please"
                            | "program"
                            | "a"
                            | "an"
                            | "на"
                            | "для"
                            | "или"
                            | "чтобы"
                            | "создай"
                            | "сделай"
                            | "нужно"
                            | "программу"
                    )
            })
            .take(32)
            .collect();
        let mut candidates = Vec::new();
        for (di, d) in self.documents.iter().enumerate() {
            let mut chunk = String::new();
            let mut start = 1;
            let mut score = |text: &str, line| {
                let lower = text.to_lowercase();
                let title = d.source.title.to_lowercase();
                let value: usize = terms
                    .iter()
                    .map(|t| {
                        let n = lower.matches(t).count().min(3);
                        if n > 0 {
                            100 + n
                        } else if title.contains(t) {
                            1
                        } else {
                            0
                        }
                    })
                    .sum();
                if value > 0 {
                    candidates.push((value, di, line, text.to_owned()));
                }
            };
            for (i, line) in d.text.lines().enumerate() {
                // Very long lines are segmented at UTF-8 boundaries, not allocated without a bound.
                for part in utf8_chunks(line, 1600) {
                    if chunk.len() + part.len() + 1 > 1800 && !chunk.is_empty() {
                        score(&chunk, start);
                        chunk.clear();
                        start = i + 1;
                    }
                    if chunk.is_empty() {
                        start = i + 1;
                    }
                    chunk.push_str(part);
                    chunk.push('\n');
                }
            }
            if !chunk.is_empty() {
                score(&chunk, start);
            }
        }
        candidates.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        let mut out = Vec::new();
        let mut used = 0;
        for (_, di, line, text) in candidates.into_iter().take(limit) {
            let remaining = budget.saturating_sub(used);
            if remaining < 64 {
                break;
            }
            let text = truncate(&text, remaining).to_owned();
            used += text.len();
            out.push(Snippet {
                source: self.documents[di].source.clone(),
                line,
                text,
            });
        }
        Ok(out)
    }
    pub fn context(&self, query: &str) -> Vec<Snippet> {
        let q = truncate(query, 256);
        self.search(q, 2, 2048).unwrap_or_default()
    }
}
fn truncate(s: &str, n: usize) -> &str {
    let mut end = n.min(s.len());
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}
fn utf8_chunks(mut s: &str, n: usize) -> Vec<&str> {
    let mut parts = Vec::new();
    while !s.is_empty() {
        let p = truncate(s, n);
        parts.push(p);
        s = &s[p.len()..];
    }
    parts
}
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|t| t.as_secs())
        .unwrap_or(0)
}
pub fn import(files: &[PathBuf], profile: Profile, version: &str) -> io::Result<Pack> {
    if files.is_empty() || files.len() > 16 {
        return Err(err("Выбери 1–16 текстовых файлов документации."));
    }
    profile::version_text(version)?;
    let mut pack = Pack {
        schema_version: 1,
        profile,
        target_version: version.into(),
        documents: Vec::new(),
    };
    for (i, path) in files.iter().enumerate() {
        if !path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
            matches!(
                e.to_ascii_lowercase().as_str(),
                "md" | "txt" | "rst" | "c" | "java" | "kt" | "rs"
            )
        }) {
            return Err(err(
                "Импорт: md/txt/rst или выбранный текстовый SDK source.",
            ));
        }
        let text = read_text(path, DOCUMENT_LIMIT)?;
        let title = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("document")
            .to_owned();
        pack.documents.push(Document {
            source: Source {
                id: format!("local-{}", i + 1),
                title: title.clone(),
                origin: format!("local:{title}"),
                version_scope: if version.is_empty() {
                    "user-supplied; version unverified".into()
                } else {
                    format!("user-declared {version}; verify SDK compatibility")
                },
                sha256: digest(text.as_bytes()),
                retrieved_unix: now(),
            },
            text,
        });
    }
    pack.validate()?;
    Ok(pack)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Location {
    pub id: String,
    pub title: String,
    pub url: String,
    pub version_scope: String,
}
pub fn sources(profile: Profile, version: &str) -> io::Result<Vec<Location>> {
    profile::version_text(version)?;
    let mut v = Vec::new();
    let mut add = |id: &str, title: &str, url: &str, scope: &str| {
        v.push(Location {
            id: id.into(),
            title: title.into(),
            url: url.into(),
            version_scope: scope.into(),
        })
    };
    match profile {
        Profile::Rust => add(
            "rust-book",
            "Rust Book",
            "https://doc.rust-lang.org/book/",
            "rolling; match the project's toolchain",
        ),
        Profile::Python => add(
            "python-tutorial",
            "Python tutorial",
            "https://docs.python.org/3/tutorial/",
            "rolling Python 3; exact minor version must be checked",
        ),
        Profile::JavaScript => add(
            "javascript",
            "JavaScript guide",
            "https://developer.mozilla.org/en-US/docs/Web/JavaScript/Guide",
            "rolling; distinguish browser from Node.js",
        ),
        Profile::TypeScript => add(
            "typescript-handbook",
            "TypeScript handbook",
            "https://www.typescriptlang.org/docs/handbook/intro.html",
            "rolling; check TypeScript and runtime versions",
        ),
        Profile::Dotnet => add(
            "dotnet",
            ".NET documentation",
            "https://learn.microsoft.com/en-us/dotnet/core/introduction",
            "rolling; check target framework and SDK",
        ),
        Profile::Java
        | Profile::Kotlin
        | Profile::PaperJava
        | Profile::PaperKotlin
        | Profile::SpigotJava
        | Profile::SpigotKotlin => {
            add(
                "java",
                "Java documentation",
                "https://dev.java/learn/",
                "rolling; check the chosen JDK",
            );
            if profile.kotlin() {
                add(
                    "kotlin",
                    "Kotlin documentation",
                    "https://kotlinlang.org/docs/home.html",
                    "rolling; check the Kotlin compiler/plugin version",
                );
            }
            if matches!(profile, Profile::PaperJava | Profile::PaperKotlin) {
                add(
                    "paper-setup",
                    "Paper project setup",
                    "https://docs.papermc.io/paper/dev/project-setup/",
                    "rolling current Paper; do not copy versions into older targets",
                );
                if !version.is_empty() {
                    add(
                        "paper-plugin-api",
                        "Paper JavaPlugin API",
                        &format!(
                            "https://jd.papermc.io/paper/{version}/org/bukkit/plugin/java/JavaPlugin.html"
                        ),
                        &format!("Paper {version}"),
                    );
                }
            }
            if matches!(profile, Profile::SpigotJava | Profile::SpigotKotlin) {
                add(
                    "spigot-maven",
                    "Spigot Maven setup",
                    "https://www.spigotmc.org/wiki/spigot-maven/",
                    "rolling; verify the requested API version",
                );
                add(
                    "spigot-plugin-api",
                    "Spigot JavaPlugin API",
                    "https://hub.spigotmc.org/javadocs/spigot/org/bukkit/plugin/java/JavaPlugin.html",
                    "rolling, not pinned to the target server version",
                );
            }
        }
        Profile::DayzEnforce => {
            add(
                "dayz-script",
                "DayZ Enforce Script syntax",
                "https://community.bistudio.com/wiki/DayZ:Enforce_Script_Syntax",
                "rolling; verify with the installed DayZ Tools/SDK",
            );
            add(
                "dayz-modding",
                "DayZ modding basics",
                "https://community.bistudio.com/wiki/DayZ:Modding_Basics",
                "rolling; verify with the installed game version",
            );
        }
        Profile::Generic => {}
    }
    Ok(v)
}
#[derive(Args, Debug)]
pub struct DocsArgs {
    #[command(subcommand)]
    pub command: DocsCommand,
}
#[derive(Subcommand, Debug)]
pub enum DocsCommand {
    /// Официальные источники выбранной платформы; без сетевых запросов
    Sources {
        #[arg(long, value_enum)]
        profile: Profile,
        #[arg(long, default_value = "")]
        target_version: String,
    },
    /// Импорт явно выбранных локальных текстов/SDK, без сети
    Import {
        #[arg(long = "file", required = true)]
        files: Vec<PathBuf>,
        #[arg(long, value_enum, default_value = "generic")]
        profile: Profile,
        #[arg(long, default_value = "")]
        target_version: String,
        #[arg(long)]
        output: PathBuf,
    },
    /// Загрузить официальный pack (только с явным --allow-network)
    Fetch {
        #[arg(long, value_enum)]
        profile: Profile,
        #[arg(long, default_value = "")]
        target_version: String,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        allow_network: bool,
    },
    /// Поиск в локальном pack с provenance/номерами строк
    Search {
        #[arg(long)]
        pack: PathBuf,
        #[arg(long)]
        query: String,
    },
}
pub fn run(args: DocsArgs, out: &mut impl Write) -> io::Result<()> {
    match args.command {
        DocsCommand::Sources {
            profile,
            target_version,
        } => serde_json::to_writer_pretty(&mut *out, &sources(profile, &target_version)?)
            .map_err(err)?,
        DocsCommand::Import {
            files,
            profile,
            target_version,
            output,
        } => {
            if output.exists() {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "Pack не перезаписывается: выбери новый путь.",
                ));
            }
            let pack = import(&files, profile, &target_version)?;
            pack.save(&output)?;
            serde_json::to_writer_pretty(&mut *out, &pack.catalog()).map_err(err)?;
        }
        DocsCommand::Fetch {
            profile,
            target_version,
            output,
            allow_network,
        } => {
            if !allow_network {
                return Err(err(
                    "Загрузка требует явного --allow-network; код и задание не отправляются.",
                ));
            }
            if output.exists() {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "Pack не перезаписывается: выбери новый путь.",
                ));
            }
            #[cfg(feature = "online-docs")]
            {
                let pack = online::fetch_blocking(profile, &target_version)?;
                pack.save(&output)?;
                serde_json::to_writer_pretty(&mut *out, &pack.catalog()).map_err(err)?;
            }
            #[cfg(not(feature = "online-docs"))]
            {
                let _ = (profile, target_version, output);
                return Err(err(
                    "Эта offline-сборка не включает загрузку: собери --features online-docs или импортируй локальные файлы.",
                ));
            }
        }
        DocsCommand::Search { pack, query } => {
            serde_json::to_writer_pretty(&mut *out, &Pack::load(&pack)?.search(&query, 4, 8192)?)
                .map_err(err)?
        }
    }
    writeln!(out)
}
