"""Bundle verbatim registry dependency notices. Build-time tool, not a CLI runtime dependency."""

import argparse
import json
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--target", required=True)
    args = parser.parse_args()
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--format-version", "1", "--filter-platform", args.target], text=True
    ))
    sections = [
        "Lyra dependency notices\n"
        "Includes resolved registry packages for the selected build target.\n"
        "These licenses apply to the dependencies, not to Lyra itself.\n"
    ]
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    pending = [metadata["resolve"]["root"]]
    reachable = set()
    while pending:
        package_id = pending.pop()
        if package_id in reachable:
            continue
        reachable.add(package_id)
        pending.extend(dep["pkg"] for dep in nodes[package_id]["deps"])
    for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
        if package["id"] not in reachable:
            continue
        if package["source"] is None:
            continue
        root = Path(package["manifest_path"]).parent
        notices = sorted(p for p in root.iterdir() if p.is_file() and p.name.upper().startswith(
            ("LICENSE", "LICENCE", "COPYING", "COPYRIGHT", "NOTICE")
        ))
        if not notices:
            raise SystemExit(f"Missing license text: {package['name']} {package['version']}")
        sections.append(
            f"\n{'=' * 72}\n{package['name']} {package['version']}\n"
            f"License expression: {package.get('license') or 'unspecified'}\n"
        )
        for notice in notices:
            sections.append(f"\n--- {notice.name} ---\n{notice.read_text(encoding='utf-8')}\n")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text("\n".join(sections), encoding="utf-8")
    print(f"Dependency notices written to {args.output}")


if __name__ == "__main__":
    main()
