# Настольная Lyra

Задача: нативное окно Windows с выбором проекта/GGUF, вводом задачи,
настройками, отзывчивым запуском/отменой, просмотром отчёта и явным apply.
CLI и ограничения существующего движка сохраняются.

Стек: Rust + eframe/egui (wgpu на Windows, GL на Linux, без web runtime), rfd для выбора файлов,
tempfile для приватного временного каталога. GUI — optional feature desktop;
CLI без него сохраняет небольшой dependency graph. Отдельный EXE запускает
себя в worker-режиме: не нужны shell, сервер, Node/Python или соседний CLI.
Ресурсы stdout/stderr ограничены; при отмене/закрытии окна процесс task завершается.
Apply нельзя отменять посередине записи. Настройки/код не сохраняются автоматически.

- [x] GUI, worker lifecycle, просмотр предложений и явное применение.
- [x] Регрессии: oversized output, отмена, ошибка модели, неизменность исходников,
      настоящий apply/конфликт, headless отрисовка и нативный запуск окна.
- [x] Windows/Linux CI с desktop feature, EXE/лицензии/SHA256 и инструкция.
- [ ] fmt/Clippy/tests, локальное окно, Windows native build и фиксация доказательств.

GPU inference/CUDA и качество 7B не входят в эту задачу и не становятся проверенными из-за GUI.
Установщик/подпись кода/автообновление и shell sandbox — отдельная работа.

## Проверки перед финальным CI

Локально Debian 13: fmt/Clippy и полный `cargo test --workspace --features desktop
--locked` проходят. 14 unit + 9 agent + 6 CLI + 3 desktop + 3 inference = 35
обычных тестов; отдельно ignored: два helper-процесса и два native GGUF smoke.
Проверены отмена/таймаут/Drop job/очистка temp, overflow stdout, headless layout,
права apply, ошибка модели и Unicode/пробелы в пути через настоящий EXE-worker.
Native GUI smoke в Xvfb/Mesa прошёл. Скриншот снят с настоящего Linux-окна,
не макет и не свидетельство Windows 11/CUDA.

Исправлена ошибка headless harness: без GPU font texture deltas теперь явно
освобождаются; реальный renderer отдельно проверяется native smoke.

CI 41034143462, source 096e6d0: Windows Server 2022 прошёл все шаги, включая
wgpu-окно и release/упаковку. Windows artifact 12968646478 (10 421 740 байт).
Linux прошёл fmt/Clippy/tests/GGUF/release, затем обнаружил недостающую системную
libxkbcommon-x11. Добавлен libxkbcommon-x11-dev; проверка не отключена.
CLI gh не смог загрузить job logs (403 blob storage); лог прочитан через
GitHub connector fetch_workflow_job_logs, точная причина установлена из stderr.
После исправления окружения CI 41047124711 (8f8ebe7) прошёл на обеих ОС,
включая native GUI smoke, release и упаковку. Кэш Linux после успешного прогона
сохранён. Далее проверяется финальная правка явного cwd и уточнение runtime в README.

## Дополнительная локальная проверка окна

Нажатиями в настоящем окне проверена цепочка «Мой компьютер» → «Обновить
диагностику» → отдельный worker → человеческое представление CPU/RAM/VRAM/дисков.
Изменение размера до минимальных 900×650 сохраняет основные элементы и прокрутку.
Обычный WM_DELETE_WINDOW завершает приложение с кодом 0. Принудительный XDestroyWindow
из xdotool windowclose вызвал panic в winit/X11; это разрушение окна X-сервером,
а не обычный запрос закрытия. Этот способ не используется smoke-тестом/продуктом.

Один замер покоя после doctor, без модели: debug Linux, Xvfb/Mesa, 2 секунды,
0 CPU ticks (CLK_TCK=100), VmRSS/VmHWM 101132 KiB (~98.8 MiB). Это короткий
замер одного процесса, без baseline до появления GUI; расход Windows/release/7B
из него не выводится. Перерисовка в покое не запрашивается постоянным таймером;
для активной задачи обновление интерфейса ограничено интервалом 250 ms.

## Проверенный Windows-архив до уточнения cwd

Source: 8f8ebe7634f8b4fd5915c6fba2d19e2c22db372f.
CI 41047124711: Windows job 124033829451 — success, включая настоящее wgpu-окно.
Артефакт: https://github.com/DraimCiDo/Lyra/actions/runs/41047124711/artifacts/12973720241
Размер ZIP 10 421 804 байта, SHA-256
`38ad0f86b28e60969c536225f44e54bd1961046080e11c4bf2b0baacf85a7f96`.

Архив скачан через GitHub connector; локально проверены SHA ZIP и оба EXE
против SHA256SUMS.txt, состав архива, исходная ревизия README, относительные
ссылки всех Markdown и все пять font notices. Windows-пути notices используют
обратный слеш; проверка наличия notices учитывает это.
PE-заголовки: обе программы AMD64; desktop — GUI subsystem 2, CLI — CUI 3.

- lyra-desktop.exe: 17 650 176 байт, SHA-256
  `d49a1ed1aa3668c5915fcd180f920cdff6f2cfc33929e2cca17130e6bb7e8536`.
- lyra.exe: 6 908 416 байт, SHA-256
  `ddf9b130434d93252a6766f02c085b1754121b61b2b28480ca50387aa0eac3a2`.

Исполнение EXE локально в Linux не заявляется: нативный запуск подтверждён
Windows Server 2022 CI. Пользовательские Windows 11/DPI/диалоги/GGUF7B/CUDA
остаются отдельной приёмкой по docs/DESKTOP.md. Артефакт хранится до 2026-11-13.

Проверка PE imports также выявила системную зависимость обеих Windows-программ:
MSVCP140.dll, VCRUNTIME140.dll, VCRUNTIME140_1.dll. В docs/DESKTOP.md и README
поставки явно добавлен Microsoft Visual C++ v14 x64 Runtime с официальной ссылкой.
DLL не копируются из окружения CI и не выдаются за включённые в portable ZIP.
Это требование запуска, а не dependency Rust/Python/Node; Windows CI уже имеет runtime.
Для полностью автономной поставки без установленного VC Runtime потребуется
отдельно проверить static CRT или лицензированную app-local поставку.

Финальная правка: `Job::start_command` задаёт собственный temp как cwd worker,
независимо от запуска из Проводника/ярлыка. Локально fmt/Clippy и все 35 тестов
повторно прошли, включая Drop/cleanup temp. Код коммит ec74504.
CLI без desktop проверен `cargo tree --no-default-features --target
x86_64-pc-windows-msvc --edges normal`: GUI dependencies отсутствуют.
