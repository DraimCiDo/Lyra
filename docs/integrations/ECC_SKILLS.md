# ECC SKILL.md import

Lyra supports importing an individual declarative `SKILL.md` file from
[Everything Claude Code](https://github.com/affaan-m/ECC) or other Agent Skills
packs, alongside Lyra's JSON skill manifests.

```bash
cargo run -- skills --manifest path/to/SKILL.md
cargo run -- skills --manifest path/to/SKILL.md --id rust-review
```

The importer reads the `name` and `description` frontmatter fields and
preserves the Markdown body as instructions. It validates identifiers, bounds
file size and instruction length, and never executes embedded shell commands,
hooks or scripts. Full YAML support, nested resources, and execution of ECC
hooks are **not** implemented.

ECC is MIT-licensed. When distributing third-party ECC content, retain its
license and attribution. This implementation is a native Rust adapter and does
not vendor ECC's copyrighted skill files.

## Skill-aware agent runtime

The shared `agent_runtime` layer applies only explicitly selected skills.
Instructions are prepended as a system message for OpenAI-compatible HTTP
providers, or supplied as system context to the local GGUF chat template.
Neither backend executes hooks or scripts from a skill.

Local example:

```sh
lyra generate --model /models/coder.gguf --prompt "Review this code" --skills ./SKILL.md --skill rust-review
```

HTTP example:

```sh
lyra chat --provider-config providers.json --allow approved-provider --prompt "Review this code" --skills ./SKILL.md --skill rust-review
```

A missing or duplicate selected skill fails before the model request. Skills
are not automatically discovered or activated. Local GGUF requests do not
automatically switch to cloud providers.

### Desktop integration

When built with `desktop,gateway-http`, Desktop HTTP provider settings also
accept a skill manifest path (`skills.json` or `SKILL.md`). Select a skill
from the validated manifest or choose **Без навыка**. The selected skill is
injected into HTTP chat requests by the shared runtime; it is never executed
as a script. The local project task workflow now consumes the same Desktop skill selection.
The skill guidance is injected into each local agent role's task envelope
(researcher, coder, reviewer) without granting new file or shell permissions.

### Local project task

```sh
lyra task --repo ./project --model ./model.gguf --task-file task.txt --skills skills.json --skill rust-review
```

The task pipeline retains its existing constrained JSON tool grammar and
proposal-only editing. Skill guidance does not override read-only agent roles
or permit unapproved writes.
