# Lyra: проверяемые локальные workflow сложных проектов

Статус: в работе. Ветка `feat/quality-workflows`, база `9aaf585`.
Поручение: реализовать согласованный план качества, а не только документацию.

## Цель и решения

Windows-first Desktop/CLI, локальная модель в приоритете; целевой ПК Ryzen 7
5700X / RTX 4060 8 GiB / RAM 32 GiB. Качество важнее скорости. Docker/WSL2
допустимы для изолированных инструментов. Облачная передача только по настройке.
Направления: Rust/native, Vue/TypeScript, backend, Python/боты,
Java/Kotlin/Minecraft, DayZ. Проверки SDK/игры не заменяются компиляцией Lyra.

## Последовательность и критерии

- [ ] 1. Воспроизводимый eval harness: фиксированные исходники, независимые
  проверки, 6 задач на каждое направление, три прогона, локальные метрики.
- [ ] 2. Надежный протокол: повторные точные правки одного файла относительно
  текущего предложения, отмена предложения, восстановление после неверного JSON,
  обнаружение отсутствия прогресса, проверяемые статусы результата.
- [ ] 3. Отдельная рабочая копия/worktree, checkpoints/resume, сохранность dirty
  файлов, проверка всех конфликтов до публикации, журнал восстановления.
- [ ] 4. Типизированный executor и Docker backend с закрепленными образами,
  изоляцией файлов/сети/процессов; Windows-only SDK через выделенную VM.
- [ ] 5. Индекс/поиск, вложенные инструкции, token budget, summaries с provenance;
  исправление chat guard относительно реально выбранного контекста.
- [ ] 6. Сравнение трех GGUF на целевом ПК, постоянный управляемый inference
  worker, замеры/OOM/отмена без скрытой подмены модели.
- [ ] 7. Профильные команды и реальные проверки всех направлений, browser
  acceptance веб-проектов, отдельная игровая/серверная приемка.
- [ ] 8. Общий SessionController CLI/Desktop, resume/review/apply, разрешения
  сессии, rules/scopes, затем MCP/hooks; CPU/CUDA поставка и миграции.

Пороги локальной приемки: 90% малых исправлений, 80% multi-file,
70% новых функций/длинных задач; отдельно по направлению. Потеря данных,
выход за права, ложное test evidence и невосстановимая сессия блокируют этап.

## Реализованный первый инкремент

- Mutable pending edits и единый read/search/list overlay, discard, два bounded reviewer repair
  rounds, syntax recovery, no-progress с учетом изменения содержимого.
- Отчет v4 с outcome; legacy v1–v3 apply; успешный настроенный verify связан
  SHA256 с предложениями и конфигурацией. Изменение кода/проверки инвалидирует proof.
- Явные CLI `verify` / `task --check-config`; Docker без сети и host execution,
  private staging, read-only source/root, non-root, seccomp/no-new-privileges,
  memory/swap/PID/CPU preflight, bounded stdout/stderr и cleanup/watchdog.
- Опциональные `--session-db` / `--resume`, immutable selected-source snapshot,
  checkpoint до model call и после tool outcome, optimistic revision guard,
  CLI show/export/delete; конфликт original при apply сохраняет внешние правки.
- Desktop читает v4; chat guard использует выбранное окно истории и принимает
  естественное описание проекта; временный worker-каталог private на Unix.
- Local eval harness с SHA256 EXE/GGUF/suite/source/checker, повторными прогонами,
  baseline gate и независимой проверкой после apply. Пока один Python case.
- Linux CI дополнен реальными Docker regressions; developer harness тестируется
  на обеих платформах. Программа целиком не завершена.

## Фактические локальные проверки

Среда: Linux cloud, Rust 1.99.1; без нативной Windows и NVIDIA GPU.
Rust установлен в профиль пользователя без изменения PATH; CMake взят из
официального PyPI wheel в scratch. Системный libclang19 присутствовал, headers
для bindgen доступны через GCC include; системные пакеты не менялись.

Выполнены fmt, Clippy all-targets с desktop,gateway-http и `-D warnings`,
полная матрица tests с этими features: 160 passed, 0 failed, 7 ignored.
Из ignored отдельно выполнены 2 real CPU inference tests и 1 Docker preflight
regression; остальные не объявляются пройденными. Добавлены отдельные проверки
CLI → loopback HTTP → checkpoint → restart/resume → export, CRLF/Unicode,
concurrent checkpoint и private permissions.
Реальный CPU GGUF smoke: 2 ignored inference-теста прошли с закрепленным
stories260K (SHA256 270cba1bd5109f42d03350f60406024560464db173c0e387d91f0426d3bd256d).
Это проверка runtime/грамматики, не coding quality.
Python harness: 3 unit-теста; diff и новые относительные ссылки проверены.

До добавления strict preflight реальный Docker прогон подтвердил read-only,
network denial, bounded output, cancel/timeout/cleanup и test-fail → fix →
test-pass, включая отказ устаревшему proof. Затем `docker info` показал
MemoryLimit=false / PidsLimit=false / CgroupDriver=none: прежний прогон НЕ
доказывает resource isolation. Финальный backend намеренно блокирует этот
executor. Отдельный live preflight regression проходит; полный положительный
sandbox-прогон требует CI/daemon с enforced cgroups, bypass не добавлен.

Локальные команды Cargo использовали:
`CMAKE=/workspace/scratch/lyra-tools/cmake/data/bin/cmake`,
`BINDGEN_EXTRA_CLANG_ARGS=-I/usr/lib/gcc/x86_64-linux-gnu/14/include`,
`CARGO_BUILD_JOBS=4`, `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`.
Эти пути описывают данное окружение, не требования Windows-поставки.

## CI первого инкремента

На `b5a60cb` [CI](https://github.com/DraimCiDo/Lyra/actions/runs/41790867762)
подтвердил Linux fmt/Clippy, default/HTTP tests, CPU GGUF smoke и полный
положительный Docker-прогон с enforced cgroups, включая memory/PID stress.
Windows default Clippy/tests прошли; HTTP Clippy обнаружил large_enum_variant
из-за Windows layout TaskArgs (408 bytes). Task-вариант CLI перенесен в Box,
без изменения аргументов или отключения lint. После исправления локальные
Clippy и 160 tests снова прошли. Native CI исправленного head требуется отдельно;
предыдущая CUDA compilation не означает GPU runtime acceptance.

## Открытые шаги и передача состояния

Не завершены: корпус 36 задач и реальные model evals, Git worktrees/multi-file
publication/recovery journal, Windows VM/SDK executor, индекс больших репозиториев,
token-aware context/summaries, постоянный inference-worker и сравнение трех GGUF,
профильные adapters/browser/game acceptance, Desktop resume/permissions/rules/MCP.
Текущие 128 файлов / 512 KiB чтения явно остаются ограничением alpha.

Следующие действия: Windows/Linux CI текущего commit с реальными Docker tests,
затем расширить независимый корпус и снять local-model baseline на целевом ПК.
Для GPU/Windows SDK приемки это окружение не подходит: доступного native runtime
и репрезентативных GGUF здесь нет. Не подменять эти gates tiny smoke или API.
