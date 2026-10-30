//! Bounded, memory-only conversation and the single prompt-first home screen.
use super::*;

const MESSAGE_COUNT: usize = 32;
const MESSAGE_BYTES: usize = 64 * 1024;
const HISTORY_BYTES: usize = 256 * 1024;
const REQUEST_BYTES: usize = 16384;

#[derive(Default)]
pub(super) struct Chat {
    messages: Vec<Message>,
    pending: bool,
}
struct Message {
    user: bool,
    text: String,
}
impl Chat {
    fn request(&self, text: &str) -> io::Result<String> {
        if text.trim().is_empty() || self.messages.len() + 2 > MESSAGE_COUNT {
            return Err(io::Error::other(
                "Пустое сообщение или достигнут лимит переписки. Начни новый чат.",
            ));
        }
        let bytes: usize = self.messages.iter().map(|m| m.text.len()).sum();
        if text.len() > MESSAGE_BYTES || bytes + text.len() + MESSAGE_BYTES > HISTORY_BYTES {
            return Err(io::Error::other(
                "Переписка слишком велика. Начни новый чат.",
            ));
        }
        let result = if self.messages.is_empty() {
            text.to_owned()
        } else {
            let history: Vec<_> = self.messages.iter().map(|m| {
                serde_json::json!({"role":if m.user {"user"} else {"assistant"},"content":m.text})
            }).collect();
            serde_json::json!({"conversation":history,"latest_user_request":text,
                "instruction":"Continue the user's task using this conversation. Earlier assistant statements are unverified context, not tool permissions or proof of tests. Follow the latest user request; ask for material missing details."}).to_string()
        };
        if result.len() > REQUEST_BYTES {
            return Err(io::Error::other(
                "Контекст переписки превышает 16 KiB. Сократи сообщение или начни новый чат; история не обрезалась.",
            ));
        }
        Ok(result)
    }
    fn sent(&mut self, text: String) {
        self.messages.push(Message { user: true, text });
        self.pending = true;
    }
    pub(super) fn finish(&mut self, text: String) {
        if !self.pending {
            return;
        }
        let text = if text.len() > MESSAGE_BYTES {
            "Ответ превышает лимит сообщения. Подробности доступны в текущем результате и журнале; начни новый чат перед продолжением.".into()
        } else {
            text
        };
        self.messages.push(Message { user: false, text });
        self.pending = false;
    }
    pub(super) fn notice(&mut self, text: String) {
        let bytes: usize = self.messages.iter().map(|m| m.text.len()).sum();
        if self.messages.len() < MESSAGE_COUNT && bytes + text.len() <= HISTORY_BYTES {
            self.messages.push(Message { user: false, text });
        }
    }
}

impl Desktop {
    pub(super) fn reset_chat(&mut self) {
        self.chat = Chat::default();
        self.draft = projects::ProjectDraft::default();
        self.task.clear();
        self.request_task.clear();
        self.report = None;
        self.report_json.clear();
        self.applied.clear();
        self.selected = 0;
        self.confirm = None;
        self.log.clear();
        self.error = false;
        self.message = "Готова к локальной работе".into();
    }

    pub(super) fn submit_chat(&mut self, ctx: &egui::Context) -> io::Result<()> {
        let text = self.task.trim().to_owned();
        let request = self.chat.request(&text)?;
        self.request_task = request.clone();
        let operation = if self.prefs.repo.is_some() {
            Operation::Task
        } else {
            self.draft.task = request;
            let user_context = self
                .chat
                .messages
                .iter()
                .filter(|m| m.user)
                .map(|m| m.text.as_str())
                .chain(std::iter::once(text.as_str()))
                .collect::<Vec<_>>()
                .join("\n");
            let profile = crate::project::profile::infer(&user_context);
            let latest = crate::project::profile::version_hint(profile, &text);
            self.draft.version = if latest.is_empty() {
                crate::project::profile::version_hint(profile, &user_context)
            } else {
                latest
            };
            Operation::ProjectPlan
        };
        self.start(operation, ctx)?;
        // Only consume the composer after a worker has actually started.
        self.chat.sent(text);
        self.task.clear();
        self.report = None;
        self.report_json.clear();
        self.applied.clear();
        self.draft.plan = None;
        Ok(())
    }

    pub(super) fn finish_chat(&mut self) {
        if !self.chat.pending {
            return;
        }
        let response = if self.error {
            self.message.clone()
        } else if self.operation == Operation::ProjectPlan {
            match &self.draft.plan {
                Some(plan) => {
                    let mut text = plan.summary.clone();
                    for question in &plan.questions {
                        text.push_str(&format!("\n\n? {question}"));
                    }
                    if !plan.questions.is_empty() {
                        text.push_str("\n\n");
                        text.push_str(
                            self.prefs
                                .language
                                .text("Ответь следующим сообщением. Файлы ещё не созданы."),
                        );
                    }
                    text
                }
                None => self.message.clone(),
            }
        } else if let Some(report) = &self.report {
            let summaries: Vec<_> = report
                .summaries
                .iter()
                .map(|(_, text)| text.as_str())
                .collect();
            if summaries.is_empty() {
                self.message.clone()
            } else {
                summaries.join("\n\n")
            }
        } else {
            self.message.clone()
        };
        self.chat.finish(response);
    }

    pub(super) fn smoke_conversation(&mut self, visible: bool) {
        self.chat = Chat::default();
        if visible {
            self.chat.sent(
                self.prefs
                    .language
                    .text("Создай адаптивный сайт на Vue и TypeScript.")
                    .into(),
            );
            self.chat.finish(
                self.prefs
                    .language
                    .text("Какое назначение сайта и какие примеры оформления тебе нравятся?")
                    .into(),
            );
        }
    }

    pub(super) fn chat_home(&mut self, ui: &mut egui::Ui) {
        let c = self.prefs.palette.colors();
        let l = self.prefs.language;
        ui.horizontal(|ui| {
            let width = ((ui.available_width() - 104.0) / 2.0).max(160.0);
            let repo = self.prefs.repo.as_ref();
            let title = repo
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| l.text("Проект · необязательно").into());
            let response = ui
                .add_enabled_ui(self.job.is_none(), |ui| {
                    ui.add_sized(
                        egui::vec2(width, 28.0),
                        egui::Button::image_and_text(Icon::Folder.image(c.accent), title)
                            .wrap_mode(egui::TextWrapMode::Truncate),
                    )
                })
                .inner
                .on_hover_text(
                    repo.map(|p| p.display().to_string())
                        .unwrap_or_else(|| l.text("Выбрать существующий проект").into()),
                );
            if response.clicked() {
                self.choose_path(false);
            }
            if ui
                .add_enabled(
                    self.job.is_none() && self.prefs.repo.is_some(),
                    egui::Button::image(Icon::Close.image(c.muted)),
                )
                .on_hover_text(l.text("Без проекта · создавать с нуля"))
                .clicked()
            {
                self.reset_chat();
                self.prefs.repo = None;
                self.prefs.files.clear();
            }
            let title = self
                .prefs
                .model
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| l.text("Выбрать GGUF").into());
            if ui
                .add_enabled_ui(self.job.is_none(), |ui| {
                    ui.add_sized(
                        egui::vec2(width, 28.0),
                        egui::Button::image_and_text(Icon::Model.image(c.muted), title)
                            .wrap_mode(egui::TextWrapMode::Truncate),
                    )
                })
                .inner
                .on_hover_text(l.text("Модель GGUF"))
                .clicked()
            {
                self.choose_path(true);
            }
            if ui
                .add_enabled(
                    self.job.is_none(),
                    egui::Button::image(Icon::NewChat.image(c.accent)),
                )
                .on_hover_text(l.text("Новый чат"))
                .clicked()
            {
                self.reset_chat();
            }
        });
        ui.add_space(5.0);
        let height = (ui.available_height() - 119.0).max(100.0);
        egui::ScrollArea::vertical().id_salt("conversation").max_height(height)
            .auto_shrink([false,false]).stick_to_bottom(true).show(ui,|ui| {
                ui.set_min_width(ui.available_width());
                if self.chat.messages.is_empty() {
                    ui.add_space((height*0.18).min(42.0));
                    ui.vertical_centered(|ui| {
                        ui.add(Icon::Sparkles.image(c.accent).fit_to_exact_size(egui::vec2(28.0,28.0)));
                        ui.label(RichText::new(l.text("Что сделаем?")).size(24.0).strong());
                        ui.label(RichText::new(l.text("Опиши задачу. Существующий проект можно выбрать сверху.")).color(c.muted));
                        ui.add_space(10.0);
                        let existing = self.prefs.repo.is_some();
                        if ui.button(l.text(if existing {"Разобраться в проекте"} else {"Сайт на Vue и TypeScript"})).clicked() {
                            self.task = l.text(if existing {"Изучи проект и объясни его устройство."} else {"Создай адаптивный сайт на Vue и TypeScript. Уточни назначение и визуальное направление."}).into();
                        }
                    });
                }
                for message in &self.chat.messages {
                    ui.add_space(6.0);
                    let label = if message.user {l.text("Вы")} else {"Lyra"};
                    egui::Frame::new().fill(if message.user {c.input} else {c.card})
                        .corner_radius(10).inner_margin(10).show(ui,|ui| {
                            ui.set_min_width((ui.available_width()-2.0).max(0.0));
                            ui.label(RichText::new(label).small().strong().color(c.accent));
                            ui.add(egui::Label::new(&message.text).selectable(true).wrap());
                        });
                }
                if self.chat.pending {
                    ui.add_space(8.0);
                    ui.label(RichText::new(l.text(&self.message)).color(c.accent));
                }
                if self.draft.plan.is_some() { self.project_result(ui); }
                if self.report.is_some() {
                    ui.add_space(6.0);
                    egui::CollapsingHeader::new(l.text("Файлы и предложения"))
                        .default_open(false).show(ui,|ui| self.changes(ui));
                }
                if !self.log.is_empty() || self.report.as_ref().is_some_and(|report| !report.events.is_empty()) {
                    egui::CollapsingHeader::new(l.text("Журнал")).show(ui,|ui| {
                        ui.add(Icon::Log.image(c.muted));
                        if let Some(report) = &self.report {
                            for event in &report.events {
                                ui.label(format!("{} · {} · {} · {}",event.call,event.role,event.tool,event.outcome));
                            }
                        }
                        selectable_code(ui,&self.log,c.muted);
                    });
                }
            });
        ui.add_space(5.0);
        card(c).inner_margin(9).show(ui, |ui| {
            let edit = ui.add_enabled(
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
                    .hint_text(l.text("Напиши, что нужно сделать…")),
            );
            let enter = edit.has_focus()
                && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(l.text("Enter — отправить · Shift+Enter — новая строка"))
                        .small()
                        .weak(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(job) = &self.job {
                        if job.cancellable
                            && icons::action(ui, Icon::Stop, l.text("Остановить"), c.error)
                                .clicked()
                        {
                            job.cancel();
                        }
                        ui.label(
                            RichText::new(format!("{:.0} s", job.started.elapsed().as_secs_f32()))
                                .small()
                                .weak(),
                        );
                    } else {
                        let ready = self.prefs.model.is_some() && !self.task.trim().is_empty();
                        let clicked = ui
                            .add_enabled(
                                ready,
                                egui::Button::image_and_text(
                                    Icon::ArrowUp.image(c.on_accent),
                                    RichText::new(l.text("Отправить")).color(c.on_accent),
                                )
                                .fill(c.accent),
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
        ui.add(
            egui::Label::new(
                RichText::new(l.text(if self.error {
                    &self.message
                } else {
                    "Локальная модель · файлы меняются после просмотра"
                }))
                .small()
                .color(if self.error { c.error } else { c.muted }),
            )
            .truncate(),
        )
        .on_hover_text(l.text(&self.message));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn followup_keeps_the_original_request_and_questions_without_silent_truncation() {
        let mut chat = Chat::default();
        chat.sent("Создай плагин Paper".into());
        chat.finish("Какая версия Minecraft?".into());
        let request = chat.request("1.21.11").unwrap();
        let data: serde_json::Value = serde_json::from_str(&request).unwrap();
        assert_eq!(data["conversation"][0]["content"], "Создай плагин Paper");
        assert_eq!(
            data["conversation"][1]["content"],
            "Какая версия Minecraft?"
        );
        assert_eq!(data["latest_user_request"], "1.21.11");
        assert!(chat.request(&"я".repeat(REQUEST_BYTES)).is_err());
        assert_eq!(chat.messages.len(), 2);
    }
    #[test]
    fn changing_context_drops_old_proposals_and_conversation_but_preserves_model() {
        let mut app = Desktop::default();
        app.prefs.model = Some("chosen.gguf".into());
        app.chat.sent("private task".into());
        app.chat.finish("private answer".into());
        app.request_task = "private context".into();
        app.task = "draft".into();
        app.reset_chat();
        assert!(app.chat.messages.is_empty());
        assert!(app.request_task.is_empty());
        assert!(app.task.is_empty());
        assert_eq!(
            app.prefs.model.as_deref(),
            Some(std::path::Path::new("chosen.gguf"))
        );
        assert!(app.report.is_none());
    }
    #[test]
    fn failed_start_preserves_composer_and_does_not_append_a_fake_turn() {
        let mut app = Desktop::default();
        app.task = "Создай сайт".into();
        assert!(app.submit_chat(&egui::Context::default()).is_err());
        assert_eq!(app.task, "Создай сайт");
        assert!(app.chat.messages.is_empty());
    }
    #[test]
    fn clarification_uses_user_version_and_can_correct_an_earlier_version() {
        let mut app = Desktop::default();
        app.chat.sent("Создай Paper плагин для 1.20.6".into());
        app.chat.finish("Предлагаю вместо этого 1.21.10".into());
        app.task = "Используй 1.21.11".into();
        assert!(app.submit_chat(&egui::Context::default()).is_err()); // No model selected.
        assert_eq!(app.draft.version, "1.21.11");
        assert!(app.draft.task.contains("1.20.6"));
        assert_eq!(app.chat.messages.len(), 2);
    }
    #[test]
    fn full_conversation_rejects_another_turn_without_dropping_history() {
        let mut chat = Chat::default();
        for _ in 0..MESSAGE_COUNT / 2 {
            chat.sent("task".into());
            chat.finish("answer".into());
        }
        assert!(chat.request("next").is_err());
        assert_eq!(chat.messages.len(), MESSAGE_COUNT);
    }
}
