# Обновления приложения и перенос в main

Поручение: updater как в ReplayLite; после завершения объединить всю работу в main.
Работа продолжается в текущей feat/compact-desktop (общий GUI, прогретый CI cache).
ReplayLite 6a7dc0c: GitHub Releases, SHA256, отдельный helper, сохранение данных,
проверка каждые 5 минут и кнопка возле шестерёнки. C# код не копируется.

- [x] Строгий release/package protocol Windows x64 CPU, semver stable/preview,
      HTTPS GitHub, лимиты/таймаут/отмена/индикатор скачивания, SHA256 и ZIP guards.
- [x] Helper из копии текущего EXE: дождаться именно родителя, backup/replace,
      rollback при ошибке, version probe, перезапуск, результат/уборка кеша.
- [x] Кнопка Lucide рядом с настройками, отдельный раздел, RU/EN, auto check toggle.
      Скачивание/установка явные; установка после завершения/отмены задачи.
- [x] Генерация update ZIP/manifest/checksum в CI, подготовка draft release по tag.
      Публикация релиза отдельно; действующие релизы не изменять автоматически.
- [x] Offline unit/integration: версии/URL/ZIP/hash, испорченный пакет, отмена,
      parent wait, настоящее обновление/перезапуск и locked-file rollback в Windows.
- [x] CI/артефакты/документация; merge в main разрешён пользователем, проверить main CI.

Контракт: управляются только lyra-desktop.exe, lyra.exe, THIRD_PARTY_NOTICES.txt,
VERSION и manifest. Проекты, GGUF, примеры/документы пользователя и AppData-настройки
не заменяются. Проверять исходные хеши; не перезаписывать изменённый пакет.
Сеть только для updater; inference остаётся локальным, CLI не проверяет обновления.
SHA с того же GitHub release обеспечивает целостность, не независимую подпись.
Нет обещания crash-atomic multi-file update при отключении питания; backup сохраняется
при неполном rollback. Запрет shell модели сохраняется, helper использует Command args.

Новые optional desktop deps: reqwest 0.12/rustls native roots + Tokio для отменяемого
HTTP, semver, sha2, ZIP с deflate. Native CRT/модели не скачиваются автоматически.

## Локальная проверка перед main

Debian 13 x64 / Rust 1.99.1: fmt, Clippy -D warnings, 46 обычных tests,
2 явных native GGUF tests, нативное окно 56 состояний через Xvfb/Mesa.
Реальные helper success/failed-health rollback с исполняемыми фикстурами:
личные файлы сохранены; живой PID родителя удерживал установку до выхода.
Отдельный тест проверяет version probe обоих настоящих entry points (GUI/CLI),
включая отказ при несовпадении версии. Обычный CLI не принимает --worker.
CI packager также проверяет версии обоих готовых release EXE.
Windows locked-file test выполняется только в нативном Windows CI.
Релиз не опубликован; draft workflow подготовлен по tag, main push релиз не создаёт.

CI обнаружил превышение 15-секундного handshake в Linux debug-файлe с DWARF
(свыше 400 MiB). Тестовая копия реального helper теперь проходит strip --strip-debug,
как release-поставка; код и production timeout не меняются. Локально та же группа
6 updater-тестов: 22.04 с до, 8.33 с после. Нативный Windows test job первой ревизии
прошёл все тесты, включая locked CLI rollback; финальный main run проверяется отдельно.
Обновлены фактические native Linux скриншоты dev.3, без модели/личного проекта.

Дополнительная проверка без изменения продукта: маленький внешний Rust harness
в scratch вызвал реальный `network::check(true)` — HTTPS/system trust работают,
GitHub вернул None (новых опубликованных Releases нет). Реальный Windows ZIP
первой ревизии принят тем же production `package::unpack`, без исполнения PE в Linux.
Независимо проверены outer ZIP, sidecar SHA256, CRC, все четыре file hashes/sizes,
VERSION, PE AMD64 + GUI/CUI subsystem и наличие инструкции VC++ Runtime.
Python packager отдельно проверен с native Linux EXE: version probes и весь
протокол ZIP/manifest/checksum; это не считается нативной Windows-проверкой.
CLI-only cargo check --no-default-features --all-targets и проверка dependency tree
также прошли. Финальный Windows asset main должен быть проверен отдельно ниже.

## Итог main · 2026-10-30

Вся цепочка feat/local-cli → local-inference → testable-agent → desktop-app →
compact-desktop перенесена в main fast-forward, без удаления веток/истории.
Код: `ff80826cce8598cfd93c9aa1b021c903ac66dcca` (updater + guards + test fix).
[Main CI 41069559616](https://github.com/DraimCiDo/Lyra/actions/runs/41069559616)
полностью успешен на Ubuntu 24.04 и Windows Server 2022 / MSVC / Rust 1.99.1.
На каждой ОС: 46 ordinary tests + 2 явных native GGUF tests, format, Clippy -D warnings,
release обоих EXE, doctor, настоящее окно 56 состояний. Windows updater-группа:
7/7, включая удержание живого родителя, restart, failed-health rollback,
locked CLI rollback и version probes настоящих CLI/GUI. Linux updater: 6/6.

[Windows update artifact](https://github.com/DraimCiDo/Lyra/actions/runs/41069559616/artifacts/12984884841)
скачан и независимо проверен; outer SHA256
`cd03c14f3fcebad18f80d932e5a5979920062a782b52a6e5d8c9284636805e53`.
Внутренний ZIP: 12 251 402 байта, SHA256
`052e7250aaea99feb62ae97269359df0bb3e341bd7700b97f9939af0ca759770`.
GUI: 22 326 784 байта, SHA256
`6f51753f255d0fdf82b294eb3b1b3b73eabdb10b2c3b87c3d5260791bf27a46e`.
CLI: 6 908 928 байт, SHA256
`fe08cdbd81fce9aaa888b4ee1bb9685a72001ebc70abba622a1f8094754ace96`.
Проверены CRC/manifest/VERSION/все file hashes, AMD64 GUI/CUI, семь RT_ICON,
RT_GROUP_ICON/RT_VERSION, notices/VC++ instructions/examples; моделей нет.
Окончательный ZIP также принят production Rust package reader.
Отдельный [Linux artifact](https://github.com/DraimCiDo/Lyra/actions/runs/41069559616/artifacts/12983944795).
Артефакты Actions хранятся 14 дней. Последующий коммит меняет только документацию.

Границы: настоящий публичный release → release по сети ещё не проверен
(релизы не публиковались); install/rollback проверены реальным helper с нативными
исполняемыми фикстурами. Windows 11 на ПК пользователя, CUDA/7B, системные диалоги
и многочасовая приёмка качества остаются отдельными проверками. Кодинг-агент — alpha,
а не подтверждённая замена Codex/Claude Code. Draft release job на main пропущен
по условию, что соответствует политике отдельной публикации.
