# ECC SKILL.md import

Lyra supports importing an individual declarative `SKILL.md` file from
[Everything Claude Code](https://github.com/affaan-m/ECC) or other Agent Skills
packs, alongside Lyra's JSON skill manifests.

An explicitly selected directory is also accepted: `SKILL.md` in its root and
in immediate child directories forms one catalog, sorted by skill id.
Duplicate ids or any invalid member reject the entire catalog. Directory
symlinks are not discovery roots; canonical paths outside the catalog are
rejected. This is bounded path validation, not a filesystem sandbox.

```bash
cargo run -- skills --manifest path/to/SKILL.md
cargo run -- skills --manifest path/to/SKILL.md --id rust-review
cargo run -- skills --manifest path/to/selected-skills
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
The GGUF adapter passes individual system/user/assistant messages into the
model's chat template, preserving turn boundaries. Explicit `--raw` mode is
for base/test models and bypasses that template; it joins message content as
plain text and does not provide structured chat roles.

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

In **Помощники**, provide a directory, `skills.json`, or `SKILL.md`, then click
**Загрузить каталог навыков**. Descriptions and instruction previews are available;
select multiple skills or clear the selection. HTTP support is not required.
The catalog is read on explicit reload and validated again at worker startup.
Selections persist in order of selection, which is also the guidance order.
Missing skills and context-budget overflow fail before the model request.
Legacy single-skill settings remain readable. Selected guidance is injected into
local/HTTP chat and project tasks by the shared runtime; project planning does
not consume it yet. No scripts are executed.
The skill guidance is injected into each local agent role's task envelope
(researcher, coder, reviewer) without granting new file or shell permissions.

### Local project task

```sh
lyra task --repo ./project --model ./model.gguf --task-file task.txt --skills skills.json --skill rust-review
```

The task pipeline retains its existing constrained JSON tool grammar and
proposal-only editing. Skill guidance does not override read-only agent roles
or permit unapproved writes.

Repeat `--skill` to select several skills from a catalog or JSON pack:

```sh
lyra task --repo ./project --model ./model.gguf --task-file task.txt --skills ./selected-skills --skill rust-patterns --skill review
```

Catalog limits: 512 root entries, 256 skills, 4 MiB aggregate content; each file
is bounded to 64 KiB and each instruction body to 16 KiB. The existing selected
guidance budget still applies. Resources and nested directories are not read.
A complete upstream checkout can exceed these bounds; use a curated local
directory. Package versioning/rollback, rules, role scopes and command palettes
remain separate work; see [stage 2a](../plans/0015-ecc-skill-catalog.md).

## Installed snapshots and rollback

Install a selected subset into a local `*.lyra-skills.json` snapshot:

```sh
lyra skill-package install --source ./selected-skills --store ./ecc.lyra-skills.json --id ecc-local --version revision-1 --origin "ECC pinned local checkout" --skill rust-patterns --skill review
lyra skill-package show --store ./ecc.lyra-skills.json
lyra task --repo ./project --model ./model.gguf --task-file task.txt --skills ./ecc.lyra-skills.json --skill review
lyra skill-package rollback --store ./ecc.lyra-skills.json
```

The snapshot stores normalized declarative content, package id, declared version
and origin, and a SHA256 over the serialized SkillPack. Source edits do not change
an installed snapshot. Installation does not copy resources/scripts or access
the network. Metadata is user-declared provenance, not a verified upstream
signature or license grant; preserve third-party attribution when distributing
the snapshot. Never publish private skill content accidentally.

Update the same store with a different version label to retain the previous
current revision. One previous revision is kept; rollback swaps the two revisions
and can be reversed. A retained version cannot be redefined with different content
or origin. Exact reinstall is a no-op. There is no global version history or
semantic-version upgrade policy. Stores are bounded to 16 MiB serialized, each
revision to 4 MiB content and the existing per-skill limits. Runtime selection
still applies its 32 KiB guidance budget independently of installation.

Writes use a same-directory temporary file and replacement under a create-new
`.lock` file. Failed validation leaves the old store intact. A leftover lock after
a crash is never stolen automatically: ensure no Lyra operation uses the store
before manually removing that lock. This coordinates cooperating Lyra processes;
it is not an OS sandbox or protection against another writer modifying files.

Desktop: in **Помощники**, select skills, expand **Пакет навыков · установка и
версии**, enter id/version/origin and choose **Установить выбранные навыки и
использовать снимок**. The store is placed beside local settings in `skills/`;
the manifest path switches to the snapshot and the selection is retained.
Load an installed snapshot to inspect metadata and switch to its previous version.
Rollback preserves selected ids; missing ids are reported and block worker startup.
See [stage 2b](../plans/0016-ecc-package-versions.md) for acceptance status.
