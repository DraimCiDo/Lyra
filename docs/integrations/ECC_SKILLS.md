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
