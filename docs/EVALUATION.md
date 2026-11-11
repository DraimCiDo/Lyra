# Корпус проектов и оценка качества

`evals/projects/suite.json` содержит 36 задач с SHA256 исходников, эталонов и
независимого checker. Эталоны находятся вне project source и не передаются модели.
Каждое направление содержит 2 исправления, 2 multi-file задачи и 2 новые функции.
Один старый seed `evals/python-multifile` сохранен для совместимости harness.

| Направление | Что реально проверяется | Ограничение |
| --- | --- | --- |
| Rust, 6 | Границы времени/путей, decoder, transfer, intervals, config | Модули std, не Windows-native SDK |
| Python/боты, 6 | TTL, retries, CSV, routing, dependencies, queue | Без API реального Telegram/Discord |
| Backend, 6 | Pagination, tenant auth, revisions, transaction, idempotency, rate limit | Контракты сервисной логики, не развернутый сервис |
| Web, 6 | Query/form/cart/selectors/history/async races | Node module tests, не Vue/TS build или browser acceptance |
| Java/Minecraft core, 6 | Permissions, cooldown, routing/config, completion/history | Java core, не Bukkit/Paper server или Kotlin compiler |
| DayZ, 6 | CfgPatches/CfgMods и пути script modules | Только structure; SDK compile/server gates обязательны |

Таким образом, 30 задач имеют unit checks, 6 — structural checks. Это начало
репрезентативного корпуса, а не доказанная полнота покрытия направлений или
готовность сложного приложения. Настоящие браузерные, Kotlin/SDK/игровые сценарии
нужны отдельными слоями приемки. DayZ structural success не закрывает game gates.

## Проверка самого корпуса

Каждая задача должна провалиться на закрепленном исходнике и пройти на эталоне.
Затем в эталон по одному возвращаются исходные версии изменяемых файлов: каждый
вариант должен снова упасть. Это проверяет, что все заявленные multi-file правки
нужны checker. Всего в текущем корпусе 120 вариантов. Это не автоматически
сгенерированные тесты к ответу модели: checker закреплен до ее запуска.

```sh
# Сначала подготовить pinned images, указанные в check.json выбранных профилей.
python tools/validate_corpus.py --binary ./target/debug/lyra --profile python --profile backend
```

По умолчанию validator исполняет варианты через `lyra verify`, с теми же enforced
Docker limits и отказом неподходящему daemon. Нужные образы объявлены с неизменным
Linux amd64 digest в каждом check.json и в corpus CI; автоматического pull внутри
продукта/harness нет. Отдельный GitHub workflow явно готовит их и проверяет все
профили на daemon с cgroups. Зависимости не устанавливаются из model output.

Для разработки **только закрепленных доверенных фикстур** есть явный режим:

```sh
python tools/validate_corpus.py --mode trusted-host --output .lyra/corpus-validation.json
```

Он запускает исходники/эталоны/реверсии из выбранного pinned suite на developer
host, требует Python, Node, rustc и Java compiler. Он не принимает модельные
отчеты или пользовательские проекты. Нельзя применять этот режим к коду модели.
На машине без нужного runtime профиль помечается blocked, а не passed.

## Прогон реальной локальной модели

```sh
python tools/evaluate.py --binary ./target/debug/lyra \
  --suite ./evals/projects/suite.json --model /path/to/pinned-model.gguf \
  --output ./.lyra/evals/model-run-001.json --repeats 3 \
  --context 8192 --tokens 2048 --calls 64 --timeout 1800
```

Здесь **весь код модели исполняется только внутри Docker**. Harness сначала
проверяет исходный провал, затем вызывает local task с доверенным checker,
проверяет scope предложений, применяет их одной командой в disposable copy и
повторяет независимую проверку. Модель не выбирает program/image/argv checker.
Каждый failed/blocked run также получает measured total wall time; RAM/VRAM без
измерения остаются null. Сохраняются SHA256 бинарника, GGUF и suite, полный task
report и конфигурация. Артефакты содержат код и остаются локальными в `.lyra/`.

Отчет разделяет `check_passed` и итоговый `passed`; незакрытые внешние gates не
превращаются в приемку. Статистика группируется по профилю, уровню и типу задачи.
Заблокированный baseline до попытки модели считается infrastructure blocked,
не оценкой ее coding quality. Для фактически запущенных задач пороги плана:
90% small fixes, 80% multi-file, 70% new features. Порог по двум маленьким задачам
и трем повторам не является статистически сильной гарантией общего качества.

Применять пороги ко всем профилям можно после реальных запусков на целевом ПК
и закрытия недостающих SDK/browser/server gates. Tiny GGUF smoke, host validation
эталонов и успешная сборка CUDA не заменяют эти прогоны.

## Продолжение и сравнение моделей

`evaluate.py --resume` дополняет незавершенный отчет при совпадении бинарника,
GGUF, suite и параметров. Уже завершенные failed runs сохраняются. Новые попытки
для улучшения результата требуют отдельного эксперимента. `--profile` выбирает
профиль, `--threads` фиксирует число потоков. `--measure-resources` включает
раздельные best-effort пробы RAM (100 ms) и VRAM (500 ms). Область измерения —
только CLI process, без Docker и потомков; недоступные метрики остаются null,
короткий GPU пик может не попасть в выборку.

Для одинакового прогона нескольких заранее скачанных моделей создайте JSON:

```json
{"schema_version":1,"models":[{"id":"model-a","path":"model-a.gguf","sha256":"ACTUAL_SHA256","gpu_layers":"balanced"}]}
```

Пути отсчитываются от JSON. Допускаются 1..8 моделей; `gpu_layers` — число,
`auto` или `balanced`. Каждый GGUF проверяется по SHA256, скачивание и routing
не выполняются. Запуск:

```sh
python tools/evaluate_models.py --binary ./target/debug/lyra \
  --models /path/to/models.json --suite ./evals/projects/suite.json \
  --output ./.lyra/evals/comparison-001 --measure-resources
```

Manifest фиксирует реальные doctor hardware и общую конфигурацию. Продолжение
с `--resume` требует совпадения идентичности hardware, бинарника, моделей и suite;
изменение свободной RAM/диска само по себе не меняет hardware identity.
