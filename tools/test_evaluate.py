"""Developer harness regressions; no model, Docker or generated code required."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
import evaluate


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
