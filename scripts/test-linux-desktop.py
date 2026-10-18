"""Drive the actual Linux WebKit desktop with Tauri WebDriver, never mocked IPC."""

import base64
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen


BASE = "http://127.0.0.1:4444"
ELEMENT = "element-6066-11e4-a52e-4f735466cecf"


def request(method, route, body=None, *, read_only=False):
    data = None if body is None else json.dumps(body).encode()
    for attempt in range(2):
        try:
            with urlopen(Request(BASE + route, data=data, method=method, headers={"Content-Type": "application/json"}), timeout=40) as response:
                return json.load(response).get("value")
        except HTTPError as error:
            raise RuntimeError(error.read().decode()) from error
        except (ConnectionResetError, URLError) as error:
            reason = error.reason if isinstance(error, URLError) else error
            # RemoteDisconnected subclasses ConnectionResetError. Only explicitly
            # read-only commands may be replayed after an ambiguous response loss.
            if not read_only or attempt != 0 or not isinstance(reason, ConnectionResetError):
                raise
            print(
                f"WARNING: WebDriver {method} {route}: {type(reason).__name__}; "
                "retrying read-only request once without replacing the session.",
                file=sys.stderr, flush=True,
            )


def wait_until(action, timeout=30, *, wait_for_connection=False):
    deadline = time.monotonic() + timeout
    last_error = None
    while time.monotonic() < deadline:
        try:
            value = action()
            if value:
                return value
        except URLError as error:
            # Startup may precede the listening socket. Once reachable, transport
            # failures and WebDriver errors must not become another polling retry.
            if not wait_for_connection or not isinstance(error.reason, ConnectionRefusedError):
                raise
            last_error = error
        time.sleep(0.2)
    raise AssertionError(f"Timed out waiting for desktop state: {last_error}")


def smoke(binary, output):
    with tempfile.TemporaryDirectory(prefix="fxi-native-", dir="/tmp") as directory:
        root = Path(directory)
        environment = {**os.environ}
        for name, folder in {"HOME": "home", "XDG_DATA_HOME": "data", "XDG_CONFIG_HOME": "config", "XDG_CACHE_HOME": "cache", "XDG_RUNTIME_DIR": "runtime"}.items():
            (root / folder).mkdir(mode=0o700)
            environment[name] = str(root / folder)
        environment["LIBGL_ALWAYS_SOFTWARE"] = "1"
        with (output / "driver.log").open("w") as log, subprocess.Popen(
            ["tauri-driver", "--port", "4444", "--native-port", "4445"],
            env=environment, stdout=log, stderr=subprocess.STDOUT, start_new_session=True,
        ) as driver:
            session = None
            try:
                wait_until(lambda: request("GET", "/status", read_only=True), wait_for_connection=True)
                capabilities = {"capabilities": {"alwaysMatch": {"tauri:options": {"application": str(binary)}}}}
                session = request("POST", "/session", capabilities)["sessionId"]
                prefix = f"/session/{session}"
                request("POST", prefix + "/timeouts", {"script": 30000, "implicit": 1000})

                def execute(script, *args, read_only=False):
                    return request("POST", prefix + "/execute/sync", {"script": script, "args": list(args)}, read_only=read_only)

                def native(command, args=None):
                    result = request("POST", prefix + "/execute/async", {
                        "script": "const done=arguments[arguments.length-1]; window.__TAURI_INTERNALS__.invoke(arguments[0], arguments[1]).then(value=>done({ok:true,value}),error=>done({ok:false,error:String(error)}));",
                        "args": [command, args or {}],
                    })
                    assert result["ok"], f"Native {command} failed: {result}"
                    return result["value"]

                def click(selector):
                    wait_until(lambda: execute("""
                        const element = document.querySelector(arguments[0]);
                        if (!element || element.closest('[inert]') || element.disabled) return false;
                        element.scrollIntoView({ block: 'nearest', inline: 'nearest' });
                        const nav = element.closest('nav');
                        if (nav?.getAnimations({ subtree: true }).some(animation => animation.playState === 'running')) return false;
                        const rect = element.getBoundingClientRect();
                        const hit = document.elementFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2);
                        return rect.width > 0 && rect.height > 0 && element.contains(hit);
                    """, selector))
                    element = request("POST", prefix + "/element", {"using": "css selector", "value": selector})
                    request("POST", prefix + f"/element/{element[ELEMENT]}/click", {})

                def navigate(parent, child, heading):
                    selector = f'nav[aria-label="Workspace navigation"] [title="{parent}"]'
                    if execute("return document.querySelector(arguments[0])?.getAttribute('aria-expanded')", selector, read_only=True) != "true":
                        click(selector)
                    click(f'nav[aria-label="Workspace navigation"] [title="{child}"]')
                    wait_until(lambda: execute("return [...document.querySelectorAll('h1,h2')].some(e=>e.textContent.trim()===arguments[0])", heading, read_only=True))

                def screenshot(name):
                    content = request("GET", prefix + "/screenshot", read_only=True)
                    (output / f"{name}.png").write_bytes(base64.b64decode(content, validate=True))

                wait_until(lambda: execute("return !!document.querySelector('nav[aria-label=\"Workspace navigation\"]')", read_only=True))
                execute("window.nativeTestErrors=[]; addEventListener('error',e=>nativeTestErrors.push(e.message)); addEventListener('unhandledrejection',e=>nativeTestErrors.push(String(e.reason)));")
                info = native("get_platform_info")
                assert info["os"] == "linux" and info["artifactPlatform"] == "linux", info
                assert not info["clientLogsSupported"], info
                screenshot("home")
                navigate("Artifacts", "Install Artifact", "Install Artifact")
                wait_until(lambda: execute("return document.body.innerText.includes('Install Linux Artifact')", read_only=True))
                screenshot("artifacts")
                navigate("MariaDB", "Manage MariaDB", "Manage MariaDB")
                wait_until(lambda: execute("return document.body.innerText.includes('Install Configuration')", read_only=True))
                assert not execute("return document.body.innerText.includes('HeidiSQL')", read_only=True)
                screenshot("mariadb")
                navigate("FXServer", "Manage Server", "Manage Server")
                wait_until(lambda: execute("return document.body.innerText.includes('Performance')", read_only=True))
                screenshot("server")
                navigate("Logs", "Client Logs", "Client Logs")
                wait_until(lambda: execute("return document.body.innerText.includes('client log discovery is unavailable')", read_only=True))
                assert not execute("return document.documentElement.scrollWidth > innerWidth + 1", read_only=True)
                assert execute("return nativeTestErrors", read_only=True) == []
                screenshot("client-logs")
                assert execute("return !!document.querySelector('[title=\"Quit application\"]')", read_only=True)
                print(json.dumps({"platform": info, "pages": ["home", "artifacts", "mariadb", "server", "client-logs"], "nativeIpc": True}))
            except Exception:
                if session:
                    try:
                        screenshot("failure")
                        (output / "failure.txt").write_text(execute("return document.body.innerText", read_only=True), encoding="utf-8")
                    except (OSError, URLError, RuntimeError):
                        pass
                raise
            finally:
                if session:
                    try:
                        request("DELETE", f"/session/{session}")
                    except (OSError, URLError, RuntimeError):
                        pass
                if driver.poll() is None:
                    os.killpg(driver.pid, signal.SIGTERM)
                    try:
                        driver.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        os.killpg(driver.pid, signal.SIGKILL)
                        driver.wait(timeout=5)


if __name__ == "__main__":
    if sys.platform != "linux" or os.environ.get("GITHUB_ACTIONS") != "true" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise SystemExit("Run this only on a disposable GitHub-hosted Linux runner.")
    binary = Path(sys.argv[1]).resolve(strict=True)
    output = Path("output/native-linux")
    output.mkdir(parents=True, exist_ok=True)
    smoke(binary, output)
    print("Native Linux desktop startup, IPC, navigation, and layout checks passed.")
