//! Owner view of an agent hub in the v2 Desktop: who is working, on what, and the
//! decisions only the owner makes (add, land, reopen, cancel). Hub operations run on a
//! background thread so a slow Git or database wait never freezes the window; the view
//! refreshes after each operation and on demand, never on a timer.
use super::{Hub, OWNER, Status, Task, team, plan};
use crate::inference;
use crate::executor;
use crate::v2::desktop::appearance::{self, ACCENT, ERROR, SUCCESS, WARNING};
use eframe::egui::{self, RichText};
use serde_json::Value;
use std::{
    io,
    path::{Path, PathBuf},
    sync::{mpsc, Arc, atomic::{AtomicBool, Ordering}},
    time::{Duration, Instant},
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
    Plan(String),
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
    /// Whether the running (or last) operation was a plain refresh.
    last_was_refresh: bool,
    team_config: team::Config,
    team: Option<team::Team>,
    team_events: Vec<team::Event>,
    next_refresh: Option<Instant>,
    plan_text: String,
    planning: bool,
    plan_cancel: Arc<AtomicBool>,
}

impl HubView {
    /// Whether an operation is running (the window should keep repainting).
    pub fn busy(&self) -> bool {
        self.pending.is_some() || self.team.as_ref().is_some_and(team::Team::running)
    }

    /// Apply the result of a finished operation, if any. Call once per frame.
    pub fn poll(&mut self) {
        if let Some(team) = &self.team {
            self.team_events = team.events();
            if team.running() && self.pending.is_none()
                && self.next_refresh.is_none_or(|at| Instant::now() >= at) {
                self.next_refresh = Some(Instant::now() + Duration::from_secs(1));
                self.refresh();
            }
        }
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
            Ok(Outcome::Plan(text)) => {
                self.plan_text = text;
                self.planning = false;
                self.notice = "Review and edit the proposed tasks before approving.".into();
            }
            Ok(Outcome::Loaded(status, tasks)) => {
                self.status = Some(status);
                self.tasks = tasks;
            }
            Ok(Outcome::Done(message)) => {
                if message.starts_with("Approved ") { self.plan_text.clear(); }
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
                self.planning = false;
                self.notice = error;
                // The hub may have changed under a failed action; show what is true now.
                // A failed refresh is not retried here: that would loop on every frame.
                if self.status.is_some() && !self.last_was_refresh {
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
        self.last_was_refresh = false;
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
        if let Some(team) = &self.team { team.stop(); }
        self.team = None;
        self.team_events.clear();
        self.plan_cancel.store(true, Ordering::Relaxed);
        self.plan_cancel = Arc::new(AtomicBool::new(false));
        self.planning = false;
        self.plan_text.clear();
        self.folder = Some(folder);
        self.generation += 1;
        self.pending = None;
        self.status = None;
        self.tasks.clear();
        self.inbox.clear();
        self.diff = None;
        self.notice.clear();
        let config_path = self.folder.as_ref().map(|p| p.join("team.json"));
        self.team_config = team::Config::default();
        if let Some(path) = config_path.filter(|p| p.exists()) {
            match team::Config::load(&path) {
                Ok(config) => self.team_config = config,
                Err(error) => self.notice = format!("Team configuration: {error}"),
            }
        }
        self.refresh();
    }

    pub fn open(&mut self, folder: PathBuf) {
        self.switch(folder);
    }

    pub fn refresh(&mut self) {
        self.spawn(|hub| Ok(Outcome::Loaded(hub.status()?, hub.tasks(None)?)));
        self.last_was_refresh = self.pending.is_some();
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
            if ui
                .add_enabled(!self.busy(), egui::Button::new(label))
                .clicked()
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

    fn team_controls(&mut self, ui: &mut egui::Ui, goal: &str, model: Option<inference::GenerateArgs>) {
        let running = self.team.as_ref().is_some_and(team::Team::running);
        let idle = self.pending.is_none() && !running;
        appearance::surface().show(ui, |ui| {
            ui.strong("Managed team");
            ui.label("Lyra assigns worktrees, runs workers and checks, and sends results to review. You land changes.");
            ui.add_enabled_ui(idle, |ui| {
                egui::CollapsingHeader::new("Workers and limits").default_open(true).show(ui, |ui| {
                    let mut remove = None;
                    for (index, member) in self.team_config.members.iter_mut().enumerate() {
                        ui.push_id(index, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(format!("{:?}", member.provider));
                                ui.add(egui::TextEdit::singleline(&mut member.name).desired_width(95.0).hint_text("Name"));
                                ui.add(egui::TextEdit::singleline(&mut member.model).desired_width(180.0).hint_text("Explicit model / GGUF path"));
                                ui.add(egui::TextEdit::singleline(&mut member.reasoning).desired_width(90.0).hint_text("Effort"));
                                if ui.small_button("Remove").clicked() { remove = Some(index); }
                            });
                            if member.provider != team::Provider::Local {
                                ui.horizontal(|ui| {
                                    ui.label("CLI executable");
                                    let mut executable = member.executable.to_string_lossy().into_owned();
                                    if ui.text_edit_singleline(&mut executable).changed() { member.executable = executable.into(); }
                                });
                            }
                        });
                    }
                    if let Some(index) = remove { self.team_config.members.remove(index); }
                    ui.horizontal_wrapped(|ui| {
                        for (label, provider, executable) in [("Add Codex", team::Provider::Codex, "codex"), ("Add Claude", team::Provider::Claude, "claude")] {
                            if ui.add_enabled(self.team_config.members.len() < 8, egui::Button::new(label)).clicked() {
                                let name = format!("{}-{}", executable, self.team_config.members.len() + 1);
                                self.team_config.members.push(team::Member { name, provider, executable: executable.into(), model: String::new(), reasoning: String::new() });
                            }
                        }
                        if ui.add_enabled(self.team_config.members.len() < 8 && model.is_some(), egui::Button::new("Add selected local model")).clicked()
                            && let Some(model) = &model {
                            self.team_config.context = model.context;
                            self.team_config.gpu_layers = model.gpu_layers;
                            self.team_config.members.push(team::Member { name: format!("lyra-{}", self.team_config.members.len() + 1),
                                provider: team::Provider::Local, executable: "lyra-v2".into(), model: model.model.to_string_lossy().into_owned(), reasoning: String::new() });
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Seconds per worker task");
                        ui.add(egui::DragValue::new(&mut self.team_config.timeout_secs).range(1..=3600));
                        ui.label("Tasks per run");
                        ui.add(egui::DragValue::new(&mut self.team_config.max_tasks).range(1..=64));
                        ui.label("Local calls / Claude turns");
                        ui.add(egui::DragValue::new(&mut self.team_config.max_calls).range(1..=64));
                        ui.label("Local context");
                        ui.add(egui::DragValue::new(&mut self.team_config.context).range(512..=32768));
                        ui.label("GPU layers");
                        ui.add(egui::DragValue::new(&mut self.team_config.gpu_layers).range(0..=1024));
                    });
                    ui.checkbox(&mut self.team_config.allow_external, "Enable external agents: allow their CLI to send project context and use my account quota");
                    appearance::muted(ui, "Codex is bounded by time and task count; its CLI has no per-task turn limit. Each local worker loads its own model. CLI authentication stays with the installed provider.");
                });
            });
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(idle, appearance::primary("Start team")).clicked() {
                    let result = (|| {
                        let folder = self.folder.clone().ok_or_else(|| io::Error::other("Open a hub first"))?;
                        self.team_config.validate()?;
                        let lyra = team::cli_executable()?;
                        let team = team::Team::start(folder.clone(), self.team_config.clone(), lyra)?;
                        // Save only on an explicit start. No tokens or conversations are stored.
                        let bytes = serde_json::to_vec_pretty(&self.team_config).map_err(io::Error::other)?;
                        let mut file = tempfile::NamedTempFile::new_in(&folder)?;
                        use std::io::Write;
                        file.write_all(&bytes)?;
                        file.persist(folder.join("team.json")).map_err(|e| e.error)?;
                        Ok(team)
                    })();
                    match result {
                        Ok(team) => { self.team = Some(team); self.next_refresh = None; self.notice = "Team started".into(); }
                        Err(error) => self.notice = format!("Cannot start team: {error}"),
                    }
                }
                if ui.add_enabled(running, egui::Button::new("Stop team")).clicked() {
                    if let Some(team) = &self.team { team.stop(); }
                    self.notice = "Stopping workers; worktrees will be preserved".into();
                }
                if ui.add_enabled(idle && model.is_some() && !goal.trim().is_empty(), egui::Button::new("Propose task breakdown")).clicked()
                    && let Some(model) = model {
                    let goal = goal.to_owned();
                    self.plan_cancel = Arc::new(AtomicBool::new(false));
                    let cancel = self.plan_cancel.clone();
                    self.spawn(move |_| {
                        let plan = plan::propose(&goal, &model, &cancel)?;
                        Ok(Outcome::Plan(serde_json::to_string_pretty(&plan).map_err(io::Error::other)?))
                    });
                    self.planning = true;
                    self.notice = "Local model is proposing tasks from the composer goal".into();
                }
                if self.planning && ui.button("Cancel planning").clicked() { self.plan_cancel.store(true, Ordering::Relaxed); }
            });
            if !self.plan_text.is_empty() {
                ui.label("Proposed tasks — editable JSON. Dependencies are one-based positions of earlier tasks. Approving adds tasks; Start team runs them.");
                ui.add_enabled(idle, egui::TextEdit::multiline(&mut self.plan_text).code_editor().desired_rows(8).desired_width(f32::INFINITY));
                if ui.add_enabled(idle, egui::Button::new("Approve and enqueue plan")).clicked() {
                    let text = self.plan_text.clone();
                    self.spawn(move |hub| {
                        let ids = plan::Plan::parse(&text)?.enqueue(hub)?;
                        Ok(Outcome::Done(format!("Approved {} tasks", ids.len())))
                    });
                }
            }
            if !self.team_events.is_empty() {
                egui::CollapsingHeader::new("Team activity").default_open(true).show(ui, |ui| {
                    for event in self.team_events.iter().rev().take(30) {
                        ui.label(format!("{} · {}{} · {}", event.agent, event.state,
                            event.task.map_or(String::new(), |id| format!(" · #{id}")), event.detail));
                    }
                });
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
        let ready = self.status.is_some() && self.pending.is_none() && !input.trim().is_empty();
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

    pub fn body(&mut self, ui: &mut egui::Ui, note: &str, model: Option<inference::GenerateArgs>) {
        if self.status.is_some() { self.team_controls(ui, note, model); }
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

impl Drop for HubView {
    fn drop(&mut self) { self.plan_cancel.store(true, Ordering::Relaxed); }
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

        // A refresh that keeps failing is reported once and not retried on every frame.
        let moved = root.path().join("first-hub-moved");
        std::fs::rename(root.path().join("second-hub"), &moved).unwrap();
        view.refresh();
        settle(&mut view);
        assert!(!view.notice.is_empty() && !view.busy());
        std::fs::rename(&moved, root.path().join("second-hub")).unwrap();

        // A folder that cannot be loaded leaves no task actions behind.
        view.switch(root.path().join("missing-hub"));
        settle(&mut view);
        assert!(view.status.is_none() && view.tasks.is_empty());
        assert!(!view.notice.is_empty());
    }
}
