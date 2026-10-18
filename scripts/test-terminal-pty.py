"""Smoke-test the real Linux terminal without starting a server or connecting to SQL."""

import errno
import fcntl
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


def smoke(binary: Path, config: Path) -> None:
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
        os.write(master, b"q")
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            child, status = os.waitpid(pid, os.WNOHANG)
            if child:
                exited = True
                assert os.waitstatus_to_exitcode(status) == 0, "Normal TUI quit failed"
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
        smoke(executable, Path(directory) / "terminal.json")
    print("Terminal navigation, preview-only SQL, clean quit, and mode restoration passed.")
