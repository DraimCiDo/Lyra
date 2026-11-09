//! Main work view: context bar, conversation, empty-state guidance and the composer.
use super::{
    chat::{conversation_width, user_bubble_width},
    kit::{self, card},
    *,
};

/// Display only observable runtime activity; this is not model chain-of-thought.
fn runtime_progress(
    ui: &mut egui::Ui,
    snapshot: &job::ProgressSnapshot,
    elapsed: Duration,
    c: theme::Colors,
    column_width: f32,
    language: Language,
) {
    use job::ProgressStage;
    let stage = snapshot.stage;
    ui.horizontal_top(|ui| {
        avatar(ui, c);
        ui.vertical(|ui| {
            ui.set_width((column_width - 40.0).max(1.0));
            ui.label(RichText::new("Lyra").strong().color(c.accent));
            card(c).inner_margin(12).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                let steps = [
                    (ProgressStage::Starting, "Запуск"),
                    (ProgressStage::Loading, "Загрузка модели"),
                    (ProgressStage::Context, "Подготовка контекста"),
                    (ProgressStage::Generating, "Генерация ответа"),
                ];
                let current = steps.iter().position(|(s, _)| *s == stage).unwrap_or(0);
                for (index, (_, label)) in steps.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if index < current {
                            status_dot(ui, c, Some(true));
                            ui.label(RichText::new(language.text(label)).color(c.muted));
                        } else if index == current {
                            ui.add(egui::Spinner::new().size(14.0));
                            ui.label(RichText::new(language.text(label)).strong());
                        } else {
                            status_dot(ui, c, None);
                            ui.label(RichText::new(language.text(label)).color(c.muted));
                        }
                    });
                }
                ui.label(
                    RichText::new(format!(
                        "{} с · {} токенов",
                        elapsed.as_secs(),
                        snapshot.generated_tokens
                    ))
                    .small()
                    .color(c.muted),
                );
            });
            activity_list(ui, snapshot, true, language);
            if !snapshot.preview.is_empty() {
                ui.add_space(8.0);
                ui.add(
                    egui::Label::new(RichText::new(&snapshot.preview).color(c.text))
                        .selectable(true)
                        .wrap(),
                );
            }
        });
    });
}

fn activity_list(
    ui: &mut egui::Ui,
    snapshot: &job::ProgressSnapshot,
    running: bool,
    language: Language,
) {
    egui::CollapsingHeader::new(language.text("Ход работы"))
        .default_open(running)
        .show(ui, |ui| {
            if snapshot.activity.is_empty() {
                ui.label(language.text("Подготовка запроса…"));
            }
            for event in &snapshot.activity {
                ui.label(RichText::new(format!("› {}", language.text(event))).small());
            }
        });
}

fn status_dot(ui: &mut egui::Ui, c: theme::Colors, state: Option<bool>) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
    match state {
        Some(true) => {
            ui.painter()
                .circle_filled(rect.center(), 7.0, c.success.gamma_multiply(0.25));
            Icon::Check.image(c.success).paint_at(
                ui,
                egui::Rect::from_center_size(rect.center(), egui::vec2(10.0, 10.0)),
            );
        }
        Some(false) => {
            ui.painter()
                .circle_stroke(rect.center(), 6.5, egui::Stroke::new(1.5, c.warning));
        }
        None => {
            ui.painter()
                .circle_stroke(rect.center(), 6.5, egui::Stroke::new(1.0, c.border));
        }
    }
}

fn avatar(ui: &mut egui::Ui, c: theme::Colors) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), 14.0, c.accent_soft());
    Icon::Sparkles.image(c.accent).paint_at(
        ui,
        egui::Rect::from_center_size(rect.center(), egui::vec2(16.0, 16.0)),
    );
}

fn language_copy() -> &'static str {
    "Копировать / Copy"
}

fn message_view(ui: &mut egui::Ui, message: &chat::Message, c: theme::Colors, column_width: f32) {
    if message.user {
        let bubble_width = user_bubble_width(&message.text, column_width);
        ui.horizontal(|ui| {
            let offset = (column_width - bubble_width - ui.spacing().item_spacing.x).max(0.0);
            ui.add_space(offset);
            egui::Frame::new()
                .fill(c.accent_soft())
                .stroke(egui::Stroke::new(1.0, c.accent.gamma_multiply(0.35)))
                .corner_radius(14)
                .inner_margin(egui::Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.set_max_width((bubble_width - 28.0).max(1.0));
                    ui.add(
                        egui::Label::new(RichText::new(&message.text).color(c.text))
                            .selectable(true)
                            .wrap(),
                    );
                });
        });
    } else {
        ui.horizontal_top(|ui| {
            avatar(ui, c);
            ui.vertical(|ui| {
                ui.set_width((column_width - 40.0).max(1.0));
                ui.label(RichText::new("Lyra").strong().color(c.accent));
                ui.add(
                    egui::Label::new(RichText::new(&message.text).color(c.text))
                        .selectable(true)
                        .wrap(),
                );
                let copy = ui.add(
                    egui::Button::new(RichText::new(language_copy()).small().color(c.muted))
                        .frame(false),
                );
                if copy.clicked() {
                    ui.ctx().copy_text(message.text.clone());
                }
            });
        });
    }
}

impl Desktop {
    pub(super) fn model_ready_for_mode(&self) -> bool {
        match self.chat_mode {
            ChatMode::Chat => {
                #[cfg(feature = "gateway-http")]
                if self.prefs.cloud_chat {
                    return true;
                }
                self.prefs.model.is_some()
            }
            ChatMode::Task => {
                #[cfg(feature = "gateway-http")]
                if self.prefs.cloud_task && self.prefs.repo.is_some() {
                    return true;
                }
                self.prefs.model.is_some()
            }
        }
    }

    fn model_label(&self) -> String {
        let l = self.prefs.language;
        let cloud = cfg!(feature = "gateway-http")
            && match self.chat_mode {
                ChatMode::Chat => self.prefs.cloud_chat,
                ChatMode::Task => self.prefs.repo.is_some() && self.prefs.cloud_task,
            };
        if cloud {
            format!("{} / {}", self.prefs.cloud_provider, self.prefs.cloud_model)
        } else if let Some(model) = &self.prefs.model {
            let name = model
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let device = if cfg!(feature = "cuda") && self.prefs.gpu_layers > 0 {
                "CUDA"
            } else {
                "CPU"
            };
            format!("{name} · {device}")
        } else {
            l.text("Модель не выбрана").to_owned()
        }
    }

    fn context_bar(&mut self, ui: &mut egui::Ui, c: theme::Colors, l: Language) {
        ui.horizontal_wrapped(|ui| {
            let repo_name = self
                .prefs
                .repo
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|p| p.to_string_lossy().into_owned());
            let label = repo_name
                .clone()
                .unwrap_or_else(|| l.text("Выбрать проект").into());
            if kit::chip(ui, c, Some(Icon::Folder), &label, repo_name.is_some(), None)
                .on_hover_text(l.text("Папка проекта"))
                .clicked()
                && self.job.is_none()
            {
                self.choose_path(false);
            }
            if self.prefs.repo.is_some()
                && self.job.is_none()
                && ui
                    .add(egui::Button::new("×").frame(false))
                    .on_hover_text(l.text("Без проекта"))
                    .clicked()
            {
                if !self.chat.messages.is_empty() {
                    self.new_session();
                }
                self.prefs.repo = None;
                self.prefs.files.clear();
                self.capture_session();
            }
            ui.add_space(6.0);
            let before = self.chat_mode;
            ui.add_enabled_ui(self.chat.messages.is_empty() && self.job.is_none(), |ui| {
                kit::segmented(
                    ui,
                    c,
                    &mut self.chat_mode,
                    &[
                        (ChatMode::Chat, l.text("Чат")),
                        (ChatMode::Task, l.text("Работа")),
                    ],
                );
            });
            if before != self.chat_mode {
                self.capture_session();
            }
            ui.add_space(6.0);
            {
                if self.dock_visible()
                    && kit::chip(
                        ui,
                        c,
                        Some(Icon::Check),
                        l.text("Результат"),
                        self.dock_open,
                        None,
                    )
                    .on_hover_text("Ctrl+J")
                    .clicked()
                {
                    self.dock_open = !self.dock_open;
                }
                let agents = if self.chat_mode == ChatMode::Chat {
                    "Lyra".to_owned()
                } else if self.prefs.repo.is_none() {
                    "planner".to_owned()
                } else if self.prefs.subagents {
                    "researcher · coder · reviewer".to_owned()
                } else {
                    "coder".to_owned()
                };
                if kit::chip(ui, c, Some(Icon::Workers), &agents, false, None).clicked() {
                    self.open_settings(SettingsPage::Workers);
                }
                let ready = self.prefs.model.is_some() || self.model_ready_for_mode();
                let tint = if ready { None } else { Some(c.warning) };
                if kit::chip(ui, c, Some(Icon::Cpu), &self.model_label(), false, tint).clicked() {
                    self.open_settings(SettingsPage::Model);
                }
            }
        });
    }

    fn welcome(&mut self, ui: &mut egui::Ui, c: theme::Colors, l: Language, width: f32) {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.add(
                Icon::Sparkles
                    .image(c.accent)
                    .fit_to_exact_size(egui::vec2(40.0, 40.0)),
            );
            ui.add_space(6.0);
            ui.label(RichText::new(l.text("Что сделаем?")).size(28.0).strong());
            ui.label(
                RichText::new(l.text("Спросите, поставьте задачу по проекту или создайте новый."))
                    .color(c.muted),
            );
        });
        ui.add_space(16.0);
        // Setup checklist: only what is still missing.
        let model_ok = self.model_ready_for_mode();
        let repo_ok = self.prefs.repo.is_some();
        if !model_ok || (!repo_ok && self.chat_mode == ChatMode::Task) {
            card(c).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(RichText::new(l.text("Быстрая настройка")).strong());
                ui.horizontal(|ui| {
                    status_dot(ui, c, Some(model_ok));
                    ui.label(l.text("Модель"));
                    if !model_ok
                        && kit::primary_button(
                            ui,
                            c,
                            Some(Icon::Download),
                            l.text("Подобрать и установить GGUF"),
                            true,
                        )
                        .clicked()
                    {
                        self.open_settings(SettingsPage::Model);
                    }
                });
                ui.horizontal(|ui| {
                    status_dot(ui, c, if repo_ok { Some(true) } else { None });
                    ui.label(l.text("Проект (для режима «Работа»)"));
                    if !repo_ok
                        && kit::ghost_button(
                            ui,
                            c,
                            Some(Icon::FolderPlus),
                            l.text("Выбрать папку"),
                            true,
                        )
                        .clicked()
                    {
                        self.choose_path(false);
                    }
                });
            });
            ui.add_space(12.0);
        }
        let suggestions: [(&str, &str, &str, Icon, ChatMode); 4] = [
            (
                "Объяснить проект",
                "Структура, модули и точки входа",
                "Объясни структуру проекта: основные модули, точки входа и как они связаны.",
                Icon::BookOpen,
                ChatMode::Task,
            ),
            (
                "Найти и исправить ошибку",
                "Причина и минимальная правка",
                "Найди причину ошибки и предложи минимальное исправление: ",
                Icon::Check,
                ChatMode::Task,
            ),
            (
                "Добавить тесты",
                "Покрыть поведение проверяемыми тестами",
                "Добавь тесты для: ",
                Icon::Play,
                ChatMode::Task,
            ),
            (
                "Создать новый проект",
                "Спланировать и создать с нуля",
                "Создай проект: ",
                Icon::FolderPlus,
                ChatMode::Task,
            ),
        ];
        let tile_w = ((width - 12.0) / 2.0 - 1.0).max(150.0);
        egui::Grid::new("suggestions")
            .spacing([12.0, 12.0])
            .show(ui, |ui| {
                for (index, (title, hint, prompt, icon, mode)) in suggestions.iter().enumerate() {
                    let response = kit::tile(ui, c, false, egui::vec2(tile_w, 78.0), |ui| {
                        ui.horizontal(|ui| {
                            ui.add(
                                icon.image(c.accent)
                                    .fit_to_exact_size(egui::vec2(16.0, 16.0)),
                            );
                            ui.label(RichText::new(l.text(title)).strong());
                        });
                        ui.label(RichText::new(l.text(hint)).small().color(c.muted));
                    });
                    if response.clicked() {
                        self.task = l.text(prompt).to_owned();
                        self.chat_mode = *mode;
                        self.capture_session();
                        self.focus_composer = true;
                    }
                    if index % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            #[cfg(feature = "gateway-http")]
            if kit::ghost_button(ui, c, Some(Icon::Graph), l.text("Issue из GitHub"), true)
                .clicked()
            {
                self.open_settings(SettingsPage::Github);
            }
            if kit::ghost_button(ui, c, Some(Icon::Graph), l.text("Граф кода"), true).clicked()
            {
                self.graph_open = true;
                self.transition = Some(Instant::now());
            }
        });
    }

    pub(super) fn workspace(&mut self, ui: &mut egui::Ui, c: theme::Colors, l: Language) {
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(20, 10))
            .show(ui, |ui| self.context_bar(ui, c, l));
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 0.0, c.border);
        let composer_reserve = 168.0 + if self.error { 22.0 } else { 0.0 };
        let height = (ui.available_height() - composer_reserve).max(120.0);
        egui::ScrollArea::vertical()
            .id_salt("conversation")
            .min_scrolled_height(height)
            .max_height(height)
            .auto_shrink([false, false])
            .stick_to_bottom(!self.chat.messages.is_empty() || self.chat.pending)
            .show(ui, |ui| {
                let available = ui.available_width();
                let column_width = conversation_width(available)
                    .min(820.0)
                    .min((available - 24.0).max(200.0));
                ui.horizontal(|ui| {
                    let offset =
                        ((available - column_width) / 2.0 - ui.spacing().item_spacing.x).max(0.0);
                    ui.add_space(offset);
                    ui.vertical(|ui| {
                        ui.set_width(column_width);
                        if self.chat.messages.is_empty() && !self.chat.pending {
                            self.welcome(ui, c, l, column_width);
                        }
                        for message in &self.chat.messages {
                            ui.add_space(16.0);
                            message_view(ui, message, c, column_width);
                        }
                        if self.chat.pending {
                            ui.add_space(16.0);
                            if let Some(worker) = &self.job {
                                let mut snapshot = worker.progress();
                                if self.operation != Operation::LocalChat {
                                    snapshot.preview.clear();
                                }
                                runtime_progress(
                                    ui,
                                    &snapshot,
                                    worker.started.elapsed(),
                                    c,
                                    column_width,
                                    l,
                                );
                            }
                        }
                        if !self.chat.pending && !self.activity.activity.is_empty() {
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(format!(
                                    "{} · {:.1} s",
                                    l.text(if self.error {
                                        "Выполнение прервано"
                                    } else {
                                        "Выполнение завершено"
                                    }),
                                    self.activity_elapsed.as_secs_f32()
                                ))
                                .small()
                                .color(c.muted),
                            );
                            activity_list(ui, &self.activity, false, l);
                        }
                        if self.draft.plan.is_some() {
                            self.project_result(ui);
                        }
                        if self.report.is_some() && !self.dock_open {
                            ui.add_space(10.0);
                            if kit::ghost_button(
                                ui,
                                c,
                                Some(Icon::Check),
                                l.text("Показать результат задачи"),
                                true,
                            )
                            .clicked()
                            {
                                self.dock_open = true;
                            }
                        }
                        ui.add_space(12.0);
                    });
                });
            });
        ui.add_space(4.0);
        let composer_width = conversation_width(ui.available_width())
            .min(820.0)
            .min((ui.available_width() - 24.0).max(200.0));
        ui.horizontal(|ui| {
            let offset = ((ui.available_width() - composer_width) / 2.0
                - ui.spacing().item_spacing.x)
                .max(0.0);
            ui.add_space(offset);
            ui.allocate_ui_with_layout(
                egui::vec2(composer_width, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_min_width(composer_width);
                    self.composer(ui, c, l);
                },
            );
        });
    }

    fn composer(&mut self, ui: &mut egui::Ui, c: theme::Colors, l: Language) {
        egui::Frame::new()
            .fill(c.card)
            .stroke(egui::Stroke::new(1.0, c.border))
            .corner_radius(theme::RADIUS + 4)
            .inner_margin(egui::Margin::symmetric(14, 10))
            .show(ui, |ui| {
                let edit = egui::ScrollArea::vertical()
                    .id_salt("composer-scroll")
                    .min_scrolled_height(46.0)
                    .max_height(110.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add_enabled(
                            self.job.is_none(),
                            egui::TextEdit::multiline(&mut self.task)
                                .id_salt("chat-composer")
                                .desired_rows(2)
                                .desired_width(f32::INFINITY)
                                .char_limit(16384)
                                .frame(egui::Frame::NONE)
                                .return_key(Some(egui::KeyboardShortcut::new(
                                    egui::Modifiers::SHIFT,
                                    egui::Key::Enter,
                                )))
                                .hint_text(l.text(if self.chat_mode == ChatMode::Chat {
                                    "Спросите что угодно…"
                                } else {
                                    "Опишите задачу по проекту…"
                                })),
                        )
                    })
                    .inner;
                if edit.changed() {
                    self.capture_session();
                }
                if std::mem::take(&mut self.focus_composer) {
                    edit.request_focus();
                }
                let enter = edit.has_focus()
                    && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                ui.horizontal(|ui| {
                    let skills = self.prefs.selected_skills();
                    if !skills.is_empty() {
                        kit::chip(ui, c, Some(Icon::Sparkles), &skills.join(", "), true, None);
                    }
                    let files = self
                        .prefs
                        .files
                        .lines()
                        .filter(|s| !s.trim().is_empty())
                        .count();
                    if files > 0
                        && kit::chip(
                            ui,
                            c,
                            Some(Icon::Folder),
                            &format!("{files} {}", l.text("файлов")),
                            false,
                            None,
                        )
                        .clicked()
                    {
                        self.open_settings(SettingsPage::Context);
                    }
                    ui.label(
                        RichText::new(l.text("Enter — отправить · Shift+Enter — новая строка"))
                            .small()
                            .color(c.muted),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(job) = &self.job {
                            if job.cancellable
                                && kit::ghost_button(
                                    ui,
                                    c,
                                    Some(Icon::Stop),
                                    l.text("Остановить"),
                                    true,
                                )
                                .clicked()
                            {
                                job.cancel();
                            }
                        } else {
                            let ready = (self.model_ready_for_mode()
                                || self.direct_chat_reply(self.task.trim()).is_some())
                                && !self.task.trim().is_empty();
                            let clicked = kit::primary_button(
                                ui,
                                c,
                                Some(Icon::ArrowUp),
                                l.text("Отправить"),
                                ready,
                            )
                            .clicked();
                            if (clicked || enter && ready) && self.real_app {
                                if let Err(e) = self.submit_chat(ui.ctx()) {
                                    self.fail(e);
                                }
                                edit.request_focus();
                            }
                        }
                    });
                });
            });
    }
}
