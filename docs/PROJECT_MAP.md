# Карта проекта Lyra

## Что существует сейчас

M0 завершён. В M1 реализованы CLI и локальная диагностика. M2 добавляет локальный inference;
alpha добавляет task/предложения/apply и ограниченные инструменты; будущие компоненты
отдельно обозначены в [архитектуре](ARCHITECTURE.md).

| Задача | Точка входа | Связанные документы |
| --- | --- | --- |
| Понять продукт и текущее состояние | [README](../README.md) | [Roadmap](ROADMAP.md) |
| Начать работу AI | [AGENTS.md](../AGENTS.md) | Эта карта, актуальный план задачи |
| Изменить стек, контракты или доступ | [Архитектура](ARCHITECTURE.md) | [Разработка](DEVELOPMENT.md), roadmap |
| Настроить сборку, CI или проверку | [Разработка](DEVELOPMENT.md) | Архитектура, актуальный этап roadmap |
| Продолжить многоэтапную задачу | [Планы](plans/) | Git status/diff, [шаблон](templates/TASK.md) |
| Проверить основание решения/API | [Источники](REFERENCES.md) | Текущая официальная документация |
| Аргументы и коды выхода CLI | [src/main.rs](../src/main.rs), [src/lib.rs](../src/lib.rs) | [Интеграционные тесты](../tests/cli.rs) |
| Диагностика/JSON железа | [src/doctor.rs](../src/doctor.rs) | Тесты внутри модуля, [модели и память](LOCAL_MODELS.md) |
| Нативная Windows/Linux сборка | [CI](../.github/workflows/ci.yml) | [Cargo.toml](../Cargo.toml), [toolchain](../rust-toolchain.toml) |
| Тексты лицензий в артефакте | [Упаковка](../tools/collect_licenses.py) | Cargo.lock, CI |

Минимальный маршрут новой сессии: `AGENTS.md` → эта карта → Git status/diff →
план текущей задачи → затронутые файлы и зависимости. Другие документы читать
по необходимости. Для изменений документации не нужен полный стек разработки.

Интеграция ECC + OmniRoute: [полный план](plans/0010-unified-ecc-omniroute.md) и [матрица статусов](integrations/PARITY.md).
Первый общий инкремент: [роли и контекст инструментов](integrations/TOOL_CONTEXT.md),
[план/критерии](plans/0012-ecc-omniroute-stage-1.md),
[lossless optimizer](../src/context_optimizer.rs).
Это план, а не доказательство наличия подсистем.

## Следующий этап

Текущее состояние M1 — в [плане](plans/0002-local-cli.md).
M2/alpha/desktop, универсальные проекты и документация реализованы в следующих
планах ниже. Следующие улучшения предложены в roadmap; CUDA и качество 7B
на пользовательском ПК ещё не проверены.
Пользовательский профиль железа хранится в игнорируемой `.lyra/user-hardware.json`,
если он есть в текущей рабочей среде. Он не входит в Git и не подменяет диагностику.

## Как обновлять карту

Когда модуль появится, добавить его фактическую точку входа, ответственность,
прямые зависимости и соответствующую проверку. После изменения сверять записи
с исходниками. Предполагаемые связи помечать как предположения.

Для будущего автоматического графа знаний полезны идеи
[Understand-Anything](https://github.com/Egonex-AI/Understand-Anything): привязка
к исходникам и ревизии, обновление по изменениям, проверка связности.
Граф — подсказка для поиска; текущий код остаётся источником истины.
Сейчас достаточно этой карты, поиска `rg` и актуальных планов.

Локальная генерация: [src/inference.rs](../src/inference.rs), [тесты](../tests/inference.rs), [план M2](plans/0003-local-inference.md).

Alpha: [оркестратор](../src/agent.rs), [границы проекта](../src/workspace.rs),
[сквозные тесты](../tests/agent.rs), [приёмка](ACCEPTANCE.md),
[план](plans/0004-testable-agent.md). Произвольного shell и полноценной OS sandbox нет.

Настольный интерфейс: [src/desktop.rs](../src/desktop.rs),
[worker](../src/desktop/job.rs), [точка входа](../src/bin/lyra-desktop.rs),
[тесты](../tests/desktop.rs), [запуск/приёмка](DESKTOP.md),
[план](plans/0005-desktop.md). Feature `desktop` добавляет отдельный нативный EXE.

Компактный GUI: [темы/локализация](../src/desktop/theme.rs),
[Lucide и переключатели](../src/desktop/icons.rs),
[локальные настройки](../src/desktop/preferences.rs), [ресурсы EXE](../build.rs),
[план](plans/0006-compact-ui.md). Конвертер значка: tools/render_icon.py и
examples/render-icon.rs; для пользователя Python не нужен.

Обновления: [controller](../src/desktop/updater/mod.rs), [HTTP/release](../src/desktop/updater/network.rs),
[package](../src/desktop/updater/package.rs), [helper/rollback](../src/desktop/updater/install.rs),
[сквозные тесты](../tests/updater.rs), [упаковка](../tools/package_update.py),
[инструкция](UPDATES.md), [текущий план](plans/0007-updater.md).

Проекты с нуля: [схема/создание/контекст](../src/project/mod.rs),
[профили](../src/project/profile.rs), [каркасы](../src/project/templates.rs),
[AI planner/grammar](../src/project/planner.rs), [результаты/worker GUI](../src/desktop/projects.rs),
[тесты](../tests/projects.rs), [инструкция](PROJECTS.md),
[текущий план](plans/0008-projects-knowledge.md).
Документация: [packs/search](../src/knowledge/mod.rs),
[явный fetch](../src/knowledge/online.rs), [тесты](../tests/knowledge.rs),
[инструкция](KNOWLEDGE.md). Новые файлы: Workspace::propose_new/apply,
Action::ProposeFile; отчёт v2, v1 legacy edits читаются. Каркасы с внешними SDK
проверяются [developer/CI harness](../tools/verify_starters.py), без исполнения AI-кода.

Чат: [история/главный экран](../src/desktop/chat.rs), [план](plans/0009-chat-web.md).
Один ввод, явные режимы Chat/Task/Project; Task требует repo, Project запускает planner.
Локальный и HTTP Chat используют общий DTO и выбор истории all/12/6.
Gateway: [контракты](../src/model_gateway.rs), [HTTP](../src/provider_http.rs),
[routing](../src/routing.rs), [runtime/skills](../src/agent_runtime.rs),
[реестр навыков](../src/skills.rs), [HTTP-тесты](../tests/gateway.rs).
Исправления интеграции и результаты CI: [план](plans/0011-pr-integration-fixes.md).
История только в RAM,
с ограничением числа/размера сообщений и сбросом при смене проекта.
Веб-профиль Vue/TypeScript: [рекомендации и приёмка](WEB_PROJECTS.md).
