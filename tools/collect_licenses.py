"""Bundle verbatim registry dependency notices. Build-time tool, not a CLI runtime dependency."""

import argparse
import json
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--target", required=True)
    parser.add_argument("--features", default="")
    args = parser.parse_args()
    command = ["cargo", "metadata", "--locked", "--format-version", "1", "--filter-platform", args.target]
    if args.features:
        command += ["--features", args.features]
    metadata = json.loads(subprocess.check_output(command, text=True, encoding="utf-8"))
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
        pending.extend(dep["pkg"] for dep in nodes[package_id]["deps"]
                       if any(kind["kind"] is None for kind in dep["dep_kinds"]))
    for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
        if package["id"] not in reachable:
            continue
        if package["source"] is None:
            continue
        root = Path(package["manifest_path"]).parent
        notices = sorted(p for p in root.iterdir() if p.is_file() and p.name.upper().startswith(
            ("LICENSE", "LICENCE", "COPYING", "COPYRIGHT", "NOTICE")
        ))
        vendored = {
            ("accesskit", "0.25.2"): "accesskit",
            ("clipboard-win", "5.4.3"): "clipboard-win",
            ("profiling", "1.0.18"): "profiling",
            **{(name, "0.37.0"): "egui" for name in (
                "ecolor", "eframe", "egui", "egui-wgpu", "egui-winit",
                "egui_glow", "emath", "epaint", "epaint_default_fonts")},
        }.get((package["name"], package["version"]))
        if vendored:
            notices += sorted(Path("third_party", vendored).glob("LICENSE*"))
        if package["name"] == "epaint_default_fonts":
            notices += sorted((root / "fonts").glob("*.txt"))
        if package["name"] == "llama-cpp-sys-2":
            notices += sorted(p for p in (root / "llama.cpp").rglob("*")
                              if p.is_file() and p.name.upper().startswith(("LICENSE", "COPYING", "NOTICE")))
        if package["name"] in ("llama-cpp-2", "llama-cpp-sys-2"):
            for notice in sorted(Path("third_party/llama-cpp-rs").glob("LICENSE-*")):
                sections.append(f"\n--- {package['name']} {package['version']} / {notice.name} ---\n{notice.read_text(encoding='utf-8')}\n")
            if not notices:
                continue
        if not notices:
            raise SystemExit(f"Missing license text: {package['name']} {package['version']}")
        sections.append(
            f"\n{'=' * 72}\n{package['name']} {package['version']}\n"
            f"License expression: {package.get('license') or 'unspecified'}\n"
        )
        for notice in notices:
            sections.append(f"\n--- {(notice.relative_to(root) if notice.is_relative_to(root) else notice)} ---\n{notice.read_text(encoding='utf-8')}\n")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text("\n".join(sections), encoding="utf-8")
    print(f"Dependency notices written to {args.output}")


if __name__ == "__main__":
    main()
