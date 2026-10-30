# Настольная Lyra: реализовано

Задача: нативное окно Windows с выбором проекта/GGUF, вводом задачи,
настройками, отзывчивым запуском/отменой, просмотром отчёта и явным apply.
CLI и ограничения существующего движка сохранены.

- [x] GUI, worker lifecycle, просмотр предложений и явное применение.
- [x] Регрессии: oversized output, отмена, ошибка модели, неизменность исходников,
      настоящий apply/конфликт, headless отрисовка и нативный запуск окна.
- [x] Windows/Linux CI с desktop feature, EXE/лицензии/SHA256 и инструкция.
- [x] fmt/Clippy/tests, локальное окно, Windows native build, commit/push.

## Решения и границы

Rust + eframe/egui: wgpu/DX12 на Windows, GL на Linux, без web runtime.
rfd — системные диалоги, tempfile — приватный временный каталог. Optional feature
`desktop`; CLI без него не подключает GUI dependencies (проверено cargo tree).
EXE запускает себя в `--worker`: не нужны shell, сервер, Node/Python или соседний CLI.
Одна задача одновременно, stdout до 2 MiB, stderr — последние 64 KiB.
Отмена/закрытие завершает task-worker и очищает temp после завершения pipes.
Apply нельзя прерывать посередине записи. Cwd worker — его собственный temp,
аргументы/пути передаются напрямую без shell. Настройки/история не сохраняются.

Windows требует Microsoft Visual C++ v14 x64 Runtime: PE imports подтверждают
MSVCP140.dll/VCRUNTIME140.dll/VCRUNTIME140_1.dll. DLL не включены в ZIP;
официальная ссылка есть в README поставки и docs/DESKTOP.md. Не заявляется
полностью автономная поставка без установленного VC Runtime.

GPU inference/CUDA, качество 7B на пользовательском ПК, произвольный shell,
установщик/подпись/автообновление и OS sandbox не реализуются этой задачей.
Системные диалоги и Windows 11/DPI пользователя требуют ручной приёмки.
Инструкция: [DESKTOP.md](../DESKTOP.md), сценарий coding: [ACCEPTANCE.md](../ACCEPTANCE.md).

## Проверки

Локально Debian 13: fmt/Clippy и `cargo test --workspace --features desktop --locked`
проходят. 14 unit + 9 agent + 6 CLI + 3 desktop + 3 inference = 35 обычных тестов;
отдельно ignored: два helper-процесса и два native GGUF smoke. Проверены
отмена/таймаут/Drop job/очистка temp, переполнение stdout, headless layout,
права apply, ошибка модели, Unicode/пробелы в пути через настоящий EXE-worker.

В Xvfb/Mesa проверены native GUI smoke, вкладка «Мой компьютер» → отдельный worker
→ отображение диагностики, размер 900×650 с прокруткой и обычный WM_DELETE_WINDOW
(exit 0). Скриншот README снят с настоящего Linux-окна, не макет и не Windows.
Принудительный XDestroyWindow из xdotool windowclose вызвал panic в winit/X11;
этот способ разрушает окно сервером и не используется обычным закрытием/smoke.

Исправлены два дефекта проверки: headless harness явно освобождает font texture
updates без renderer; Linux CI устанавливает libxkbcommon-x11-dev, отсутствие
которого обнаружил первый native smoke. Проверки не отключались.

Один замер покоя после doctor без модели: debug Linux, Xvfb/Mesa, 2 секунды,
0 CPU ticks (CLK_TCK=100), VmRSS/VmHWM 101132 KiB (~98.8 MiB). Это короткий
замер без baseline; расход Windows/release/7B из него не выводится. В покое нет
таймера постоянной перерисовки; для активной задачи UI обновляется через 250 ms.

## Принятая ревизия и артефакты

Source: **d7392e07fc601ea39f407e70b367856c549f20d7**.
[CI 41049044429](https://github.com/DraimCiDo/Lyra/actions/runs/41049044429):
**success Windows Server 2022 и Ubuntu 24.04**. Пройдены fmt/Clippy/tests,
два реальных CPU GGUF smoke, release обоих бинарников, запуск CLI,
открытие/отрисовка/закрытие настоящего GUI и упаковка уведомлений лицензий.
Это не проверка качества решения кодинг-задач моделью.

[Windows ZIP](https://github.com/DraimCiDo/Lyra/actions/runs/41049044429/artifacts/12973603208):
10 422 111 байт; SHA-256
`d9182272820d52affccfbde9233530dc5e4f2cd85df21b139a9c35eee506d4ad`.
Linux artifact: 12973379085. Артефакты хранятся до 2026-11-13.

Windows ZIP скачан через GitHub connector и проверен локально: SHA ZIP,
оба бинарника против SHA256SUMS.txt, PE AMD64/GUI subsystem, состав архива,
source stamp, VC Runtime prerequisite, пять font notices и относительные ссылки
всех Markdown. Windows-пути в notices используют обратный слеш.

| Бинарник | Размер | PE subsystem | SHA-256 |
| --- | --- | --- | --- |
| lyra-desktop.exe | 17 650 176 | GUI (2) | ebe678a39d6b3418691afd3f5fcd6d45aebe8ddaf850971434f8d90f4bc5f341 |
| lyra.exe | 6 908 416 | CUI (3) | e7757b429d36a09220dfadd91d36eab98e8652730eb8ab90e9d0846714ad32fe |

Запуск Windows EXE в локальном Linux не заявляется: исполнение подтверждено
нативным Windows CI. Архив не содержит модели, пользовательских профилей или логов.
Следующий самостоятельный этап — ручная приёмка на Windows 11 с целевым GGUF.
