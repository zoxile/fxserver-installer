"""Screen assertions must handle incremental updates, never old transcript hits."""

import importlib.util
from pathlib import Path
import tempfile
import unittest

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


if __name__ == "__main__":
    unittest.main()
