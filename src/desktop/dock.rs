//! Right-hand result dock: proposals with a coloured diff, verification evidence, plan and log.
use super::{
    diff::{self, Tag},
    kit, *,
};

struct Row {
    path: String,
    added: usize,
    removed: usize,
    create: bool,
    binary: bool,
}

impl Desktop {
    pub(super) fn dock_visible(&self) -> bool {
        self.report.is_some()
    }

    fn proposal_rows(&self) -> Vec<Row> {
        let Some(report) = &self.report else {
            return Vec::new();
        };
        let mut rows: Vec<Row> = report
            .proposals
            .iter()
            .map(|e| {
                let d = diff::lines(&e.original, &e.replacement);
                let (added, removed) = diff::stats(&d);
                Row {
                    path: e.path.clone(),
                    added,
                    removed,
                    create: e.create,
                    binary: false,
                }
            })
            .collect();
        rows.extend(report.file_proposals.iter().map(|f| Row {
            path: f.path.clone(),
            added: 0,
            removed: 0,
            create: f.original.is_none(),
            binary: true,
        }));
        rows
    }

    pub(super) fn dock(&mut self, ui: &mut egui::Ui, c: theme::Colors) {
        let l = self.prefs.language;
        let Some(report) = &self.report else { return };
        let (status, outcome_ok, counts) = (
            report.status.clone(),
            report.status == "completed",
            [
                report.proposals.len() + report.file_proposals.len(),
                report.verifications.len() + report.executions.len(),
                report.work_plan.iter().len() + report.summaries.len(),
                report.events.len(),
            ],
        );
        ui.horizontal(|ui| {
            ui.label(RichText::new(l.text("Результат")).strong().size(16.0));
            kit::badge(ui, &status, if outcome_ok { c.success } else { c.warning });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(egui::Button::new("×").frame(false))
                    .on_hover_text("Ctrl+J")
                    .clicked()
                {
                    self.dock_open = false;
                }
            });
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            for (tab, label, count) in [
                (DockTab::Changes, "Изменения", counts[0]),
                (DockTab::Checks, "Проверки", counts[1]),
                (DockTab::Plan, "План и итоги", counts[2]),
                (DockTab::Log, "Журнал", counts[3]),
            ] {
                let text = if count > 0 {
                    format!("{} {count}", l.text(label))
                } else {
                    l.text(label).to_owned()
                };
                if kit::chip(ui, c, None, &text, self.dock_tab == tab, None).clicked() {
                    self.dock_tab = tab;
                }
            }
        });
        ui.add_space(6.0);
        match self.dock_tab {
            DockTab::Changes => self.dock_changes(ui, c),
            DockTab::Checks => self.dock_checks(ui, c),
            DockTab::Plan => self.dock_plan(ui, c),
            DockTab::Log => self.dock_log(ui, c),
        }
    }

    fn dock_changes(&mut self, ui: &mut egui::Ui, c: theme::Colors) {
        let l = self.prefs.language;
        let rows = self.proposal_rows();
        let Some(report) = &self.report else { return };
        if rows.is_empty() {
            ui.label(RichText::new(l.text(
                "Применимых предложений нет. Завершение протокола не доказывает решения задачи.",
            ))
            .color(c.muted));
            return;
        }
        let can_one = self.job.is_none() && can_apply(report, self.selected, &self.applied);
        let can_all = self.job.is_none() && workflows::can_apply_all(report, &self.applied);
        #[cfg(feature = "gateway-http")]
        let can_pr = self.github_rx.is_none()
            && !self.prefs.github_repo.trim().is_empty()
            && report.status == "completed"
            && !report.proposals.is_empty();
        let verified = report.verifications.last().map(|v| v.result.passed());
        ui.horizontal_wrapped(|ui| {
            if kit::primary_button(ui, c, Some(Icon::Check), l.text("Применить"), can_one).clicked()
            {
                self.confirm = Some(Operation::Apply(self.selected));
            }
            if kit::ghost_button(ui, c, None, l.text("Весь набор"), can_all).clicked() {
                self.confirm = Some(Operation::ApplyAll);
            }
            #[cfg(feature = "gateway-http")]
            if kit::ghost_button(ui, c, None, l.text("Черновик PR"), can_pr).clicked() {
                self.publish_flag.set(true);
            }
            if kit::ghost_button(ui, c, Some(Icon::Save), l.text("JSON"), true).clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .set_file_name("lyra-report.json")
                    .add_filter("JSON", &["json"])
                    .save_file()
                && let Err(e) = std::fs::write(path, &self.report_json)
            {
                self.message = e.to_string();
                self.error = true;
            }
        });
        match verified {
            Some(true) => kit::badge(ui, l.text("Проверка пройдена"), c.success),
            Some(false) => kit::badge(ui, l.text("Проверка не пройдена"), c.error),
            None => kit::badge(ui, l.text("Проверка не запускалась"), c.muted),
        }
        ui.add_space(6.0);
        egui::ScrollArea::vertical()
            .id_salt("dock-rows")
            .max_height(132.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for (index, row) in rows.iter().enumerate() {
                    let secondary = if row.binary {
                        l.text("двоичный файл").to_owned()
                    } else {
                        format!(
                            "{}+{}  −{}",
                            if row.create {
                                format!("{} ", l.text("новый"))
                            } else {
                                String::new()
                            },
                            row.added,
                            row.removed
                        )
                    };
                    let title = if self.applied.contains(&index) {
                        format!("{} · {}", row.path, l.text("применено"))
                    } else {
                        row.path.clone()
                    };
                    if kit::list_row(ui, c, index == self.selected, &title, &secondary).clicked() {
                        self.selected = index;
                    }
                }
            });
        ui.add_space(6.0);
        if self.applied.contains(&self.selected) {
            ui.label(
                RichText::new(l.text("Применено. Запусти тесты проекта отдельно."))
                    .color(c.success),
            );
        }
        self.dock_selected(ui, c);
    }

    fn dock_selected(&mut self, ui: &mut egui::Ui, c: theme::Colors) {
        let l = self.prefs.language;
        let Some(report) = &self.report else { return };
        if let Some(edit) = report.proposals.get(self.selected).cloned() {
            let d = diff::lines(&edit.original, &edit.replacement);
            egui::ScrollArea::both()
                .id_salt("dock-diff")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Frame::new()
                        .fill(c.input)
                        .corner_radius(theme::RADIUS_SMALL)
                        .inner_margin(egui::Margin::symmetric(0, 6))
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            let mut number = (0usize, 0usize);
                            for (tag, line) in &d {
                                let (bg, mark, fg) = match tag {
                                    Tag::Same => (Color32::TRANSPARENT, ' ', c.muted),
                                    Tag::Added => (c.success.gamma_multiply(0.16), '+', c.text),
                                    Tag::Removed => (c.error.gamma_multiply(0.16), '−', c.text),
                                };
                                match tag {
                                    Tag::Same => {
                                        number.0 += 1;
                                        number.1 += 1;
                                    }
                                    Tag::Added => number.1 += 1,
                                    Tag::Removed => number.0 += 1,
                                }
                                let shown = if *tag == Tag::Added {
                                    number.1
                                } else {
                                    number.0
                                };
                                egui::Frame::new().fill(bg).show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(format!(
                                                "{shown:>4} {mark} {}",
                                                line.trim_end_matches(['\r', '\n'])
                                            ))
                                            .monospace()
                                            .color(fg),
                                        )
                                        .selectable(true)
                                        .extend(),
                                    );
                                });
                            }
                        });
                });
            return;
        }
        let Some(file) = self
            .selected
            .checked_sub(report.proposals.len())
            .and_then(|i| report.file_proposals.get(i))
            .cloned()
        else {
            return;
        };
        let root = report.artifact_store.clone();
        ui.label(format!(
            "{} · {} bytes · SHA256 {}",
            file.path, file.replacement.bytes, file.replacement.sha256
        ));
        ui.label(
            RichText::new(l.text("Содержимое и внешний вид требуют просмотра перед применением."))
                .small()
                .color(c.warning),
        );
        let Some(root) = root else { return };
        if kit::ghost_button(
            ui,
            c,
            Some(Icon::Save),
            l.text("Сохранить копию для просмотра…"),
            true,
        )
        .clicked()
            && let Some(destination) = rfd::FileDialog::new()
                .set_file_name(
                    std::path::Path::new(&file.path)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy(),
                )
                .save_file()
            && let Err(e) = crate::universal::artifacts::Store::open(&root)
                .and_then(|s| s.get(&file.replacement))
                .and_then(|b| std::fs::write(destination, b))
        {
            self.message = e.to_string();
            self.error = true;
        }
        if self.failed_artifact_preview.as_ref() != Some(&file.replacement.sha256)
            && file.path.to_lowercase().ends_with(".png")
            && self
                .artifact_preview
                .as_ref()
                .is_none_or(|(sha, _)| sha != &file.replacement.sha256)
        {
            let decoded = (|| -> io::Result<egui::ColorImage> {
                let bytes =
                    crate::universal::artifacts::Store::open(&root)?.get(&file.replacement)?;
                let mut reader = image::ImageReader::with_format(
                    std::io::Cursor::new(bytes),
                    image::ImageFormat::Png,
                );
                let mut limits = image::Limits::default();
                limits.max_image_width = Some(2048);
                limits.max_image_height = Some(2048);
                limits.max_alloc = Some(32 * 1024 * 1024);
                reader.limits(limits);
                let image = reader.decode().map_err(io::Error::other)?.into_rgba8();
                Ok(egui::ColorImage::from_rgba_unmultiplied(
                    [image.width() as usize, image.height() as usize],
                    image.as_raw(),
                ))
            })();
            match decoded {
                Err(_) => self.failed_artifact_preview = Some(file.replacement.sha256.clone()),
                Ok(image) => {
                    self.artifact_preview = Some((
                        file.replacement.sha256.clone(),
                        ui.ctx().load_texture(
                            "document-preview",
                            image,
                            egui::TextureOptions::LINEAR,
                        ),
                    ));
                }
            }
        }
        if let Some((sha, texture)) = &self.artifact_preview
            && sha == &file.replacement.sha256
        {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add(
                    egui::Image::new(texture)
                        .max_width(ui.available_width())
                        .max_height(520.0),
                );
            });
        }
    }

    fn dock_checks(&mut self, ui: &mut egui::Ui, c: theme::Colors) {
        let l = self.prefs.language;
        let Some(report) = &self.report else { return };
        if report.verifications.is_empty() && report.executions.is_empty() {
            ui.label(
                RichText::new(l.text(
                    "Проверки не запускались. Настройте Docker-проверку в «Проверки и среды».",
                ))
                .color(c.muted),
            );
            return;
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (index, v) in report.verifications.iter().enumerate() {
                    kit::card(c).inner_margin(12).show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("{} {}", l.text("Проверка"), index + 1))
                                    .strong(),
                            );
                            if v.result.passed() {
                                kit::badge(ui, l.text("пройдена"), c.success);
                            } else {
                                kit::badge(ui, l.text("не пройдена"), c.error);
                            }
                            ui.label(
                                RichText::new(format!(
                                    "exit {:?} · {} ms{}",
                                    v.result.exit_code,
                                    v.result.elapsed_ms,
                                    if v.result.timed_out {
                                        " · timeout"
                                    } else {
                                        ""
                                    }
                                ))
                                .small()
                                .color(c.muted),
                            );
                        });
                        for (name, text) in
                            [("stdout", &v.result.stdout), ("stderr", &v.result.stderr)]
                        {
                            if !text.trim().is_empty() {
                                ui.label(RichText::new(name).small().color(c.muted));
                                selectable_code(ui, text, c.text);
                            }
                        }
                    });
                }
                for run in &report.executions {
                    kit::card(c).inner_margin(12).show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.label(
                            RichText::new(format!(
                                "{} · {}",
                                l.text("Среда выполнения"),
                                run.environment
                            ))
                            .strong(),
                        )
                        .on_hover_text(&run.image);
                        ui.label(
                            RichText::new(format!("exit {:?}", run.exit_code))
                                .small()
                                .color(c.muted),
                        );
                        for (path, sha) in &run.outputs {
                            ui.label(
                                RichText::new(format!(
                                    "{path} · {}",
                                    sha.chars().take(12).collect::<String>()
                                ))
                                .small(),
                            );
                        }
                    });
                }
            });
    }

    fn dock_plan(&mut self, ui: &mut egui::Ui, c: theme::Colors) {
        let l = self.prefs.language;
        let Some(report) = &self.report else { return };
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if let Some(plan) = &report.work_plan {
                    ui.label(RichText::new(l.text("Выбранные технологии и план")).strong());
                    ui.label(&plan.goal);
                    for t in &plan.technologies {
                        ui.label(RichText::new(format!("{} · {}", t.name, t.purpose)).strong());
                        ui.label(RichText::new(&t.rationale).small().color(c.muted));
                    }
                    for (i, step) in plan.steps.iter().enumerate() {
                        ui.label(format!("{}. {step}", i + 1));
                    }
                    if !plan.required_capabilities.is_empty() {
                        ui.label(format!(
                            "{}: {}",
                            l.text("Необходимые возможности"),
                            plan.required_capabilities.join(", ")
                        ));
                    }
                    kit::divider(ui, c);
                }
                for (role, summary) in &report.summaries {
                    ui.label(RichText::new(format!("{}: {role}", l.text("Итог"))).strong());
                    ui.add(egui::Label::new(summary).selectable(true).wrap());
                    ui.add_space(6.0);
                }
                if !report.documentation_sources.is_empty() {
                    kit::divider(ui, c);
                    ui.label(RichText::new(l.text("Документация в контексте")).strong());
                    for s in &report.documentation_sources {
                        ui.label(
                            RichText::new(format!("{} · {}", s.title, s.version_scope)).small(),
                        )
                        .on_hover_text(&s.origin);
                    }
                }
                ui.add_space(6.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(format!(
                            "{}: {}",
                            l.text("Проект отчёта"),
                            report.workspace.display()
                        ))
                        .small()
                        .color(c.muted),
                    )
                    .truncate(),
                );
            });
    }

    fn dock_log(&mut self, ui: &mut egui::Ui, c: theme::Colors) {
        let Some(report) = &self.report else { return };
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for event in &report.events {
                    ui.label(
                        RichText::new(format!(
                            "{} · {} · {} · {}",
                            event.call, event.role, event.tool, event.outcome
                        ))
                        .monospace()
                        .size(12.0)
                        .color(
                            if event.outcome.starts_with("denied")
                                || event.outcome.contains("error")
                            {
                                c.error
                            } else {
                                c.text
                            },
                        ),
                    );
                }
                if !self.log.is_empty() {
                    kit::divider(ui, c);
                    selectable_code(ui, &self.log, c.muted);
                }
            });
    }
}
