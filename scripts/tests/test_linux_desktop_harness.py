"""Transport-only tests: no driver, application, network, or filesystem fixtures."""

from contextlib import redirect_stderr
from http.client import BadStatusLine, RemoteDisconnected
import importlib.util
import io
import json
from pathlib import Path
import unittest
from unittest.mock import Mock, patch
from urllib.error import HTTPError, URLError


spec = importlib.util.spec_from_file_location(
    "linux_desktop_harness", Path(__file__).resolve().parents[1] / "test-linux-desktop.py"
)
harness = importlib.util.module_from_spec(spec)
spec.loader.exec_module(harness)

PREFIX = "/session/existing-session"
QUERY = {"script": "return nativeTestErrors", "args": []}


def response(value):
    return io.BytesIO(json.dumps({"value": value}).encode())


def webdriver_error():
    return HTTPError(
        harness.BASE + PREFIX + "/execute/sync", 500, "WebDriver error", {},
        io.BytesIO(b'{"value":{"error":"javascript error","message":"fixture failure"}}'),
    )


class DesktopTransportTests(unittest.TestCase):
    def test_read_only_disconnect_retries_once_in_the_same_session(self):
        warning = io.StringIO()
        with patch.object(harness, "urlopen", side_effect=[RemoteDisconnected("closed"), response([])]) as opened, redirect_stderr(warning):
            self.assertEqual(harness.request("POST", PREFIX + "/execute/sync", QUERY, read_only=True), [])
        self.assertEqual(opened.call_count, 2)
        for call in opened.call_args_list:
            request = call.args[0]
            self.assertEqual(request.full_url, harness.BASE + PREFIX + "/execute/sync")
            self.assertEqual(request.get_method(), "POST")
            self.assertEqual(json.loads(request.data), QUERY)
        self.assertEqual(warning.getvalue().count("WARNING:"), 1)
        self.assertIn("RemoteDisconnected", warning.getvalue())
        self.assertIn("without replacing the session", warning.getvalue())

    def test_explicit_status_and_screenshot_reads_retry_connection_resets(self):
        for route in ("/status", PREFIX + "/screenshot"):
            for error in (ConnectionResetError("reset"), URLError(ConnectionResetError("reset"))):
                with self.subTest(route=route, error=error):
                    with patch.object(harness, "urlopen", side_effect=[error, response("result")]) as opened, redirect_stderr(io.StringIO()):
                        self.assertEqual(harness.request("GET", route, read_only=True), "result")
                    self.assertEqual(opened.call_count, 2)

    def test_second_transport_failure_is_surfaced(self):
        for second in (RemoteDisconnected("second"), ConnectionResetError("second"), URLError(ConnectionResetError("second"))):
            with self.subTest(second=second):
                warning = io.StringIO()
                with patch.object(harness, "urlopen", side_effect=[RemoteDisconnected("first"), second, response([])]) as opened, redirect_stderr(warning):
                    with self.assertRaises(type(second)) as raised:
                        harness.request("POST", PREFIX + "/execute/sync", QUERY, read_only=True)
                self.assertIs(raised.exception, second)
                self.assertEqual(opened.call_count, 2)
                self.assertEqual(warning.getvalue().count("WARNING:"), 1)

    def test_default_requests_and_mutations_never_retry(self):
        commands = [
            ("GET", "/status", None),
            ("POST", PREFIX + "/execute/sync", QUERY),
            ("POST", PREFIX + "/execute/sync", {"script": "window.nativeTestErrors=[]", "args": []}),
            ("POST", PREFIX + "/element/button/click", {}),
            ("POST", PREFIX + "/execute/async", {"script": "invoke('mutation')", "args": []}),
            ("POST", "/session", {"capabilities": {}}),
            ("POST", PREFIX + "/timeouts", {"script": 30000}),
            ("DELETE", PREFIX, None),
        ]
        for method, route, body in commands:
            with self.subTest(method=method, route=route, body=body):
                warning = io.StringIO()
                error = RemoteDisconnected("ambiguous delivery")
                with patch.object(harness, "urlopen", side_effect=[error, response(None)]) as opened, redirect_stderr(warning):
                    with self.assertRaises(RemoteDisconnected):
                        harness.request(method, route, body)
                self.assertEqual(opened.call_count, 1)
                self.assertEqual(warning.getvalue(), "")

    def test_http_webdriver_errors_never_retry_even_for_reads(self):
        warning = io.StringIO()
        with patch.object(harness, "urlopen", side_effect=[webdriver_error(), response([])]) as opened, redirect_stderr(warning):
            with self.assertRaisesRegex(RuntimeError, "javascript error.*fixture failure"):
                harness.request("POST", PREFIX + "/execute/sync", QUERY, read_only=True)
        self.assertEqual(opened.call_count, 1)
        self.assertEqual(warning.getvalue(), "")

    def test_http_error_after_disconnect_is_not_retried(self):
        with patch.object(harness, "urlopen", side_effect=[RemoteDisconnected("closed"), webdriver_error(), response([])]) as opened, redirect_stderr(io.StringIO()):
            with self.assertRaisesRegex(RuntimeError, "javascript error"):
                harness.request("POST", PREFIX + "/execute/sync", QUERY, read_only=True)
        self.assertEqual(opened.call_count, 2)

    def test_other_transport_and_protocol_failures_never_retry(self):
        for error in (
            TimeoutError("timeout"), URLError(TimeoutError("timeout")),
            ConnectionRefusedError("refused"), URLError(ConnectionRefusedError("refused")),
            URLError("connection reset by peer"), BadStatusLine("invalid status"),
        ):
            with self.subTest(error=error):
                with patch.object(harness, "urlopen", side_effect=[error, response([])]) as opened, redirect_stderr(io.StringIO()):
                    with self.assertRaises(type(error)):
                        harness.request("POST", PREFIX + "/execute/sync", QUERY, read_only=True)
                self.assertEqual(opened.call_count, 1)

    def test_malformed_json_is_not_retried(self):
        with patch.object(harness, "urlopen", side_effect=[io.BytesIO(b"not json"), response([])]) as opened:
            with self.assertRaises(json.JSONDecodeError):
                harness.request("POST", PREFIX + "/execute/sync", QUERY, read_only=True)
        self.assertEqual(opened.call_count, 1)

    def test_response_body_disconnect_uses_the_same_retry_limit(self):
        broken = Mock()
        broken.__enter__ = Mock(return_value=broken)
        broken.__exit__ = Mock(return_value=False)
        broken.read.side_effect = RemoteDisconnected("body closed")
        with patch.object(harness, "urlopen", side_effect=[broken, response([])]) as opened, redirect_stderr(io.StringIO()):
            self.assertEqual(harness.request("POST", PREFIX + "/execute/sync", QUERY, read_only=True), [])
        self.assertEqual(opened.call_count, 2)
        broken.__exit__.assert_called_once()

    def test_polling_cannot_replay_an_exhausted_transport_retry(self):
        for startup in (False, True):
            with self.subTest(startup=startup):
                with patch.object(harness, "urlopen", side_effect=[RemoteDisconnected("first"), RemoteDisconnected("second"), response(True)]) as opened, redirect_stderr(io.StringIO()):
                    with self.assertRaisesRegex(RemoteDisconnected, "second"):
                        harness.wait_until(
                            lambda: harness.request("GET", "/status", read_only=True),
                            wait_for_connection=startup,
                        )
                self.assertEqual(opened.call_count, 2)

    def test_polling_surfaces_webdriver_errors_and_assertions(self):
        with patch.object(harness, "urlopen", side_effect=[webdriver_error(), response(True)]) as opened:
            with self.assertRaisesRegex(RuntimeError, "javascript error"):
                harness.wait_until(lambda: harness.request("POST", PREFIX + "/execute/sync", QUERY, read_only=True))
        self.assertEqual(opened.call_count, 1)
        action = Mock(side_effect=AssertionError("JavaScript result assertion"))
        with self.assertRaisesRegex(AssertionError, "JavaScript result assertion"):
            harness.wait_until(action)
        action.assert_called_once()

    def test_polling_still_waits_for_false_conditions(self):
        action = Mock(side_effect=[False, None, True])
        with patch.object(harness.time, "sleep") as sleep:
            self.assertTrue(harness.wait_until(action))
        self.assertEqual(action.call_count, 3)
        self.assertEqual(sleep.call_count, 2)

    def test_only_startup_may_wait_for_a_refused_connection(self):
        error = URLError(ConnectionRefusedError("not listening yet"))
        action = Mock(side_effect=[error, True])
        with patch.object(harness.time, "sleep"):
            self.assertTrue(harness.wait_until(action, wait_for_connection=True))
        self.assertEqual(action.call_count, 2)
        action = Mock(side_effect=[error, True])
        with self.assertRaises(URLError):
            harness.wait_until(action)
        action.assert_called_once()


if __name__ == "__main__":
    unittest.main()
