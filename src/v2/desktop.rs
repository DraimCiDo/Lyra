pub(super) mod appearance;

use super::migration::ImportBundle;
use super::{TaskReport, TaskRequest, TaskState, TaskStore, Verification, cli::LocalModel};
use crate::{
    desktop::{diff, icons::Icon},
    executor, inference,
    model_gateway::{Message, Role},
};
use appearance::{ACCENT, Appearance, ERROR, MUTED, SUCCESS, WARNING};
use eframe::egui::{self, RichText};
use std::{
    collections::BTreeMap,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const STARTUP: Duration = Duration::from_millis(900);
const STARTUP_STILL: Duration = Duration::from_millis(420);
const TRANSITION: Duration = Duration::from_millis(180);

pub fn launch(smoke: bool, hub: Option<PathBuf>) -> eframe::Result {
    let rendered = Arc::new(AtomicBool::new(false));
    let frame = Arc::clone(&rendered);
    let appearance = if smoke {
        Appearance::default()
    } else {
        data_folder()
            .ok()
            .map(|folder| Appearance::load(&folder))
            .unwrap_or_default()
    };
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1120.0, 780.0])
        .with_min_inner_size([760.0, 580.0])
        .with_transparent(appearance.transparent_window);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(appearance::LOGO) {
        viewport = viewport.with_icon(icon);
    }
    eframe::run_native(
        "Lyra v2",
        eframe::NativeOptions {
            viewport,
            #[cfg(not(windows))]
            renderer: eframe::Renderer::Glow,
            #[cfg(windows)]
            renderer: eframe::Renderer::Wgpu,
            #[cfg(windows)]
            wgpu_options: windows_presentation(appearance.transparent_window),
            ..Default::default()
        },
        Box::new(move |context| {
            egui_extras::install_image_loaders(&context.egui_ctx);
            appearance::configure(&context.egui_ctx, appearance.reduce_motion);
            Ok(Box::new(Desktop::new(
                smoke.then_some(frame),
                appearance,
                hub,
            )))
        }),
    )?;
    if smoke && !rendered.load(Ordering::Relaxed) {
        return Err(eframe::Error::AppCreation(Box::new(io::Error::other(
            "Lyra v2 startup and workspace views did not render",
        ))));
    }
    Ok(())
}

#[cfg(windows)]
fn windows_presentation(transparent: bool) -> eframe::WgpuConfiguration {
    let mut options = eframe::WgpuConfiguration::default();
    if transparent && let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_setup {
        // The HWND swap chain exposes only opaque alpha. DirectComposition supports
        // premultiplied alpha through the same DX12 renderer.
        setup
            .instance_descriptor
            .backend_options
            .dx12
            .presentation_system = wgpu::Dx12SwapchainKind::DxgiFromVisual;
    }
    options
}

enum Output {
    Progress(String),
    Task(PathBuf, Box<TaskReport>),
    ChatDelta(String),
    Chat(String),
    Error(String),
}

struct ChatWriter<'a> {
    sender: &'a mpsc::Sender<Output>,
    content: String,
}

impl Write for ChatWriter<'_> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let text = std::str::from_utf8(data).map_err(io::Error::other)?;
        if !text.is_empty() {
            self.content.push_str(text);
            let _ = self.sender.send(Output::ChatDelta(text.to_owned()));
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct Worker {
    cancel: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

#[derive(Clone, Copy, Default, Eq, PartialEq)]
enum Screen {
    #[default]
    Work,
    Chat,
    Agents,
}

impl Screen {
    fn label(self) -> &'static str {
        match self {
            Self::Work => "Work",
            Self::Chat => "Chat",
            Self::Agents => "Agents",
        }
    }
}

struct Desktop {
    model: Option<PathBuf>,
    repo: Option<PathBuf>,
    checker: Option<PathBuf>,
    state_folder: Option<PathBuf>,
    report: Option<Box<TaskReport>>,
    diff_rows: BTreeMap<usize, Vec<(diff::Tag, String)>>,
    messages: Vec<Message>,
    chat_preview: String,
    import_preview: Option<ImportBundle>,
    imported: Option<ImportBundle>,
    task_draft: String,
    chat_draft: String,
    hub_draft: String,
    status: String,
    status_error: bool,
    screen: Screen,
    screen_entered: Instant,
    appearance: Appearance,
    transparent_window: bool,
    splash_started: Option<Instant>,
    splash_finished: bool,
    pending_hub: Option<PathBuf>,
    settings_open: bool,
    hub: super::hub::desktop::HubView,
    reviewer: bool,
    /// Requested context window; `None` picks the model's trained window up to 8192.
    context: Option<u32>,
    gpu_layers: u32,
    smoke: Option<Arc<AtomicBool>>,
    smoke_step: u8,
    worker: Option<Worker>,
    events: Option<mpsc::Receiver<Output>>,
}

fn data_folder() -> io::Result<PathBuf> {
    #[cfg(windows)]
    let base = std::env::var_os("LOCALAPPDATA");
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_DATA_HOME").or_else(|| {
        std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(".local/share").into_os_string())
    });
    let mut path =
        PathBuf::from(base.ok_or_else(|| io::Error::other("App data location unavailable"))?);
    path.push("lyra-v2");
    Ok(path)
}

fn new_task_folder() -> io::Result<PathBuf> {
    let path = data_folder()?.join("tasks");
    std::fs::create_dir_all(&path)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?;
    Ok(path.join(format!("{}-{}", timestamp.as_nanos(), std::process::id())))
}

fn short_path(path: Option<&Path>) -> String {
    path.map_or_else(
        || "Choose…".into(),
        |path| {
            let name = path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy();
            let mut characters = name.chars();
            let mut label: String = characters.by_ref().take(36).collect();
            if characters.next().is_some() {
                label.push('…');
            }
            label
        },
    )
}

fn choose_path(ui: &mut egui::Ui, label: &str, path: Option<&Path>) -> bool {
    let response = ui.button(format!("{label}  ·  {}", short_path(path)));
    if let Some(path) = path {
        response.on_hover_text(path.display().to_string()).clicked()
    } else {
        response.clicked()
    }
}

impl Desktop {
    fn new(smoke: Option<Arc<AtomicBool>>, appearance: Appearance, hub: Option<PathBuf>) -> Self {
        Self {
            model: None,
            repo: None,
            checker: None,
            state_folder: None,
            report: None,
            diff_rows: BTreeMap::new(),
            messages: Vec::new(),
            chat_preview: String::new(),
            import_preview: None,
            imported: None,
            task_draft: String::new(),
            chat_draft: String::new(),
            hub_draft: String::new(),
            status: String::new(),
            status_error: false,
            screen: if hub.is_some() {
                Screen::Agents
            } else {
                Screen::Work
            },
            screen_entered: Instant::now(),
            appearance,
            transparent_window: appearance.transparent_window,
            splash_started: None,
            splash_finished: false,
            pending_hub: hub,
            settings_open: false,
            hub: Default::default(),
            reviewer: false,
            context: None,
            gpu_layers: 0,
            smoke,
            smoke_step: 0,
            worker: None,
            events: None,
        }
    }

    fn start(
        &mut self,
        work: impl FnOnce(&AtomicBool, &mpsc::Sender<Output>) -> io::Result<()> + Send + 'static,
    ) {
        self.status = "Starting…".into();
        self.status_error = false;
        let (send, receive) = mpsc::channel();
        self.events = Some(receive);
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let thread = thread::spawn(move || {
            if let Err(error) = work(&flag, &send) {
                let _ = send.send(Output::Error(error.to_string()));
            }
        });
        self.worker = Some(Worker { cancel, thread });
    }

    fn collect(&mut self) {
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.thread.is_finished())
            && let Some(worker) = self.worker.take()
            && worker.thread.join().is_err()
        {
            self.status = "The task worker stopped unexpectedly.".into();
            self.status_error = true;
        }
        if let Some(receiver) = &self.events {
            for output in receiver.try_iter() {
                match output {
                    Output::Progress(status) => {
                        self.status = status;
                        self.status_error = false;
                    }
                    Output::Error(status) => {
                        self.status = status;
                        self.status_error = true;
                    }
                    Output::ChatDelta(text) => {
                        self.chat_preview.push_str(&text);
                        self.status.clear();
                    }
                    Output::Chat(content) => {
                        self.chat_preview.clear();
                        self.messages.push(Message {
                            role: Role::Assistant,
                            content,
                        });
                        self.status.clear();
                    }
                    Output::Task(folder, report) => {
                        self.status.clear();
                        self.status_error = false;
                        self.state_folder = Some(folder);
                        self.report = Some(report);
                        self.diff_rows.clear();
                    }
                }
            }
        }
    }

    fn model_args(&self) -> Option<inference::GenerateArgs> {
        let model = self.model.clone()?;
        let trained = crate::resources::gguf_context_length(&model).ok().flatten();
        let context = fit_context(self.context, trained);
        Some(
            LocalModel {
                model,
                context,
                max_tokens: (context / 4).min(1024),
                threads: inference::default_threads(),
                gpu_layers: self.gpu_layers,
                timeout_secs: 300,
            }
            .inference_args(),
        )
    }

    fn submit(&mut self) {
        let Some(model) = self.model_args() else {
            return;
        };
        if self.screen == Screen::Chat {
            self.chat_preview.clear();
            if self
                .messages
                .last()
                .is_some_and(|message| message.role == Role::User)
            {
                self.messages.pop();
            }
            self.messages.push(Message {
                role: Role::User,
                content: std::mem::take(&mut self.chat_draft),
            });
            let messages = self.messages[self.messages.len().saturating_sub(64)..].to_vec();
            self.start(move |cancel, send| {
                let mut response = ChatWriter {
                    sender: send,
                    content: String::new(),
                };
                inference::with_cancellable_session(&model, cancel, |session| {
                    let messages = fit_history(&messages, session.input_token_limit(), |turns| {
                        session.prompt_tokens(turns)
                    })?;
                    session.complete_chat_messages(messages, &mut response)
                })?;
                let _ = send.send(Output::Chat(response.content));
                Ok(())
            });
        } else if let Some(repo) = self.repo.clone() {
            let task = self.task_draft.clone();
            let check = self.checker.clone();
            let reviewer = self.reviewer;
            self.report = None;
            self.diff_rows.clear();
            self.state_folder = None;
            self.start(move |cancel, send| {
                let check = check.as_deref().map(executor::load_config).transpose()?;
                let mut report = TaskReport::new(TaskRequest {
                    repo,
                    task,
                    files: Vec::new(),
                    check,
                    max_calls: 32,
                    reviewer,
                })?;
                let folder = new_task_folder()?;
                let store = TaskStore::create(&folder, &report)?;
                run_task(&model, cancel, send, folder, store, &mut report)
            });
        }
    }

    fn open_state(&mut self, folder: PathBuf) {
        match TaskStore::open(&folder).and_then(|store| store.load()) {
            Ok(report) => {
                self.status.clear();
                self.status_error = false;
                self.repo = Some(report.request.repo.clone());
                self.report = Some(Box::new(report));
                self.diff_rows.clear();
                self.state_folder = Some(folder);
            }
            Err(error) => self.show_error(error),
        }
    }

    fn resume(&mut self) {
        if let (Some(model), Some(folder)) = (self.model_args(), self.state_folder.clone()) {
            self.start(move |cancel, send| {
                let store = TaskStore::open(&folder)?;
                let mut report = store.load()?;
                run_task(&model, cancel, send, folder, store, &mut report)
            });
        }
    }

    fn apply(&mut self) {
        if let Some(folder) = self.state_folder.clone() {
            self.start(move |_, send| {
                let report = TaskStore::open(&folder)?.apply()?;
                let _ = send.send(Output::Task(folder, Box::new(report)));
                Ok(())
            });
        }
    }

    fn show_error(&mut self, error: impl std::fmt::Display) {
        self.status = error.to_string();
        self.status_error = true;
    }

    fn select_screen(&mut self, screen: Screen) {
        if self.screen != screen {
            self.screen = screen;
            self.screen_entered = Instant::now();
            self.status.clear();
            if screen == Screen::Agents {
                self.hub.refresh();
            }
        }
    }

    fn choose_model(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("GGUF model", &["gguf"])
            .pick_file()
        {
            self.model = Some(path);
        }
    }

    fn choose_project(&mut self) {
        if let Some(path) = rfd::FileDialog::new().pick_folder() {
            self.repo = Some(path);
        }
    }

    fn choose_check(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("JSON config", &["json"])
            .pick_file()
        {
            self.checker = Some(path);
        }
    }

    fn import_controls(&mut self, ui: &mut egui::Ui) {
        if ui.button("Import v1 data").clicked()
            && let Some(source) = rfd::FileDialog::new().pick_folder()
        {
            match ImportBundle::preview(&source) {
                Ok(preview) => self.import_preview = Some(preview),
                Err(error) => self.show_error(error),
            }
        }
        if self.imported.is_none()
            && ui.button("Open imported data").clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Lyra import", &["json"])
                .pick_file()
        {
            match ImportBundle::load(&path) {
                Ok(import) => self.imported = Some(import),
                Err(error) => self.show_error(error),
            }
        }
        if let Some(preview) = &self.import_preview {
            let settings = if preview.preferences.is_some() {
                "Model and project settings available"
            } else {
                "No settings available"
            };
            ui.label(format!(
                "{settings} · {} conversations",
                preview.conversations.len()
            ));
            for observation in &preview.observations {
                ui.label(observation);
            }
            if ui.button("Save v2 import").clicked() {
                let mut dialog = rfd::FileDialog::new()
                    .set_file_name("lyra-v1-import.json")
                    .add_filter("Lyra import", &["json"]);
                if let Ok(folder) = data_folder() {
                    if let Err(error) = std::fs::create_dir_all(&folder) {
                        self.show_error(error);
                        return;
                    }
                    dialog = dialog.set_directory(folder);
                }
                if let Some(path) = dialog.save_file() {
                    match preview.install(&path) {
                        Ok(()) => self.imported = self.import_preview.take(),
                        Err(error) => self.show_error(error),
                    }
                }
            }
            if ui.button("Dismiss preview").clicked() {
                self.import_preview = None;
            }
        }
        if let Some(imported) = &self.imported {
            if let Some(settings) = &imported.preferences
                && ui.button("Use imported model and project").clicked()
            {
                self.model.clone_from(&settings.model);
                self.repo.clone_from(&settings.project);
            }
            if self.screen == Screen::Chat && !imported.conversations.is_empty() {
                ui.menu_button("Imported conversations", |ui| {
                    for (index, messages) in imported.conversations.iter().enumerate() {
                        let title = messages
                            .iter()
                            .find(|message| message.role == Role::User)
                            .and_then(|message| message.content.lines().next())
                            .map(|line| line.chars().take(48).collect::<String>())
                            .unwrap_or_else(|| format!("Conversation {}", index + 1));
                        if ui.button(title).clicked() {
                            self.messages.clone_from(messages);
                            self.chat_preview.clear();
                            ui.close();
                        }
                    }
                });
            }
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            appearance::logo(ui, 32.0);
            ui.label(RichText::new("Lyra").size(24.0).strong());
        });
        ui.add_space(26.0);
        for (screen, icon) in [
            (Screen::Work, Icon::Folder),
            (Screen::Chat, Icon::NewChat),
            (Screen::Agents, Icon::Workers),
        ] {
            let active = self.screen == screen;
            let button = egui::Button::image_and_text(
                icon.image(if active { ACCENT } else { MUTED }),
                RichText::new(screen.label()).strong(),
            )
            .selected(active)
            .min_size(egui::vec2(ui.available_width(), 40.0));
            if ui.add_enabled(self.worker.is_none(), button).clicked() {
                self.select_screen(screen);
            }
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.label(RichText::new("Lyra v2").small().color(MUTED));
            if ui
                .add(egui::Button::image_and_text(
                    Icon::Settings.image(MUTED),
                    "Settings",
                ))
                .clicked()
            {
                self.settings_open = !self.settings_open;
            }
        });
    }

    fn settings(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let mut open = self.settings_open;
        egui::Window::new("Settings")
            .open(&mut open)
            .default_width(430.0)
            .resizable(false)
            .show(ctx, |ui| {
                ui.strong("Appearance");
                let motion = ui.checkbox(&mut self.appearance.reduce_motion, "Reduce motion");
                let transparency = ui.checkbox(
                    &mut self.appearance.transparent_window,
                    "Transparent window background",
                );
                appearance::muted(ui, "Window transparency takes effect after restart and depends on desktop support.");
                if motion.changed() {
                    appearance::configure(ctx, self.appearance.reduce_motion);
                }
                if (motion.changed() || transparency.changed())
                    && let Err(error) = data_folder().and_then(|folder| self.appearance.save(&folder))
                {
                    self.show_error(error);
                }
                ui.separator();
                ui.strong("Import from Lyra v1");
                ui.add_enabled_ui(self.worker.is_none(), |ui| self.import_controls(ui));
                if self.status_error {
                    ui.label(RichText::new(&self.status).color(ERROR));
                }
            });
        self.settings_open = open;
    }

    fn controls(&mut self, ui: &mut egui::Ui) {
        ui.heading(self.screen.label());
        ui.add_space(4.0);
        ui.add_enabled_ui(self.worker.is_none(), |ui| {
            ui.horizontal_wrapped(|ui| {
                if self.screen != Screen::Chat && choose_path(ui, "Project", self.repo.as_deref()) {
                    self.choose_project();
                }
                if self.screen != Screen::Agents && choose_path(ui, "Model", self.model.as_deref())
                {
                    self.choose_model();
                }
                if self.screen == Screen::Work
                    && ui.button("Open saved task").clicked()
                    && let Some(folder) = rfd::FileDialog::new().pick_folder()
                {
                    self.open_state(folder);
                }
                if self.screen == Screen::Chat
                    && !self.messages.is_empty()
                    && ui.button("New chat").clicked()
                {
                    self.messages.clear();
                    self.chat_preview.clear();
                    self.chat_draft.clear();
                    self.status.clear();
                }
            });
            if self.screen == Screen::Agents {
                ui.horizontal_wrapped(|ui| {
                    if choose_path(ui, "Check config", self.checker.as_deref()) {
                        self.choose_check();
                    }
                    self.hub
                        .controls(ui, self.repo.as_deref(), self.checker.as_deref());
                });
                return;
            }
            ui.horizontal_wrapped(|ui| {
                #[cfg(feature = "cuda")]
                self.gpu_control(ui);
                egui::ComboBox::from_id_salt("context")
                    .selected_text(match self.context {
                        None => "Context · Auto".to_owned(),
                        Some(value) => format!("Context · {value}"),
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.context, None, "Auto");
                        for value in [2048, 4096, 8192, 16384, 32768] {
                            ui.selectable_value(&mut self.context, Some(value), value.to_string());
                        }
                    })
                    .response
                    .on_hover_text("Limited to the model's trained window");
                if self.screen == Screen::Work {
                    if choose_path(ui, "Check config", self.checker.as_deref()) {
                        self.choose_check();
                    }
                    if self.checker.is_some() && ui.small_button("Clear check").clicked() {
                        self.checker = None;
                    }
                    ui.checkbox(&mut self.reviewer, "Reviewer").on_hover_text(
                        "Ask the model to review the proposed changes before verification",
                    );
                }
            });
        });
    }

    #[cfg(feature = "cuda")]
    fn gpu_control(&mut self, ui: &mut egui::Ui) {
        use crate::resources::{GPU_AUTO, GPU_BALANCED};
        egui::ComboBox::from_id_salt("inference_device")
            .selected_text(match self.gpu_layers {
                GPU_AUTO => "NVIDIA GPU Auto",
                GPU_BALANCED => "NVIDIA GPU Balanced",
                _ => "CPU",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.gpu_layers, 0, "CPU");
                ui.selectable_value(&mut self.gpu_layers, GPU_AUTO, "NVIDIA GPU Auto");
                ui.selectable_value(&mut self.gpu_layers, GPU_BALANCED, "NVIDIA GPU Balanced");
            });
    }

    fn welcome(&mut self, ui: &mut egui::Ui) {
        let chat = self.screen == Screen::Chat;
        ui.add_space(12.0);
        appearance::surface().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            appearance::logo(ui, 48.0);
            ui.label(RichText::new(if chat { "Start a conversation" } else { "Start a coding task" })
                .size(28.0).strong());
            appearance::muted(ui, if chat {
                "Ask a question or reason through a problem with your local model."
            } else {
                "Choose a project and a local model. Review the diff and check results before applying."
            });
            if self.worker.is_none() {
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if !chat && self.repo.is_none()
                        && ui.add(appearance::primary("Choose project")).clicked()
                    {
                        self.choose_project();
                    }
                    if self.model.is_none() && ui.button("Choose GGUF model").clicked() {
                        self.choose_model();
                    }
                    if self.model.is_some() && (chat || self.repo.is_some())
                        && ui.add(appearance::primary(if chat { "Write a message" } else { "Describe the task" })).clicked()
                    {
                        ui.memory_mut(|memory| memory.request_focus(composer_id(self.screen)));
                    }
                });
            }
        });
    }

    fn result(&mut self, ui: &mut egui::Ui) {
        match self.screen {
            Screen::Chat if self.messages.is_empty() => self.welcome(ui),
            Screen::Chat => {
                for message in &self.messages {
                    chat_card(ui, &message.role, &message.content);
                }
                if !self.chat_preview.is_empty() {
                    chat_card(ui, &Role::Assistant, &self.chat_preview);
                }
            }
            Screen::Work => {
                if let Some(report) = &self.report {
                    if task_result(ui, report, &mut self.diff_rows, self.worker.is_none()) {
                        self.apply();
                    }
                } else if self.worker.is_none() {
                    self.welcome(ui);
                } else {
                    appearance::surface().show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.strong("Task in progress");
                        appearance::muted(ui, "Proposals and check results will appear here.");
                    });
                }
            }
            Screen::Agents => self.hub.body(ui, &self.hub_draft),
        }
    }

    fn composer(&mut self, ui: &mut egui::Ui) {
        if self.screen == Screen::Agents {
            self.hub.composer(ui, &mut self.hub_draft);
            return;
        }
        let chat = self.screen == Screen::Chat;
        let ready = self.model.is_some() && (chat || self.repo.is_some());
        let idle = self.worker.is_none();
        if !self.status.is_empty() || !idle {
            ui.horizontal_wrapped(|ui| {
                if !idle {
                    if self.appearance.reduce_motion {
                        ui.label(RichText::new("Working").color(ACCENT));
                    } else {
                        ui.spinner();
                    }
                }
                if !self.status.is_empty() {
                    ui.label(RichText::new(&self.status).color(if self.status_error {
                        ERROR
                    } else {
                        MUTED
                    }));
                }
            });
        }
        let editing = if chat {
            &mut self.chat_draft
        } else {
            &mut self.task_draft
        };
        let edit = ui.add_enabled(
            idle,
            egui::TextEdit::multiline(editing)
                .id(composer_id(self.screen))
                .desired_width(f32::INFINITY)
                .desired_rows(3)
                .hint_text(if chat {
                    "Message Lyra…"
                } else {
                    "Describe the coding task…"
                }),
        );
        let can_submit = ready && !editing.trim().is_empty() && idle;
        let shortcut = edit.has_focus()
            && ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter));
        ui.horizontal_wrapped(|ui| {
            if let Some(worker) = &self.worker {
                let cancelling = worker.cancel.load(Ordering::Relaxed);
                if ui
                    .add_enabled(
                        !cancelling,
                        egui::Button::new(if cancelling {
                            "Cancelling…"
                        } else {
                            "Cancel"
                        }),
                    )
                    .clicked()
                {
                    worker.cancel.store(true, Ordering::Relaxed);
                }
                return;
            }
            if ui
                .add_enabled(
                    can_submit,
                    appearance::primary(if chat { "Send" } else { "Run task" }),
                )
                .clicked()
                || (can_submit && shortcut)
            {
                self.submit();
                return;
            }
            if !ready {
                appearance::muted(
                    ui,
                    if self.model.is_none() {
                        "Choose a GGUF model to continue."
                    } else {
                        "Choose a project to continue."
                    },
                );
            } else {
                appearance::muted(
                    ui,
                    if cfg!(target_os = "macos") {
                        "⌘+Enter to submit"
                    } else {
                        "Ctrl+Enter to submit"
                    },
                );
            }
            if !chat
                && self.model.is_some()
                && self.report.as_ref().is_some_and(|report| {
                    matches!(report.state, TaskState::Running | TaskState::Cancelled)
                })
                && ui.button("Resume task").clicked()
            {
                self.resume();
            }
        });
    }

    fn splash(&mut self, ui: &mut egui::Ui) -> bool {
        if self.splash_finished {
            return false;
        }
        let elapsed = self
            .splash_started
            .get_or_insert_with(Instant::now)
            .elapsed();
        let still = self.appearance.reduce_motion;
        let duration = if still { STARTUP_STILL } else { STARTUP };
        if elapsed >= duration || ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.splash_finished = true;
            return false;
        }
        let progress = elapsed.as_secs_f32() / duration.as_secs_f32();
        let opacity = if still {
            1.0
        } else {
            (1.2 - progress * 1.2).clamp(0.0, 1.0)
        };
        egui::Area::new(egui::Id::unique("lyra_startup"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ui.ctx(), |ui| {
                ui.set_opacity(opacity);
                ui.vertical_centered(|ui| {
                    ui.set_width(200.0);
                    appearance::logo(ui, 98.0);
                    ui.add_space(12.0);
                    ui.label(RichText::new("Lyra").size(43.0).strong());
                    ui.label(RichText::new("Opening workspace").color(MUTED));
                });
            });
        let repaint = if still {
            duration.saturating_sub(elapsed)
        } else {
            Duration::from_millis(16)
        };
        ui.ctx().request_repaint_after(repaint);
        true
    }
}

/// Keep larger windows available, but never exceed the trained context of the model.
fn fit_context(requested: Option<u32>, trained: Option<u32>) -> u32 {
    let requested = requested.unwrap_or(8192);
    trained
        .map_or(requested, |trained| requested.min(trained))
        .clamp(512, 32768)
}

/// Drop the oldest complete turns until the conversation fits the input budget.
/// A conversation that already fits is returned unchanged.
fn fit_history(
    messages: &[Message],
    limit: usize,
    mut tokens: impl FnMut(&[Message]) -> io::Result<usize>,
) -> io::Result<&[Message]> {
    let mut start = 0;
    loop {
        let window = &messages[start..];
        if tokens(window)? <= limit {
            return Ok(window);
        }
        let next = window
            .iter()
            .enumerate()
            .skip(1)
            .find(|(_, message)| message.role == Role::User)
            .map(|(index, _)| start + index);
        match next {
            Some(next) => start = next,
            None => {
                return Err(io::Error::other(
                    "The message exceeds the model context. Shorten it or choose a larger context.",
                ));
            }
        }
    }
}

fn run_task(
    model: &inference::GenerateArgs,
    cancel: &AtomicBool,
    send: &mpsc::Sender<Output>,
    folder: PathBuf,
    store: TaskStore,
    report: &mut TaskReport,
) -> io::Result<()> {
    let _ = send.send(Output::Progress("Loading local model…".into()));
    let result = inference::with_cancellable_session(model, cancel, |session| {
        report.run_local(session, &mut |report| {
            store.save(report)?;
            if let Some(event) = report.events.last() {
                let _ = send.send(Output::Progress(format!(
                    "Step {} · {} · {}",
                    event.call, event.action, event.detail
                )));
            }
            Ok(())
        })
    });
    drop(store);
    let report = TaskStore::open(&folder)?.load()?;
    let _ = send.send(Output::Task(folder, Box::new(report)));
    result
}

impl Drop for Desktop {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.cancel.store(true, Ordering::Relaxed);
            let _ = worker.thread.join();
        }
    }
}

fn composer_id(screen: Screen) -> egui::Id {
    egui::Id::unique(("lyra_composer", screen.label()))
}

fn chat_card(ui: &mut egui::Ui, role: &Role, text: &str) {
    let human = *role == Role::User;
    appearance::surface().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.label(
            RichText::new(if human { "You" } else { "Lyra" })
                .strong()
                .color(if human { ACCENT } else { SUCCESS }),
        );
        if human {
            ui.add(egui::Label::new(text).selectable(true));
        } else {
            for (language, block) in code_blocks(text) {
                if let Some(language) = language {
                    egui::Frame::new()
                        .fill(ui.visuals().extreme_bg_color)
                        .corner_radius(8)
                        .inner_margin(10)
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.horizontal_wrapped(|ui| {
                                appearance::muted(ui, language);
                                if ui.small_button("Copy code").clicked() {
                                    ui.ctx().copy_text(block.to_owned());
                                }
                            });
                            ui.add(
                                egui::Label::new(RichText::new(block).monospace()).selectable(true),
                            );
                        });
                } else {
                    ui.add(egui::Label::new(block).selectable(true));
                }
            }
        }
    });
}

fn code_blocks(mut text: &str) -> Vec<(Option<&str>, &str)> {
    let mut blocks = Vec::new();
    while !text.is_empty() {
        let fence = if text.starts_with("```") {
            Some(0)
        } else {
            text.find("\n```").map(|index| index + 1)
        };
        let Some(fence) = fence else {
            blocks.push((None, text));
            break;
        };
        if fence > 0 {
            blocks.push((None, text[..fence].trim_end()));
        }
        let Some((language, code)) = text[fence + 3..].split_once('\n') else {
            blocks.push((None, &text[fence..]));
            break;
        };
        let language = language.trim();
        let language = if language.is_empty() {
            "Code"
        } else {
            language
        };
        let end = code
            .split_inclusive('\n')
            .scan(0, |position, line| {
                let start = *position;
                *position += line.len();
                Some((start, *position, line))
            })
            .find(|(_, _, line)| line.trim_end() == "```");
        if let Some((start, after, _)) = end {
            blocks.push((Some(language), code[..start].trim_end_matches(['\r', '\n'])));
            text = code[after..].trim_start_matches(['\r', '\n']);
        } else {
            blocks.push((Some(language), code));
            break;
        }
    }
    blocks
}

fn task_result(
    ui: &mut egui::Ui,
    report: &TaskReport,
    diff_rows: &mut BTreeMap<usize, Vec<(diff::Tag, String)>>,
    idle: bool,
) -> bool {
    let (state, tone) = match report.state {
        TaskState::Running => ("Interrupted or running", WARNING),
        TaskState::ReadyToApply => ("Ready for review", ACCENT),
        TaskState::Complete => ("Complete", SUCCESS),
        TaskState::Failed => ("Task failed", ERROR),
        TaskState::Cancelled => ("Cancelled", WARNING),
        TaskState::Applying => ("Publication needs reconciliation", WARNING),
        TaskState::Applied => ("Applied", SUCCESS),
    };
    let mut apply = false;
    appearance::surface().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.heading("Task result");
            appearance::chip(ui, state, tone);
        });
        if !report.summary.is_empty() {
            ui.label(&report.summary);
        }
        appearance::muted(
            ui,
            format!(
                "{} · {} model calls",
                report.request.repo.display(),
                report.calls
            ),
        );
        ui.separator();
        match &report.verification {
            Verification::Skipped => {
                appearance::chip(ui, "Verification skipped", WARNING);
            }
            Verification::Unavailable { message } => {
                ui.label(RichText::new(message).color(WARNING));
            }
            Verification::Passed { result } | Verification::Failed { result } => {
                let (label, color) = if result.passed() {
                    ("Verification passed", SUCCESS)
                } else {
                    ("Verification failed", ERROR)
                };
                ui.collapsing(RichText::new(label).color(color).strong(), |ui| {
                    if !result.stdout.is_empty() {
                        ui.monospace(&result.stdout);
                    }
                    if !result.stderr.is_empty() {
                        ui.monospace(&result.stderr);
                    }
                    if result.stdout.is_empty() && result.stderr.is_empty() {
                        appearance::muted(ui, "No output from the checker.");
                    }
                });
            }
        }
    });
    if !report.proposals.is_empty() {
        appearance::surface().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.strong(format!("Proposed files ({})", report.proposals.len()));
            for (index, proposal) in report.proposals.iter().enumerate() {
                ui.collapsing(&proposal.path, |ui| {
                    let rows = diff_rows.entry(index).or_insert_with(|| {
                        diff::lines(&proposal.original, &proposal.replacement)
                            .into_iter()
                            .map(|(tag, line)| {
                                (tag, line.trim_end_matches(['\n', '\r']).to_owned())
                            })
                            .collect()
                    });
                    show_diff(ui, rows);
                });
            }
            if matches!(report.state, TaskState::ReadyToApply | TaskState::Applying) {
                ui.add_space(4.0);
                apply = ui
                    .add_enabled(idle, appearance::primary("Apply changes"))
                    .clicked();
            }
        });
    }
    apply
}

fn show_diff(ui: &mut egui::Ui, rows: &[(diff::Tag, String)]) {
    let height = ui.text_style_height(&egui::TextStyle::Monospace);
    egui::ScrollArea::both()
        .max_height(320.0)
        .auto_shrink([false, true])
        .show_rows(ui, height, rows.len(), |ui, range| {
            for (tag, line) in &rows[range] {
                let (prefix, color) = match tag {
                    diff::Tag::Same => (" ", MUTED),
                    diff::Tag::Added => ("+", SUCCESS),
                    diff::Tag::Removed => ("-", ERROR),
                };
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("{prefix} {line}"))
                            .monospace()
                            .color(color),
                    )
                    .wrap_mode(egui::TextWrapMode::Extend),
                );
            }
        });
}

impl eframe::App for Desktop {
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn logic(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.collect();
        if self.worker.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        appearance::background(ui.ctx(), self.transparent_window);
        if self.splash(ui) {
            return;
        }
        if let Some(folder) = self.pending_hub.take() {
            self.hub.open(folder);
        }
        let edge = appearance::surface().outer_margin(egui::Margin::same(8));
        egui::Panel::left("lyra_sidebar")
            .exact_size(178.0)
            .frame(edge)
            .show(ui, |ui| self.sidebar(ui));
        egui::Panel::top("lyra_controls")
            .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(16, 12)))
            .show(ui, |ui| self.controls(ui));
        egui::Panel::bottom("lyra_composer")
            .frame(edge)
            .show(ui, |ui| self.composer(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(12, 4)))
            .show(ui, |ui| {
                let elapsed = self.screen_entered.elapsed();
                if !self.appearance.reduce_motion && elapsed < TRANSITION {
                    ui.set_opacity(0.72 + 0.28 * elapsed.as_secs_f32() / TRANSITION.as_secs_f32());
                    ui.ctx().request_repaint_after(Duration::from_millis(16));
                }
                egui::ScrollArea::vertical()
                    .id_salt(self.screen.label())
                    .stick_to_bottom(self.screen == Screen::Chat)
                    .show(ui, |ui| self.result(ui));
            });
        self.settings(ui.ctx());
        if let Some(rendered) = &self.smoke {
            match self.smoke_step {
                0 => self.select_screen(Screen::Chat),
                1 => self.select_screen(Screen::Agents),
                _ => {
                    rendered.store(true, Ordering::Relaxed);
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    return;
                }
            }
            self.smoke_step += 1;
            ui.ctx().request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(role: Role, content: &str) -> Message {
        Message {
            role,
            content: content.into(),
        }
    }

    #[test]
    fn context_follows_the_trained_window_and_the_user_choice() {
        assert_eq!(fit_context(None, Some(4096)), 4096);
        assert_eq!(fit_context(None, Some(131_072)), 8192);
        assert_eq!(fit_context(Some(16384), Some(131_072)), 16384);
        assert_eq!(fit_context(Some(16384), Some(2048)), 2048);
        assert_eq!(fit_context(None, None), 8192);
    }

    #[test]
    fn assistant_code_preserves_indent_and_supports_streaming_fences() {
        assert_eq!(
            code_blocks("Example\n```rust\n  let x = 1;\n```\n\nExplanation"),
            vec![
                (None, "Example"),
                (Some("rust"), "  let x = 1;"),
                (None, "Explanation")
            ]
        );
        assert_eq!(
            code_blocks("```\n  partial"),
            vec![(Some("Code"), "  partial")]
        );
        assert_eq!(code_blocks("```rust"), vec![(None, "```rust")]);
    }

    #[test]
    fn history_drops_whole_old_turns_until_it_fits() {
        let messages = vec![
            message(Role::User, "aaaa"),
            message(Role::Assistant, "bbbb"),
            message(Role::User, "cc"),
            message(Role::Assistant, "dd"),
            message(Role::User, "e"),
        ];
        let count = |turns: &[Message]| Ok(turns.iter().map(|m| m.content.len()).sum());
        assert_eq!(fit_history(&messages, 100, count).unwrap().len(), 5);
        let fitted = fit_history(&messages, 6, count).unwrap();
        assert_eq!(fitted.len(), 3);
        assert_eq!(fitted[0].content, "cc");
        assert!(fit_history(&messages, 0, count).is_err());
    }
}
