"""Developer harness regressions; no model, Docker or generated code required."""
import json
import os
import sys
import threading
import time
import _thread
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
import evaluate
import evaluate_models
from collections import Counter


class SuiteIntegrity(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name) / "suite"
        shutil.copytree(Path(__file__).resolve().parents[1] / "evals/python-multifile", self.root)

    def tearDown(self):
        self.temp.cleanup()

    def test_pinned_fixture_and_checker_load(self):
        suite = evaluate.load_suite(self.root / "suite.json")
        self.assertEqual(suite["cases"][0]["profile"], "python")
        (self.root / "source/pricing.py").write_text("changed source", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "Fixture changed"):
            evaluate.load_suite(self.root / "suite.json")

    def test_private_checker_cannot_be_changed_silently(self):
        (self.root / "check.json").write_text("{}", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "checker changed"):
            evaluate.load_suite(self.root / "suite.json")

    def test_git_windows_checkout_preserves_pinned_fixture_bytes(self):
        repository = Path(__file__).resolve().parents[1]
        export = Path(self.temp.name) / "windows-checkout"
        export.mkdir()
        paths = subprocess.check_output(["git", "-C", str(repository), "ls-files", "-z", "evals/python-multifile"], timeout=15).decode("utf-8").strip("\0").split("\0")
        subprocess.run(["git", "-C", str(repository), "-c", "core.autocrlf=true", "checkout-index",
                        f"--prefix={export.as_posix()}/", "--", *paths], check=True, capture_output=True, timeout=15)
        evaluate.load_suite(export / "evals/python-multifile/suite.json")

    def test_project_corpus_pins_references_and_separates_sdk_gates(self):
        repository = Path(__file__).resolve().parents[1]
        suite = evaluate.load_suite(repository / "evals/projects/suite.json")
        self.assertEqual(len(suite["cases"]), 36)
        self.assertEqual(set(Counter(case["profile"] for case in suite["cases"]).values()), {6})
        self.assertEqual(Counter(case["kind"] for case in suite["cases"]),
                         {"small_fix": 12, "multifile": 12, "new_feature": 12})
        structural = [case for case in suite["cases"] if case["acceptance_level"] == "structural"]
        self.assertEqual(len(structural), 6)
        self.assertTrue(all(case["required_gates"] for case in structural))

    def test_summary_does_not_promote_structural_checks_to_full_acceptance(self):
        results = [{"profile": "dayz", "acceptance_level": "structural", "passed": False,
                    "check_passed": True, "status": "external_gates_pending"}]
        summary = evaluate.summarize(results)["dayz/structural"]
        self.assertEqual(summary["passed"], 0)
        self.assertEqual(summary["checks_passed"], 1)
        self.assertEqual(summary["external_gates_pending"], 1)

    def test_missing_executor_does_not_become_a_model_quality_score(self):
        summary = evaluate.summarize([{"profile": "rust", "passed": False, "status": "blocked",
                                       "model_run_attempted": False}])["rust/unit"]
        self.assertEqual(summary["blocked_before_model"], 1)
        self.assertEqual(summary["by_kind"], {})

    def test_invoke_measures_the_child_process_and_timeout_reaps_it(self):
        run = evaluate.invoke([sys.executable, "-c", "import time; data=bytearray(32*1024*1024);time.sleep(.5);print('{}')"], 10, True)
        self.assertEqual(run["exit_code"], 0)
        if os.name == "nt" or Path("/proc/self/status").exists():
            self.assertIsNotNone(run["peak_ram_bytes"])
            self.assertGreaterEqual(run["peak_ram_bytes"], 32*1024*1024)
        self.assertEqual(run["resource_measurement"]["scope"], "sampled_cli_process_only")
        timed = evaluate.invoke([sys.executable, "-c", "import time;time.sleep(30)"], .1)
        self.assertTrue(timed["timed_out"])
        self.assertIsNotNone(timed["exit_code"])

    @unittest.skipIf(os.name == "nt", "POSIX process existence assertion; taskkill is exercised by timeout test")
    def test_keyboard_interrupt_does_not_leave_the_cli_process_running(self):
        marker = Path(self.temp.name)/"pid"
        code = "import os,time;from pathlib import Path;Path("+repr(str(marker))+").write_text(str(os.getpid()));time.sleep(30)"
        def interrupt_after_start():
            until = time.monotonic()+5
            while not marker.exists() and time.monotonic()<until:
                time.sleep(.01)
            _thread.interrupt_main()
        thread = threading.Thread(target=interrupt_after_start)
        thread.start()
        with self.assertRaises(KeyboardInterrupt):
            evaluate.invoke([sys.executable, "-c", code], 10)
        thread.join(timeout=1)
        pid = int(marker.read_text())
        with self.assertRaises(ProcessLookupError):
            os.kill(pid, 0)

    def test_resume_preserves_completed_runs_and_refuses_another_configuration(self):
        repository = Path(__file__).resolve().parents[1]
        weights = Path(self.temp.name)/"weights.gguf"; weights.write_bytes(b"GGUF fixture never loaded")
        output = Path(self.temp.name)/"run.json"
        command = [sys.executable, str(repository/"tools/evaluate.py"), "--binary", sys.executable,
                   "--suite", str(self.root/"suite.json"), "--model", str(weights), "--output", str(output), "--repeats", "1"]
        # Python as fake CLI refuses verify, so this checks orchestration without running a model.
        first = subprocess.run(command, capture_output=True, timeout=15)
        self.assertEqual(first.returncode, 1, first.stderr)
        report = output.read_bytes()
        resumed = subprocess.run([*command, "--resume"], capture_output=True, timeout=15)
        self.assertEqual(resumed.returncode, 1, resumed.stderr)
        self.assertEqual(output.read_bytes(), report)
        previous = json.loads(report)["configuration"]["threads"]
        refused = subprocess.run([*command, "--resume", "--threads", str(2 if previous==1 else 1)], capture_output=True, timeout=15)
        self.assertEqual(refused.returncode, 2)
        self.assertEqual(output.read_bytes(), report)

    def test_local_model_matrix_pins_files_and_rejects_duplicate_ids_and_bad_hashes(self):
        model=Path(self.temp.name)/"model.gguf";model.write_bytes(b"GGUF metadata fixture")
        path=Path(self.temp.name)/"models.json"
        entry={"id":"local-one","path":str(model),"gpu_layers":"auto"}
        path.write_text(json.dumps({"schema_version":1,"models":[entry]}))
        loaded=evaluate_models.load_models(path)
        self.assertEqual(loaded[0]["gpu_layers"],1024)
        self.assertEqual(loaded[0]["sha256"],evaluate.file_sha256(model))
        path.write_text(json.dumps({"schema_version":1,"models":[entry,entry]}))
        with self.assertRaisesRegex(ValueError,"duplicate"):
            evaluate_models.load_models(path)
        path.write_text(json.dumps({"schema_version":1,"models":[dict(entry,sha256="wrong")]}))
        with self.assertRaisesRegex(ValueError,"hash differs"):
            evaluate_models.load_models(path)

    def test_hardware_identity_ignores_free_memory_but_keeps_device_identity(self):
        first={"os":"Windows","architecture":"x86_64","cpu":{"brand":"CPU"},"memory":{"total_bytes":32,"available_bytes":16},"gpus":[{"model":"GPU"}]}
        second=json.loads(json.dumps(first));second["memory"]["available_bytes"]=4
        self.assertEqual(evaluate_models.hardware_identity(first),evaluate_models.hardware_identity(second))
        second["gpus"][0]["model"]="other"
        self.assertNotEqual(evaluate_models.hardware_identity(first),evaluate_models.hardware_identity(second))

    def test_empty_suite_and_escape_are_rejected(self):
        path = self.root / "suite.json"
        suite = json.loads(path.read_text())
        suite["cases"][0]["source"] = "../../outside"
        path.write_text(json.dumps(suite))
        with self.assertRaisesRegex(ValueError, "escapes root"):
            evaluate.load_suite(path)
        path.write_text('{"schema_version":1,"cases":[]}')
        with self.assertRaisesRegex(ValueError, "1..256"):
            evaluate.load_suite(path)


if __name__ == "__main__":
    unittest.main()
