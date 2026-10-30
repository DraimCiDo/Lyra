# Источники и ориентиры

Просмотрено при подготовке основы: **2026-10-30**. Это материалы для проектирования;
их поведение и зависимости не переносятся в Lyra автоматически.
Перед реализацией повторно сверять актуальный контракт API и лицензии.

## Наши примеры

- [ReplayLite — AGENTS.md](https://github.com/DraimCiDo/ReplayLite/blob/main/AGENTS.md)
  и [README](https://github.com/DraimCiDo/ReplayLite/blob/main/README.md):
  последовательность работы, внимание к ресурсам, доказательства проверок.
- [DraimCiDoMOD — AGENTS.md](https://github.com/Server-DraimCiDo/DraimCiDoMOD/blob/main/AGENTS.md)
  и [README](https://github.com/Server-DraimCiDo/DraimCiDoMOD/blob/main/README.md):
  границы задачи, сохранение чужой работы, разделение реализации и фактической приёмки.
  Доступ к этому примеру ограничен правами GitHub.

Взяты общие принципы. Игровые правила, правила записи видео, ограничения старых
сред и разрешения на релизы других проектов здесь не действуют.

## Агенты для кодинга

| Источник | Что изучено и пригодится |
| --- | --- |
| [OpenAI Codex](https://github.com/openai/codex), [Rust workspace](https://github.com/openai/codex/blob/main/codex-rs/Cargo.toml) | Локальный агент, Rust-кодовая база; ориентир для разделения ядра, CLI и исполнения. Масштаб workspace копировать не нужно. |
| [Claude Code: overview](https://code.claude.com/docs/en/overview) | Пользовательский процесс: работа с репозиторием, правками и командами из среды разработчика. Это документация продукта, не основание считать его код доступным для копирования. |
| [Aider](https://github.com/Aider-AI/aider) | Отбор контекста проекта и понятная интеграция изменений с Git. |
| [OpenCode](https://github.com/anomalyco/opencode) | Ориентир для взаимодействия в терминале и разделения режима анализа/изменений. |

На этом этапе прочитаны README и указанные документы. Аудит реализации,
оценка защищённости и сравнение производительности этих проектов не выполнялись.

## Understand-Anything и продуктивная работа с репозиторием

По предложению пользователя изучены [README](https://github.com/Egonex-AI/Understand-Anything),
[инструкции разработки](https://github.com/Egonex-AI/Understand-Anything/blob/main/CLAUDE.md),
[модель графа](https://github.com/Egonex-AI/Understand-Anything/blob/main/understand-anything-plugin/packages/core/src/types.ts),
[fingerprints](https://github.com/Egonex-AI/Understand-Anything/blob/main/understand-anything-plugin/packages/core/src/fingerprint.ts)
и [аудит достоверности данных](https://github.com/Egonex-AI/Understand-Anything/blob/main/docs/incremental/source-evidence-audit.md).
Просмотренная ревизия дерева: `4e6d3299017a7432da15957874992837a315c5b4`.

В Lyra применены принципы: карта точек входа, анализ влияния изменений,
проверка фактов по исходникам и обновление контекста по изменившимся файлам.
Схема данных или успешный парсинг не доказывают полноту знаний. Изменение тела
функции без смены сигнатуры требует проверки поведения, даже если структура прежняя.

На текущем этапе это [карта](PROJECT_MAP.md) и правила в AGENTS.md.
Tree-sitter, автоматический граф, dashboard и pipeline AI-помощников не установлены
и не реализованы. Код проекта не копировался; его MIT-лицензия проверена как
справочная информация, а не автоматически выбранная лицензия Lyra.

## OpenAI и Windows

- [Responses API: migration guide](https://developers.openai.com/api/docs/guides/migrate-to-responses):
  база для выбора первого API и проверки формата запросов/состояния.
- [Function calling](https://developers.openai.com/api/docs/guides/function-calling):
  протокол вызова инструментов и требования строгих JSON-схем.
- [Codex: Windows sandbox](https://developers.openai.com/codex/windows/):
  ориентир для изучения нативных границ файлов, сети и процессов. Это описание
  Codex, а не готовое решение по sandbox для Lyra.
- [Rust: MSVC prerequisites](https://rust-lang.github.io/rustup/installation/windows-msvc.html):
  требования компилятора и Windows SDK для будущей нативной сборки.

Проверка этих материалов не означает, что API-интеграция или Windows sandbox
уже реализованы. Конкретные модели, тарифы, схемы и совместимость SDK проверяются
при реализации соответствующего этапа.

## Уточнение M1: локальная работа вместо OpenAI-first

Пользователь изменил требование: AI работает на ПК без обязательного внешнего
сервиса. Материалы OpenAI выше остаются историческими ориентирами, а не планом
облачной зависимости.

- [Rust stable manifest](https://static.rust-lang.org/dist/channel-rust-stable.toml):
  при M1 проверена версия 1.99.1 от 2026-10-15, она закреплена в toolchain.
- [clap](https://docs.rs/clap/4.6.7/clap/),
  [sysinfo](https://docs.rs/sysinfo/0.40.0/sysinfo/),
  [serde_json](https://docs.rs/serde_json/1.0.154/serde_json/):
  API сверены с загруженными исходниками закреплённых зависимостей.
- [llama.cpp](https://github.com/ggml-org/llama.cpp): кандидат локального runtime
  для следующего этапа; в M1 не встроен.
- [Официальный Qwen2.5-Coder-7B-Instruct GGUF](https://huggingface.co/Qwen/Qwen2.5-Coder-7B-Instruct-GGUF/tree/main):
  проверены файл Q4_K_M около 4,68 GB и лицензия Apache-2.0.
  Это данные весов, не замер их VRAM/скорости на пользовательском ПК.

## M2 — встроенный локальный runtime

- [llama-cpp-2 0.1.160](https://docs.rs/llama-cpp-2/0.1.160/llama_cpp_2/):
  API сверен с исходниками закреплённого crate (vocab, batch, context, sampling).
- [Исходная ревизия binding](https://github.com/utilityai/llama-cpp-rs/tree/0dd5814c9b061930965d144c1653cd4d05054873):
  лицензии MIT/Apache-2.0 сохранены в `third_party/llama-cpp-rs`.
- [ggml-org tinyllamas](https://huggingface.co/ggml-org/models/tree/main/tinyllamas):
  небольшой тестовый GGUF для CPU smoke, не модель кодинга и не часть поставки.
