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

/// Semantic colour tokens. Layers go background → surface → card → input → hover.
#[derive(Clone, Copy)]
pub struct Colors {
    /// Main canvas.
    pub background: Color32,
    /// Navigation rail, side panels and status bar.
    pub surface: Color32,
    /// Raised cards and popups.
    pub card: Color32,
    pub input: Color32,
    pub hover: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub error: Color32,
}
const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}
impl Colors {
    /// Accent blended into a surface: selected rows, chips, soft highlights.
    pub fn accent_soft(&self) -> Color32 {
        self.mix(self.card, self.accent, 0.16)
    }
    pub fn mix(&self, from: Color32, to: Color32, t: f32) -> Color32 {
        Color32::from(egui::lerp(egui::Rgba::from(from)..=egui::Rgba::from(to), t))
    }
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
                background: rgb(13, 17, 18),
                surface: rgb(17, 23, 24),
                card: rgb(22, 30, 31),
                input: rgb(28, 38, 39),
                hover: rgb(36, 49, 50),
                border: rgb(40, 54, 55),
                text: rgb(232, 240, 237),
                muted: rgb(140, 160, 156),
                accent: rgb(61, 214, 168),
                on_accent: rgb(6, 30, 24),
                success: rgb(96, 214, 140),
                warning: rgb(232, 176, 75),
                error: rgb(242, 114, 127),
            },
            Self::Ocean => Colors {
                background: rgb(12, 16, 23),
                surface: rgb(16, 21, 30),
                card: rgb(21, 28, 40),
                input: rgb(27, 36, 51),
                hover: rgb(35, 47, 66),
                border: rgb(38, 50, 69),
                text: rgb(230, 237, 247),
                muted: rgb(139, 154, 177),
                accent: rgb(76, 163, 255),
                on_accent: rgb(5, 22, 45),
                success: rgb(96, 214, 140),
                warning: rgb(232, 176, 75),
                error: rgb(242, 114, 127),
            },
            Self::Violet => Colors {
                background: rgb(15, 15, 20),
                surface: rgb(20, 20, 27),
                card: rgb(26, 26, 35),
                input: rgb(33, 33, 45),
                hover: rgb(42, 42, 58),
                border: rgb(44, 44, 60),
                text: rgb(236, 236, 244),
                muted: rgb(150, 150, 172),
                accent: rgb(112, 96, 236),
                on_accent: rgb(255, 255, 255),
                success: rgb(96, 214, 140),
                warning: rgb(232, 176, 75),
                error: rgb(242, 114, 127),
            },
            Self::Light => Colors {
                background: rgb(246, 246, 249),
                surface: rgb(238, 238, 244),
                card: rgb(255, 255, 255),
                input: rgb(240, 240, 246),
                hover: rgb(228, 228, 238),
                border: rgb(220, 220, 232),
                text: rgb(27, 27, 36),
                muted: rgb(86, 86, 106),
                accent: rgb(91, 75, 219),
                on_accent: rgb(255, 255, 255),
                success: rgb(22, 128, 75),
                warning: rgb(150, 92, 0),
                error: rgb(196, 48, 64),
            },
        }
    }
}

pub const RADIUS: u8 = 10;
pub const RADIUS_SMALL: u8 = 7;

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
    let v = &mut style.visuals;
    v.panel_fill = c.background;
    v.window_fill = c.card;
    v.window_stroke = Stroke::new(1.0, c.border);
    v.window_corner_radius = egui::CornerRadius::same(14);
    v.menu_corner_radius = egui::CornerRadius::same(RADIUS);
    v.extreme_bg_color = c.input;
    v.faint_bg_color = c.surface;
    v.code_bg_color = c.input;
    v.override_text_color = Some(c.text);
    v.weak_text_color = Some(c.muted);
    v.selection.bg_fill = c.accent_soft();
    v.selection.stroke = Stroke::new(1.0, c.accent);
    v.hyperlink_color = c.accent;
    v.window_shadow = egui::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(if light { 40 } else { 140 }),
    };
    v.popup_shadow = v.window_shadow;
    v.widgets.noninteractive.bg_fill = c.card;
    v.widgets.noninteractive.weak_bg_fill = c.card;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, c.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, c.text);
    v.widgets.inactive.bg_fill = c.input;
    v.widgets.inactive.weak_bg_fill = c.input;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, c.border);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, c.text);
    v.widgets.hovered.bg_fill = c.hover;
    v.widgets.hovered.weak_bg_fill = c.hover;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, c.accent.gamma_multiply(0.7));
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, c.text);
    v.widgets.active.bg_fill = c.accent_soft();
    v.widgets.active.weak_bg_fill = c.accent_soft();
    v.widgets.active.bg_stroke = Stroke::new(1.0, c.accent);
    v.widgets.active.fg_stroke = Stroke::new(1.0, c.text);
    v.widgets.open.bg_fill = c.hover;
    v.widgets.open.weak_bg_fill = c.hover;
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = egui::CornerRadius::same(RADIUS_SMALL);
    }
    style.animation_time = if animations { 0.12 } else { 0.0 };
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    style.spacing.interact_size.y = 28.0;
    style.spacing.slider_width = 240.0;
    style.spacing.scroll.bar_width = 8.0;
    style.spacing.scroll.floating = true;
    style.spacing.combo_width = 180.0;
    for (kind, size) in [
        (egui::TextStyle::Body, 14.0),
        (egui::TextStyle::Button, 14.0),
        (egui::TextStyle::Small, 12.0),
        (egui::TextStyle::Heading, 22.0),
    ] {
        style
            .text_styles
            .insert(kind, egui::FontId::proportional(size));
    }
    style
        .text_styles
        .insert(egui::TextStyle::Monospace, egui::FontId::monospace(12.5));
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
            "Настройки и история чатов хранятся локально. Отчёты предложений и технический журнал автоматически не сохраняются." => {
                "Settings and chat history are stored locally. Proposal reports and technical logs are not saved automatically."
            }
            "Навыки ECC" => "ECC skills",
            "Выберите GGUF и устройство для локальной модели. Для HTTP-моделей используйте раздел провайдеров." => {
                "Choose a GGUF and device for local inference. HTTP models are configured under providers."
            }
            "Подключение внешних AI-сервисов. Запросы отправляются только при включении соответствующего режима." => {
                "Connect external AI services. Requests are sent only when the corresponding mode is enabled."
            }
            "Выберите файлы проекта и объём истории, передаваемой модели." => {
                "Choose project files and how much conversation history to send to the model."
            }
            "Дополнительные роли для задач проекта и общий лимит их обращений к модели." => {
                "Additional roles for project tasks and their shared model-call budget."
            }
            "Инструкции ECC для модели: выбор, просмотр, локальная установка и откат пакетов." => {
                "ECC guidance for the model: selection, preview, local installation and package rollback."
            }
            "Тема, язык и анимации интерфейса." => {
                "Interface theme, language and animations."
            }
            "Проверьте доступные CPU, RAM и NVIDIA GPU перед выбором модели." => {
                "Check available CPU, RAM and NVIDIA GPU before choosing a model."
            }
            "Версия приложения и обновления. CPU и CUDA поставляются отдельными пакетами." => {
                "Application version and updates. CPU and CUDA are distributed as separate packages."
            }
            "Подключите справочные материалы проекта. Это источники для агента, а не веса модели." => {
                "Connect project reference material. These are sources for the agent, not model weights."
            }
            "Где выполнять локальную модель" => {
                "Where to run the local model"
            }
            "NVIDIA GPU · Авто (рекомендуется)" => {
                "NVIDIA GPU · Auto (recommended)"
            }
            "Только CPU" => "CPU only",
            "Это CPU-сборка. Для NVIDIA GPU скачайте отдельный пакет CUDA; веса GGUF можно использовать те же." => {
                "This is a CPU build. Download the separate CUDA package for NVIDIA GPU; the same GGUF weights can be used."
            }
            "Выбран CPU. Для ускорения на NVIDIA выберите GPU Авто; существующий выбор не меняется автоматически." => {
                "CPU is selected. Choose GPU Auto for NVIDIA acceleration; your existing selection is preserved."
            }
            "GPU Авто оценивает доступную VRAM и переносит подходящее число слоёв на NVIDIA GPU. Остальная часть использует CPU и RAM." => {
                "GPU Auto estimates available VRAM and offloads a suitable number of layers to NVIDIA GPU. The remainder uses CPU and RAM."
            }
            "Выбрано ручное или сбалансированное распределение. Настройка слоёв находится в разделе Дополнительно." => {
                "Manual or balanced allocation is selected. Layer controls are under Advanced."
            }
            "В настройках сохранён GPU-режим. Выберите Только CPU или запустите CUDA-сборку." => {
                "A GPU mode is saved in settings. Select CPU only or run the CUDA build."
            }
            "Нет модели? Подобрать и установить GGUF" => {
                "No model? Choose and install a GGUF"
            }
            "Дополнительно · память, длина ответа и время" => {
                "Advanced · memory, response length and time"
            }
            "Начните с контекста 4096. Большое окно требует больше RAM/VRAM; длинный ответ и загрузка весов требуют времени." => {
                "Start with a 4096 context. Larger windows need more RAM/VRAM; longer responses and loading weights take time."
            }
            "Таймаут включает загрузку модели и все шаги задачи." => {
                "The timeout includes model loading and all task steps."
            }
            "Ручное распределение GPU / CPU" => "Manual GPU / CPU allocation",
            "Сбалансированный режим оставляет часть слоёв на CPU и не обязательно быстрее Авто." => {
                "Balanced mode leaves some layers on CPU and is not necessarily faster than Auto."
            }
            "Сбалансированный" => "Balanced",
            "Ручная настройка" => "Manual",
            "Слоёв на CUDA" => "Layers on CUDA",
            "Полная история" => "Full history",
            "Последние 12 сообщений" => "Latest 12 messages",
            "Последние 6 сообщений" => "Latest 6 messages",
            "Пакет навыков · установка и версии" => {
                "Skill package · installation and versions"
            }
            "Локальный снимок выбранных навыков. Версия и источник указываются вами; SHA256 не является подписью автора." => {
                "A local snapshot of selected skills. Version and origin are declared by you; SHA256 is not an author signature."
            }
            "ID пакета (a-z, 0-9, дефис)" => "Package ID (a-z, 0-9, hyphen)",
            "Версия пакета" => "Package version",
            "Источник / ревизия" => "Origin / revision",
            "Установить выбранные навыки и использовать снимок" => {
                "Install selected skills and use snapshot"
            }
            "Пакет установлен локально." => "Package installed locally.",
            "Предыдущая версия" => "Previous version",
            "Переключить на предыдущую версию" => {
                "Switch to previous version"
            }
            "Версия пакета переключена. Проверьте выбранные навыки." => {
                "Package version switched. Check selected skills."
            }
            "ECC Skills · файл или каталог" => "ECC Skills · file or directory",
            "Загрузить каталог навыков" => "Load skill catalog",
            "Навыки — инструкции для модели. Скрипты и hooks не исполняются. Выбранные навыки применяются в порядке выбора." => {
                "Skills guide the model. Scripts and hooks are not executed. Selected skills are applied in selection order."
            }
            "Выбрано" => "Selected",
            "Снять выбор навыков" => "Clear skill selection",
            "Инструкции" => "Instructions",
            "Выбранный навык отсутствует в каталоге. Снимите выбор или верните источник." => {
                "A selected skill is missing from this catalog. Clear the selection or restore its source."
            }
            "Сокращено контекста:" => "Context saved:",
            "байт" => "bytes",
            "Дедупликация результатов инструментов (задачи)" => {
                "Deduplicate tool results (tasks)"
            }
            "Дедупликация сохраняет выбранные наблюдения дословно. При нехватке окна контекст отдельно сокращается с хешами. Локальные задачи считают токены выбранной моделью." => {
                "Deduplication preserves selected observations verbatim. Context selection separately marks omissions with hashes. Local tasks count tokens using the selected model."
            }
            "Выбрать проект" => "Choose project",
            "Проект · необязательно" => "Project · optional",
            "Выбрать существующий проект" => "Choose existing project",
            "Без проекта · создавать с нуля" => {
                "No project · create from scratch"
            }
            "Выбрать GGUF" => "Choose GGUF",
            "Новый чат" => "New chat",
            "Что сделаем?" => "What shall we do?",
            "Ход работы" => "Activity",
            "Подготовка запроса…" => "Preparing request…",
            "Подготовка запроса" => "Preparing request",
            "Загрузка GGUF" => "Loading GGUF",
            "Обработка контекста" => "Processing context",
            "Генерация ответа" => "Generating response",
            "Запускаю выполнение…" => "Starting execution…",
            "Подготавливаю контекст запроса…" => {
                "Preparing request context…"
            }
            "Выполнение прервано" => "Execution interrupted",
            "Выполнение завершено" => "Execution finished",
            "Работа" => "Work",
            "Агенты" => "Agents",
            "Выбрать репозиторий" => "Choose repository",
            "Без проекта" => "No project",
            "Подобрать локальную GGUF" => "Find a local GGUF",
            "рекомендация для контекста 4096" => {
                "recommendation for 4096 context"
            }
            "Источник и лицензия" => "Source and license",
            "Для этой модели рекомендуется больше RAM" => {
                "More RAM is recommended for this model"
            }
            "Установить GGUF…" => "Install GGUF…",
            "Папка для моделей GGUF" => "GGUF models folder",
            "Локальная модель не выбрана" => "No local model selected",
            "Подобрать и установить GGUF" => "Find and install GGUF",
            "GGUF установлена и выбрана" => "GGUF installed and selected",
            "Разобраться в проекте" => "Understand the project",
            "Изучи проект и объясни его устройство." => {
                "Explore the project and explain its structure."
            }
            "Ответь следующим сообщением. Файлы ещё не созданы." => {
                "Reply in your next message. No files have been created."
            }
            "Опиши задачу. Существующий проект можно выбрать сверху." => {
                "Describe your task. Choose an existing project above if needed."
            }
            "Сайт на Vue и TypeScript" => "Website with Vue and TypeScript",
            "Создай адаптивный сайт на Vue и TypeScript. Уточни назначение и визуальное направление." => {
                "Create a responsive website with Vue and TypeScript. Ask about its purpose and visual direction."
            }
            "Создай адаптивный сайт на Vue и TypeScript." => {
                "Create a responsive website with Vue and TypeScript."
            }
            "Какое назначение сайта и какие примеры оформления тебе нравятся?" => {
                "What is the website for, and which visual references do you like?"
            }
            "Вы" => "You",
            "Файлы и предложения" => "Files and proposals",
            "Файлы нового проекта" => "New project files",
            "Имя папки" => "Folder name",
            "Создать проект…" => "Create project…",
            "Где сохранить новый проект?" => {
                "Where should the new project be saved?"
            }
            "Сборка и тесты ещё не запускались. Создание — только в новой папке." => {
                "Build and tests have not run. Only a new folder can be created."
            }
            "Напиши, что нужно сделать…" => "Describe what you need…",
            "Enter — отправить · Shift+Enter — новая строка" => {
                "Enter to send · Shift+Enter for a new line"
            }
            "Отправить" => "Send",
            "Локальная модель · файлы меняются после просмотра" => {
                "Local model · review changes before applying"
            }
            "1. Выбери локальную GGUF. Папка проекта — по необходимости." => {
                "1. Choose a local GGUF. A project folder is optional."
            }
            "2. Напиши задачу в чате. Без папки Lyra подготовит новый проект." => {
                "2. Write a message. Without a folder, Lyra plans a new project."
            }
            "3. Ответь на вопросы, просмотри файлы и явно создай проект или примени правку." => {
                "3. Answer questions, review files, then explicitly create or apply."
            }
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
            "Граф проекта в контексте агентов" => {
                "Project graph in agent context"
            }
            "Добавляет ~3 КБ структуры (имена файлов, без кода) в запросы; для малых контекстов GGUF увеличьте контекст." => {
                "Adds ~3 KB of structure (file names, no code) to requests; raise the GGUF context for small windows."
            }
            "Открыть graph.json (Graphify/Lyra)" => "Open graph.json (Graphify/Lyra)",
            "Граф кода" => "Code graph",
            "Построить граф проекта" => "Build project graph",
            "Влияние PR из GitHub" => "GitHub PR impact",
            "Символы" => "Symbols",
            "Без связей" => "Isolated",
            "изменён в PR" => "changed in PR",
            "затронут, глубина" => "impacted, depth",
            "файл" => "file",
            "символ" => "symbol",
            "связей" => "links",
            "колесо — масштаб, перетаскивание — сдвиг, клик — узел" => {
                "wheel zoom, drag pan, click a node"
            }
            "Выберите папку проекта и постройте граф: файлы, символы, импорты и сообщества. Содержимое файлов не сохраняется." => {
                "Choose a project folder and build the graph: files, symbols, imports and communities. File contents are not stored."
            }
            "Загрузить issue как задачу" => "Load issue as task",
            "Переменная токена" => "Token variable",
            "Черновик PR в GitHub" => "Draft PR on GitHub",
            "Токен читается из переменной окружения и нигде не сохраняется. Публикация PR — кнопка рядом с отчётом." => {
                "The token is read from an environment variable and never stored. Publish a PR with the button next to the report."
            }
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
            "Проверка проекта в Docker" => "Project checks in Docker",
            "Выбрать проверку JSON…" => "Choose JSON check…",
            "Отключить проверку" => "Disable check",
            "Модель сможет запускать только выбранную проверку в изолированной копии без сети." => {
                "The model may run only the selected check in an isolated offline copy."
            }
            "Сохранять задачи для продолжения" => {
                "Save tasks for resuming"
            }
            "База содержит выбранные исходники и предложения. Хранится локально, отдельно от истории чата." => {
                "The database contains selected sources and proposals. It stays local, separate from chat history."
            }
            "Выбрать базу задач…" => "Choose task database…",
            "Сохраненные задачи…" => "Saved tasks…",
            "Сохраненные задачи" => "Saved tasks",
            "В базе пока нет сохраненных задач." => {
                "No saved tasks in this database yet."
            }
            "Чтение сохраненных задач…" => "Reading saved tasks…",
            "Не удалось прочитать базу задач. Проверьте путь и журнал событий." => {
                "Could not read task database. Check its path and Activity."
            }
            "Сохраненные задачи прочитаны." => "Saved tasks read.",
            "Продолжение использует текущую модель и выбранную проверку. Исходный проект меняется только через применение предложений." => {
                "Resume uses the current model and selected check. The original project changes only when proposals are applied."
            }
            "Продолжить задачу" => "Resume task",
            "Для продолжения увеличьте общий лимит выше числа сохраненных вызовов." => {
                "To resume, set the total budget above the number of saved calls."
            }
            "Предыдущая страница" => "Previous page",
            "Следующая страница" => "Next page",
            "Выбранные технологии и план" => {
                "Selected technologies and plan"
            }
            "Среда выполнения" => "Execution environment",
            "Необходимые возможности" => "Required capabilities",
            "Сохранить копию для просмотра…" => {
                "Save a copy for review…"
            }
            "Среды выполнения и документы" => {
                "Execution environments and documents"
            }
            "Выбрать реестр сред…" => "Choose environment registry…",
            "Скачать образ этой среды" => "Download this environment image",
            "Отключить выполнение скриптов" => {
                "Disable script execution"
            }
            "Выбрать входные файлы…" => "Choose input files…",
            "Скрипты выполняются без сети в разрешенных контейнерах. Исходники меняются после применения результата." => {
                "Scripts run offline in permitted containers. Source files change only when results are applied."
            }
            "Применить весь набор…" => "Apply all changes…",
            "Применить весь набор?" => "Apply all changes?",
            "Восстановление проекта" => "Project recovery",
            "Применение набора правок…" => "Applying all changes…",
            "Чтение состояния восстановления…" => {
                "Reading recovery state…"
            }
            "Восстановление файлов…" => "Recovering files…",
            "Откатить публикацию…" => "Roll back publication…",
            "Завершить публикацию…" => "Complete publication…",
            "Откатить публикацию?" => "Roll back publication?",
            "Завершить публикацию?" => "Complete publication?",
            "Обновить состояние" => "Refresh state",
            "Применено" => "Applied",
            "Исходное состояние" => "Original state",
            "Есть незавершенная публикация." => {
                "There is an unfinished publication."
            }
            "Незавершенных публикаций нет." => {
                "No unfinished publications."
            }
            "Изменения из журнала отменены." => {
                "Journaled changes were rolled back."
            }
            "Публикация из журнала завершена." => {
                "Journaled publication was completed."
            }
            "Запись исходников не началась; можно очистить подготовку." => {
                "Source writes did not start; preparation can be cleared."
            }
            "Подготовка очищена; исходники не менялись." => {
                "Preparation cleared; source files were unchanged."
            }
            "Исходники в окне не показываются. Внешние изменения блокируют восстановление." => {
                "Source code is not shown here. External changes block recovery."
            }
            "Состояние не прочитано. Подробности — в журнале событий." => {
                "State was not read. See Activity for details."
            }
            "Набор правок применен. Запустите тесты проекта отдельно." => {
                "Changes applied. Run project tests separately."
            }
            "Состояние восстановления прочитано." => {
                "Recovery state read."
            }
            "Восстановление завершено. Проверьте файлы проекта." => {
                "Recovery completed. Review the project files."
            }
            "Операция не завершена. Проверьте восстановление и журнал событий." => {
                "Operation did not complete. Inspect recovery and Activity for details."
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
            "Модель выполняет только явно выбранную Docker-проверку. Ревью AI не заменяет тестирование." => {
                "The model runs only an explicitly selected Docker check. AI review does not replace testing."
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
            "GitHub-коннектор недоступен в этой сборке." => {
                "The GitHub connector is not available in this build."
            }
            "Issue из GitHub" => "GitHub issue",
            "Issue как задача" => "Issue as a task",
            "Issues как задачи, публикация предложений в виде черновика PR." => {
                "Issues as tasks, proposals published as a draft PR."
            }
            "Быстрая настройка" => "Quick setup",
            "Бюджет общий для всех ролей." => {
                "The budget is shared by all roles."
            }
            "Введите команду…" => "Type a command…",
            "Весь набор" => "Apply all",
            "Выберите GGUF и устройство для локальной модели. Для облачных моделей используйте HTTP-провайдеры." => {
                "Choose a GGUF and device for the local model. Use HTTP providers for cloud models."
            }
            "Выбрать папку" => "Choose folder",
            "Выбрать папку проекта…" => "Choose project folder…",
            "Выполняется задача: часть настроек недоступна" => {
                "A task is running: some settings are unavailable"
            }
            "Выполняется…" => "Working…",
            "Где разрешены облачные запросы" => {
                "Where cloud requests are allowed"
            }
            "Данные" => "Data",
            "Дедупликация результатов инструментов" => {
                "Deduplicate tool results"
            }
            "Добавить тесты" => "Add tests",
            "Добавь тесты для: " => "Add tests for: ",
            "Загрузить как задачу" => "Load as task",
            "Загрузка модели" => "Loading model",
            "Запуск" => "Starting",
            "Знания" => "Knowledge",
            "Изменения" => "Changes",
            "Изолированные Docker-проверки, среды выполнения скриптов и сохранение сессий." => {
                "Isolated Docker checks, script environments and saved sessions."
            }
            "История чата" => "Chat history",
            "Команды" => "Commands",
            "Контекст агентов" => "Agent context",
            "Копировать / Copy" => "Copy",
            "Локальная модель" => "Local model",
            "Модели" => "Models",
            "Модель" => "Model",
            "Модель не выбрана" => "No model selected",
            "Найди причину ошибки и предложи минимальное исправление: " => {
                "Find the cause of the bug and propose a minimal fix: "
            }
            "Найти и исправить ошибку" => "Find and fix a bug",
            "Не удалось загрузить реестр провайдеров." => {
                "Could not load the provider registry."
            }
            "Ничего не найдено" => "Nothing found",
            "Новый" => "New",
            "Номер issue" => "Issue number",
            "Облачная модель для задач проекта" => {
                "Cloud model for project tasks"
            }
            "Объясни структуру проекта: основные модули, точки входа и как они связаны." => {
                "Explain the project structure: main modules, entry points and how they connect."
            }
            "Объяснить проект" => "Explain the project",
            "Объём контекста" => "Context size",
            "Опишите задачу по проекту…" => {
                "Describe a task for the project…"
            }
            "Открыть граф кода" => "Open code graph",
            "Папка проекта" => "Project folder",
            "Папка проекта, файлы контекста и объём истории, передаваемой модели." => {
                "Project folder, context files and how much history is sent to the model."
            }
            "Перейти к чату" => "Go to chat",
            "План и итоги" => "Plan and summaries",
            "Повторные старые наблюдения передаются один раз со ссылками; последние, ошибки и патчи дословно." => {
                "Repeated old observations are sent once with references; the latest ones, errors and patches stay verbatim."
            }
            "Подготовка контекста" => "Preparing context",
            "Поиск по настройкам" => "Search settings",
            "Показать или скрыть панель изменений" => {
                "Show or hide the results panel"
            }
            "Показать или скрыть список чатов" => {
                "Show or hide the chat list"
            }
            "Показать результат задачи" => "Show task result",
            "Покрыть поведение проверяемыми тестами" => {
                "Cover behaviour with verifiable tests"
            }
            "При запуске задачи фрагменты исходного кода и задание отправляются выбранному провайдеру." => {
                "When a task runs, source fragments and the task are sent to the selected provider."
            }
            "Приложение" => "Application",
            "Причина и минимальная правка" => "Cause and minimal fix",
            "Проверить" => "Validate",
            "Проверка" => "Check",
            "Проверка не запускалась" => "Not verified",
            "Проверка не пройдена" => "Check failed",
            "Проверка пройдена" => "Check passed",
            "Проверки" => "Checks",
            "Проверки и среды" => "Checks and environments",
            "Проверки не запускались. Настройте Docker-проверку в «Проверки и среды»." => {
                "No checks were run. Configure a Docker check in “Checks and environments”."
            }
            "Проект (для режима «Работа»)" => "Project (for Work mode)",
            "Проект и контекст" => "Project and context",
            "Публикация PR — в панели результата: вкладка «Изменения» → «Черновик PR»." => {
                "Publish a PR from the results panel: Changes → Draft PR."
            }
            "Разрешить HTTP-запросы из чата" => {
                "Allow HTTP requests from chat"
            }
            "Реестр провайдеров" => "Provider registry",
            "Результат" => "Result",
            "Репозиторий" => "Repository",
            "Роли" => "Roles",
            "Роли работают последовательно с одной моделью. Исследователь и ревьюер не изменяют файлы; дополнительные проверки требуют времени." => {
                "Roles run sequentially with one model. The researcher and reviewer never change files; extra reviews take time."
            }
            "Сбросить" => "Reset",
            "Содержимое и внешний вид требуют просмотра перед применением." => {
                "Review the content and appearance before applying."
            }
            "Создай проект: " => "Create a project: ",
            "Создать новый проект" => "Create a new project",
            "Сокращается только история чата; исходники и ошибки не переписываются." => {
                "Only the chat history is shortened; sources and errors are not rewritten."
            }
            "Сохраняется только путь. API-ключи должны находиться в переменных окружения." => {
                "Only the path is stored. API keys must live in environment variables."
            }
            "Спланировать и создать с нуля" => {
                "Plan and create from scratch"
            }
            "Спросите что угодно…" => "Ask anything…",
            "Спросите, поставьте задачу по проекту или создайте новый." => {
                "Ask a question, give a task for your project or start a new one."
            }
            "Структура, модули и точки входа" => {
                "Structure, modules and entry points"
            }
            "Токен читается из переменной окружения и нигде не сохраняется." => {
                "The token is read from an environment variable and never stored."
            }
            "Файлы контекста" => "Context files",
            "Чат" => "Chat",
            "Чат и задачи" => "Chat and tasks",
            "Чаты" => "Chats",
            "Черновик PR" => "Draft PR",
            "Язык и движение" => "Language and motion",
            "двоичный файл" => "binary file",
            "не выбран" => "not selected",
            "не пройдена" => "failed",
            "новый" => "new",
            "применено" => "applied",
            "пройдена" => "passed",
            "файлов" => "files",
            "не выбрана" => "not selected",
            "Файл GGUF" => "GGUF file",
            "Устройство" => "Device",
            "Файл проверки (JSON)" => "Check file (JSON)",
            "Реестр сред" => "Environment registry",
            "Входные файлы" => "Input files",
            "До 32 файлов внутри рабочей папки; попадают в контекст задачи." => {
                "Up to 32 files inside the project folder; added to the task context."
            }
            "Сохранённые задачи" => "Saved tasks",
            "База задач" => "Task database",
            "Структура проекта одним взглядом" => {
                "Project structure at a glance"
            }
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
                (c.warning, c.card),
                (c.text, c.surface),
                (c.muted, c.surface),
                (c.muted, c.background),
                (c.text, c.hover),
            ] {
                let a = luminance(text);
                let b = luminance(background);
                let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);
                assert!(ratio >= 4.5, "{palette:?}: contrast {ratio}");
            }
        }
    }
}
