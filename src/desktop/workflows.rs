//! UI adapters for the shared CLI publication/recovery controller.
use super::*;
use serde::Deserialize;
#[derive(Clone, Deserialize)]
pub(super) struct RecoveryFile {
    pub path: String,
    pub state_before: String,
}
#[derive(Clone, Deserialize)]
pub(super) struct RecoveryView {
    pub status: String,
    #[serde(default)]
    pub files: Vec<RecoveryFile>,
}
pub(super) fn can_apply_all(report: &Report, applied: &[usize]) -> bool {
    !report.proposals.is_empty()
        && report.proposals.len() <= crate::workspace::MAX_PROPOSALS
        && (0..report.proposals.len()).all(|i| can_apply(report, i, applied))
}
impl Desktop {
    pub(super) fn prepare_publication_job(
        &mut self,
        operation: Operation,
        temp: &tempfile::TempDir,
        args: &mut Vec<OsString>,
    ) -> io::Result<Option<Duration>> {
        match operation {
            Operation::Apply(_) | Operation::ApplyAll => {
                let report = self
                    .report
                    .as_ref()
                    .ok_or_else(|| io::Error::other("Нет отчёта."))?;
                let valid = match operation {
                    Operation::Apply(i) => can_apply(report, i, &self.applied),
                    _ => can_apply_all(report, &self.applied),
                };
                if !valid {
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
                ]);
                match operation {
                    Operation::Apply(i) => args.extend(["--index".into(), i.to_string().into()]),
                    _ => args.push("--all".into()),
                }
                Ok(None)
            }
            Operation::RecoveryStatus
            | Operation::RecoveryRollback
            | Operation::RecoveryComplete => {
                if operation == Operation::RecoveryStatus {
                    self.recovery_root = self
                        .prefs
                        .repo
                        .clone()
                        .or_else(|| self.report.as_ref().map(|r| r.workspace.clone()));
                    self.recovery_view = None;
                }
                let root = self
                    .recovery_root
                    .as_ref()
                    .ok_or_else(|| io::Error::other("Выберите проект."))?;
                args.extend(["recover".into(), "--repo".into(), root.as_os_str().into()]);
                if operation != Operation::RecoveryStatus {
                    args.extend([
                        "--action".into(),
                        if operation == Operation::RecoveryRollback {
                            "rollback"
                        } else {
                            "complete"
                        }
                        .into(),
                    ]);
                }
                self.recovery_open = true;
                Ok((operation == Operation::RecoveryStatus).then_some(Duration::from_secs(15)))
            }
            _ => Err(io::Error::other("Unexpected publication operation")),
        }
    }
    pub(super) fn complete_publication_job(&mut self, result: &ResultData) {
        if result.code != Some(0) {
            self.fail("Операция не завершена. Проверьте восстановление и журнал событий.");
            return;
        }
        match self.operation {
            Operation::Apply(i) => self.applied.push(i),
            Operation::ApplyAll => {
                if let Some(report) = &self.report {
                    self.applied = (0..report.proposals.len()).collect();
                }
            }
            _ => {
                let view: RecoveryView = match serde_json::from_slice(&result.stdout) {
                    Ok(view) => view,
                    Err(e) => {
                        self.fail(e);
                        return;
                    }
                };
                if self.operation != Operation::RecoveryStatus
                    && self
                        .report
                        .as_ref()
                        .is_some_and(|r| Some(&r.workspace) == self.recovery_root.as_ref())
                {
                    // A journal may belong to another report. Never mark unrelated proposals applied.
                    self.report = None;
                    self.report_json.clear();
                    self.applied.clear();
                    self.confirm = None;
                }
                self.recovery_view = Some(view);
            }
        }
        self.error = false;
        self.message = match self.operation {
            Operation::Apply(_) => "Правка применена. Запустите тесты проекта отдельно.",
            Operation::ApplyAll => "Набор правок применен. Запустите тесты проекта отдельно.",
            Operation::RecoveryStatus => "Состояние восстановления прочитано.",
            _ => "Восстановление завершено. Проверьте файлы проекта.",
        }
        .into();
    }
    pub(super) fn recovery_window(&mut self, ctx: &egui::Context) {
        if !self.recovery_open {
            return;
        }
        let l = self.prefs.language;
        let mut open = true;
        egui::Window::new(l.text("Восстановление проекта"))
            .id(egui::Id::unique("publication-recovery"))
            .open(&mut open)
            .default_width(560.0)
            .show(ctx, |ui| {
                if let Some(root) = &self.recovery_root {
                    ui.label(root.display().to_string());
                }
                ui.label(l.text(
                    "Исходники в окне не показываются. Внешние изменения блокируют восстановление.",
                ));
                if let Some(view) = &self.recovery_view {
                    ui.label(l.text(match view.status.as_str() {
                        "pending" => "Есть незавершенная публикация.",
                        "no_pending_publication" => "Незавершенных публикаций нет.",
                        "rolled_back" => "Изменения из журнала отменены.",
                        "completed" => "Публикация из журнала завершена.",
                        "staging_only_no_project_writes" => {
                            "Запись исходников не началась; можно очистить подготовку."
                        }
                        _ => "Подготовка очищена; исходники не менялись.",
                    }));
                    for file in &view.files {
                        ui.label(format!(
                            "{} · {}",
                            file.path,
                            l.text(if file.state_before == "applied" {
                                "Применено"
                            } else {
                                "Исходное состояние"
                            })
                        ));
                    }
                    let pending = view.status == "pending";
                    let staging = view.status == "staging_only_no_project_writes";
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                self.job.is_none() && (pending || staging),
                                egui::Button::new(l.text("Откатить публикацию…")),
                            )
                            .clicked()
                        {
                            self.confirm = Some(Operation::RecoveryRollback);
                        }
                        if ui
                            .add_enabled(
                                self.job.is_none() && pending,
                                egui::Button::new(l.text("Завершить публикацию…")),
                            )
                            .clicked()
                        {
                            self.confirm = Some(Operation::RecoveryComplete);
                        }
                    });
                } else if self.job.is_none() {
                    ui.label(l.text("Состояние не прочитано. Подробности — в журнале событий."));
                }
                if ui
                    .add_enabled(
                        self.job.is_none(),
                        egui::Button::new(l.text("Обновить состояние")),
                    )
                    .clicked()
                    && let Err(e) = self.start(Operation::RecoveryStatus, ctx)
                {
                    self.fail(e);
                }
            });
        self.recovery_open = open;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovery_target_is_pinned_and_independent_of_later_project_selection() {
        let mut app = Desktop::default();
        let old = PathBuf::from("original project");
        app.prefs.repo = Some(old.clone());
        let temp = tempfile::tempdir().unwrap();
        let mut args = Vec::new();
        app.prepare_publication_job(Operation::RecoveryStatus, &temp, &mut args)
            .unwrap();
        app.prefs.repo = Some(PathBuf::from("other project"));
        args.clear();
        assert!(
            app.prepare_publication_job(Operation::RecoveryRollback, &temp, &mut args)
                .unwrap()
                .is_none()
        );
        assert_eq!(args[2], old.as_os_str());
        assert_eq!(args.last().unwrap(), "rollback");
    }
    #[test]
    fn apply_all_requires_every_proposal_to_be_ready_and_unapplied() {
        let report:Report=serde_json::from_value(serde_json::json!({"schema_version":4,"workspace":"project","status":"completed","outcome":"changes_proposed", "proposals":[
            {"path":"a.rs","original":"a","replacement":"b"},{"path":"b.rs","original":"c","replacement":"d"}],
            "summaries":[],"events":[],"calls":1,"subagents":false,"truncated_index":false,"model_review_is_verified_test":false})).unwrap();
        assert!(can_apply_all(&report, &[]));
        assert!(!can_apply_all(&report, &[0]));
        let mut incomplete = report;
        incomplete.status = "cancelled".into();
        assert!(!can_apply_all(&incomplete, &[]));
    }
    #[test]
    fn recovery_window_renders_without_source_or_model_and_does_not_apply() {
        let ctx = egui::Context::default();
        configure(&ctx);
        let mut app = Desktop::default();
        app.recovery_open = true;
        app.recovery_root = Some(PathBuf::from("проект с пробелами"));
        app.recovery_view = Some(RecoveryView {
            status: "pending".into(),
            files: vec![RecoveryFile {
                path: "src/lib.rs".into(),
                state_before: "applied".into(),
            }],
        });
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.recovery_window(ui.ctx())
        });
        output.textures_delta.clear();
        assert!(!output.shapes.is_empty());
        assert!(app.job.is_none());
        assert!(app.applied.is_empty());
    }
}

impl Desktop {
    fn configured_session_db(&self) -> io::Result<PathBuf> {
        self.prefs
            .task_session_db
            .clone()
            .or_else(|| preferences::config_path().map(|p| p.with_file_name("coding-sessions.db")))
            .ok_or_else(|| io::Error::other("Выберите файл базы задач."))
    }
    pub(super) fn append_task_workflow_args(&self, args: &mut Vec<OsString>) -> io::Result<()> {
        if let Some(config) = &self.prefs.check_config {
            crate::executor::load_config(config)?;
            args.extend(["--check-config".into(), config.as_os_str().into()]);
        }
        if self.prefs.save_task_sessions || self.resume_task.is_some() {
            let db = if self.resume_task.is_some() {
                self.saved_tasks_db
                    .clone()
                    .ok_or_else(|| io::Error::other("Нет выбранной базы задач."))?
            } else {
                self.configured_session_db()?
            };
            args.extend(["--session-db".into(), db.into_os_string()]);
        }
        if let Some(id) = &self.resume_task {
            args.extend(["--resume".into(), id.clone().into()]);
        }
        Ok(())
    }
    pub(super) fn workflow_settings(&mut self, ui: &mut egui::Ui) {
        let l = self.prefs.language;
        ui.separator();
        ui.label(l.text("Проверка проекта в Docker"));
        if let Some(path) = &self.prefs.check_config {
            ui.label(path.display().to_string());
        }
        ui.horizontal(|ui| {
            if ui.button(l.text("Выбрать проверку JSON…")).clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("JSON", &["json"])
                    .pick_file()
            {
                match crate::executor::load_config(&path) {
                    Ok(_) => self.prefs.check_config = Some(path),
                    Err(e) => self.fail(e),
                }
            }
            if self.prefs.check_config.is_some()
                && ui.button(l.text("Отключить проверку")).clicked()
            {
                self.prefs.check_config = None;
            }
        });
        ui.label(l.text(
            "Модель сможет запускать только выбранную проверку в изолированной копии без сети.",
        ));
        ui.separator();
        ui.checkbox(
            &mut self.prefs.save_task_sessions,
            l.text("Сохранять задачи для продолжения"),
        );
        ui.label(l.text("База содержит выбранные исходники и предложения. Хранится локально, отдельно от истории чата."));
        if let Ok(path) = self.configured_session_db() {
            ui.label(path.display().to_string());
        }
        ui.horizontal(|ui| {
            if ui.button(l.text("Выбрать базу задач…")).clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .set_file_name("coding-sessions.db")
                    .save_file()
            {
                self.prefs.task_session_db = Some(path);
            }
            if ui.button(l.text("Сохраненные задачи…")).clicked() {
                self.saved_tasks_offset = 0;
                if let Err(e) = self.start(Operation::SessionList, ui.ctx()) {
                    self.fail(e);
                }
            }
        });
    }
    pub(super) fn prepare_saved_tasks_job(
        &mut self,
        args: &mut Vec<OsString>,
    ) -> io::Result<Option<Duration>> {
        let db = self.configured_session_db()?;
        self.saved_tasks_db = Some(db.clone());
        self.saved_tasks_open = true;
        args.extend([
            "session".into(),
            "list".into(),
            "--db".into(),
            db.into_os_string(),
            "--offset".into(),
            self.saved_tasks_offset.to_string().into(),
        ]);
        Ok(Some(Duration::from_secs(15)))
    }
    pub(super) fn complete_saved_tasks_job(&mut self, result: &ResultData) {
        if result.code != Some(0) {
            self.saved_tasks = None;
            self.fail("Не удалось прочитать базу задач. Проверьте путь и журнал событий.");
            return;
        }
        match serde_json::from_slice::<crate::session::SessionPage>(&result.stdout) {
            Ok(page) => {
                self.saved_tasks = Some(page);
                self.error = false;
                self.message = "Сохраненные задачи прочитаны.".into();
            }
            Err(e) => self.fail(e),
        }
    }
    pub(super) fn saved_tasks_window(&mut self, ctx: &egui::Context) {
        if !self.saved_tasks_open {
            return;
        }
        let l = self.prefs.language;
        let mut open = true;
        let mut chosen = None;
        let mut refresh = false;
        egui::Window::new(l.text("Сохраненные задачи")).id(egui::Id::unique("saved-coding-tasks"))
            .open(&mut open).default_width(650.0).show(ctx,|ui|{
                if let Some(db)=&self.saved_tasks_db {ui.label(db.display().to_string());}
                ui.label(l.text("Продолжение использует текущую модель и выбранную проверку. Исходный проект меняется только через применение предложений."));
                ui.add(egui::Slider::new(&mut self.prefs.calls,1..=256).text(l.text("Общий лимит вызовов")));
                if let Some(page)=&self.saved_tasks {
                    egui::ScrollArea::vertical().max_height(420.0).show(ui,|ui|{
                        for saved in &page.sessions {
                            ui.group(|ui|{
                                ui.label(saved.task.lines().next().unwrap_or("").chars().take(100).collect::<String>());
                                ui.label(saved.workspace.display().to_string());
                                ui.label(format!("{} · {} · {}",saved.session_id,saved.status,saved.calls));
                                if ui.add_enabled(self.job.is_none()&&saved.calls<self.prefs.calls,
                                    egui::Button::new(l.text("Продолжить задачу"))).clicked() {
                                    chosen=Some((saved.session_id.clone(),saved.workspace.clone(),saved.task.clone()));
                                }
                                if saved.calls>=self.prefs.calls {ui.label(l.text("Для продолжения увеличьте общий лимит выше числа сохраненных вызовов."));}
                            });
                        }
                    });
                    ui.horizontal(|ui|{
                        if ui.add_enabled(self.job.is_none()&&page.offset>0,egui::Button::new(l.text("Предыдущая страница"))).clicked(){self.saved_tasks_offset=page.offset.saturating_sub(20);refresh=true;}
                        if ui.add_enabled(self.job.is_none()&&page.has_more,egui::Button::new(l.text("Следующая страница"))).clicked(){self.saved_tasks_offset=page.offset+20;refresh=true;}
                    });
                }
                if ui.add_enabled(self.job.is_none(),egui::Button::new(l.text("Обновить состояние"))).clicked(){refresh=true;}
            });
        self.saved_tasks_open = open;
        if refresh && let Err(e) = self.start(Operation::SessionList, ctx) {
            self.fail(e);
        }
        if let Some((id, root, task)) = chosen {
            self.prefs.repo = Some(root);
            self.request_task = task.clone();
            self.resume_task = Some(id);
            match self.start(Operation::Task, ctx) {
                Ok(()) => {
                    self.chat_mode = ChatMode::Task;
                    self.chat.sent(task);
                    self.saved_tasks_open = false;
                    self.settings_open = false;
                }
                Err(e) => {
                    self.resume_task = None;
                    self.fail(e);
                }
            }
        }
    }
}

#[cfg(test)]
mod saved_task_tests {
    use super::*;
    #[test]
    fn persistence_is_opt_in_and_resume_keeps_the_selected_database() {
        let mut app = Desktop::default();
        let mut args = Vec::new();
        app.append_task_workflow_args(&mut args).unwrap();
        assert!(args.is_empty());
        app.prefs.save_task_sessions = true;
        app.prefs.task_session_db = Some(PathBuf::from("first.db"));
        app.append_task_workflow_args(&mut args).unwrap();
        assert_eq!(args[1], "first.db");
        args.clear();
        app.resume_task = Some("selected-id".into());
        app.saved_tasks_db = Some(PathBuf::from("pinned.db"));
        app.prefs.task_session_db = Some(PathBuf::from("different.db"));
        app.append_task_workflow_args(&mut args).unwrap();
        assert_eq!(args[1], "pinned.db");
        assert_eq!(args.last().unwrap(), "selected-id");
    }
    #[test]
    fn saved_task_pages_render_without_starting_a_model_or_changing_the_call_budget() {
        let ctx = egui::Context::default();
        configure(&ctx);
        let mut app = Desktop::default();
        let calls = app.prefs.calls;
        app.saved_tasks_open = true;
        app.saved_tasks = Some(crate::session::SessionPage {
            offset: 0,
            has_more: false,
            sessions: vec![crate::session::SessionListing {
                session_id: "sample".into(),
                workspace: PathBuf::from("проект"),
                task: "Task".into(),
                status: "step_limit: coder".into(),
                calls,
            }],
        });
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.saved_tasks_window(ui.ctx())
        });
        output.textures_delta.clear();
        assert!(!output.shapes.is_empty());
        assert!(app.job.is_none());
        assert_eq!(app.prefs.calls, calls);
    }
}
