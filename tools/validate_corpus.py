#!/usr/bin/env python3
"""Validate pinned developer fixtures, reference implementations and per-file mutants.

Default mode runs all fixture code through Lyra's Docker executor. --mode
trusted-host explicitly opts in to running ONLY the pinned repository fixtures
and their reference/reversion variants on the developer host, without models.
This mode is for developing the corpus; it never reads or executes model reports.
Neither mode constitutes model, browser, game SDK or server acceptance.
"""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import evaluate


def host_check(config, work, profile):
    args = config["args"]
    if profile in ("python", "backend", "dayz"):
        checker = args[-1].replace("\"/source\"", repr(work.as_posix())).replace("'/source'", repr(work.as_posix()))
        return evaluate.invoke([sys.executable, "-c", checker], 90)
    if profile == "web":
        checker = args[-1].replace("/source/main.mjs", (work / "main.mjs").as_uri())
        return evaluate.invoke(["node", "--input-type=module", "-e", checker], 90)
    checker = args[-1].split("\n", 1)[1].split("\nLYRA_TRUSTED_CHECK\n", 1)[0]
    if profile == "rust":
        checker = checker.replace('"/source/lib.rs"', json.dumps((work / "lib.rs").as_posix(), ensure_ascii=False))
        file = work.parent / "check.rs"
        file.write_text(checker, encoding="utf-8")
        executable = work.parent / ("check.exe" if sys.platform == "win32" else "check")
        compiled = evaluate.invoke(["rustc", "--edition=2024", "--test", str(file), "-o", str(executable)], 90)
        if compiled["exit_code"] != 0:
            return compiled
        return evaluate.invoke([str(executable)], 90)
    if profile == "java_minecraft":
        compiler = ["javac"] if shutil.which("javac") else ["java", "com.sun.tools.javac.Main"]
        file = work.parent / "CaseTest.java"
        file.write_text(checker, encoding="utf-8")
        classes = work.parent / "classes"
        classes.mkdir()
        compiled = evaluate.invoke([*compiler, "-encoding", "UTF-8", "-d", str(classes),
                                   *map(str, work.glob("*.java")), str(file)], 90)
        if compiled["exit_code"] != 0:
            return compiled
        return evaluate.invoke(["java", "-cp", str(classes), "CaseTest"], 90)
    raise ValueError("Unsupported corpus profile")


def validate(case, root, binary, mode):
    original = evaluate.inside(root, case["source"])
    reference = evaluate.inside(root, case["reference"])
    changed = [name for name in case["reference_sha256"]
               if case["reference_sha256"][name] != case["sha256"].get(name)]
    variants = [("baseline", None), ("reference", None), *[("revert", name) for name in changed]]
    outcomes = []
    for kind, name in variants:
        with tempfile.TemporaryDirectory(prefix="lyra-corpus-trusted-") as folder:
            work = Path(folder) / "project"
            shutil.copytree(original if kind == "baseline" else reference, work)
            if kind == "revert":
                target = evaluate.inside(work, name)
                if name in case["sha256"]:
                    shutil.copyfile(evaluate.inside(original, name), target)
                else:
                    target.unlink()
            config = evaluate.inside(root, case["check_config"])
            if mode == "trusted-host":
                run = host_check(json.loads(config.read_text(encoding="utf-8")), work, case["profile"])
                actual = run["exit_code"]
                valid = not run["timed_out"] and actual == (0 if kind == "reference" else case.get("baseline_exit_code", 1))
            else:
                run = evaluate.invoke([str(binary), "verify", "--repo", str(work), "--config", str(config)], 120)
                body = run["body"] or {}
                actual = body.get("exit_code")
                valid = (body.get("cleanup_confirmed") is True and not body.get("timed_out") and
                         not body.get("cancelled") and actual == (0 if kind == "reference" else case.get("baseline_exit_code", 1)))
            outcomes.append({"variant": kind, "reverted_file": name, "passed": valid,
                             "observed_exit_code": actual, "diagnostic": run["diagnostic"] if not valid else ""})
    return {"id": case["id"], "profile": case["profile"], "acceptance_level": case.get("acceptance_level", "unit"),
            "passed": all(run["passed"] for run in outcomes), "variants": outcomes,
            "note": "Fixture validation only; no model or external SDK acceptance"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--suite", type=Path, default=Path(__file__).resolve().parents[1] / "evals/projects/suite.json")
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--mode", choices=["docker", "trusted-host"], default="docker")
    parser.add_argument("--profile", action="append", default=[])
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.mode == "docker" and (not args.binary or not args.binary.is_file()):
        parser.error("Docker mode requires --binary pointing to built Lyra")
    suite = evaluate.load_suite(args.suite)
    results = []
    for case in suite["cases"]:
        if args.profile and case["profile"] not in args.profile:
            continue
        try:
            result = validate(case, args.suite.parent.resolve(), args.binary.resolve() if args.binary else None, args.mode)
        except (OSError, RuntimeError, ValueError) as error:
            result = {"id": case["id"], "passed": False, "blocked": str(error)}
        results.append(result)
        print(f"{case['id']}: {'validated' if result['passed'] else 'FAILED/BLOCKED'}", flush=True)
        if not result["passed"]:
            print(json.dumps(result, ensure_ascii=False), flush=True)
    if not results:
        parser.error("No cases selected")
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps({"mode": args.mode, "suite_sha256": evaluate.file_sha256(args.suite),
                                          "results": results}, ensure_ascii=False, indent=2), encoding="utf-8")
    return 0 if all(result["passed"] for result in results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
