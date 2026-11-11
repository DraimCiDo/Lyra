//! Human-readable local model settings; advanced tuning remains explicit.
use super::*;

impl Desktop {
    pub(super) fn model_settings(&mut self, ui: &mut egui::Ui) {
        use super::kit::{self, section, setting_row};
        let l = self.prefs.language;
        let c = self.prefs.palette.colors();
        let enabled = self.job.is_none();
        ui.add_enabled_ui(enabled, |ui| {
            let file = self
                .prefs
                .model
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| l.text("не выбрана").into());
            let full = self
                .prefs
                .model
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            section(ui, c, l.text("Модель"), "", |ui| {
                setting_row(ui, c, l.text("Файл GGUF"), &file, |ui| {
                    if kit::primary_button(ui, c, Some(Icon::Model), l.text("Выбрать…"), true)
                        .on_hover_text(&full)
                        .clicked()
                    {
                        self.choose_path(true);
                    }
                });
                kit::divider(ui, c);
                ui.label(RichText::new(l.text("Нет модели? Подобрать и установить GGUF")).strong());
                self.model_suggestion(ui);
            });
            ui.add_space(10.0);
            section(ui, c, l.text("Устройство"), "", |ui| {
                let mode = &mut self.prefs.gpu_layers;
                let description = if !cfg!(feature = "cuda") {
                    "Это CPU-сборка. Для NVIDIA GPU скачайте отдельный пакет CUDA; веса GGUF можно использовать те же."
                } else if *mode == 0 {
                    "Выбран CPU. Для ускорения на NVIDIA выберите GPU Авто; существующий выбор не меняется автоматически."
                } else if *mode == crate::resources::GPU_AUTO {
                    "GPU Авто оценивает доступную VRAM и переносит подходящее число слоёв на NVIDIA GPU. Остальная часть использует CPU и RAM."
                } else {
                    "Выбрано ручное или сбалансированное распределение. Настройка слоёв находится в разделе «Дополнительно»."
                };
                setting_row(ui, c, l.text("Где выполнять локальную модель"), l.text(description), |ui| {
                    ui.vertical(|ui| {
                        ui.add_enabled_ui(cfg!(feature = "cuda"), |ui| {
                            ui.selectable_value(mode, crate::resources::GPU_AUTO, l.text("NVIDIA GPU · Авто (рекомендуется)"));
                        });
                        ui.selectable_value(mode, 0, l.text("Только CPU"));
                    });
                });
                if !cfg!(feature = "cuda") && *mode > 0 {
                    ui.colored_label(c.error, l.text("В настройках сохранён GPU-режим. Выберите Только CPU или запустите CUDA-сборку."));
                }
            });
            ui.add_space(10.0);
            section(ui, c, l.text("Дополнительно · память, длина ответа и время"), l.text("Начните с контекста 4096. Большое окно требует больше RAM/VRAM; длинный ответ и загрузка весов требуют времени."), |ui| {
                setting_row(ui, c, l.text("Контекст"), "", |ui| {
                    egui::ComboBox::from_id_salt("context").selected_text(self.prefs.context.to_string()).show_ui(ui, |ui| {
                        for size in [1024, 2048, 4096, 8192, 16384, 32768] {
                            ui.selectable_value(&mut self.prefs.context, size, size.to_string());
                        }
                    });
                });
                kit::divider(ui, c);
                setting_row(ui, c, l.text("Токенов на ответ"), l.text("Максимум токенов одного действия модели."), |ui| {
                    ui.add(egui::Slider::new(&mut self.prefs.tokens, 1..=8192).logarithmic(true));
                });
                kit::divider(ui, c);
                setting_row(ui, c, l.text("CPU-потоки"), l.text("Больше потоков не всегда быстрее. Начальное значение учитывает доступные CPU, максимум 8."), |ui| {
                    ui.add(egui::Slider::new(&mut self.prefs.threads, 1..=256).logarithmic(true));
                });
                kit::divider(ui, c);
                setting_row(ui, c, l.text("Таймаут, с"), l.text("Таймаут включает загрузку модели и все шаги задачи."), |ui| {
                    ui.add(egui::Slider::new(&mut self.prefs.timeout, 1..=3600).logarithmic(true));
                });
                if cfg!(feature = "cuda") {
                    kit::divider(ui, c);
                    setting_row(ui, c, l.text("Ручное распределение GPU / CPU"), l.text("Сбалансированный режим оставляет часть слоёв на CPU и не обязательно быстрее Авто."), |ui| {
                        let mode = &mut self.prefs.gpu_layers;
                        ui.vertical(|ui| {
                            ui.selectable_value(mode, crate::resources::GPU_BALANCED, l.text("Сбалансированный"));
                            if ui.selectable_label((1..crate::resources::GPU_BALANCED).contains(mode), l.text("Ручная настройка")).clicked() {
                                *mode = 16;
                            }
                            if (1..crate::resources::GPU_BALANCED).contains(mode) {
                                ui.add(egui::Slider::new(mode, 1..=256).text(l.text("Слоёв на CUDA")));
                            }
                        });
                    });
                }
                kit::divider(ui, c);
                if kit::ghost_button(ui, c, None, l.text("Сбросить числовые параметры"), true).clicked() {
                    self.prefs.reset_inference();
                }
            });
        });
    }
}
