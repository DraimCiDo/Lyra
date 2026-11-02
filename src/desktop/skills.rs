//! Explicit local catalog selection, shared by local and HTTP workers.
use super::*;

impl Desktop {
    pub(super) fn skill_args(&self) -> io::Result<Vec<OsString>> {
        let selected = self.prefs.selected_skills();
        if selected.is_empty() {
            return Ok(Vec::new());
        }
        let path = self
            .prefs
            .skill_manifest
            .as_ref()
            .ok_or_else(|| io::Error::other("Укажите файл или каталог ECC Skills."))?;
        let pack = crate::skills::SkillPack::load_source(path)?;
        crate::agent_runtime::skill_instructions(Some(&pack), &selected)?;
        let mut args = vec![OsString::from("--skills"), path.as_os_str().into()];
        for id in selected {
            args.extend([OsString::from("--skill"), id.into()]);
        }
        Ok(args)
    }

    pub(super) fn skill_settings(&mut self, ui: &mut egui::Ui) {
        let l = self.prefs.language;
        ui.label(l.text("ECC Skills · файл или каталог"));
        let mut path = self
            .prefs
            .skill_manifest
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        if ui
            .add(
                egui::TextEdit::singleline(&mut path)
                    .desired_width(f32::INFINITY)
                    .hint_text("skills/ · skills.json · SKILL.md"),
            )
            .changed()
        {
            self.prefs.skill_manifest =
                (!path.trim().is_empty()).then(|| PathBuf::from(path.trim()));
            self.skill_catalog = None;
        }
        if ui.button(l.text("Загрузить каталог навыков")).clicked() {
            self.skill_catalog = Some(
                self.prefs
                    .skill_manifest
                    .as_ref()
                    .ok_or_else(|| "Укажите файл или каталог ECC Skills.".to_owned())
                    .and_then(|path| {
                        crate::skills::SkillPack::load_source(path)
                            .map_err(|error| error.to_string())
                    }),
            );
        }
        ui.label(RichText::new(l.text("Навыки — инструкции для модели. Скрипты и hooks не исполняются. Выбранные навыки применяются в порядке выбора.")).small().weak());
        let mut selected = self.prefs.selected_skills();
        ui.label(format!("{}: {}", l.text("Выбрано"), selected.join(", ")));
        if ui.button(l.text("Снять выбор навыков")).clicked() {
            selected.clear();
        }
        if let Some(catalog) = &self.skill_catalog {
            match catalog {
                Ok(pack) => {
                    for skill in &pack.skills {
                        let mut enabled = selected.contains(&skill.id);
                        if ui
                            .checkbox(&mut enabled, &skill.id)
                            .on_hover_text(&skill.description)
                            .changed()
                        {
                            if enabled && selected.len() < 32 {
                                selected.push(skill.id.clone());
                            } else if !enabled {
                                selected.retain(|id| id != &skill.id);
                            }
                        }
                        if enabled {
                            egui::CollapsingHeader::new(format!(
                                "{} · {}",
                                l.text("Инструкции"),
                                skill.id
                            ))
                            .show(ui, |ui| {
                                ui.label(&skill.instructions);
                            });
                        }
                    }
                    if selected.iter().any(|id| pack.find(id).is_none()) {
                        ui.colored_label(Color32::LIGHT_RED, l.text("Выбранный навык отсутствует в каталоге. Снимите выбор или верните источник."));
                    }
                }
                Err(error) => {
                    ui.colored_label(Color32::LIGHT_RED, error);
                }
            }
        }
        self.prefs.active_skill.clear();
        self.prefs.active_skills = selected;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_multi_selection_matches_cli_and_missing_skill_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("skills.json");
        std::fs::write(&path, r#"{"schema_version":1,"skills":[{"id":"review","description":"review","instructions":"Check tests"},{"id":"rust","description":"rust","instructions":"Check ownership"}]}"#).unwrap();
        let mut app = Desktop::default();
        app.prefs.skill_manifest = Some(path.clone());
        app.prefs.active_skills = vec!["rust".into(), "review".into()];
        assert_eq!(
            app.skill_args().unwrap(),
            vec![
                OsString::from("--skills"),
                path.into_os_string(),
                "--skill".into(),
                "rust".into(),
                "--skill".into(),
                "review".into(),
            ]
        );
        app.prefs.active_skills.push("missing".into());
        assert!(app.skill_args().is_err());
        app.prefs.active_skills.clear();
        app.prefs.active_skill = "review".into();
        assert_eq!(
            app.skill_args().unwrap().last(),
            Some(&OsString::from("review"))
        );
    }
}
