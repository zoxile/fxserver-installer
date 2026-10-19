"""Opt-in Linux PTY regression using a bounded, synthetic server process.

Set FXINS_CONSOLE_PTY_BINARY to an already-built Linux fxins binary, then run
unittest discovery for this file. No build, real FXServer, RCON, DB, or service
operation is performed. The ordinary terminal smoke runners remain unchanged.
"""

import importlib.util
import os
from pathlib import Path
import re
import sys
import tempfile
import time
import unittest
from unittest.mock import Mock, patch


SCRIPTS = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("terminal_console_smoke", SCRIPTS / "terminal_smoke.py")
terminal = importlib.util.module_from_spec(spec)
spec.loader.exec_module(terminal)


def console_fixture(root):
    arguments, files = terminal.config_fixture(root)
    executable = root / "artifact/alpine/opt/cfx-server/FXServer"
    executable.parent.mkdir(parents=True)
    executable.write_text("fixture, not an FXServer executable", encoding="utf-8")
    launcher = root / "artifact/run.sh"
    launcher.write_text(
        '#!/bin/bash\n'
        'printf "%s\\n" "$$" > "$TXHOST_DATA_PATH/fixture.pid"\n'
        'phase=1\n'
        'while (( SECONDS < 60 )); do\n'
        '  if IFS= read -r -t 0.05 command; then\n'
        '    if [[ "$command" = quit ]]; then\n'
        '      printf stopped > "$TXHOST_DATA_PATH/fixture.stopped"\n'
        '      exit 0\n'
        '    fi\n'
        '    printf unexpected > "$TXHOST_DATA_PATH/fixture.unexpected-input"\n'
        '    exit 1\n'
        '  fi\n'
        '  if (( phase <= 23 )) && [[ -f "$TXHOST_DATA_PATH/fixture.emit-$phase" ]]; then\n'
        '    case "$phase" in\n'
        '      1) first=1; last=60 ;;\n'
        '      2) first=61; last=100 ;;\n'
        '      *) first=$((101 + (phase - 3) * 100)); last=$((first + 99)) ;;\n'
        '    esac\n'
        '    (( last > 2105 )) && last=2105\n'
        '    for (( i=first; i<=last; i++ )); do\n'
        '      case "$((i % 3))" in\n'
        '        1) label="fixture-keep Alpha" ;;\n'
        '        2) label="fixture-drop Alpha" ;;\n'
        '        0) label="fixture-keep Beta" ;;\n'
        '      esac\n'
        '      printf "%s row-%04d\\n" "$label" "$i"\n'
        '    done\n'
        '    printf done > "$TXHOST_DATA_PATH/fixture.done-$phase"\n'
        '    ((phase+=1))\n'
        '  fi\n'
        'done\n'
        'exit 2\n',
        encoding="utf-8",
    )
    files.update({path: path.read_bytes() for path in (executable, launcher)})
    return arguments, files


def console_history(probe, root):
    txdata = root / "txData"

    def rows(text):
        return re.findall(r"fixture-(?:keep|drop) (?:Alpha|Beta) row-\d{4}", text)

    probe.send(b"\x1bOP")
    probe.contains("S: start")
    probe.send(b"f")  # Force-start only the fixed, non-executable fixture above.
    probe.contains("Force start despite preflight errors?")
    probe.send(b"y")
    probe.expect(lambda text: "RUNNING" in text and (txdata / "fixture.pid").exists(), "synthetic launcher starts without a real executable")
    probe.send(b"\x1bOQ")
    probe.contains("Following")
    (txdata / "fixture.emit-1").touch()
    probe.contains("row-0060")

    probe.send(b"/")
    probe.send(b"ALPHA fixture-keep")
    probe.expect(
        lambda text: "| 20 matches" in text and "row-0058" in text and "fixture-drop" not in text and " Beta " not in text,
        "synthetic logs require both case-insensitive filter words",
    )
    probe.send(b"\r")
    probe.send(b"\x1b[H")
    probe.expect(lambda text: "Paused" in text and "row-0001" in text and "row-0058" not in text, "Home pauses on the oldest matching log")
    anchor = rows(probe.screen.text)
    assert len(anchor) > 1, "Expected multiple synthetic log rows in the paused viewport"
    probe.send(b"\x1bOS")  # F4: shared output scrolling must not change Console.
    probe.contains("Enter: SQL")
    probe.send(b"\x1b[6~")
    (txdata / "fixture.emit-2").touch()
    probe.expect(lambda text: "Enter: SQL" in text and (txdata / "fixture.done-2").exists(), "synthetic logs arrive while another tab is visible")
    probe.remains(lambda text: "Enter: SQL" in text, "background polling stays on the selected tab")
    probe.send(b"\x1bOQ")
    probe.expect(lambda text: "Paused" in text and "| 34 matches" in text and rows(text) == anchor, "Console restores its anchor after another tab scrolls and background logs arrive")

    probe.send(b"\r")
    probe.contains("RCON command")
    probe.send(b"fixture / command q")
    probe.contains("fixture / command q")
    probe.send(b"\x06")
    probe.send(b"\x1b[F")
    probe.remains(lambda text: "Paused" in text and "RCON command" in text and rows(text) == anchor, "command editing does not move the paused log viewport")
    probe.send(b"\x1b")
    probe.expect(lambda text: "RCON command" not in text and rows(text) == anchor, "cancelled command preserves the paused anchor")

    probe.send(b"\x06")
    probe.send(b"\x15")
    probe.send(b"absent-synthetic-row")
    probe.expect(lambda text: "| 0 matches" in text and "No matching console rows." in text and not rows(text), "nonmatching filter hides all synthetic rows")
    probe.send(b"\r")
    probe.expect(
        lambda text: "Filter / Ctrl+F (Esc: clear):" in text and "absent-synthetic-row" in text and "No matching console rows." in text and "RCON command" not in text,
        "Enter visibly closes filter editing while retaining the no-match query",
    )
    probe.send(b"\x1b")
    # A bare Esc must be decoded before the next ESC-prefixed key is written.
    probe.expect(
        lambda text: "Filter / Ctrl+F (Esc: clear):" in text and "absent-synthetic-row" not in text and "No matching console rows." not in text and bool(rows(text)),
        "Esc visibly clears the filter before sending End",
    )
    probe.send(b"\x1b[F")
    probe.expect(lambda text: "Following" in text and "row-0100" in text, "cleared filter and End reveal logs collected while paused")
    probe.send(b"\x1b[H")
    probe.expect(lambda text: "Paused" in text and "row-0001" in text, "pause at oldest row before bounded-history eviction")
    count = re.search(r"Paused \| (\d+)/2000 \|", probe.screen.text)
    assert count, f"Missing retained-row count\n{probe.screen.text}"
    baseline = int(count[1])
    deadline = time.monotonic() + 25
    for phase in range(3, 24):
        last = min(100 + (phase - 2) * 100, 2105)
        retained = min(baseline + last - 100, 2000)
        oldest = f"row-{last - 1999:04d}" if last >= 2000 else None
        (txdata / f"fixture.emit-{phase}").touch()
        # A screen acknowledgment gates every <=100-row batch. Once full, the
        # oldest visible row advances, so a constant 2000 count is not enough.
        probe.expect(
            lambda text: f"Paused | {retained}/2000 | {retained} matches" in text
            and (oldest is None or (rows(text) and rows(text)[0].endswith(oldest))),
            f"synthetic batch {phase} reaches the UI before the next batch is released",
            timeout=min(5, max(0, deadline - time.monotonic())),
        )
    probe.expect(
        lambda text: "Paused | 2000/2000 | 2000 matches" in text and "Paused rows expired; oldest retained shown." in text and "row-0106" in text and "row-0001" not in text,
        "history rollover retains 2000 rows and reports an expired paused anchor",
    )
    probe.send(b"\x1b[F")
    probe.expect(lambda text: "Following" in text and "row-2105" in text, "End follows the newest retained synthetic row")

    probe.send(b"\x1bOP")
    probe.contains("S: start")
    probe.send(b"x")
    probe.contains("Stop the managed server?")
    probe.send(b"y")
    probe.expect(
        lambda text: "STOPPED" in text and "Complete" in text
        and "Confirm Y" not in text and (txdata / "fixture.stopped").exists(),
        "Server view completes stop and the synthetic launcher acknowledges graceful quit",
    )
    assert (txdata / "fixture.stopped").read_text() == "stopped", "Fixture did not receive graceful quit"
    assert not (txdata / "fixture.unexpected-input").exists(), "A console edit was sent to the fixture"
    child = int((txdata / "fixture.pid").read_text())
    probe.expect(lambda _: not Path(f"/proc/{child}").exists(), "synthetic launcher is reaped after graceful stop", timeout=5)


class ConsoleFixtureTests(unittest.TestCase):
    def test_filter_exit_waits_for_closed_and_cleared_frames_before_end(self):
        class ReachedFollowCheck(Exception):
            pass

        probe = Mock()
        probe.screen.text = "fixture-keep Alpha row-0001\nfixture-keep Alpha row-0004"

        def stop_after_transition(predicate, description, **kwargs):
            if description == "cleared filter and End reveal logs collected while paused":
                raise ReachedFollowCheck

        probe.expect.side_effect = stop_after_transition
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "txData").mkdir()
            with self.assertRaises(ReachedFollowCheck):
                console_history(probe, root)

        calls = probe.method_calls
        query = next(index for index, (name, args, _) in enumerate(calls) if name == "send" and args == (b"absent-synthetic-row",))
        transition = calls[query + 2:]
        self.assertEqual([name for name, _, _ in transition], ["send", "expect", "send", "expect", "send", "expect"])
        self.assertEqual([args[0] for name, args, _ in transition if name == "send"], [b"\r", b"\x1b", b"\x1b[F"])
        closed, cleared = transition[1][1][0], transition[3][1][0]
        editing = "Paused | 101/2000 | 0 matches | 0 new\nFilter (Enter/Esc: done): absent-synthetic-row\nNo matching console rows."
        inactive = editing.replace("Filter (Enter/Esc: done):", "Filter / Ctrl+F (Esc: clear):")
        empty_filter = "Paused | 101/2000 | 101 matches | 0 new\nFilter / Ctrl+F (Esc: clear): \nfixture-keep Alpha row-0100"
        self.assertFalse(closed(editing), "Enter acknowledgment must not accept the pre-Enter frame")
        self.assertTrue(closed(inactive))
        self.assertFalse(cleared(inactive), "Esc acknowledgment must not accept the pre-Esc frame")
        self.assertTrue(cleared(empty_filter))

    def test_synthetic_fixture_tracks_all_immutable_files_and_uses_no_real_binary(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            _, files = console_fixture(root)
            self.assertEqual(set(files), {path for path in root.rglob("*") if path.is_file()})
            self.assertEqual(len(files), 7)
            executable = root / "artifact/alpine/opt/cfx-server/FXServer"
            self.assertEqual(executable.read_text(), "fixture, not an FXServer executable")
            self.assertFalse((root / "txData/fixture.pid").exists())
            terminal.assert_unchanged(files)

    @unittest.skipUnless(sys.platform == "linux" and os.environ.get("FXINS_CONSOLE_PTY_BINARY"), "requires Linux and explicit FXINS_CONSOLE_PTY_BINARY")
    def test_real_pty_synthetic_history_filter_pause_restore_and_eviction(self):
        spec = importlib.util.spec_from_file_location("linux_console_pty", SCRIPTS / "test-terminal-pty.py")
        runner = importlib.util.module_from_spec(spec)
        with patch.dict(sys.modules, {"terminal_smoke": terminal}):
            spec.loader.exec_module(runner)
        binary = Path(os.environ["FXINS_CONSOLE_PTY_BINARY"]).resolve(strict=True)
        with tempfile.TemporaryDirectory(prefix="fxins-console-pty-") as directory:
            root = Path(directory)
            with patch.object(runner, "config_fixture", console_fixture), patch.object(runner, "actions_and_config", lambda probe: console_history(probe, root)):
                runner.smoke(binary, root, None)


if __name__ == "__main__":
    unittest.main()
