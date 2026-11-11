//! Native shell around the same CLI contracts. No inference or filesystem policy in widgets.
mod chat;
mod chat_policy;
mod diff;
mod dock;
mod graph_view;
mod history;
mod icons;
mod job;
mod kit;
mod models;
mod preferences;
mod projects;
mod settings;
mod settings_ui;
mod shell;
mod skills;
mod theme;
pub mod updater;
mod workflows;
mod workspace;
use crate::agent::Report;
#[cfg(feature = "gateway-http")]
use crate::github;
use eframe::egui::{self, Color32, RichText};
use icons::Icon;
use job::{Job, ResultData};
use preferences::Preferences;
use std::{
    ffi::OsString,
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
    time::{Duration, Instant},
};
use theme::{Language, Palette};

const WINDOW_SIZE: [f32; 2] = [1160.0, 760.0];
const MIN_SIZE: [f32; 2] = [820.0, 600.0];
const SETTINGS_CYCLE: usize = SettingsPage::ALL.len() + 2;
const SMOKE_STEPS: usize = SETTINGS_CYCLE * Palette::ALL.len() * 2;

pub fn worker_main() -> ExitCode {
    use clap::Parser;
    let args = std::env::args_os()
        .take(1)
        .chain(std::env::args_os().skip(2));
    let cli = match crate::Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            let _ = error.print();
            return ExitCode::from(error.exit_code() as u8);
        }
    };
    match crate::run(cli, &mut io::stdout().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(io::stderr().lock(), "Ошибка: {error}");
            ExitCode::from(match error.kind() {
                io::ErrorKind::Interrupted => 130,
                io::ErrorKind::TimedOut => 124,
                _ => 1,
            })
        }
    }
}

pub fn launch() -> eframe::Result {
    run_window(false)
}

/// Opens and renders a native window, then closes it; used by native CI.
pub fn smoke_test() -> eframe::Result {
    run_window(true)
}

fn run_window(smoke: bool) -> eframe::Result {
    let frames = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = frames.clone();
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size(WINDOW_SIZE)
        .with_min_inner_size(MIN_SIZE);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/app.png")) {
        viewport = viewport.with_icon(icon);
    }
    eframe::run_native(
        "Lyra",
        eframe::NativeOptions {
            viewport,
            #[cfg(not(windows))]
            renderer: eframe::Renderer::Glow,
            #[cfg(windows)]
            renderer: eframe::Renderer::Wgpu,
            ..Default::default()
        },
        Box::new(move |cc| {
            configure(&cc.egui_ctx);
            let mut app = if smoke {
                Desktop::default()
            } else {
                Desktop::load()
            };
            app.smoke = smoke.then_some(counter);
            Ok(Box::new(app))
        }),
    )?;
    if smoke && frames.load(std::sync::atomic::Ordering::Relaxed) < SMOKE_STEPS {
        return Err(eframe::Error::AppCreation(Box::new(io::Error::other(
            "Не отрисованы все состояния компактного интерфейса.",
        ))));
    }
    Ok(())
}

fn configure(ctx: &egui::Context) {
    egui_extras::install_image_loaders(ctx);
    theme::configure(ctx, Palette::Violet, true);
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SettingsPage {
    Model,
    #[cfg(feature = "gateway-http")]
    Providers,
    Context,
    Workers,
    Checks,
    Github,
    Skills,
    Knowledge,
    Appearance,
    Hardware,
    Updates,
}
impl SettingsPage {
    const ALL: [Self; 10 + cfg!(feature = "gateway-http") as usize] = [
        Self::Model,
        #[cfg(feature = "gateway-http")]
        Self::Providers,
        Self::Context,
        Self::Workers,
        Self::Checks,
        Self::Github,
        Self::Skills,
        Self::Knowledge,
        Self::Appearance,
        Self::Hardware,
        Self::Updates,
    ];
    /// Navigation groups in display order.
    const GROUPS: [&'static str; 4] = ["Работа", "Модели", "Знания", "Приложение"];
    fn group(self) -> &'static str {
        match self {
            Self::Context | Self::Workers | Self::Checks | Self::Github => "Работа",
            Self::Model => "Модели",
            #[cfg(feature = "gateway-http")]
            Self::Providers => "Модели",
            Self::Skills | Self::Knowledge => "Знания",
            Self::Appearance | Self::Hardware | Self::Updates => "Приложение",
        }
    }
    fn title(self) -> &'static str {
        match self {
            Self::Model => "Локальная модель",
            #[cfg(feature = "gateway-http")]
            Self::Providers => "HTTP-провайдеры",
            Self::Context => "Проект и контекст",
            Self::Workers => "Агенты",
            Self::Checks => "Проверки и среды",
            Self::Github => "GitHub",
            Self::Skills => "Навыки ECC",
            Self::Appearance => "Оформление",
            Self::Hardware => "Мой компьютер",
            Self::Updates => "Обновления",
            Self::Knowledge => "Документация",
        }
    }
    fn icon(self) -> Icon {
        match self {
            Self::Model => Icon::Cpu,
            #[cfg(feature = "gateway-http")]
            Self::Providers => Icon::Model,
            Self::Context => Icon::Folder,
            Self::Workers => Icon::Workers,
            Self::Checks => Icon::Check,
            Self::Github => Icon::Graph,
            Self::Skills => Icon::Sparkles,
            Self::Appearance => Icon::Palette,
            Self::Hardware => Icon::Monitor,
            Self::Updates => Icon::Download,
            Self::Knowledge => Icon::BookOpen,
        }
    }
    /// Extra words matched by the settings search.
    fn keywords(self) -> &'static str {
        match self {
            Self::Model => "gguf cuda gpu cpu память модель загрузка скачать",
            #[cfg(feature = "gateway-http")]
            Self::Providers => "api openai http ключ провайдер облако",
            Self::Context => "файлы история токены дедупликация оптимизация проект папка",
            Self::Workers => "подагенты ревьюер исследователь лимит вызовов граф",
            Self::Checks => "docker тесты проверка среды скрипты документы сессии восстановление",
            Self::Github => "github issue pull request pr токен",
            Self::Skills => "ecc навыки skills пакет откат",
            Self::Appearance => "тема язык анимации цвет",
            Self::Hardware => "диагностика ram vram nvidia",
            Self::Updates => "версия релиз обновить",
            Self::Knowledge => "документы справка источники docs",
        }
    }
    fn description(self) -> &'static str {
        match self {
            Self::Model => {
                "Выберите GGUF и устройство для локальной модели. Для облачных моделей используйте HTTP-провайдеры."
            }
            #[cfg(feature = "gateway-http")]
            Self::Providers => {
                "Подключение внешних AI-сервисов. Запросы отправляются только при включении соответствующего режима."
            }
            Self::Context => "Папка проекта, файлы контекста и объём истории, передаваемой модели.",
            Self::Workers => {
                "Дополнительные роли для задач проекта и общий лимит их обращений к модели."
            }
            Self::Checks => {
                "Изолированные Docker-проверки, среды выполнения скриптов и сохранение сессий."
            }
            Self::Github => "Issues как задачи, публикация предложений в виде черновика PR.",
            Self::Skills => {
                "Инструкции ECC для модели: выбор, просмотр, локальная установка и откат пакетов."
            }
            Self::Appearance => "Тема, язык и анимации интерфейса.",
            Self::Hardware => "Проверьте доступные CPU, RAM и NVIDIA GPU перед выбором модели.",
            Self::Updates => {
                "Версия приложения и обновления. CPU и CUDA поставляются отдельными пакетами."
            }
            Self::Knowledge => {
                "Подключите справочные материалы проекта. Это источники для агента, а не веса модели."
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum DockTab {
    #[default]
    Changes,
    Checks,
    Plan,
    Log,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
enum ChatMode {
    #[default]
    Chat,
    Task,
}

#[derive(Clone, Copy, PartialEq)]
enum Operation {
    Task,
    LocalChat,
    #[cfg(feature = "gateway-http")]
    CloudChat,
    Apply(usize),
    ApplyAll,
    RecoveryStatus,
    RecoveryRollback,
    RecoveryComplete,
    SessionList,
    EnvironmentPrepare,
    Doctor,
    ProjectPlan,
    ProjectCreate,
    DocsFetch,
    DocsImport,
}

struct Desktop {
    artifact_temp: Option<tempfile::TempDir>,
    artifact_preview: Option<(String, egui::TextureHandle)>,
    failed_artifact_preview: Option<String>,
    environment_prepare_id: String,
    conversations: history::History,
    history_available: bool,
    model_installer: models::Installer,
    skill_catalog: Option<Result<crate::skills::SkillPack, String>>,
    skill_package_ui: skills::PackageUi,
    chat: chat::Chat,
    chat_mode: ChatMode,
    activity: job::ProgressSnapshot,
    activity_elapsed: Duration,
    draft: projects::ProjectDraft,
    request_task: String,
    #[cfg_attr(not(feature = "gateway-http"), allow(dead_code))]
    graph_open: bool,
    sessions_open: bool,
    dock_open: bool,
    dock_tab: DockTab,
    palette_open: bool,
    focus_composer: bool,
    palette_query: String,
    settings_query: String,
    graph: graph_view::GraphView,
    #[cfg_attr(not(feature = "gateway-http"), allow(dead_code))]
    publish_flag: std::cell::Cell<bool>,
    github_rx: Option<std::sync::mpsc::Receiver<io::Result<GithubOut>>>,
    knowledge_ui: projects::KnowledgeUi,
    updates: updater::Controller,
    update_confirm: bool,
    real_app: bool,
    prefs: Preferences,
    last_attempted: Preferences,
    storage: Option<PathBuf>,
    preference_error: Option<String>,
    dirty_since: Option<Instant>,
    style_key: Option<(Palette, bool, Language)>,
    settings_open: bool,
    settings_page: SettingsPage,
    transition: Option<Instant>,
    help_open: bool,
    smoke: Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>,
    task: String,
    job: Option<Job>,
    operation: Operation,
    report: Option<Report>,
    report_json: Vec<u8>,
    applied: Vec<usize>,
    selected: usize,
    message: String,
    log: String,
    hardware: Option<serde_json::Value>,
    confirm: Option<Operation>,
    recovery_open: bool,
    recovery_root: Option<PathBuf>,
    recovery_view: Option<workflows::RecoveryView>,
    saved_tasks_open: bool,
    saved_tasks: Option<crate::session::SessionPage>,
    saved_tasks_offset: usize,
    saved_tasks_db: Option<PathBuf>,
    resume_task: Option<String>,
    error: bool,
}

impl Default for Desktop {
    fn default() -> Self {
        Self {
            artifact_temp: None,
            artifact_preview: None,
            failed_artifact_preview: None,
            environment_prepare_id: String::new(),
            conversations: history::History::default(),
            history_available: true,
            model_installer: models::Installer::default(),
            skill_catalog: None,
            skill_package_ui: skills::PackageUi::default(),
            chat: chat::Chat::default(),
            chat_mode: ChatMode::Chat,
            activity: job::ProgressSnapshot::default(),
            activity_elapsed: Duration::ZERO,
            draft: projects::ProjectDraft::default(),
            request_task: String::new(),
            graph_open: false,
            sessions_open: true,
            dock_open: true,
            dock_tab: DockTab::Changes,
            palette_open: false,
            focus_composer: false,
            palette_query: String::new(),
            settings_query: String::new(),
            graph: graph_view::GraphView::default(),
            publish_flag: std::cell::Cell::new(false),
            github_rx: None,
            knowledge_ui: projects::KnowledgeUi::default(),
            updates: updater::Controller::default(),
            update_confirm: false,
            real_app: false,
            prefs: Preferences::default(),
            last_attempted: Preferences::default(),
            storage: None,
            preference_error: None,
            dirty_since: None,
            style_key: None,
            settings_open: false,
            settings_page: SettingsPage::Model,
            transition: None,
            help_open: false,
            smoke: None,
            task: String::new(),
            job: None,
            operation: Operation::Task,
            report: None,
            report_json: Vec::new(),
            applied: Vec::new(),
            selected: 0,
            message: "Готова к локальной работе".into(),
            log: String::new(),
            hardware: None,
            confirm: None,
            recovery_open: false,
            recovery_root: None,
            recovery_view: None,
            saved_tasks_open: false,
            saved_tasks: None,
            saved_tasks_offset: 0,
            saved_tasks_db: None,
            resume_task: None,
            error: false,
        }
    }
}

#[allow(dead_code)]
enum GithubOut {
    Task(String),
    Pull(String),
}

impl Desktop {
    /// GitHub work runs on a short-lived thread; the token is read from the environment only.
    #[cfg(feature = "gateway-http")]
    fn github_fetch_issue(&mut self) {
        let (repo, issue, env) = (
            self.prefs.github_repo.trim().to_owned(),
            self.prefs.github_issue.trim().to_owned(),
            self.prefs.github_token_env.clone(),
        );
        let (tx, rx) = std::sync::mpsc::channel();
        self.github_rx = Some(rx);
        self.message = "GitHub: загрузка issue…".into();
        self.error = false;
        std::thread::spawn(move || {
            let result = (|| {
                let number: u64 = issue
                    .parse()
                    .map_err(|_| io::Error::other("Номер issue должен быть числом."))?;
                let client = github::GithubClient::new(github::DEFAULT_API, &env)?;
                let bundle = client.issue(&repo, number)?;
                Ok(GithubOut::Task(github::issue_task(&repo, &bundle)))
            })();
            let _ = tx.send(result);
        });
    }

    #[cfg(feature = "gateway-http")]
    fn github_publish(&mut self) {
        let Some(report) = &self.report else { return };
        let proposals: Vec<_> = report
            .proposals
            .get(self.selected)
            .cloned()
            .into_iter()
            .collect();
        let failed_checks = report
            .verifications
            .last()
            .is_some_and(|v| !v.result.passed());
        let (repo, env) = (
            self.prefs.github_repo.trim().to_owned(),
            self.prefs.github_token_env.clone(),
        );
        let title = self
            .request_task
            .lines()
            .next()
            .unwrap_or("Lyra changes")
            .chars()
            .take(120)
            .collect::<String>();
        let branch = format!(
            "lyra/{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs())
        );
        let (tx, rx) = std::sync::mpsc::channel();
        self.github_rx = Some(rx);
        self.message = "GitHub: публикация черновика PR…".into();
        self.error = false;
        std::thread::spawn(move || {
            let result = (|| {
                if failed_checks {
                    return Err(io::Error::other(
                        "Последняя проверка не прошла; публикация отменена.",
                    ));
                }
                let client = github::GithubClient::new(github::DEFAULT_API, &env)?;
                let url = client.publish(
                    &repo,
                    &branch,
                    "main",
                    &title,
                    "Prepared by Lyra; review before merging.",
                    &proposals,
                    true,
                )?;
                Ok(GithubOut::Pull(url))
            })();
            let _ = tx.send(result);
        });
    }

    #[cfg_attr(not(feature = "gateway-http"), allow(dead_code))]
    fn poll_github(&mut self, ctx: &egui::Context) {
        #[cfg(feature = "gateway-http")]
        if self.publish_flag.take() && self.github_rx.is_none() {
            self.github_publish();
        }
        let Some(rx) = &self.github_rx else { return };
        match rx.try_recv() {
            Ok(Ok(GithubOut::Task(text))) => {
                self.request_task = text;
                self.message = "GitHub: issue загружен в поле задачи".into();
            }
            Ok(Ok(GithubOut::Pull(url))) => self.message = format!("GitHub: {url}"),
            Ok(Err(e)) => {
                self.message = format!("GitHub: {e}");
                self.error = true;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(200));
                return;
            }
            Err(_) => {}
        }
        self.github_rx = None;
    }
    fn fail(&mut self, error: impl std::fmt::Display) {
        self.message = error.to_string();
        self.error = true;
    }

    fn start(&mut self, operation: Operation, ctx: &egui::Context) -> io::Result<()> {
        if self.job.is_some() || self.updates.kind() == Some(updater::Kind::Prepare) {
            return Err(io::Error::other("Дождитесь текущей операции."));
        }
        let temp = crate::workspace::private_tempdir("lyra-desktop-")?;
        let mut args: Vec<OsString> = Vec::new();
        let deadline = match operation {
            Operation::LocalChat => {
                let model = self
                    .prefs
                    .model
                    .as_ref()
                    .ok_or_else(|| io::Error::other("Выберите локальную GGUF-модель."))?;
                let request = self.chat_request(self.task.trim())?;
                let request_path = temp.path().join("local-chat-request.json");
                std::fs::write(
                    &request_path,
                    serde_json::to_vec(&request).map_err(io::Error::other)?,
                )?;
                args.extend([
                    "local-chat".into(),
                    "--request-file".into(),
                    request_path.into_os_string(),
                    "--model".into(),
                    model.as_os_str().into(),
                    "--prompt".into(),
                    "chat".into(),
                ]);
                for (flag, value) in [
                    ("--context", self.prefs.context as u64),
                    ("--max-tokens", self.prefs.tokens as u64),
                    ("--threads", self.prefs.threads as u64),
                    ("--timeout-secs", self.prefs.timeout),
                ] {
                    args.extend([flag.into(), value.to_string().into()]);
                }
                args.extend([
                    "--gpu-layers".into(),
                    self.prefs.gpu_layers.to_string().into(),
                ]);
                args.extend(self.skill_args()?);
                Some(Duration::from_secs(self.prefs.timeout + 5))
            }
            #[cfg(feature = "gateway-http")]
            Operation::CloudChat => {
                use std::io::Read;
                let registry = self
                    .prefs
                    .provider_registry
                    .as_ref()
                    .ok_or_else(|| io::Error::other("Укажите реестр HTTP-провайдеров."))?;
                let mut bytes = Vec::new();
                std::fs::File::open(registry)?
                    .take(64 * 1024 + 1)
                    .read_to_end(&mut bytes)?;
                let configs = crate::provider_http::parse_registry(&bytes)?;
                let selected = configs
                    .iter()
                    .find(|config| {
                        config.descriptor.provider == self.prefs.cloud_provider
                            && config.descriptor.model == self.prefs.cloud_model
                    })
                    .ok_or_else(|| io::Error::other("Выбранная модель отсутствует в реестре."))?;
                crate::provider_http::OpenAiCompatibleBackend::new(selected.clone())?;
                let request = self.chat_request(self.task.trim())?;
                let request_path = temp.path().join("chat-request.json");
                std::fs::write(
                    &request_path,
                    serde_json::to_vec(&request).map_err(io::Error::other)?,
                )?;
                args.extend([
                    "chat".into(),
                    "--provider-config".into(),
                    registry.as_os_str().into(),
                    "--allow".into(),
                    self.prefs.cloud_provider.clone().into(),
                    "--model".into(),
                    self.prefs.cloud_model.clone().into(),
                    "--request-file".into(),
                    request_path.into_os_string(),
                ]);
                args.extend(self.skill_args()?);
                Some(Duration::from_secs(100))
            }
            Operation::Task => {
                let repo = self
                    .prefs
                    .repo
                    .as_ref()
                    .ok_or_else(|| io::Error::other("Выберите папку проекта."))?;
                validate_task(&self.request_task, &self.prefs.files)?;
                let task_path = temp.path().join("task.txt");
                std::fs::write(&task_path, &self.request_task)?;
                args.extend([
                    "task".into(),
                    "--repo".into(),
                    repo.as_os_str().into(),
                    "--task-file".into(),
                    task_path.into_os_string(),
                ]);
                #[cfg(feature = "gateway-http")]
                if self.prefs.cloud_task {
                    use std::io::Read;
                    let registry = self
                        .prefs
                        .provider_registry
                        .as_ref()
                        .ok_or_else(|| io::Error::other("Укажите реестр HTTP-провайдеров."))?;
                    if self.prefs.cloud_provider.is_empty() || self.prefs.cloud_model.is_empty() {
                        return Err(io::Error::other(
                            "Выберите облачную модель в настройках провайдеров.",
                        ));
                    }
                    let mut bytes = Vec::new();
                    std::fs::File::open(registry)?
                        .take(64 * 1024 + 1)
                        .read_to_end(&mut bytes)?;
                    let configs = crate::provider_http::parse_registry(&bytes)?;
                    let selected = configs
                        .into_iter()
                        .find(|config| {
                            config.descriptor.provider == self.prefs.cloud_provider
                                && config.descriptor.model == self.prefs.cloud_model
                        })
                        .ok_or_else(|| {
                            io::Error::other("Облачная модель отсутствует в реестре.")
                        })?;
                    crate::provider_http::OpenAiCompatibleBackend::new(selected)?;
                    args.extend([
                        "--provider-config".into(),
                        registry.as_os_str().into(),
                        "--allow".into(),
                        self.prefs.cloud_provider.clone().into(),
                        "--cloud-model".into(),
                        self.prefs.cloud_model.clone().into(),
                    ]);
                } else {
                    let model = self
                        .prefs
                        .model
                        .as_ref()
                        .ok_or_else(|| io::Error::other("Выберите GGUF-модель."))?;
                    args.extend(["--model".into(), model.as_os_str().into()]);
                }
                #[cfg(not(feature = "gateway-http"))]
                {
                    let model = self
                        .prefs
                        .model
                        .as_ref()
                        .ok_or_else(|| io::Error::other("Выберите GGUF-модель."))?;
                    args.extend(["--model".into(), model.as_os_str().into()]);
                }
                for (flag, value) in [
                    ("--context", self.prefs.context as u64),
                    ("--max-tokens", self.prefs.tokens as u64),
                    ("--max-calls", self.prefs.calls as u64),
                    ("--threads", self.prefs.threads as u64),
                    ("--timeout-secs", self.prefs.timeout),
                ] {
                    args.extend([flag.into(), value.to_string().into()]);
                }
                args.extend([
                    "--gpu-layers".into(),
                    self.prefs.gpu_layers.to_string().into(),
                ]);
                if let Some(docs) = &self.prefs.documentation {
                    args.extend(["--docs".into(), docs.as_os_str().into()]);
                }
                for file in self
                    .prefs
                    .files
                    .lines()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    args.extend(["--file".into(), file.into()]);
                }
                args.extend(self.skill_args()?);
                if self.prefs.subagents {
                    args.push("--subagents".into());
                }
                if self.prefs.dedup_tool_context {
                    args.push("--dedup-tool-context".into());
                }
                self.append_task_workflow_args(&mut args)?;
                if self.prefs.code_graph {
                    args.push("--code-graph".into());
                }
                if let Some(registry) = &self.prefs.environments {
                    let current = crate::retrieval::sha256(
                        crate::knowledge::read_text(registry, 256 * 1024)?.as_bytes(),
                    );
                    if self.prefs.environment_manifest_sha256.as_ref() != Some(&current) {
                        return Err(io::Error::other(
                            "Реестр сред изменился. Выберите его повторно и подтвердите разрешенные среды.",
                        ));
                    }
                    let artifacts = crate::workspace::private_tempdir("lyra-desktop-artifacts-")?;
                    args.extend([
                        "--environments".into(),
                        registry.as_os_str().into(),
                        "--artifacts-dir".into(),
                        artifacts.path().as_os_str().into(),
                    ]);
                    for id in &self.prefs.allowed_environments {
                        args.extend(["--allow-environment".into(), id.into()]);
                    }
                    self.artifact_temp = Some(artifacts);
                }
                self.report = None;
                self.report_json.clear();
                self.applied.clear();
                self.selected = 0;
                Some(Duration::from_secs(self.prefs.timeout + 5))
            }
            Operation::Apply(_)
            | Operation::ApplyAll
            | Operation::RecoveryStatus
            | Operation::RecoveryRollback
            | Operation::RecoveryComplete => {
                self.prepare_publication_job(operation, &temp, &mut args)?
            }
            Operation::EnvironmentPrepare => {
                let registry = self
                    .prefs
                    .environments
                    .as_ref()
                    .ok_or_else(|| io::Error::other("Выберите реестр сред"))?;
                args.extend([
                    "environment".into(),
                    "--registry".into(),
                    registry.as_os_str().into(),
                    "prepare".into(),
                    self.environment_prepare_id.clone().into(),
                    "--accept".into(),
                ]);
                Some(Duration::from_secs(610))
            }
            Operation::SessionList => self.prepare_saved_tasks_job(&mut args)?,
            Operation::ProjectPlan
            | Operation::ProjectCreate
            | Operation::DocsFetch
            | Operation::DocsImport => self.prepare_project_job(operation, &temp, &mut args)?,
            Operation::Doctor => {
                args.extend(["doctor".into(), "--json".into()]);
                Some(Duration::from_secs(30))
            }
        };
        let ctx = ctx.clone();
        self.job = Some(Job::start(
            args,
            temp,
            !matches!(
                operation,
                Operation::Apply(_)
                    | Operation::ApplyAll
                    | Operation::RecoveryRollback
                    | Operation::RecoveryComplete
                    | Operation::ProjectCreate
            ),
            deadline,
            move || ctx.request_repaint(),
        )?);
        self.operation = operation;
        if operation == Operation::Task {
            self.resume_task = None;
        }
        self.error = false;
        self.message = match operation {
            Operation::Task => "Модель работает…",
            Operation::LocalChat => "Локальная модель отвечает…",
            #[cfg(feature = "gateway-http")]
            Operation::CloudChat => "HTTP-провайдер отвечает…",
            Operation::Apply(_) => "Применение выбранной правки…",
            Operation::ApplyAll => "Применение набора правок…",
            Operation::RecoveryStatus => "Чтение состояния восстановления…",
            Operation::EnvironmentPrepare => "Подготовка выбранной среды…",
            Operation::SessionList => "Чтение сохраненных задач…",
            Operation::RecoveryRollback | Operation::RecoveryComplete => "Восстановление файлов…",
            Operation::Doctor => "Сбор сведений о ПК…",
            Operation::ProjectPlan => "Готовлю план проекта…",
            Operation::ProjectCreate => "Создаю новый проект…",
            Operation::DocsFetch => "Загружаю документацию…",
            Operation::DocsImport => "Импортирую документацию…",
        }
        .into();
        Ok(())
    }

    fn poll(&mut self) {
        let Some(result) = self.job.as_ref().and_then(Job::poll) else {
            return;
        };
        if let Some(worker) = &self.job {
            self.activity = worker.progress();
            self.activity_elapsed = worker.started.elapsed();
        }
        self.job = None;
        match result {
            Ok(result) => self.complete(result),
            Err(error) => {
                self.fail(error);
                self.finish_chat();
            }
        }
    }

    fn complete(&mut self, result: ResultData) {
        self.log = result.stderr.clone();
        let structured_task_timeout = self.operation == Operation::Task
            && serde_json::from_slice::<Report>(&result.stdout)
                .is_ok_and(|r| matches!(r.schema_version, 1..=5) && r.status == "timeout");
        if (result.timed_out || result.code == Some(124)) && !structured_task_timeout {
            self.fail(if self.operation == Operation::ProjectPlan {
                "Истёк таймаут подготовки плана. Увеличь таймаут в настройках модели или сократи запрос. Файлы проекта ещё не создавались. Подробности — в журнале."
            } else {
                "Истёк таймаут операции. Увеличь таймаут в настройках модели. Проверь результат операции; подробности — в журнале."
            });
            self.finish_chat();
            return;
        }
        if result.cancelled {
            self.fail(
                if matches!(self.operation, Operation::DocsFetch | Operation::DocsImport) {
                    "Операция остановлена. Проверь путь пакета: сохранение могло завершиться."
                } else {
                    "Задача остановлена. Правки не применялись."
                },
            );
            self.finish_chat();
            return;
        }
        let seconds = result.elapsed.as_secs_f32();
        match self.operation {
            Operation::EnvironmentPrepare => {
                if result.code == Some(0) {
                    self.message = "Среда подготовлена".into();
                    self.error = false;
                } else {
                    self.fail("Подготовка среды не завершена; проверьте журнал");
                }
            }
            Operation::LocalChat => {
                if result.code == Some(0) {
                    match String::from_utf8(result.stdout) {
                        Ok(text) if !text.trim().is_empty() => {
                            self.message = text.trim().to_owned();
                            self.error = false;
                        }
                        Ok(_) => self.fail("Локальная GGUF-модель вернула пустой ответ."),
                        Err(_) => self.fail("Некорректная кодировка ответа локальной модели."),
                    }
                } else {
                    self.fail("Локальная модель завершилась с ошибкой. Подробности — в журнале.");
                }
            }
            #[cfg(feature = "gateway-http")]
            Operation::CloudChat => {
                if result.code == Some(0) {
                    match String::from_utf8(result.stdout) {
                        Ok(text) => {
                            self.message = text.trim_end().to_owned();
                            self.error = false;
                        }
                        Err(_) => self.fail("Некорректная кодировка ответа HTTP-провайдера."),
                    }
                } else {
                    self.fail("HTTP-запрос завершился ошибкой. Подробности — в журнале.");
                }
            }
            Operation::ProjectPlan
            | Operation::ProjectCreate
            | Operation::DocsFetch
            | Operation::DocsImport => self.complete_project_job(result.code, &result.stdout),
            Operation::Task => match serde_json::from_slice::<Report>(&result.stdout) {
                Ok(report) if matches!(report.schema_version, 1..=5) => {
                    self.error = result.code != Some(0) || report.status != "completed";
                    self.message = format!(
                        "{} · {} {} · {:.1} s · {} {}",
                        if report.status == "timeout" {
                            self.prefs
                                .language
                                .text("Истёк таймаут задачи; предложения не применялись")
                        } else if self.error {
                            self.prefs.language.text("Задача не завершена")
                        } else if report.proposals.is_empty() && report.file_proposals.is_empty() {
                            self.prefs
                                .language
                                .text("Протокол завершён; предложений нет")
                        } else {
                            self.prefs.language.text("Предложения готовы к просмотру")
                        },
                        report.calls,
                        self.prefs.language.text("вызовов"),
                        seconds,
                        report.proposals.len() + report.file_proposals.len(),
                        self.prefs.language.text("предложений")
                    );
                    if !report.context_optimization.is_empty() {
                        let before: usize = report
                            .context_optimization
                            .iter()
                            .map(|s| s.prompt_bytes_before)
                            .sum();
                        let after: usize = report
                            .context_optimization
                            .iter()
                            .map(|s| s.prompt_bytes_after)
                            .sum();
                        self.message.push_str(&format!(
                            " · {} {} {}",
                            self.prefs.language.text("Сокращено контекста:"),
                            before.saturating_sub(after),
                            self.prefs.language.text("байт")
                        ));
                    }
                    self.report = Some(report);
                    self.report_json = result.stdout;
                }
                _ => self.fail(format!(
                    "Не получен отчёт задачи (код {:?}). Подробности — в журнале.",
                    result.code
                )),
            },
            Operation::Apply(_)
            | Operation::ApplyAll
            | Operation::RecoveryStatus
            | Operation::RecoveryRollback
            | Operation::RecoveryComplete => self.complete_publication_job(&result),
            Operation::SessionList => self.complete_saved_tasks_job(&result),
            Operation::Doctor => {
                if result.code == Some(0) {
                    self.hardware = serde_json::from_slice(&result.stdout).ok();
                    self.message = "Сведения собраны локально".into();
                } else {
                    self.fail("Не удалось получить диагностику. Откройте журнал.");
                }
            }
        }
        if !self.error
            && matches!(
                self.operation,
                Operation::Apply(_)
                    | Operation::ApplyAll
                    | Operation::RecoveryRollback
                    | Operation::RecoveryComplete
                    | Operation::ProjectCreate
            )
        {
            self.chat.notice(self.message.clone());
        }
        self.finish_chat();
    }

    fn choose_path(&mut self, model: bool) {
        let l = self.prefs.language;
        if model {
            if let Some(p) = rfd::FileDialog::new()
                .set_title(l.text("Модель GGUF"))
                .add_filter("GGUF", &["gguf"])
                .pick_file()
            {
                self.prefs.model = Some(p);
            }
        } else if let Some(p) = rfd::FileDialog::new()
            .set_title(l.text("Проект"))
            .pick_folder()
            && self.prefs.repo.as_ref() != Some(&p)
        {
            if !self.chat.is_empty() {
                self.new_session();
            }
            self.prefs.files.clear();
            self.prefs.repo = Some(p);
            self.capture_session();
        }
    }

    fn update_settings(&mut self, ui: &mut egui::Ui) {
        let c = self.prefs.palette.colors();
        let l = self.prefs.language;
        ui.label(format!(
            "Lyra {} · {} · {}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            if cfg!(feature = "cuda") {
                "CUDA"
            } else {
                "CPU"
            }
        ));
        ui.label(
            RichText::new(l.text("GitHub Releases · скачивание и установка только по кнопке."))
                .small()
                .weak(),
        );
        if !updater::Controller::supported() {
            ui.label(l.text("Установка обновлений доступна в portable Windows x64 CPU. Для других сборок скачай пакет вручную."));
            return;
        }
        icons::toggle(
            ui,
            &mut self.prefs.check_updates,
            l.text("Проверять обновления каждые 5 минут"),
            self.prefs.animations,
            c.accent,
            c.on_accent,
        );
        let preview_changed = ui
            .add_enabled_ui(!self.updates.busy(), |ui| {
                icons::toggle(
                    ui,
                    &mut self.prefs.preview_updates,
                    l.text("Предварительные версии"),
                    self.prefs.animations,
                    c.accent,
                    c.on_accent,
                )
                .changed()
            })
            .inner;
        if preview_changed && !self.updates.busy() {
            self.updates.available = None;
            self.updates.ready = None;
        }
        ui.add_space(6.0);
        ui.label(l.text(&self.updates.message));
        if let Some(r) = &self.updates.available {
            ui.label(format!("{} → {}", env!("CARGO_PKG_VERSION"), r.version));
        }
        if self.updates.kind() == Some(updater::Kind::Download) {
            let total = self.updates.available.as_ref().map(|r| r.size).unwrap_or(1);
            ui.add(
                egui::ProgressBar::new(self.updates.downloaded() as f32 / total as f32)
                    .show_percentage(),
            );
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !self.updates.busy() && self.updates.ready.is_none(),
                    egui::Button::new(l.text("Проверить обновления")),
                )
                .clicked()
                && self.real_app
            {
                self.updates.check(self.prefs.preview_updates, ui.ctx());
            }
            if self.updates.available.is_some()
                && self.updates.ready.is_none()
                && ui
                    .add_enabled(
                        !self.updates.busy(),
                        egui::Button::new(l.text("Скачать обновление")),
                    )
                    .clicked()
                && self.real_app
            {
                self.updates.download(ui.ctx());
            }
            if self.updates.ready.is_some()
                && ui
                    .add_enabled(
                        !self.updates.busy() && self.job.is_none(),
                        egui::Button::new(l.text("Установить и перезапустить")),
                    )
                    .clicked()
            {
                self.update_confirm = true;
            }
            if self.updates.busy()
                && self.updates.kind() != Some(updater::Kind::Prepare)
                && ui.button(l.text("Отмена")).clicked()
            {
                self.updates.cancel();
            }
        });
        if self.job.is_some() {
            ui.label(l.text("Перед установкой заверши или останови текущую операцию."));
        }
        if let Some(error) = &self.updates.error {
            ui.add(egui::Label::new(RichText::new(error).small().color(c.error)).wrap());
        }
        ui.label(RichText::new(l.text("Модель работает локально. Проверку обновлений можно отключить; CLI к сети не обращается.")).small().weak());
    }

    fn hardware_view(&self, ui: &mut egui::Ui) {
        let c = self.prefs.palette.colors();
        let l = self.prefs.language;
        let Some(r) = &self.hardware else {
            ui.label(
                RichText::new(l.text("Сведения о текущем компьютере появятся по кнопке."))
                    .small()
                    .weak(),
            );
            return;
        };
        let name = |v: &serde_json::Value| v.as_str().unwrap_or("—").to_owned();
        ui.label(RichText::new(name(&r["os"])).strong());
        for (title, value) in [
            ("Процессор", name(&r["cpu"]["brand"])),
            (
                "Доступная память",
                format!(
                    "{} / {}",
                    memory_label(&r["memory"]["available_bytes"]),
                    memory_label(&r["memory"]["total_bytes"])
                ),
            ),
        ] {
            ui.horizontal(|ui| {
                ui.label(RichText::new(l.text(title)).color(c.muted));
                ui.label(value);
            });
        }
        if let Some(gpus) = r["gpus"].as_array() {
            for gpu in gpus {
                ui.label(format!(
                    "{} · {} VRAM",
                    name(&gpu["model"]),
                    memory_label(&gpu["dedicated_vram_bytes"])
                ));
            }
        }
        if let Some(disks) = r["disks"].as_array() {
            for disk in disks {
                ui.label(format!(
                    "{} · {} {} / {}",
                    name(&disk["mount_point"]),
                    memory_label(&disk["available_bytes"]),
                    l.text("свободно"),
                    memory_label(&disk["total_bytes"])
                ));
            }
        }
        if let Some(warnings) = r["warnings"].as_array() {
            for warning in warnings {
                ui.label(RichText::new(name(warning)).small().weak());
            }
        }
        ui.label(
            RichText::new(if cfg!(feature = "cuda") {
                "CUDA включена в бинарнике. Фактическая доступность и свободная VRAM проверяются при запуске GGUF."
            } else {
                "Сборка CPU-only; CUDA требует отдельного бинарника."
            })
                .small()
                .weak(),
        );
    }

    fn load() -> Self {
        let mut app = Self::default();
        app.storage = preferences::config_path();
        if let Some(path) = &app.storage {
            match Preferences::load(path) {
                Ok(p) => app.prefs = p,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => app.preference_error = Some(e.to_string()),
            }
        }
        app.real_app = true;
        app.updates.configure(
            app.storage
                .as_ref()
                .and_then(|p| p.parent())
                .map(|p| p.join("updater")),
        );
        if let Some(path) = app
            .storage
            .as_ref()
            .and_then(|p| p.parent())
            .map(|p| p.join("conversations.json"))
        {
            match history::History::load(&path) {
                Ok(history) => app.conversations = history,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    app.history_available = false;
                    app.preference_error =
                        Some(format!("История: {error}. Файл сохранён без изменений."));
                }
            }
        }
        if let Some(session) = app
            .conversations
            .sessions
            .get(app.conversations.active)
            .cloned()
        {
            app.chat = session.chat;
            app.chat_mode = session.mode;
            app.prefs.repo = session.repo;
            app.prefs.files = session.files;
            app.task = session.draft;
        } else {
            app.capture_session();
        }
        app.model_installer.detect();
        app.last_attempted = app.prefs.clone();
        app
    }
    fn save_preferences(&mut self, ctx: &egui::Context) {
        if self.prefs.repo != self.last_attempted.repo
            || self.prefs.files != self.last_attempted.files
        {
            self.capture_session();
        }
        if self.storage.is_none() || self.prefs == self.last_attempted {
            self.dirty_since = None;
            return;
        }
        let elapsed = self.dirty_since.get_or_insert_with(Instant::now).elapsed();
        if elapsed < Duration::from_millis(300) || ctx.input(|i| i.pointer.any_down()) {
            ctx.request_repaint_after(Duration::from_millis(300));
            return;
        }
        if let Some(path) = &self.storage {
            self.preference_error = self.prefs.save(path).err().map(|e| e.to_string());
        }
        self.last_attempted = self.prefs.clone();
        self.dirty_since = None;
    }
}
impl Drop for Desktop {
    fn drop(&mut self) {
        self.job = None;
        self.capture_session();
        self.save_history(&egui::Context::default(), true);
        if self.prefs != self.last_attempted
            && let Some(path) = &self.storage
        {
            let _ = self.prefs.save(path);
        }
    }
}

impl eframe::App for Desktop {
    fn logic(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.poll();
        self.poll_model_install(ctx);
        let visible = !ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        self.updates.tick(
            ctx,
            self.prefs.check_updates,
            self.prefs.preview_updates,
            visible,
            self.real_app,
        );
        if self.updates.close_now {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if self.updates.kind() == Some(updater::Kind::Prepare)
            && ctx.input(|i| i.viewport().close_requested())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        if self.job.is_some() && !ctx.input(|i| i.viewport().minimized.unwrap_or(false)) {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        if let Some(counter) = &self.smoke {
            let step = counter
                .load(std::sync::atomic::Ordering::Relaxed)
                .min(SMOKE_STEPS - 1);
            self.prefs.palette = Palette::ALL[step / (SETTINGS_CYCLE * 2)];
            self.prefs.language = if (step / SETTINGS_CYCLE).is_multiple_of(2) {
                Language::Ru
            } else {
                Language::En
            };
            self.prefs.animations = self.prefs.language == Language::Ru;
            self.settings_open =
                step % SETTINGS_CYCLE != 0 && step % SETTINGS_CYCLE != SETTINGS_CYCLE - 1;
            self.smoke_conversation(step % SETTINGS_CYCLE == SETTINGS_CYCLE - 1);
            self.settings_page = SettingsPage::ALL[(step % SETTINGS_CYCLE)
                .saturating_sub(1)
                .min(SettingsPage::ALL.len() - 1)];
        }
        self.poll_github(ui.ctx());
        self.draw(ui);
        if self.real_app {
            self.updates.acknowledge_startup();
        }
        if let Some(counter) = &self.smoke {
            if counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1 >= SMOKE_STEPS {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                ui.ctx().request_repaint_after(Duration::from_millis(25));
            }
        }
    }
}

fn selectable_code(ui: &mut egui::Ui, text: &str, color: Color32) {
    ui.add(
        egui::Label::new(RichText::new(text).monospace().color(color))
            .selectable(true)
            .wrap(),
    );
}
fn memory_label(value: &serde_json::Value) -> String {
    value
        .as_u64()
        .map(|n| format!("{:.1} GiB", n as f64 / 1_073_741_824.0))
        .unwrap_or_else(|| "—".into())
}
fn validate_task(task: &str, files: &str) -> io::Result<()> {
    if task.trim().is_empty() || task.len() > 16384 {
        return Err(io::Error::other(
            "Задание должно содержать 1–16384 байта UTF-8.",
        ));
    }
    if files.lines().filter(|s| !s.trim().is_empty()).count() > 32 {
        return Err(io::Error::other("Выберите не больше 32 файлов."));
    }
    Ok(())
}
fn can_apply(report: &Report, index: usize, applied: &[usize]) -> bool {
    matches!(report.schema_version, 1..=5)
        && report.status == "completed"
        && (report.schema_version < 4
            || matches!(
                report.outcome,
                crate::agent::TaskOutcome::ChangesProposed
                    | crate::agent::TaskOutcome::ChangesVerified
            ))
        && !applied.contains(&index)
        && (report
            .proposals
            .get(index)
            .is_some_and(|p| !p.create || report.schema_version >= 2)
            || report.schema_version == 5
                && index >= report.proposals.len()
                && index - report.proposals.len() < report.file_proposals.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn utf8_budget_and_file_count_are_bounded() {
        assert!(validate_task(&"я".repeat(8193), "").is_err());
        assert!(validate_task("fix", &"src/lib.rs\n".repeat(33)).is_err());
        assert!(validate_task("Исправь", "src/lib.rs").is_ok());
    }
    #[test]
    fn preview_preserves_apply_policy_and_renders_unicode() {
        let mut report: Report = serde_json::from_value(serde_json::json!({
            "schema_version":1,"workspace":"project","status":"incomplete",
            "summaries":[["coder","Предложение, а не проверенный тест"]],
            "proposals":[{"path":"src/lib.rs","original":"left - right\r\n","replacement":"left + right\r\n"}],
            "events":[],"calls":1,"subagents":false,"truncated_index":false,"model_review_is_verified_test":false
        })).unwrap();
        assert!(!can_apply(&report, 0, &[]));
        report.status = "completed".into();
        assert!(can_apply(&report, 0, &[]));
        report.schema_version = 3;
        assert!(can_apply(&report, 0, &[]));
        assert!(!can_apply(&report, 1, &[]));
        assert!(!can_apply(&report, 0, &[0]));
        report.proposals[0].create = true;
        report.proposals[0].original.clear();
        report.outcome = crate::agent::TaskOutcome::ChangesProposed;
        for schema in [2, 3, 4] {
            report.schema_version = schema;
            assert!(can_apply(&report, 0, &[]));
        }
        report.schema_version = 1;
        assert!(!can_apply(&report, 0, &[]));
        report.schema_version = 4;
        report.outcome = crate::agent::TaskOutcome::Incomplete;
        assert!(!can_apply(&report, 0, &[]));
        report.outcome = crate::agent::TaskOutcome::ChangesProposed;
        report.proposals[0].create = false;
        report.proposals[0].original = "left - right\r\n".into();
        let ctx = egui::Context::default();
        configure(&ctx);
        let mut app = Desktop::default();
        app.report = Some(report);
        app.confirm = Some(Operation::Apply(0));
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 460.0),
                )),
                ..Default::default()
            },
            |ui| app.draw(ui),
        );
        // No renderer in this headless layout test; acknowledge unuploaded font textures.
        output.textures_delta.clear();
        assert!(!output.shapes.is_empty());
        assert_eq!(
            app.report.as_ref().unwrap().proposals[0].original,
            "left - right\r\n"
        );
        assert!(app.applied.is_empty());
    }
    #[test]
    fn compact_pages_render_all_themes_languages_and_minimum_size() {
        let ctx = egui::Context::default();
        configure(&ctx);
        let mut app = Desktop::default();
        for size in [WINDOW_SIZE, MIN_SIZE] {
            for palette in Palette::ALL {
                for language in [Language::Ru, Language::En] {
                    app.prefs.palette = palette;
                    app.prefs.language = language;
                    app.prefs.animations = false;
                    for page in 0usize..SETTINGS_CYCLE {
                        app.settings_open = page != 0 && page != SETTINGS_CYCLE - 1;
                        app.smoke_conversation(page == SETTINGS_CYCLE - 1);
                        app.settings_page = SettingsPage::ALL
                            [page.saturating_sub(1).min(SettingsPage::ALL.len() - 1)];
                        let mut output = ctx.run_ui(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    egui::vec2(size[0], size[1]),
                                )),
                                ..Default::default()
                            },
                            |ui| app.draw(ui),
                        );
                        output.textures_delta.clear();
                        assert!(!output.shapes.is_empty());
                    }
                }
            }
        }
        assert!(app.storage.is_none());
        assert!(app.report.is_none());
    }
    #[test]
    fn settings_search_command_palette_and_dock_work_without_a_display() {
        let ctx = egui::Context::default();
        configure(&ctx);
        let mut app = Desktop::default();
        app.settings_query = "github".into();
        assert_eq!(app.matching_pages(), vec![SettingsPage::Github]);
        app.settings_query = "nothing-matches-this".into();
        assert!(app.matching_pages().is_empty());
        app.settings_query.clear();
        assert_eq!(app.matching_pages().len(), SettingsPage::ALL.len());
        // Palette actions switch views and apply preferences without starting work.
        app.run_palette_action(shell::PaletteAction::Settings(SettingsPage::Checks), &ctx);
        assert!(app.settings_open && app.settings_page == SettingsPage::Checks);
        app.run_palette_action(shell::PaletteAction::Theme(Palette::Light), &ctx);
        assert_eq!(app.prefs.palette, Palette::Light);
        app.run_palette_action(shell::PaletteAction::Graph, &ctx);
        assert!(app.graph_open && !app.settings_open);
        app.run_palette_action(shell::PaletteAction::Work, &ctx);
        assert!(!app.graph_open && !app.settings_open);
        assert!(app.job.is_none());
        let labels: Vec<_> = app.palette_items().into_iter().map(|(_, l, _)| l).collect();
        assert!(labels.iter().any(|l| l.contains("Новый чат")));
        // Every dock tab renders for a report with a text and a created file.
        let report: Report = serde_json::from_value(serde_json::json!({
            "schema_version":4,"workspace":"project","status":"completed","outcome":"changes_proposed",
            "summaries":[["coder","done"]],
            "proposals":[{"path":"a.txt","original":"x\r\ny\r\n","replacement":"x\r\nz\r\n"},
                         {"path":"b.txt","original":"","replacement":"new\n","create":true}],
            "events":[{"call":1,"role":"coder","tool":"propose_edit","outcome":"ok","argument":"a.txt"}],
            "calls":1,"subagents":false,"truncated_index":false,"model_review_is_verified_test":false
        })).unwrap();
        app.report = Some(report);
        for palette in Palette::ALL {
            for tab in [
                DockTab::Changes,
                DockTab::Checks,
                DockTab::Plan,
                DockTab::Log,
            ] {
                for selected in [0, 1] {
                    app.prefs.palette = palette;
                    app.dock_tab = tab;
                    app.selected = selected;
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(1160.0, 760.0),
                            )),
                            ..Default::default()
                        },
                        |ui| app.draw(ui),
                    );
                    output.textures_delta.clear();
                    assert!(!output.shapes.is_empty());
                }
            }
        }
        assert!(
            app.applied.is_empty(),
            "rendering must never apply a proposal"
        );
    }
    #[test]
    fn empty_window_renders_without_display() {
        let ctx = egui::Context::default();
        configure(&ctx);
        let mut app = Desktop::default();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(760.0, 480.0),
                )),
                ..Default::default()
            },
            |ui| app.draw(ui),
        );
        // No renderer in this headless layout test; acknowledge unuploaded font textures.
        output.textures_delta.clear();
        assert!(!output.shapes.is_empty());
    }
}
