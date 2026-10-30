//! Native shell around the same CLI contracts. No inference or filesystem policy in widgets.
mod job;
use crate::{agent::Report, inference};
use eframe::egui::{self, Color32, RichText};
use job::{Job, ResultData};
use std::{
    ffi::OsString,
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
    time::Duration,
};

const ACCENT: Color32 = Color32::from_rgb(170, 150, 255);

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
    eframe::run_native(
        "Lyra",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1180.0, 820.0])
                .with_min_inner_size([900.0, 650.0]),
            #[cfg(not(windows))]
            renderer: eframe::Renderer::Glow,
            #[cfg(windows)]
            renderer: eframe::Renderer::Wgpu,
            ..Default::default()
        },
        Box::new(move |cc| {
            configure(&cc.egui_ctx);
            Ok(Box::new(Desktop {
                smoke: smoke.then_some(counter),
                ..Default::default()
            }))
        }),
    )?;
    if smoke && frames.load(std::sync::atomic::Ordering::Relaxed) < 3 {
        return Err(eframe::Error::AppCreation(Box::new(io::Error::other(
            "Окно не прошло три кадра отрисовки.",
        ))));
    }
    Ok(())
}

fn configure(ctx: &egui::Context) {
    let mut style = egui::Style {
        visuals: egui::Visuals::dark(),
        ..Default::default()
    };
    style.visuals.panel_fill = Color32::from_rgb(19, 23, 35);
    style.visuals.window_fill = Color32::from_rgb(25, 30, 44);
    style.visuals.selection.bg_fill = Color32::from_rgb(76, 62, 125);
    style.visuals.hyperlink_color = ACCENT;
    style.spacing.item_spacing = egui::vec2(10.0, 10.0);
    style.spacing.button_padding = egui::vec2(12.0, 8.0);
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_style_of(egui::Theme::Dark, style);
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Changes,
    Activity,
    Hardware,
}
#[derive(Clone, Copy, PartialEq)]
enum Operation {
    Task,
    Apply(usize),
    Doctor,
}

struct Desktop {
    smoke: Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>,
    repo: Option<PathBuf>,
    model: Option<PathBuf>,
    task: String,
    files: String,
    subagents: bool,
    context: u32,
    tokens: u32,
    calls: u32,
    threads: u32,
    timeout: u64,
    job: Option<Job>,
    operation: Operation,
    report: Option<Report>,
    report_json: Vec<u8>,
    applied: Vec<usize>,
    selected: usize,
    tab: Tab,
    message: String,
    log: String,
    hardware: String,
    confirm: Option<usize>,
    error: bool,
}

impl Default for Desktop {
    fn default() -> Self {
        Self {
            smoke: None,
            repo: None,
            model: None,
            task: String::new(),
            files: String::new(),
            subagents: false,
            context: 4096,
            tokens: 512,
            calls: 12,
            threads: inference::default_threads(),
            timeout: 600,
            job: None,
            operation: Operation::Task,
            report: None,
            report_json: Vec::new(),
            applied: Vec::new(),
            selected: 0,
            tab: Tab::Changes,
            message: "Готова к локальной работе".into(),
            log: String::new(),
            hardware: String::new(),
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
        if self.job.is_some() {
            return Err(io::Error::other("Дождитесь текущей операции."));
        }
        let temp = tempfile::Builder::new().prefix("lyra-desktop-").tempdir()?;
        let mut args: Vec<OsString> = Vec::new();
        let deadline = match operation {
            Operation::Task => {
                let repo = self
                    .repo
                    .as_ref()
                    .ok_or_else(|| io::Error::other("Выберите папку проекта."))?;
                let model = self
                    .model
                    .as_ref()
                    .ok_or_else(|| io::Error::other("Выберите GGUF-модель."))?;
                validate_task(&self.task, &self.files)?;
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
                    ("--context", self.context as u64),
                    ("--max-tokens", self.tokens as u64),
                    ("--max-calls", self.calls as u64),
                    ("--threads", self.threads as u64),
                    ("--timeout-secs", self.timeout),
                ] {
                    args.extend([flag.into(), value.to_string().into()]);
                }
                for file in self.files.lines().map(str::trim).filter(|s| !s.is_empty()) {
                    args.extend(["--file".into(), file.into()]);
                }
                if self.subagents {
                    args.push("--subagents".into());
                }
                self.report = None;
                self.report_json.clear();
                self.applied.clear();
                self.selected = 0;
                self.tab = Tab::Changes;
                Some(Duration::from_secs(self.timeout + 5))
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
            Operation::Doctor => {
                args.extend(["doctor".into(), "--json".into()]);
                Some(Duration::from_secs(30))
            }
        };
        let ctx = ctx.clone();
        self.job = Some(Job::start(
            args,
            temp,
            !matches!(operation, Operation::Apply(_)),
            deadline,
            move || ctx.request_repaint(),
        )?);
        self.operation = operation;
        self.error = false;
        self.message = match operation {
            Operation::Task => "Модель работает…",
            Operation::Apply(_) => "Применение выбранной правки…",
            Operation::Doctor => "Сбор сведений о ПК…",
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
            self.fail("Задача остановлена. Правки не применялись.");
            return;
        }
        let seconds = result.elapsed.as_secs_f32();
        match self.operation {
            Operation::Task => match serde_json::from_slice::<Report>(&result.stdout) {
                Ok(report) if report.schema_version == 1 => {
                    self.error = result.code != Some(0) || report.status != "completed";
                    self.message = format!(
                        "{} · {} вызовов · {:.1} с · {} предложений",
                        if self.error {
                            "Задача не завершена"
                        } else if report.proposals.is_empty() {
                            "Протокол завершён; предложений нет"
                        } else {
                            "Предложения готовы к просмотру"
                        },
                        report.calls,
                        seconds,
                        report.proposals.len()
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
                    self.hardware = String::from_utf8_lossy(&result.stdout).into_owned();
                    self.message = "Сведения собраны локально".into();
                } else {
                    self.fail("Не удалось получить диагностику. Откройте журнал.");
                }
            }
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(10.0);
        ui.label(RichText::new("РАБОЧЕЕ ПРОСТРАНСТВО").small().color(ACCENT));
        ui.add_enabled_ui(self.job.is_none(), |ui| {
            ui.label("Проект");
            if ui.button("Выбрать папку…").clicked() && let Some(path) = rfd::FileDialog::new().set_title("Проект Lyra").pick_folder() { self.repo = Some(path); }
            path_label(ui, self.repo.as_ref(), "Папка не выбрана");
            ui.separator();
            ui.label("Локальная модель");
            if ui.button("Открыть GGUF…").clicked() && let Some(path) = rfd::FileDialog::new().add_filter("GGUF", &["gguf"]).pick_file() { self.model = Some(path); }
            path_label(ui, self.model.as_ref(), "Модель не выбрана");
            ui.label(RichText::new("CPU · без облака и подписки").small().color(Color32::from_rgb(130, 200, 173)));
            ui.separator();
            ui.checkbox(&mut self.subagents, "Исследователь и ревьюер").on_hover_text("Последовательно, с одной моделью. Дополнительные роли увеличивают число вычислений.");
            if self.subagents { self.calls = self.calls.max(3); }
            egui::CollapsingHeader::new("Параметры модели").show(ui, |ui| {
                egui::Grid::new("parameters").num_columns(2).show(ui, |ui| {
                    ui.label("Контекст"); ui.add(egui::DragValue::new(&mut self.context).range(128..=32768)); ui.end_row();
                    ui.label("Токенов / ответ"); ui.add(egui::DragValue::new(&mut self.tokens).range(1..=8192)); ui.end_row();
                    ui.label("Вызовов"); ui.add(egui::DragValue::new(&mut self.calls).range(if self.subagents { 3..=24 } else { 1..=24 })); ui.end_row();
                    ui.label("CPU-потоков"); ui.add(egui::DragValue::new(&mut self.threads).range(1..=256)); ui.end_row();
                    ui.label("Таймаут, с"); ui.add(egui::DragValue::new(&mut self.timeout).range(1..=3600)); ui.end_row();
                });
            });
            egui::CollapsingHeader::new("Файлы контекста").show(ui, |ui| {
                ui.label("До 32 путей, по одному на строке. Пусто — ограниченный индекс проекта.");
                ui.add(egui::TextEdit::multiline(&mut self.files).desired_rows(4).desired_width(f32::INFINITY).hint_text("src/main.rs\nCargo.toml"));
            });
        });
        ui.separator();
        ui.label(RichText::new("ALPHA  ·  ЛОКАЛЬНО").small().color(ACCENT));
        ui.label("Модель предлагает изменения. Вы выбираете, что применить.");
        ui.label(RichText::new("Shell и автоматического запуска тестов пока нет. Качество зависит от выбранной модели.").small().weak());
    }

    fn body(&mut self, ui: &mut egui::Ui) {
        ui.heading("Над чем поработаем?");
        ui.label(
            RichText::new("Опиши изменение. Lyra изучит выбранный проект и предложит правки.")
                .weak(),
        );
        ui.add_enabled(
            self.job.is_none(),
            egui::TextEdit::multiline(&mut self.task)
                .desired_rows(4)
                .desired_width(f32::INFINITY)
                .char_limit(16384)
                .hint_text("Например: исправь вычисление суммы, сохрани существующие тесты."),
        );
        ui.horizontal(|ui| {
            let ready = self.job.is_none()
                && self.repo.is_some()
                && self.model.is_some()
                && !self.task.trim().is_empty();
            if ui
                .add_enabled(
                    ready,
                    egui::Button::new(RichText::new("Запустить задачу").strong())
                        .fill(Color32::from_rgb(86, 66, 153)),
                )
                .clicked()
                && let Err(error) = self.start(Operation::Task, ui.ctx())
            {
                self.fail(error);
            }
            if let Some(job) = &self.job {
                ui.spinner();
                ui.label(format!("{:.0} с", job.started.elapsed().as_secs_f32()));
                if job.cancellable && ui.button("Остановить").clicked() {
                    job.cancel();
                }
            }
        });
        ui.add_space(4.0);
        ui.label(RichText::new(&self.message).color(if self.error {
            Color32::from_rgb(255, 170, 150)
        } else {
            Color32::from_rgb(150, 213, 185)
        }));
        ui.separator();
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Changes, "Предложения");
            ui.selectable_value(&mut self.tab, Tab::Activity, "Журнал");
            ui.selectable_value(&mut self.tab, Tab::Hardware, "Мой компьютер");
        });
        egui::ScrollArea::vertical()
            .id_salt("results")
            .auto_shrink([false, false])
            .show(ui, |ui| match self.tab {
                Tab::Changes => self.changes(ui),
                Tab::Activity => {
                    if let Some(report) = &self.report {
                        ui.label(format!("Статус протокола: {}", report.status));
                        for event in &report.events {
                            ui.label(format!(
                                "{} · {} · {} · {}",
                                event.call, event.role, event.tool, event.outcome
                            ));
                        }
                    }
                    ui.separator();
                    ui.label("Журнал движка (последние 64 KiB)");
                    selectable_code(ui, &self.log, Color32::from_rgb(180, 187, 205));
                }
                Tab::Hardware => {
                    ui.label("Сведения относятся к компьютеру, на котором сейчас запущена Lyra.");
                    if ui
                        .add_enabled(
                            self.job.is_none(),
                            egui::Button::new("Обновить диагностику"),
                        )
                        .clicked()
                        && let Err(error) = self.start(Operation::Doctor, ui.ctx())
                    {
                        self.fail(error);
                    }
                    selectable_code(ui, &self.hardware, Color32::from_rgb(180, 187, 205));
                }
            });
    }

    fn changes(&mut self, ui: &mut egui::Ui) {
        let Some(report) = &self.report else {
            ui.add_space(24.0);
            ui.heading("Сначала — предложение, затем — решение");
            ui.label("Выбери проект и instruction-модель GGUF слева, затем введи задачу.");
            ui.label("Здесь появятся исходный и предложенный код. До отдельного применения файлы не меняются.");
            return;
        };
        ui.label(format!("Проект отчёта: {}", report.workspace.display()));
        for (role, summary) in &report.summaries {
            egui::CollapsingHeader::new(format!("Итог: {role}"))
                .default_open(true)
                .show(ui, |ui| {
                    ui.label(summary);
                });
        }
        if report.proposals.is_empty() {
            ui.label("Применимых предложений нет. Завершение протокола само по себе не означает решения задачи.");
            return;
        }
        ui.horizontal_wrapped(|ui| {
            for (index, edit) in report.proposals.iter().enumerate() {
                ui.selectable_value(
                    &mut self.selected,
                    index,
                    format!(
                        "{}{}",
                        edit.path,
                        if self.applied.contains(&index) {
                            " · применено"
                        } else {
                            ""
                        }
                    ),
                );
            }
        });
        if let Some(edit) = report.proposals.get(self.selected) {
            ui.columns(2, |columns| {
                columns[0].label(RichText::new("СЕЙЧАС").color(Color32::from_rgb(240, 160, 160)));
                selectable_code(
                    &mut columns[0],
                    &edit.original,
                    Color32::from_rgb(240, 190, 190),
                );
                columns[1]
                    .label(RichText::new("ПРЕДЛОЖЕНО").color(Color32::from_rgb(140, 220, 177)));
                selectable_code(
                    &mut columns[1],
                    &edit.replacement,
                    Color32::from_rgb(174, 224, 194),
                );
            });
        }
        ui.separator();
        ui.label("Ревью модели не заменяет тесты. После применения запусти проверки проекта.");
        let allowed = self.job.is_none() && can_apply(report, self.selected, &self.applied);
        if ui
            .add_enabled(
                allowed,
                egui::Button::new("Применить выбранное предложение…"),
            )
            .clicked()
        {
            self.confirm = Some(self.selected);
        }
        if ui.button("Сохранить JSON-отчёт…").clicked()
            && let Some(path) = rfd::FileDialog::new()
                .set_file_name("lyra-report.json")
                .add_filter("JSON", &["json"])
                .save_file()
            && let Err(error) = std::fs::write(path, &self.report_json)
        {
            self.fail(error);
        }
    }

    fn draw(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("header")
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(27, 30, 47))
                    .inner_margin(18),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("✦  LYRA").size(25.0).strong().color(ACCENT));
                    ui.label("Твоё локальное пространство для кодинга");
                });
            });
        egui::Panel::left("workspace")
            .default_size(300.0)
            .size_range(260.0..=380.0)
            .resizable(true)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(24, 28, 41))
                    .inner_margin(18),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.sidebar(ui));
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(19, 23, 35))
                    .inner_margin(24),
            )
            .show(ui, |ui| self.body(ui));
        if let Some(index) = self.confirm {
            let path = self
                .report
                .as_ref()
                .and_then(|r| r.proposals.get(index))
                .map(|e| e.path.clone())
                .unwrap_or_default();
            egui::Modal::new(egui::Id::unique("confirm-apply")).show(ui.ctx(), |ui| {
                ui.heading("Применить изменение?");
                ui.label(format!("Файл: {path}"));
                ui.label(
                    "Исходное содержимое будет повторно проверено. При конфликте запись отменится.",
                );
                ui.horizontal(|ui| {
                    if ui.button("Применить").clicked() {
                        self.confirm = None;
                        if let Err(error) = self.start(Operation::Apply(index), ui.ctx()) {
                            self.fail(error);
                        }
                    }
                    if ui.button("Отмена").clicked() {
                        self.confirm = None;
                    }
                });
            });
        }
    }
}

impl eframe::App for Desktop {
    fn logic(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.poll();
        if self.job.is_some() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        self.draw(ui);
        if let Some(counter) = &self.smoke {
            if counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= 2 {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                ui.ctx().request_repaint_after(Duration::from_millis(100));
            }
        }
    }
}

fn path_label(ui: &mut egui::Ui, path: Option<&PathBuf>, fallback: &str) {
    ui.label(
        RichText::new(
            path.map(|p| p.display().to_string())
                .unwrap_or_else(|| fallback.into()),
        )
        .small()
        .weak(),
    );
}
fn selectable_code(ui: &mut egui::Ui, text: &str, color: Color32) {
    ui.add(
        egui::Label::new(RichText::new(text).monospace().color(color))
            .selectable(true)
            .wrap(),
    );
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
    report.schema_version == 1
        && report.status == "completed"
        && report.proposals.get(index).is_some()
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
        let mut app = Desktop {
            report: Some(report),
            confirm: Some(0),
            ..Default::default()
        };
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 650.0),
                )),
                ..Default::default()
            },
            |ui| app.draw(ui),
        );
        assert!(!output.shapes.is_empty());
        assert_eq!(
            app.report.as_ref().unwrap().proposals[0].original,
            "left - right\r\n"
        );
        assert!(app.applied.is_empty());
    }
    #[test]
    fn empty_window_renders_without_display() {
        let ctx = egui::Context::default();
        configure(&ctx);
        let mut app = Desktop::default();
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1180.0, 820.0),
                )),
                ..Default::default()
            },
            |ui| app.draw(ui),
        );
        assert!(!output.shapes.is_empty());
    }
}
