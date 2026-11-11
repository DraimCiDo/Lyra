//! Settings: grouped navigation with search on the left, one focused page on the right.
//! Every page is a list of titled sections; individual options are title + hint + control rows.
use super::{
    kit::{self, section, setting_row},
    *,
};

impl Desktop {
    pub(super) fn matching_pages(&self) -> Vec<SettingsPage> {
        let q = self.settings_query.trim().to_lowercase();
        let l = self.prefs.language;
        SettingsPage::ALL
            .into_iter()
            .filter(|p| {
                q.is_empty()
                    || l.text(p.title()).to_lowercase().contains(&q)
                    || p.title().to_lowercase().contains(&q)
                    || l.text(p.description()).to_lowercase().contains(&q)
                    || p.keywords().contains(&q)
            })
            .collect()
    }

    fn settings_nav(&mut self, ui: &mut egui::Ui, c: theme::Colors) {
        let l = self.prefs.language;
        let matches = self.matching_pages();
        ui.add(
            egui::TextEdit::singleline(&mut self.settings_query)
                .hint_text(l.text("Поиск по настройкам"))
                .desired_width(f32::INFINITY)
                .margin(egui::Margin::symmetric(10, 7)),
        );
        ui.add_space(6.0);
        egui::ScrollArea::vertical()
            .id_salt("settings-nav")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for group in SettingsPage::GROUPS {
                    let pages: Vec<_> = matches
                        .iter()
                        .copied()
                        .filter(|p| p.group() == group)
                        .collect();
                    if pages.is_empty() {
                        continue;
                    }
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(l.text(group).to_uppercase())
                            .small()
                            .strong()
                            .color(c.muted),
                    );
                    for page in pages {
                        let selected = self.settings_page == page;
                        let (rect, response) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 34.0),
                            egui::Sense::click(),
                        );
                        if selected || response.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                8,
                                if selected { c.accent_soft() } else { c.hover },
                            );
                        }
                        page.icon()
                            .image(if selected { c.accent } else { c.muted })
                            .paint_at(
                                ui,
                                egui::Rect::from_center_size(
                                    egui::pos2(rect.left() + 20.0, rect.center().y),
                                    egui::vec2(17.0, 17.0),
                                ),
                            );
                        ui.painter().text(
                            egui::pos2(rect.left() + 38.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            l.text(page.title()),
                            egui::FontId::proportional(14.0),
                            c.text,
                        );
                        if page == SettingsPage::Updates && self.updates.available.is_some() {
                            ui.painter().circle_filled(
                                egui::pos2(rect.right() - 14.0, rect.center().y),
                                4.0,
                                c.accent,
                            );
                        }
                        if response
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                            && !selected
                        {
                            self.settings_page = page;
                            self.transition = Some(Instant::now());
                        }
                    }
                }
                if matches.is_empty() {
                    ui.label(RichText::new(l.text("Ничего не найдено")).color(c.muted));
                }
            });
        if let Some(first) = matches.first()
            && !matches.contains(&self.settings_page)
        {
            self.settings_page = *first;
        }
    }

    pub(super) fn settings(&mut self, ui: &mut egui::Ui) {
        let c = self.prefs.palette.colors();
        let l = self.prefs.language;
        let wide = ui.available_width() > 760.0;
        if wide {
            egui::Panel::left("settings-nav-panel")
                .exact_size(248.0)
                .resizable(false)
                .show_separator_line(false)
                .frame(
                    egui::Frame::new()
                        .fill(c.surface)
                        .stroke(egui::Stroke::new(1.0, c.border))
                        .inner_margin(egui::Margin::symmetric(12, 14)),
                )
                .show(ui, |ui| {
                    ui.label(RichText::new(l.text("Настройки")).strong().size(18.0));
                    ui.add_space(8.0);
                    self.settings_nav(ui, c);
                });
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(c.background)
                    .inner_margin(egui::Margin::symmetric(24, 16)),
            )
            .show(ui, |ui| {
                if !wide {
                    egui::ScrollArea::horizontal()
                        .id_salt("settings-chips")
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for page in SettingsPage::ALL {
                                    if kit::chip(
                                        ui,
                                        c,
                                        Some(page.icon()),
                                        l.text(page.title()),
                                        self.settings_page == page,
                                        None,
                                    )
                                    .clicked()
                                    {
                                        self.settings_page = page;
                                    }
                                }
                            });
                        });
                    ui.add_space(6.0);
                }
                let page = self.settings_page;
                ui.horizontal(|ui| {
                    ui.add(
                        page.icon()
                            .image(c.accent)
                            .fit_to_exact_size(egui::vec2(24.0, 24.0)),
                    );
                    ui.heading(l.text(page.title()));
                });
                ui.label(RichText::new(l.text(page.description())).color(c.muted));
                ui.add_space(8.0);
                egui::ScrollArea::vertical()
                    .id_salt(("settings", page as u8))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_max_width(ui.available_width().min(860.0));
                        let busy = self.job.is_some();
                        if busy
                            && matches!(
                                page,
                                SettingsPage::Model
                                    | SettingsPage::Context
                                    | SettingsPage::Workers
                                    | SettingsPage::Checks
                                    | SettingsPage::Skills
                            )
                        {
                            kit::badge(
                                ui,
                                l.text("Выполняется задача: часть настроек недоступна"),
                                c.warning,
                            );
                        }
                        self.settings_body(ui, c, l);
                        if let Some(error) = &self.preference_error {
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(format!(
                                    "{}: {error}",
                                    l.text("Настройки не сохранены")
                                ))
                                .small()
                                .color(c.error),
                            );
                        }
                        ui.add_space(24.0);
                    });
            });
    }

    fn settings_body(&mut self, ui: &mut egui::Ui, c: theme::Colors, l: Language) {
        let enabled = self.job.is_none();
        match self.settings_page {
            SettingsPage::Model => self.model_settings(ui),
            #[cfg(feature = "gateway-http")]
            SettingsPage::Providers => self.providers_settings(ui, c, l),
            SettingsPage::Context => self.context_settings(ui, c, l),
            SettingsPage::Workers => self.agent_settings(ui, c, l),
            SettingsPage::Checks => {
                ui.add_enabled_ui(enabled, |ui| self.workflow_settings(ui));
            }
            SettingsPage::Github => self.github_settings(ui, c, l),
            SettingsPage::Skills => {
                section(ui, c, "", "", |ui| {
                    ui.add_enabled_ui(enabled, |ui| self.skill_settings(ui));
                });
            }
            SettingsPage::Knowledge => {
                section(ui, c, "", "", |ui| self.knowledge_settings(ui));
            }
            SettingsPage::Appearance => self.appearance_settings(ui, c, l),
            SettingsPage::Hardware => {
                section(ui, c, "", "", |ui| {
                    if ui
                        .add_enabled(
                            enabled,
                            egui::Button::image_and_text(
                                Icon::Monitor.image(c.accent),
                                l.text("Обновить диагностику"),
                            ),
                        )
                        .clicked()
                        && let Err(e) = self.start(Operation::Doctor, ui.ctx())
                    {
                        self.fail(e);
                    }
                    self.hardware_view(ui);
                });
            }
            SettingsPage::Updates => {
                section(ui, c, "", "", |ui| self.update_settings(ui));
            }
        }
    }

    fn context_settings(&mut self, ui: &mut egui::Ui, c: theme::Colors, l: Language) {
        let enabled = self.job.is_none();
        ui.add_enabled_ui(enabled, |ui| {
            section(ui, c, l.text("Проект"), "", |ui| {
                let name = self
                    .prefs
                    .repo
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| l.text("не выбран").to_owned());
                setting_row(ui, c, l.text("Папка проекта"), &name, |ui| {
                    if self.prefs.repo.is_some()
                        && kit::ghost_button(ui, c, None, l.text("Сбросить"), true).clicked()
                    {
                        if !self.chat.messages.is_empty() {
                            self.new_session();
                        }
                        self.prefs.repo = None;
                        self.prefs.files.clear();
                        self.capture_session();
                    }
                    if kit::primary_button(ui, c, Some(Icon::FolderPlus), l.text("Выбрать…"), true).clicked() {
                        self.choose_path(false);
                    }
                });
            });
            ui.add_space(10.0);
            section(
                ui,
                c,
                l.text("Файлы контекста"),
                l.text("Пусто — ограниченный индекс проекта. Секретные и служебные файлы исключаются движком."),
                |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.prefs.files)
                            .desired_rows(6)
                            .desired_width(f32::INFINITY)
                            .char_limit(32768)
                            .hint_text("src/lib.rs\nCargo.toml"),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{} / 32",
                            self.prefs.files.lines().filter(|s| !s.trim().is_empty()).count()
                        ))
                        .small()
                        .color(c.muted),
                    );
                },
            );
            ui.add_space(10.0);
            section(ui, c, l.text("Объём контекста"), "", |ui| {
                setting_row(
                    ui,
                    c,
                    l.text("История чата"),
                    l.text("Сокращается только история чата; исходники и ошибки не переписываются."),
                    |ui| {
                        egui::ComboBox::from_id_salt("history-window")
                            .selected_text(l.text(match self.prefs.optimization {
                                preferences::Optimization::Safe => "Полная история",
                                preferences::Optimization::Balanced => "Последние 12 сообщений",
                                preferences::Optimization::Aggressive => "Последние 6 сообщений",
                            }))
                            .show_ui(ui, |ui| {
                                for (value, label) in [
                                    (preferences::Optimization::Safe, "Полная история"),
                                    (preferences::Optimization::Balanced, "Последние 12 сообщений"),
                                    (preferences::Optimization::Aggressive, "Последние 6 сообщений"),
                                ] {
                                    ui.selectable_value(&mut self.prefs.optimization, value, l.text(label));
                                }
                            });
                    },
                );
                kit::divider(ui, c);
                setting_row(
                    ui,
                    c,
                    l.text("Дедупликация результатов инструментов"),
                    l.text("Повторные старые наблюдения передаются один раз со ссылками; последние, ошибки и патчи дословно."),
                    |ui| {
                        icons::toggle(ui, &mut self.prefs.dedup_tool_context, "", self.prefs.animations, c.accent, c.on_accent);
                    },
                );
            });
        });
    }

    fn agent_settings(&mut self, ui: &mut egui::Ui, c: theme::Colors, l: Language) {
        ui.add_enabled_ui(self.job.is_none(), |ui| {
            section(ui, c, l.text("Роли"), "", |ui| {
                setting_row(
                    ui,
                    c,
                    l.text("Исследователь и ревьюер"),
                    l.text("Роли работают последовательно с одной моделью. Исследователь и ревьюер не изменяют файлы; дополнительные проверки требуют времени."),
                    |ui| {
                        icons::toggle(ui, &mut self.prefs.subagents, "", self.prefs.animations, c.accent, c.on_accent);
                    },
                );
                if self.prefs.subagents {
                    self.prefs.calls = self.prefs.calls.max(3);
                }
                kit::divider(ui, c);
                let min = if self.prefs.subagents { 3 } else { 1 };
                setting_row(
                    ui,
                    c,
                    l.text("Общий лимит вызовов"),
                    l.text("Бюджет общий для всех ролей."),
                    |ui| {
                        ui.add(egui::Slider::new(&mut self.prefs.calls, min..=256).logarithmic(false));
                    },
                );
            });
            ui.add_space(10.0);
            section(ui, c, l.text("Контекст агентов"), "", |ui| {
                setting_row(
                    ui,
                    c,
                    l.text("Граф проекта в контексте агентов"),
                    l.text("Добавляет ~3 КБ структуры (имена файлов, без кода) в запросы; для малых контекстов GGUF увеличьте контекст."),
                    |ui| {
                        icons::toggle(ui, &mut self.prefs.code_graph, "", self.prefs.animations, c.accent, c.on_accent);
                    },
                );
            });
        });
    }

    fn github_settings(&mut self, ui: &mut egui::Ui, c: theme::Colors, l: Language) {
        #[cfg(feature = "gateway-http")]
        {
            section(ui, c, l.text("Репозиторий"), "", |ui| {
                setting_row(ui, c, l.text("Репозиторий"), "owner/name", |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.prefs.github_repo)
                            .hint_text("owner/name")
                            .desired_width(220.0),
                    );
                });
                kit::divider(ui, c);
                setting_row(
                    ui,
                    c,
                    l.text("Переменная токена"),
                    l.text("Токен читается из переменной окружения и нигде не сохраняется."),
                    |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.prefs.github_token_env)
                                .desired_width(220.0),
                        );
                    },
                );
            });
            ui.add_space(10.0);
            section(ui, c, l.text("Issue как задача"), "", |ui| {
                setting_row(ui, c, l.text("Номер issue"), "", |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.prefs.github_issue)
                            .hint_text("123")
                            .desired_width(90.0),
                    );
                    let ready = self.github_rx.is_none()
                        && !self.prefs.github_repo.trim().is_empty()
                        && !self.prefs.github_issue.trim().is_empty();
                    if kit::primary_button(ui, c, None, l.text("Загрузить как задачу"), ready)
                        .clicked()
                    {
                        self.github_fetch_issue();
                        self.show_work();
                    }
                });
                ui.label(
                    RichText::new(l.text(
                        "Публикация PR — в панели результата: вкладка «Изменения» → «Черновик PR».",
                    ))
                    .small()
                    .color(c.muted),
                );
            });
        }
        #[cfg(not(feature = "gateway-http"))]
        section(ui, c, "", "", |ui| {
            ui.label(l.text("GitHub-коннектор недоступен в этой сборке."));
        });
    }

    #[cfg(feature = "gateway-http")]
    fn providers_settings(&mut self, ui: &mut egui::Ui, c: theme::Colors, l: Language) {
        ui.add_enabled_ui(self.job.is_none(), |ui| {
            section(
                ui,
                c,
                l.text("Реестр провайдеров"),
                l.text("Сохраняется только путь. API-ключи должны находиться в переменных окружения."),
                |ui| {
                    let mut path = self
                        .prefs
                        .provider_registry
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default();
                    ui.horizontal(|ui| {
                        if ui
                            .add(egui::TextEdit::singleline(&mut path).desired_width(ui.available_width() - 130.0).hint_text("C:\\providers.json"))
                            .changed()
                        {
                            self.prefs.provider_registry = if path.trim().is_empty() { None } else { Some(PathBuf::from(path.trim())) };
                        }
                        if kit::ghost_button(ui, c, Some(Icon::Check), l.text("Проверить"), true).clicked() {
                            let result = (|| -> io::Result<usize> {
                                use std::io::Read;
                                let path = self.prefs.provider_registry.as_ref().ok_or_else(|| io::Error::other("Укажите путь к JSON."))?;
                                let mut bytes = Vec::new();
                                std::fs::File::open(path)?.take(64 * 1024 + 1).read_to_end(&mut bytes)?;
                                let configs = crate::provider_http::parse_registry(&bytes)?;
                                for config in &configs {
                                    crate::provider_http::OpenAiCompatibleBackend::new(config.clone())?;
                                }
                                Ok(configs.len())
                            })();
                            match result {
                                Ok(count) => {
                                    self.message = format!("Реестр проверен: {count} провайдеров.");
                                    self.error = false;
                                }
                                Err(error) => self.fail(error),
                            }
                        }
                    });
                },
            );
            ui.add_space(10.0);
            section(ui, c, l.text("Где разрешены облачные запросы"), "", |ui| {
                setting_row(ui, c, l.text("Разрешить HTTP-запросы из чата"), "", |ui| {
                    icons::toggle(ui, &mut self.prefs.cloud_chat, "", self.prefs.animations, c.accent, c.on_accent);
                });
                kit::divider(ui, c);
                setting_row(
                    ui,
                    c,
                    l.text("Облачная модель для задач проекта"),
                    l.text("При запуске задачи фрагменты исходного кода и задание отправляются выбранному провайдеру."),
                    |ui| {
                        icons::toggle(ui, &mut self.prefs.cloud_task, "", self.prefs.animations, c.accent, c.on_accent);
                    },
                );
                if let Some(path) = &self.prefs.provider_registry {
                    let available = (|| -> io::Result<Vec<crate::provider_http::ProviderConfig>> {
                        use std::io::Read;
                        let mut bytes = Vec::new();
                        std::fs::File::open(path)?.take(64 * 1024 + 1).read_to_end(&mut bytes)?;
                        crate::provider_http::parse_registry(&bytes)
                    })();
                    kit::divider(ui, c);
                    match available {
                        Ok(configs) => setting_row(ui, c, l.text("Модель"), "", |ui| {
                            let selected = format!("{} / {}", self.prefs.cloud_provider, self.prefs.cloud_model);
                            egui::ComboBox::from_id_salt("cloud-model").selected_text(selected).show_ui(ui, |ui| {
                                for config in &configs {
                                    let (provider, model) = (&config.descriptor.provider, &config.descriptor.model);
                                    if ui
                                        .selectable_label(
                                            self.prefs.cloud_provider == *provider && self.prefs.cloud_model == *model,
                                            format!("{provider} / {model}"),
                                        )
                                        .clicked()
                                    {
                                        self.prefs.cloud_provider = provider.clone();
                                        self.prefs.cloud_model = model.clone();
                                    }
                                }
                            });
                        }),
                        Err(_) => {
                            ui.label(RichText::new(l.text("Не удалось загрузить реестр провайдеров.")).color(c.error));
                        }
                    }
                }
            });
        });
    }

    fn appearance_settings(&mut self, ui: &mut egui::Ui, c: theme::Colors, l: Language) {
        section(ui, c, l.text("Тема"), "", |ui| {
            ui.horizontal_wrapped(|ui| {
                for palette in Palette::ALL {
                    let p = palette.colors();
                    let selected = self.prefs.palette == palette;
                    let response = kit::tile(ui, c, selected, egui::vec2(150.0, 92.0), |ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(126.0, 40.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 6, p.background);
                        ui.painter().rect_filled(
                            egui::Rect::from_min_size(rect.min, egui::vec2(18.0, 40.0)),
                            6,
                            p.surface,
                        );
                        ui.painter().rect_filled(
                            egui::Rect::from_min_size(
                                rect.min + egui::vec2(26.0, 8.0),
                                egui::vec2(60.0, 8.0),
                            ),
                            3,
                            p.card,
                        );
                        ui.painter().rect_filled(
                            egui::Rect::from_min_size(
                                rect.min + egui::vec2(26.0, 22.0),
                                egui::vec2(34.0, 10.0),
                            ),
                            4,
                            p.accent,
                        );
                        ui.label(RichText::new(l.text(palette.name())).strong());
                    });
                    if response.clicked() {
                        self.prefs.palette = palette;
                    }
                }
            });
        });
        ui.add_space(10.0);
        section(ui, c, l.text("Язык и движение"), "", |ui| {
            setting_row(ui, c, l.text("Язык"), "", |ui| {
                kit::segmented(
                    ui,
                    c,
                    &mut self.prefs.language,
                    &[(Language::Ru, "Русский"), (Language::En, "English")],
                );
            });
            kit::divider(ui, c);
            setting_row(
                ui,
                c,
                l.text("Мягкие анимации"),
                l.text(
                    "Короткие переходы и отклик переключателей. В покое нет постоянных анимаций.",
                ),
                |ui| {
                    let animate = self.prefs.animations;
                    icons::toggle(
                        ui,
                        &mut self.prefs.animations,
                        "",
                        animate,
                        c.accent,
                        c.on_accent,
                    );
                },
            );
        });
        ui.add_space(10.0);
        section(ui, c, l.text("Данные"), "", |ui| {
            ui.label(
                RichText::new(l.text("Настройки и история чатов хранятся локально. Отчёты предложений и технический журнал автоматически не сохраняются."))
                    .small()
                    .color(c.muted),
            );
            if let Some(path) = &self.storage {
                ui.add(
                    egui::Label::new(
                        RichText::new(path.display().to_string())
                            .small()
                            .monospace(),
                    )
                    .truncate(),
                )
                .on_hover_text(path.display().to_string());
            }
        });
    }
}
