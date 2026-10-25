"""Bounded VT screen checks and inert scenarios shared by PTY/ConPTY tests.

This is a test screen, not a general terminal emulator. It implements the cursor,
erase, and text operations emitted by crossterm and ConPTY; unsupported screen
mutations fail explicitly instead of silently making assertions pass.
"""

import codecs
import json
import os
import time
import unicodedata
from pathlib import Path


class Screen:
    def __init__(self, columns=80, rows=24):
        self.columns, self.rows = columns, rows
        self.cells = [[" "] * columns for _ in range(rows)]
        self.x = self.y = 0
        self.saved = (0, 0)
        self.pending = ""
        self.decoder = codecs.getincrementaldecoder("utf-8")("strict")
        self.bracketed_paste = False

    @property
    def text(self):
        return "\n".join("".join(row).rstrip() for row in self.cells)

    def feed(self, data):
        self.pending += self.decoder.decode(data)
        while self.pending:
            text = self.pending
            if text[0] == "\x1b":
                if len(text) < 2:
                    break
                if text[1] == "[":
                    end = next((i for i in range(2, len(text)) if "@" <= text[i] <= "~"), None)
                    if end is None:
                        break
                    self.csi(text[2:end], text[end])
                    self.pending = text[end + 1:]
                elif text[1] == "]":
                    ends = [(text.find(end, 2), len(end)) for end in ("\x07", "\x1b\\")]
                    ends = [item for item in ends if item[0] >= 0]
                    if not ends:
                        break
                    end, length = min(ends)
                    self.pending = text[end + length:]
                elif text[1] in "()":
                    if len(text) < 3:
                        break
                    self.pending = text[3:]
                else:
                    code = text[1]
                    if code == "7":
                        self.saved = (self.x, self.y)
                    elif code == "8":
                        self.x, self.y = self.saved
                    elif code in "=>":
                        pass
                    else:
                        raise AssertionError(f"Unsupported VT escape: {text[:20]!r}")
                    self.pending = text[2:]
            else:
                self.put(text[0])
                self.pending = text[1:]
        assert len(self.pending) <= 4096, "Unterminated/oversized terminal control sequence"

    def put(self, char):
        if char == "\r":
            self.x = 0
        elif char == "\n":
            self.y += 1
            if self.y == self.rows:
                self.cells.pop(0)
                self.cells.append([" "] * self.columns)
                self.y -= 1
        elif char == "\b":
            self.x = max(0, self.x - 1)
        elif char == "\t":
            self.x = min(self.columns - 1, (self.x // 8 + 1) * 8)
        elif char < " " or char == "\x7f":
            pass
        elif unicodedata.combining(char):
            if self.x:
                self.cells[self.y][min(self.x, self.columns) - 1] += char
        else:
            width = 2 if unicodedata.east_asian_width(char) in "WF" else 1
            if self.x >= self.columns:
                self.x = 0
                self.put("\n")
            self.cells[self.y][self.x] = char
            if width == 2 and self.x + 1 < self.columns:
                self.cells[self.y][self.x + 1] = ""
            self.x = min(self.columns, self.x + width)

    def csi(self, raw, final):
        if final in "mhl" or (final == "q" and raw.endswith(" ")):
            if raw == "?2004":
                self.bracketed_paste = final == "h"
            if raw == "?1049" and final == "h":
                self.cells = [[" "] * self.columns for _ in range(self.rows)]
                self.x = self.y = 0
            return
        values = [int(value or 0) for value in raw.split(";")]
        first = values[0] or 1
        if final in "Hf":
            self.y = min(self.rows - 1, first - 1)
            self.x = min(self.columns - 1, (values[1] if len(values) > 1 and values[1] else 1) - 1)
        elif final in "ABCD":
            self.x = min(self.columns - 1, max(0, self.x + (first if final == "C" else -first if final == "D" else 0)))
            self.y = min(self.rows - 1, max(0, self.y + (first if final == "B" else -first if final == "A" else 0)))
        elif final in "EF":
            self.x = 0
            self.y = min(self.rows - 1, max(0, self.y + (first if final == "E" else -first)))
        elif final == "G":
            self.x = min(self.columns - 1, first - 1)
        elif final == "d":
            self.y = min(self.rows - 1, first - 1)
        elif final in "JK":
            mode = values[0]
            assert mode in (0, 1, 2, 3), f"Unknown erase mode: {raw}{final}"
            if mode == 3:
                return  # Scrollback is not part of this bounded screen.
            for y in range(self.rows):
                for x in range(self.columns):
                    position = (y, x) if final == "J" else (0, x)
                    cursor = (self.y, self.x) if final == "J" else (0, self.x)
                    if (final == "J" or y == self.y) and (mode == 2 or (mode == 0 and position >= cursor) or (mode == 1 and position <= cursor)):
                        self.cells[y][x] = " "
        elif final == "X":
            for x in range(self.x, min(self.columns, self.x + first)):
                self.cells[self.y][x] = " "
        elif final == "s":
            self.saved = (self.x, self.y)
        elif final == "u":
            self.x, self.y = self.saved
        else:
            raise AssertionError(f"Unsupported VT screen operation: {raw}{final}")


class Probe:
    def __init__(self, send, read, columns=80, rows=24):
        self.send, self.read = send, read
        self.screen = Screen(columns, rows)
        self.total = 0
        self.tail = bytearray()

    def _read(self, timeout, description):
        chunk = self.read(timeout)
        if chunk is None:
            return
        assert chunk, f"Terminal closed before {description}\n{self.screen.text}"
        self.total += len(chunk)
        assert self.total <= 2 * 1024 * 1024, "TUI exceeded the 2 MiB scenario output limit"
        self.tail.extend(chunk)
        del self.tail[:-4096]
        self.screen.feed(chunk)

    def expect(self, predicate, description, timeout=15):
        deadline = time.monotonic() + timeout
        while not predicate(self.screen.text):
            if time.monotonic() >= deadline:
                raise AssertionError(f"Timed out: {description}\nScreen:\n{self.screen.text}\nRecent VT: {bytes(self.tail)!r}")
            self._read(min(0.1, max(0, deadline - time.monotonic())), description)

    def remains(self, predicate, description, duration=0.3):
        # A negative assertion must consume queued output, not trust an old frame.
        deadline = time.monotonic() + duration
        while True:
            assert predicate(self.screen.text), f"Changed during {description}\nScreen:\n{self.screen.text}\nRecent VT: {bytes(self.tail)!r}"
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                return
            self._read(min(0.1, remaining), description)

    def contains(self, marker):
        self.expect(lambda text: marker in text, repr(marker))

    def selected(self, title):
        self.expect(lambda text: any(line.strip(" \u2502").startswith("> ") and title in line for line in text.splitlines()), f"selected action {title!r}")


def isolated_environment(root):
    environment = {**os.environ, "TERM": "xterm-256color"}
    for key in ("HOME", "USERPROFILE", "LOCALAPPDATA", "APPDATA", "XDG_DATA_HOME", "XDG_CONFIG_HOME"):
        environment[key] = str(root / "private-home")
    for key in ("DISPLAY", "WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS", "GNOME_KEYRING_CONTROL"):
        environment.pop(key, None)
    return environment


def config_fixture(root):
    artifact = root / "artifact"
    artifact.mkdir(parents=True)
    txdata = root / "txData"
    profile = txdata / "default"
    profile.mkdir(parents=True)
    data = root / "server-data"
    (data / "resources").mkdir(parents=True)
    cfg = data / "server.cfg"
    cfg.write_bytes(
        b'# inert-pty-fixture\nset fixture_marker "original"\n'
        b'# literal[.] first\n# literal. decoy\n# literal[.] last\n'
    )
    (profile / "config.json").write_text(json.dumps({"version": 2, "server": {"dataPath": str(data)}}), encoding="utf-8")
    tracked = [cfg, profile / "config.json"]
    for name, version in (("fixture_alpha", "1.0.0"), ("fixture_beta", "2.0.0"), ("fixture_gamma", "1.0.0")):
        resource = data / "resources" / name
        resource.mkdir()
        manifest = resource / "fxmanifest.lua"
        manifest.write_bytes(f"fx_version 'cerulean'\ngame 'gta5'\nversion '{version}'\n".encode())
        tracked.append(manifest)
    arguments = ["--config", str(root / "terminal.json"), "--artifact-path", str(artifact), "--tx-data-path", str(txdata), "--profile", "default"]
    return arguments, {path: path.read_bytes() for path in tracked}


def result_search(probe):
    # Separated matches make navigation observable without depending on SGR colors.
    first, last = "needle[.] first", "needle[.] last"
    probe.send(b"\x06")  # Ctrl+F
    probe.contains("Find (exact) | Enter: find")
    probe.send(b"needle[.]")
    probe.send(b"\r")
    probe.expect(lambda text: "Find (exact) | Enter/Shift+Enter" in text and first in text and last not in text, "first literal result match")

    # Result arrows scroll; next/previous match are Enter, Ctrl+N, and Ctrl+P.
    probe.send(b"\x1b[B")
    probe.expect(lambda text: "Find (exact) | Enter/Shift+Enter" in text and first not in text and last not in text, "Down scrolls past the first result match")
    probe.send(b"\x1b[A")
    probe.contains(first)
    for key, visible, hidden in ((b"\r", last, first), (b"\r", first, last), (b"\x10", last, first), (b"\x0e", first, last)):
        probe.send(key)
        probe.expect(lambda text: visible in text and hidden not in text, f"result search reveals {visible!r}")

    probe.send(b"\x06")
    probe.contains("Find (exact) | Enter: find")
    probe.send(b"\x15")  # Ctrl+U clears only the search field.
    probe.send(b"absent-result-match")
    probe.send(b"\r")
    probe.contains("Find (exact): no matches")
    probe.send(b"\x06")
    probe.contains("Find (exact) | Enter: find")
    probe.send(b"\x1b")
    probe.expect(lambda text: "Result |" in text and "Find (exact): no matches" in text, "Esc closes result search without leaving the result")


def config_navigation(probe):
    def clean(text):
        return "inert-pty-fixture" in text and "server.cfg" in text and "server.cfg *" not in text

    probe.send(b"\x06")
    probe.expect(lambda text: clean(text) and "Find:" in text, "clean cfg find prompt")
    probe.send(b"literal[.]")
    for key, match in ((b"\r", "Match 3:3"), (b"\r", "Match 5:3"), (b"\x1b[A", "Match 3:3"), (b"\x1b[A", "Match 5:3 (wrapped)"), (b"\x1b[B", "Match 3:3 (wrapped)")):
        probe.send(key)
        probe.expect(lambda text: clean(text) and "Find: literal[.]" in text and match in text, f"clean cfg literal find: {match}")
    probe.send(b"\x1b")
    probe.expect(lambda text: clean(text) and "Find:" not in text, "Esc closes find without closing or dirtying the cfg")

    for line in (5, 1):
        probe.send(b"\x07")  # Ctrl+G; return to line 1 before the edit/undo scenario.
        probe.expect(lambda text: clean(text) and "Line:" in text, "clean cfg go-to-line prompt")
        probe.send(str(line).encode())
        probe.send(b"\r")
        probe.expect(lambda text: clean(text) and f"Line {line}." in text and "Line:" not in text, f"go to line {line} without dirtying the cfg")


def resource_filter(probe):
    probe.send(b"\x1bOR")  # F3 (SS3), recognized by crossterm and ConPTY.
    probe.contains("No resources loaded.")
    probe.send(b"r")  # Read-only manifest scan, not a resource/server command.
    probe.expect(lambda text: "Filter 3/3" in text and all(name in text for name in ("fixture_alpha", "fixture_beta", "fixture_gamma")), "three inert resources scanned")
    probe.selected("fixture_alpha")
    probe.send(b"/")
    probe.send(b"FIXTURE 2.0.0")
    probe.expect(lambda text: "Filter 1/3" in text and "FIXTURE 2.0.0" in text and "fixture_beta" in text and "fixture_alpha" not in text and "fixture_gamma" not in text, "case-insensitive name/version resource filter")
    probe.selected("fixture_beta")
    probe.send(b"\r")  # Finish filtering, retaining the query and selection.
    probe.send(b"\x1b")  # Clear only after Enter has left the filter prompt.
    probe.expect(lambda text: "Filter 3/3" in text and "FIXTURE" not in text and "fixture_gamma" in text, "Enter finishes resource filtering and Esc clears it")
    probe.selected("fixture_alpha")


def console_controls(probe):
    def state(text, mode, query=""):
        status = "Paused | 0/2000 | 0 matches | 0 new" if mode == "Paused" else "Following | 0/2000 rows | 0 matches"
        return status in text and "Filter" in text and query in text and "No console output yet." in text

    def expect_state(mode, description, query=""):
        probe.expect(lambda text: state(text, mode, query), description)

    probe.send(b"\x1bOQ")  # F2: the isolated profile has never started a server.
    expect_state("Following", "empty console follows without requiring log entries")
    probe.send(b" ")
    expect_state("Paused", "Space pauses an empty console")
    probe.send(b"\x1b[F")
    expect_state("Following", "End resumes an empty console")

    query = "MiXeD multi word q/s"
    probe.send(b"/")
    probe.contains("Filter (Enter/Esc: done):")
    probe.send(query.encode())
    expect_state("Following", "slash opens a filter and spaces stay in the query", query)
    assert "RCON command" not in probe.screen.text
    probe.send(b"\r")
    probe.expect(lambda text: state(text, "Following", query) and "Filter / Ctrl+F (Esc: clear):" in text, "Enter finishes editing the console filter")
    probe.remains(lambda text: state(text, "Following", query) and "RCON command" not in text, "Enter closes filter editing without opening a command")
    probe.send(b" ")
    expect_state("Paused", "Space pauses after Enter leaves the filter", query)

    for away, marker, back in ((b"\x1bOP", "S: start", b"\x1bOQ"), (b"\t", "No resources loaded.", b"\x1b[Z")):
        probe.send(away)
        probe.contains(marker)
        probe.send(back)
        expect_state("Paused", "console filter and pause survive tab navigation", query)

    probe.send(b"\r")
    probe.contains("RCON command")
    command = "fixture / two words q"
    probe.send(command.encode())
    probe.expect(lambda text: state(text, "Paused", query) and "RCON command" in text and command in text, "RCON input owns slash, spaces, and q")
    probe.send(b"\x06")
    probe.send(b"\x1b[F")
    probe.remains(lambda text: state(text, "Paused", query) and "RCON command" in text and command in text, "Ctrl+F and End in RCON do not filter or resume logs")
    probe.send(b"\x1b")  # Never submit a nonempty command, even to a fake endpoint.
    probe.expect(lambda text: state(text, "Paused", query) and "RCON command" not in text and command not in text, "Esc cancels RCON without clearing the console filter")
    probe.send(b"\r")
    probe.contains("RCON command")
    assert command not in probe.screen.text, "Cancelled RCON command survived reopening"
    probe.send(b"\r")  # An empty command must only close the input.
    probe.expect(lambda text: state(text, "Paused", query) and "RCON command" not in text, "empty RCON input closes without submitting a job")
    probe.remains(lambda text: state(text, "Paused", query) and "[RCON]" not in text and "[RCON error]" not in text, "empty command does not produce an RCON response")

    probe.send(b"\x06")
    probe.contains("Filter (Enter/Esc: done):")
    probe.send(b"\x15")
    replacement = "retained on escape"
    probe.send(b"\x1b[200~" + replacement.encode() + b"\x1b[201~")
    probe.expect(lambda text: state(text, "Paused", replacement) and query not in text and "RCON command" not in text, "Ctrl+F and Ctrl+U route a pasted replacement into the console filter")
    probe.send(b"\x1b")
    probe.expect(lambda text: state(text, "Paused", replacement) and "Filter / Ctrl+F (Esc: clear):" in text, "Esc finishes editing the console filter")
    probe.remains(lambda text: state(text, "Paused", replacement), "Esc leaves filter editing while retaining the query")
    probe.send(b" ")
    expect_state("Following", "Space resumes after Esc leaves the filter", replacement)
    probe.send(b"\x1b")
    probe.expect(lambda text: state(text, "Following") and replacement not in text, "Esc outside editing clears only the filter")

    probe.send(b"/")
    probe.contains("Filter (Enter/Esc: done):")
    probe.send(b"x" * 250 + b"CAP256")
    expect_state("Following", "256-byte filter remains editable", "CAP256")
    probe.send(b"OVERFLOW")
    probe.remains(lambda text: state(text, "Following", "CAP256") and "OVERFLOW" not in text, "typed console filter is bounded at 256 bytes")
    probe.send(b"\x15")
    probe.send(b"cleared")
    probe.expect(lambda text: state(text, "Following", "cleared") and "CAP256" not in text, "Ctrl+U still works at the filter byte limit")
    probe.send(b"\r")
    probe.send(b"\x1b")
    probe.expect(lambda text: state(text, "Following") and "cleared" not in text, "console scenario leaves no active filter or command")


def confirmation_batch_guard(probe):
    title, message = "Confirm Y | Cancel N/Esc", "Stop the managed server?"
    # One write must not approve an unseen confirmation. This disposable profile
    # has no managed server; never use start/install/SQL as the batching probe.
    probe.send(b"xy")
    probe.expect(lambda text: title in text and message in text, "batched x/y leaves the stop confirmation open")
    settled = time.monotonic() + 0.3
    probe.expect(
        lambda text: time.monotonic() >= settled and title in text and message in text,
        "stop confirmation remains open after queued input drains",
        timeout=2,
    )
    probe.send(b"N")
    probe.expect(lambda text: "S: start" in text and title not in text and message not in text, "N cancels the stop confirmation")


def actions_and_config(probe, paste_undo_limit=1):
    console_controls(probe)
    probe.send(b"\x1b[19~")  # F8, then wrap from the first item to the last.
    probe.contains("Search actions")
    first, last = "MariaDB installation and service status", "Remove app firewall rules"
    probe.selected(first)
    probe.send(b"\x1b[A")
    probe.selected(last)
    assert first not in probe.screen.text, "Long Actions list did not scroll to selection"
    probe.send(b"\x1b[B")
    probe.selected(first)
    assert last not in probe.screen.text, "Actions list did not scroll back after wrapping"

    probe.send(b"\x0b")  # Ctrl+K resets/opens search.
    probe.contains("Search actions")
    probe.send(b"JSON")
    probe.selected("Format JSON text")
    probe.send(b"\x1b[A")
    probe.selected("Validate or format JSON file")
    probe.send(b"\x1b[B")
    probe.selected("Format JSON text")
    probe.send(b"\r")
    probe.contains("JSON text")
    probe.send(b"\r")
    probe.contains("JSON text is required.")
    value = json.dumps({"fixture_pty": 314159, "items": ["needle[.] first", *range(32), "needle[.] last"]}, separators=(",", ":")).encode()
    probe.send(value)
    probe.contains("*" * 16)
    assert "fixture_pty" not in probe.screen.text and "needle[.]" not in probe.screen.text, "Secret-marked JSON field leaked before submission"
    probe.send(b"\r")
    probe.contains("Result |")
    probe.contains('"fixture_pty": 314159')
    result_search(probe)

    probe.send(b"\x1b[20~")  # F9, load only the disposable profile's cfg files.
    probe.contains("No config files loaded.")
    probe.send(b"r")
    probe.contains("Configuration files loaded.")
    probe.contains("server.cfg")
    probe.send(b"\r")
    probe.contains("inert-pty-fixture")
    config_navigation(probe)
    probe.send(b"\x1b[F")  # End of first line; type and undo one edit.
    probe.send(b"T")
    probe.contains("inert-pty-fixtureT")
    probe.contains("server.cfg *")
    probe.send(b"\x1a")  # Ctrl+Z
    probe.expect(lambda text: "inert-pty-fixture" in text and "inert-pty-fixtureT" not in text and "server.cfg *" not in text, "typed edit undone")

    paste = b"\x1b[200~\n# paste-one\n# paste-two\x1b[201~"
    probe.send(paste)
    probe.contains("paste-one")
    probe.contains("paste-two")
    probe.contains("server.cfg *")
    for _ in range(paste_undo_limit):
        before = probe.screen.text.splitlines()[3:21]
        output_before = probe.total
        probe.send(b"\x1a")
        # Deleting a trailing space may only redraw the cursor's styling. The
        # final clean-buffer assertion still requires every pasted line to go.
        probe.expect(lambda text: text.splitlines()[3:21] != before or (paste_undo_limit > 1 and probe.total > output_before), "paste undo updated the editor")
        if "server.cfg *" not in probe.screen.text:
            break
    assert "server.cfg *" not in probe.screen.text, f"Paste was not undone in {paste_undo_limit} event(s)\n{probe.screen.text}"
    assert "inert-pty-fixture" in probe.screen.text and "paste-one" not in probe.screen.text and "paste-two" not in probe.screen.text
    probe.send(paste)
    probe.contains("paste-two")
    probe.send(b"\x1b")
    probe.contains("Unsaved changes. Ctrl+S saves; Ctrl+W discards.")
    probe.send(b"\x17")  # Ctrl+W discards. Never send Ctrl+S or access a keyring.
    probe.expect(lambda text: "server.cfg" in text and "bytes)" in text and "paste-two" not in text and "inert-pty-fixture" not in text, "discard returned to cfg list")
    probe.send(b"\r")
    probe.contains('set fixture_marker "original"')
    assert "paste-two" not in probe.screen.text and "server.cfg *" not in probe.screen.text
    probe.send(b"\x1b")
    probe.expect(lambda text: "server.cfg" in text and "bytes)" in text and "inert-pty-fixture" not in text, "clean cfg closes without an unsaved warning")
    resource_filter(probe)
    probe.send(b"\x1bOP")  # F1: q must quit, not type into an Actions form/editor.
    probe.contains("S: start")
    confirmation_batch_guard(probe)


def assert_unchanged(files):
    for path, content in files.items():
        assert path.read_bytes() == content, f"Read/edit/discard workflow changed fixture {path}"
