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
- [ ] CI/артефакты/документация; merge в main разрешён пользователем, проверить main CI.

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
