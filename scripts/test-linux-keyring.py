"""Exercise the real Secret Service in an isolated GitHub-hosted CI session."""

import hashlib
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


def check_environment():
    if sys.platform != "linux" or os.environ.get("GITHUB_ACTIONS") != "true" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise SystemExit("This integration test runs only in a disposable GitHub-hosted Linux runner.")


def session(test_binary: str):
    root = Path(os.environ["FXI_INTEGRATION_ROOT"]).resolve(strict=True)
    assert root.parent == Path("/tmp") and root.name.startswith("fxi-native-")
    command = [test_binary, "platform::secrets::tests::native_keyring_persists_across_processes_and_fails_closed", "--exact", "--ignored", "--nocapture"]
    with subprocess.Popen(
        ["gnome-keyring-daemon", "--foreground", "--unlock", "--components=secrets"],
        stdin=subprocess.PIPE,
    ) as keyring:
        try:
            keyring.stdin.write(b"public-ci-keyring-fixture")
            keyring.stdin.close()
            time.sleep(1)
            assert keyring.poll() is None, "Test keyring exited unexpectedly"
            for mode in ("write", "read"):
                subprocess.run(command, env={**os.environ, "FXI_KEYRING_MODE": mode}, check=True, timeout=30)
            namespace = hashlib.sha256(os.fsencode((root / "data/com.fxserver.installer").resolve())).hexdigest()
            subprocess.run([
                "/usr/bin/secret-tool", "clear", "application", "com.fxserver.installer",
                "purpose", "encryption-key-v1", "storage", namespace,
            ], check=True, timeout=10)
            subprocess.run(command, env={**os.environ, "FXI_KEYRING_MODE": "missing"}, check=True, timeout=30)
        finally:
            if keyring.poll() is None:
                keyring.terminate()
                try:
                    keyring.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    keyring.kill()
                    keyring.wait(timeout=5)


if __name__ == "__main__":
    check_environment()
    binary = str(Path(sys.argv[1]).resolve(strict=True))
    if "--session" in sys.argv:
        session(binary)
    else:
        with tempfile.TemporaryDirectory(prefix="fxi-native-", dir="/tmp") as directory:
            root = Path(directory)
            environment = {**os.environ, "FXI_INTEGRATION_ROOT": directory}
            for name, folder in {"HOME": "home", "XDG_DATA_HOME": "data", "XDG_CONFIG_HOME": "config", "XDG_CACHE_HOME": "cache", "XDG_RUNTIME_DIR": "runtime"}.items():
                (root / folder).mkdir(mode=0o700)
                environment[name] = str(root / folder)
            for name in ("DBUS_SESSION_BUS_ADDRESS", "GNOME_KEYRING_CONTROL"):
                environment.pop(name, None)
            subprocess.run(["dbus-run-session", "--", sys.executable, __file__, binary, "--session"], env=environment, check=True, timeout=120)
        print("Native keyring persistence, private file permissions, and missing-key fail-closed checks passed.")
