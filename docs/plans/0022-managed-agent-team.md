# Managed agent team

Status: implementation in progress. Base: PR #7, fc91d5b.

## Approved result and scope

Owner approved solo implementation and checks on 2026-11-24. Lyra manages configured
Codex, Claude Code and local GGUF workers: start/stop, isolated task assignment,
bounded execution, status and errors, checked review handoff. A local model proposes
an editable task/dependency plan; the owner approves it before enqueueing. Landing
remains explicit. External workers require explicit context-upload consent.

Add a versioned local team JSON configuration without changing the hub database
schema. No new libraries. CLI adapters use documented argv, never generated shell
commands. Reuse the existing task board, worktrees, checker and local worker.

## Intended commits

1. Managed worker lifecycle, CLI configuration and regression tests.
2. Desktop team controls, reviewed planning, documentation and UI coverage.
3. Focused fixes found by the approved checks, if needed.

## Approved checks

In /workspace/Lyra, Linux (Docker Rust 1.99.1 if host Rust is absent):
- cargo fmt --all -- --check
- cargo clippy --all-targets --locked -- -D warnings
- cargo clippy --all-targets --locked --features desktop -- -D warnings
- cargo clippy --all-targets --locked --features desktop,gateway-http -- -D warnings
- cargo test --locked
- cargo test --locked --features desktop
- cargo test --locked --features desktop,gateway-http
- Build lyra-v2-desktop and run its real window under Xvfb; inspect screenshot.

Lifecycle tests use fake CLI programs, no paid inference. Push triggers CI.
Native Windows and real multi-provider acceptance require separate evidence.

## Acceptance

- [ ] One start manages 2 Codex, 1 Claude, 1 local worker without shared checkouts.
- [ ] External consent, limits and model choices validated before spawning.
- [ ] Stop, timeout, process failure preserve work and never land automatically.
- [ ] Failed tasks do not loop; dependencies resume after owner landing.
- [ ] Plan validation and enqueue are atomic; no model text executes as commands.
- [ ] Desktop shows live state only while explicitly enabled; closes stop workers.

## Evidence and handoff

Read-only inspection completed; working branch feat/lyra-v2-planning. No checks run yet.
