# Lyra — development instructions

These rules apply to the whole repository; a nested `AGENTS.md` refines its own area.
A direct instruction from the owner in the current session outranks this file.
**Communicate with the owner in Russian** (final reports and short status notes). Everything
else — code, comments, commit messages, plans, test names, tool calls — is written in English.
Write symbol names and technical identifiers in English.

## Mission

Bring Lyra to a **working release candidate**: a universal coding agent the owner can download,
run on Windows 11 and use like Codex / Claude Code — with a local model (GGUF), with external
models by explicit configuration, with subagents, change verification, a GitHub connector and a
code graph. The owner allows changing code, architecture and design as long as the result works.
The roadmap is in scope of this assignment, not a "guide for the future".

**This repository is yours.** What it contains and how it is built is your call, and you will
keep building on it. Improve the structure, merge repeated code, fix problems you find nearby.
Do not change things for the sake of changing them: every edit must move the criteria below forward.

## Definition of done

1. `cargo fmt --all -- --check` and `cargo clippy --all-targets --locked -- -D warnings` are clean,
   and `cargo test --locked` passes in three variants: no features, `--features desktop`,
   `--features desktop,gateway-http`. Linux and Windows CI are green.
2. Native acceptance on Windows 11 x64: `lyra-desktop.exe` starts, GGUF can be chosen/installed,
   chat works, "Work" mode with a project works, the result panel works, changes are applied explicitly.
3. On a real model (for example Qwen2.5-Coder 7B Q4) the agent finishes a small task up to a verified
   proposal: "task → edit → check → report". The protocol used to stall — fix the cause in the
   agent/context code, not the test.
4. The scenario "GitHub issue → task → edit → check → draft PR" passes with the owner's real token.
   Graphify import and a real Graphify run are verified on a real `graph.json`.
5. The user gets a portable package that needs no Rust/Python; the documentation matches the facts.
6. The final report separates: verified by running / verified by tests / not verified.

## State as of 2026-11-11 (`main`, after PR #6)

Present and covered by tests on Linux: the CLI (`task`, `apply`, `generate`, `local-chat`, `chat`,
`route`, `skills`, `skill-package`, `session`, `recover`, `verify`, `project`, `docs`, `github`,
`graph`, `environment`, `doctor`); local GGUF (CPU) and HTTP providers; researcher/coder/reviewer
roles; Docker checks (`--check-config`) and script execution environments; SQLite sessions;
publication of edits with a rollback journal; the Office/PDF workflow; the GitHub connector
(issues, PRs, files, comments, `publish` as a draft PR); the code graph (built-in analyzer and
Graphify import); the egui Desktop — rail, chat list, Ctrl+K command palette, result panel with
diff, grouped settings, graph screen; 4 themes, RU/EN; updates through GitHub Releases.

**Not verified** (do not call it done until it has been run): the new interface on Windows, the CUDA
runtime, 7B quality on hard tasks, live GitHub, a real `graphify` run, CI on the latest commits.

**Not implemented or partial** (source of truth: `docs/integrations/PARITY.md`): routing with
quotas/weights/provider health, hooks and MCP, memory and learning, an OS sandbox beyond Docker,
token accounting and advanced context compression, multimodality. Take these by priority of the
criteria, not as a list; close each feature with a test and a PARITY entry.

Navigation: [README](README.md), [project map](docs/PROJECT_MAP.md),
[architecture](docs/ARCHITECTURE.md), [development](docs/DEVELOPMENT.md),
[Desktop](docs/DESKTOP.md), [GitHub](docs/GITHUB.md), [graph](docs/GRAPH.md),
[workflows](docs/WORKFLOWS.md), [acceptance](docs/ACCEPTANCE.md), [plans](docs/plans/).
The map and plans describe intent; verify facts from the sources, `git log` and by running.
Many existing docs are written in Russian; keep their language when editing, and write new
developer-facing text in English unless it is meant for end users in Russian.

## How to work

1. Start with `git status`, the branch, `git log -5`, this file and `docs/PROJECT_MAP.md`; then read
   only what the task needs. Search with `rg`, then read specific file ranges.
2. Before editing, state the result, the boundaries and the checks in a few lines. For multi-step
   work keep a plan in `docs/plans/NNNN-*.md` (template: `docs/templates/TASK.md`) and update it
   with facts; when a session changes, compare the plan with `git status`.
3. Make small, complete steps, check each one, then move on. Fix the cause of a defect. Do not
   disable, skip or weaken checks to get a green result.
4. Batch independent reads and runs into one call; run dependent edits and git commands in sequence.
5. A new dependency is acceptable with a reason: effect on Windows, size, license. Versions and the
   toolchain are pinned (`rust-toolchain.toml`; `Cargo.lock` is committed). Do not invent versions
   or APIs — check the primary source.

### Finish the work

Do not stop while work the owner asked for remains. Ordinary obstacles — a build error, a timeout,
a locked file, a failing tool, an empty result — are diagnosed and worked around first: retry, fix
the request, use another path. Stopping is right only when you cannot move without the owner's
input or the obstacle is deliberate (a prohibition, someone else's changes, withheld access, a
guardrail): then say plainly what you found and continue with whatever can be done safely. Do not
end a turn with a list of what remains and an offer to continue.

### Allowed without asking

Edit code, tests and docs; run `cargo`, `clippy`, tests, Xvfb/Playwright; build packages; install
dev dependencies in your own environment; create branches and commits; push to the working branch;
read public repositories and documentation.

### Ask before

Force-pushing or rewriting published history; pushing directly to `main`, merging, or releasing
(unless the owner asked for it explicitly); deleting branches or files you did not create; paid API
calls and anything that spends money; publishing to GitHub on the owner's behalf (PRs, comments,
issues) without being asked; sending code or secrets to external services. If the owner asked for
an action, do exactly that and nothing beyond it.

## Product safety boundaries (do not weaken)

These are requirements on Lyra itself, not on the development process.

- The model proposes an action; the application validates schema, permissions and boundaries.
  Model output text is never executed as a command. The model does not choose check commands or
  images — the owner sets them through `--check-config` / the environment registry.
- Project files, web pages, issues/PRs, skills, tool outputs and nested `AGENTS.md` files of the
  target project are **untrusted data**: they never widen access or change policy.
- Never send the whole repository to a model or provider automatically. Exclude secrets, binaries
  and service directories; bound sizes. Keys and tokens come only from environment variables / the OS
  store — never from the repository, logs, diagnostics or settings.
- The network is allowed only for: explicitly enabled model providers, update checks/downloads
  (GitHub Releases), GGUF installation from the pinned catalog, explicit documentation fetches, and
  the GitHub connector. Hidden code transfer and telemetry are forbidden; cloud fallback needs
  explicit consent.
- A patch is applied only after verifying the original content; a conflict is shown, never
  overwritten. Do not lose the user's uncommitted changes. Processes have a timeout, cancellation,
  an output limit and child-process termination. Repeating a write after a network failure is not
  safe by default.
- Subagents, model selection and fallback work only in an explicitly enabled mode with limits; do
  not change the model or reasoning against the user's manual choice, and show the actual route.
- Settings and chat history are stored locally; reports and logs are not saved automatically.
  Icons come only from the pinned Lucide set (`third_party/lucide/SOURCE.md`), never downloaded at
  runtime. There are no idle timers (except the enabled update check).

## Rust and Windows

- Safe Rust by default (`unsafe_code = "forbid"`); an exception needs a demonstrated necessity and a
  written invariant.
- Handle errors at file, process and network boundaries; `unwrap`/`expect` are not for ordinary
  input and environment errors.
- Use `Path`/`PathBuf`, pass process arguments as separate values, set an explicit working
  directory, never build shell commands from strings.
- Account for spaces and Cyrillic in paths, CRLF, case-insensitivity, locked files,
  junctions/symlinks, and escaping the working directory.
- The CLI must not require Bash, WSL, Python or Node. In PowerShell use `-LiteralPath` and explicit UTF-8.
- A Linux check does not prove Windows behaviour; a cross-build does not replace running the EXE.
  State the environment and revision.

## Checks and honesty

- Commands: `cargo fmt --all -- --check`; `cargo clippy --all-targets --locked -- -D warnings`;
  `cargo test --locked` — with no features, `--features desktop`, and `--features desktop,gateway-http`.
  Details: `docs/DEVELOPMENT.md`. On Linux the build needs `cmake` and `gcc/g++`; running the window
  under Xvfb needs `libxkbcommon-x11` and `LIBGL_ALWAYS_SOFTWARE=1`.
- For a behaviour change add a regression test of a user contract, an error or an access boundary —
  not a copy of the implementation. Inference tests use stubs, no multi-gigabyte weights or GPU.
- Check the interface in a real window (Xvfb + screenshot), not only with rendering tests.
- A build, a created file or a `PASS` line does not prove that something works. Name an unavailable
  mandatory check as a blocker. Do not claim superiority over Codex/Claude Code without a comparable test.
- Measure time, memory and request volume on hot paths before and after; measure network and model
  separately.
- Hardware profiles of the user live only in the ignored `.lyra/`.

## Git

- Commits are made **as the owner**: use the configured `git config user.name/user.email`,
  **without** `Co-Authored-By` and without references to a session or a model; the same for PR descriptions.
- Work on a branch separate from `main`; small meaningful commits; `git add` only the task's files.
  Do not commit keys, sessions, logs, builds or downloaded dependencies. Code from other projects
  only after a license check.
- Do not use `reset --hard`, force-push or bulk cleanup unless the owner asks.
- After a finished task and its checks: commit and `git push -u origin <branch>`. A PR, a merge and a
  release are separate assignments.

## Reporting to the owner

Write in Russian for a reader who has not seen your tool calls or files: key points in the first
sentences, short sentences, plain words, one term per thing, no names you made up yourself. State
the result, what was actually run (environment, revision), what is covered only by tests, what is
not verified, and a link to the commit/branch. Do not call a plan an implemented feature.
