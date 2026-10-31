pub mod agent;
pub mod agent_runtime;
#[cfg(feature = "desktop")]
pub mod desktop;
pub mod doctor;
pub mod inference;
pub mod knowledge;
pub mod model_gateway;
pub mod project;
#[cfg(feature = "gateway-http")]
pub mod provider_http;
pub mod routing;
pub mod skills;
pub mod workspace;

use clap::{Parser, Subcommand};
use std::io::{self, Write};

#[derive(Debug, Parser)]
#[command(
    name = "lyra",
    version,
    about = "Локальная генерация текста и диагностика ПК",
    long_about = "Локальный кодинг-агент: task, предложения правок и явный apply.\nПроизвольные команды модели ещё не реализованы.",
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Создание проектов с нуля: blueprint, каркас, явное создание, контекст
    Project(project::ProjectArgs),
    /// Локальная документация, поиск и явная загрузка официальных источников
    Docs(knowledge::DocsArgs),
    /// Выполнить задачу: чтение проекта, предложения, опциональные подагенты
    Task(agent::TaskArgs),
    /// Применить одно выбранное предложение из завершённого JSON-отчёта
    Apply(agent::ApplyArgs),
    /// Сгенерировать ответ локальной моделью (не исполняет сгенерированный код)
    Generate(inference::GenerateArgs),
    /// Локальный диалог GGUF с сохранением истории ролей, без задач и файлов проекта
    LocalChat {
        #[arg(long)]
        request_file: std::path::PathBuf,
        #[command(flatten)]
        generate: inference::GenerateArgs,
    },
    /// Выполнить Chat Completions через явно разрешённый HTTP-провайдер
    #[cfg(feature = "gateway-http")]
    Chat {
        #[arg(long)]
        provider_config: std::path::PathBuf,
        #[arg(long = "allow", required = true)]
        allowed_providers: Vec<String>,
        #[arg(
            long,
            conflicts_with = "request_file",
            required_unless_present = "request_file"
        )]
        prompt: Option<String>,
        #[arg(long, conflicts_with = "prompt")]
        request_file: Option<std::path::PathBuf>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long, default_value_t = 256)]
        max_output_tokens: u32,
        #[arg(long, default_value_t = 1)]
        max_attempts: usize,
        #[arg(long)]
        skills: Option<std::path::PathBuf>,
        #[arg(long = "skill")]
        selected_skills: Vec<String>,
    },
    /// Проверить маршрутизацию моделей без отправки данных провайдерам
    Route {
        #[arg(long)]
        models: std::path::PathBuf,
        #[arg(long = "allow", required = true)]
        allowed_providers: Vec<String>,
        #[arg(long)]
        prompt: String,
        #[arg(long, default_value_t = 256)]
        max_output_tokens: u32,
        #[arg(long)]
        require_tools: bool,
    },
    /// Просмотреть проверенный набор навыков ECC-совместимого формата JSON
    Skills {
        #[arg(long)]
        manifest: std::path::PathBuf,
        #[arg(long)]
        id: Option<String>,
    },
    /// Показать характеристики компьютера, на котором запущена Lyra
    Doctor {
        /// Вывести JSON (схема v1), пригодный для подбора локальной модели
        #[arg(long)]
        json: bool,
    },
}

pub fn run(cli: Cli, output: &mut impl Write) -> io::Result<()> {
    match cli.command {
        Command::Project(args) => project::run(args, output),
        Command::Docs(args) => knowledge::run(args, output),
        Command::Task(args) => agent::task(args, output),
        Command::Apply(args) => agent::apply(args, output),
        Command::Generate(args) => {
            if args.selected_skills.is_empty() && args.skills.is_none() {
                inference::generate(&args, output)
            } else {
                let pack = args
                    .skills
                    .as_ref()
                    .map(|path| {
                        if path.file_name().is_some_and(|name| name == "SKILL.md") {
                            skills::SkillPack::load_markdown(path)
                        } else {
                            skills::SkillPack::load(path)
                        }
                    })
                    .transpose()?;
                let request = model_gateway::Request {
                    messages: vec![model_gateway::Message {
                        role: model_gateway::Role::User,
                        content: args.prompt.clone(),
                    }],
                    max_output_tokens: args.max_tokens,
                    require_tools: false,
                };
                let response = agent_runtime::complete_gguf(
                    &args,
                    &request,
                    pack.as_ref(),
                    &args.selected_skills,
                )?;
                write!(output, "{}", response.text)
            }
        }
        Command::LocalChat {
            request_file,
            generate,
        } => {
            use std::io::Read;
            let mut bytes = Vec::new();
            std::fs::File::open(request_file)?
                .take(256 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() > 256 * 1024 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Local chat request exceeds 256 KiB",
                ));
            }
            let request: model_gateway::Request = serde_json::from_slice(&bytes).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "Invalid local chat request")
            })?;
            let pack = generate
                .skills
                .as_ref()
                .map(|path| {
                    if path.file_name().is_some_and(|name| name == "SKILL.md") {
                        skills::SkillPack::load_markdown(path)
                    } else {
                        skills::SkillPack::load(path)
                    }
                })
                .transpose()?;
            let response = agent_runtime::complete_gguf(
                &generate,
                &request,
                pack.as_ref(),
                &generate.selected_skills,
            )?;
            write!(output, "{}", response.text)
        }
        #[cfg(feature = "gateway-http")]
        Command::Chat {
            provider_config,
            allowed_providers,
            prompt,
            request_file,
            model,
            max_output_tokens,
            max_attempts,
            skills,
            selected_skills,
        } => {
            use std::io::Read;
            let mut bytes = Vec::new();
            std::fs::File::open(provider_config)?
                .take(64 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            let mut configs = provider_http::parse_registry(&bytes)?;
            if let Some(model) = &model {
                configs.retain(|config| &config.descriptor.model == model);
            }
            let request = if let Some(path) = request_file {
                let mut bytes = Vec::new();
                std::fs::File::open(path)?
                    .take(256 * 1024 + 1)
                    .read_to_end(&mut bytes)?;
                if bytes.len() > 256 * 1024 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Chat request exceeds 256 KiB",
                    ));
                }
                serde_json::from_slice::<model_gateway::Request>(&bytes).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "Invalid chat request")
                })?
            } else {
                model_gateway::Request {
                    messages: vec![model_gateway::Message {
                        role: model_gateway::Role::User,
                        content: prompt.unwrap_or_default(),
                    }],
                    max_output_tokens,
                    require_tools: false,
                }
            };
            let pack = skills
                .as_ref()
                .map(|path| {
                    if path.file_name().is_some_and(|name| name == "SKILL.md") {
                        skills::SkillPack::load_markdown(path)
                    } else {
                        skills::SkillPack::load(path)
                    }
                })
                .transpose()?;
            let request =
                agent_runtime::prepare_request(&request, pack.as_ref(), &selected_skills)?;
            let mut backends: Vec<provider_http::OpenAiCompatibleBackend> = configs
                .into_iter()
                .map(provider_http::OpenAiCompatibleBackend::new)
                .collect::<io::Result<_>>()?;
            let mut references: Vec<&mut dyn model_gateway::ModelBackend> = backends
                .iter_mut()
                .map(|backend| backend as &mut dyn model_gateway::ModelBackend)
                .collect();
            let response = routing::complete_with_fallback(
                &mut references,
                &routing::RoutingPolicy {
                    allowed_providers,
                    max_attempts,
                },
                &request,
            )?;
            writeln!(output, "{}", response.text)
        }
        Command::Route {
            models,
            allowed_providers,
            prompt,
            max_output_tokens,
            require_tools,
        } => {
            use std::io::Read;
            let mut bytes = Vec::new();
            std::fs::File::open(models)?
                .take(64 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() > 64 * 1024 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Model registry exceeds 64 KiB",
                ));
            }
            let descriptors: Vec<model_gateway::ModelDescriptor> =
                serde_json::from_slice(&bytes).map_err(io::Error::other)?;
            let request = model_gateway::Request {
                messages: vec![model_gateway::Message {
                    role: model_gateway::Role::User,
                    content: prompt,
                }],
                max_output_tokens,
                require_tools,
            };
            let selected =
                model_gateway::select_priority(&descriptors, &allowed_providers, &request)?;
            writeln!(output, "{}/{}", selected.provider, selected.model)
        }
        Command::Skills { manifest, id } => {
            let pack = if manifest.file_name().is_some_and(|name| name == "SKILL.md") {
                skills::SkillPack::load_markdown(&manifest)?
            } else {
                skills::SkillPack::load(&manifest)?
            };
            if let Some(id) = id {
                let skill = pack
                    .find(&id)
                    .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Skill not found"))?;
                writeln!(output, "{}", skill.instructions)
            } else {
                for skill in &pack.skills {
                    writeln!(output, "{}: {}", skill.id, skill.description)?;
                }
                Ok(())
            }
        }
        Command::Doctor { json } => {
            let report = doctor::collect()?;
            doctor::write_report(&report, json, output)
        }
    }
}
