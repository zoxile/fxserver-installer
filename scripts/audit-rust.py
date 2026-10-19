"""Retry transient RustSec downloads without relaxing the audit policy."""

import subprocess
import sys
import time


def audit():
    command = ["cargo", "audit", "--color", "never", "--file", "src-tauri/Cargo.lock"]
    for attempt in range(3):
        try:
            result = subprocess.run(
                command, capture_output=True, text=True, encoding="utf-8",
                errors="replace", timeout=180, check=False,
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            print(f"Rust security audit could not complete: {error}", file=sys.stderr)
            return 1
        print(result.stdout, end="", flush=True)
        print(result.stderr, end="", file=sys.stderr, flush=True)
        if result.returncode == 0:
            return 0
        transport_failure = (
            "couldn't fetch advisory database" in result.stderr
            and "error sending request for url" in result.stderr
        )
        if not transport_failure or attempt == 2:
            return result.returncode
        delay = 2 * (attempt + 1)
        print(
            f"RustSec download failed; retrying in {delay}s ({attempt + 2}/3).",
            file=sys.stderr, flush=True,
        )
        time.sleep(delay)
    return 1


if __name__ == "__main__":
    raise SystemExit(audit())
