# Agent hub: several agents on one project

Status: implemented (preview); acceptance with several real agents pending.
Branch and base revision: `feat/lyra-v2-planning` (PR #7), on top of `6c0d42b`.

## Result and boundaries

The owner asked that several Codex, several Claude Code and local Lyra agents can connect to
Lyra and work on one project at the same time, with every function working.

In scope: a shared local task board, per-task git worktrees, advisory file leases, messages,
an MCP stdio server for external agents, a local GGUF worker, owner-only landing and
cancellation, documentation. Out of scope: a Desktop view for the hub, Lyra calling external
MCP servers (MCP client), automatic task splitting by a model, network transport.

## Decisions

- **Storage:** one SQLite database (WAL, 10 s busy timeout) in a hub folder outside the
  project. `rusqlite` is already a dependency; no new dependency. Separate schema with its own
  `application_id`, not the v1 session store.
- **Isolation:** each claimed task gets `lyra/task-N` and a worktree under the hub folder.
  Agents never share a checkout, so simultaneous edits cannot overwrite each other. Leases are
  advisory coordination for overlapping tasks, not a security boundary.
- **Transport:** MCP over stdio (newline-delimited JSON-RPC 2.0), because Codex and Claude
  Code both start MCP servers as subprocesses. Protocol versions 2025-06-18, 2025-03-26 and
  2024-11-05 are accepted. The agent name is fixed by the owner's client configuration.
- **Authority:** agents can create, claim, hand off and release tasks, lease paths, message and
  run the owner's check. Only the owner lands (merges) and cancels. Landing requires a committed
  worktree, a clean base checkout on the base branch and, with a check configured, a passing
  check recorded for the exact branch commit. Conflicting merges are aborted.
- **Local worker:** reuses the v2 runtime and `TaskStore` in the task worktree; one model load
  serves `--loop`. It commits with the project's git identity.

## Acceptance criteria

- [x] Concurrent claims from several agents never hand out a task twice; each task has its own
  branch and worktree (test with four threads).
- [x] Dependencies, ownership and allowed transitions are enforced (test).
- [x] Leases are all-or-nothing, case-insensitive and cover folders (test).
- [x] Messages reach direct and broadcast recipients once (test).
- [x] Landing refuses uncommitted work, a dirty base and a check that did not pass on the exact
  commit; it merges a reviewed branch (tests).
- [x] MCP initialize, tools/list, tools/call and error codes (test); a real Claude Code CLI
  connected (`claude mcp list` → Connected) on Linux.
- [x] A local worker turns a hub task into a committed branch in review; no-change runs release
  the task (tests with a scripted model).
- [ ] Two real Codex and two real Claude Code sessions finish separate tasks on one project.
- [ ] Native Windows run (paths with spaces/Cyrillic are covered on Linux only).

## Checks run

See the PR #7 verification section for the exact commands and revision.
