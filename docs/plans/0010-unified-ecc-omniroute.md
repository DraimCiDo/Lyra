# Lyra Unified Platform — полная программа интеграции ECC и OmniRoute

Статус: **утверждённое направление, план реализации; не реализовано целиком**.
Дата: 2026-10-31. База: текущий Rust CLI + egui Desktop + GGUF alpha.
Источники: [ECC](https://github.com/affaan-m/ECC) / [Everything Claude Code](https://github.com/affaan-m/everything-claude-code),
[OmniRoute](https://github.com/cheaper-inference/OmniRoute).
Перед переносом проверять фактические URL, upstream SHA, LICENSE, зависимости и ограничения API.

## 1. Цель и границы

Lyra — единая Windows-first coding platform, объединяющая workflow ECC с
model gateway и оптимизацией OmniRoute. Поддерживаются локальные и облачные модели;
полностью локальный режим сохраняется. «Всё из ECC и OmniRoute» означает
**покрытие функциональных категорий с матрицей совместимости**, а не обещание
буквального копирования Node/Next.js/Claude-specific UI в Rust/egui.
Ни одна функция не считается завершённой без реализации, тестов и приёмки.

Запрещено: скрытая отправка кода, обход ToS провайдеров, скрытый fallback,
сбор секретов, автоматический запуск недоверенных hooks без разрешения,
потеря пользовательских изменений, неконтролируемые расходы.
Внешний inference включается пользователем; отправляемые данные и выбранный
провайдер видимы. Лицензии и third-party notices обязательны.

## 2. Схема

```text
egui Desktop / CLI / future IDE
             |
  Lyra Session + Task Controller
             |
  Agent Runtime (ECC)
  Skills | Rules | Hooks | Commands | Agents | Workflows
  Memory | Learning | Evaluation | MCP tools | Security
             |
  Context Optimizer (ECC + OmniRoute)
  Retrieval | Dedup | RTK | Compression | Summaries | Cache policy
             |
  Model Gateway (OmniRoute)
  Registry | Provider adapters | Router | Quota | Budget | Retry
  Cache | Telemetry (local) | Streaming | Model capability checks
          /               \
  GGUF / local runtimes    Cloud APIs / optional OmniRoute service
             |
  Typed Tools + Permissions + Windows Process Sandbox
             |
       Repository / tests / Git
```

Контракты: модель получает нормализованные messages, tool schemas, usage,
stream events и cancel; gateway возвращает provider/model/usage/cost/cache
metadata. Agent Runtime не знает формат конкретного API. Hooks и skills
не могут расширять capability grant. Инструменты выполняются вне модели.

## 3. ECC: полная функциональная матрица

| Область | Целевой результат в Lyra | Условия приёмки |
| --- | --- | --- |
| Agents / subagents | Registry ролей; planner, researcher, coder, reviewer, tester, security, docs, language-specific | Права каждой роли изолированы; лимиты шагов; воспроизводимые сценарии |
| Skills | Manifest, версии, параметры, discovery, dependency resolution, selective install/update | Включение/выключение, конфликт версий, rollback, тесты |
| Rules | Common + language/framework/project rules; precedence | Приоритеты детерминированы; untrusted rules не выдают права |
| Commands | Палитра и slash commands, шаблоны задач | Схема аргументов, preview, permissions |
| Hooks | Before/after task/tool/commit, validation, async-safe event bus | Таймауты, fail-closed для критичных gates, аудит |
| MCP | Client stdio/HTTP и реестр разрешённых tools | Scope/consent, schema validation, cancellation |
| Workflows | Plan → research → implement → test → review; TDD, debugging, refactor, docs, security | Реальные тесты отделены от мнения модели |
| Session memory | Resume/checkpoint, scoped project memory, provenance, expiry | Восстановление без утечки секретов |
| Continuous learning / instincts | Extract → evidence → confidence → approval → reusable skill; import/export/evolve | Нет автоизменения политики и весов модели |
| Context efficiency | Small prompts, selective skills, progressive disclosure, context handoff | Token/latency/quality baseline |
| Verification / evals | Test runner, build resolver, reviewers, pass@k, quality gates | Изолированные прогоны, CI, отчёт доказательств |
| Security | Secret scanning, injection defense, AgentShield-like audits, supply-chain checks | Негативные тесты и блокировка критичных действий |
| Parallel agents | Worktrees, conflict detection, concurrency/resource budget | Не перезаписывают друг друга; отмена |
| Ecosystem | Языковые наборы, community packs, migration from ECC formats | Матрица импортируемых/неподдерживаемых форматов |

ECC-specific Claude Code/Codex/OpenCode команды адаптировать к Lyra API, а не
запускать как доверенный shell. Количество upstream agents/skills/commands
фиксируется автоматически на pinned revision; не обещать число по README.

## 4. OmniRoute: полная функциональная матрица

| Область | Целевой результат | Условия приёмки |
| --- | --- | --- |
| Provider registry | OpenAI-compatible + нативные adapters, локальные GGUF/Ollama | Capabilities, discovery, contract tests |
| Authentication | API keys/OAuth where permitted, OS credential vault | Нет токенов в settings/logs/repo |
| Model routing | priority, round-robin, weighted, cost, latency, quality, quota, context, cache affinity | Детерминированные fixtures, explainable decision |
| Auto/combo | Strategies, chained pipeline, fusion, parallel panel, model selection | Max cost/time, cancel fanout |
| Quotas / budgets | RPM/TPM, daily/monthly caps, per-provider spend | Atomic accounting, no silent overrun |
| Resilience | 429/5xx backoff, circuit breaker, health checks, cooldown, failover | Retry только безопасных запросов, no duplicate writes |
| Compression | Session dedup, retrieval markers, lite, RTK/tool filtering, structured compaction, relevance, Caveman, progressive aging, LLMLingua-like optional semantic compression, Ultra, experimental modalities | Fidelity gate, raw recovery, code-safe tests |
| Prompt caching | Stable prefix, cache-aware routing, provider cache accounting | Cache-hit measured, not assumed |
| Response/semantic cache | Hash and scoped semantic cache, TTL/invalidation | No cross-project leakage |
| Memory | Local opt-in retrieval, embeddings optional, decay and provenance | Deletion/export, source attribution |
| API surface | OpenAI-compatible local gateway, SSE/streaming, embeddings, files/batch when justified | Contract tests and access control |
| MCP / A2A / ACP | Protocol bridges and agent discovery | Scope isolation, no arbitrary remote execution |
| Security | PII/secret redaction, prompt injection defense, audit | Opt-in masking; no destructive false positives |
| Operations | Headless gateway, optional sidecar, config/import/export, diagnostics | Windows lifecycle and graceful shutdown |
| UI / observability | Provider cards, routes, quota/cost charts, errors, token savings | Local-only diagnostics by default |
| Plugins / marketplace | Signed/trusted extension manifests and permissions | Supply-chain scanning, revoke/rollback |
| Multimodal | Image/audio/video/OCR through capable providers | Capability detection, consent, bounded payloads |
| Remote / proxy / webhooks | Explicit remote gateway, scoped tokens, notifications | Network allowlist, security review |
| Provider-specific extras | Subscription providers and experimental transports only if lawful and ToS-compliant | Unsupported marked clearly |

Do not reproduce upstream transparent MITM/TLS impersonation by default:
requires separate security, platform and legal review; not essential to coding.
Upstream «15–95%» savings is marketing/benchmark-specific, not a Lyra guarantee.

## 5. Context Optimizer — единая реализация

Pipeline: classify payload → protect code/JSON/URLs/paths/errors/tool schema →
dedup → select relevant retrieval → compress tool output → age/summarize old
dialogue → budget to tokenizer → fidelity check → cache-aware prompt build.
Every stage emits input/output byte and token counts (tokenizer-specific),
latency, fidelity flags, and a reversible raw reference where applicable.

Modes in Desktop:
- Safe: lossless dedup and selective retrieval; preserve original context on demand.
- Balanced: Safe + conservative log/JSON filtering, bounded summaries and cache-aware prompts.
- Aggressive: Balanced + stronger aging and opt-in semantic pruning with fidelity gate.

The current Desktop implementation (if present in branch/main) only windows
conversation history (Safe all / Balanced 12 / Aggressive 6); it is **not**
this complete pipeline. Migrate setting without changing its meaning silently:
new mode semantics require schema migration and release notes.

Never compress patch hunks, exact error signatures, security policies, user
latest instruction, tool JSON schema, or source snippets used for edit preconditions.
If fidelity cannot be established, return original content. Compression must not
change permissions or command arguments.

## 6. Sequenced delivery — PR-sized milestones

| PR | Milestone | Scope | Gate |
| --- | --- | --- | --- |
| P00 | Baseline / upstream audit | Pin ECC/OmniRoute revisions, license/SBOM, feature inventory, benchmark fixtures | Reproducible inventory and source links |
| P01 | Core contracts | Provider/Model/Stream/Usage/ToolCall traits, errors, cancellation, mock adapter | Rust unit/contract tests |
| P02 | Provider support | GGUF bridge, OpenAI-compatible adapter, opt-in network and credential vault | Windows smoke + mocked HTTP |
| P03 | Gateway/router | Registry, priority/weighted/cost/quota, health/fallback | 429/timeout/budget regression |
| P04 | Context engine | Token budget, dedup, protected spans, RTK-style filters, reversible store | Golden tests, no data corruption |
| P05 | Advanced compression | Summaries, relevance, caching, optional semantic engine, fidelity gate | Quality and savings benchmark |
| P06 | ECC manifests | Skills/rules/commands registry, import adapters, selective packs | Schema/precedence/security tests |
| P07 | Agent workflows | Roles, orchestration, typed tools, checkpoints, worktrees | End-to-end coding fixture |
| P08 | Hooks/MCP | Event bus, permission model, scoped MCP client | Malicious hook/prompt injection tests |
| P09 | Memory/learning | SQLite project memory, provenance, instincts and approval | Delete/export/expiry tests |
| P10 | Verification/security | Windows sandbox, build/test execution, security scan and evidence report | Native Windows isolation acceptance |
| P11 | UI integration | Provider picker, route/budget, optimizer slider, skills, agents, logs | Desktop smoke, RU/EN, settings migration |
| P12 | Extended parity | Advanced OmniRoute strategies/protocols/multimodal, ECC language packs | Per-feature parity matrix |
| P13 | Release | Migration, package, licenses, performance, docs | CI + native Windows acceptance |

Dependency order: P00 → P01 → P02/P04/P06 → P03/P05/P07 →
P08/P09/P10 → P11/P12 → P13. PRs may split further, but no feature is
marked shipped until its acceptance gate passes.

## 7. Benchmarks and exit criteria

Fixtures: Rust bugfix, DayZ Enforce Script mod, Vue/TypeScript project,
large logs, multi-file patch, injected README, expired provider quota,
failed provider, context overflow, cancellation and resume.
Measure input/output/cache tokens, transmitted bytes, API cost, wall time,
peak RAM/VRAM, exact-match tool integrity, test pass rate and patch conflicts.
Compare Safe/Balanced/Aggressive on identical tasks and models.
Release gate: no secret exfiltration, no unauthorized writes, no silent
cloud requests, no irreversible compression loss, successful Windows CI
and actual native Windows test. Publish unsupported upstream features explicitly.

## 8. Governance and status tracking

Maintain `docs/integrations/PARITY.md`: every upstream feature has
upstream path/revision, Lyra component, status
(planned / implemented / verified / unsupported), test and reason.
Review upstream changes periodically; never auto-merge upstream code.
Use feature flags for incomplete modules; optional sidecar must not be
a hidden mandatory dependency. Keep existing Lyra alpha functional
throughout migration.

## 9. First executable work after this plan

1. Audit both upstream trees and lock revisions/licenses in parity matrix.
2. Implement P01 with mock tests before adding any provider credentials.
3. Land P02/P04 independently, then join them through shared usage metadata.
4. Move the current context slider into the shared context pipeline when
   the migration and fidelity tests exist.
