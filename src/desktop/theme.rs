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
