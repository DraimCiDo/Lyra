# Verification protocol

Read when proposing or performing checks. No stack-wide command is assumed.

## Approval request

State exact commands or actions, working directory, environment, dependencies,
expected duration/resource use, and whether external services or mutable data are
involved. A plan that lists a batch of checks and is approved authorizes that batch; ask
again only for commands not listed. A future task does not inherit approval
for unrelated checks. Higher-priority environment requirements still apply.

Suggested request:

> Implementation is committed. (Only needed for checks not already in the approved plan.) May I run [commands] in [environment]?
> Expected cost: [estimate or unknown]. This covers [scope].

Do not install dependencies or execute package scripts as "read-only inspection".
Do not launch applications, Studio sessions, servers, benchmarks, CI workflows,
or automatic retry loops as an implicit consequence of implementation approval.
If Git hooks or push-triggered CI will execute checks, disclose that before the
operation and obtain authorization rather than silently disabling required hooks.

## Sequence

1. Inspect the diff for obvious mistakes and unrelated edits without executing it.
2. Run the approved static/error checks, if any.
3. Run the approved focused tests. Broader suites require approval if not included.
4. Diagnose failures; distinguish code defects, baseline failures, and environment issues.
5. Commit each authorized fix separately and rerun the relevant approved checks.
6. Report pass/fail, relevant evidence, and unrun checks. Do not turn failure into success
   by weakening assertions, disabling checks, or changing unrelated configuration.

Repeat a passed check only after new changes or unresolved evidence. Check the
real exit code of each command, not its summary line.

An expired approval (different environment, materially changed command, new costs,
or expanded scope) needs clarification. A documented pending check is preferable
to a fabricated result. "No check run" does not mean "no bugs".

For approval-policy changes, use scenario review rather than claiming Markdown
rules enforce permissions. Discuss a small fix, dependency addition, schema change,
server load test, paired review, and uncommitted work with the owner.
