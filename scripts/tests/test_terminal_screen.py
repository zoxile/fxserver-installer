"""Screen assertions must handle incremental updates, never old transcript hits."""

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("terminal_smoke", Path(__file__).resolve().parents[1] / "terminal_smoke.py")
terminal = importlib.util.module_from_spec(spec)
spec.loader.exec_module(terminal)


class ScreenTests(unittest.TestCase):
    def test_fragmented_cursor_updates_reconstruct_marker(self):
        screen = terminal.Screen(40, 4)
        for chunk in [b"\x1b[2;3HJSON old", b"\x1b[2;8", b"Htext\x1b[0K"]:
            screen.feed(chunk)
        self.assertIn("JSON text", screen.text)
        self.assertNotIn("old", screen.text)

    def test_erased_markers_do_not_survive_in_transcript(self):
        screen = terminal.Screen(20, 4)
        screen.feed(b"old-result\x1b[2J\x1b[Hnew-result")
        self.assertIn("new-result", screen.text)
        self.assertNotIn("old-result", screen.text)

    def test_line_clear_does_not_clear_other_rows(self):
        screen = terminal.Screen(20, 4)
        screen.feed(b"first\x1b[2;1Hsecond\x1b[3G\x1b[K")
        self.assertEqual(screen.text.splitlines()[:2], ["first", "se"])

    def test_split_utf8_and_style_do_not_split_visible_words(self):
        screen = terminal.Screen(20, 4)
        for byte in "\x1b[32m\u2502cfg \u754c\x1b[0m\u2502".encode():
            screen.feed(bytes([byte]))
        self.assertIn("\u2502cfg \u754c\u2502", screen.text)

    def test_alternate_screen_and_bracketed_paste(self):
        screen = terminal.Screen(20, 4)
        screen.feed(b"previous\x1b[?1049h\x1b[?2004hcurrent")
        self.assertTrue(screen.bracketed_paste)
        self.assertNotIn("previous", screen.text)
        screen.feed(b"\x1b[?2004l")
        self.assertFalse(screen.bracketed_paste)

    def test_relative_cursor_and_saved_position(self):
        screen = terminal.Screen(20, 4)
        screen.feed(b"abc\x1b7\x1b[2B\x1b[1Gthird\x1b8X")
        self.assertEqual(screen.text.splitlines()[:3], ["abcX", "", "third"])

    def test_osc_is_ignored_even_when_fragmented(self):
        screen = terminal.Screen(20, 4)
        screen.feed(b"\x1b]0;not-visible\x1b")
        screen.feed(b"\\visible")
        self.assertEqual(screen.text.strip(), "visible")

    def test_unknown_mutation_and_oversized_escape_fail_closed(self):
        with self.assertRaisesRegex(AssertionError, "Unsupported"):
            terminal.Screen().feed(b"\x1b[2L")
        with self.assertRaisesRegex(AssertionError, "oversized"):
            terminal.Screen().feed(b"\x1b]" + b"x" * 4097)

    def test_probe_observes_current_selected_line(self):
        chunks = iter([b"> first\x1b[2;1H  last", b"\x1b[H  \x1b[2;1H> "])
        probe = terminal.Probe(lambda _: None, lambda _: next(chunks))
        probe.selected("first")
        probe.selected("last")
        self.assertIn("  first", probe.screen.text)

    def test_probe_eof_and_output_budget_fail(self):
        with self.assertRaisesRegex(AssertionError, "closed"):
            terminal.Probe(lambda _: None, lambda _: b"").contains("absent")
        probe = terminal.Probe(lambda _: None, lambda _: b"x")
        probe.total = 2 * 1024 * 1024
        with self.assertRaisesRegex(AssertionError, "output limit"):
            probe.contains("absent")

    def test_remains_consumes_queued_frames_and_ignores_idle_reads(self):
        chunks = iter([None, b"\x1b[HFollowing\x1b[2;1Hupdated"])
        probe = terminal.Probe(lambda _: None, lambda _: next(chunks, None))
        probe.screen.feed(b"Following")
        ticks = iter(index / 100 for index in range(1000))
        with patch.object(terminal.time, "monotonic", side_effect=lambda: next(ticks)):
            probe.remains(lambda text: "Following" in text, "follow state is unchanged")
        self.assertIn("updated", probe.screen.text)
        self.assertGreater(probe.total, 0)

    def test_remains_rejects_a_transient_violation_even_if_later_restored(self):
        chunks = iter([b"\x1b[2J\x1b[HPaused", b"\x1b[2J\x1b[HFollowing"])
        probe = terminal.Probe(lambda _: None, lambda _: next(chunks, None))
        probe.screen.feed(b"Following")
        with self.assertRaisesRegex(AssertionError, "Changed during"):
            probe.remains(lambda text: "Following" in text, "input must not pause")

    def test_remains_keeps_eof_and_output_budget_guards(self):
        for chunk, total, message in ((b"", 0, "closed"), (b"x", 2 * 1024 * 1024, "output limit")):
            with self.subTest(message=message):
                probe = terminal.Probe(lambda _: None, lambda _: chunk)
                probe.total = total
                with self.assertRaisesRegex(AssertionError, message):
                    probe.remains(lambda _: True, "bounded live screen")

    def test_confirmation_probe_batches_input_then_cancels_the_visible_dialog(self):
        writes, chunks = [], []

        def send(data):
            writes.append(data)
            if data == b"xy":
                chunks.append(b"\x1b[2J\x1b[HConfirm Y | Cancel N/Esc\r\nStop the managed server?")
            elif data == b"N":
                self.assertIn("Stop the managed server?", probe.screen.text)
                chunks.append(b"\x1b[2J\x1b[HS: start")
            else:
                self.fail(f"Unexpected confirmation input: {data!r}")

        probe = terminal.Probe(send, lambda _: chunks.pop(0) if chunks else None)
        ticks = iter(index / 10 for index in range(1000))
        with patch.object(terminal.time, "monotonic", side_effect=lambda: next(ticks)):
            terminal.confirmation_batch_guard(probe)
        self.assertEqual(writes, [b"xy", b"N"])
        self.assertNotIn("Confirm Y", probe.screen.text)

    def test_confirmation_probe_rejects_a_dialog_that_disappears_before_cancellation(self):
        writes = []
        chunks = iter([
            b"\x1b[2J\x1b[HConfirm Y | Cancel N/Esc\r\nStop the managed server?",
            b"\x1b[2J\x1b[HS: start",
        ])
        probe = terminal.Probe(writes.append, lambda _: next(chunks, None))
        ticks = iter(index / 10 for index in range(1000))
        with patch.object(terminal.time, "monotonic", side_effect=lambda: next(ticks)):
            with self.assertRaisesRegex(AssertionError, "confirmation remains open"):
                terminal.confirmation_batch_guard(probe)
        self.assertEqual(writes, [b"xy"])

    def test_fixtures_are_disposable_and_unchanged(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            arguments, files = terminal.config_fixture(root)
            self.assertIn("--artifact-path", arguments)
            self.assertIn("--tx-data-path", arguments)
            self.assertNotIn("DBUS_SESSION_BUS_ADDRESS", terminal.isolated_environment(root))
            terminal.assert_unchanged(files)
            next(iter(files)).write_bytes(b"changed")
            with self.assertRaisesRegex(AssertionError, "changed fixture"):
                terminal.assert_unchanged(files)

    def test_config_find_fixture_has_literal_matches_and_a_regex_decoy(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            _, files = terminal.config_fixture(root)
            lines = files[root / "server-data/server.cfg"].decode().splitlines()
            self.assertEqual(lines[:2], ['# inert-pty-fixture', 'set fixture_marker "original"'])
            matches = [(row + 1, line.index("literal[.]") + 1) for row, line in enumerate(lines) if "literal[.]" in line]
            self.assertEqual(matches, [(3, 3), (5, 3)])
            self.assertEqual(lines[3], "# literal. decoy")

    def test_resource_fixtures_are_metadata_only_and_all_files_are_tracked(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            arguments, files = terminal.config_fixture(root)
            options = dict(zip(arguments[::2], arguments[1::2]))
            self.assertEqual(Path(options["--config"]), root / "terminal.json")
            self.assertEqual(Path(options["--artifact-path"]), root / "artifact")
            self.assertEqual(list((root / "artifact").iterdir()), [])
            profile = Path(options["--tx-data-path"]) / options["--profile"] / "config.json"
            data = Path(json.loads(files[profile])["server"]["dataPath"])
            self.assertEqual(data, root / "server-data")
            self.assertEqual(set(files), {path for path in root.rglob("*") if path.is_file()})
            self.assertEqual(len(files), 5)
            for name, version in (("fixture_alpha", "1.0.0"), ("fixture_beta", "2.0.0"), ("fixture_gamma", "1.0.0")):
                resource = data / "resources" / name
                manifest = resource / "fxmanifest.lua"
                self.assertEqual(list(resource.iterdir()), [manifest])
                self.assertEqual(files[manifest].decode().splitlines(), ["fx_version 'cerulean'", "game 'gta5'", f"version '{version}'"])
            self.assertFalse((root / "terminal.json").exists())
            terminal.assert_unchanged(files)

    def test_unchanged_guard_covers_each_resource_manifest(self):
        with tempfile.TemporaryDirectory() as directory:
            _, files = terminal.config_fixture(Path(directory))
            manifests = [path for path in files if path.name == "fxmanifest.lua"]
            self.assertEqual(len(manifests), 3)
            for path in manifests:
                with self.subTest(resource=path.parent.name):
                    path.write_bytes(files[path] + b"-- unexpected change\n")
                    with self.assertRaisesRegex(AssertionError, "changed fixture"):
                        terminal.assert_unchanged(files)
                    path.write_bytes(files[path])
            terminal.assert_unchanged(files)


class ConsoleScenarioTests(unittest.TestCase):
    def run_console_script(self, unexpected_reply=False, accept_overflow=False):
        query, replacement, command = "MiXeD multi word q/s", "retained on escape", "fixture / two words q"

        def frame(mode="Following", query="", command=None, reply="", filtering=False):
            status = "Paused | 0/2000 | 0 matches | 0 new" if mode == "Paused" else "Following | 0/2000 rows | 0 matches"
            label = "Filter (Enter/Esc: done): " if filtering else "Filter / Ctrl+F (Esc: clear): "
            lines = [status, label + query[-49:], "No console output yet."]
            if command is not None:
                lines.extend(["RCON command", command])
            lines.extend([reply, "Enter: RCON | Space: pause/follow"])
            return b"\x1b[2J\x1b[H" + "\r\n".join(lines).encode()

        following = frame(query=query)
        paused = frame("Paused", query)
        editing_command = frame("Paused", query, command)
        capped = "x" * 250 + "CAP256"
        steps = [
            (b"\x1bOQ", frame()),
            (b" ", frame("Paused")),
            (b"\x1b[F", frame()),
            (b"/", frame(filtering=True)),
            (query.encode(), frame(query=query, filtering=True)),
            (b"\r", following),
            (b" ", paused),
            (b"\x1bOP", b"\x1b[2J\x1b[HS: start"),
            (b"\x1bOQ", paused),
            (b"\t", b"\x1b[2J\x1b[HNo resources loaded."),
            (b"\x1b[Z", paused),
            (b"\r", frame("Paused", query, "")),
            (command.encode(), editing_command),
            (b"\x06", editing_command),
            (b"\x1b[F", editing_command),
            (b"\x1b", paused),
            (b"\r", frame("Paused", query, "")),
            (b"\r", frame("Paused", query, reply="[RCON error] unexpected submission" if unexpected_reply else "")),
            (b"\x06", frame("Paused", query, filtering=True)),
            (b"\x15", frame("Paused", filtering=True)),
            (b"\x1b[200~" + replacement.encode() + b"\x1b[201~", frame("Paused", replacement, filtering=True)),
            (b"\x1b", frame("Paused", replacement)),
            (b" ", frame(query=replacement)),
            (b"\x1b", frame()),
            (b"/", frame(filtering=True)),
            (capped.encode(), frame(query=capped, filtering=True)),
            (b"OVERFLOW", frame(query=capped + ("OVERFLOW" if accept_overflow else ""), filtering=True)),
            (b"\x15", frame(filtering=True)),
            (b"cleared", frame(query="cleared", filtering=True)),
            (b"\r", frame(query="cleared")),
            (b"\x1b", frame()),
        ]
        writes, chunks = [], []

        def send(data):
            self.assertTrue(steps, f"Unexpected extra input {data!r}")
            expected, output = steps.pop(0)
            self.assertEqual(data, expected)
            writes.append(data)
            chunks.append(output)

        probe = terminal.Probe(send, lambda _: chunks.pop(0) if chunks else None)
        ticks = iter(index / 100 for index in range(10000))
        with patch.object(terminal.time, "monotonic", side_effect=lambda: next(ticks)):
            terminal.console_controls(probe)
        self.assertFalse(steps)
        self.assertFalse(chunks)
        return writes

    def test_console_script_cancels_typed_rcon_and_allows_an_rcon_footer_hint(self):
        writes = self.run_console_script()
        typed = writes.index(b"fixture / two words q")
        self.assertEqual(writes[typed + 1:typed + 4], [b"\x06", b"\x1b[F", b"\x1b"])

    def test_console_script_rejects_an_actual_rcon_result(self):
        with self.assertRaisesRegex(AssertionError, "empty command does not produce an RCON response"):
            self.run_console_script(unexpected_reply=True)

    def test_console_script_rejects_a_filter_larger_than_256_bytes(self):
        with self.assertRaisesRegex(AssertionError, "bounded at 256 bytes"):
            self.run_console_script(accept_overflow=True)


if __name__ == "__main__":
    unittest.main()
