# ECC + OmniRoute parity tracker

Source projects: [ECC](https://github.com/affaan-m/ECC) (also [Everything Claude Code](https://github.com/affaan-m/everything-claude-code)) and [OmniRoute](https://github.com/cheaper-inference/OmniRoute).
See [integration plan](../plans/0010-unified-ecc-omniroute.md).

**Status semantics:** planned = not implemented; implemented = code exists but not
necessarily validated; verified = tests + Windows acceptance recorded; unsupported
= consciously excluded with rationale. Do not infer status from upstream claims.
Upstream revision/license audit is P00 and remains pending.

| Source | Feature family | Lyra destination | Status | Acceptance |
| --- | --- | --- | --- | --- |
| ECC | Sequential researcher/coder/reviewer roles and proposal/apply | src/agent.rs | implemented before integration | Existing agent tests; not worktree orchestration |
| ECC | Worktrees, checkpoints and extended orchestration | Agent Runtime | planned | P07 |
| ECC | JSON packs, bounded SKILL.md import, explicit selection | src/skills.rs / src/agent_runtime.rs | implemented | Unit tests / gateway tests; full YAML and resources pending |
| ECC | Selective install of complete upstream packs | Skills Registry | planned | P06 |
| ECC | Rules, commands, language packs | Rules/Commands | planned | P06/P12 |
| ECC | Hooks | Event Bus | planned | P08 |
| ECC | MCP | Tools Adapter | planned | P08 |
| ECC | Session memory and checkpoints | Memory | planned | P09 |
| ECC | Instincts/continuous learning | Learning | planned | P09 |
| ECC | Token optimization | Context Optimizer | planned | P04/P05 |
| ECC | Security/verification/evals | Security/Verification | planned | P10 |
| OmniRoute | Provider-neutral DTOs, output budget/capability validation, allowlist | src/model_gateway.rs | implemented | Contract unit tests; complete P01 not claimed |
| OmniRoute | OpenAI-compatible text HTTP and local GGUF adapters | src/provider_http.rs / src/agent_runtime.rs | implemented | Loopback HTTP tests / CPU GGUF smoke; paid APIs unverified |
| OmniRoute | Registry, ordered CLI chat fallback, transient error filtering | src/routing.rs / src/lib.rs | implemented | Stub/HTTP fallback tests; task uses one selected backend |
| OmniRoute | Weighted/cost/quota routing, health tracking, provider catalog | Router / Gateway Policy | planned | P03/P12 |
| OmniRoute | Shared cloud-task timeout | src/agent.rs / src/provider_http.rs | implemented | Expired-deadline and multi-call/body-read regression tests |
| OmniRoute | 12 compression engine categories | Context Optimizer | planned | P04/P05/P12 |
| OmniRoute | Cache, memory, cost telemetry | Cache/Usage | planned | P05/P09 |
| OmniRoute | MCP/A2A/ACP, API endpoints | Protocols | planned | P08/P12 |
| OmniRoute | Security, PII, plugins | Policy/Plugins | planned | P10/P12 |
| OmniRoute | Multimodal and remote operation | Gateway Extensions | planned | P12 |
| OmniRoute | Transparent MITM / TLS impersonation | None | unsupported | Requires separate review |
| Lyra | Basic Desktop history window, shared across chat/task/project | src/desktop/chat.rs | integrated from PR #2 into PR #3 | Exact 12/6 boundaries and unchanged latest request tests |
| Lyra | Opt-in Desktop HTTP chat/task and skill selection | src/desktop.rs | implemented | Native smoke + unit tests; full interactive acceptance pending |
| Lyra | CUDA Auto/Balanced resource estimates | src/resources.rs / src/inference.rs | implemented | Metadata/budget tests; GPU runtime unverified |

P00 must replace these families with a granular feature inventory pinned to
exact upstream SHA and link each to a test. A broad family row is not proof of
feature parity.

## Verification evidence

On pre-fix head `caeed74`, [Windows CI](https://github.com/DraimCiDo/Lyra/actions/runs/41310505003)
passed Desktop and HTTP Clippy/tests, CPU GGUF smoke, release build and Desktop
launch smoke. The Linux job exhausted disk while restoring a whole-target cache;
the integration fix starts a smaller v2 cache and disables CI debug/incremental data.
[CUDA CI](https://github.com/DraimCiDo/Lyra/actions/runs/41310505143) failed to find
the Visual Studio CUDA toolset; the fix includes its installation and a preflight.
These historical results do not verify the modified head. See
[integration fixes](../plans/0011-pr-integration-fixes.md) for current checks.
No live paid-provider or NVIDIA GPU quality/runtime acceptance is claimed.
