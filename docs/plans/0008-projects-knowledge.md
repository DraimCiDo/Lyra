# Универсальные проекты с нуля и документация

Статус: в работе. Исходная main a7dfa3a. Пользователь поручил реализовать
создание проектов/изучение документации и уточнил, что задачи бывают разными.
Работа в main; ранее разрешены push/объединение основной разработки в main.

## Результат и границы

Универсальный AI blueprint → предпросмотр → явное создание новой папки;
проверенные стартовые каркасы помогают начать без модели. Профиль/версии,
требования, допущения и вопросы фиксируются до записи. Для существующих проектов
добавляются предложения новых файлов. Rust, Java/Kotlin, Paper/Spigot и DayZ —
первые профили; generic AI blueprint не ограничен ими.

Документация: ограниченные локальные packs, явный HTTPS fetch официальных источников,
поиск фрагментов с provenance/version/hash, доступ модели через search_docs.
Промпты/код не отправляются; inference остаётся локальным. Удалённые данные —
недоверенный контекст, они не расширяют инструменты и права.

Произвольный shell модели не включается. Внешние проекты могут исполнять build.rs,
Gradle/Maven plugins и другой код: пока нет OS sandbox, приложение выдаёт статические
проверки и конкретные команды ручной приёмки, а не фиктивный PASS компиляции.
Каркасы проверяет разработчик/CI реальными компиляторами на временных проектах.
Ни равенство GPT 6.1 Sol, ни качество 7B не обещаются без общего benchmark.

## Слои и решения

- project: профиль/версии/детекция, строгий blueprint, AI grammar, каркасы,
  создание только отсутствующей папки, без перезаписи существующего проекта.
- knowledge: локальное хранение, SHA256/provenance, ограниченный retrieval,
  отдельный optional online-docs feature с reqwest/Tokio и HTML-to-text.
- workspace/agent: explicit propose_file + search_docs, v1 legacy edit support,
  новая схема отчёта для создания файлов; read-only роли не получают запись.
- desktop: компактный мастер нового проекта и раздел документации; CLI worker
  сохраняет границы вывода/отмены и UI. Скачивание — отдельная явная кнопка.
- Новые данные не являются инструкциями; материалы не загружаются на старте и
  не обновляются фоновым таймером. Запросы к внешнему API модели не появляются.

## Приёмка

- [x] Generic blueprint планируется моделью без записи; вопросы блокируют создание.
- [x] Явное создание новой папки, Unicode parent; существующие файлы/папки не меняются.
- [x] Новые файлы в существующем проекте: предпросмотр, конфликт/ignore/path guards.
- [x] Локальная документация и model search_docs с источником/версией; ограничение объёма.
- [x] Явный official fetch: HTTPS/allowlist/лимиты/таймаут; offline по умолчанию.
- [x] Каркасы Rust/Paper/Spigot Java/Kotlin/DayZ; честная внешняя приёмка.
- [ ] GUI/CLI, backward-compatible prefs/reports, native Windows/Linux tests/build.
- [ ] Документы/карта/результаты/commit/push main.

## Исследование

Paper docs project-setup и Spigot Maven открываются. Bohemia wiki возвращает
403 из среды: нельзя выдавать страницу за изученную; нужен локальный SDK/import.
Maven Central metadata: compiler 3.16.0 (stable, не 4 beta), shade 3.6.2,
Kotlin 2.4.21 stable (не 2.5 beta). HTML-to-text crate html2text 0.17.4 проверен
по crates.io metadata, default features пустые; отдельный optional parser.
JDK 21 установлен локально. Maven CLI пока отсутствует; возможна локальная
установка в scratch/CI для проверки каркасов, не runtime-зависимость Lyra.

## Реализация и локальная приёмка

Код реализован: CLI project plan/starter/create/inspect и docs sources/import/fetch/search;
универсальный model blueprint и исследование search_docs, отдельное создание после
просмотра. GUI master и knowledge settings, 21 Lucide icon, RU/EN, 72 native views.
Task имеет search_docs/propose_file, отчёт v2 с реально переданными источниками,
входящие v1 обычных edits читаются. Убрано дублирование содержимого proposals в history.
Обязательные пути проверяются и ограничивают native grammar; коррекция ограничена
общим бюджетом вызовов. Пустой __init__.py поддерживается. Shell модели не добавлен.

Локально Debian 13 / Rust 1.99.1: полный набор tests, Clippy -D warnings,
CLI-only check, две явные tiny GGUF проверки (включая scoped project grammar),
72 native views Xvfb/Mesa. Финальный полный повтор фиксируется ниже после последних правок.
Official fetch через настоящий Rust client: Rust Book и Paper/JavaPlugin 1.21.11
успешно сохранены/проверены, без модели/кода в запросах.

Каркасы: Rust cargo test --offline; Python 3.11 unittest; Node 24.19 built-in TS test.
Paper/Spigot Java/Kotlin: все 4 Maven verify прошли с полным Temurin JDK 21.0.13+9,
Maven 3.9.16 (скачаны в scratch, проверены официальные checksums). Начальный Java runtime
не имел полного SDK/ct.sym, из-за чего --release 21 отклонялся; глобальные настройки
не менялись. Proxy settings использованы только в scratch, сеть через sidecar.
JAR проверены: Main.class major 65, plugin.yml, Bukkit API не включён, Kotlin runtime
и Apache license присутствуют только в Kotlin вариантах. Сервер/игра не запускались.

Реальный Qwen2.5-Coder 0.5B Q4: запрос нескольких файлов с docs завершился таймаутом
120 с, blueprint/проект не созданы. Однофайловый hello сгенерирован за 8.39 с,
но модель ошиблась в имени файла. После required-file gate + native scoped grammar
получен main.py с print('Hello, Lyra!'); содержимое проверено разработчиком,
явно создан проект и выполнен Python с ожидаемым stdout. Это smoke механики,
не доказательство качества сложных задач/7B/GPT 6.1 Sol.

OmniRoute README исследован: это gateway готовых моделей. В текущую задачу не
добавляются внешние model providers, автоматическая смена модели или cloud fallback.
Публичные релизы не публикуются; пользователь разрешил main, не публикацию release.

Перед CI: 69 ordinary tests Linux (последний добавленный profile-hint тест отдельно),
Clippy -D warnings, CLI-only check, native GBNF 2/2, 72 native GUI views прошли.
Windows count ожидается на один меньше из-за platform-specific CLI/symlink/updater tests.
Точные результаты main CI и артефактов ещё не получены.
