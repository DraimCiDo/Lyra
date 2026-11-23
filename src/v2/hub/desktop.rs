//! Owner view of an agent hub in the v2 Desktop: who is working, on what, and the
//! decisions only the owner makes (add, land, reopen, cancel). Hub operations run on a
//! background thread so a slow Git or database wait never freezes the window; the view
//! refreshes after each operation and on demand, never on a timer.
use super::{Hub, OWNER, Status, Task};
use crate::executor;
use crate::v2::desktop::appearance::{self, ACCENT, ERROR, SUCCESS, WARNING};
use eframe::egui::{self, RichText};
use serde_json::Value;
use std::{
    io,
    path::{Path, PathBuf},
    sync::mpsc,
};

/// What a finished background operation produced.
enum Outcome {
    Loaded(Value, Vec<Task>),
    Done(String),
    Diff {
        id: i64,
        commit: String,
        patch: String,
        truncated: bool,
    },
    Inbox(Vec<Value>),
    Created(PathBuf),
}

struct Finished {
    /// The hub folder generation the operation was started for.
    generation: u64,
    outcome: Result<Outcome, String>,
}

#[derive(Default)]
pub struct HubView {
    folder: Option<PathBuf>,
    /// Bumped whenever the folder changes; results of older operations are dropped.
    generation: u64,
    status: Option<Value>,
    tasks: Vec<Task>,
    inbox: Vec<Value>,
    /// The diff the owner is looking at: task, exact commit, patch, and whether it was cut.
    diff: Option<(i64, String, String, bool)>,
    notice: String,
    pending: Option<mpsc::Receiver<Finished>>,
}

impl HubView {
    /// Whether an operation is running (the window should keep repainting).
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }

    /// Apply the result of a finished operation, if any. Call once per frame.
    pub fn poll(&mut self) {
        let Some(receiver) = &self.pending else {
            return;
        };
        let finished = match receiver.try_recv() {
            Ok(finished) => finished,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Finished {
                generation: self.generation,
                outcome: Err("The hub operation stopped unexpectedly".into()),
            },
        };
        self.pending = None;
        if finished.generation != self.generation {
            return;
        }
        match finished.outcome {
            Ok(Outcome::Loaded(status, tasks)) => {
                self.status = Some(status);
                self.tasks = tasks;
            }
            Ok(Outcome::Done(message)) => {
                self.notice = message;
                self.refresh();
            }
            Ok(Outcome::Diff {
                id,
                commit,
                patch,
                truncated,
            }) => self.diff = Some((id, commit, patch, truncated)),
            Ok(Outcome::Inbox(messages)) => self.inbox = messages,
            Ok(Outcome::Created(folder)) => {
                self.switch(folder);
                self.notice = "Hub created".into();
            }
            Err(error) => {
                self.notice = error;
                // The hub may have changed under a failed action; show what is true now.
                if self.status.is_some() {
                    self.refresh();
                }
            }
        }
    }

    /// Run `work` against the owner's hub off the render thread.
    fn spawn(&mut self, work: impl FnOnce(&Hub) -> io::Result<Outcome> + Send + 'static) {
        let Some(folder) = self.folder.clone() else {
            return;
        };
        if self.pending.is_some() {
            return;
        }
        let generation = self.generation;
        let (send, receive) = mpsc::channel();
        self.pending = Some(receive);
        std::thread::spawn(move || {
            let outcome = Hub::open(&folder, OWNER, None)
                .and_then(|hub| work(&hub))
                .map_err(|error| error.to_string());
            let _ = send.send(Finished {
                generation,
                outcome,
            });
        });
    }

    /// Open or create a hub: forget everything that belonged to the previous one first.
    fn switch(&mut self, folder: PathBuf) {
        self.folder = Some(folder);
        self.generation += 1;
        self.pending = None;
        self.status = None;
        self.tasks.clear();
        self.inbox.clear();
        self.diff = None;
        self.notice.clear();
        self.refresh();
    }

    pub fn open(&mut self, folder: PathBuf) {
        self.switch(folder);
    }

    pub fn refresh(&mut self) {
        self.spawn(|hub| Ok(Outcome::Loaded(hub.status()?, hub.tasks(None)?)));
    }

    fn create(&mut self, repo: PathBuf, check: Option<PathBuf>, folder: PathBuf) {
        // Creating does not need an existing hub; run it like any other operation.
        let generation = self.generation;
        let (send, receive) = mpsc::channel();
        self.pending = Some(receive);
        std::thread::spawn(move || {
            let outcome = (|| {
                let check = check.as_deref().map(executor::load_config).transpose()?;
                let base =
                    super::git::ok(&repo, &["symbolic-ref", "--short", "HEAD"]).map_err(|_| {
                        io::Error::other("Check out a local branch in the project first")
                    })?;
                Hub::init(&folder, &repo, &base, check)?;
                Ok(Outcome::Created(folder))
            })()
            .map_err(|error: io::Error| error.to_string());
            let _ = send.send(Finished {
                generation,
                outcome,
            });
        });
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
                self.switch(folder);
            }
            if let Some(repo) = repo
                && !self.busy()
                && ui
                    .button("Create hub for project")
                    .on_hover_text("Uses the selected project and check config")
                    .clicked()
                && let Some(folder) = rfd::FileDialog::new().set_file_name("lyra-hub").save_file()
            {
                self.create(repo.to_owned(), checker.map(Path::to_owned), folder);
            }
            if self.folder.is_some() {
                if ui
                    .add_enabled(!self.busy(), egui::Button::new("Refresh"))
                    .clicked()
                {
                    self.refresh();
                }
                if ui
                    .add_enabled(!self.busy(), egui::Button::new("Inbox"))
                    .clicked()
                {
                    self.spawn(|hub| Ok(Outcome::Inbox(hub.recent_messages(100)?)));
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
        let ready = self.status.is_some() && !self.busy() && !input.trim().is_empty();
        if ui
            .add_enabled(ready, appearance::primary("Add task"))
            .clicked()
        {
            let text = std::mem::take(input);
            let text = text.trim();
            let (title, rest) = text.split_once('\n').unwrap_or((text, ""));
            let description = if rest.trim().is_empty() { title } else { rest };
            let (title, description) = (title.to_owned(), description.to_owned());
            self.spawn(move |hub| {
                let id = hub.create_task(&title, &description, &[])?;
                Ok(Outcome::Done(format!("Task {id} added")))
            });
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
                let idle = self.pending.is_none();
                let reviewed = self
                    .diff
                    .as_ref()
                    .filter(|(id, _, _, truncated)| *id == task.id && !*truncated)
                    .map(|(_, commit, ..)| commit.clone());
                ui.horizontal_wrapped(|ui| match task.status {
                    Status::Review => {
                        if ui
                            .add_enabled(idle, egui::Button::new("View diff").small())
                            .clicked()
                        {
                            action = Some(("diff", task.id, None));
                        }
                        let land = ui.add_enabled(
                            idle && reviewed.is_some(),
                            egui::Button::new("Land reviewed commit").small(),
                        );
                        if reviewed.is_none() {
                            land.clone().on_disabled_hover_text(
                                "View the whole diff first. A cut-off diff cannot be landed here; review it with 'hub diff' and land with 'hub land --commit'.",
                            );
                        }
                        if land.clicked() {
                            action = Some(("land", task.id, reviewed.clone()));
                        }
                        if ui
                            .add_enabled(idle, egui::Button::new("Reopen").small())
                            .on_hover_text("Uses the composer text as the note")
                            .clicked()
                        {
                            action = Some(("reopen", task.id, None));
                        }
                    }
                    Status::Open | Status::Claimed => {
                        if ui
                            .add_enabled(idle, egui::Button::new("Cancel").small())
                            .clicked()
                        {
                            action = Some(("cancel", task.id, None));
                        }
                    }
                    Status::Done | Status::Cancelled => {}
                });
            });
        }
        match action {
            Some(("diff", id, _)) => self.spawn(move |hub| {
                let diff = hub.diff(id, 256 * 1024)?;
                Ok(Outcome::Diff {
                    id,
                    commit: diff["commit"].as_str().unwrap_or_default().into(),
                    patch: diff["patch"].as_str().unwrap_or_default().into(),
                    truncated: diff["truncated"] == true,
                })
            }),
            Some(("land", id, Some(commit))) => self.spawn(move |hub| {
                hub.land(id, &commit)?;
                Ok(Outcome::Done(format!("Task {id} landed")))
            }),
            Some(("reopen", id, _)) => {
                let note = if note.trim().is_empty() {
                    "Reopened by the owner"
                } else {
                    note
                };
                let note = note.to_owned();
                self.spawn(move |hub| {
                    hub.reopen(id, &note)?;
                    Ok(Outcome::Done(format!("Task {id} reopened")))
                });
            }
            Some(("cancel", id, _)) => self.spawn(move |hub| {
                hub.cancel(id)?;
                Ok(Outcome::Done(format!("Task {id} cancelled")))
            }),
            _ => {}
        }
        if let Some((id, commit, patch, truncated)) = &self.diff {
            egui::CollapsingHeader::new(format!(
                "Diff of task {id} · commit {}",
                &commit[..commit.len().min(12)]
            ))
            .default_open(true)
            .show(ui, |ui| {
                if *truncated {
                    ui.label(
                        RichText::new("This diff is cut off; Land is disabled for it.")
                            .color(WARNING),
                    );
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::hub::git;
    use std::time::{Duration, Instant};

    fn hub_folder(root: &Path, name: &str) -> PathBuf {
        let repo = root.join(format!("{name}-repo"));
        std::fs::create_dir(&repo).unwrap();
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["config", "user.name", "T"],
            &["config", "user.email", "t@example.invalid"],
            &["config", "commit.gpgsign", "false"],
        ] {
            git::ok(&repo, args).unwrap();
        }
        std::fs::write(repo.join("a.txt"), "a\n").unwrap();
        git::ok(&repo, &["add", "-A"]).unwrap();
        git::ok(&repo, &["commit", "-q", "-m", "init"]).unwrap();
        let folder = root.join(format!("{name}-hub"));
        Hub::init(&folder, &repo, "main", None).unwrap();
        folder
    }

    fn settle(view: &mut HubView) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while view.busy() {
            assert!(
                Instant::now() < deadline,
                "the hub operation did not finish"
            );
            view.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn switching_hubs_clears_the_previous_hubs_data_and_drops_stale_results() {
        let root = tempfile::tempdir().unwrap();
        let first = hub_folder(root.path(), "first");
        let second = hub_folder(root.path(), "second");
        Hub::open(&first, OWNER, None)
            .unwrap()
            .create_task("Only in the first hub", "x", &[])
            .unwrap();
        let mut view = HubView::default();
        view.switch(first);
        settle(&mut view);
        assert_eq!(view.tasks.len(), 1);
        view.inbox.push(serde_json::json!({"body": "old"}));
        view.diff = Some((1, "abc".into(), "patch".into(), false));

        // A result still in flight for the first hub must not land in the second.
        view.refresh();
        view.switch(second);
        assert!(view.status.is_none() && view.tasks.is_empty());
        assert!(view.inbox.is_empty() && view.diff.is_none());
        settle(&mut view);
        assert!(view.tasks.is_empty());
        assert!(view.status.is_some());

        // A folder that cannot be loaded leaves no task actions behind.
        view.switch(root.path().join("missing-hub"));
        settle(&mut view);
        assert!(view.status.is_none() && view.tasks.is_empty());
        assert!(!view.notice.is_empty());
    }
}
