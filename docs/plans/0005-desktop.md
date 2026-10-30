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
- [ ] Windows/Linux CI с desktop feature, EXE/лицензии/SHA256 и инструкция.
- [ ] fmt/Clippy/tests, локальное окно, Windows native build и фиксация доказательств.

GPU/CUDA и качество 7B не входят в эту задачу и не становятся проверенными из-за GUI.
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
Финальный CI после исправления окружения фиксируется ниже.
