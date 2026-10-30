# Универсальные проекты с нуля и документация

Статус: реализовано, нативная приёмка получена. Исходная main a7dfa3a. Пользователь поручил реализовать
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
- [x] GUI/CLI, backward-compatible prefs/reports, native Windows/Linux tests/build.
- [x] Документы/карта/результаты/commit/push main.

## Исследование

Paper docs project-setup и Spigot Maven открываются. Bohemia wiki возвращает
403 из среды: нельзя выдавать страницу за изученную; нужен локальный SDK/import.
Maven Central metadata: compiler 3.16.0 (stable, не 4 beta), shade 3.6.2,
Kotlin 2.4.21 stable (не 2.5 beta). HTML-to-text crate html2text 0.17.4 проверен
по crates.io metadata, default features пустые; отдельный optional parser.
Для проверки каркасов в scratch установлены полный JDK 21 и Maven CLI;
они не являются runtime-зависимостями Lyra. Результаты приведены ниже.

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
72 native views Xvfb/Mesa. Полный набор окончательной ревизии проверен в CI ниже.
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

## Окончательная нативная приёмка · 2026-10-30

Проверенный код: [ca031a9](https://github.com/DraimCiDo/Lyra/commit/ca031a9b713031b224838cd0f6a1f09ed873d7f2).
[Запуск main CI 41099862697](https://github.com/DraimCiDo/Lyra/actions/runs/41099862697).
Результаты получены из полных журналов обоих jobs и скачанных артефактов этой
ревизии. Следующий коммит меняет только Markdown и не меняет проверенные бинарники.
На 18:04 UTC GitHub API всё ещё возвращает `in_progress` для общего запуска и
устаревшее состояние Clippy, хотя полные журналы уже содержат успешные проверки,
публикацию артефактов и завершение cleanup обоих runners. Итоговый API-статус
`success` на этот момент не подтверждён; таблица ниже фиксирует результаты
конкретных проверок, а не подменяет состояние сервиса GitHub.

| Проверка | Ubuntu 24.04.5 x64 | Windows Server 2022 10.0.20348 x64 |
| --- | --- | --- |
| Format / Clippy desktop all-targets -D warnings | прошли | прошли |
| Обычные тесты | 69 passed, 0 failed | 68 passed, 0 failed |
| Явные native tiny GGUF / grammar тесты | 2 passed | 2 passed |
| Release CLI / desktop, doctor | прошли | прошли |
| Встроенные Python / TypeScript тесты | прошли | прошли |
| Paper/Spigot Java/Kotlin Maven verify и проверка JAR | все 4 прошли | все 4 прошли |
| Оконный GUI smoke: 72 состояния | прошёл, Xvfb | прошёл, нативный EXE |
| Упаковка / notices / загрузка артефактов | прошли | прошли |

Каркасы проверялись с полным Temurin JDK 21 и Node 24. Native GUI smoke проходит
4 темы × 2 языка × главную/7 разделов настроек/мастер проекта. Часть тестов зависит
от платформы, поэтому суммы обычных тестов различаются. Это Windows Server CI,
не Windows 11 и не видеокарта пользователя; CUDA и полноценная 7B остаются непроверенными.

Артефакты доступны 14 дней, не являются опубликованным релизом:

- [Windows portable](https://github.com/DraimCiDo/Lyra/actions/runs/41099862697/artifacts/13000845146).
- [Windows update assets](https://github.com/DraimCiDo/Lyra/actions/runs/41099862697/artifacts/12998164832).
- [Linux portable](https://github.com/DraimCiDo/Lyra/actions/runs/41099862697/artifacts/12998463701).

Скачанные Linux и Windows update artifacts совпали с SHA256 GitHub. В Windows
проверены CRC ZIP, версия `0.2.0-dev.4`, все размеры/хеши manifest, source SHA,
PE AMD64 и GUI/console subsystems, 7 размеров иконки, notices и новые инструкции.
Производственный Rust `updater::package::unpack` также принял настоящий Windows
пакет и проверил payload. Это проверка чтения пакета на Linux, не установка Windows.

Внутренний `lyra-windows-x86_64.zip`: 14 685 174 байта,
SHA256 `8edd4300c45e04d72f54ee77674470cb9d6595324a1a20dfc1f6939136f0c138`.
GUI EXE: 24 151 040 байт; CLI EXE: 11 446 784 байта. Модели/Java/Node/Python
в поставку не включаются. GGUF подготавливается отдельно; компилятор/SDK нужен
целевому проекту. Игра/сервер для Minecraft/DayZ не запускались.

Следующие улучшения предложены в [roadmap](../ROADMAP.md): изоляция запуска
проверок, общий benchmark качества, измерение CUDA/памяти и более точный контекст.
Они не включены в завершённый объём этой задачи.
