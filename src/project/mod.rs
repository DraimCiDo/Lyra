//! New-project plans are data. Materialization never executes generated code.
mod planner;
pub mod profile;
mod templates;
use crate::knowledge::{self, Source};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use clap::{Args, Subcommand};
pub use planner::{
    PLAN_GRAMMAR, PlanArgs, PlanRequest, Planner, plan_grammar, plan_with_constraints,
    plan_with_model,
};
use profile::Profile;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    io::{self, Write},
    path::{Path, PathBuf},
};

pub const PLAN_LIMIT: usize = 768 * 1024;
pub const FILE_COUNT: usize = 32;
pub const CONTENT_LIMIT: usize = 512 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlannedFile {
    pub path: String,
    pub content: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Blueprint {
    pub schema_version: u32,
    pub name: String,
    pub profile: Profile,
    pub target_version: String,
    pub summary: String,
    pub requirements: Vec<String>,
    pub assumptions: Vec<String>,
    pub questions: Vec<String>,
    pub files: Vec<PlannedFile>,
    #[serde(default)]
    pub required_files: Vec<String>,
    /// Application-owned provenance, not a model's claim to have verified an API.
    #[serde(default)]
    pub provided_sources: Vec<Source>,
}
#[derive(Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct ProjectInfo {
    pub schema_version: u32,
    pub name: String,
    pub profile: Profile,
    pub target_version: String,
    pub summary: String,
    pub manual_checks: Vec<String>,
    pub compiled: bool,
}
#[derive(Args, Debug)]
pub struct ProjectArgs {
    #[command(subcommand)]
    pub command: ProjectCommand,
}
#[derive(Subcommand, Debug)]
pub enum ProjectCommand {
    /// Предложить AI blueprint (JSON), без создания файлов
    Plan(PlanArgs),
    /// Подготовить проверенный стартовый каркас (JSON), без модели/записи
    Starter(StarterArgs),
    /// Создать только новую папку из просмотренного blueprint
    Create {
        #[arg(long)]
        plan: PathBuf,
        #[arg(long)]
        parent: PathBuf,
    },
    /// Определить контекст проекта и показать ручные проверки (без исполнения)
    Inspect {
        #[arg(long)]
        repo: PathBuf,
    },
}
#[derive(Args, Debug)]
pub struct StarterArgs {
    #[arg(long)]
    pub name: String,
    #[arg(long, value_enum)]
    pub profile: Profile,
    #[arg(long, default_value = "")]
    pub target_version: String,
    #[arg(long, default_value = "")]
    pub task: String,
}
fn err(message: impl std::fmt::Display) -> io::Error {
    io::Error::other(message.to_string())
}
pub fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes().next().is_some_and(|c| c.is_ascii_lowercase())
        && s.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"-_".contains(&c))
        && !reserved(s)
}
fn reserved(name: &str) -> bool {
    let n = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        n.as_str(),
        "con"
            | "conin$"
            | "conout$"
            | "com¹"
            | "com²"
            | "com³"
            | "lpt¹"
            | "lpt²"
            | "lpt³"
            | "prn"
            | "aux"
            | "nul"
            | "com1"
            | "com2"
            | "com3"
            | "com4"
            | "com5"
            | "com6"
            | "com7"
            | "com8"
            | "com9"
            | "lpt1"
            | "lpt2"
            | "lpt3"
            | "lpt4"
            | "lpt5"
            | "lpt6"
            | "lpt7"
            | "lpt8"
            | "lpt9"
    )
}
pub fn validate_path(path: &str, dotfiles: bool) -> io::Result<()> {
    if path.is_empty()
        || path.len() > 240
        || path.contains(['\\', ':'])
        || path.chars().any(char::is_control)
        || path.starts_with('/')
    {
        return Err(err("Недопустимый относительный путь."));
    }
    let components: Vec<_> = path.split('/').collect();
    if components.len() > 12 {
        return Err(err("Слишком глубокий путь."));
    }
    for part in components {
        let lower = part.to_lowercase();
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.ends_with(['.', ' '])
            || part.contains(['<', '>', '"', '|', '?', '*'])
            || reserved(part)
            || lower == ".git"
            || lower == ".lyra"
            || lower == "lyra.project.json"
            || lower == "lyra.blueprint.json"
            || lower == ".env"
            || lower.starts_with(".env.") && lower != ".env.example"
            || [".pem", ".key", ".p12", ".pfx", ".exe", ".dll", ".gguf"]
                .iter()
                .any(|e| lower.ends_with(e))
            || part.starts_with('.')
                && (!dotfiles
                    || !matches!(
                        lower.as_str(),
                        ".gitignore" | ".gitattributes" | ".editorconfig" | ".env.example"
                    ))
        {
            return Err(err(format!("Путь запрещён: {path}")));
        }
    }
    Ok(())
}
impl Blueprint {
    pub fn validate(&self) -> io::Result<()> {
        if self.schema_version != 1
            || !valid_name(&self.name)
            || self.summary.len() > 4096
            || self.files.len() > FILE_COUNT
            || self.provided_sources.len() > 16
        {
            return Err(err("Неподдерживаемый или слишком большой blueprint."));
        }
        profile::version_text(&self.target_version)?;
        for items in [&self.requirements, &self.assumptions, &self.questions] {
            if items.len() > 16 || items.iter().any(|x| x.len() > 2048) {
                return Err(err("Слишком длинные требования/допущения/вопросы."));
            }
        }
        let mut paths = BTreeSet::new();
        let mut total = 0;
        for file in &self.files {
            validate_path(&file.path, true)?;
            if file.content.len() > crate::workspace::FILE_LIMIT
                || file
                    .content
                    .chars()
                    .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
                || !paths.insert(file.path.to_lowercase())
            {
                return Err(err("Повтор пути, двоичный или слишком большой файл."));
            }
            total += file.content.len();
            if total > CONTENT_LIMIT {
                return Err(err("План превышает 512 KiB."));
            }
        }
        for path in &paths {
            let mut prefix = String::new();
            for part in path.split('/').take(path.split('/').count() - 1) {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(part);
                if paths.contains(&prefix) {
                    return Err(err("Один путь одновременно файл и каталог."));
                }
            }
        }
        if self.required_files.len() > FILE_COUNT {
            return Err(err("Слишком много обязательных файлов."));
        }
        let mut required = BTreeSet::new();
        for path in &self.required_files {
            validate_path(path, true)?;
            if !required.insert(path.to_lowercase()) {
                return Err(err("Повтор обязательного пути."));
            }
        }
        for source in &self.provided_sources {
            source.validate()?;
        }
        if serde_json::to_vec(self).map_err(err)?.len() > PLAN_LIMIT {
            return Err(err("Сериализованный blueprint превышает лимит."));
        }
        Ok(())
    }
    pub fn ready(&self) -> io::Result<()> {
        self.validate()?;
        if !self.questions.is_empty() || self.files.is_empty() {
            return Err(err(
                "Ответь на вопросы/дополни план перед созданием проекта.",
            ));
        }
        for path in &self.required_files {
            if !self.files.iter().any(|f| f.path == *path) {
                return Err(err(format!("Отсутствует обязательный файл: {path}")));
            }
        }
        Ok(())
    }
}
pub fn starter(args: &StarterArgs) -> io::Result<Blueprint> {
    templates::starter(args)
}
pub fn read_plan(path: &Path) -> io::Result<Blueprint> {
    let p: Blueprint =
        serde_json::from_str(&knowledge::read_text(path, PLAN_LIMIT)?).map_err(err)?;
    p.validate()?;
    Ok(p)
}
pub fn create(plan: &Blueprint, parent: &Path) -> io::Result<ProjectInfo> {
    plan.ready()?;
    let parent = parent.canonicalize()?;
    let dir = Dir::open_ambient_dir(&parent, ambient_authority())?;
    // create_dir is exclusive on both Unix and Windows. No rename-over-empty-directory race.
    dir.create_dir(&plan.name)?;
    let target = parent.join(&plan.name);
    let result = (|| {
        let project = dir.open_dir(&plan.name)?;
        for file in &plan.files {
            if let Some(parent) = Path::new(&file.path)
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
            {
                project.create_dir_all(parent)?;
            }
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            let mut output = project.open_with(&file.path, &options)?;
            output.write_all(file.content.as_bytes())?;
            output.sync_all()?;
        }
        let info = ProjectInfo {
            schema_version: 1,
            name: plan.name.clone(),
            profile: plan.profile,
            target_version: plan.target_version.clone(),
            summary: plan.summary.clone(),
            manual_checks: plan.profile.checks(),
            compiled: false,
        };
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        let mut file = project.open_with("lyra.project.json", &options)?;
        serde_json::to_writer_pretty(&mut file, &info).map_err(err)?;
        file.sync_all()?;
        Ok(info)
    })();
    // Never recursively delete a new directory on failure: another process may have added files.
    result.map_err(|e: io::Error| {
        err(format!(
            "Неполный проект сохранён для проверки: {}: {e}",
            target.display()
        ))
    })
}
pub fn run(args: ProjectArgs, out: &mut impl Write) -> io::Result<()> {
    match args.command {
        ProjectCommand::Plan(args) => return planner::run(args, out),
        ProjectCommand::Starter(args) => {
            serde_json::to_writer_pretty(&mut *out, &starter(&args)?).map_err(err)?
        }
        ProjectCommand::Create { plan, parent } => {
            serde_json::to_writer_pretty(&mut *out, &create(&read_plan(&plan)?, &parent)?)
                .map_err(err)?
        }
        ProjectCommand::Inspect { repo } => {
            let mut workspace = crate::workspace::Workspace::open(&repo, &[])?;
            serde_json::to_writer_pretty(&mut *out, &inspect(&mut workspace)?).map_err(err)?;
        }
    }
    writeln!(out)
}
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Context {
    pub profile: Profile,
    pub target_version: String,
    pub evidence: Vec<String>,
    pub warnings: Vec<String>,
    pub manual_checks: Vec<String>,
}
pub fn inspect(workspace: &mut crate::workspace::Workspace) -> io::Result<Context> {
    let mut context = Context::default();
    if workspace.files.contains("lyra.project.json") {
        match workspace
            .read("lyra.project.json")
            .and_then(|t| serde_json::from_str::<ProjectInfo>(&t).map_err(err))
        {
            Ok(info)
                if info.schema_version == 1
                    && profile::version_text(&info.target_version).is_ok() =>
            {
                context.profile = info.profile;
                context.target_version = info.target_version;
                context
                    .evidence
                    .push("lyra.project.json (untrusted project metadata)".into());
            }
            _ => context
                .warnings
                .push("Project metadata is invalid; use repository evidence.".into()),
        }
    }
    if context.profile == Profile::Generic {
        let mut candidates = Vec::new();
        if workspace
            .files
            .iter()
            .any(|p| !p.contains('/') && p.ends_with(".csproj"))
        {
            candidates.push((Profile::Dotnet, ".csproj"));
        }
        if workspace.files.contains("Cargo.toml") {
            candidates.push((Profile::Rust, "Cargo.toml"));
        }
        if workspace.files.contains("pyproject.toml")
            || workspace.files.contains("requirements.txt")
        {
            candidates.push((Profile::Python, "Python manifest"));
        }
        if workspace.files.contains("package.json") {
            let ts = workspace.files.contains("tsconfig.json")
                || workspace
                    .files
                    .iter()
                    .any(|p| p.ends_with(".ts") || p.ends_with(".tsx"));
            candidates.push((
                if ts {
                    Profile::TypeScript
                } else {
                    Profile::JavaScript
                },
                "package.json (runtime needs confirmation)",
            ));
        }
        if let Some(config) = workspace
            .files
            .iter()
            .find(|p| p.eq_ignore_ascii_case("config.cpp"))
            .cloned()
            && workspace.files.iter().any(|p| {
                p.to_ascii_lowercase().starts_with("scripts/")
                    && p.to_ascii_lowercase().ends_with(".c")
            })
        {
            let is_dayz = workspace
                .read(&config)
                .is_ok_and(|t| t.to_ascii_lowercase().contains("dz_"));
            if is_dayz {
                candidates.push((
                    Profile::DayzEnforce,
                    "config.cpp + DZ_ addons + Enforce scripts",
                ));
            } else {
                context.warnings.push("Enforce-like scripts detected; confirm the game/SDK (DayZ and Reforger APIs differ).".into());
            }
        }
        for name in ["pom.xml", "build.gradle.kts", "build.gradle"] {
            if workspace.files.contains(name)
                && let Ok(text) = workspace.read(name)
            {
                let kotlin = text.contains("kotlin");
                let p = if text.contains("paper-api") {
                    if kotlin {
                        Profile::PaperKotlin
                    } else {
                        Profile::PaperJava
                    }
                } else if text.contains("spigot-api") {
                    if kotlin {
                        Profile::SpigotKotlin
                    } else {
                        Profile::SpigotJava
                    }
                } else if kotlin {
                    Profile::Kotlin
                } else {
                    Profile::Java
                };
                candidates.push((p, name));
            }
        }
        candidates.sort_by_key(|(p, _)| *p as u8);
        candidates.dedup_by_key(|(p, _)| *p);
        if candidates.len() == 1 {
            context.profile = candidates[0].0;
        } else if candidates.len() > 1 {
            context.warnings.push(
                "Mixed project: select a target explicitly; no automatic language assumption."
                    .into(),
            );
        }
        context
            .evidence
            .extend(candidates.into_iter().map(|(_, s)| s.to_owned()));
    }
    context.manual_checks = context.profile.checks();
    Ok(context)
}
