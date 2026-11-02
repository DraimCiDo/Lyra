//! Explicit local catalog selection, shared by local and HTTP workers.
use super::*;

#[derive(Default)]
pub(super) struct PackageUi {
    id: String,
    version: String,
    origin: String,
    installed: Option<crate::skill_package::Store>,
}

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
            self.skill_package_ui.installed = None;
        }
        if ui.button(l.text("Загрузить каталог навыков")).clicked() {
            self.skill_package_ui.installed = None;
            self.skill_catalog = Some(
                self.prefs
                    .skill_manifest
                    .as_ref()
                    .ok_or_else(|| "Укажите файл или каталог ECC Skills.".to_owned())
                    .and_then(|path| {
                        if crate::skill_package::is_store(path) {
                            let store = crate::skill_package::Store::load(path)
                                .map_err(|error| error.to_string())?;
                            let pack = store.current.pack.clone();
                            self.skill_package_ui.installed = Some(store);
                            return Ok(pack);
                        }
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
        self.package_settings(ui);
    }

    fn package_settings(&mut self, ui: &mut egui::Ui) {
        let l = self.prefs.language;
        egui::CollapsingHeader::new(l.text("Пакет навыков · установка и версии")).show(ui, |ui| {
            ui.label(RichText::new(l.text("Локальный снимок выбранных навыков. Версия и источник указываются вами; SHA256 не является подписью автора.")).small().weak());
            ui.label(l.text("ID пакета (a-z, 0-9, дефис)"));
            ui.text_edit_singleline(&mut self.skill_package_ui.id);
            ui.label(l.text("Версия пакета"));
            ui.text_edit_singleline(&mut self.skill_package_ui.version);
            ui.label(l.text("Источник / ревизия"));
            ui.text_edit_singleline(&mut self.skill_package_ui.origin);
            if ui.button(l.text("Установить выбранные навыки и использовать снимок")).clicked() {
                let result = (|| -> io::Result<(PathBuf, crate::skill_package::Store)> {
                    let id = &self.skill_package_ui.id;
                    if !crate::skill_package::valid_id(id) { return Err(io::Error::other("Invalid package id")); }
                    let parent = self.storage.as_ref().and_then(|path| path.parent())
                        .ok_or_else(|| io::Error::other("Local settings directory unavailable"))?;
                    let target = parent.join("skills").join(format!("{id}.lyra-skills.json"));
                    let source = self.prefs.skill_manifest.as_ref()
                        .ok_or_else(|| io::Error::other("Select a skill source first"))?;
                    let store = crate::skill_package::install(source, &target, id,
                        &self.skill_package_ui.version, &self.skill_package_ui.origin, &self.prefs.selected_skills())?;
                    Ok((target, store))
                })();
                match result {
                    Ok((target, store)) => {
                        self.prefs.skill_manifest = Some(target);
                        self.skill_catalog = Some(Ok(store.current.pack.clone()));
                        self.skill_package_ui.installed = Some(store);
                        self.error = false;
                        self.message = l.text("Пакет установлен локально.").into();
                    }
                    Err(error) => self.fail(error),
                }
            }
            if let Some(store) = &self.skill_package_ui.installed {
                let metadata = &store.current.metadata;
                ui.label(format!("{} · {}", metadata.id, metadata.version));
                ui.label(format!("{}: {}", l.text("Источник / ревизия"), metadata.origin));
                ui.label(format!("SHA256: {}", metadata.sha256));
                if let Some(previous) = &store.previous {
                    ui.label(format!("{}: {}", l.text("Предыдущая версия"), previous.metadata.version));
                    if ui.button(l.text("Переключить на предыдущую версию")).clicked() {
                        let result = self.prefs.skill_manifest.as_ref()
                            .ok_or_else(|| io::Error::other("Select an installed package first"))
                            .and_then(|path| crate::skill_package::rollback(path));
                        match result {
                            Ok(store) => {
                                self.skill_catalog = Some(Ok(store.current.pack.clone()));
                                self.skill_package_ui.installed = Some(store);
                                self.error = false;
                                self.message = l.text("Версия пакета переключена. Проверьте выбранные навыки.").into();
                            }
                            Err(error) => self.fail(error),
                        }
                    }
                }
            }
        });
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
