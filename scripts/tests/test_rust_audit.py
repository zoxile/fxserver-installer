import importlib.util
import io
from pathlib import Path
import subprocess
import unittest
from contextlib import redirect_stderr, redirect_stdout
from unittest.mock import patch


spec = importlib.util.spec_from_file_location("rust_audit", Path(__file__).parents[1] / "audit-rust.py")
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)

NETWORK_ERROR = (
    "error: couldn't fetch advisory database: git operation failed\n"
    "Caused by: error sending request for url (https://github.com/RustSec/advisory-db.git)\n"
)


def result(code, stderr="", stdout=""):
    return subprocess.CompletedProcess([], code, stdout, stderr)


class RustAuditTests(unittest.TestCase):
    def run_audit(self, results):
        with patch.object(audit.subprocess, "run", side_effect=results) as run, \
                patch.object(audit.time, "sleep") as sleep, \
                redirect_stdout(io.StringIO()) as stdout, redirect_stderr(io.StringIO()) as stderr:
            code = audit.audit()
        for call in run.call_args_list:
            self.assertEqual(call.args[0], ["cargo", "audit", "--color", "never", "--file", "src-tauri/Cargo.lock"])
            self.assertEqual(call.kwargs["timeout"], 180)
        return code, run.call_count, sleep.call_args_list, stdout.getvalue() + stderr.getvalue()

    def test_transport_retry_requires_a_subsequent_successful_audit(self):
        code, calls, delays, output = self.run_audit([result(1, NETWORK_ERROR), result(0, stdout="audit passed")])
        self.assertEqual((code, calls), (0, 2))
        self.assertEqual([call.args[0] for call in delays], [2])
        self.assertIn(NETWORK_ERROR, output)
        self.assertIn("audit passed", output)

    def test_repeated_transport_failures_remain_failures(self):
        code, calls, delays, _ = self.run_audit([result(1, NETWORK_ERROR)] * 3)
        self.assertEqual((code, calls), (1, 3))
        self.assertEqual([call.args[0] for call in delays], [2, 4])

    def test_findings_and_other_errors_never_retry(self):
        for error in (
            "error: 1 vulnerability found! RUSTSEC-fixture",
            "error: couldn't fetch advisory database: permission denied",
            "error: error sending request for url while checking the registry",
        ):
            with self.subTest(error=error):
                code, calls, delays, output = self.run_audit([result(1, error)])
                self.assertEqual((code, calls, delays), (1, 1, []))
                self.assertIn(error, output)

    def test_finding_after_transport_failure_still_fails(self):
        code, calls, _, _ = self.run_audit([result(1, NETWORK_ERROR), result(1, "vulnerability found")])
        self.assertEqual((code, calls), (1, 2))

    def test_missing_tool_and_timeout_fail_without_retry_or_offline_fallback(self):
        for error in (OSError("tool missing"), subprocess.TimeoutExpired("cargo audit", 180)):
            with self.subTest(error=error):
                code, calls, delays, output = self.run_audit([error])
                self.assertEqual((code, calls, delays), (1, 1, []))
                self.assertIn("could not complete", output)


if __name__ == "__main__":
    unittest.main()
