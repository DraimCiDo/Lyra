# Проверки и checkpoints задач

Первый инкремент [программы качества](plans/0018-quality-workflows.md).
CLI поддерживает изменяемые предложения, обратную связь reviewer → coder,
явные Docker-проверки и опциональные SQLite-checkpoints. Это не завершенная
приемка сложных проектов или полная реализация программы.

## Правки и результат

`propose_edit` работает относительно текущего предложения файла, если оно есть.
`read_file`, `search` и `list` используют один pending-набор, включая новые файлы; исходный файл сохраняется как
предусловие `apply`. `discard_proposal` отзывает предложение без записи на диск.
Reviewer может вызвать `request_changes`; coder исправляет pending-версию,
затем reviewer проверяет повторно. Не более двух repair rounds и общего бюджета
`--max-calls` (1–256, default 12). Четыре подряд одинаковых action/result
останавливают задачу как `no_progress`; два синтаксически неверных JSON допускают
восстановление. Неизвестные инструменты/поля не исполняются.

Отчет v4 сохраняет `status` для совместимости и добавляет `outcome`:
`incomplete`, `analysis_complete`, `changes_proposed`, `changes_verified`.
Последний означает только успешную явно заданную проверку текущего набора
предложений. Полнота тестов и правильность требований не гарантируются.
Мнение reviewer по-прежнему не является доказательством теста.
Отмена/deadline проверяются до и после model/tool вызова; поздний `finish`
не превращает отмену в успех. Коды CLI: 130 — отмена, 124 — timeout,
включая HTTP task. Ожидание blocking HTTP остается кооперативным.
Отчеты v1–v3 читаются старым путем apply; новые отчеты требуют новой Lyra.

## Явная проверка Docker

Docker нужен только для этого режима; запуск обычного CLI/GGUF остается нативным.
Нужны Linux containers и заранее установленный образ с `/usr/bin/timeout`.
Образ задается неизменным digest, сеть и автоматический pull запрещены.
Пути с запятой не поддерживаются Docker bind mount и явно отклоняются.

Проверенный пример `evals/python-multifile/check.json` использует pinned
официальный Python image. Его явная подготовка:

```sh
docker pull python@sha256:ae14d4ce8771606477a81446e7ef429edd6762305fc50c84507c1041fa8285d1
lyra verify --repo ./evals/python-multifile/source --config ./evals/python-multifile/check.json
```

Исходный fixture должен провалить assertion. Конфигурация задает program/argv,
image, timeout и memory; это доверенный выбор пользователя, не вывод модели.
Она может содержать независимые проверки, которые не передаются модели.
Контейнер получает копии только разрешенных UTF-8 файлов. Исходники и root FS
read-only, сеть отключена, capabilities сняты, no-new-privileges включен.
Нет host credentials, сокетов, Docker socket и произвольных host mounts.
Запись разрешена в ограниченный `/tmp`; `CARGO_TARGET_DIR=/tmp/target`.
Измененные предложения подставляются в копию перед проверкой, без записи оригинала.
Лимиты: 2 CPU, 128 процессов, 128–16384 MiB RAM, 64 KiB каждого потока вывода.
Постоянное Docker-логирование отключено (`--log-driver=none`).
Отмена/таймаут удаляют контейнер; без подтвержденной очистки проверка не успешна.
Внутренний timeout ограничивает выполнение даже при аварийном завершении Lyra.

```sh
lyra task --repo ./project --model /path/to/model.gguf --task-file ./TASK.txt \
  --check-config ./trusted-check.json --max-calls 48 --timeout-secs 1800
```

В этом режиме coder получает `verify` без параметров. Он не выбирает image,
argv или права. Ошибки доступны для исправления; завершение без успешной проверки
текущих предложений дает `verification_required`, и apply отказывает.
SHA256 связывает результаты с конкретными предложениями. Любая последующая
правка делает прежний результат недостаточным для завершения.

Это первый backend: Windows VM/SDK, установка зависимостей, profile adapters,
полные toolchains/сети SDK и Desktop-настройки executor еще не реализованы.
Текущие ограничения контекста alpha (128 файлов, 512 KiB чтения, исключение
скрытых файлов) сохраняются; превышение при snapshot блокирует запуск, не
превращается в неполную успешную проверку. Отбор по имени не находит все секреты
в исходном коде. Docker daemon/image должны быть доверенными. Preflight требует реально
доступных memory/swap/PID/CPU limits и default seccomp: если daemon игнорирует
эти ограничения, исполнение блокируется до запуска кода.

## Локальная история работы

`--session-db` явно разрешает сохранить выбранные исходники, задачу, предложения
и доказательства в локальной SQLite. API-ключи и environment туда не записываются.
База содержит приватный код; не добавлять ее в Git и не публиковать автоматически.
SQLite встроена в EXE через `rusqlite =0.41.1` / bundled; сервис БД не нужен.
Новая база и временные snapshot-каталоги получают private permissions на Unix.
Windows использует ACL пользовательского каталога. SQLite-транзакции сохраняют
checkpoint перед запросом модели и после каждого принятого/отклоненного действия.

```sh
lyra task --repo ./project --model /path/to/model.gguf --task-file ./TASK.txt \
  --session-db ./.lyra/tasks.sqlite --max-calls 48
lyra session show --db ./.lyra/tasks.sqlite --id SESSION_ID
lyra task --repo ./project --model /path/to/model.gguf --task-file ./TASK.txt \
  --session-db ./.lyra/tasks.sqlite --resume SESSION_ID --max-calls 96
lyra session show --db ./.lyra/tasks.sqlite --id SESSION_ID --report
lyra session delete --db ./.lyra/tasks.sqlite --id SESSION_ID
```

ID выводится в stderr. Resume требует тот же root и текст задания; использует
immutable snapshot, включая незакоммиченные изменения на момент старта.
Принятые предложения и результаты сохраняются, недавняя tool-history собирается
заново чтением. Общий счетчик вызовов сохраняется; бюджет можно увеличить явно.
Optimistic revision check не допускает перезапись checkpoint другим worker.
Новая политика check_config задается пользователем при каждом запуске.
Оригинал меняется только явным apply; конфликт с внешними правками блокирует его.

Хранилище ограничено 32 MiB, отчет — 2 MiB. Удаление удаляет исходники/checkpoint
с secure_delete, но не обещает физическое стирание SSD/backup. Git worktrees,
публикация нескольких файлов с recovery journal, удаление/переименование файлов,
долгосрочная память и Desktop resume остаются отдельными шагами.

## Developer eval harness

```sh
python -m unittest discover -s tools -p test_evaluate.py
python tools/evaluate.py --binary ./target/debug/lyra \
  --suite ./evals/python-multifile/suite.json --model /path/to/model.gguf \
  --output ./.lyra/evals/run-001.json --repeats 3
```

Suite закрепляет хешами все исходники и независимый checker.
`evals/**` хранится без преобразования Git line endings, включая CRLF cases. Сначала проверяется
ожидаемый исходный провал, затем реальный local task с Docker verify, разрешенные
пути изменений, явное применение в disposable copy и повторная независимая
проверка. Код модели никогда не исполняется harness на host. Ошибки окружения
отделяются от неуспеха задачи. Результат содержит measured wall time, статус,
вызовы/отчет, SHA256 EXE/GGUF/suite; неизвестные RAM/VRAM остаются null.
Вывод содержит исходный/предложенный код и остается локальным.

Пока включена одна небольшая Python multi-file задача. Это инфраструктура,
не полный корпус 36 задач и не доказательство качества GGUF на целевом ПК.
