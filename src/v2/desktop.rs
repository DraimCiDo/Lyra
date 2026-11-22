use super::migration::ImportBundle;
use super::{TaskReport, TaskRequest, TaskState, TaskStore, Verification, cli::LocalModel};
use crate::{
    desktop::diff,
    executor, inference,
    model_gateway::{Message, Role},
};
use eframe::egui::{self, RichText};
use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub fn launch(smoke: bool) -> eframe::Result {
    let rendered = Arc::new(AtomicBool::new(false));
    let frame = Arc::clone(&rendered);
    eframe::run_native(
        "Lyra v2",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1020.0, 740.0])
                .with_min_inner_size([710.0, 540.0]),
            #[cfg(not(windows))]
            renderer: eframe::Renderer::Glow,
            #[cfg(windows)]
            renderer: eframe::Renderer::Wgpu,
            ..Default::default()
        },
        Box::new(move |context| {
            context.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(Desktop::new(smoke.then_some(frame))))
        }),
    )?;
    if smoke && !rendered.load(Ordering::Relaxed) {
        return Err(eframe::Error::AppCreation(Box::new(io::Error::other(
            "Lyra v2 window did not render",
        ))));
    }
    Ok(())
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

struct Desktop {
    model: Option<PathBuf>,
    repo: Option<PathBuf>,
    checker: Option<PathBuf>,
    state_folder: Option<PathBuf>,
    report: Option<Box<TaskReport>>,
    messages: Vec<Message>,
    chat_preview: String,
    import_preview: Option<ImportBundle>,
    imported: Option<ImportBundle>,
    input: String,
    status: String,
    chat: bool,
    reviewer: bool,
    /// Requested context window; `None` picks the model's trained window up to 8192.
    context: Option<u32>,
    smoke: Option<Arc<AtomicBool>>,
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
            path.file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
                .into_owned()
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
    fn new(smoke: Option<Arc<AtomicBool>>) -> Self {
        Self {
            model: None,
            repo: None,
            checker: None,
            state_folder: None,
            report: None,
            messages: Vec::new(),
            chat_preview: String::new(),
            import_preview: None,
            imported: None,
            input: String::new(),
            status: String::new(),
            chat: false,
            reviewer: false,
            context: None,
            smoke,
            worker: None,
            events: None,
        }
    }

    fn start(
        &mut self,
        work: impl FnOnce(&AtomicBool, &mpsc::Sender<Output>) -> io::Result<()> + Send + 'static,
    ) {
        self.status = "Starting…".into();
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
        }
        if let Some(receiver) = &self.events {
            for output in receiver.try_iter() {
                match output {
                    Output::Progress(status) | Output::Error(status) => self.status = status,
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
                        self.status = report.summary.clone();
                        self.state_folder = Some(folder);
                        self.report = Some(report);
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
                timeout_secs: 300,
            }
            .inference_args(),
        )
    }

    fn submit(&mut self) {
        let Some(model) = self.model_args() else {
            return;
        };
        if self.chat {
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
                content: std::mem::take(&mut self.input),
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
            let task = self.input.clone();
            let check = self.checker.clone();
            let reviewer = self.reviewer;
            self.report = None;
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
                self.status = report.summary.clone();
                self.repo = Some(report.request.repo.clone());
                self.report = Some(Box::new(report));
                self.state_folder = Some(folder);
            }
            Err(error) => self.status = error.to_string(),
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

    fn import_controls(&mut self, ui: &mut egui::Ui) {
        if ui.button("Import v1 data").clicked()
            && let Some(source) = rfd::FileDialog::new().pick_folder()
        {
            match ImportBundle::preview(&source) {
                Ok(preview) => self.import_preview = Some(preview),
                Err(error) => self.status = error.to_string(),
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
                Err(error) => self.status = error.to_string(),
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
                        self.status = error.to_string();
                        return;
                    }
                    dialog = dialog.set_directory(folder);
                }
                if let Some(path) = dialog.save_file() {
                    match preview.install(&path) {
                        Ok(()) => self.imported = self.import_preview.take(),
                        Err(error) => self.status = error.to_string(),
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
            if self.chat && !imported.conversations.is_empty() {
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

    fn controls(&mut self, ui: &mut egui::Ui) {
        let idle = self.worker.is_none();
        ui.horizontal(|ui| {
            ui.heading("Lyra v2");
            ui.add_space(18.0);
            ui.add_enabled_ui(idle, |ui| {
                ui.selectable_value(&mut self.chat, false, "Work");
                ui.selectable_value(&mut self.chat, true, "Chat");
            });
        });
        ui.add_space(6.0);
        ui.add_enabled_ui(idle, |ui| {
            ui.horizontal_wrapped(|ui| {
                if choose_path(ui, "GGUF", self.model.as_deref())
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("GGUF model", &["gguf"])
                        .pick_file()
                {
                    self.model = Some(path);
                }
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
                if self.chat && !self.messages.is_empty() && ui.button("New chat").clicked() {
                    self.messages.clear();
                    self.chat_preview.clear();
                }
                if !self.chat {
                    ui.checkbox(&mut self.reviewer, "Reviewer");
                    if choose_path(ui, "Project", self.repo.as_deref())
                        && let Some(path) = rfd::FileDialog::new().pick_folder()
                    {
                        self.repo = Some(path);
                    }
                    if choose_path(ui, "Check config", self.checker.as_deref())
                        && let Some(path) = rfd::FileDialog::new()
                            .add_filter("JSON config", &["json"])
                            .pick_file()
                    {
                        self.checker = Some(path);
                    }
                    if self.checker.is_some() && ui.small_button("Clear check").clicked() {
                        self.checker = None;
                    }
                    if ui.button("Open saved task").clicked()
                        && let Some(folder) = rfd::FileDialog::new().pick_folder()
                    {
                        self.open_state(folder);
                    }
                }
            });
            ui.horizontal_wrapped(|ui| self.import_controls(ui));
        });
    }

    fn result(&self, ui: &mut egui::Ui) {
        if self.chat {
            for message in &self.messages {
                ui.strong(if message.role == Role::User {
                    "You"
                } else {
                    "Lyra"
                });
                ui.label(&message.content);
                ui.add_space(8.0);
            }
            if !self.chat_preview.is_empty() {
                ui.strong("Lyra");
                ui.label(&self.chat_preview);
            }
        } else if let Some(report) = &self.report {
            ui.label(format!(
                "{} · {} model calls",
                report.request.repo.display(),
                report.calls
            ));
            ui.label(match report.state {
                TaskState::Running => "Interrupted or running",
                TaskState::ReadyToApply => "Ready for review",
                TaskState::Complete => "Complete",
                TaskState::Failed => "Task failed",
                TaskState::Cancelled => "Cancelled",
                TaskState::Applying => "Publication needs reconciliation",
                TaskState::Applied => "Applied",
            });
            match &report.verification {
                Verification::Skipped => {
                    ui.label("Verification skipped");
                }
                Verification::Unavailable { message } => {
                    ui.label(message);
                }
                Verification::Passed { result } | Verification::Failed { result } => {
                    ui.collapsing(
                        if result.passed() {
                            "Verification passed"
                        } else {
                            "Verification failed"
                        },
                        |ui| {
                            ui.monospace(&result.stdout);
                            ui.monospace(&result.stderr);
                        },
                    );
                }
            }
            for proposal in &report.proposals {
                ui.collapsing(&proposal.path, |ui| {
                    for (tag, line) in diff::lines(&proposal.original, &proposal.replacement) {
                        let (prefix, color) = match tag {
                            diff::Tag::Same => (" ", ui.visuals().text_color()),
                            diff::Tag::Added => ("+", egui::Color32::LIGHT_GREEN),
                            diff::Tag::Removed => ("-", egui::Color32::LIGHT_RED),
                        };
                        ui.label(
                            RichText::new(format!("{prefix} {line}"))
                                .monospace()
                                .color(color),
                        );
                    }
                });
            }
        }
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

impl eframe::App for Desktop {
    fn logic(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.collect();
        if self.worker.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        egui::Panel::top("controls").show(ui, |ui| self.controls(ui));
        egui::Panel::bottom("composer").show(ui, |ui| {
            if !self.status.is_empty() {
                ui.label(&self.status);
            }
            ui.add(
                egui::TextEdit::multiline(&mut self.input)
                    .desired_width(f32::INFINITY)
                    .desired_rows(3)
                    .hint_text(if self.chat {
                        "Message Lyra"
                    } else {
                        "Describe the coding task"
                    }),
            );
            ui.horizontal(|ui| {
                if let Some(worker) = &self.worker {
                    if ui.button("Cancel").clicked() {
                        worker.cancel.store(true, Ordering::Relaxed);
                    }
                    return;
                }
                let can_submit = self.model.is_some()
                    && !self.input.trim().is_empty()
                    && (self.chat || self.repo.is_some());
                if ui
                    .add_enabled(
                        can_submit,
                        egui::Button::new(if self.chat { "Send" } else { "Run task" }),
                    )
                    .clicked()
                {
                    self.submit();
                }
                if !self.chat
                    && let Some(report) = &self.report
                {
                    match report.state {
                        TaskState::ReadyToApply | TaskState::Applying => {
                            if ui.button("Apply changes").clicked() {
                                self.apply();
                            }
                        }
                        TaskState::Running | TaskState::Cancelled
                            if self.model.is_some() && ui.button("Resume task").clicked() =>
                        {
                            self.resume();
                        }
                        _ => {}
                    }
                }
            });
        });
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.result(ui));
        });
        if let Some(rendered) = &self.smoke {
            rendered.store(true, Ordering::Relaxed);
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
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
