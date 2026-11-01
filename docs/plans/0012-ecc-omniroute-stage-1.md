# ECC + OmniRoute: первый общий рабочий инкремент

Статус: этап 1 реализован и проверен на CPU/Windows и в CI. База: PR #3, db25bc0. Пользователь подтвердил реализацию
по этапам, а не только расширение спецификации.

## Проверенные upstream

- ECC: b38be6a7f6350964d59328b52f0624e1d1d552d1, MIT, Copyright 2026 Affaan Mustafa.
  Контракты ролей: agents/; навыки: skills/; события: hooks/;
  правила: rules/; память: skills/unified-memory/ и соответствующий runtime.
- OmniRoute: 139070680f77728e2309eb4df18463282a1c990a, MIT,
  Copyright 2026 diegosouzapw. Точки входа:
  open-sse/services/compression/engines/registry.ts,
  open-sse/services/compression/engines/session-dedup/,
  open-sse/services/compression/engines/ccr/,
  open-sse/services/compression/fidelityGate.ts,
  docs/compression/COMPRESSION_ENGINES.md и docs/routing/.

Это исходные ревизии аудита, не автоматическое слежение за main.
Реализация ниже — собственный Rust-код; исходники upstream не копируются.
Полная пофункциональная инвентаризация P00 ещё не завершена.

## Этап 1: роли, восстановление после ошибки, обратимое сокращение контекста

ECC:

- Именованные контракты researcher/coder/reviewer: цель, разрешённые инструменты,
  признак read_only; одинаковая основа для GGUF и HTTP.
- Наблюдения инструментов: status, summary, next_actions, artifacts, data.
- Явное состояние pending proposals: предложение не является записью в файл;
  повтор того же файла не нужен, завершение — отдельный finish.
- Отклонённые old/new сохраняются в контексте ошибки. Нельзя утверждать,
  что их содержимое есть в proposals, если предложение отклонено.

OmniRoute-inspired:

- Отдельный opt-in --dedup-tool-context и переключатель Desktop.
- Повторяющиеся успешные старые read/search/doc results заменяются ссылками
  на единственную копию в словаре текущего запроса.
- Последний результат, ошибки, task, права, схемы действий, patches и proposals
  остаются дословными. Нет межпроектного или скрытого постоянного cache.
- Перед отправкой выполняется round-trip fidelity check и проверка фактического
  уменьшения JSON. При любой неудаче возвращается исходный контекст.
- Отчёт содержит байты контекста до/после и число ссылок; HTTP backend сообщает
  размер сериализованного JSON body, переданного HTTP-клиенту. Токены и стоимость не угадываются.
- Safe/Balanced/Aggressive сохраняют прежнюю семантику окна истории.

Границы: это не semantic compression, не cross-turn серверный cache, не полный
CCR retrieve protocol и не перенос всех engines. Нет нового shell/test runner,
автоприменения незавершённых отчётов или ослабления точных preconditions.

## Проверки и готовность

- [x] Роли не получают лишних tools или прав.
- [x] Ошибки exact-edit и повторные предложения дают пригодную обратную связь.
- [x] Декодирование packed envelope совпадает с исходным Value, включая Unicode/CRLF.
- [x] Последний результат/ошибки/patches не сокращаются; без выгоды — passthrough.
- [x] HTTP stub измеряет меньший реальный body и восстанавливает прежний контекст.
- [x] CLI и Desktop включают один и тот же pipeline; старые настройки остаются off.
- [x] fmt/Clippy/tests/сборка; native Windows acceptance и реальная GGUF-фикстура.

Windows/Linux CI c0aee0f и d5010eb прошёл. Native CPU coding fixtures:
Qwen2.5-Coder-7B — completed за 5 вызовов; Qwen3.5-9B — за 4; отдельный apply,
unittest exit 0 и сохранённый CRLF в обоих случаях. Это небольшая арифметическая
фикстура с проверкой AST до исполнения, не общий sandbox или quality benchmark.
HTTP fixture: body 36965 → 14813 байт (59,93% именно на повторных чтениях),
exact restore и последнее наблюдение сохранены. Desktop d5010eb отобразил
счётчик на завершённой read-only задаче; [приёмка UI](0013-desktop-conversations-models.md).

Локально по-прежнему нет установленного Cargo/MSVC; есть отдельный rustfmt 1.99.1.
Исходники проверяются CI, артефакт — на ПК. Финальные результаты фиксируются
в PR #3; документационный коммит не требует повторения проверок неизменного кода.

## Следующие законченные этапы

| Этап | Что пользователь сможет делать | Критерий |
| --- | --- | --- |
| 2 — Skills/Rules/Commands | Видеть каталог, выбирать набор по проекту/роли, управлять версиями и приоритетами правил, вызывать workflow из палитры | Конфликты, rollback, отсутствие исполнения при импорте |
| 3 — Context Engine | Видеть token budget, выбирать безопасную фильтрацию tool output, retrieval и summaries; получать raw recovery | Golden corpus, точные код/ошибки/патчи, measured savings + quality |
| 4 — Smart Gateway | Выбирать маршрут по цене/задержке/квоте, видеть health, backoff, cost и cache receipts | Quota/concurrency/cancel fixtures; никаких скрытых cloud fallback |
| 5 — Agent workflows | Plan → implement → sandbox test → review, отдельные навыки/бюджеты ролей, worktrees и checkpoints | Реальный coding fixture и изоляция Windows |
| 6 — Hooks/MCP | Подключать разрешённые инструменты и проверяемые before/after gates | Таймауты, запрет расширения полномочий, malicious fixtures |
| 7 — Memory/Learning | Resume, искать scoped memory, проверять происхождение, подтверждать reusable instincts | Export/delete/expiry, отсутствие auto-policy changes |
| 8 — Extended parity | Streaming/API gateway, advanced routing, протоколы и multimodal в поддерживаемых adapters | Пофункциональная матрица upstream → Lyra → тест |

Каждый этап отдельно реализуется, измеряется и открывается в CLI/Desktop.
Наличие пункта в этой таблице не означает готовую возможность.
