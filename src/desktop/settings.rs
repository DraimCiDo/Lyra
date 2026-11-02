//! Human-readable local model settings; advanced tuning remains explicit.
use super::*;

impl Desktop {
    pub(super) fn model_settings(&mut self, ui: &mut egui::Ui) {
        let l = self.prefs.language;
        let c = self.prefs.palette.colors();
        let title = self
            .prefs
            .model
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| l.text("Выбрать…").into());
        if ui
            .add(
                egui::Button::image_and_text(
                    Icon::Model.image(c.accent),
                    format!("{} · {title}", l.text("Модель GGUF")),
                )
                .min_size(egui::vec2(ui.available_width(), 32.0))
                .wrap_mode(egui::TextWrapMode::Truncate),
            )
            .on_hover_text(
                self.prefs
                    .model
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
            )
            .clicked()
        {
            self.choose_path(true);
        }
        ui.add_space(8.0);
        ui.label(RichText::new(l.text("Где выполнять локальную модель")).strong());
        let mode = &mut self.prefs.gpu_layers;
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(cfg!(feature = "cuda"), |ui| {
                ui.selectable_value(
                    mode,
                    crate::resources::GPU_AUTO,
                    l.text("NVIDIA GPU · Авто (рекомендуется)"),
                );
            });
            ui.selectable_value(mode, 0, l.text("Только CPU"));
        });
        let description = if !cfg!(feature = "cuda") {
            "Это CPU-сборка. Для NVIDIA GPU скачайте отдельный пакет CUDA; веса GGUF можно использовать те же."
        } else if *mode == 0 {
            "Выбран CPU. Для ускорения на NVIDIA выберите GPU Авто; существующий выбор не меняется автоматически."
        } else if *mode == crate::resources::GPU_AUTO {
            "GPU Авто оценивает доступную VRAM и переносит подходящее число слоёв на NVIDIA GPU. Остальная часть использует CPU и RAM."
        } else {
            "Выбрано ручное или сбалансированное распределение. Настройка слоёв находится в разделе Дополнительно."
        };
        ui.label(RichText::new(l.text(description)).small().weak());
        if !cfg!(feature = "cuda") && *mode > 0 {
            ui.colored_label(c.error, l.text("В настройках сохранён GPU-режим. Выберите Только CPU или запустите CUDA-сборку."));
        }
        ui.add_space(8.0);
        egui::CollapsingHeader::new(l.text("Нет модели? Подобрать и установить GGUF"))
            .show(ui, |ui| self.model_suggestion(ui));
        egui::CollapsingHeader::new(l.text("Дополнительно · память, длина ответа и время")).show(ui, |ui| {
            ui.label(l.text("Начните с контекста 4096. Большое окно требует больше RAM/VRAM; длинный ответ и загрузка весов требуют времени."));
            ui.horizontal_wrapped(|ui| {
                ui.label(l.text("Контекст"));
                egui::ComboBox::from_id_salt("context").selected_text(self.prefs.context.to_string()).show_ui(ui, |ui| {
                    for size in [1024,2048,4096,8192,16384,32768] { ui.selectable_value(&mut self.prefs.context,size,size.to_string()); }
                });
            });
            setting_slider(ui,l.text("Токенов на ответ"),&mut self.prefs.tokens,1..=8192,true,l.text("Максимум токенов одного действия модели."),c.muted);
            setting_slider(ui,l.text("CPU-потоки"),&mut self.prefs.threads,1..=256,true,l.text("Больше потоков не всегда быстрее. Начальное значение учитывает доступные CPU, максимум 8."),c.muted);
            ui.label(l.text("Таймаут, с"));
            ui.add(egui::Slider::new(&mut self.prefs.timeout,1..=3600).logarithmic(true));
            ui.label(RichText::new(l.text("Таймаут включает загрузку модели и все шаги задачи.")).small().weak());
            ui.add_enabled_ui(cfg!(feature="cuda"), |ui| {
                ui.label(l.text("Ручное распределение GPU / CPU"));
                ui.label(RichText::new(l.text("Сбалансированный режим оставляет часть слоёв на CPU и не обязательно быстрее Авто.")).small().weak());
                let mode = &mut self.prefs.gpu_layers;
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(mode,crate::resources::GPU_BALANCED,l.text("Сбалансированный"));
                    if ui.selectable_label((1..crate::resources::GPU_BALANCED).contains(mode),l.text("Ручная настройка")).clicked() { *mode=16; }
                });
                if (1..crate::resources::GPU_BALANCED).contains(mode) { ui.add(egui::Slider::new(mode,1..=256).text(l.text("Слоёв на CUDA"))); }
            });
            if ui.small_button(l.text("Сбросить числовые параметры")).clicked() { self.prefs.reset_inference(); }
        });
    }
}
