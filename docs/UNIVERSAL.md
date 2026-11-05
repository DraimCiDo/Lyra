# Универсальные задачи, среды и документы

Языковые профили — стартовые шаблоны. Планировщик принимает свободное название
технологии, а WorkPlan содержит несколько технологий с назначением и обоснованием.
Явные требования пользователя и существующий стек имеют приоритет. Для новой
задачи выбираются язык, сочетание инструментов или готовая операция по результату,
платформе, библиотекам и ограничениям. Запрос «сделай сайт» не фиксирует Vue.

## Среды выполнения

Manifest `environments/runtimes.json` содержит закрепленные Linux amd64 среды
Python, Go и PHP. Дополнительная среда подключается JSON-записью без Rust enum:
`id`, `description`, `image`, `program`, `args`, `script_extension`, `capabilities`,
`documentation`, `memory_mib`, `timeout_secs`. `args` предшествуют пути скрипта.
Образ обязан содержать POSIX sh, tar и `/usr/bin/timeout`. Допускаются OCI digest
и локальный immutable `sha256:` image ID. Версия manifest — 1.

```sh
lyra environment --registry environments/runtimes.json list
lyra environment --registry environments/runtimes.json inspect golang
# Без --accept показывается запрос на подготовку; загрузки не выполняются.
lyra environment --registry environments/runtimes.json prepare golang
lyra environment --registry environments/runtimes.json prepare golang --accept
lyra task --workspace ./project --model /path/model.gguf --task-file TASK.txt \
  --environments environments/runtimes.json --allow-environment golang \
  --artifacts-dir ./.lyra/task-artifacts
```

`--workspace` совместим с `--repo`. Desktop привязывает разрешения к SHA256 manifest: после его изменения требуется
повторный выбор и выдача разрешений. Manifest выбирается пользователем отдельно
от содержимого проекта; модель не может расширять разрешенные среды или менять
image/argv запуска. Нужный язык может отсутствовать в manifest: план остается
возможным, исполнение требует подготовки подходящей среды.

Скрипт получает только перечисленные входы в `/source`, записывает объявленные
результаты в `/output`. Pending-файлы доступны следующим шагам без применения.
Сеть отключена, входы/root read-only, пользователь non-root; enforced CPU,
memory/swap/PID/seccomp обязательны. Scratch — 512 MiB, output tmpfs — 256 MiB.
stdout программы — диагностика; результаты возвращаются ограниченным tar-потоком.
Ссылки, устройства, лишние и отсутствующие файлы отклоняются. Код модели на host
не выполняется. Программа runtime может использовать библиотеки своего образа.

## Office и PDF

Образ документов строится отдельно по встроенному рецепту: закрепленный Python,
подписанный Debian snapshot, LibreOffice, Poppler, Tesseract rus/eng и Unicode fonts;
Python-зависимости закреплены версиями и хешами wheel. Для Office/OCR нужно
несколько GiB свободного места в Docker. Python/Office на host не обязательны.

```sh
lyra environment --registry ./documents.json build-documents
lyra environment --registry ./documents.json build-documents --accept
# При корпоративном proxy для pip можно передать --proxy-ca /path/ca-bundle.pem.
lyra task --workspace ./documents --model /path/model.gguf --task-file TASK.txt \
  --file input.docx --environments ./documents.json --allow-environment documents \
  --artifacts-dir ./.lyra/document-results
```

Создается новый manifest с фактическим image ID; существующий файл не заменяется.
В image доступен `import lyra_documents`: `inspect(path,start,count)`,
`create(path,spec)`, `edit(source,destination,operations)`, `validate(path)`,
`render(source,destination)`, `ocr(path,start,count)`. Агент получает описание
возможностей среды и отдельный инструмент `inspect_document`.

DOCX: абзацы/runs/таблицы; XLSX: листы, ячейки, формулы; PPTX: слайды и текстовые
runs; PDF: текст, страницы, поворот, формы, аннотации, объединение и генерация.
Изменение run сохраняет соседние runs и оформление; редактирование ячеек сохраняет
формулы и стили остальных ячеек. Макросы, ActiveX, вложенные программы и подписанные
Office-пакеты требуют другого адаптера. Удаление неизвестных компонентов пакета
блокирует преобразование. Произвольная замена верстки PDF не выполняется молча.
OCR помечается как распознавание с непроверенной точностью.

Проверка структуры и создание preview не доказывают смысловую или визуальную
корректность. Office render создает PDF; PDF render — PNG первой страницы.
В Desktop PNG показывается с ограничением размеров; документ/preview можно
экспортировать отдельной копией для просмотра. Превью тоже нужно объявлять
выходным файлом. Для последующих страниц PDF используйте явно заданные операции
инструментов среды. Кириллица поддерживается установленным Unicode font.

## Результаты и восстановление

Отчет v5 добавляет `work_plan`, `artifact_store`, `file_proposals`; документы не
помещаются в JSON/base64. Blobs проверяются по SHA256 и размеру. Лимиты: 64 MiB
на файл, 256 MiB входов/выходов, 32 предложения; metadata файлов — до 256 KiB.
Исходные документы и зависимости проверяются перед публикацией. Текст и документы
можно применять одним набором; отдельные файлы заменяются через временный файл,
общий журнал поддерживает `recover` и сохраняет внешние правки.

```sh
lyra apply --workspace ./documents --report report.json --all
lyra recover --repo ./documents
```

Старые отчеты v1–v4, blueprint v1 и journal v1 продолжают читаться. Blueprint и
project metadata v2 сохраняют открытый WorkPlan. SQLite v1 мигрирует транзакционно
в v2: текст остается в базе, двоичные исходники/результаты — в private sidecar
`<db-name>.artifacts/<session-id>`. Сохранение явно включается `--session-db` или
настройкой Desktop. Resume использует сохраненные байты; delete удаляет blobs
данной сессии. Без persistence Desktop хранит результаты во временной папке.
CLI сохраняет blobs только в выбранный `--artifacts-dir`; JSON нужно сохранять
вместе с доступом к этому хранилищу. После публикации journal backups очищаются.

Обычный `--check-config` проверяет текстовый snapshot. Он не дает доказательства
для двоичных предложений: такой набор не получает `changes_verified`. Для работы
с документами используйте проверки/рендеринг среды и просмотр результата.

## Приемка

Rust regressions проверяют открытые технологии, blobs/conflicts, rollback,
пагинацию старых сессий и восстановление двоичных снимков. Шесть developer tests
Office/PDF выполняют только доверенные фикстуры. Universal CI отдельно строит
образ, выполняет Office/PDF/render через sandbox и запускает Go/PHP. Это проверка
инструментов; качество выбора и выполнения задач моделью оценивается отдельно.
