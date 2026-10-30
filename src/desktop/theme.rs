use eframe::egui::{self, Color32, Stroke};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Palette {
    Mint,
    Ocean,
    Violet,
    Light,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    Ru,
    En,
}

#[derive(Clone, Copy)]
pub struct Colors {
    pub background: Color32,
    pub card: Color32,
    pub input: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
    pub success: Color32,
    pub error: Color32,
}
const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}
impl Palette {
    pub const ALL: [Self; 4] = [Self::Mint, Self::Ocean, Self::Violet, Self::Light];
    pub fn name(self) -> &'static str {
        match self {
            Self::Mint => "Мята",
            Self::Ocean => "Океан",
            Self::Violet => "Лаванда",
            Self::Light => "Светлая",
        }
    }
    pub fn colors(self) -> Colors {
        match self {
            Self::Mint => Colors {
                background: rgb(18, 26, 27),
                card: rgb(27, 38, 39),
                input: rgb(37, 50, 51),
                border: rgb(58, 77, 75),
                text: rgb(231, 239, 235),
                muted: rgb(167, 185, 179),
                accent: rgb(139, 204, 180),
                on_accent: rgb(20, 33, 29),
                success: rgb(150, 215, 178),
                error: rgb(240, 163, 169),
            },
            Self::Ocean => Colors {
                background: rgb(18, 26, 35),
                card: rgb(28, 39, 51),
                input: rgb(39, 53, 69),
                border: rgb(58, 77, 96),
                text: rgb(230, 237, 245),
                muted: rgb(166, 182, 201),
                accent: rgb(143, 188, 219),
                on_accent: rgb(19, 32, 44),
                success: rgb(154, 215, 190),
                error: rgb(240, 163, 169),
            },
            Self::Violet => Colors {
                background: rgb(25, 23, 34),
                card: rgb(36, 33, 47),
                input: rgb(49, 44, 62),
                border: rgb(74, 65, 91),
                text: rgb(239, 234, 245),
                muted: rgb(187, 176, 202),
                accent: rgb(191, 171, 221),
                on_accent: rgb(35, 25, 48),
                success: rgb(158, 218, 187),
                error: rgb(240, 163, 169),
            },
            Self::Light => Colors {
                background: rgb(238, 239, 234),
                card: rgb(250, 250, 245),
                input: rgb(227, 232, 222),
                border: rgb(190, 202, 188),
                text: rgb(35, 48, 40),
                muted: rgb(82, 100, 87),
                accent: rgb(54, 111, 86),
                on_accent: rgb(250, 250, 245),
                success: rgb(27, 103, 66),
                error: rgb(161, 57, 68),
            },
        }
    }
}
pub fn configure(ctx: &egui::Context, palette: Palette, animations: bool) {
    let c = palette.colors();
    let light = palette == Palette::Light;
    let mut style = egui::Style {
        visuals: if light {
            egui::Visuals::light()
        } else {
            egui::Visuals::dark()
        },
        ..Default::default()
    };
    style.visuals.panel_fill = c.background;
    style.visuals.window_fill = c.card;
    style.visuals.extreme_bg_color = c.input;
    style.visuals.faint_bg_color = c.input;
    style.visuals.override_text_color = Some(c.text);
    style.visuals.weak_text_color = Some(c.muted);
    style.visuals.selection.bg_fill = Color32::from(egui::lerp(
        egui::Rgba::from(c.input)..=egui::Rgba::from(c.accent),
        0.25,
    ));
    style.visuals.selection.stroke = Stroke::new(1.0, c.text);
    style.visuals.hyperlink_color = c.accent;
    for v in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.noninteractive,
    ] {
        v.bg_fill = c.input;
        v.weak_bg_fill = c.card;
        v.bg_stroke = Stroke::new(1.0, c.border);
        v.fg_stroke = Stroke::new(1.0, c.text);
        v.corner_radius = egui::CornerRadius::same(8);
    }
    for v in [
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
    ] {
        v.bg_fill = c.input;
        v.weak_bg_fill = c.input;
        v.bg_stroke = Stroke::new(1.0, c.accent);
        v.fg_stroke = Stroke::new(1.0, c.accent);
        v.corner_radius = egui::CornerRadius::same(8);
    }
    style.animation_time = if animations { 0.14 } else { 0.0 };
    style.spacing.item_spacing = egui::vec2(8.0, 7.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.spacing.slider_width = 220.0;
    for (kind, size) in [
        (egui::TextStyle::Body, 14.0),
        (egui::TextStyle::Button, 14.0),
        (egui::TextStyle::Small, 11.5),
        (egui::TextStyle::Heading, 18.0),
    ] {
        style
            .text_styles
            .insert(kind, egui::FontId::proportional(size));
    }
    style
        .text_styles
        .insert(egui::TextStyle::Monospace, egui::FontId::monospace(13.0));
    let theme = if light {
        egui::Theme::Light
    } else {
        egui::Theme::Dark
    };
    ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(if light {
        egui::SystemTheme::Light
    } else {
        egui::SystemTheme::Dark
    }));
    ctx.set_theme(theme);
    ctx.set_style_of(theme, style);
}

impl Language {
    pub fn text(self, ru: &str) -> &str {
        if self == Self::Ru {
            return ru;
        }
        match ru {
            "Обязательные файлы, по одному на строке (необязательно)" => {
                "Required files, one per line (optional)"
            }
            "Новый проект" => "New project",
            "Документация" => "Documentation",
            "Закрыть" => "Close",
            "Имя" => "Name",
            "Версия платформы / SDK" => "Platform / SDK version",
            "Опиши результат, ограничения и критерии приёмки. Неизвестное можно оставить вопросом." => {
                "Describe the result, constraints and acceptance criteria. Leave material unknowns as questions."
            }
            "Что создать?" => "What should be created?",
            "AI-проект" => "AI project",
            "Стартовый каркас" => "Starter scaffold",
            "Контекст / токены ответа" => "Context / response tokens",
            "Для нескольких файлов обычно нужны 8192 контекста и 2048–4096 токенов ответа. Измени их в настройках модели." => {
                "Multiple files usually need 8192 context and 2048–4096 response tokens. Adjust these in model settings."
            }
            "Каркас не реализует задачу. После создания продолжи её на главной." => {
                "A scaffold does not implement the task. Continue it on the main page after creation."
            }
            "Подготовить план" => "Prepare plan",
            "Требования и источники" => "Requirements and sources",
            "Это предложения. Компиляция и внешние тесты не выполнялись." => {
                "These are proposals. Compilation and external tests have not been run."
            }
            "Куда создать…" => "Choose parent folder…",
            "Создать и открыть проект" => "Create and open project",
            "Создаётся только новая папка. Существующий проект не перезаписывается." => {
                "Only a new folder is created. Existing projects are never overwritten."
            }
            "Документация обрабатывается локально. Загрузка официальных страниц — только по кнопке." => {
                "Documentation is processed locally. Official pages are downloaded only on request."
            }
            "Выбрать pack…" => "Choose pack…",
            "Отключить документы" => "Disable documents",
            "Импортировать файлы…" => "Import files…",
            "Загрузить официальные документы…" => {
                "Download official documents…"
            }
            "Символ или вопрос по API" => "API symbol or question",
            "Поиск" => "Search",
            "Официальные источники профиля" => {
                "Official sources for this profile"
            }
            "Для любого другого языка или SDK импортируй локальные тексты. Rolling-документация не доказывает совместимость с версией проекта." => {
                "Import local texts for any other language or SDK. Rolling documentation does not prove compatibility with the project version."
            }
            "План готов. Проверь файлы и вопросы." => {
                "Plan ready. Review the files and questions."
            }
            "Проект создан. Сборка и внешние тесты ещё не выполнялись." => {
                "Project created. Build and external tests have not been run."
            }
            "Документация сохранена локально и выбрана для задач." => {
                "Documentation saved locally and selected for tasks."
            }
            "Готовлю план проекта…" => "Preparing project plan…",
            "Создаю новый проект…" => "Creating new project…",
            "Загружаю документацию…" => "Downloading documentation…",
            "Импортирую документацию…" => "Importing documentation…",
            "Операция остановлена. Проверь путь пакета: сохранение могло завершиться." => {
                "Operation stopped. Check the pack path: saving may have completed."
            }
            "Новый файл" => "New file",
            "Документация в контексте" => "Documentation in context",
            "Другой / выбор AI" => "Other / AI choice",

            "Обновления" => "Updates",
            "Проверять обновления каждые 5 минут" => {
                "Check for updates every 5 minutes"
            }
            "Предварительные версии" => "Preview releases",
            "Проверить обновления" => "Check for updates",
            "Скачать обновление" => "Download update",
            "Установить и перезапустить" => "Install and restart",
            "Установить и перезапустить?" => "Install and restart?",
            "GitHub Releases · скачивание и установка только по кнопке." => {
                "GitHub Releases · download and install on request."
            }
            "Установка обновлений доступна в portable Windows x64 CPU. Для других сборок скачай пакет вручную." => {
                "In-place updates require portable Windows x64 CPU. Download other builds manually."
            }
            "Перед установкой заверши или останови текущую операцию." => {
                "Finish or stop the current operation before installing."
            }
            "Модель работает локально. Проверку обновлений можно отключить; CLI к сети не обращается." => {
                "Inference stays local. Update checks are optional; the CLI uses no network."
            }
            "Текущая задача и отчёт в памяти не переносятся. Сохрани нужный отчёт перед продолжением." => {
                "The current task and report in memory will not transfer. Save any report you need before continuing."
            }
            "Проекты, модели и локальные настройки сохраняются." => {
                "Projects, models and local preferences are preserved."
            }
            "Обновления ещё не проверялись" => {
                "Updates have not been checked yet"
            }
            "Предыдущее обновление не выполнено" => {
                "The previous update failed"
            }
            "Обновление не выполнено" => "Update failed",
            "Проверяю GitHub Releases…" => "Checking GitHub Releases…",
            "Скачивание и проверка…" => "Downloading and verifying…",
            "Готовлю перезапуск…" => "Preparing to restart…",
            "Доступна новая версия" => "A new version is available",
            "Новых опубликованных версий пока нет" => {
                "No newer published release is available"
            }
            "Пакет проверен. Можно установить" => {
                "Package verified. Ready to install"
            }
            "Загрузка или проверка отменена" => {
                "Download or check cancelled"
            }
            "Обновление установлено" => "Update installed",

            "Мята" => "Mint",
            "Океан" => "Ocean",
            "Лаванда" => "Lavender",
            "Светлая" => "Light",
            "Вернуться к задаче" => "Back to task",
            "Настройки" => "Settings",
            "Как пользоваться" => "How to use",
            "Локально · CPU" => "Local · CPU",
            "Модель GGUF" => "GGUF model",
            "Проект" => "Project",
            "Выбрать…" => "Choose…",
            "Опиши задачу для выбранного проекта…" => {
                "Describe a task for the selected project…"
            }
            "Запустить" => "Run task",
            "Остановить" => "Stop",
            "С исследователем и ревьюером" => {
                "With researcher and reviewer"
            }
            "Предложения" => "Proposals",
            "Журнал" => "Activity",
            "Статус" => "Status",
            "После задачи здесь появятся действия и журнал движка." => {
                "Actions and engine output will appear here after a task."
            }
            "Модель и ресурсы" => "Model and resources",
            "Контекст проекта" => "Project context",
            "Помощники" => "Workers",
            "Оформление" => "Appearance",
            "Мой компьютер" => "My computer",
            "Контекст" => "Context",
            "Контекст включает задание, код, историю и ответ. Большое окно требует больше памяти." => {
                "The context holds the task, code, history and response. Larger contexts need more memory."
            }
            "Токенов на ответ" => "Tokens per response",
            "Максимум токенов одного действия модели." => {
                "Maximum tokens for a single model action."
            }
            "CPU-потоки" => "CPU threads",
            "Больше потоков не всегда быстрее. Начальное значение учитывает доступные CPU, максимум 8." => {
                "More threads are not always faster. The initial value follows available CPUs, capped at 8."
            }
            "Таймаут, с" => "Timeout, seconds",
            "Общее время задачи, включая загрузку модели." => {
                "Total task time, including model loading."
            }
            "Сбросить числовые параметры" => "Reset numeric parameters",
            "Файлы контекста · до 32 путей, по одному на строке" => {
                "Context files · up to 32 paths, one per line"
            }
            "Пусто — ограниченный индекс проекта. Секретные и служебные файлы исключаются движком." => {
                "Leave empty for a bounded project index. The engine excludes secrets and service files."
            }
            "Исследователь и ревьюер" => "Researcher and reviewer",
            "Роли работают последовательно с одной моделью. Дополнительные проверки требуют времени." => {
                "Roles run sequentially with one model. Additional reviews take more time."
            }
            "Общий лимит вызовов" => "Total call budget",
            "Бюджет общий для всех ролей. Исследователь и ревьюер не изменяют файлы." => {
                "All roles share this budget. The researcher and reviewer cannot modify files."
            }
            "Тема" => "Theme",
            "Язык" => "Language",
            "Мягкие анимации" => "Gentle animations",
            "Короткие переходы и отклик переключателей. В покое нет постоянных анимаций." => {
                "Short transitions and toggle feedback. No continuous animation while idle."
            }
            "Параметры и выбранные пути сохраняются локально. Задания, код и журнал не сохраняются автоматически." => {
                "Preferences and selected paths stay on this computer. Tasks, code and logs are never saved automatically."
            }
            "Обновить диагностику" => "Refresh hardware",
            "Настройки не сохранены" => "Preferences could not be saved",
            "Сведения о текущем компьютере появятся по кнопке." => {
                "Use the button to inspect the current computer."
            }
            "Процессор" => "CPU",
            "Память" => "Memory",
            "Доступная память" => "Available memory",
            "свободно" => "free",
            "Модель работает на CPU. Ускорение CUDA проверяется отдельно." => {
                "The model runs on CPU. CUDA acceleration requires separate validation."
            }
            "Выбери проект и модель, затем задай задачу." => {
                "Choose a project and model, then describe your task."
            }
            "Предложения появятся здесь. Файлы меняются только после твоего подтверждения." => {
                "Proposals will appear here. Files change only after your confirmation."
            }
            "Проект отчёта" => "Report workspace",
            "Применимых предложений нет. Завершение протокола не доказывает решения задачи." => {
                "No applicable proposals. Finishing the protocol does not prove the task was solved."
            }
            "Применить…" => "Apply…",
            "Сохранить JSON-отчёт" => "Save JSON report",
            "Применено. Запусти тесты проекта отдельно." => {
                "Applied. Run project tests separately."
            }
            "Исходный код" => "Original code",
            "Предложенный код" => "Proposed code",
            "Итог" => "Summary",
            "Применить изменение?" => "Apply this change?",
            "Исходное содержимое будет проверено ещё раз. При конфликте запись отменится." => {
                "The original content will be checked again. A conflict prevents the write."
            }
            "Применить" => "Apply",
            "Отмена" => "Cancel",
            "1. Выбери папку проекта и instruction-модель GGUF." => {
                "1. Choose a project folder and an instruction GGUF model."
            }
            "2. Опиши задачу и запусти. При необходимости останови её." => {
                "2. Describe your task and run it. Stop it if needed."
            }
            "3. Просмотри предложение, подтверди применение и запусти тесты проекта." => {
                "3. Review the proposal, confirm application and run the project tests."
            }
            "Модель не запускает shell и тесты. Ревью AI не заменяет проверку кода." => {
                "The model cannot run a shell or tests. AI review does not replace testing."
            }
            "Понятно" => "Got it",
            "Готова к локальной работе" => "Ready for local work",
            "Модель работает…" => "Model is working…",
            "Применение выбранной правки…" => {
                "Applying the selected change…"
            }
            "Сбор сведений о ПК…" => "Reading local hardware…",
            "Сведения собраны локально" => "Hardware collected locally",
            "Задача остановлена. Правки не применялись." => {
                "Task stopped. No changes were applied."
            }
            "Правка применена. Запустите тесты проекта отдельно." => {
                "Change applied. Run project tests separately."
            }
            "Правка не применена: конфликт или ошибка доступа. Откройте журнал." => {
                "Change was not applied: conflict or access error. Open Activity."
            }
            "Не удалось получить диагностику. Откройте журнал." => {
                "Hardware collection failed. Open Activity."
            }
            "Задача не завершена" => "Task incomplete",
            "Протокол завершён; предложений нет" => {
                "Protocol finished; no proposals"
            }
            "Предложения готовы к просмотру" => {
                "Proposals ready for review"
            }
            "вызовов" => "calls",
            "предложений" => "proposals",
            _ => ru,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn luminance(c: Color32) -> f32 {
        let linear = |v: u8| {
            let v = v as f32 / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(c.r()) + 0.7152 * linear(c.g()) + 0.0722 * linear(c.b())
    }
    #[test]
    fn palettes_keep_body_hint_and_action_text_readable() {
        for palette in Palette::ALL {
            let c = palette.colors();
            for (text, background) in [
                (c.text, c.background),
                (c.text, c.card),
                (c.text, c.input),
                (c.muted, c.card),
                (c.muted, c.input),
                (c.on_accent, c.accent),
                (c.error, c.card),
                (c.success, c.card),
            ] {
                let a = luminance(text);
                let b = luminance(background);
                let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);
                assert!(ratio >= 4.5, "{palette:?}: contrast {ratio}");
            }
        }
    }
}
