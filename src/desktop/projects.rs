//! Thin GUI for project/knowledge CLI contracts; no model or external command execution here.
use super::*;
use crate::{
    knowledge,
    project::{self, Blueprint, profile::Profile},
};
use std::path::Path;

pub(super) struct Wizard {
    pub open: bool,
    parent: Option<PathBuf>,
    name: String,
    profile: Profile,
    version: String,
    task: String,
    required_files: String,
    ai: bool,
    plan: Option<Blueprint>,
    selected: usize,
    error: Option<String>,
}
impl Default for Wizard {
    fn default() -> Self {
        Self {
            open: false,
            parent: None,
            name: "new-project".into(),
            profile: Profile::Generic,
            version: String::new(),
            task: String::new(),
            required_files: String::new(),
            ai: true,
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
                validate_task(&self.wizard.task, "")?;
                if !project::valid_name(&self.wizard.name) {
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
                std::fs::write(&path, &self.wizard.task)?;
                args.extend([
                    "project".into(),
                    "plan".into(),
                    "--name".into(),
                    self.wizard.name.clone().into(),
                    "--task-file".into(),
                    path.into_os_string(),
                    "--profile".into(),
                    serde_json::to_value(self.wizard.profile)
                        .map_err(io::Error::other)?
                        .as_str()
                        .unwrap_or("generic")
                        .into(),
                    "--target-version".into(),
                    self.wizard.version.clone().into(),
                    "--model".into(),
                    model.as_os_str().into(),
                ]);
                // Explicit existing resource settings remain authoritative. No hidden context enlargement.
                for (flag, value) in [
                    ("--context", self.prefs.context as u64),
                    ("--max-tokens", self.prefs.tokens as u64),
                    ("--threads", self.prefs.threads as u64),
                    ("--timeout-secs", self.prefs.timeout),
                ] {
                    args.extend([flag.into(), value.to_string().into()]);
                }
                for path in self
                    .wizard
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
                    .wizard
                    .plan
                    .as_ref()
                    .ok_or_else(|| io::Error::other("Сначала подготовь и просмотри план."))?;
                plan.ready()?;
                let parent = self
                    .wizard
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
            let message = if self.log.trim().is_empty() {
                format!("Операция не выполнена (код {code:?}).")
            } else {
                self.log
                    .chars()
                    .rev()
                    .take(2048)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect()
            };
            if matches!(
                self.operation,
                Operation::ProjectPlan | Operation::ProjectCreate
            ) {
                self.wizard.error = Some(message.clone());
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
                        self.wizard.plan = Some(plan);
                        self.wizard.error = None;
                        self.wizard.selected = 0;
                        self.message = "План готов. Проверь файлы и вопросы.".into();
                    }
                    Err(e) => {
                        self.wizard.error = Some(e.to_string());
                        self.fail(e);
                    }
                }
            }
            Operation::ProjectCreate => {
                match serde_json::from_slice::<project::ProjectInfo>(bytes) {
                    Ok(info) => {
                        if let Some(parent) = &self.wizard.parent {
                            self.prefs.repo = Some(parent.join(info.name));
                            self.prefs.files.clear();
                        }
                        self.task = self.wizard.task.clone();
                        self.report = None;
                        self.report_json.clear();
                        self.applied.clear();
                        self.wizard.open = false;
                        self.wizard.plan = None;
                        self.settings_open = false;
                        self.message =
                            "Проект создан. Сборка и внешние тесты ещё не выполнялись.".into();
                        self.error = false;
                    }
                    Err(e) => {
                        self.wizard.error = Some(e.to_string());
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
    pub(super) fn project_dialog(&mut self, ctx: &egui::Context) {
        if !self.wizard.open {
            return;
        }
        let l = self.prefs.language;
        let c = self.prefs.palette.colors();
        egui::Modal::new(egui::Id::unique("new-project")).show(ctx,|ui|{
            ui.set_width((ctx.content_rect().width()-60.0).clamp(400.0,660.0));
            ui.horizontal(|ui|{ui.heading(l.text("Новый проект"));ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{if icons::button(ui,Icon::Close,l.text("Закрыть"),false,c.muted).clicked(){self.wizard.open=false;}});});
            egui::ScrollArea::vertical().id_salt("project-wizard").max_height((ctx.content_rect().height()-110.0).max(230.0)).show(ui,|ui|{
                let key=(self.wizard.name.clone(),self.wizard.profile,self.wizard.version.clone(),self.wizard.task.clone(),self.wizard.ai,self.wizard.required_files.clone());
                ui.add_enabled_ui(self.job.is_none(),|ui|{
                    ui.horizontal(|ui|{ui.label(l.text("Имя"));ui.add(egui::TextEdit::singleline(&mut self.wizard.name).char_limit(64).desired_width(160.0));profile_picker(ui,"new-profile",&mut self.wizard.profile,l);});
                    ui.horizontal(|ui|{ui.label(l.text("Версия платформы / SDK"));ui.add(egui::TextEdit::singleline(&mut self.wizard.version).char_limit(80).desired_width(160.0).hint_text("1.21.11"));});
                    ui.label(RichText::new(l.text("Опиши результат, ограничения и критерии приёмки. Неизвестное можно оставить вопросом.")).small().weak());
                    ui.add(egui::TextEdit::multiline(&mut self.wizard.task).desired_rows(3).desired_width(f32::INFINITY).char_limit(16384).hint_text(l.text("Что создать?")));
                    ui.label(RichText::new(l.text("Обязательные файлы, по одному на строке (необязательно)")).small().weak());
                    ui.add(egui::TextEdit::multiline(&mut self.wizard.required_files).desired_rows(1).desired_width(f32::INFINITY).char_limit(8192).hint_text("main.py"));
                    ui.horizontal(|ui|{ui.radio_value(&mut self.wizard.ai,true,l.text("AI-проект"));ui.radio_value(&mut self.wizard.ai,false,l.text("Стартовый каркас"));});
                    if self.wizard.ai {
                        ui.label(RichText::new(format!("{}: {} / {}",l.text("Контекст / токены ответа"),self.prefs.context,self.prefs.tokens)).small().weak());
                        ui.label(RichText::new(l.text("Для нескольких файлов обычно нужны 8192 контекста и 2048–4096 токенов ответа. Измени их в настройках модели.")).small().weak());
                    } else {ui.label(RichText::new(l.text("Каркас не реализует задачу. После создания продолжи её на главной.")).small().weak());}
                    if key!=(self.wizard.name.clone(),self.wizard.profile,self.wizard.version.clone(),self.wizard.task.clone(),self.wizard.ai,self.wizard.required_files.clone()){self.wizard.plan=None;}
                    if icons::action(ui,Icon::Sparkles,l.text("Подготовить план"),c.accent).clicked(){
                        self.wizard.plan=None;self.wizard.error=None;
                        if self.wizard.ai {if self.real_app&&let Err(e)=self.start(Operation::ProjectPlan,ui.ctx()){self.wizard.error=Some(e.to_string());}}
                        else {match project::starter(&project::StarterArgs{name:self.wizard.name.clone(),profile:self.wizard.profile,target_version:self.wizard.version.clone(),task:self.wizard.task.clone()}){Ok(mut p)=>{p.required_files=self.wizard.required_files.lines().map(str::trim).filter(|s|!s.is_empty()).map(str::to_owned).collect();match p.validate(){Ok(())=>{self.wizard.plan=Some(p);self.wizard.selected=0;},Err(e)=>self.wizard.error=Some(e.to_string())}},Err(e)=>self.wizard.error=Some(e.to_string())}}
                    }
                });

                if self.job.is_some(){ui.horizontal(|ui|{ui.label(l.text(&self.message));if self.job.as_ref().is_some_and(|j|j.cancellable)&&ui.button(l.text("Остановить")).clicked()&&let Some(j)=&self.job{j.cancel();}});}
                if let Some(plan)=&self.wizard.plan {
                    ui.separator();ui.label(&plan.summary);
                    for question in &plan.questions{ui.label(RichText::new(format!("? {question}")).color(c.accent));}
                    for assumption in &plan.assumptions{ui.label(RichText::new(assumption).small().weak());}
                    egui::CollapsingHeader::new(l.text("Требования и источники")).show(ui,|ui|{for r in &plan.requirements{ui.label(r);}for s in &plan.provided_sources {ui.label(RichText::new(format!("{} · {} · {}",s.title,s.version_scope,s.origin)).small().weak());}});
                    if !plan.files.is_empty(){
                        self.wizard.selected=self.wizard.selected.min(plan.files.len()-1);
                        egui::ComboBox::from_id_salt("planned-files").selected_text(&plan.files[self.wizard.selected].path).show_ui(ui,|ui|{for(i,f)in plan.files.iter().enumerate(){ui.selectable_value(&mut self.wizard.selected,i,&f.path);}});
                        egui::ScrollArea::vertical().id_salt("planned-code").max_height(120.0).show(ui,|ui|{selectable_code(ui,&plan.files[self.wizard.selected].content,c.text);});
                    }
                    ui.label(RichText::new(l.text("Это предложения. Компиляция и внешние тесты не выполнялись.")).small().color(c.accent));
                    ui.horizontal(|ui|{
                        if ui.add_enabled(self.job.is_none(),egui::Button::new(l.text("Куда создать…"))).clicked()&&let Some(p)=rfd::FileDialog::new().pick_folder(){self.wizard.parent=Some(p);}
                        if let Some(parent)=&self.wizard.parent{ui.add(egui::Label::new(parent.join(&plan.name).display().to_string()).truncate());}
                    });
                    let ready=plan.ready().is_ok()&&self.wizard.parent.is_some()&&self.job.is_none()&&self.updates.kind()!=Some(updater::Kind::Prepare);
                    if ui.add_enabled(ready,egui::Button::image_and_text(Icon::FolderPlus.image(c.accent),l.text("Создать и открыть проект"))).clicked()&&self.real_app&&let Err(e)=self.start(Operation::ProjectCreate,ui.ctx()){self.wizard.error=Some(e.to_string());}
                    ui.label(RichText::new(l.text("Создаётся только новая папка. Существующий проект не перезаписывается.")).small().weak());
                }
                if let Some(e)=&self.wizard.error{ui.label(RichText::new(e).small().color(c.error));}
            });
        });
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
                        .add_filter("Docs / SDK", &["md", "txt", "rst", "c", "java", "kt", "rs"])
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
    fn project_wizard_creates_through_the_real_cli_and_selects_the_new_root() {
        let t = tempfile::tempdir().unwrap();
        let mut app = Desktop::default();
        app.wizard.parent = Some(t.path().to_path_buf());
        app.wizard.task = "Implement a CLI".into();
        app.wizard.plan = Some(
            project::starter(&project::StarterArgs {
                name: "from-ui".into(),
                profile: Profile::Rust,
                target_version: String::new(),
                task: app.wizard.task.clone(),
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
        assert_eq!(app.task, "Implement a CLI");
        assert!(!app.wizard.open);
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
