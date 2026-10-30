//! Native shell around the same CLI contracts. No inference or filesystem policy in widgets.
mod icons;
mod job;
mod preferences;
mod projects;
mod theme;
pub mod updater;
use crate::agent::Report;
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

const WINDOW_SIZE: [f32; 2] = [760.0, 480.0];
const MIN_SIZE: [f32; 2] = [640.0, 460.0];
const SMOKE_STEPS: usize = 72;

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

#[derive(Clone, Copy, PartialEq)]
enum SettingsPage {
    Model,
    Context,
    Workers,
    Appearance,
    Hardware,
    Updates,
    Knowledge,
}
impl SettingsPage {
    const ALL: [Self; 7] = [
        Self::Model,
        Self::Context,
        Self::Workers,
        Self::Appearance,
        Self::Hardware,
        Self::Updates,
        Self::Knowledge,
    ];
    fn title(self) -> &'static str {
        match self {
            Self::Model => "Модель и ресурсы",
            Self::Context => "Контекст проекта",
            Self::Workers => "Помощники",
            Self::Appearance => "Оформление",
            Self::Hardware => "Мой компьютер",
            Self::Updates => "Обновления",
            Self::Knowledge => "Документация",
        }
    }
    fn icon(self) -> Icon {
        match self {
            Self::Model => Icon::Cpu,
            Self::Context => Icon::Folder,
            Self::Workers => Icon::Workers,
            Self::Appearance => Icon::Palette,
            Self::Hardware => Icon::Monitor,
            Self::Updates => Icon::Download,
            Self::Knowledge => Icon::BookOpen,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Changes,
    Activity,
}
#[derive(Clone, Copy, PartialEq)]
enum Operation {
    Task,
    Apply(usize),
    Doctor,
    ProjectPlan,
    ProjectCreate,
    DocsFetch,
    DocsImport,
}

struct Desktop {
    wizard: projects::Wizard,
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
    tab: Tab,
    message: String,
    log: String,
    hardware: Option<serde_json::Value>,
    confirm: Option<usize>,
    error: bool,
}

impl Default for Desktop {
    fn default() -> Self {
        Self {
            wizard: projects::Wizard::default(),
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
            tab: Tab::Changes,
            message: "Готова к локальной работе".into(),
            log: String::new(),
            hardware: None,
            confirm: None,
            error: false,
        }
    }
}

impl Desktop {
    fn fail(&mut self, error: impl std::fmt::Display) {
        self.message = error.to_string();
        self.error = true;
    }

    fn start(&mut self, operation: Operation, ctx: &egui::Context) -> io::Result<()> {
        if self.job.is_some() || self.updates.kind() == Some(updater::Kind::Prepare) {
            return Err(io::Error::other("Дождитесь текущей операции."));
        }
        let temp = tempfile::Builder::new().prefix("lyra-desktop-").tempdir()?;
        let mut args: Vec<OsString> = Vec::new();
        let deadline = match operation {
            Operation::Task => {
                let repo = self
                    .prefs
                    .repo
                    .as_ref()
                    .ok_or_else(|| io::Error::other("Выберите папку проекта."))?;
                let model = self
                    .prefs
                    .model
                    .as_ref()
                    .ok_or_else(|| io::Error::other("Выберите GGUF-модель."))?;
                validate_task(&self.task, &self.prefs.files)?;
                let task_path = temp.path().join("task.txt");
                std::fs::write(&task_path, &self.task)?;
                args.extend([
                    "task".into(),
                    "--repo".into(),
                    repo.as_os_str().into(),
                    "--model".into(),
                    model.as_os_str().into(),
                    "--task-file".into(),
                    task_path.into_os_string(),
                ]);
                for (flag, value) in [
                    ("--context", self.prefs.context as u64),
                    ("--max-tokens", self.prefs.tokens as u64),
                    ("--max-calls", self.prefs.calls as u64),
                    ("--threads", self.prefs.threads as u64),
                    ("--timeout-secs", self.prefs.timeout),
                ] {
                    args.extend([flag.into(), value.to_string().into()]);
                }
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
                if self.prefs.subagents {
                    args.push("--subagents".into());
                }
                self.report = None;
                self.report_json.clear();
                self.applied.clear();
                self.selected = 0;
                self.tab = Tab::Changes;
                Some(Duration::from_secs(self.prefs.timeout + 5))
            }
            Operation::Apply(index) => {
                let report = self
                    .report
                    .as_ref()
                    .ok_or_else(|| io::Error::other("Нет отчёта."))?;
                if !can_apply(report, index, &self.applied) {
                    return Err(io::Error::other("Предложение нельзя применить."));
                }
                let path = temp.path().join("report.json");
                std::fs::write(&path, &self.report_json)?;
                args.extend([
                    "apply".into(),
                    "--repo".into(),
                    report.workspace.as_os_str().into(),
                    "--report".into(),
                    path.into_os_string(),
                    "--index".into(),
                    index.to_string().into(),
                ]);
                None // Do not kill a worker while it commits a file replacement.
            }
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
            !matches!(operation, Operation::Apply(_) | Operation::ProjectCreate),
            deadline,
            move || ctx.request_repaint(),
        )?);
        self.operation = operation;
        self.error = false;
        self.message = match operation {
            Operation::Task => "Модель работает…",
            Operation::Apply(_) => "Применение выбранной правки…",
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
        self.job = None;
        match result {
            Ok(result) => self.complete(result),
            Err(error) => self.fail(error),
        }
    }

    fn complete(&mut self, result: ResultData) {
        self.log = result.stderr;
        if result.cancelled {
            self.fail(
                if matches!(self.operation, Operation::DocsFetch | Operation::DocsImport) {
                    "Операция остановлена. Проверь путь пакета: сохранение могло завершиться."
                } else {
                    "Задача остановлена. Правки не применялись."
                },
            );
            return;
        }
        let seconds = result.elapsed.as_secs_f32();
        match self.operation {
            Operation::ProjectPlan
            | Operation::ProjectCreate
            | Operation::DocsFetch
            | Operation::DocsImport => self.complete_project_job(result.code, &result.stdout),
            Operation::Task => match serde_json::from_slice::<Report>(&result.stdout) {
                Ok(report) if matches!(report.schema_version, 1 | 2) => {
                    self.error = result.code != Some(0) || report.status != "completed";
                    self.message = format!(
                        "{} · {} {} · {:.1} s · {} {}",
                        if self.error {
                            self.prefs.language.text("Задача не завершена")
                        } else if report.proposals.is_empty() {
                            self.prefs
                                .language
                                .text("Протокол завершён; предложений нет")
                        } else {
                            self.prefs.language.text("Предложения готовы к просмотру")
                        },
                        report.calls,
                        self.prefs.language.text("вызовов"),
                        seconds,
                        report.proposals.len(),
                        self.prefs.language.text("предложений")
                    );
                    self.report = Some(report);
                    self.report_json = result.stdout;
                }
                _ => self.fail(format!(
                    "Не получен отчёт задачи (код {:?}). Подробности — в журнале.",
                    result.code
                )),
            },
            Operation::Apply(index) => {
                if result.code == Some(0) {
                    self.applied.push(index);
                    self.message = "Правка применена. Запустите тесты проекта отдельно.".into();
                    self.error = false;
                } else {
                    self.fail("Правка не применена: конфликт или ошибка доступа. Откройте журнал.");
                }
            }
            Operation::Doctor => {
                if result.code == Some(0) {
                    self.hardware = serde_json::from_slice(&result.stdout).ok();
                    self.message = "Сведения собраны локально".into();
                } else {
                    self.fail("Не удалось получить диагностику. Откройте журнал.");
                }
            }
        }
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        let c = self.prefs.palette.colors();
        let l = self.prefs.language;
        ui.horizontal(|ui| {
            if self.settings_open {
                if icons::button(ui, Icon::Back, l.text("Вернуться к задаче"), false, c.text)
                    .clicked()
                {
                    self.settings_open = false;
                    self.transition = Some(Instant::now());
                }
                ui.heading(l.text("Настройки"));
            } else {
                ui.add(
                    Icon::Sparkles
                        .image(c.accent)
                        .fit_to_exact_size(egui::vec2(24.0, 24.0)),
                );
                ui.label(RichText::new("Lyra").size(26.0).strong());
                ui.label(RichText::new(env!("CARGO_PKG_VERSION")).small().weak());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !self.settings_open
                    && icons::button(ui, Icon::Settings, l.text("Настройки"), false, c.text)
                        .clicked()
                {
                    self.settings_open = true;
                    self.transition = Some(Instant::now());
                }
                let response = icons::button(
                    ui,
                    Icon::Download,
                    l.text("Обновления"),
                    self.settings_open && self.settings_page == SettingsPage::Updates,
                    c.accent,
                );
                if self.updates.available.is_some() {
                    let elapsed = self
                        .updates
                        .attention
                        .map(|t| t.elapsed())
                        .unwrap_or(Duration::from_secs(1));
                    let blink = self.prefs.animations && elapsed < Duration::from_millis(900);
                    if !blink || (elapsed.as_millis() / 150).is_multiple_of(2) {
                        ui.painter().circle_filled(
                            response.rect.right_top() + egui::vec2(-4.0, 4.0),
                            3.0,
                            c.accent,
                        );
                    }
                    if blink {
                        ui.ctx().request_repaint_after(Duration::from_millis(150));
                    }
                }
                if response.clicked() {
                    self.settings_open = true;
                    self.settings_page = SettingsPage::Updates;
                    self.transition = Some(Instant::now());
                }
                if icons::button(ui, Icon::Help, l.text("Как пользоваться"), false, c.muted)
                    .clicked()
                {
                    self.help_open = true;
                }
                ui.label(
                    RichText::new(l.text("Локально · CPU"))
                        .small()
                        .color(c.accent),
                );
            });
        });
    }

    fn home(&mut self, ui: &mut egui::Ui) {
        let c = self.prefs.palette.colors();
        let l = self.prefs.language;
        ui.columns(2, |columns| {
            for (column, model) in columns.iter_mut().zip([false, true]) {
                let path = if model {
                    self.prefs.model.as_ref()
                } else {
                    self.prefs.repo.as_ref()
                };
                let title = if model {
                    l.text("Модель GGUF")
                } else {
                    l.text("Проект")
                };
                let name = path
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| l.text("Выбрать…").into());
                let full = path
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| l.text("Выбрать…").into());
                let width = column.available_width();
                let response = column
                    .add_enabled(
                        self.job.is_none(),
                        egui::Button::image_and_text(
                            if model { Icon::Model } else { Icon::Folder }.image(c.accent),
                            format!("{title}  ·  {name}"),
                        )
                        .min_size(egui::vec2(width, 40.0))
                        .fill(c.card)
                        .corner_radius(10)
                        .wrap_mode(egui::TextWrapMode::Truncate),
                    )
                    .on_hover_text(full);
                if response.clicked() {
                    self.choose_path(model);
                }
            }
        });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.job.is_none(),
                    egui::Button::image_and_text(
                        Icon::FolderPlus.image(c.accent),
                        l.text("Новый проект"),
                    ),
                )
                .clicked()
            {
                self.wizard.open = true;
            }
            if ui.small_button(l.text("Документация")).clicked() {
                self.settings_open = true;
                self.settings_page = SettingsPage::Knowledge;
                self.transition = Some(Instant::now());
            }
            if let Some(path) = &self.prefs.documentation {
                ui.add(
                    egui::Label::new(
                        RichText::new(path.file_name().unwrap_or_default().to_string_lossy())
                            .small()
                            .weak(),
                    )
                    .truncate(),
                );
            }
        });
        ui.add_space(3.0);
        ui.add_enabled(
            self.job.is_none(),
            egui::TextEdit::multiline(&mut self.task)
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .char_limit(16384)
                .hint_text(l.text("Опиши задачу для выбранного проекта…")),
        );
        ui.horizontal(|ui| {
            let ready = self.job.is_none()
                && self.prefs.repo.is_some()
                && self.prefs.model.is_some()
                && !self.task.trim().is_empty();
            let start = ui.add_enabled(
                ready,
                egui::Button::image_and_text(
                    Icon::Play.image(c.on_accent),
                    RichText::new(l.text("Запустить"))
                        .color(c.on_accent)
                        .strong(),
                )
                .fill(c.accent),
            );
            if start.clicked()
                && let Err(e) = self.start(Operation::Task, ui.ctx())
            {
                self.fail(e);
            }
            if let Some(job) = &self.job {
                ui.label(format!("{:.0} s", job.started.elapsed().as_secs_f32()));
                if job.cancellable
                    && icons::action(ui, Icon::Stop, l.text("Остановить"), c.error).clicked()
                {
                    job.cancel();
                }
            } else if self.prefs.subagents {
                ui.add(Icon::Workers.image(c.muted));
                ui.label(
                    RichText::new(l.text("С исследователем и ревьюером"))
                        .small()
                        .weak(),
                );
            }
        });
        let message = self.prefs.language.text(&self.message);
        ui.add(
            egui::Label::new(RichText::new(message).small().color(if self.error {
                c.error
            } else {
                c.success
            }))
            .truncate(),
        )
        .on_hover_text(message);
        ui.horizontal(|ui| {
            for (tab, icon, title) in [
                (Tab::Changes, Icon::Model, "Предложения"),
                (Tab::Activity, Icon::Log, "Журнал"),
            ] {
                if ui
                    .add(
                        egui::Button::image_and_text(
                            icon.image(if self.tab == tab { c.accent } else { c.muted }),
                            l.text(title),
                        )
                        .selected(self.tab == tab),
                    )
                    .clicked()
                    && self.tab != tab
                {
                    self.tab = tab;
                    self.transition = Some(Instant::now());
                }
            }
        });
        egui::ScrollArea::vertical()
            .id_salt("results")
            .auto_shrink([false, false])
            .show(ui, |ui| match self.tab {
                Tab::Changes => self.changes(ui),
                Tab::Activity => {
                    if let Some(report) = &self.report {
                        ui.label(format!("{}: {}", l.text("Статус"), report.status));
                        for event in &report.events {
                            ui.label(format!(
                                "{} · {} · {} · {}",
                                event.call, event.role, event.tool, event.outcome
                            ));
                        }
                    }
                    selectable_code(ui, &self.log, c.muted);
                    if self.log.is_empty() {
                        ui.label(
                            RichText::new(
                                l.text("После задачи здесь появятся действия и журнал движка."),
                            )
                            .weak(),
                        );
                    }
                }
            });
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
        {
            self.prefs.repo = Some(p);
        }
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        let c = self.prefs.palette.colors();
        let l = self.prefs.language;
        ui.horizontal(|ui| {
            for page in SettingsPage::ALL {
                if icons::button(
                    ui,
                    page.icon(),
                    l.text(page.title()),
                    self.settings_page == page,
                    if self.settings_page == page {
                        c.accent
                    } else {
                        c.muted
                    },
                )
                .clicked()
                    && self.settings_page != page
                {
                    self.settings_page = page;
                    self.transition = Some(Instant::now());
                }
            }
            ui.separator();
            ui.label(RichText::new(l.text(self.settings_page.title())).strong());
        });
        ui.add_space(4.0);
        egui::ScrollArea::vertical().id_salt(("settings",self.settings_page as u8)).auto_shrink([false,false]).show(ui,|ui|{
            card(c).show(ui,|ui|{
                ui.set_min_width((ui.available_width()-2.0).max(0.0));
                match self.settings_page{
                    SettingsPage::Knowledge=>ui.vertical(|ui|self.knowledge_settings(ui)).response,
                    SettingsPage::Updates=>ui.vertical(|ui| self.update_settings(ui)).response,
                    SettingsPage::Model=>ui.add_enabled_ui(self.job.is_none(),|ui|{
                        ui.columns(2,|columns|{
                            columns[0].horizontal(|ui|{
                                ui.label(l.text("Контекст"));
                                help(ui,l.text("Контекст включает задание, код, историю и ответ. Большое окно требует больше памяти."),c.muted);
                            });
                            egui::ComboBox::from_id_salt("context").selected_text(self.prefs.context.to_string()).show_ui(&mut columns[0],|ui|{
                                for size in[1024,2048,4096,8192,16384,32768]{ui.selectable_value(&mut self.prefs.context,size,size.to_string());}
                            });
                            columns[0].add_space(8.0);
                            setting_slider(&mut columns[0],l.text("CPU-потоки"),&mut self.prefs.threads,1..=256,true,l.text("Больше потоков не всегда быстрее. Начальное значение учитывает доступные CPU, максимум 8."),c.muted);
                            setting_slider(&mut columns[1],l.text("Токенов на ответ"),&mut self.prefs.tokens,1..=8192,true,l.text("Максимум токенов одного действия модели."),c.muted);
                            columns[1].add_space(8.0);
                            columns[1].horizontal(|ui|{ui.label(l.text("Таймаут, с"));help(ui,l.text("Общее время задачи, включая загрузку модели."),c.muted);});
                            columns[1].add(egui::Slider::new(&mut self.prefs.timeout,1..=3600).logarithmic(true));
                        });
                        ui.add_space(8.0);
                        if ui.small_button(l.text("Сбросить числовые параметры")).clicked(){self.prefs.reset_inference();}
                    }).response,
                    SettingsPage::Context=>ui.add_enabled_ui(self.job.is_none(),|ui|{
                        ui.label(l.text("Файлы контекста · до 32 путей, по одному на строке"));
                        ui.label(RichText::new(l.text("Пусто — ограниченный индекс проекта. Секретные и служебные файлы исключаются движком.")).small().weak());
                        ui.add(egui::TextEdit::multiline(&mut self.prefs.files).desired_rows(6).desired_width(f32::INFINITY).char_limit(32768).hint_text("src/lib.rs\nCargo.toml"));
                        ui.label(format!("{} / 32",self.prefs.files.lines().filter(|s|!s.trim().is_empty()).count()));
                    }).response,
                    SettingsPage::Workers=>ui.add_enabled_ui(self.job.is_none(),|ui|{
                        icons::toggle(ui,&mut self.prefs.subagents,l.text("Исследователь и ревьюер"),self.prefs.animations,c.accent,c.on_accent);
                        if self.prefs.subagents{self.prefs.calls=self.prefs.calls.max(3);}
                        ui.label(RichText::new(l.text("Роли работают последовательно с одной моделью. Дополнительные проверки требуют времени.")).small().weak());
                        let min=if self.prefs.subagents{3}else{1};
                        setting_slider(ui,l.text("Общий лимит вызовов"),&mut self.prefs.calls,min..=24,false,l.text("Бюджет общий для всех ролей. Исследователь и ревьюер не изменяют файлы."),c.muted);
                    }).response,
                    SettingsPage::Appearance=>ui.vertical(|ui|{
                        ui.label(l.text("Тема"));
                        ui.horizontal_wrapped(|ui|{for palette in Palette::ALL{
                            if ui.add(egui::Button::image_and_text(if palette==Palette::Light{Icon::Sun}else{Icon::Moon}.image(palette.colors().accent),l.text(palette.name())).selected(self.prefs.palette==palette)).clicked(){self.prefs.palette=palette;}
                        }});
                        ui.horizontal(|ui|{
                            ui.label(l.text("Язык"));
                            egui::ComboBox::from_id_salt("language").selected_text(if l==Language::Ru{"Русский"}else{"English"}).show_ui(ui,|ui|{ui.selectable_value(&mut self.prefs.language,Language::Ru,"Русский");ui.selectable_value(&mut self.prefs.language,Language::En,"English");});
                        });
                        let animate=self.prefs.animations;
                        icons::toggle(ui,&mut self.prefs.animations,l.text("Мягкие анимации"),animate,c.accent,c.on_accent);
                        ui.label(RichText::new(l.text("Короткие переходы и отклик переключателей. В покое нет постоянных анимаций.")).small().weak());
                        ui.separator();
                        ui.label(RichText::new(l.text("Параметры и выбранные пути сохраняются локально. Задания, код и журнал не сохраняются автоматически.")).small().weak());
                        if let Some(path)=&self.storage{ui.add(egui::Label::new(RichText::new(path.display().to_string()).small().weak()).truncate()).on_hover_text(path.display().to_string());}
                    }).response,
                    SettingsPage::Hardware=>ui.vertical(|ui|{
                        if ui.add_enabled(self.job.is_none(),egui::Button::image_and_text(Icon::Monitor.image(c.accent),l.text("Обновить диагностику"))).clicked()&&let Err(e)=self.start(Operation::Doctor,ui.ctx()){self.fail(e);}
                        self.hardware_view(ui);
                    }).response,
                };
            });
            if let Some(error)=&self.preference_error{ui.label(RichText::new(format!("{}: {error}",l.text("Настройки не сохранены"))).small().color(c.error));}
        });
    }

    fn update_settings(&mut self, ui: &mut egui::Ui) {
        let c = self.prefs.palette.colors();
        let l = self.prefs.language;
        ui.label(format!(
            "Lyra {} · Windows x64 CPU",
            env!("CARGO_PKG_VERSION")
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
            RichText::new(l.text("Модель работает на CPU. Ускорение CUDA проверяется отдельно."))
                .small()
                .weak(),
        );
    }

    fn changes(&mut self, ui: &mut egui::Ui) {
        let c = self.prefs.palette.colors();
        let l = self.prefs.language;
        let Some(report) = &self.report else {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add(Icon::Sparkles.image(c.accent));
                ui.label(l.text("Выбери проект и модель, затем задай задачу."));
            });
            ui.label(
                RichText::new(l.text(
                    "Предложения появятся здесь. Файлы меняются только после твоего подтверждения.",
                ))
                .small()
                .weak(),
            );
            return;
        };
        ui.add(
            egui::Label::new(
                RichText::new(format!(
                    "{}: {}",
                    l.text("Проект отчёта"),
                    report.workspace.display()
                ))
                .small()
                .weak(),
            )
            .truncate(),
        )
        .on_hover_text(report.workspace.display().to_string());
        if report.proposals.is_empty() {
            ui.label(l.text(
                "Применимых предложений нет. Завершение протокола не доказывает решения задачи.",
            ));
        } else {
            ui.horizontal_wrapped(|ui| {
                egui::ComboBox::from_id_salt("proposal")
                    .selected_text(
                        report
                            .proposals
                            .get(self.selected)
                            .map(|e| e.path.as_str())
                            .unwrap_or("—"),
                    )
                    .show_ui(ui, |ui| {
                        for (index, edit) in report.proposals.iter().enumerate() {
                            ui.selectable_value(&mut self.selected, index, &edit.path);
                        }
                    });
                let allowed = self.job.is_none() && can_apply(report, self.selected, &self.applied);
                if ui
                    .add_enabled(
                        allowed,
                        egui::Button::image_and_text(
                            Icon::Check.image(c.accent),
                            l.text("Применить…"),
                        ),
                    )
                    .clicked()
                {
                    self.confirm = Some(self.selected);
                }
                if icons::button(
                    ui,
                    Icon::Save,
                    l.text("Сохранить JSON-отчёт"),
                    false,
                    c.muted,
                )
                .clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .set_file_name("lyra-report.json")
                        .add_filter("JSON", &["json"])
                        .save_file()
                    && let Err(e) = std::fs::write(path, &self.report_json)
                {
                    self.message = e.to_string();
                    self.error = true;
                }
            });
            if self.applied.contains(&self.selected) {
                ui.label(
                    RichText::new(l.text("Применено. Запусти тесты проекта отдельно."))
                        .color(c.success),
                );
            }
            if let Some(edit) = report.proposals.get(self.selected) {
                ui.columns(2, |columns| {
                    columns[0].label(
                        RichText::new(l.text(if edit.create {
                            "Новый файл"
                        } else {
                            "Исходный код"
                        }))
                        .small()
                        .color(c.error),
                    );
                    selectable_code(&mut columns[0], &edit.original, c.text);
                    columns[1].label(
                        RichText::new(l.text("Предложенный код"))
                            .small()
                            .color(c.success),
                    );
                    selectable_code(&mut columns[1], &edit.replacement, c.text);
                });
            }
        }
        if !report.documentation_sources.is_empty() {
            egui::CollapsingHeader::new(l.text("Документация в контексте")).show(ui, |ui| {
                for s in &report.documentation_sources {
                    ui.label(RichText::new(format!("{} · {}", s.title, s.version_scope)).small())
                        .on_hover_text(&s.origin);
                }
            });
        }
        for (role, summary) in &report.summaries {
            egui::CollapsingHeader::new(format!("{}: {role}", l.text("Итог"))).show(ui, |ui| {
                ui.label(summary);
            });
        }
    }

    fn draw(&mut self, ui: &mut egui::Ui) {
        let key = (
            self.prefs.palette,
            self.prefs.animations,
            self.prefs.language,
        );
        if self.style_key != Some(key) {
            theme::configure(ui.ctx(), key.0, key.1);
            self.style_key = Some(key);
        }
        let c = self.prefs.palette.colors();
        let l = self.prefs.language;
        egui::Panel::top("header")
            .frame(
                egui::Frame::new()
                    .fill(c.background)
                    .inner_margin(egui::Margin::symmetric(16, 8)),
            )
            .show(ui, |ui| self.header(ui));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(c.background)
                    .inner_margin(egui::Margin::symmetric(16, 8)),
            )
            .show(ui, |ui| {
                if let Some(start) = self.transition {
                    let t = start.elapsed().as_secs_f32() / 0.14;
                    if self.prefs.animations && t < 1.0 {
                        ui.set_opacity(0.45 + 0.55 * t);
                        ui.ctx().request_repaint_after(Duration::from_millis(16));
                    } else {
                        self.transition = None;
                    }
                }
                if self.settings_open {
                    self.settings(ui);
                } else {
                    self.home(ui);
                }
            });
        self.project_dialog(ui.ctx());
        if let Some(index) = self.confirm {
            let path = self
                .report
                .as_ref()
                .and_then(|r| r.proposals.get(index))
                .map(|e| e.path.clone())
                .unwrap_or_default();
            egui::Modal::new(egui::Id::unique("confirm-apply")).show(ui.ctx(), |ui| {
                ui.heading(l.text("Применить изменение?"));
                ui.label(path);
                ui.label(l.text(
                    "Исходное содержимое будет проверено ещё раз. При конфликте запись отменится.",
                ));
                ui.horizontal(|ui| {
                    if icons::action(ui, Icon::Check, l.text("Применить"), c.accent).clicked()
                    {
                        self.confirm = None;
                        if let Err(e) = self.start(Operation::Apply(index), ui.ctx()) {
                            self.fail(e);
                        }
                    }
                    if icons::action(ui, Icon::Close, l.text("Отмена"), c.muted).clicked() {
                        self.confirm = None;
                    }
                });
            });
        }
        if self.update_confirm {
            egui::Modal::new(egui::Id::unique("confirm-update")).show(ui.ctx(), |ui| {
                ui.set_max_width(430.0);
                ui.heading(l.text("Установить и перезапустить?"));
                ui.label(l.text("Текущая задача и отчёт в памяти не переносятся. Сохрани нужный отчёт перед продолжением."));
                ui.label(l.text("Проекты, модели и локальные настройки сохраняются."));
                ui.horizontal(|ui| {
                    if ui.add_enabled(self.job.is_none(), egui::Button::new(l.text("Установить и перезапустить"))).clicked() {
                        self.update_confirm = false;
                        let saved = self.storage.as_ref().ok_or_else(|| io::Error::other("Settings directory unavailable")).and_then(|p| self.prefs.save(p));
                        match saved {
                            Ok(()) => {self.last_attempted = self.prefs.clone(); self.updates.prepare(ui.ctx());}
                            Err(e) => self.preference_error = Some(e.to_string()),
                        }
                    }
                    if ui.button(l.text("Отмена")).clicked() {self.update_confirm = false;}
                });
            });
        }
        if self.help_open {
            egui::Modal::new(egui::Id::unique("help")).show(ui.ctx(), |ui| {
                ui.heading(l.text("Как пользоваться"));
                ui.label(l.text("1. Выбери папку проекта и instruction-модель GGUF."));
                ui.label(l.text("2. Опиши задачу и запусти. При необходимости останови её."));
                ui.label(l.text(
                    "3. Просмотри предложение, подтверди применение и запусти тесты проекта.",
                ));
                ui.label(
                    RichText::new(l.text(
                        "Модель не запускает shell и тесты. Ревью AI не заменяет проверку кода.",
                    ))
                    .small()
                    .weak(),
                );
                if ui.button(l.text("Понятно")).clicked() {
                    self.help_open = false;
                }
            });
        }
        if self.style_key
            != Some((
                self.prefs.palette,
                self.prefs.animations,
                self.prefs.language,
            ))
        {
            ui.ctx().request_repaint();
        }
        self.save_preferences(ui.ctx());
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
        app.last_attempted = app.prefs.clone();
        app
    }
    fn save_preferences(&mut self, ctx: &egui::Context) {
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
            self.prefs.palette = Palette::ALL[step / 18];
            self.prefs.language = if (step / 9) % 2 == 0 {
                Language::Ru
            } else {
                Language::En
            };
            self.prefs.animations = self.prefs.language == Language::Ru;
            self.settings_open = step % 9 != 0 && step % 9 != 8;
            self.wizard.open = step % 9 == 8;
            self.settings_page = SettingsPage::ALL[(step % 9).saturating_sub(1).min(6)];
        }
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

fn card(c: theme::Colors) -> egui::Frame {
    egui::Frame::new()
        .fill(c.card)
        .stroke(egui::Stroke::new(1.0, c.border))
        .corner_radius(12)
        .inner_margin(12)
}
fn help(ui: &mut egui::Ui, text: &str, color: Color32) {
    ui.add(
        Icon::Help
            .image(color)
            .fit_to_exact_size(egui::vec2(14.0, 14.0)),
    )
    .on_hover_text(text);
}
fn setting_slider(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut u32,
    range: std::ops::RangeInclusive<u32>,
    log: bool,
    tip: &str,
    color: Color32,
) {
    ui.horizontal(|ui| {
        ui.label(label);
        help(ui, tip, color);
    });
    ui.add(egui::Slider::new(value, range).logarithmic(log));
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
    matches!(report.schema_version, 1 | 2)
        && report.status == "completed"
        && report
            .proposals
            .get(index)
            .is_some_and(|p| !p.create || report.schema_version == 2)
        && !applied.contains(&index)
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
        assert!(!can_apply(&report, 1, &[]));
        assert!(!can_apply(&report, 0, &[0]));
        let ctx = egui::Context::default();
        configure(&ctx);
        let mut app = Desktop::default();
        app.report = Some(report);
        app.confirm = Some(0);
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
                    for page in 0usize..9 {
                        app.settings_open = page != 0 && page != 8;
                        app.wizard.open = page == 8;
                        app.settings_page = SettingsPage::ALL[page.saturating_sub(1).min(6)];
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
