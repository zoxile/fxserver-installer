"""Validate the packages produced by this checkout, without installing them."""

import configparser
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile


def exactly_one(root, pattern):
    paths = list(root.glob(pattern))
    assert len(paths) == 1, f"Expected one {pattern}, found {paths}"
    assert paths[0].stat().st_size > 1_000_000, f"Package is unexpectedly small: {paths[0]}"
    return paths[0].resolve()


root = Path(sys.argv[1]).resolve(strict=True)
deb = exactly_one(root, "deb/*.deb")
rpm = exactly_one(root, "rpm/*.rpm")
appimage = exactly_one(root, "appimage/*.AppImage")
subprocess.run(["dpkg-deb", "--info", str(deb)], check=True, timeout=20)
subprocess.run(["rpm", "--query", "--package", "--info", str(rpm)], check=True, timeout=20)
for kind, package in (("deb", deb), ("rpm", rpm), ("appimage", appimage)):
    with tempfile.TemporaryDirectory(prefix=f"fxi-package-{kind}-") as directory:
        destination = Path(directory)
        if kind == "deb":
            command = ["dpkg-deb", "--extract", str(package), directory]
        elif kind == "rpm":
            command = ["bsdtar", "--extract", "--file", str(package), "--directory", directory]
        else:
            command = [str(package), "--appimage-extract"]
        subprocess.run(command, cwd=directory, stdout=subprocess.DEVNULL, check=True, timeout=120)
        desktops = list(destination.rglob("*.desktop"))
        assert desktops, f"No desktop entry in {kind} package"
        names = []
        for desktop in desktops:
            entry = configparser.ConfigParser(interpolation=None)
            entry.read(desktop)
            if entry.get("Desktop Entry", "Name", fallback="") == "FXServer Installer":
                names.append(Path(shlex.split(entry.get("Desktop Entry", "Exec"))[0]).name)
        assert names, f"Wrong desktop name in {kind}"
        binaries = [path for name in names for path in destination.rglob(name) if path.is_file() and "bin" in path.parts]
        assert binaries, f"No application binary in {kind} package"
        assert all(path.stat().st_mode & 0o111 for path in binaries), f"Application is not executable in {kind}"
        assert list(destination.rglob("*.png")), f"No icon in {kind} package"
        package_root = destination / "squashfs-root" if kind == "appimage" else destination
        terminal_dir = package_root / "usr/bin"
        terminals = [terminal_dir / name for name in ("fxins", "fxserver-manager")]
        for terminal in terminals:
            assert terminal.is_file(), f"Missing {terminal.name} in {kind} /usr/bin"
            assert terminal.stat().st_mode & 0o111, f"{terminal.name} is not executable in {kind}"
        environment = {key: value for key, value in os.environ.items() if key not in ("DISPLAY", "WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS")}
        environment["PATH"] = str(terminal_dir) + os.pathsep + environment.get("PATH", "")
        for terminal in terminals:
            assert Path(shutil.which(terminal.name, path=environment["PATH"])).resolve() == terminal.resolve()
            result = subprocess.run([terminal.name, "--help"], env=environment, capture_output=True, text=True, check=True, timeout=20)
            assert "database" in result.stdout and "--help" in result.stdout, f"Missing terminal commands in {kind}"
        subprocess.run(["node", str(Path(__file__).with_name("smoke-cli.mjs").resolve()), *(str(path) for path in terminals)], env=environment, check=True, timeout=120)
print("DEB, RPM, and AppImage metadata, desktop contents, and terminal startup passed.")
