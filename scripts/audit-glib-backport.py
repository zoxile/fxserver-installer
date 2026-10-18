"""Keep RustSec coverage for the pinned GLib crate despite its local Cargo patch."""

import json
from pathlib import Path
import subprocess
import tempfile
import tomllib


PATCHED_ADVISORY = "RUSTSEC-2024-0429"


def review(report):
    findings = list(report["vulnerabilities"]["list"])
    for warnings in report.get("warnings", {}).values():
        findings.extend(warnings)
    unresolved = []
    for finding in findings:
        advisory = finding["advisory"]
        package = finding["package"]
        if (advisory["id"] == PATCHED_ADVISORY and package["name"] == "glib"
                and package["version"] == "0.18.5"):
            print(f"{PATCHED_ADVISORY}: covered by the separately verified upstream backport.")
        else:
            unresolved.append(f"{advisory['id']}: {advisory['title']}")
    return unresolved


def main():
    root = Path(__file__).resolve().parent.parent
    with (root / "src-tauri/Cargo.lock").open("rb") as source:
        packages = tomllib.load(source)["package"]
    glib = [package for package in packages if package["name"] == "glib"]
    if len(glib) != 1 or glib[0]["version"] != "0.18.5" or "source" in glib[0]:
        raise SystemExit("Review the GLib backport audit policy after changing its pinned dependency.")
    with tempfile.TemporaryDirectory(prefix="fxi-glib-audit-") as directory:
        lock = Path(directory) / "Cargo.lock"
        lock.write_text(
            'version = 4\n\n[[package]]\nname = "glib"\nversion = "0.18.5"\n'
            'source = "registry+https://github.com/rust-lang/crates.io-index"\n'
            'checksum = "233daaf6e83ae6a12a52055f568f9d7cf4671dabb78ff9560ab6da230ce00ee5"\n',
            encoding="utf-8",
        )
        result = subprocess.run(
            ["cargo", "audit", "--file", str(lock), "--json", "--no-fetch"],
            check=False, capture_output=True, text=True, timeout=60,
        )
    if result.returncode not in (0, 1):
        raise SystemExit("GLib advisory lookup failed: " + result.stderr[-4000:])
    report = json.loads(result.stdout)
    if report["settings"].get("ignore"):
        raise SystemExit("GLib advisory lookup must not suppress advisories.")
    unresolved = review(report)
    if unresolved:
        raise SystemExit("Unresolved vendored GLib advisories:\n" + "\n".join(unresolved))
    print("Registry-equivalent GLib audit found no additional unresolved advisories.")


if __name__ == "__main__":
    main()
