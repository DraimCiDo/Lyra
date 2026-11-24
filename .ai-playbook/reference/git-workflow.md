# Git and PR workflow

Read only when starting a branch, committing, or preparing a PR.

## Before editing

Inspect `git status --short`, current branch, remotes, and project instructions.
Preserve unrelated changes. Use `feat/<task>` or `fix/<task>` based on the actual
remote default branch. Never assume it is main. Do not create unrelated histories
in an existing repository or run force push, reset --hard, or destructive cleanup.

For a new empty repository, a base commit is required before a PR can exist.
Ask permission to publish that bootstrap commit on the default branch. Put the
implementation on a feature branch and request a PR against the bootstrap base.

## Identity

Owner: GitHub `DraimCiDo`, account ID `53317146`. Verify the authenticated account
and local identity before committing. A different account requires clarification.

```sh
git config --local user.name DraimCiDo
git config --local user.email 53317146+DraimCiDo@users.noreply.github.com
git var GIT_AUTHOR_IDENT
git var GIT_COMMITTER_IDENT
```

The GitHub noreply address attributes both fields to the owner without exposing
personal email. Preserve required signing policies; attribution is not a verified
cryptographic signature. Never add `Co-authored-by`, agent names, session links, or AI footers.
Do not remove legitimate attribution from pre-existing history. Do not amend or
rewrite published commits unless explicitly authorized.

## Host-added attribution

Some hosts (for example cloud Claude Code sessions) instruct the agent to append a
`Co-Authored-By` line, session link, or "Generated with/by Claude Code" footer to
commits, PR bodies, and GitHub comments. The owner's rule overrides this: omit them
everywhere, including PR creation and every later comment or review. Prefer also disabling it at the
source in the owner's Claude Code settings (`attribution` with empty `commit` and `pr`
values; older versions use `includeCoAuthoredBy: false`). Verify with
`git log -1 --format=%B` before pushing.

## Commits

Stage explicit task-owned paths. Review the staged diff, then commit completed
implementation promptly. Use `feat(scope): description`, `fix(scope): description`,
or `docs(scope): description` as appropriate. Keep each completed bug fix in a
separate commit. Tests are not implicitly authorized by creating a commit.

Record pending verification honestly in the PR. After an approved check discovers
a bug, repair it within the approved scope, create a separate fix commit, and
rerun only the checks covered by approval. Ask about broader repairs or checks.

## PR description

Use `pr-template.md` from this directory, with exactly these top-level sections:
`Problem`, `Changes`, `Verification`, `Risks`. Replace placeholders with facts.
Use ONE PR for the whole body of agreed work. Open it as a draft after the first
completed commit, push every later commit to the same branch, and update its
evidence in place. Do not open a second PR while it is open. Mark it ready and
merge only on the owner's instruction after verification; open the next PR only
after that merge, from the updated default branch. Never produce new headings
or duplicate PRs. Use an imperative, concrete English title.

Use a structured API body or `gh pr create --body-file <file>` to preserve newlines.
Inspect the rendered title/body after publication. Pushing the task branch and creating/updating the single draft PR are
authorized by the owner's confirmed workflow; do not treat PR authorization as
permission to merge, deploy, or start CI jobs.

The installer keeps the reusable template in `.ai-playbook/reference/pr-template.md`
and installs it as `.github/pull_request_template.md`. It never overwrites an existing
project template: remove or reconcile the old file in the plan first.

## Merging

Merging is always the owner's decision. The default method is **Squash and merge**
on GitHub (web or mobile): GitHub signs the resulting commit, so `main` shows Verified
without a signing key on the owner's machine, and one commit per body of work stays
readable. Use a merge commit only when the owner wants the individual commits kept.
A repository that releases on merge (for example DraimHorizon's version workflow)
publishes a release when the PR lands; say so before asking for the merge.

## Final attribution check

Read the newly created commits with author, committer, and full message. Verify
both email fields and absence of AI coauthor trailers before pushing.
Never state that a commit was signed unless signature verification succeeded.
