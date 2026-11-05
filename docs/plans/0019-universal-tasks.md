# Универсальные задачи и документы

Основание: утвержденный пользователем план 2026-11-05. Автовыбор технологий,
офисные форматы/PDF, выполнение временных программ в изоляции. Подготовка среды
требует явного разрешения; публикация исходников остается отдельной операцией.

Порядок: открытый WorkPlan и совместимость blueprint → реестр сред и prepare →
изолированное выполнение с входами/выходами → SHA256 blobs и файловые предложения,
checkpoint/recovery → Office/PDF worker → Desktop → сквозная приемка и документация.

Проверки: неизвестные заранее языки, смешанные задачи, отсутствие среды, отказ
разрешения, двоичные входы, сохранность оригиналов, поврежденные файлы, отмена,
ограничения ресурсов, аварийное восстановление, совместимость прежних отчетов.
Не считать валидацию/рендеринг документа подтверждением его смысловой корректности.

Лимиты: 64 MiB/документ, 256 MiB входов и результатов, 32 предложения. Локальный
Docker без enforced cgroups блокируется; положительное исполнение требует CI.

## Реализация

Открытый WorkPlan, blueprint/project metadata v2, свободная grammar технологии;
старые presets и blueprint v1 сохранены. Среды — JSON registry с digest/image ID,
inspect/explicit prepare и bundled document build. В agent loop добавлены
set_work_plan, run_script, inspect_document, read_artifact. Входы readonly;
результаты bounded tar, ссылки/лишние файлы отклоняются. Pending документы доступны
следующим шагам. Отчеты v5, SHA256 blobs, mixed byte publication/recovery под общим
lock; backup blobs очищаются после публикации. Legacy journal/report читаются.
SQLite v2 с двоичными snapshot sidecars и транзакционной миграцией v1. Desktop
сохраняет grants, удерживает временные результаты, показывает PNG/export copies.

Document worker: DOCX/XLSX/PPTX/PDF create/edit/inspect/validate, render и OCR;
неподдерживаемые активные/подписанные пакеты отклоняются, lossful package edits
блокируются. Wheel версии/хеши получены из PyPI; Debian/base image закреплены.
Новые dependencies Rust: tar 0.4.46 (проверяемый импорт контейнерных результатов),
image 0.25.10 PNG-only/Desktop (ограниченный декодер preview, уже присутствовал
транзитивно). Python/Office библиотеки устанавливаются в отдельный Docker image.

## Проверки и ограничения среды

Локально Linux cloud; native Windows/GPU недоступны, Docker не имеет enforced
cgroups. Поэтому positive execution проверяется CI, без обхода preflight.
Шесть trusted fixture document tests прошли; никакой model output не запускался
на host. Новые Rust regressions проверяют arbitrary technologies, blobs/conflicts,
rollback, input drift и binary resume. Первый full run выявил устаревший тест
закрытой grammar — заменен контрактом открытого выбора. Ресурсная ошибка linker
при заполненном диске устранена очисткой только task-owned package cache; guard
резерва места GGUF сохранен. Финальные результаты записываются по факту.

Смысловая/визуальная приемка и качество выбора моделью требуют реальных прогонов.
Legacy check-config пока проверяет текстовый snapshot; binary proposals не могут
получить changes_verified на основании этой проверки. Показатель остается честным.

Первый CI на 2fafc98: Linux/Windows Clippy и тестовые матрицы прошли; все четыре
Docker corpus jobs завершились success. Document image build и release/CUDA
проверяются отдельно. Review добавил skip неизмененных файлов при rollback
(Windows Word-lock regression), cleanup orphan staging без удаления project data,
кеширование отказа PNG preview и scoped instructions для file proposals.
Локально review Clippy прошел, universal regressions 9 passed / 3 ignored.

Первый document image CI остановлен snapshot.debian.org HTTP 504 на нескольких
пакетах. Добавлены bounded apt retries/timeout, без смены закрепленного snapshot.

Повторный CI обнаружил устойчивые 503 на Snapshot InRelease. Dependency closure
разрешен один раз apt с GPG-verified snapshot metadata внутри точного base image:
190 пакетов, 148.26 MiB downloads. В debian.lock.json закреплены версии/размеры/SHA256;
основной Debian mirror может выдавать только те же байты, Snapshot — резерв.
Установка local .deb выполняется с network=none; подмена версий/отключение TLS
или hash checks не применяется. Downloader имеет bounded retries и stdlib tests.

OS lock downloader: 3 stdlib tests passed, реальный coinor-libcoinmp0 скачан
с основного mirror и SHA256/размер проверены. Latest local Clippy прошел;
universal regressions 11 passed / 3 ignored, session/project regressions прошли.
Execution records фиксируют image/скрипт/результат; volatile elapsed исключен
из no-progress сравнения (отдельная regression).

CPU CI на fc87ea2 выявил устаревшее help assertion «ещё не реализованы» после
обновления CLI. Проверка заменена на наличие environment, изоляции и отдельного
apply; отсутствие необходимости модели/ключа и точная версия проверяются прежним
тестом. Universal controller tests теперь 11 passed / 3 ignored на Linux.
