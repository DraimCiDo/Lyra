# Codex and Claude coordination

Read only after the owner selects paired mode. Paired mode does not automatically
authorize spawning subagents or additional paid sessions.

## Start

Confirmed default: one editor and one read-only reviewer; state who is which and
proceed unless the owner changes it. Separate implementations only on request. The reviewer reads the approved
plan and diff, identifies concrete problems, and does not edit, run tests, or
start tools that execute project code without authorization.

Copy `task-template.md` to `.ai-playbook/TASK.md` for the task. Record the owner's
actual approvals, not inferred consent. Include the branch and exact commit being
reviewed. Avoid long transcripts, secrets, copied logs, or duplicated source code.

## Handoff

1. The editor commits completed work and updates the task record.
2. The owner opens the other app and asks it to read the project instructions and
   `.ai-playbook/TASK.md`, then review the recorded commit/diff.
3. The reviewer records findings with file, location, impact, and evidence.
4. The editor implements approved fixes, commits each fix, and records outcomes.
5. Run only checks explicitly approved for the task. Report unrun checks.

Separate apps do not automatically share chats, approval state, or tools. This is
a file-based handoff protocol, not a background bridge between subscriptions.
For mobile access, commit/push a sanitized task record only with authorization;
local unpushed files are unavailable to remote sessions.

## Simultaneous editing

Use separate Git worktrees and branches. Assign disjoint scopes and one integration
owner. Never have two agents edit the same working tree concurrently. Worktrees
share history but their uncommitted task files differ: exchange commit IDs or an
explicitly shared coordination document. There is no automatic distributed lock.

The integration owner reviews the source commits and applies/cherry-picks them
only within the approved plan. Preserve owner attribution. Ask before resolving
conflicts that change agreed behavior. Do not merge the PR without approval.

## Token discipline

Do not run both agents on every small change. Ask for a review of the relevant
diff and a short list of findings. One review pass is the starting point; further
passes need a concrete unresolved concern. Two agents can consume more total quota.
