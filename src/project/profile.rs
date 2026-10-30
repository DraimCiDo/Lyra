//! Target hints are evidence, not a promise that a task or API is supported.
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::io;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Profile {
    #[default]
    Generic,
    Rust,
    Python,
    #[serde(rename = "javascript")]
    #[value(name = "javascript")]
    JavaScript,
    #[serde(rename = "typescript")]
    #[value(name = "typescript")]
    TypeScript,
    Java,
    Kotlin,
    Dotnet,
    PaperJava,
    PaperKotlin,
    SpigotJava,
    SpigotKotlin,
    DayzEnforce,
}
impl Profile {
    pub const ALL: [Self; 13] = [
        Self::Generic,
        Self::Rust,
        Self::Python,
        Self::JavaScript,
        Self::TypeScript,
        Self::Java,
        Self::Kotlin,
        Self::Dotnet,
        Self::PaperJava,
        Self::PaperKotlin,
        Self::SpigotJava,
        Self::SpigotKotlin,
        Self::DayzEnforce,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Generic => "Другой / выбор AI",
            Self::Rust => "Rust",
            Self::Python => "Python",
            Self::JavaScript => "JavaScript",
            Self::TypeScript => "TypeScript",
            Self::Java => "Java",
            Self::Kotlin => "Kotlin",
            Self::Dotnet => ".NET / C#",
            Self::PaperJava => "Paper / Java",
            Self::PaperKotlin => "Paper / Kotlin",
            Self::SpigotJava => "Spigot / Java",
            Self::SpigotKotlin => "Spigot / Kotlin",
            Self::DayzEnforce => "DayZ / Enforce Script",
        }
    }
    pub fn minecraft(self) -> bool {
        matches!(
            self,
            Self::PaperJava | Self::PaperKotlin | Self::SpigotJava | Self::SpigotKotlin
        )
    }
    pub fn kotlin(self) -> bool {
        matches!(self, Self::Kotlin | Self::PaperKotlin | Self::SpigotKotlin)
    }
    pub fn starter(self) -> bool {
        matches!(
            self,
            Self::Rust
                | Self::Python
                | Self::TypeScript
                | Self::PaperJava
                | Self::PaperKotlin
                | Self::SpigotJava
                | Self::SpigotKotlin
                | Self::DayzEnforce
        )
    }
    pub fn guidance(self) -> &'static str {
        match self {
            Self::Generic => {
                "Choose a stack from the actual requirements and platform. Ask questions for material unknowns. Do not default every project to Rust. Define measurable acceptance criteria and keep the first implementation small."
            }
            Self::Rust => {
                "Use Cargo and Rust's standard library first. No optimization claims without measurements. Avoid unnecessary unsafe and dependencies. Add executable tests for behavior."
            }
            Self::Python => {
                "Use pyproject.toml when dependencies or packaging are needed. Prefer the standard library for small tools. Record the required Python version and test command."
            }
            Self::JavaScript | Self::TypeScript => {
                "Distinguish browser, Node.js and other runtimes. Record runtime and package versions. Do not invent package versions; ask when they cannot be established."
            }
            Self::Java | Self::Kotlin => {
                "Record JDK and build tool versions. Use documentation matching the actual dependencies. Kotlin requires a strategy for distributing its runtime."
            }
            Self::Dotnet => {
                "Determine .NET target framework, OS and application type before selecting libraries. Record the SDK and deployment requirements."
            }
            Self::PaperJava | Self::PaperKotlin | Self::SpigotJava | Self::SpigotKotlin => {
                "Identify Paper versus Spigot, exact Minecraft/API version and required JDK. Paper-only APIs do not work on Spigot. API dependencies are provided, not bundled. Kotlin runtime must be bundled or otherwise explicitly provided. Do not invent API methods. A compiled plugin still needs a real server acceptance test."
            }
            Self::DayzEnforce => {
                "DayZ scripts use Enforce Script, not C/C++ even though source files end in .c. Distinguish config.cpp, mod.cpp and scripts/3_Game,4_World,5_Mission. Check against the user's actual DayZ Tools/SDK scripts and target game version. Do not substitute Arma Reforger APIs or claim that a Rust compiler validates Enforce Script."
            }
        }
    }
    pub fn checks(self) -> Vec<String> {
        match self {
            Self::Rust => vec![
                "cargo test --offline".into(),
                "cargo clippy --all-targets -- -D warnings".into(),
            ],
            Self::Python => vec!["python -m unittest discover -s tests".into()],
            Self::JavaScript => vec!["Review package.json and run the declared runtime test command (for example npm test)".into()],
            Self::TypeScript => vec![
                "node --test tests/greeting.test.ts (starter requires Node.js 22.18+)".into(),
                "Run a separately configured, pinned TypeScript type checker".into(),
            ],
            Self::PaperJava
            | Self::PaperKotlin
            | Self::SpigotJava
            | Self::SpigotKotlin
            | Self::Java
            | Self::Kotlin => vec![
                "mvn --batch-mode verify (requires the recorded JDK, Maven and dependencies)"
                    .into(),
                "Run acceptance tests on the exact target server/runtime".into(),
            ],
            Self::Dotnet => vec!["dotnet test".into()],
            Self::DayzEnforce => vec![
                "Validate config and compile scripts in the matching DayZ Tools/Workbench".into(),
                "Pack/sign and load on a separate test client/server".into(),
            ],
            Self::Generic => vec![
                "Review requirements, choose runtime/SDK and define executable acceptance tests"
                    .into(),
            ],
        }
    }
}
pub fn infer(task: &str) -> Profile {
    let s = task.to_lowercase();
    let words: Vec<_> = s
        .split(|c: char| !c.is_alphanumeric() && !matches!(c, '#' | '.'))
        .collect();
    let has = |word: &str| words.contains(&word);
    let kotlin = has("kotlin") || has("котлин");
    if has("dayz") || s.contains("day z") || has("enforce") {
        Profile::DayzEnforce
    } else if has("spigot") {
        if kotlin {
            Profile::SpigotKotlin
        } else {
            Profile::SpigotJava
        }
    } else if has("paper")
        && (has("minecraft") || s.contains("плагин") || s.contains("plugin") || s.contains("api"))
    {
        if kotlin {
            Profile::PaperKotlin
        } else {
            Profile::PaperJava
        }
    } else if has("minecraft") || has("майнкрафт") {
        Profile::Generic
    } else if has("rust") || has("раст") {
        Profile::Rust
    } else if has("python") || has("питон") {
        Profile::Python
    } else if has("javascript") || has("джаваскрипт") {
        Profile::JavaScript
    } else if has("typescript") || has("типскрипт") {
        Profile::TypeScript
    } else if kotlin {
        Profile::Kotlin
    } else if has("java") {
        Profile::Java
    } else if has(".net") || has("c#") {
        Profile::Dotnet
    } else {
        Profile::Generic
    }
}
pub fn version_text(version: &str) -> io::Result<()> {
    if version.len() > 80
        || !version
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b".-_+".contains(&c))
    {
        return Err(io::Error::other(
            "Версия: до 80 символов ASCII, без пробелов/URL/команд.",
        ));
    }
    Ok(())
}
/// Starter recipes intentionally support a narrow, documented JDK 21 range.
/// AI blueprints can target other versions, with their own evidence and acceptance.
pub fn minecraft_version(version: &str) -> io::Result<()> {
    if !matches!(
        version,
        "1.20.6"
            | "1.21"
            | "1.21.1"
            | "1.21.2"
            | "1.21.3"
            | "1.21.4"
            | "1.21.5"
            | "1.21.6"
            | "1.21.7"
            | "1.21.8"
            | "1.21.9"
            | "1.21.10"
            | "1.21.11"
    ) {
        return Err(io::Error::other(
            "Каркас Minecraft рассчитан на 1.20.6–1.21.11 / JDK 21. Для 26.x и других версий используй AI blueprint с точными зависимостями и документацией.",
        ));
    }
    Ok(())
}
