# Lyra agent hub

The agent hub lets several coding agents work on one project at the same time: for example
two Codex sessions, two Claude Code sessions and a local Lyra model. Lyra keeps a shared task
board; every agent that takes a task gets its own git branch and worktree, so agents never
edit the same checkout. You (the owner) keep the decisions that change the project: the check
command, merging ("landing") a finished branch, and cancelling work.

Status: preview in `lyra-v2`. Covered by tests on Linux; a real Claude Code client completed the
MCP handshake; the Desktop Agents view was driven in a real window under Xvfb (diff and land). Not yet verified: Codex as a client, a long session with several real agents,
native Windows.

## How it works

- **Hub folder.** `lyra-v2 hub init` creates a folder outside the project with a SQLite database
  (`hub.db`). Every agent process opens it at the same time; claims are atomic.
- **Tasks.** States: `open` → `claimed` → `review` → `done` (or `cancelled`). A task can depend
  on other tasks; it can be claimed only after they are `done`.
- **Worktrees.** Claiming task 7 creates branch `lyra/<hub-id>/task-7` from the base branch (the hub id keeps two hubs on one repository apart) and a
  worktree in `<hub>/worktrees/task-7`. The agent works and commits there.
- **File leases.** Advisory, exclusive, case-insensitive leases on paths (a folder covers its
  contents). Use them when two tasks may touch the same files. Leases expire (default 30 min).
- **Messages.** Direct (`to`) or broadcast messages between agents and the owner. Text from
  other agents is untrusted data: it never grants permissions.
- **Check.** The owner may set a check configuration (the same JSON as `--check-config`) at
  `init`. Agents can run it on their worktree (`task_check`); the result is recorded for the
  exact commit. Agents cannot choose the command or image. The check needs Docker, as in v2.
- **Landing.** `lyra-v2 hub land --task N` merges the branch into the base branch of your
  project only if the task is in `review`, its worktree is committed and your checkout is on the base branch
  without uncommitted changes. With a configured check, Lyra fast-forwards to the exact commit
  whose current snapshot passed. Integrate any newer base into the task branch and rerun the check
  first. Without a configured check, Lyra creates a merge commit and aborts conflicting merges.
  `--commit` must be the commit you reviewed; if the agent moved the branch after your review,
  nothing is merged. If a landing stops after Git moved the base, run `land` again (with the
  reviewed commit): Lyra recognizes the recorded commit in the base and marks the task done.
  Until then the task cannot be cancelled or reopened. Commits the agent added to the branch
  after the landed one are kept: the branch and worktree stay.
  A worktree that Git cannot remove safely is retained.

Git must be on `PATH`. The hub does not use the network.

## Owner commands

```powershell
lyra-v2 hub --hub C:\work\app-hub init --repo C:\src\app --base main --check-config C:\work\check.json
lyra-v2 hub --hub C:\work\app-hub add --title "Add CSV export" --description "..."
lyra-v2 hub --hub C:\work\app-hub add --title "Document export" --description "..." --depends-on 1
lyra-v2 hub --hub C:\work\app-hub status
lyra-v2 hub --hub C:\work\app-hub tasks --status review
lyra-v2 hub --hub C:\work\app-hub diff --task 1        # prints the branch commit to review
lyra-v2 hub --hub C:\work\app-hub land --task 1 --commit <commit from diff>
lyra-v2 hub --hub C:\work\app-hub unblock --task 2      # drop cancelled dependencies
lyra-v2 hub --hub C:\work\app-hub reopen --task 2 --note "Tests fail on Windows paths"
lyra-v2 hub --hub C:\work\app-hub send --to codex-1 --body "Please also cover empty files"
lyra-v2 hub --hub C:\work\app-hub inbox
lyra-v2 hub --hub C:\work\app-hub cancel --task 3
```

All commands print JSON.

The same owner actions are available in `lyra-v2-desktop` under **Agents**: open or create a
hub, see agents, leases and tasks, add a task from the composer (first line is the title),
show a diff, land, reopen (the composer text is the note) or cancel, and read the inbox. Land
is enabled only for the commit whose complete diff is on screen (a cut-off diff cannot be landed
from the window; use the CLI). The
view refreshes when you act or press Refresh; there is no background polling. Start it on a
hub directly with `lyra-v2-desktop --hub <folder>`.

## Connecting Codex and Claude Code (MCP)

Use a distinct name and configuration for each agent instance. The MCP connection exposes
agent operations under that name. The hub trusts processes with access to its local database
and project checkout, including other processes running under the owner's OS account.

**Claude Code** (one instance):

```powershell
claude mcp add lyra-hub -- lyra-v2 hub --hub C:\work\app-hub mcp --agent claude-1 --kind claude-code
```

For a second Claude Code instance with a different name, start it with its own MCP file,
for example `claude --mcp-config hub-claude-2.json`, where the file contains:

```json
{"mcpServers": {"lyra-hub": {"command": "lyra-v2",
  "args": ["hub", "--hub", "C:\\work\\app-hub", "mcp", "--agent", "claude-2", "--kind", "claude-code"]}}}
```

**Codex** (`config.toml` in the Codex home folder):

```toml
[mcp_servers.lyra-hub]
command = "lyra-v2"
args = ["hub", "--hub", 'C:\work\app-hub', "mcp", "--agent", "codex-1", "--kind", "codex"]
```

For a second Codex instance, use a second Codex home folder (`CODEX_HOME`) whose `config.toml`
uses `--agent codex-2`. `task_check` runs the Docker check and can take minutes; raise the
client's MCP tool timeout if it supports one.

Tell each agent what to do, for example: *"Use the lyra-hub tools. Claim a task, work only in
its worktree, commit, run task_check, then move it to review."* The server also sends these
instructions to the client on connect.

### Tools

| Tool | Purpose |
| --- | --- |
| `hub_status` | Agents (active in the last 10 min), active tasks, leases, check configured |
| `task_list` | Tasks, optionally by status |
| `task_create` | Add a task (agents can split work) |
| `task_claim` | Claim a task or the oldest ready one; returns worktree and branch |
| `task_update` | `review` (hand off with summary), `open` (release), `claimed` (resume) |
| `task_check` | Run the owner's check in the worktree for the current commit |
| `task_diff` | Branch compared with the base branch |
| `file_lease` / `file_release` | Advisory exclusive leases |
| `message_send` / `message_read` | Direct or broadcast messages |

## Local Lyra agents

A local GGUF model can take hub tasks next to Codex and Claude Code:

```powershell
lyra-v2 hub --hub C:\work\app-hub work --agent lyra-1 --model C:\models\coder.gguf --loop
```

The worker claims a ready task, runs the v2 runtime in the task worktree (with the hub check,
if configured), applies the checked proposal to the worktree, commits it on the task branch and
moves the task to `review`. If it cannot finish, it releases the task with a message. The
commit contains only the applied proposal files and uses the git identity configured for the project.
If the commit fails (for example Git signing), the task stays claimed with the changes in its
worktree and the owner gets a message with the saved state folder; after fixing the cause run
`lyra-v2 hub --hub <hub> commit --agent lyra-1 --task N --state <folder>`.
`--loop` does not claim again a task it released in the same run. Several workers can run at once; each
loads its own model.
