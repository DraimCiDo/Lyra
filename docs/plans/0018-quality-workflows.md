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
- Отмена/deadline повторно проверяются после model/tool вызова: поздний finish
  не завершает отмененную задачу; HTTP CLI использует коды 130/124.
- Отчет v4 с outcome; legacy v1–v3 apply; успешный настроенный verify связан
  SHA256 с предложениями и конфигурацией. Изменение кода/проверки инвалидирует proof.
- Явные CLI `verify` / `task --check-config`; Docker без сети и host execution,
  private staging, read-only source/root, non-root, seccomp/no-new-privileges,
  memory/swap/PID/CPU preflight, bounded stdout/stderr и cleanup/watchdog.
- Опциональные `--session-db` / `--resume`, immutable selected-source snapshot,
  checkpoint до model call и после tool outcome, optimistic revision guard,
  CLI show/export/delete; конфликт original при apply сохраняет внешние правки.
- Desktop читает v4 и показывает controller facts до model summary; chat guard использует выбранное окно истории и принимает
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
полная матрица tests с этими features: 163 passed, 0 failed, 7 ignored.
Из ignored отдельно выполнены 2 real CPU inference tests и 1 Docker preflight
regression; остальные не объявляются пройденными. Добавлены отдельные проверки
CLI → loopback HTTP → checkpoint → restart/resume → export, CRLF/Unicode,
concurrent checkpoint и private permissions.
Реальный CPU GGUF smoke: 2 ignored inference-теста прошли с закрепленным
stories260K (SHA256 270cba1bd5109f42d03350f60406024560464db173c0e387d91f0426d3bd256d).
Это проверка runtime/грамматики, не coding quality.
Python harness: 4 unit-теста; diff и новые относительные ссылки проверены.

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
Clippy и 162 tests снова прошли. Native CI исправленного head требуется отдельно;
предыдущая CUDA compilation не означает GPU runtime acceptance.
На следующем Windows head оба Rust-набора прошли; Python harness выявил
CRLF-преобразование Git. Для byte-pinned evals задан `/evals/** -text`,
добавлен regression checkout-index с core.autocrlf=true без ослабления SHA256.

На `692daae` Windows/Linux CI полностью прошел, включая native Desktop launch,
release/starter acceptance и Linux Docker limits. Последующие изменения UI facts
и приоритета отмены требуют повторного CI. Windows HTTP regression на `1700ec8`
обнаружил наследование nonblocking accepted socket (WSA10035); тест явно
переключает принятый сокет в blocking mode. Это изменение только тестового сервера.
Executor дополнительно проверяет Linux image metadata и блокирует VOLUME вне
переопределенных `/tmp`/`/source`, чтобы не создавать неограниченную writable
область. Локальные fmt, Clippy и 163 tests после обеих правок прошли.

## Реальная coding-проба малой GGUF

Linux cloud CPU, 2 threads; официальный Qwen2.5-Coder-1.5B-Instruct Q4_K_M
из каталога, SHA256 cc324af070c2ecbfd324a30884d2f951a7ff756aba85cb811a6ec436933bb046.
Фикстура examples/buggy-add скопирована с CRLF, исходный cargo test падает.
Task: context 8192, max-tokens 1024, max-calls 12, timeout 600, session-db.
За 36.51 s модель выполнила search(query="symbol") и finish, без предложения.
Протокол completed, outcome analysis_complete, файл не изменен; apply не вызывался.
Это НЕ успешная coding-приемка. По прямому prompt с исходником (тот же GGUF,
context 8192/max-tokens 512/2 threads) за 11.06 s возвращено правильное
left + right; ответ не применялся и не исполнялся. Эти разные запросы не являются
сравнительным benchmark скорости или доказательством качества целевой 9B/RTX.

На основе этой находки Desktop явно показывает отсутствие предложений и
подтвержденных проверок до комментария модели. Это исправляет представление
результата, но не выдает неуспех модели за исправленную coding-надежность.
Временные веса удалены после обоих процессов; приватные отчеты остались в .lyra.

## Открытые шаги и передача состояния

Не завершены: корпус 36 задач и реальные model evals, Git worktrees и полностью автоматическое
byte-level recovery в Windows, Windows VM/SDK executor, индекс больших репозиториев,
token-aware context/summaries, постоянный inference-worker и сравнение трех GGUF,
профильные adapters/browser/game acceptance, Desktop resume/permissions/rules/MCP.
Текущие 128 файлов / 512 KiB чтения явно остаются ограничением alpha.

Следующие действия: Windows/Linux CI текущего commit с реальными Docker tests,
затем расширить независимый корпус и снять local-model baseline на целевом ПК.
Для GPU/Windows SDK приемки это окружение не подходит: доступного native runtime
и репрезентативных GGUF здесь нет. Не подменять эти gates tiny smoke или API.


## Второй инкремент: публикация и восстановление набора файлов

Исходная ревизия 6918265; продолжается та же рабочая ветка. На 6918265 оба
[Windows/Linux CPU jobs](https://github.com/DraimCiDo/Lyra/actions/runs/41802367906)
полностью прошли (release, Desktop launch, упаковка), также прошла
[CUDA build](https://github.com/DraimCiDo/Lyra/actions/runs/41802367844).
Это сборка CUDA без доступного NVIDIA runtime на runner, не GPU model acceptance.

Результат: CLI `apply --all`, проверка всего набора до первой записи, постоянная
OS file lock и bounded приватный undo journal `.lyra/apply-journal.json` до
записи оригиналов. `--index` использует тот же механизм. CLI `recover` показывает
состояние без вывода исходников; `--action rollback|complete` восстанавливает
набор после завершения процесса. Внешнее содержимое блокирует всю операцию,
журнал сохраняется. При обычной ошибке записи выполняется безопасная попытка
отката. Eval harness применяет весь набор одной командой. Новых зависимостей нет.

Границы: последовательные замены файлов с восстановлением, не filesystem-wide
atomic transaction. Не блокирует сторонние редакторы/процессы. Empty directories
после отката могут оставаться; journal не заменяет Git backup. Worktrees,
Desktop-кнопка применения всего набора и GUI recovery остаются отдельными шагами.
Windows CREATE_NEW при прерывании внутри записи нового файла может оставить
неполное содержимое: recovery сохраняет его и журнал, требует ручного разбора,
не удаляет файл только из-за совпадения префикса. Это явно не автоматическое
восстановление всех byte-level interruption points.

Критерии:
- [x] Все конфликты/read-only/duplicate/prefix paths отклоняются до первой записи.
- [x] Apply/recovery сохраняют CRLF, Unicode, внешние dirty edits и permissions.
- [x] Каждый разрыв 0..N публикаций восстанавливается в обе стороны; настоящий
  process exit освобождает lock и оставляет пригодный журнал.
- [x] Внешние правки и symlink не перезаписываются/не удаляются при recovery.
- [ ] CLI, harness, fmt/Clippy/tests и нативный Windows CI проверены.

Локально: 180 Rust tests passed / 7 ignored с desktop,gateway-http, включая
14 targeted publication regressions и 3 CLI integration tests. Clippy all-targets
с `-D warnings`, fmt и 4 Python harness tests прошли. Исходный тест
fsync выявил O_PATH descriptor cap-std. Для sync directory открывается через
`Dir::open(".")`, а не копируется O_PATH handle. Нативный Windows CI нового head требуется отдельно; предыдущий успешный CI
6918265 не считается проверкой второго инкремента.
