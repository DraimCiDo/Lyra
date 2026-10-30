# Разработка и проверки

## Текущий этап

Есть Cargo-проект, CLI/doctor, тесты и CI Linux/Windows. Команды ниже применимы
к текущему checkout. Локальный CPU inference и alpha task/apply реализованы; произвольные команды модели отсутствуют.

## Окружение Windows

1. Git for Windows и PowerShell; Windows 11 x64.
2. Rust через [rustup](https://rustup.rs/), host/target `x86_64-pc-windows-msvc`.
3. MSVC C++ build tools и Windows SDK согласно
   [официальным требованиям Rust](https://rust-lang.github.io/rustup/installation/windows-msvc.html).
   Использовать подходящую пользователю редакцию Visual Studio / Build Tools.
4. Версия Rust **1.99.1** и `rustfmt`/`clippy` закреплены в `rust-toolchain.toml`;
   зависимости закреплены в `Cargo.lock`. CI использует `windows-2022` MSVC.

Команды проверки установленного окружения, которые не собирают приложение:

```powershell
git --version
rustup show active-toolchain
rustc --version
cargo --version
```

Целевая пользовательская система — Windows 11 x64; совместимость остальных
версий не заявлена. Не устанавливать весь стек для правки Markdown.

## Git и планы

Основная ветка нового репозитория — `main`. Для отдельной задачи предпочитать
короткую ветку `docs/<topic>`, `feat/<topic>` или `fix/<topic>`; существующую работу
продолжать в её ветке. Пример коммита: `docs: prepare repository foundation`.
Перед переключением ветки проверить рабочее дерево и сохранить чужие правки.

План сложной задачи в `docs/plans/` должен содержать:

- задачу и границы, исходное состояние и важные решения;
- шаги со статусом и критериями приёмки;
- применимые проверки, среду, фактический результат и блокеры;
- следующий шаг, если работа прерывается.

План не требует отдельного утверждения для уже порученной работы.

## Матрица проверок

| Изменение | Минимальная проверка |
| --- | --- |
| Документация | `git diff --check`, относительные ссылки, актуальность команд и статуса, итоговый diff. |
| Переносимая логика Rust | Форматирование, Clippy, релевантные тесты, сборка; нативный Windows CI. |
| Локальный inference | Фикстуры/заглушки, поток по частям, ошибки загрузки/OOM и отмена; отдельный прогон на реальной модели. |
| Контекст и патчи | CRLF, Unicode/пробелы в пути, исключения, лимиты, конфликт и отсутствие потери чужих изменений. |
| Исполнение и права | Отказ политики, границы файлов/сети, junction, таймаут/отмена и остановка дерева процессов в Windows. |
| Горячий путь | Те же данные и конфигурация до/после, ресурсы и задержки локальной операции отдельно от API. |

Для документации достаточно diff и проверки ссылок; не добавлять тесты текста.

Базовый набор:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --release --locked --target x86_64-pc-windows-msvc
```

Linux release: `cargo build --release --locked`. Cross-check из Linux:
`rustup target add x86_64-pc-windows-msvc`, затем
`cargo check --workspace --all-targets --locked --target x86_64-pc-windows-msvc`.
Он проверяет компиляцию Windows-кода, но не создаёт EXE без linker/Windows SDK.
Нативную сборку выполняет CI; это не замена проверке драйвера/CUDA на ПК пользователя.

В CI проверяются format/Clippy/tests, release и запуск `doctor --json`.
Артефакты содержат binary, README, Cargo.lock, SHA-256 и тексты лицензий
зависимостей. Actions закреплены по SHA, token имеет только `contents: read`.
`tools/collect_licenses.py` — инструмент упаковки для CI (Python уже есть на
runner); Python не нужен пользователю для запуска Lyra. Стабильный релиз не создаётся.

Тесты M1: help/version, коды ошибок аргументов, локальный JSON/text snapshot,
Unicode/экранирование в отчёте, различение отсутствующих данных и нуля,
передача ошибок вывода. Они не выполняют inference и не проверяют GPU-ускорение.
Диагностика не создаёт длительного задания: нет фоновых процессов и частично
записанного состояния, которое нужно восстанавливать при Ctrl+C.

## Приёмка поведения после реализации

В изолированном временном Git-репозитории задать небольшую задачу с заранее
известным тестом. Проверить чтение инструкций, отбор контекста, diff, отказ
неразрешённой операции, выполнение разрешённой проверки и итоговый отчёт.
Начальные незакоммиченные изменения пользователя должны сохраниться.
Повторить с путями с пробелами/кириллицей и CRLF в нативном PowerShell.

Проверки адаптера выполнять на фикстурах без GPU и скачивания весов.
Сквозной прогон с локальной моделью проводить отдельно на тестовом проекте,
с ограничением RAM/VRAM, контекста и времени. Не включать облачный fallback.

## Измерения ресурсов

При первом CLI снять исходные значения: холодный/повторный старт, CPU в ожидании,
пиковый working set и private bytes, размер EXE и поставки, время поиска/патча,
объём прочитанных данных. После inference добавить время загрузки весов,
prompt processing и tokens/s, VRAM, KV-cache и размер контекста.
Облачные тарифы не участвуют в оценке локального исполнения.

Сохранять ОС, CPU/RAM, версию сборки, модель/настройки, размер тестового проекта
и несколько измерений с медианой/диапазоном. Численные бюджеты выбрать после
первого воспроизводимого baseline; обещаний превосходства над конкурентами пока нет.

## Релизы

Версия разработки CLI — `0.2.0-dev.4`; артефакт CI не является стабильным релизом.
До стабильного релиза нужны нативная проверка Windows, понятная инструкция,
лицензия/уведомления зависимостей, артефакты и их контрольные суммы.
Не добавлять фиктивный номер версии, ссылку на отсутствующий EXE или значок зелёного CI.

## Сборка встроенного inference (M2)

Кроме Rust нужны CMake, C++ compiler и libclang для bindgen. Linux CI устанавливает
`cmake libclang-dev`; Windows CI использует MSVC и LLVM/libclang из образа runner.
Они нужны при сборке, а не при запуске готового CPU-артефакта.
CUDA — feature `cuda`; нужен совместимый CUDA Toolkit и NVIDIA driver.
Нативная cross-check из Linux теперь также требует Windows C++ headers/SDK:
одного Rust target для C++ runtime недостаточно, используйте native Windows CI.

Обычные тесты не загружают веса. В CI отдельный явный smoke-шаг загружает
маленький GGUF (около 1,2 MB) с закреплёнными revision/SHA-256 и выполняет
генерацию на CPU. Он не требует Qwen 7B или CUDA. Для отдельного CPU smoke-теста задайте
`LYRA_TEST_MODEL` абсолютным путём к небольшой base GGUF, затем выполните
`cargo test --test inference -- --ignored`. Этот тест не измеряет качество 7B.

## Проверки alpha

`tests/agent.rs` проверяет оркестратор через детерминированный backend без LLM:
роли, бюджеты, права, gitignore, обход путей, конфликты, CRLF и цепочку настоящего
Rust-теста до/после применения через CLI. Только test harness запускает Cargo
на доверенном временном fixture; продукт не исполняет модельные команды.
Windows CI отдельно создаёт junction и проверяет отказ доступа.

В native ignored smoke добавлены повторные запросы одной загруженной модели,
изоляция KV-context, грамматика и constrained output. Это не проверка качества 7B.
В CI кэшируются Cargo registry и target (без credentials), чтобы повторные
проверки Rust-оркестратора не требовали каждый раз полного пересоздания C++ runtime.
Пользовательская приёмка: [ACCEPTANCE.md](ACCEPTANCE.md).

## Desktop feature

Интерфейс и нативная приёмка: [DESKTOP.md](DESKTOP.md). Полный CI включает
`--features desktop` в Clippy/tests/release, собирает оба бинарника, открывает
настоящее окно smoke-режима и упаковывает две контрольные суммы.
CLI без feature сохраняет прежний путь сборки. Лицензии поставки собираются
через `python tools/collect_licenses.py --features desktop --target TARGET --output FILE`.
Новые зависимости: eframe/egui (MIT/Apache-2.0, нативный renderer), rfd (MIT, диалоги),
tempfile (MIT/Apache-2.0, временные файлы). Runtime не требует Python/Node/WebView.

Компактный UI 0.2.0-dev.4: проверяются 4 темы × RU/EN × главная/7 настроек/мастер
на 760×480 и 640×460, контраст текста ≥4.5, декодирование встроенных SVG/PNG,
ограничения/перезапись файла preferences. Native --smoke-test перебирает 72
состояний без чтения/записи пользовательской конфигурации. Windows SDK нужен
также build-only winresource для значка EXE. Optional egui_extras/svg даёт
локальный SVG renderer; CLI без desktop не подключает UI зависимости.
Конвертер значка разработчика: `python tools/render_icon.py` (Rust + Pillow).
Готовые PNG/ICO закоммичены, обычная сборка приложения Python не требует.

### Portable updater

Контракт, использование и восстановление: [UPDATES.md](UPDATES.md).
Optional reqwest/rustls + Tokio обеспечивают ограниченный HTTPS и отмену;
semver выбирает канал/версию, sha2 проверяет целостность, zip извлекает только
фиксированные файлы. Rustls использует native OS trust, без OpenSSL DLL.
HTTP/Tokio/HTML parser отсутствуют в `cargo tree --no-default-features -e normal`.
SHA256 и tempfile теперь также обслуживают offline packs и создание проектов.
`tools/package_update.py --verify-executables` проверяет версии готовых Windows
EXE и генерирует update assets; Python нужен только разработчику/CI.
Linux helper-тесты используют strip --strip-debug для тестовой копии настоящего
EXE, как release-поставка. Нужен binutils strip; он уже входит в Ubuntu CI.
Обычный push main только собирает артефакты. Тег `v<Cargo version>` после успешного
CI готовит draft release; публикация выполняется отдельно.

### Проекты и документация

[PROJECTS.md](PROJECTS.md), [KNOWLEDGE.md](KNOWLEDGE.md),
[план/результаты](plans/0008-projects-knowledge.md). CLI-only tests не требуют
интернета/SDK/GPU. Полный CI отдельно ставит полный JDK 21 и Node 24, затем
`python tools/verify_starters.py --binary PATH --minecraft` компилирует только
доверенные встроенные каркасы и проверяет JAR/API scope/Kotlin runtime/licenses.
Rust-каркас проходит cargo test --offline в обычном интеграционном тесте.
Maven нужен разработчику для этой приёмки, не пользователю готовой Lyra.
API SNAPSHOT требует сети и может изменяться; Minecraft-сервер/DayZ Tools не
подменяются Rust-тестом. html2text 0.17.4 — optional Rust HTML parser без JS/runtime;
не добавляет браузер/WebView. Default CLI имеет только offline-документацию.
