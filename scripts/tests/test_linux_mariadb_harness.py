"""Safety guard checks. No database, package, or service commands are executed."""
import importlib.util
import os
from pathlib import Path
import sys
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location(
    "mariadb_harness", Path(__file__).resolve().parents[1] / "test-linux-mariadb.py"
)
harness = importlib.util.module_from_spec(spec)
spec.loader.exec_module(harness)


class GuardTests(unittest.TestCase):
    def test_non_linux_rejected_before_any_filesystem_or_process_probe(self):
        with patch.object(sys, "platform", "win32"), patch.object(harness, "any_database_process") as probe:
            with self.assertRaisesRegex(RuntimeError, "never runs on Windows"):
                harness.check_context()
            probe.assert_not_called()

    def test_missing_ci_guard_rejected(self):
        with patch.object(sys, "platform", "linux"), patch.dict(os.environ, {}, clear=True):
            with self.assertRaisesRegex(RuntimeError, "GitHub Actions"):
                harness.check_context()

    def test_self_hosted_runner_rejected(self):
        with patch.object(sys, "platform", "linux"), patch.dict(os.environ, {
            "GITHUB_ACTIONS": "true", "RUNNER_ENVIRONMENT": "self-hosted"
        }, clear=True):
            with self.assertRaisesRegex(RuntimeError, "GitHub-hosted"):
                harness.check_context()

    def test_explicit_disposable_acknowledgement_required(self):
        with patch.object(sys, "platform", "linux"), patch.dict(os.environ, {
            "GITHUB_ACTIONS": "true", "RUNNER_ENVIRONMENT": "github-hosted"
        }, clear=True):
            with self.assertRaisesRegex(RuntimeError, "acknowledgement"):
                harness.check_context()

    def test_run_id_required_before_container_probes(self):
        with patch.object(sys, "platform", "linux"), patch.dict(os.environ, {
            "GITHUB_ACTIONS": "true", "RUNNER_ENVIRONMENT": "github-hosted",
            "FXI_DISPOSABLE_MARIADB": harness.ACK, "GITHUB_RUN_ID": "../../outside"
        }, clear=True), patch.object(harness, "any_database_process") as probe:
            with self.assertRaisesRegex(RuntimeError, "run ID"):
                harness.check_context()
            probe.assert_not_called()

    @unittest.skipUnless(sys.platform == "linux", "Linux mount parsing")
    def test_bind_mount_and_persistent_volume_rejected(self):
        sources = [
            "1 0 0:1 / / rw - overlay overlay rw\n2 1 0:2 /host/data /var/lib/mysql rw - ext4 /dev/sda1 rw\n",
            "1 0 0:1 / / rw - overlay overlay rw\n2 1 0:2 /var/lib/docker/volumes/db /var/lib/mysql rw - ext4 /dev/sda1 rw\n",
        ]
        for source in sources:
            with patch.object(Path, "read_text", return_value=source), patch.object(Path, "resolve", lambda p: p):
                with self.assertRaisesRegex(RuntimeError, "container-owned"):
                    harness.require_private_mount(Path("/var/lib/mysql"))

    @unittest.skipUnless(sys.platform == "linux", "Linux mount parsing")
    def test_container_overlay_and_tmpfs_allowed(self):
        source = "1 0 0:1 / / rw - overlay overlay rw\n2 1 0:2 / /tmp rw - tmpfs tmpfs rw\n"
        with patch.object(Path, "read_text", return_value=source), patch.object(Path, "resolve", lambda p: p):
            harness.require_private_mount(Path("/var/lib/mysql"))
            harness.require_private_mount(Path("/tmp"))

    @unittest.skipUnless(sys.platform == "linux", "Linux process groups")
    def test_command_deadline_is_bounded(self):
        with self.assertRaisesRegex(RuntimeError, "deadline"):
            harness.run([sys.executable, "-c", "import time; time.sleep(30)"], env=os.environ, timeout=0.1)

    @unittest.skipUnless(sys.platform == "linux", "Linux pipe polling")
    def test_output_limit_is_bounded(self):
        with self.assertRaisesRegex(RuntimeError, "output limit"):
            harness.run([sys.executable, "-c", "import sys; sys.stdout.write('x' * (5 * 1024 * 1024))"], env=os.environ)


if __name__ == "__main__":
    unittest.main()
