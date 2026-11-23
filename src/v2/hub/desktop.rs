//! Owner view of an agent hub in the v2 Desktop: who is working, on what, and the
//! decisions only the owner makes (add, land, reopen, cancel). Refreshes on demand only.
use super::{Hub, OWNER, Status, Task};
use crate::executor;
use crate::v2::desktop::appearance::{self, ACCENT, ERROR, SUCCESS, WARNING};
use eframe::egui::{self, RichText};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct HubView {
    folder: Option<PathBuf>,
    status: Option<Value>,
    tasks: Vec<Task>,
    inbox: Vec<Value>,
    diff: Option<(i64, String)>,
    notice: String,
}

impl HubView {
    fn hub(&self) -> Option<Result<Hub, String>> {
        self.folder
            .as_deref()
            .map(|folder| Hub::open(folder, OWNER, None).map_err(|error| error.to_string()))
    }

    pub fn open(&mut self, folder: PathBuf) {
        self.folder = Some(folder);
        self.diff = None;
        self.refresh();
    }

    pub fn refresh(&mut self) {
        let Some(hub) = self.hub() else { return };
        let loaded = hub.and_then(|hub| {
            let status = hub.status().map_err(|e| e.to_string())?;
            let tasks = hub.tasks(None).map_err(|e| e.to_string())?;
            Ok((status, tasks))
        });
        match loaded {
            Ok((status, tasks)) => {
                self.status = Some(status);
                self.tasks = tasks;
            }
            Err(error) => self.notice = error,
        }
    }

    fn act(&mut self, action: impl FnOnce(&Hub) -> std::io::Result<String>) {
        if let Some(hub) = self.hub() {
            self.notice = match hub.and_then(|hub| action(&hub).map_err(|e| e.to_string())) {
                Ok(message) | Err(message) => message,
            };
        }
        self.refresh();
    }

    pub fn controls(&mut self, ui: &mut egui::Ui, repo: Option<&Path>, checker: Option<&Path>) {
        ui.horizontal_wrapped(|ui| {
            let label = self
                .folder
                .as_deref()
                .map_or("Open hub…".to_owned(), |folder| {
                    format!(
                        "Hub · {}",
                        folder.file_name().unwrap_or_default().to_string_lossy()
                    )
                });
            if ui.button(label).clicked()
                && let Some(folder) = rfd::FileDialog::new().pick_folder()
            {
                self.open(folder);
            }
            if let Some(repo) = repo
                && ui
                    .button("Create hub for project")
                    .on_hover_text("Uses the selected project and check config")
                    .clicked()
                && let Some(folder) = rfd::FileDialog::new().set_file_name("lyra-hub").save_file()
            {
                let created = checker
                    .map(executor::load_config)
                    .transpose()
                    .and_then(|check| {
                        let base = super::git::ok(repo, &["rev-parse", "--abbrev-ref", "HEAD"])?;
                        Hub::init(&folder, repo, &base, check)
                    });
                match created {
                    Ok(()) => {
                        self.folder = Some(folder);
                        self.notice = "Hub created".into();
                        self.refresh();
                    }
                    Err(error) => self.notice = error.to_string(),
                }
            }
            if self.folder.is_some() {
                if ui.button("Refresh").clicked() {
                    self.refresh();
                }
                if ui.button("Inbox").clicked()
                    && let Some(Ok(hub)) = self.hub()
                {
                    match hub.read(100) {
                        Ok(messages) => self.inbox.extend(messages),
                        Err(error) => self.notice = error.to_string(),
                    }
                }
            }
        });
    }

    /// The composer adds a task: the first line is the title, the rest the description.
    pub fn composer(&mut self, ui: &mut egui::Ui, input: &mut String) {
        if !self.notice.is_empty() {
            ui.label(&self.notice);
        }
        ui.add(
            egui::TextEdit::multiline(input)
                .desired_width(f32::INFINITY)
                .desired_rows(3)
                .hint_text("New task: first line is the title, then the description"),
        );
        let ready = self.folder.is_some() && !input.trim().is_empty();
        if ui
            .add_enabled(ready, appearance::primary("Add task"))
            .clicked()
        {
            let text = input.trim();
            let (title, rest) = text.split_once('\n').unwrap_or((text, ""));
            let description = if rest.trim().is_empty() { title } else { rest };
            if let Some(result) = self.hub() {
                match result.and_then(|hub| {
                    hub.create_task(title, description, &[])
                        .map_err(|e| e.to_string())
                }) {
                    Ok(id) => {
                        input.clear();
                        self.notice = format!("Task {id} added");
                    }
                    Err(error) => self.notice = error,
                }
                self.refresh();
            }
        }
    }

    pub fn body(&mut self, ui: &mut egui::Ui, note: &str) {
        let Some(status) = &self.status else {
            appearance::surface().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.heading("Set up an agent hub");
                appearance::muted(ui, "Open an existing hub. To create one, choose a project and select Create hub for project.");
            });
            return;
        };
        appearance::surface().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.strong("Connected agents");
                let configured = status["check_configured"] == true;
                appearance::chip(
                    ui,
                    if configured {
                        "Check configured"
                    } else {
                        "No check configured"
                    },
                    if configured { SUCCESS } else { WARNING },
                );
            });
            appearance::muted(
                ui,
                format!(
                    "{} · base {}",
                    status["project"].as_str().unwrap_or_default(),
                    status["base"].as_str().unwrap_or_default()
                ),
            );
            let agents = status["agents"].as_array().map_or(&[][..], Vec::as_slice);
            if agents.is_empty() {
                appearance::muted(ui, "No agent has connected yet.");
            }
            for agent in agents {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!(
                        "{} · {}",
                        agent["name"].as_str().unwrap_or_default(),
                        agent["kind"].as_str().unwrap_or_default()
                    ));
                    let active = agent["active"] == true;
                    appearance::chip(
                        ui,
                        if active { "Active" } else { "Idle" },
                        if active { SUCCESS } else { ACCENT },
                    );
                });
            }
            for lease in status["leases"].as_array().into_iter().flatten() {
                appearance::muted(
                    ui,
                    format!(
                        "Lease · {} · {}",
                        lease["path"].as_str().unwrap_or_default(),
                        lease["agent"].as_str().unwrap_or_default()
                    ),
                );
            }
        });
        ui.strong("Task queue");
        if self.tasks.is_empty() {
            appearance::muted(ui, "Add a task to the queue below.");
        }
        let mut action = None;
        for task in &self.tasks {
            appearance::surface().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.strong(format!("#{} {}", task.id, task.title));
                    let (label, color) = match task.status {
                        Status::Open => ("Queued", ACCENT),
                        Status::Claimed => ("In progress", ACCENT),
                        Status::Review => ("Ready for review", WARNING),
                        Status::Done => ("Landed", SUCCESS),
                        Status::Cancelled => ("Cancelled", WARNING),
                    };
                    appearance::chip(ui, label, color);
                    if let Some(check) = &task.check {
                        let passed = check["passed"] == true;
                        appearance::chip(
                            ui,
                            if passed {
                                "Check passed"
                            } else {
                                "Check failed"
                            },
                            if passed { SUCCESS } else { ERROR },
                        );
                    }
                });
                if let Some(owner) = &task.owner {
                    appearance::muted(ui, owner);
                }
                if !task.summary.is_empty() {
                    ui.label(&task.summary);
                }
                ui.horizontal_wrapped(|ui| match task.status {
                    Status::Review => {
                        if ui.small_button("View diff").clicked() {
                            action = Some(("diff", task.id));
                        }
                        if ui.small_button("Land").clicked() {
                            action = Some(("land", task.id));
                        }
                        if ui
                            .small_button("Reopen")
                            .on_hover_text("Uses the composer text as the note")
                            .clicked()
                        {
                            action = Some(("reopen", task.id));
                        }
                    }
                    Status::Open | Status::Claimed => {
                        if ui.small_button("Cancel").clicked() {
                            action = Some(("cancel", task.id));
                        }
                    }
                    Status::Done | Status::Cancelled => {}
                });
            });
        }
        match action {
            Some(("diff", id)) => {
                if let Some(Ok(hub)) = self.hub() {
                    match hub.diff(id, 256 * 1024) {
                        Ok(diff) => {
                            self.diff =
                                Some((id, diff["patch"].as_str().unwrap_or_default().into()))
                        }
                        Err(error) => self.notice = error.to_string(),
                    }
                }
            }
            Some(("land", id)) => self.act(|hub| hub.land(id).map(|_| format!("Task {id} landed"))),
            Some(("reopen", id)) => {
                let note = if note.trim().is_empty() {
                    "Reopened by the owner"
                } else {
                    note
                };
                let note = note.to_owned();
                self.act(|hub| {
                    hub.update(id, Status::Open, &note)?;
                    hub.send(None, Some(id), &note)?;
                    Ok(format!("Task {id} reopened"))
                });
            }
            Some(("cancel", id)) => {
                self.act(|hub| hub.cancel(id).map(|_| format!("Task {id} cancelled")))
            }
            _ => {}
        }
        if let Some((id, patch)) = &self.diff {
            egui::CollapsingHeader::new(format!("Diff of task {id}"))
                .default_open(true)
                .show(ui, |ui| {
                    for line in patch.lines() {
                        let color = if line.starts_with('+') {
                            SUCCESS
                        } else if line.starts_with('-') {
                            ERROR
                        } else {
                            ui.visuals().text_color()
                        };
                        ui.label(RichText::new(line).monospace().color(color));
                    }
                });
        }
        if !self.inbox.is_empty() {
            ui.add_space(6.0);
            ui.strong("Messages (untrusted text from agents)");
            for message in &self.inbox {
                ui.label(format!(
                    "{}: {}",
                    message["from"].as_str().unwrap_or_default(),
                    message["body"].as_str().unwrap_or_default()
                ));
            }
        }
    }
}
