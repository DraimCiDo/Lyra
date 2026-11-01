//! Thin GUI for project/knowledge CLI contracts; no model or external command execution here.
use super::*;
use crate::{
    knowledge,
    project::{self, Blueprint, profile::Profile},
};
use std::path::Path;

pub(super) struct ProjectDraft {
    pub(super) parent: Option<PathBuf>,
    pub(super) name: String,
    pub(super) profile: Profile,
    pub(super) version: String,
    pub(super) task: String,
    pub(super) required_files: String,
    pub(super) plan: Option<Blueprint>,
    pub(super) selected: usize,
    pub(super) error: Option<String>,
}
impl Default for ProjectDraft {
    fn default() -> Self {
        Self {
            parent: None,
            name: "new-project".into(),
            profile: Profile::Generic,
            version: String::new(),
            task: String::new(),
            required_files: String::new(),
            plan: None,
            selected: 0,
            error: None,
        }
    }
}
pub(super) struct KnowledgeUi {
    profile: Profile,
    version: String,
    output: Option<PathBuf>,
    imports: Vec<PathBuf>,
    preview: Option<knowledge::Pack>,
    preview_path: Option<PathBuf>,
    error: Option<String>,
    query: String,
    matches: Vec<knowledge::Snippet>,
}
impl Default for KnowledgeUi {
    fn default() -> Self {
        Self {
            profile: Profile::Generic,
            version: String::new(),
            output: None,
            imports: Vec::new(),
            preview: None,
            preview_path: None,
            error: None,
            query: String::new(),
            matches: Vec::new(),
        }
    }
}
fn profile_picker(ui: &mut egui::Ui, id: &str, profile: &mut Profile, language: Language) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(language.text(profile.label()))
        .show_ui(ui, |ui| {
            for p in Profile::ALL {
                ui.selectable_value(profile, p, language.text(p.label()));
            }
        });
}
impl Desktop {
    pub(super) fn prepare_project_job(
        &self,
        operation: Operation,
        temp: &tempfile::TempDir,
        args: &mut Vec<OsString>,
    ) -> io::Result<Option<Duration>> {
        match operation {
            Operation::ProjectPlan => {
                validate_task(&self.draft.task, "")?;
                if !project::valid_name(&self.draft.name) {
                    return Err(io::Error::other(
                        "Имя проекта: a-z, 0-9, - или _, до 64 символов.",
                    ));
                }
                let model = self
                    .prefs
                    .model
                    .as_ref()
                    .ok_or_else(|| io::Error::other("Выберите GGUF-модель."))?;
                let path = temp.path().join("task.txt");
                std::fs::write(&path, &self.draft.task)?;
                args.extend([
                    "project".into(),
                    "plan".into(),
                    "--name".into(),
                    self.draft.name.clone().into(),
                    "--task-file".into(),
                    path.into_os_string(),
                    "--profile".into(),
                    serde_json::to_value(self.draft.profile)
                        .map_err(io::Error::other)?
                        .as_str()
                        .unwrap_or("generic")
                        .into(),
                    "--target-version".into(),
                    self.draft.version.clone().into(),
                    "--model".into(),
                    model.as_os_str().into(),
                ]);
                // Explicit existing resource settings remain authoritative. No hidden context enlargement.
                for (flag, value) in [
                    ("--context", self.prefs.context as u64),
                    ("--max-tokens", self.prefs.tokens as u64),
                    ("--threads", self.prefs.threads as u64),
                    ("--gpu-layers", self.prefs.gpu_layers as u64),
                    ("--timeout-secs", self.prefs.timeout),
                ] {
                    args.extend([flag.into(), value.to_string().into()]);
                }
                for path in self
                    .draft
                    .required_files
                    .lines()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    args.extend(["--require-file".into(), path.into()]);
                }
                if let Some(docs) = &self.prefs.documentation {
                    args.extend(["--docs".into(), docs.as_os_str().into()]);
                }
                Ok(Some(Duration::from_secs(self.prefs.timeout + 5)))
            }
            Operation::ProjectCreate => {
                let plan = self
                    .draft
                    .plan
                    .as_ref()
                    .ok_or_else(|| io::Error::other("Сначала подготовь и просмотри план."))?;
                plan.ready()?;
                let parent = self
                    .draft
                    .parent
                    .as_ref()
                    .ok_or_else(|| io::Error::other("Выбери родительскую папку."))?;
                let path = temp.path().join("blueprint.json");
                std::fs::write(&path, serde_json::to_vec(plan).map_err(io::Error::other)?)?;
                args.extend([
                    "project".into(),
                    "create".into(),
                    "--plan".into(),
                    path.into_os_string(),
                    "--parent".into(),
                    parent.as_os_str().into(),
                ]);
                Ok(None)
            }
            Operation::DocsFetch | Operation::DocsImport => {
                let path = self
                    .knowledge_ui
                    .output
                    .as_ref()
                    .ok_or_else(|| io::Error::other("Выбери новый путь сохранения pack."))?;
                let profile =
                    serde_json::to_value(self.knowledge_ui.profile).map_err(io::Error::other)?;
                args.extend([
                    "docs".into(),
                    if operation == Operation::DocsFetch {
                        "fetch"
                    } else {
                        "import"
                    }
                    .into(),
                    "--profile".into(),
                    profile.as_str().unwrap_or("generic").into(),
                    "--target-version".into(),
                    self.knowledge_ui.version.clone().into(),
                    "--output".into(),
                    path.as_os_str().into(),
                ]);
                if operation == Operation::DocsFetch {
                    args.push("--allow-network".into());
                } else {
                    for file in &self.knowledge_ui.imports {
                        args.extend(["--file".into(), file.as_os_str().into()]);
                    }
                }
                Ok(Some(Duration::from_secs(95)))
            }
            _ => Err(io::Error::other("Unexpected project operation")),
        }
    }
    pub(super) fn complete_project_job(&mut self, code: Option<i32>, bytes: &[u8]) {
        if code != Some(0) {
            let message = if self.operation == Operation::ProjectPlan {
                "Не удалось подготовить корректный план проекта. Проверь модель и параметры генерации. Подробности — в журнале.".to_owned()
            } else {
                format!("Операция не выполнена (код {code:?}). Подробности — в журнале.")
            };
            if matches!(
                self.operation,
                Operation::ProjectPlan | Operation::ProjectCreate
            ) {
                self.draft.error = Some(message.clone());
            } else {
                self.knowledge_ui.error = Some(message.clone());
            }
            self.fail(message);
            return;
        }
        match self.operation {
            Operation::ProjectPlan => {
                match serde_json::from_slice::<Blueprint>(bytes)
                    .map_err(io::Error::other)
                    .and_then(|p| {
                        p.validate()?;
                        Ok(p)
                    }) {
                    Ok(plan) => {
                        self.draft.plan = Some(plan);
                        self.draft.error = None;
                        self.draft.selected = 0;
                        self.message = "План готов. Проверь файлы и вопросы.".into();
                    }
                    Err(e) => {
                        self.draft.error = Some(e.to_string());
                        self.fail(e);
                    }
                }
            }
            Operation::ProjectCreate => {
                match serde_json::from_slice::<project::ProjectInfo>(bytes) {
                    Ok(info) => {
                        if let Some(parent) = &self.draft.parent {
                            self.prefs.repo = Some(parent.join(info.name));
                            self.prefs.files.clear();
                        }
                        self.task.clear();
                        self.report = None;
                        self.report_json.clear();
                        self.applied.clear();
                        self.draft.plan = None;
                        self.settings_open = false;
                        self.message =
                            "Проект создан. Сборка и внешние тесты ещё не выполнялись.".into();
                        self.error = false;
                    }
                    Err(e) => {
                        self.draft.error = Some(e.to_string());
                        self.fail(e);
                    }
                }
            }
            Operation::DocsFetch | Operation::DocsImport => {
                self.prefs.documentation = self.knowledge_ui.output.clone();
                self.knowledge_ui.preview = None;
                self.knowledge_ui.preview_path = None;
                self.knowledge_ui.error = None;
                self.message = "Документация сохранена локально и выбрана для задач.".into();
                self.error = false;
            }
            _ => {}
        }
    }
    pub(super) fn project_result(&mut self, ui: &mut egui::Ui) {
        let l = self.prefs.language;
        let c = self.prefs.palette.colors();
        let Some(plan) = self.draft.plan.as_mut() else {
            return;
        };
        ui.add_space(6.0);
        let mut create = false;
        egui::CollapsingHeader::new(format!(
            "{} · {}",
            l.text("Файлы нового проекта"),
            plan.files.len()
        ))
        .default_open(false)
        .show(ui, |ui| {
            for assumption in &plan.assumptions {
                ui.label(RichText::new(assumption).small().weak());
            }
            egui::CollapsingHeader::new(l.text("Требования и источники")).show(ui, |ui| {
                for requirement in &plan.requirements {
                    ui.label(requirement);
                }
                for source in &plan.provided_sources {
                    ui.label(
                        RichText::new(format!("{} · {}", source.title, source.version_scope))
                            .small(),
                    )
                    .on_hover_text(&source.origin);
                }
            });
            if !plan.files.is_empty() {
                self.draft.selected = self.draft.selected.min(plan.files.len() - 1);
                egui::ComboBox::from_id_salt("planned-files")
                    .selected_text(&plan.files[self.draft.selected].path)
                    .show_ui(ui, |ui| {
                        for (i, file) in plan.files.iter().enumerate() {
                            ui.selectable_value(&mut self.draft.selected, i, &file.path);
                        }
                    });
                egui::ScrollArea::vertical()
                    .id_salt("planned-code")
                    .max_height(160.0)
                    .show(ui, |ui| {
                        selectable_code(ui, &plan.files[self.draft.selected].content, c.text);
                    });
            }
            ui.add_enabled_ui(self.job.is_none(), |ui| {
                ui.horizontal(|ui| {
                    ui.label(l.text("Имя папки"));
                    ui.add(
                        egui::TextEdit::singleline(&mut plan.name)
                            .char_limit(64)
                            .desired_width(150.0),
                    );
                    create = ui
                        .add_enabled(
                            plan.ready().is_ok()
                                && self.updates.kind() != Some(updater::Kind::Prepare),
                            egui::Button::image_and_text(
                                Icon::FolderPlus.image(c.accent),
                                l.text("Создать проект…"),
                            ),
                        )
                        .clicked();
                });
            });
            ui.label(
                RichText::new(
                    l.text("Сборка и тесты ещё не запускались. Создание — только в новой папке."),
                )
                .small()
                .weak(),
            );
        });
        if create
            && self.real_app
            && let Some(parent) = rfd::FileDialog::new()
                .set_title(l.text("Где сохранить новый проект?"))
                .pick_folder()
        {
            self.draft.parent = Some(parent);
            if let Err(e) = self.start(Operation::ProjectCreate, ui.ctx()) {
                self.fail(e);
            }
        }
    }
    pub(super) fn knowledge_settings(&mut self, ui: &mut egui::Ui) {
        let l = self.prefs.language;
        let c = self.prefs.palette.colors();
        ui.label(l.text("Документация обрабатывается локально. Загрузка официальных страниц — только по кнопке."));
        ui.add_enabled_ui(self.job.is_none(), |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button(l.text("Выбрать pack…")).clicked()
                    && let Some(p) = rfd::FileDialog::new()
                        .add_filter("Document pack", &["json"])
                        .pick_file()
                {
                    self.prefs.documentation = Some(p);
                    self.knowledge_ui.preview_path = None;
                }
                if ui.button(l.text("Отключить документы")).clicked() {
                    self.prefs.documentation = None;
                    self.knowledge_ui.preview = None;
                    self.knowledge_ui.preview_path = None;
                    self.knowledge_ui.matches.clear();
                }
            });
            if let Some(path) = &self.prefs.documentation {
                ui.add(
                    egui::Label::new(RichText::new(path.display().to_string()).small().weak())
                        .truncate(),
                )
                .on_hover_text(path.display().to_string());
            }
            ui.horizontal(|ui| {
                profile_picker(ui, "docs-profile", &mut self.knowledge_ui.profile, l);
                ui.add(
                    egui::TextEdit::singleline(&mut self.knowledge_ui.version)
                        .desired_width(140.0)
                        .char_limit(80)
                        .hint_text(l.text("Версия платформы / SDK")),
                );
            });
            ui.horizontal_wrapped(|ui| {
                if ui.button(l.text("Импортировать файлы…")).clicked()
                    && let Some(files) = rfd::FileDialog::new()
                        .add_filter(
                            "Docs / SDK",
                            &[
                                "md", "txt", "rst", "c", "java", "kt", "rs", "ts", "vue", "css",
                            ],
                        )
                        .pick_files()
                    && let Some(path) = pack_output()
                {
                    self.knowledge_ui.imports = files;
                    self.knowledge_ui.output = Some(path);
                    if self.real_app
                        && let Err(e) = self.start(Operation::DocsImport, ui.ctx())
                    {
                        self.knowledge_ui.error = Some(e.to_string());
                    }
                }
                if ui
                    .button(l.text("Загрузить официальные документы…"))
                    .clicked()
                    && let Some(path) = pack_output()
                {
                    self.knowledge_ui.output = Some(path);
                    if self.real_app
                        && let Err(e) = self.start(Operation::DocsFetch, ui.ctx())
                    {
                        self.knowledge_ui.error = Some(e.to_string());
                    }
                }
            });
        });
        if matches!(self.operation, Operation::DocsFetch | Operation::DocsImport)
            && self.job.is_some()
        {
            ui.horizontal(|ui| {
                ui.label(l.text(&self.message));
                if ui.button(l.text("Остановить")).clicked()
                    && let Some(j) = &self.job
                {
                    j.cancel();
                }
            });
        }
        if self.knowledge_ui.preview_path != self.prefs.documentation {
            self.knowledge_ui.preview_path = self.prefs.documentation.clone();
            self.knowledge_ui.matches.clear();
            match self
                .prefs
                .documentation
                .as_deref()
                .map(knowledge::Pack::load)
                .transpose()
            {
                Ok(p) => {
                    self.knowledge_ui.preview = p;
                    self.knowledge_ui.error = None;
                }
                Err(e) => {
                    self.knowledge_ui.preview = None;
                    self.knowledge_ui.error = Some(e.to_string());
                }
            }
        }
        if let Some(pack) = &self.knowledge_ui.preview {
            ui.separator();
            ui.label(format!(
                "{} · {} · {}",
                pack.profile.label(),
                pack.target_version,
                pack.documents.len()
            ));
            for s in pack.catalog() {
                ui.label(
                    RichText::new(format!("{} · {}", s.title, s.version_scope))
                        .small()
                        .weak(),
                )
                .on_hover_text(&s.origin);
            }
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.knowledge_ui.query)
                        .char_limit(256)
                        .hint_text(l.text("Символ или вопрос по API")),
                );
                if ui.button(l.text("Поиск")).clicked() {
                    match pack.search(&self.knowledge_ui.query, 4, 8192) {
                        Ok(m) => self.knowledge_ui.matches = m,
                        Err(e) => self.knowledge_ui.error = Some(e.to_string()),
                    }
                }
            });
            for snippet in &self.knowledge_ui.matches {
                egui::CollapsingHeader::new(format!("{}:{}", snippet.source.title, snippet.line))
                    .show(ui, |ui| {
                        selectable_code(ui, &snippet.text, c.text);
                    });
            }
        }
        if let Some(e) = &self.knowledge_ui.error {
            ui.label(RichText::new(e).small().color(c.error));
        }
        ui.add_space(4.0);
        egui::CollapsingHeader::new(l.text("Официальные источники профиля")).show(ui, |ui| {
            match knowledge::sources(self.knowledge_ui.profile, &self.knowledge_ui.version) {
                Ok(sources) => {
                    for s in sources {
                        ui.label(
                            RichText::new(format!("{} · {}", s.title, s.version_scope)).small(),
                        )
                        .on_hover_text(s.url);
                    }
                }
                Err(e) => {
                    ui.label(e.to_string());
                }
            }
        });
        ui.label(RichText::new(l.text("Для любого другого языка или SDK импортируй локальные тексты. Rolling-документация не доказывает совместимость с версией проекта.")).small().weak());
    }
}
fn pack_output() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_file_name("documentation.json")
        .add_filter("Document pack", &["json"])
        .save_file()
        .filter(|p| !p.as_os_str().is_empty() && p.parent() != Some(Path::new("")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    #[test]
    fn chat_project_creates_through_the_real_cli_and_selects_the_new_root() {
        let t = tempfile::tempdir().unwrap();
        let mut app = Desktop::default();
        app.draft.parent = Some(t.path().to_path_buf());
        app.draft.task = "Implement a CLI".into();
        app.draft.plan = Some(
            project::starter(&project::StarterArgs {
                name: "from-ui".into(),
                profile: Profile::Rust,
                target_version: String::new(),
                task: app.draft.task.clone(),
            })
            .unwrap(),
        );
        let temp = tempfile::tempdir().unwrap();
        let mut args = Vec::new();
        assert!(
            app.prepare_project_job(Operation::ProjectCreate, &temp, &mut args)
                .unwrap()
                .is_none()
        );
        let cli = crate::Cli::try_parse_from(std::iter::once(OsString::from("lyra")).chain(args))
            .unwrap();
        let mut out = Vec::new();
        crate::run(cli, &mut out).unwrap();
        app.operation = Operation::ProjectCreate;
        app.complete_project_job(Some(0), &out);
        assert_eq!(
            app.prefs.repo.as_deref(),
            Some(t.path().join("from-ui").as_path())
        );
        assert!(app.task.is_empty());
        assert!(t.path().join("from-ui/src/main.rs").is_file());
        assert!(!app.error);
    }
    #[test]
    fn document_import_contract_preserves_provenance_and_selects_pack() {
        let t = tempfile::tempdir().unwrap();
        let source = t.path().join("SDK.md");
        std::fs::write(
            &source,
            "# SDK\nThe documented API takes a string argument.\n",
        )
        .unwrap();
        let mut app = Desktop::default();
        app.knowledge_ui.imports = vec![source];
        app.knowledge_ui.output = Some(t.path().join("pack.json"));
        app.knowledge_ui.profile = Profile::Generic;
        let temp = tempfile::tempdir().unwrap();
        let mut args = Vec::new();
        app.prepare_project_job(Operation::DocsImport, &temp, &mut args)
            .unwrap();
        let cli = crate::Cli::try_parse_from(std::iter::once(OsString::from("lyra")).chain(args))
            .unwrap();
        let mut out = Vec::new();
        crate::run(cli, &mut out).unwrap();
        app.operation = Operation::DocsImport;
        app.complete_project_job(Some(0), &out);
        let pack = knowledge::Pack::load(app.prefs.documentation.as_deref().unwrap()).unwrap();
        assert_eq!(pack.documents[0].source.origin, "local:SDK.md");
        assert!(!app.error);
    }
}
