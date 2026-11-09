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
from eval_resources import Monitor


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
        if case.get("acceptance_level", "unit") not in ("unit", "structural"):
            raise ValueError("Unsupported acceptance level")
        if case.get("acceptance_level") == "structural" and not case.get("required_gates"):
            raise ValueError("Structural checks require explicit external gates")
        if "reference" in case:
            reference = inside(root, case["reference"])
            actual_reference = {p.relative_to(reference).as_posix(): file_sha256(p)
                                for p in reference.rglob("*") if p.is_file()}
            if any(p.is_symlink() for p in reference.rglob("*")) or actual_reference != case["reference_sha256"]:
                raise ValueError(f"Reference changed or contains links: {case['id']}")
    return suite


def stop_process(process):
    if os.name == "nt":
        try:
            stopped = subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"],
                                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10, check=False)
            if stopped.returncode and process.poll() is None:
                process.kill()
        except (OSError, subprocess.TimeoutExpired):
            if process.poll() is None:
                process.kill()
    else:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    process.wait(timeout=10)


def invoke(argv, timeout, measure_resources=False):
    started = time.monotonic()
    with tempfile.TemporaryFile() as out, tempfile.TemporaryFile() as err:
        kwargs = {"start_new_session": True} if os.name != "nt" else {
            "creationflags": subprocess.CREATE_NEW_PROCESS_GROUP}
        process = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=out, stderr=err, **kwargs)
        monitor = Monitor(process, measure_resources)
        timed_out = False
        try:
            process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            timed_out = True
            stop_process(process)
        except BaseException:
            stop_process(process)
            raise
        finally:
            metrics = monitor.finish()
        out.seek(0); err.seek(0)
        raw = out.read(2 * 1024 * 1024 + 1)
        stderr = err.read(65536).decode("utf-8", errors="replace")
    try:
        body = json.loads(raw) if len(raw) <= 2 * 1024 * 1024 else None
    except (ValueError, UnicodeDecodeError):
        body = None
    return {"exit_code": process.returncode, "timed_out": timed_out,
            "elapsed_ms": round((time.monotonic() - started) * 1000), "body": body,
            "diagnostic": stderr, "oversized_stdout": len(raw) > 2 * 1024 * 1024, **metrics}


def _run_case(args, root, case, iteration):
    started = time.monotonic()
    result = {"case": case["id"], "profile": case["profile"], "iteration": iteration,
              "kind": case.get("kind", "multifile"), "passed": False, "check_passed": False, "model_run_attempted": False, "acceptance_level": case.get("acceptance_level", "unit"),
              "required_gates_pending": case.get("required_gates", []), "status": "blocked", "peak_ram_bytes": None, "peak_vram_bytes": None}
    with tempfile.TemporaryDirectory(prefix="lyra-eval-") as folder:
        work = Path(folder)
        repo = work / "project"
        shutil.copytree(inside(root, case["source"]), repo)
        copied = {p.relative_to(repo).as_posix(): file_sha256(p) for p in repo.rglob("*") if p.is_file()}
        if copied != case["sha256"]:
            result["status"] = "fixture_changed"
            return result
        checker_path = work / "trusted-check.json"
        shutil.copyfile(inside(root, case["check_config"]), checker_path)
        if file_sha256(checker_path) != case["check_sha256"]:
            result["status"] = "checker_changed"
            return result
        checker = str(checker_path)
        selected = [arg for name in case["files"] for arg in ("--file", name)]
        check_timeout = min(3600, max(1, int(json.loads(Path(checker).read_text(encoding="utf-8"))["timeout_secs"]))) + 20
        verify = [str(args.binary), "verify", "--repo", str(repo), "--config", checker, *selected]
        baseline = invoke(verify, check_timeout)
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
                   "--timeout-secs", str(args.timeout), "--gpu-layers", str(args.gpu_layers), "--threads", str(args.threads), *selected]
        result["model_run_attempted"] = True
        task = invoke(command, args.timeout + 15, args.measure_resources)
        result["peak_ram_bytes"] = task["peak_ram_bytes"]
        result["peak_vram_bytes"] = task["peak_vram_bytes"]
        result["resource_measurement"] = task["resource_measurement"]
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
        if report.get("proposals"):
            applied = invoke([str(args.binary), "apply", "--repo", str(repo), "--report",
                              str(report_file), "--all"], 30)
            if applied["exit_code"] != 0:
                result["status"] = "apply_failed"
                result["apply"] = applied
                return result
        final_files = [arg for name in sorted(set(case["files"]) | {p["path"] for p in report.get("proposals", [])})
                       for arg in ("--file", name)]
        final = invoke([str(args.binary), "verify", "--repo", str(repo), "--config", checker, *final_files], check_timeout)
        result["verification"] = final
        result["check_passed"] = final["exit_code"] == 0 and (final["body"] or {}).get("cleanup_confirmed", False)
        result["passed"] = result["check_passed"] and not result["required_gates_pending"]
        result["status"] = "passed" if result["passed"] else "external_gates_pending" if result["check_passed"] else "verification_failed"
        result["elapsed_ms"] = round((time.monotonic() - started) * 1000)
    return result


def run_case(args, root, case, iteration):
    started = time.monotonic()
    result = _run_case(args, root, case, iteration)
    result["elapsed_ms"] = round((time.monotonic() - started) * 1000)
    return result


def summarize(results):
    groups = {}
    for result in results:
        key = result["profile"] + "/" + result.get("acceptance_level", "unit")
        group = groups.setdefault(key, {"runs": 0, "passed": 0, "checks_passed": 0, "external_gates_pending": 0, "blocked_before_model": 0, "by_kind": {}})
        group["runs"] += 1
        if not result.get("model_run_attempted", True):
            group["blocked_before_model"] += 1
            continue
        kind = result.get("kind", "multifile")
        detail = group["by_kind"].setdefault(kind, {"runs": 0, "passed": 0})
        detail["runs"] += 1
        detail["passed"] += int(result["passed"])
        group["passed"] += int(result["passed"])
        group["checks_passed"] += int(result.get("check_passed", False))
        group["external_gates_pending"] += int(result["status"] == "external_gates_pending")
    thresholds = {"small_fix": .9, "multifile": .8, "new_feature": .7}
    for group in groups.values():
        for kind, detail in group["by_kind"].items():
            detail["pass_rate"] = detail["passed"] / detail["runs"]
            detail["threshold"] = thresholds.get(kind)
            detail["threshold_met"] = detail["pass_rate"] >= thresholds[kind] if kind in thresholds else None
    return groups


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
    parser.add_argument("--threads", type=int, default=min(os.cpu_count() or 2, 8))
    parser.add_argument("--measure-resources", action="store_true")
    parser.add_argument("--profile", action="append", default=[])
    parser.add_argument("--resume", action="store_true")
    args = parser.parse_args()
    args.binary = args.binary.resolve(); args.model = args.model.resolve(); args.suite = args.suite.resolve()
    if args.output.exists() and not args.resume:
        parser.error("Output already exists; choose a new run file or use --resume")
    if not (128 <= args.context <= 32768 and 1 <= args.tokens < args.context and args.tokens <= 8192 and 1 <= args.calls <= 256 and 1 <= args.timeout <= 3600 and 1 <= args.threads <= 256 and 0 <= args.gpu_layers <= 1024):
        parser.error("Resource settings exceed CLI bounds")
    suite = load_suite(args.suite)
    if not args.binary.is_file() or not args.model.is_file():
        parser.error("Binary and explicit local GGUF must exist")
    report = {"schema_version": 1, "suite_sha256": hashlib.sha256(args.suite.read_bytes()).hexdigest(),
              "binary_sha256": file_sha256(args.binary),
              "model_sha256": file_sha256(args.model),
              "model": str(args.model), "configuration": {k: v for k, v in vars(args).items() if k not in ("binary", "suite", "model", "output", "resume")},
              "results": []}
    if args.resume and args.output.exists():
        if args.output.stat().st_size > 256 * 1024 * 1024:
            parser.error("Resume report exceeds 256 MiB")
        existing = json.loads(args.output.read_text(encoding="utf-8"))
        if any(existing.get(key) != report[key] for key in ["schema_version", "suite_sha256", "binary_sha256", "model_sha256", "configuration"]):
            parser.error("Resume requires the same binary, model, suite and configuration")
        report = existing
    done = {(r["case"], r["iteration"]) for r in report["results"]}
    if len(done) != len(report["results"]):
        parser.error("Duplicate runs in resume report")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    selected = [case for case in suite["cases"] if not args.profile or case["profile"] in args.profile]
    if not selected:
        parser.error("No cases selected")
    allowed = {(case["id"], iteration) for case in selected for iteration in range(1, args.repeats + 1)}
    if not done <= allowed:
        parser.error("Resume report contains runs outside this experiment")
    for case in selected:
        for iteration in range(1, args.repeats + 1):
            if (case["id"], iteration) in done:
                continue
            result = run_case(args, args.suite.parent, case, iteration)
            report["results"].append(result)
            report["summary"] = summarize(report["results"])
            # An interruption preserves previous runs. Never overwrite a reviewed baseline.
            staging = args.output.with_name(args.output.name + ".tmp")
            staging.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
            staging.replace(args.output)
            print(f"{case['id']} #{iteration}: {result['status']}", flush=True)
    return 0 if all(r["passed"] for r in report["results"]) else 1


if __name__ == "__main__":
    raise SystemExit(main())
