#!/usr/bin/env python3
"""Verify the original crate and the exact RUSTSEC-2024-0429 backport."""

import argparse
import hashlib
import io
from pathlib import Path, PurePosixPath
import tarfile
from urllib.request import urlopen


ARCHIVE_URL = "https://static.crates.io/crates/glib/glib-0.18.5.crate"
ARCHIVE_SHA256 = "233daaf6e83ae6a12a52055f568f9d7cf4671dabb78ff9560ab6da230ce00ee5"
PATCH_SHA256 = "2229104a8f6d0b4669c88d75f6868ff719ffe269d3f114ef7610a4b667baf8b2"
PATCH_FILE = "patches/RUSTSEC-2024-0429.patch"
LOCAL_FILES = {".gitattributes", "BACKPORT.md", PATCH_FILE, "verify_backport.py"}
REPLACEMENTS = (
    (b"let p: *mut libc::c_char = std::ptr::null_mut();",
     b"let mut p: *mut libc::c_char = std::ptr::null_mut();"),
    (b"                &p,\n", b"                &mut p,\n"),
)


def verify(archive: bytes, root: Path) -> int:
    if hashlib.sha256(archive).hexdigest() != ARCHIVE_SHA256:
        raise ValueError("Original crates.io archive checksum mismatch")
    if hashlib.sha256((root / PATCH_FILE).read_bytes()).hexdigest() != PATCH_SHA256:
        raise ValueError("Recorded upstream patch has changed")

    originals = set()
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as crate:
        for member in crate.getmembers():
            name = PurePosixPath(member.name)
            if (not member.isfile() or name.parts[0] != "glib-0.18.5"
                    or ".." in name.parts or "\\" in member.name):
                raise ValueError(f"Unexpected archive entry: {member.name}")
            relative = name.relative_to("glib-0.18.5").as_posix()
            if relative in originals:
                raise ValueError(f"Duplicate archive entry: {relative}")
            originals.add(relative)
            with crate.extractfile(member) as source:
                expected = source.read()
            if relative == "src/variant_iter.rs":
                for before, after in REPLACEMENTS:
                    if expected.count(before) != 1:
                        raise ValueError("Upstream patch context does not match")
                    expected = expected.replace(before, after, 1)
            actual = root / relative
            if not actual.resolve().is_relative_to(root):
                raise ValueError(f"Vendor file resolves outside crate: {relative}")
            if actual.read_bytes() != expected:
                raise ValueError(f"Unexpected vendor change: {relative}")

    files = {path.relative_to(root).as_posix() for path in root.rglob("*") if path.is_file()}
    if files != originals | LOCAL_FILES:
        raise ValueError(f"Unexpected vendor files: {files ^ (originals | LOCAL_FILES)}")
    return len(originals)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, help="Use an existing checksummed .crate archive")
    args = parser.parse_args()
    if args.archive:
        archive = args.archive.read_bytes()
    else:
        with urlopen(ARCHIVE_URL, timeout=30) as response:
            archive = response.read(267680)
    count = verify(archive, Path(__file__).resolve().parent)
    print(f"Verified {count} original GLib 0.18.5 files; only the exact two-line backport differs.")


if __name__ == "__main__":
    main()
