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
    pub(super) fn model_request(
        &self,
        latest: &str,
        tokens: u32,
    ) -> io::Result<crate::model_gateway::Request> {
        if latest.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Пустой запрос.",
            ));
        }
        let mut messages: Vec<crate::model_gateway::Message> = self
            .messages
            .iter()
            .map(|message| crate::model_gateway::Message {
                role: if message.user {
                    crate::model_gateway::Role::User
                } else {
                    crate::model_gateway::Role::Assistant
                },
                content: message.text.clone(),
            })
            .collect();
        messages.push(crate::model_gateway::Message {
            role: crate::model_gateway::Role::User,
            content: latest.to_owned(),
        });
        Ok(crate::model_gateway::Request {
            messages,
            max_output_tokens: tokens,
            require_tools: false,
        })
    }

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

/// Keep conversation content readable on wide displays instead of stretching every line.
fn conversation_width(available: f32) -> f32 {
    available.max(1.0).min(820.0)
}

/// User bubbles grow with the message but never occupy the entire conversation width.
/// UTF-8 character counts are only a visual estimate; egui wraps the actual text.
fn user_bubble_width(text: &str, available: f32) -> f32 {
    let cap = (available * 0.76).min(560.0).max(1.0);
    let longest_line = text
        .lines()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0)
        .min(256);
    (longest_line as f32 * 7.8 + 32.0)
        .max(100.0_f32.min(cap))
        .min(cap)
}

fn show_conversation_message(
    ui: &mut egui::Ui,
    message: &Message,
    colors: theme::Colors,
    column_width: f32,
) {
    if message.user {
        let bubble_width = user_bubble_width(&message.text, column_width);
        ui.horizontal(|ui| {
            ui.add_space((column_width - bubble_width).max(0.0));
            ui.allocate_ui_with_layout(
                egui::vec2(bubble_width, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_min_width(bubble_width);
                    egui::Frame::new()
                        .fill(colors.input)
                        .corner_radius(16)
                        .inner_margin(egui::Margin::symmetric(14, 10))
                        .show(ui, |ui| {
                            ui.set_min_width((bubble_width - 28.0).max(1.0));
                            ui.set_max_width((bubble_width - 28.0).max(1.0));
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&message.text).color(colors.text),
                                )
                                .selectable(true)
                                .wrap(),
                            );
                        });
                },
            );
        });
    } else {
        ui.horizontal(|ui| {
            ui.add(
                Icon::Sparkles
                    .image(colors.accent)
                    .fit_to_exact_size(egui::vec2(19.0, 19.0)),
            );
            ui.vertical(|ui| {
                ui.set_width((column_width - 36.0).max(1.0));
                ui.label(RichText::new("Lyra").strong().color(colors.accent));
                ui.add(
                    egui::Label::new(RichText::new(&message.text).color(colors.text))
                        .selectable(true)
                        .wrap(),
                );
            });
        });
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

    fn chat_operation(&self) -> Operation {
        match self.chat_mode {
            ChatMode::Chat => {
                #[cfg(feature = "gateway-http")]
                if self.prefs.cloud_chat {
                    return Operation::CloudChat;
                }
                Operation::LocalChat
            }
            ChatMode::Task => Operation::Task,
            ChatMode::Project => Operation::ProjectPlan,
        }
    }

    pub(super) fn submit_chat(&mut self, ctx: &egui::Context) -> io::Result<()> {
        let text = self.task.trim().to_owned();
        let request = self.chat.request(&text)?;
        self.request_task = request.clone();
        let operation = self.chat_operation();
        if operation == Operation::ProjectPlan {
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
        }
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
            let repo = self.prefs.repo.as_ref();
            let title = repo
                .and_then(|p| p.file_name())
                .map(|s| {
                    let s = s.to_string_lossy();
                    let mut title = s.chars().take(36).collect::<String>();
                    if s.chars().count() > 36 {
                        title.push('…');
                    }
                    title
                })
                .unwrap_or_else(|| l.text("Выбрать проект").into());
            if ui
                .allocate_ui_with_layout(
                    egui::vec2((ui.available_width() - 100.0).max(160.0), 28.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.add_enabled(
                            self.job.is_none(),
                            egui::Button::image_and_text(Icon::Folder.image(c.muted), title)
                                .frame(false)
                                .wrap_mode(egui::TextWrapMode::Truncate),
                        )
                    },
                )
                .inner
                .on_hover_text(
                    repo.map(|p| p.display().to_string())
                        .unwrap_or_else(|| l.text("Выбрать существующий проект").into()),
                )
                .clicked()
            {
                self.choose_path(false);
            }
            if self.prefs.repo.is_some()
                && ui
                    .add_enabled(
                        self.job.is_none(),
                        egui::Button::image(Icon::Close.image(c.muted)).frame(false),
                    )
                    .on_hover_text(l.text("Без проекта · создавать с нуля"))
                    .clicked()
            {
                self.reset_chat();
                self.prefs.repo = None;
                self.prefs.files.clear();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        self.job.is_none(),
                        egui::Button::image(Icon::NewChat.image(c.accent)).frame(false),
                    )
                    .on_hover_text(l.text("Новый чат"))
                    .clicked()
                {
                    self.reset_chat();
                }
            });
        });
        ui.add_enabled_ui(self.job.is_none(), |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.chat_mode, ChatMode::Chat, "Чат");
                ui.selectable_value(&mut self.chat_mode, ChatMode::Task, "Задача");
                ui.selectable_value(&mut self.chat_mode, ChatMode::Project, "Новый проект");
            });
        });
        if self.chat_mode == ChatMode::Task && self.prefs.repo.is_none() {
            ui.label(
                RichText::new("Для выполнения задачи выбери папку проекта.")
                    .small()
                    .weak(),
            );
        }
        #[cfg(feature = "gateway-http")]
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.prefs.cloud_chat, "HTTP Gateway");
            if self.prefs.cloud_chat {
                ui.label(format!(
                    "{} / {}",
                    self.prefs.cloud_provider, self.prefs.cloud_model
                ));
                ui.label(
                    RichText::new("Запросы отправляются выбранному провайдеру")
                        .small()
                        .weak(),
                );
            } else {
                ui.label(RichText::new("Локальный GGUF").small().weak());
            }
        });
        ui.add_space(5.0);
        let reserve = if self.error { 132.0 } else { 110.0 };
        let height = (ui.available_height() - reserve).max(100.0);
        egui::ScrollArea::vertical()
            .id_salt("conversation")
            .max_height(height)
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                let available = ui.available_width();
                let column_width = conversation_width(available);
                ui.horizontal(|ui| {
                    ui.add_space(((available - column_width) / 2.0).max(0.0));
                    ui.vertical(|ui| {
                        ui.set_min_width(column_width);
                        if self.chat.messages.is_empty() {
                            ui.add_space((height * 0.18).min(42.0));
                            ui.vertical_centered(|ui| {
                                ui.add(
                                    Icon::Sparkles
                                        .image(c.accent)
                                        .fit_to_exact_size(egui::vec2(28.0, 28.0)),
                                );
                                ui.label(
                                    RichText::new(l.text("Что сделаем?")).size(24.0).strong(),
                                );
                            });
                        }
                        for message in &self.chat.messages {
                            ui.add_space(14.0);
                            show_conversation_message(ui, message, c, column_width);
                        }
                        if self.chat.pending {
                            ui.add_space(14.0);
                            ui.horizontal(|ui| {
                                ui.add(Icon::Sparkles.image(c.accent));
                                ui.label(RichText::new(l.text(&self.message)).color(c.muted));
                            });
                        }
                        if self.draft.plan.is_some() {
                            self.project_result(ui);
                        }
                        if self.report.is_some() {
                            ui.add_space(8.0);
                            egui::CollapsingHeader::new(l.text("Файлы и предложения"))
                                .default_open(false)
                                .show(ui, |ui| self.changes(ui));
                        }
                        if !self.log.is_empty()
                            || self
                                .report
                                .as_ref()
                                .is_some_and(|report| !report.events.is_empty())
                        {
                            egui::CollapsingHeader::new(l.text("Журнал")).show(ui, |ui| {
                                ui.add(Icon::Log.image(c.muted));
                                if let Some(report) = &self.report {
                                    for event in &report.events {
                                        ui.label(format!(
                                            "{} · {} · {} · {}",
                                            event.call, event.role, event.tool, event.outcome
                                        ));
                                    }
                                }
                                selectable_code(ui, &self.log, c.muted);
                            });
                        }
                    });
                });
            });
        ui.add_space(5.0);
        card(c).inner_margin(9).show(ui, |ui| {
            let edit = egui::ScrollArea::vertical()
                .id_salt("composer-scroll")
                .min_scrolled_height(32.0)
                .max_height(36.0)
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
                            .hint_text(l.text("Напиши, что нужно сделать…")),
                    )
                })
                .inner;
            let enter = edit.has_focus()
                && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            ui.horizontal(|ui| {
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
                        let model_ready = match self.chat_mode {
                            ChatMode::Chat => {
                                #[cfg(feature = "gateway-http")]
                                if self.prefs.cloud_chat {
                                    true
                                } else {
                                    self.prefs.model.is_some()
                                }
                                #[cfg(not(feature = "gateway-http"))]
                                {
                                    self.prefs.model.is_some()
                                }
                            }
                            ChatMode::Task => {
                                #[cfg(feature = "gateway-http")]
                                if self.prefs.cloud_task {
                                    true
                                } else {
                                    self.prefs.model.is_some()
                                }
                                #[cfg(not(feature = "gateway-http"))]
                                {
                                    self.prefs.model.is_some()
                                }
                            }
                            ChatMode::Project => self.prefs.model.is_some(),
                        };
                        let ready = model_ready && !self.task.trim().is_empty();
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
        if self.error {
            ui.add(
                egui::Label::new(RichText::new(l.text(&self.message)).small().color(c.error))
                    .truncate(),
            )
            .on_hover_text(l.text(&self.message));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chat_layout_caps_long_messages_and_keeps_short_bubbles_compact() {
        assert_eq!(conversation_width(1600.0), 820.0);
        assert_eq!(conversation_width(520.0), 520.0);
        assert!(user_bubble_width("Привет", 820.0) < 180.0);
        assert!(user_bubble_width(&"очень длинный текст ".repeat(100), 820.0) <= 560.0);
        assert!(user_bubble_width("a".repeat(400).as_str(), 400.0) <= 304.0);
        assert!(user_bubble_width("Привет\\nмир", 400.0) < 200.0);
    }

    #[test]
    fn model_request_preserves_roles_and_latest_user_message() {
        let mut chat = Chat::default();
        chat.sent("First".into());
        chat.finish("Answer".into());
        let request = chat.model_request("Follow-up", 256).unwrap();
        assert_eq!(request.messages.len(), 3);
        assert_eq!(request.messages[0].role, crate::model_gateway::Role::User);
        assert_eq!(
            request.messages[1].role,
            crate::model_gateway::Role::Assistant
        );
        assert_eq!(request.messages[2].content, "Follow-up");
        assert_eq!(request.max_output_tokens, 256);
        assert!(chat.model_request(" ", 256).is_err());
    }

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
        app.chat_mode = ChatMode::Project;
        app.task = "Используй 1.21.11".into();
        assert!(app.submit_chat(&egui::Context::default()).is_err()); // No model selected.
        assert_eq!(app.draft.version, "1.21.11");
        assert!(app.draft.task.contains("1.20.6"));
        assert_eq!(app.chat.messages.len(), 2);
    }
    #[test]
    fn conversation_routing_requires_explicit_task_or_project_mode() {
        let mut app = Desktop::default();
        assert!(matches!(app.chat_operation(), Operation::LocalChat));
        app.prefs.repo = Some("example".into());
        assert!(matches!(app.chat_operation(), Operation::LocalChat));
        app.chat_mode = ChatMode::Task;
        assert!(matches!(app.chat_operation(), Operation::Task));
        app.chat_mode = ChatMode::Project;
        assert!(matches!(app.chat_operation(), Operation::ProjectPlan));
        #[cfg(feature = "gateway-http")]
        {
            app.chat_mode = ChatMode::Chat;
            app.prefs.cloud_chat = true;
            assert!(matches!(app.chat_operation(), Operation::CloudChat));
            app.prefs.cloud_chat = false;
            assert!(matches!(app.chat_operation(), Operation::LocalChat));
        }
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
