"""Smoke-test the real Linux terminal without starting a server or connecting to SQL."""

import errno
import fcntl
import json
import os
import pty
import select
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time
from pathlib import Path


def server_signal_smoke(binary: Path, root: Path, shutdown_signal: signal.Signals) -> None:
    root.mkdir()
    artifact = root / "server"
    executable = artifact / "alpine/opt/cfx-server/FXServer"
    executable.parent.mkdir(parents=True)
    executable.write_text("fixture, not an FXServer executable", encoding="utf-8")
    (artifact / "run.sh").write_text(
        '#!/bin/bash\nprintf "%s\\n" "$$" > "$TXHOST_DATA_PATH/fixture.pid"\n'
        'if IFS= read -r -t 10 command && [ "$command" = quit ]; then\n'
        '  printf stopped > "$TXHOST_DATA_PATH/fixture.stopped"\nfi\n',
        encoding="utf-8",
    )
    txdata = root / "txData"
    profile = txdata / "default"
    profile.mkdir(parents=True)
    data = root / "data"
    (data / "resources").mkdir(parents=True)
    (data / "server.cfg").write_text("# Inert signal-test fixture\n", encoding="utf-8")
    (profile / "config.json").write_text(
        json.dumps({"version": 2, "server": {"dataPath": str(data)}}), encoding="utf-8"
    )
    command = [
        str(binary), "--config", str(root / "terminal.json"),
        "--artifact-path", str(artifact), "--tx-data-path", str(txdata), "run", "--force",
    ]
    with subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE) as process:
        try:
            deadline = time.monotonic() + 15
            while not (txdata / "fixture.pid").exists():
                if process.poll() is not None or time.monotonic() >= deadline:
                    raise AssertionError("CLI did not launch the inert server fixture")
                time.sleep(0.05)
            os.kill(process.pid, shutdown_signal)
            stdout, stderr = process.communicate(timeout=15)
            assert process.returncode == 0, f"CLI shutdown failed: {stdout!r} {stderr!r}"
            assert (txdata / "fixture.stopped").read_text() == "stopped", "Graceful quit was not sent"
            child = int((txdata / "fixture.pid").read_text())
            assert not Path(f"/proc/{child}").exists(), "CLI left its fixture process behind"
        finally:
            if process.poll() is None:
                process.kill()
                process.communicate(timeout=5)


def smoke(binary: Path, config: Path, shutdown_signal: signal.Signals | None) -> None:
    command = [str(binary), "--config", str(config)]
    preview = subprocess.run(
        [*command, "query", "SELECT 1;"], check=True, capture_output=True, timeout=10
    )
    assert b"Preview only" in preview.stdout
    ready_read, ready_write = os.pipe()
    pid, master = pty.fork()
    if pid == 0:
        os.close(ready_write)
        os.read(ready_read, 1)
        os.close(ready_read)
        fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
        os.execvpe(str(binary), [*command, "tui"], {**os.environ, "TERM": "xterm-256color"})

    os.close(ready_read)
    before = termios.tcgetattr(master)
    os.write(ready_write, b"1")
    os.close(ready_write)
    output = bytearray()
    exited = False

    def wait_for(marker: bytes) -> None:
        deadline = time.monotonic() + 15
        while marker not in output:
            if time.monotonic() >= deadline:
                raise AssertionError(f"TUI did not render {marker!r}; output: {output[-3000:]!r}")
            if not select.select([master], [], [], 0.1)[0]:
                continue
            try:
                chunk = os.read(master, 65536)
            except OSError as error:
                if error.errno == errno.EIO:
                    raise AssertionError("TUI exited before rendering the expected view") from error
                raise
            if not chunk:
                raise AssertionError("TUI closed its output unexpectedly")
            output.extend(chunk)
            if len(output) > 256 * 1024:
                raise AssertionError("Idle TUI produced excessive output")

    try:
        wait_for(b"FXServer Installer")
        assert b"\x1b[?2004h" in output, "Bracketed paste was not enabled"
        os.write(master, b"\x1b[200~q\x1b[201~")
        os.write(master, b"\t" * 6)
        wait_for(b"password")
        if shutdown_signal is None:
            os.write(master, b"q")
        else:
            os.kill(pid, shutdown_signal)
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            child, status = os.waitpid(pid, os.WNOHANG)
            if child:
                exited = True
                assert os.waitstatus_to_exitcode(status) == 0, f"TUI shutdown failed: {shutdown_signal}"
                break
            time.sleep(0.05)
        assert exited, "TUI did not quit"
        restored = termios.tcgetattr(master)
        flags = termios.ECHO | termios.ICANON | termios.ISIG
        assert restored[3] & flags == before[3] & flags, "Terminal settings were not restored"
    finally:
        if not exited:
            os.kill(pid, signal.SIGKILL)
            os.waitpid(pid, 0)
        os.close(master)


if __name__ == "__main__":
    executable = Path(sys.argv[1]).resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="fxi-terminal-smoke-") as directory:
        for shutdown in (None, signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
            smoke(executable, Path(directory) / "terminal.json", shutdown)
            if shutdown is not None:
                server_signal_smoke(executable, Path(directory) / shutdown.name, shutdown)
    print("Terminal navigation, paste safety, SQL preview, signal shutdown, and mode restoration passed.")
