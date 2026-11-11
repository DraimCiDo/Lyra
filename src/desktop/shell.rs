//! Application shell: navigation rail, conversation list, status bar, command palette, modals.
use super::{kit, *};
use eframe::egui::{Key, KeyboardShortcut, Modifiers};

const RAIL_WIDTH: f32 = 60.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum PaletteAction {
    NewChat,
    ChooseProject,
    Work,
    Graph,
    Settings(SettingsPage),
    Theme(Palette),
    Language(Language),
    ToggleSidebar,
    ToggleDock,
    Help,
}

impl Desktop {
    pub(super) fn open_settings(&mut self, page: SettingsPage) {
        self.settings_open = true;
        self.graph_open = false;
        self.settings_page = page;
        self.transition = Some(Instant::now());
    }

    pub(super) fn show_work(&mut self) {
        self.settings_open = false;
        self.graph_open = false;
        self.transition = Some(Instant::now());
    }

    fn show_graph(&mut self) {
        self.settings_open = false;
        self.graph_open = true;
        self.transition = Some(Instant::now());
    }

    pub(super) fn palette_items(&self) -> Vec<(PaletteAction, String, String)> {
        let l = self.prefs.language;
        let mut items = vec![
            (
                PaletteAction::NewChat,
                l.text("Новый чат").to_owned(),
                "Ctrl+N".to_owned(),
            ),
            (
                PaletteAction::ChooseProject,
                l.text("Выбрать папку проекта…").to_owned(),
                String::new(),
            ),
            (
                PaletteAction::Work,
                l.text("Перейти к чату").to_owned(),
                String::new(),
            ),
            (
                PaletteAction::Graph,
                l.text("Открыть граф кода").to_owned(),
                String::new(),
            ),
            (
                PaletteAction::ToggleSidebar,
                l.text("Показать или скрыть список чатов").to_owned(),
                "Ctrl+B".to_owned(),
            ),
            (
                PaletteAction::ToggleDock,
                l.text("Показать или скрыть панель изменений").to_owned(),
                "Ctrl+J".to_owned(),
            ),
        ];
        for page in SettingsPage::ALL {
            items.push((
                PaletteAction::Settings(page),
                format!("{}: {}", l.text("Настройки"), l.text(page.title())),
                if page == SettingsPage::Model {
                    "Ctrl+,".into()
                } else {
                    String::new()
                },
            ));
        }
        for palette in Palette::ALL {
            items.push((
                PaletteAction::Theme(palette),
                format!("{}: {}", l.text("Тема"), l.text(palette.name())),
                String::new(),
            ));
        }
        items.push((
            PaletteAction::Language(Language::Ru),
            "Язык: Русский".into(),
            String::new(),
        ));
        items.push((
            PaletteAction::Language(Language::En),
            "Language: English".into(),
            String::new(),
        ));
        items.push((
            PaletteAction::Help,
            l.text("Как пользоваться").to_owned(),
            "F1".to_owned(),
        ));
        items
    }

    pub(super) fn run_palette_action(&mut self, action: PaletteAction, ctx: &egui::Context) {
        match action {
            PaletteAction::NewChat => {
                self.show_work();
                self.new_session();
            }
            PaletteAction::ChooseProject => self.choose_path(false),
            PaletteAction::Work => self.show_work(),
            PaletteAction::Graph => self.show_graph(),
            PaletteAction::Settings(page) => self.open_settings(page),
            PaletteAction::Theme(p) => self.prefs.palette = p,
            PaletteAction::Language(lang) => self.prefs.language = lang,
            PaletteAction::ToggleSidebar => self.sessions_open = !self.sessions_open,
            PaletteAction::ToggleDock => self.dock_open = !self.dock_open,
            PaletteAction::Help => self.help_open = true,
        }
        ctx.request_repaint();
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let pressed = |ctx: &egui::Context, key: Key, mods: Modifiers| {
            ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(mods, key)))
        };
        if pressed(ctx, Key::K, Modifiers::COMMAND) {
            self.palette_open = !self.palette_open;
            self.palette_query.clear();
        }
        if pressed(ctx, Key::N, Modifiers::COMMAND) {
            self.run_palette_action(PaletteAction::NewChat, ctx);
        }
        if pressed(ctx, Key::Comma, Modifiers::COMMAND) {
            self.open_settings(self.settings_page);
        }
        if pressed(ctx, Key::B, Modifiers::COMMAND) {
            self.sessions_open = !self.sessions_open;
        }
        if pressed(ctx, Key::J, Modifiers::COMMAND) {
            self.dock_open = !self.dock_open;
        }
        if pressed(ctx, Key::F1, Modifiers::NONE) {
            self.help_open = true;
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            if self.palette_open {
                self.palette_open = false;
            } else if self.help_open {
                self.help_open = false;
            } else if self.settings_open || self.graph_open {
                self.show_work();
            }
        }
    }

    pub(super) fn draw(&mut self, ui: &mut egui::Ui) {
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
        let ctx = ui.ctx().clone();
        if self.real_app || self.smoke.is_none() {
            self.shortcuts(&ctx);
        }

        egui::Panel::left("rail")
            .exact_size(RAIL_WIDTH)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(c.surface)
                    .inner_margin(egui::Margin::symmetric(8, 12)),
            )
            .show(ui, |ui| self.rail(ui, c));

        egui::Panel::bottom("status")
            .exact_size(26.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(c.surface)
                    .inner_margin(egui::Margin::symmetric(12, 4)),
            )
            .show(ui, |ui| self.status_bar(ui, c));

        let work = !self.settings_open && !self.graph_open;
        // Side panels yield to the conversation on narrow windows.
        let total = ctx.content_rect().width();
        let dock_shown = work && self.dock_open && self.dock_visible() && total >= 820.0;
        let sessions_shown =
            work && self.sessions_open && total >= if dock_shown { 1240.0 } else { 900.0 };
        if sessions_shown {
            egui::Panel::left("sessions")
                .default_size(256.0)
                .size_range(200.0..=360.0)
                .frame(
                    egui::Frame::new()
                        .fill(c.surface)
                        .stroke(egui::Stroke::new(1.0, c.border))
                        .inner_margin(egui::Margin::symmetric(10, 12)),
                )
                .show(ui, |ui| self.sessions_panel(ui, c));
        }
        if dock_shown {
            egui::Panel::right("dock")
                .default_size(if total < 1300.0 { 400.0 } else { 460.0 })
                .size_range(320.0..=760.0)
                .frame(
                    egui::Frame::new()
                        .fill(c.surface)
                        .stroke(egui::Stroke::new(1.0, c.border))
                        .inner_margin(egui::Margin::symmetric(14, 12)),
                )
                .show(ui, |ui| self.dock(ui, c));
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(c.background))
            .show(ui, |ui| {
                if let Some(start) = self.transition {
                    let t = start.elapsed().as_secs_f32() / 0.14;
                    if self.prefs.animations && t < 1.0 {
                        ui.set_opacity(0.5 + 0.5 * t);
                        ui.ctx().request_repaint_after(Duration::from_millis(16));
                    } else {
                        self.transition = None;
                    }
                }
                if self.settings_open {
                    self.settings(ui);
                } else if self.graph_open {
                    egui::Frame::new()
                        .inner_margin(egui::Margin::symmetric(20, 14))
                        .show(ui, |ui| {
                            let repo = self.prefs.repo.clone();
                            self.graph.ui(
                                ui,
                                c,
                                l,
                                repo.as_ref(),
                                &self.prefs.github_repo,
                                &self.prefs.github_token_env,
                            );
                        });
                } else {
                    self.workspace(ui, c, l);
                }
            });
        self.modals(ui, c, l);
        self.command_palette(&ctx, c);
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
        self.save_history(ui.ctx(), false);
    }

    fn rail(&mut self, ui: &mut egui::Ui, c: theme::Colors) {
        let l = self.prefs.language;
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.add(
                Icon::Sparkles
                    .image(c.accent)
                    .fit_to_exact_size(egui::vec2(28.0, 28.0)),
            );
            ui.add_space(14.0);
            let work = !self.settings_open && !self.graph_open;
            if kit::rail_button(ui, c, Icon::NewChat, l.text("Чат и задачи"), work).clicked()
            {
                if work {
                    self.sessions_open = !self.sessions_open;
                } else {
                    self.show_work();
                }
            }
            if kit::rail_button(ui, c, Icon::Graph, l.text("Граф кода"), self.graph_open).clicked()
            {
                self.show_graph();
            }
            if (self.prefs.repo.is_some() || self.report.is_some())
                && self.job.is_none()
                && kit::rail_button(ui, c, Icon::Folder, l.text("Восстановление проекта"), false)
                    .clicked()
                && let Err(e) = self.start(Operation::RecoveryStatus, ui.ctx())
            {
                self.fail(e);
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                if kit::rail_button(
                    ui,
                    c,
                    Icon::Settings,
                    &format!("{} (Ctrl+,)", l.text("Настройки")),
                    self.settings_open && self.settings_page != SettingsPage::Updates,
                )
                .clicked()
                {
                    self.open_settings(self.settings_page);
                }
                let response = kit::rail_button(
                    ui,
                    c,
                    Icon::Download,
                    l.text("Обновления"),
                    self.settings_open && self.settings_page == SettingsPage::Updates,
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
                            response.rect.right_top() + egui::vec2(-6.0, 6.0),
                            4.0,
                            c.accent,
                        );
                    }
                    if blink {
                        ui.ctx().request_repaint_after(Duration::from_millis(150));
                    }
                }
                if response.clicked() {
                    self.open_settings(SettingsPage::Updates);
                }
                if kit::rail_button(
                    ui,
                    c,
                    Icon::Help,
                    &format!("{} (F1)", l.text("Как пользоваться")),
                    false,
                )
                .clicked()
                {
                    self.help_open = true;
                }
                if kit::rail_button(
                    ui,
                    c,
                    Icon::Search,
                    &format!("{} (Ctrl+K)", l.text("Команды")),
                    self.palette_open,
                )
                .clicked()
                {
                    self.palette_open = !self.palette_open;
                    self.palette_query.clear();
                }
            });
        });
    }

    fn sessions_panel(&mut self, ui: &mut egui::Ui, c: theme::Colors) {
        let l = self.prefs.language;
        ui.horizontal(|ui| {
            ui.label(RichText::new(l.text("Чаты")).strong().size(15.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::chip(ui, c, Some(Icon::NewChat), l.text("Новый"), false, None)
                    .on_hover_text("Ctrl+N")
                    .clicked()
                    && self.job.is_none()
                {
                    self.new_session();
                }
            });
        });
        ui.add_space(6.0);
        let mut open = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for (index, session) in self.conversations.sessions.iter().enumerate().rev() {
                    let repo = session
                        .repo
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let response = ui.add_enabled_ui(self.job.is_none(), |ui| {
                        kit::list_row(
                            ui,
                            c,
                            index == self.conversations.active,
                            &session.title(),
                            &repo,
                        )
                    });
                    if response.inner.clicked() {
                        open = Some(index);
                    }
                }
            });
        if let Some(index) = open {
            self.open_session(index);
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui, c: theme::Colors) {
        let l = self.prefs.language;
        ui.horizontal_centered(|ui| {
            let (text, color) = if let Some(job) = &self.job {
                let snapshot = job.progress();
                let label = snapshot
                    .activity
                    .last()
                    .map(|a| l.text(a).to_owned())
                    .unwrap_or_else(|| l.text("Выполняется…").to_owned());
                (
                    format!("{label} · {} с", job.started.elapsed().as_secs()),
                    c.accent,
                )
            } else if self.error {
                (l.text(&self.message).to_owned(), c.error)
            } else {
                (l.text(&self.message).to_owned(), c.muted)
            };
            if self.job.is_some() {
                ui.add(egui::Spinner::new().size(12.0));
            } else {
                let dot = if self.error { c.error } else { c.success };
                let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 3.5, dot);
            }
            ui.add(egui::Label::new(RichText::new(&text).small().color(color)).truncate())
                .on_hover_text(&text);
            if let Some(job) = &self.job
                && job.cancellable
                && ui
                    .add(
                        egui::Button::new(
                            RichText::new(l.text("Остановить")).small().color(c.error),
                        )
                        .frame(false),
                    )
                    .clicked()
            {
                job.cancel();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                        .small()
                        .color(c.muted),
                );
                kit::kbd(ui, c, "Ctrl+K");
            });
        });
    }

    fn command_palette(&mut self, ctx: &egui::Context, c: theme::Colors) {
        if !self.palette_open {
            return;
        }
        let l = self.prefs.language;
        let query = self.palette_query.to_lowercase();
        let items: Vec<_> = self
            .palette_items()
            .into_iter()
            .filter(|(_, label, _)| query.is_empty() || label.to_lowercase().contains(&query))
            .collect();
        let mut chosen = None;
        let modal = egui::Modal::new(egui::Id::unique("command-palette")).show(ctx, |ui| {
            ui.set_width(520.0f32.min(ctx.content_rect().width() - 40.0));
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.palette_query)
                    .hint_text(l.text("Введите команду…"))
                    .desired_width(f32::INFINITY)
                    .margin(egui::Margin::symmetric(10, 8)),
            );
            edit.request_focus();
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .max_height(340.0)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    if items.is_empty() {
                        ui.label(RichText::new(l.text("Ничего не найдено")).color(c.muted));
                    }
                    for (index, (action, label, hint)) in items.iter().enumerate() {
                        let r = kit::list_row(ui, c, index == 0, label, "");
                        if !hint.is_empty() {
                            ui.painter().text(
                                r.rect.right_center() - egui::vec2(10.0, 0.0),
                                egui::Align2::RIGHT_CENTER,
                                hint,
                                egui::FontId::monospace(12.0),
                                c.muted,
                            );
                        }
                        if r.clicked() {
                            chosen = Some(*action);
                        }
                    }
                });
            if ui.input(|i| i.key_pressed(Key::Enter))
                && let Some((action, _, _)) = items.first()
            {
                chosen = Some(*action);
            }
        });
        if modal.should_close() {
            self.palette_open = false;
        }
        if let Some(action) = chosen {
            self.palette_open = false;
            self.run_palette_action(action, ctx);
        }
    }

    fn modals(&mut self, ui: &mut egui::Ui, c: theme::Colors, l: Language) {
        if let Some(operation) = self.confirm {
            let path = match operation {
                Operation::Apply(index) => self
                    .report
                    .as_ref()
                    .and_then(|r| {
                        r.proposals.get(index).map(|e| e.path.clone()).or_else(|| {
                            index
                                .checked_sub(r.proposals.len())
                                .and_then(|i| r.file_proposals.get(i))
                                .map(|p| p.path.clone())
                        })
                    })
                    .unwrap_or_default(),
                Operation::ApplyAll => self
                    .report
                    .as_ref()
                    .map(|r| {
                        r.proposals
                            .iter()
                            .map(|e| e.path.as_str())
                            .chain(r.file_proposals.iter().map(|p| p.path.as_str()))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default(),
                _ => self
                    .recovery_root
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
            };
            egui::Modal::new(egui::Id::unique("confirm-apply")).show(ui.ctx(), |ui| {
                ui.heading(l.text(match operation {
                    Operation::ApplyAll => "Применить весь набор?",
                    Operation::RecoveryRollback => "Откатить публикацию?",
                    Operation::RecoveryComplete => "Завершить публикацию?",
                    _ => "Применить изменение?",
                }));
                egui::ScrollArea::vertical()
                    .max_height(220.0)
                    .show(ui, |ui| {
                        ui.label(path);
                    });
                ui.label(l.text(
                    "Исходное содержимое будет проверено ещё раз. При конфликте запись отменится.",
                ));
                ui.horizontal(|ui| {
                    if icons::action(ui, Icon::Check, l.text("Применить"), c.accent).clicked()
                    {
                        self.confirm = None;
                        if let Err(e) = self.start(operation, ui.ctx()) {
                            self.fail(e);
                        }
                    }
                    if icons::action(ui, Icon::Close, l.text("Отмена"), c.muted).clicked() {
                        self.confirm = None;
                    }
                });
            });
        }
        self.recovery_window(ui.ctx());
        self.saved_tasks_window(ui.ctx());
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
                ui.label(l.text("1. Выбери локальную GGUF. Папка проекта — по необходимости."));
                ui.label(l.text("2. Напиши задачу в чате. Без папки Lyra подготовит новый проект."));
                ui.label(l.text(
                    "3. Ответь на вопросы, просмотри файлы и явно создай проект или примени правку.",
                ));
                ui.label(
                    RichText::new(l.text(
                        "Модель выполняет только явно выбранную Docker-проверку. Ревью AI не заменяет тестирование.",
                    ))
                    .small()
                    .weak(),
                );
                if ui.button(l.text("Понятно")).clicked() {
                    self.help_open = false;
                }
            });
        }
    }
}
