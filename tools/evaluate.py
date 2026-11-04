#!/usr/bin/env python3
"""Developer-only, local-GGUF evaluation. Generated code runs only via lyra verify.

A trusted suite pins every source file. Hidden checks remain outside model context.
No model download, paid API, dependency installation or host execution of generated
code happens here. Reports include only measured metrics; unknown resources are null.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import time


def file_sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def inside(root, name):
    path = (root / name).resolve()
    if path == root or root not in path.parents:
        raise ValueError(f"Suite path escapes root: {name}")
    return path


def load_suite(path):
    if path.stat().st_size > 1024 * 1024:
        raise ValueError("Suite exceeds 1 MiB")
    suite = json.loads(path.read_text(encoding="utf-8"))
    cases = suite.get("cases", [])
    if suite.get("schema_version") != 1 or not 1 <= len(cases) <= 256:
        raise ValueError("Expected suite v1 with 1..256 cases")
    seen = set()
    root = path.parent.resolve()
    for case in cases:
        if not case["id"] or case["id"] in seen:
            raise ValueError("Missing or duplicate case id")
        seen.add(case["id"])
        source = inside(root, case["source"])
        expected = case["sha256"]
        actual = {str(p.relative_to(source)).replace(os.sep, "/"):
                  hashlib.sha256(p.read_bytes()).hexdigest()
                  for p in source.rglob("*") if p.is_file()}
        if any(p.is_symlink() for p in source.rglob("*")) or actual != expected:
            raise ValueError(f"Fixture changed or contains links: {case['id']}")
        if case["baseline"] not in ("pass", "fail"):
            raise ValueError("Case baseline must be pass or fail")
        if not case["files"] or not set(case["files"]) <= set(expected):
            raise ValueError("Selected files must belong to pinned source")
        if not 0 < len(case["task"].encode("utf-8")) <= 16384:
            raise ValueError("Task must fit the CLI task budget")
        checker = inside(root, case["check_config"])
        if hashlib.sha256(checker.read_bytes()).hexdigest() != case["check_sha256"]:
            raise ValueError(f"Hidden checker changed: {case['id']}")
    return suite


def invoke(argv, timeout):
    started = time.monotonic()
    with tempfile.TemporaryFile() as out, tempfile.TemporaryFile() as err:
        kwargs = {"start_new_session": True} if os.name != "nt" else {
            "creationflags": subprocess.CREATE_NEW_PROCESS_GROUP}
        process = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=out, stderr=err, **kwargs)
        timed_out = False
        try:
            process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            timed_out = True
            if os.name == "nt":
                subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"],
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10, check=False)
            else:
                os.killpg(process.pid, signal.SIGKILL)
            process.wait(timeout=10)
        out.seek(0); err.seek(0)
        raw = out.read(2 * 1024 * 1024 + 1)
        stderr = err.read(65536).decode("utf-8", errors="replace")
    try:
        body = json.loads(raw) if len(raw) <= 2 * 1024 * 1024 else None
    except (ValueError, UnicodeDecodeError):
        body = None
    return {"exit_code": process.returncode, "timed_out": timed_out,
            "elapsed_ms": round((time.monotonic() - started) * 1000), "body": body,
            "diagnostic": stderr, "oversized_stdout": len(raw) > 2 * 1024 * 1024}


def run_case(args, root, case, iteration):
    started = time.monotonic()
    result = {"case": case["id"], "profile": case["profile"], "iteration": iteration,
              "passed": False, "status": "blocked", "peak_ram_bytes": None, "peak_vram_bytes": None}
    with tempfile.TemporaryDirectory(prefix="lyra-eval-") as folder:
        work = Path(folder)
        repo = work / "project"
        shutil.copytree(inside(root, case["source"]), repo)
        checker = str(inside(root, case["check_config"]))
        selected = [arg for name in case["files"] for arg in ("--file", name)]
        verify = [str(args.binary), "verify", "--repo", str(repo), "--config", checker, *selected]
        baseline = invoke(verify, 120)
        result["baseline"] = baseline
        body = baseline["body"] or {}
        if baseline["timed_out"] or not body.get("cleanup_confirmed") or body.get("exit_code") is None:
            return result
        if body.get("timed_out") or body.get("cancelled") or (case["baseline"] == "fail" and body.get("exit_code") != case.get("baseline_exit_code", 1)):
            return result
        baseline_passed = baseline["exit_code"] == 0
        if baseline_passed != (case["baseline"] == "pass"):
            result["status"] = "invalid_baseline"
            return result
        task_file = work / "TASK.txt"
        task_file.write_text(case["task"], encoding="utf-8")
        command = [str(args.binary), "task", "--model", str(args.model), "--repo", str(repo),
                   "--task-file", str(task_file), "--check-config", checker, "--context", str(args.context),
                   "--max-tokens", str(args.tokens), "--max-calls", str(args.calls),
                   "--timeout-secs", str(args.timeout), "--gpu-layers", str(args.gpu_layers), *selected]
        task = invoke(command, args.timeout + 15)
        result["task"] = task
        report = task["body"] or {}
        if task["exit_code"] != 0 or report.get("outcome") != "changes_verified":
            result["status"] = report.get("status", "task_failed")
            return result
        if not report.get("proposals") and not case.get("allow_no_changes", False):
            result["status"] = "no_changes"
            return result
        if not set(p["path"] for p in report.get("proposals", [])) <= set(case["allowed_changes"]):
            result["status"] = "unexpected_changes"
            return result
        report_file = work / "report.json"
        report_file.write_text(json.dumps(report, ensure_ascii=False), encoding="utf-8")
        for index, _ in enumerate(report.get("proposals", [])):
            applied = invoke([str(args.binary), "apply", "--repo", str(repo), "--report",
                              str(report_file), "--index", str(index)], 30)
            if applied["exit_code"] != 0:
                result["status"] = "apply_failed"
                result["apply"] = applied
                return result
        final_files = [arg for name in sorted(set(case["files"]) | {p["path"] for p in report.get("proposals", [])})
                       for arg in ("--file", name)]
        final = invoke([str(args.binary), "verify", "--repo", str(repo), "--config", checker, *final_files], 120)
        result["verification"] = final
        result["passed"] = final["exit_code"] == 0 and (final["body"] or {}).get("cleanup_confirmed", False)
        result["status"] = "passed" if result["passed"] else "verification_failed"
        result["elapsed_ms"] = round((time.monotonic() - started) * 1000)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--suite", type=Path, required=True)
    parser.add_argument("--model", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--repeats", type=int, choices=range(1, 11), default=3)
    parser.add_argument("--context", type=int, default=8192)
    parser.add_argument("--tokens", type=int, default=1536)
    parser.add_argument("--calls", type=int, default=48)
    parser.add_argument("--timeout", type=int, default=1800)
    parser.add_argument("--gpu-layers", type=int, default=0)
    args = parser.parse_args()
    args.binary = args.binary.resolve(); args.model = args.model.resolve(); args.suite = args.suite.resolve()
    if args.output.exists():
        parser.error("Output already exists; choose a new run file")
    if not (128 <= args.context <= 32768 and 1 <= args.tokens <= 8192 and 1 <= args.calls <= 256 and 1 <= args.timeout <= 3600):
        parser.error("Resource settings exceed CLI bounds")
    suite = load_suite(args.suite)
    if not args.binary.is_file() or not args.model.is_file():
        parser.error("Binary and explicit local GGUF must exist")
    report = {"schema_version": 1, "suite_sha256": hashlib.sha256(args.suite.read_bytes()).hexdigest(),
              "binary_sha256": file_sha256(args.binary),
              "model_sha256": file_sha256(args.model),
              "model": str(args.model), "configuration": {k: v for k, v in vars(args).items() if k not in ("binary", "suite", "model", "output")},
              "results": []}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    for case in suite["cases"]:
        for iteration in range(1, args.repeats + 1):
            result = run_case(args, args.suite.parent, case, iteration)
            report["results"].append(result)
            # An interruption preserves previous runs. Never overwrite a reviewed baseline.
            staging = args.output.with_name(args.output.name + ".tmp")
            staging.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
            staging.replace(args.output)
            print(f"{case['id']} #{iteration}: {result['status']}", flush=True)
    return 0 if all(r["passed"] for r in report["results"]) else 1


if __name__ == "__main__":
    raise SystemExit(main())
