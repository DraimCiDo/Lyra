# Граф кода

Два источника графа, одна визуализация и один набор команд:

1. **Встроенный анализатор Lyra** (`lyra graph build/remote`) — эвристический, без зависимостей и без модели.
2. **[Graphify](https://github.com/Graphify-Labs/graphify)** (Apache-2.0, Python, tree-sitter AST) — внешний
   инструмент, ставится отдельно (`uv tool install graphifyy`). Lyra **не включает его код** и не требует
   Python: она читает его `graph.json` как недоверенные данные (`lyra graph import`) либо по явной команде
   запускает установленный `graphify extract . --code-only` без LLM (`GRAPHIFY_BACKEND=none`,
   `lyra graph graphify`). Импорт проверен на реальных `worked/httpx` и `worked/rsl-siege-manager` из
   репозитория Graphify; **сам запуск `graphify` в этой среде проверен только заглушкой**
   (установка пакета из PyPI была запрещена политикой среды). Отношения Graphify (`calls`, `uses`,
   `inherits`, …) сохраняются, `INFERRED`-рёбра помечаются.

Встроенный анализатор — собственная реализация идеи структурной навигации, не копия Graphify.

Узлы — файлы и символы (fn/struct/class/def/function/…; для Markdown — заголовки), рёбра —
`defines`, `imports` (Rust/Python/JS/TS/Go/Java/Kotlin), `references` (уникальные CamelCase/
snake_case имена между файлами одного языка). Сообщества — детерминированное label propagation
с демпфированием хабов. В `graph.json`, `GRAPH_REPORT.md`, `graph.html` только **пути и имена**,
не содержимое файлов; секреты, бинарники, `.git`, `target`, `node_modules`, gitignore пропускаются.
Лимиты: 3000 файлов, 256 КиБ на файл, 40 000 рёбер.

## CLI

| Команда | Действие |
| --- | --- |
| `lyra graph build --repo DIR --out OUT` | `graph.json`, `GRAPH_REPORT.md`, интерактивный `graph.html` (без внешних скриптов) |
| `lyra graph remote --github o/r [--git-ref main] --out OUT` | то же по дереву GitHub через коннектор (read-only, до 300 файлов) |
| `lyra graph import --graph graph.json --out OUT` | импорт Graphify (или Lyra) `graph.json` → отчёт и `graph.html` Lyra |
| `lyra graph graphify --repo DIR --out OUT [--exe graphify]` | запуск установленного Graphify offline + импорт (пишет `graphify-out/` в репозиторий, как сам Graphify) |
| `lyra graph query --graph graph.json --term X` | узел и его соседи |
| `lyra graph path --graph graph.json --from A --to B` | кратчайшая связь |
| `lyra graph pr (--repo DIR \| --graph graph.json) --github o/r --number N --out OUT [--depth 2]` | изменённые файлы PR и «радиус поражения» (зависимые файлы); подсвечены в `graph.html` |
| `lyra task … --code-graph` | сводка структуры (≤3 КиБ, помечена как недоверенные данные) в контексте всех ролей |

Для `remote`/`pr` нужна сборка `gateway-http` и токен в переменной окружения (`--token-env`,
`--api-url` — как у [GitHub-коннектора](GITHUB.md)).

## Desktop

Кнопка «Граф кода» в шапке: построение графа выбранного проекта или «Открыть graph.json (Graphify/Lyra)», поиск, символы/файлы без связей,
масштаб колесом, сдвиг перетаскиванием, клик по узлу, «Влияние PR из GitHub» (номер PR +
`owner/name` из настроек «Помощники»: красные — изменены, оранжевые — затронуты).
Настройка «Граф проекта в контексте агентов» включает `--code-graph` (по умолчанию выключена).

## Ограничения

Анализ эвристический (построчный разбор, без AST и без разрешения типов): возможны пропущенные и
лишние связи, динамические импорты не видны. Граф — подсказка для навигации, не доказательство
зависимостей. Для точного AST-графа и семантического слоя используйте Graphify; облачные функции Graphify Cloud
(app.graphify.com) не интегрированы и не нужны.
