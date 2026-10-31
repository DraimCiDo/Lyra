# ECC + OmniRoute parity tracker

Source projects: [ECC](https://github.com/affaan-m/ECC) (also [Everything Claude Code](https://github.com/affaan-m/everything-claude-code)) and [OmniRoute](https://github.com/cheaper-inference/OmniRoute).
See [integration plan](../plans/0010-unified-ecc-omniroute.md).

**Status semantics:** planned = not implemented; implemented = code exists but not
necessarily validated; verified = tests + Windows acceptance recorded; unsupported
= consciously excluded with rationale. Do not infer status from upstream claims.
Upstream revision/license audit is P00 and remains pending.

| Source | Feature family | Lyra destination | Status | Acceptance |
| --- | --- | --- | --- | --- |
| ECC | Agents, orchestration, worktrees | Agent Runtime | planned | P07 |
| ECC | Validated JSON skill packs and CLI inspection | src/skills.rs / lyra skills | implemented; unverified | Unit tests / Windows CI |
| ECC | Skills, selective install | Skills Registry | planned | P06 |
| ECC | Rules, commands, language packs | Rules/Commands | planned | P06/P12 |
| ECC | Hooks | Event Bus | planned | P08 |
| ECC | MCP | Tools Adapter | planned | P08 |
| ECC | Session memory and checkpoints | Memory | planned | P09 |
| ECC | Instincts/continuous learning | Learning | planned | P09 |
| ECC | Token optimization | Context Optimizer | planned | P04/P05 |
| ECC | Security/verification/evals | Security/Verification | planned | P10 |
| OmniRoute | Provider-neutral contracts and permission-safe priority selection | src/model_gateway.rs | implemented; unverified | P01 unit tests / Windows CI |
| OmniRoute | Providers/model catalog | Model Gateway | planned | P02 |
| OmniRoute | Routing/combo/auto | Router | planned | P03/P12 |
| OmniRoute | Quotas, budget, fallback, resilience | Gateway Policy | planned | P03 |
| OmniRoute | 12 compression engine categories | Context Optimizer | planned | P04/P05/P12 |
| OmniRoute | Cache, memory, cost telemetry | Cache/Usage | planned | P05/P09 |
| OmniRoute | MCP/A2A/ACP, API endpoints | Protocols | planned | P08/P12 |
| OmniRoute | Security, PII, plugins | Policy/Plugins | planned | P10/P12 |
| OmniRoute | Multimodal and remote operation | Gateway Extensions | planned | P12 |
| OmniRoute | Transparent MITM / TLS impersonation | None | unsupported | Requires separate review |
| Lyra | Basic Desktop history window | Desktop chat | implemented in PR #2; verify merge | Tests and Windows CI pending |

P00 must replace these families with a granular feature inventory pinned to
exact upstream SHA and link each to a test. A broad family row is not proof of
feature parity.
