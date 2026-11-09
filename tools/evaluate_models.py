#!/usr/bin/env python3
"""Compare explicitly selected local GGUF files without routing or downloads.

The same pinned suite/configuration is used for each model. Resume requires the
same binary, suite, model hashes and reported hardware. Existing failed runs are
preserved; resume completes unfinished runs rather than selecting successful reruns.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import evaluate


def load_models(path):
    if path.stat().st_size > 65536:
        raise ValueError("Model matrix exceeds 64 KiB")
    matrix = json.loads(path.read_text(encoding="utf-8"))
    models = matrix.get("models", [])
    if matrix.get("schema_version") != 1 or not 1 <= len(models) <= 8:
        raise ValueError("Expected matrix v1 with 1..8 models")
    result = []; seen = set()
    for item in models:
        name = item["id"]
        if not re.fullmatch(r"[a-z0-9][a-z0-9_-]{0,63}", name) or name in seen:
            raise ValueError("Invalid or duplicate model id")
        seen.add(name)
        model = (path.parent / item["path"]).resolve()
        if not model.is_file():
            raise ValueError(f"Missing local model: {name}")
        layers = item["gpu_layers"]
        if layers in ("auto", "balanced"):
            # Numeric constants from src/resources.rs; no hidden CPU fallback.
            layers = 1024 if layers == "auto" else 1023
        if isinstance(layers, bool) or not isinstance(layers, int) or not 0 <= layers <= 1024:
            raise ValueError("gpu_layers must be 0..1024, auto or balanced")
        digest = evaluate.file_sha256(model)
        if item.get("sha256", digest) != digest:
            raise ValueError(f"GGUF hash differs: {name}")
        result.append({"id": name, "path": str(model), "sha256": digest, "gpu_layers": layers})
    return result


def hardware_identity(doctor):
    stable = {key: doctor.get(key) for key in ("os", "architecture", "cpu", "gpus")}
    stable["total_ram_bytes"] = doctor.get("memory", {}).get("total_bytes")
    return hashlib.sha256(json.dumps(stable, sort_keys=True).encode()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--models", type=Path, required=True)
    parser.add_argument("--suite", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--context", type=int, default=8192)
    parser.add_argument("--tokens", type=int, default=2048)
    parser.add_argument("--calls", type=int, default=64)
    parser.add_argument("--timeout", type=int, default=1800)
    parser.add_argument("--threads", type=int, default=min(os.cpu_count() or 2, 8))
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--profile", action="append", default=[])
    parser.add_argument("--measure-resources", action="store_true")
    parser.add_argument("--resume", action="store_true")
    args = parser.parse_args()
    args.binary = args.binary.resolve(); args.suite = args.suite.resolve(); args.models = args.models.resolve()
    if not args.binary.is_file():
        parser.error("Explicit built CLI binary required")
    if not (128 <= args.context <= 32768 and 1 <= args.tokens < args.context and args.tokens <= 8192 and
            1 <= args.calls <= 256 and 1 <= args.timeout <= 3600 and 1 <= args.threads <= 256 and 1 <= args.repeats <= 10):
        parser.error("Invalid resource settings")
    evaluate.load_suite(args.suite)
    models = load_models(args.models)
    doctor_run = evaluate.invoke([str(args.binary), "doctor", "--json"], 20)
    doctor = doctor_run["body"]
    if doctor_run["exit_code"] != 0 or not isinstance(doctor, dict):
        parser.error("Unable to record actual hardware")
    identity = {"binary_sha256": evaluate.file_sha256(args.binary), "suite_sha256": evaluate.file_sha256(args.suite),
                "models": models, "hardware_sha256": hardware_identity(doctor),
                "configuration": {key: getattr(args, key) for key in
                                  ("context", "tokens", "calls", "timeout", "threads", "repeats", "profile", "measure_resources")}}
    manifest = args.output / "manifest.json"
    if args.output.exists():
        if not args.resume or not manifest.is_file() or manifest.stat().st_size > 1024 * 1024:
            parser.error("Choose a new output directory or resume an existing matching matrix")
        if json.loads(manifest.read_text(encoding="utf-8"))["identity"] != identity:
            parser.error("Matrix identity/hardware changed; choose a new output directory")
    else:
        args.output.mkdir(parents=True)
        manifest.write_text(json.dumps({"schema_version": 1, "identity": identity, "actual_hardware": doctor},
                                       ensure_ascii=False, indent=2), encoding="utf-8")
    failures = False; results = []
    for model in models:
        output = args.output / (model["id"] + ".json")
        argv = [sys.executable, str(Path(__file__).with_name("evaluate.py")), "--binary", str(args.binary),
                "--suite", str(args.suite), "--model", model["path"], "--output", str(output),
                "--gpu-layers", str(model["gpu_layers"])]
        for field in ("context", "tokens", "calls", "timeout", "threads", "repeats"):
            argv.extend(["--" + field, str(getattr(args, field))])
        for profile in args.profile:
            argv.extend(["--profile", profile])
        if args.measure_resources:
            argv.append("--measure-resources")
        if args.resume:
            argv.append("--resume")
        # Child inherits foreground cancellation; evaluate owns/reaps each task process group.
        code = subprocess.run(argv, check=False).returncode
        failures |= code != 0
        if output.is_file() and output.stat().st_size <= 256 * 1024 * 1024:
            report = json.loads(output.read_text(encoding="utf-8"))
            if report.get("model_sha256") != model["sha256"]:
                raise ValueError("Model changed during experiment")
            results.append({"id": model["id"], "exit_code": code, "summary": report.get("summary", {})})
        else:
            results.append({"id": model["id"], "exit_code": code, "status": "no_report"})
        (args.output / "summary.json").write_text(json.dumps(results, ensure_ascii=False, indent=2), encoding="utf-8")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
