#!/usr/bin/env python3
"""Real MariaDB checks, restricted to disposable GitHub-hosted Linux containers.

Run as the container entrypoint, as root. The app itself runs as fxi-db-ci and
elevates only package/service commands through sudo. Never mount host DB data,
the Docker socket, or a host PID namespace. Destroy the container after any error.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import selectors
import shutil
import signal
import socket
import stat
import subprocess
import sys
import tempfile
import time


ACK = "github-container-v1"
USER = "fxi-db-ci"
DATABASE = "fxi_ci_fixture"
OUTPUT_LIMIT = 4 * 1024 * 1024
FIXTURE_LOGS = ("bootstrap.log", "server.log", "daemon-output.log")
LOG_READ_LIMIT = 16 * 1024


def option_value(value):
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n").replace("\r", "\\r").replace("\t", "\\t") + '"'


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def read_fixture_log(root, name):
    require(name in FIXTURE_LOGS, "Only known fixture error logs may be read.")
    require(root.parent == Path("/tmp") and root.name.startswith("fxi-mariadb-integration-")
            and root.resolve() == root, "Invalid fixture log directory.")
    directory = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        descriptor = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC, dir_fd=directory)
    finally:
        os.close(directory)
    with os.fdopen(descriptor, "rb") as source:
        metadata = os.fstat(source.fileno())
        require(stat.S_ISREG(metadata.st_mode) and metadata.st_nlink == 1,
                "Fixture error log must be a single-link regular file.")
        offset = max(0, metadata.st_size - LOG_READ_LIMIT)
        source.seek(offset)
        content = source.read(LOG_READ_LIMIT)
        if offset:
            # Drop a partial first line, which could contain a truncated secret.
            content = content.partition(b"\n")[2]
        return content.decode("utf-8", "replace")


def check_context():
    require(sys.platform == "linux", "This harness never runs on Windows or macOS.")
    require(os.environ.get("GITHUB_ACTIONS") == "true", "GitHub Actions is required.")
    require(os.environ.get("RUNNER_ENVIRONMENT") == "github-hosted", "Only GitHub-hosted runners are allowed.")
    require(os.environ.get("FXI_DISPOSABLE_MARIADB") == ACK, "Disposable-container acknowledgement is missing.")
    require(re.fullmatch(r"[1-9][0-9]*", os.environ.get("GITHUB_RUN_ID", "")), "A GitHub run ID is required.")
    require(os.geteuid() == 0, "Only the container harness, not the app, must run as root.")
    require(Path("/.dockerenv").is_file(), "Run in a new Docker container, not on the runner host.")
    require(not Path("/var/run/docker.sock").exists(), "Do not expose the Docker socket to this container.")
    require(not any_database_process(), "Container already has a database process; refusing to touch it.")
    for path in [Path("/tmp"), Path("/var/lib/mysql"), Path("/etc"), Path("/run")]:
        require_private_mount(path)


def require_private_mount(path):
    require(path.resolve() == path, f"Unexpected symlink in test path: {path}")
    mounts = []
    for line in Path("/proc/self/mountinfo").read_text().splitlines():
        before, after = line.split(" - ", 1)
        fields = before.split()
        mount = Path(fields[4])
        if mount == path or mount in path.parents:
            mounts.append((len(mount.parts), fields[3], after.split()[0]))
    require(bool(mounts), f"Cannot identify mount for {path}")
    _, root, filesystem = max(mounts)
    require(root == "/" and filesystem in {"overlay", "tmpfs"}, f"{path} must be container-owned overlay/tmpfs, not a host bind or volume.")


def any_database_process():
    for path in Path("/proc").glob("[0-9]*/comm"):
        try:
            if path.read_text().strip() in {"mariadbd", "mysqld", "mysqld_safe"}:
                return True
        except (OSError, UnicodeError):
            pass
    return False


def run(command, *, env, user=None, stdin=None, tty=False, timeout=60):
    """Bound time and output; never invoke a shell or log argv containing SQL."""
    kwargs = {"env": env, "start_new_session": True}
    if user is not None:
        kwargs.update(user=user.pw_uid, group=user.pw_gid, extra_groups=[])
    master = slave = None
    if tty:
        import pty
        master, slave = pty.openpty()
        kwargs.update(stdin=slave, stdout=slave, stderr=slave)
    else:
        kwargs.update(stdin=subprocess.PIPE if stdin is not None else subprocess.DEVNULL,
                      stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    child = subprocess.Popen([str(arg) for arg in command], **kwargs)
    if slave is not None:
        os.close(slave)
    stdout, stderr = bytearray(), bytearray()
    deadline = time.monotonic() + timeout
    selector = selectors.DefaultSelector()
    try:
        if stdin is not None:
            require(len(stdin) <= 4096, "Harness stdin is limited to 4 KiB.")
            child.stdin.write(stdin)
            child.stdin.close()
        if tty:
            selector.register(master, selectors.EVENT_READ, stdout)
        else:
            selector.register(child.stdout, selectors.EVENT_READ, stdout)
            selector.register(child.stderr, selectors.EVENT_READ, stderr)
        while selector.get_map():
            require(time.monotonic() < deadline, "Command deadline exceeded; discard this container.")
            for key, _ in selector.select(min(0.2, max(0, deadline - time.monotonic()))):
                try:
                    chunk = os.read(key.fd, 8192)
                except OSError as error:
                    if not tty or error.errno != 5:
                        raise
                    chunk = b""
                if not chunk:
                    selector.unregister(key.fileobj)
                else:
                    key.data.extend(chunk)
                    require(len(stdout) + len(stderr) <= OUTPUT_LIMIT, "Command output limit exceeded.")
        code = child.wait(timeout=max(0.01, deadline - time.monotonic()))
        return code, stdout.decode("utf-8", "replace"), stderr.decode("utf-8", "replace")
    finally:
        selector.close()
        if child.returncode is None:
            # The leader has not been reaped, so its process group cannot be reused.
            os.killpg(child.pid, signal.SIGKILL)
            child.wait(timeout=5)
        if master is not None:
            os.close(master)
        for pipe in [child.stdin, child.stdout, child.stderr]:
            if pipe is not None:
                pipe.close()


class Fixture:
    def __init__(self, args):
        import pwd
        self.args = args
        self.user = pwd.getpwnam(USER)
        require(self.user.pw_uid == 10001, "Use the dedicated fxi-db-ci image user (UID 10001).")
        self.root = Path(tempfile.mkdtemp(prefix="fxi-mariadb-integration-", dir="/tmp"))
        os.chown(self.root, self.user.pw_uid, self.user.pw_gid)
        self.environment = dict(os.environ, HOME=str(self.root), XDG_DATA_HOME=str(self.root / "app-data"),
                                LC_ALL="C", TERM="xterm-256color", PATH="/usr/sbin:/usr/bin:/sbin:/bin")
        for key in ["MYSQL_PWD", "MYSQL_HOST", "MYSQL_TCP_PORT", "MYSQL_UNIX_PORT", "LD_PRELOAD", "LD_LIBRARY_PATH"]:
            self.environment.pop(key, None)
        self.config = self.root / "settings.json"
        self.write_private(self.config, "{}")
        self.password = secrets.token_hex(20) + "#'\"\\= space"
        self.reader_password = secrets.token_hex(20)
        self.daemon = None
        self.daemon_log = None
        self.systemd_configured = False
        self.datadir = Path("/var/lib/mysql") if args.packages else self.root / "data"
        self.socket = self.root / "db.sock"
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            self.port = listener.getsockname()[1]
        self.results = {"distro": self.os_release(), "checks": [], "limits": []}

    @staticmethod
    def os_release():
        values = {}
        for line in Path("/etc/os-release").read_text().splitlines():
            if "=" in line:
                key, value = line.split("=", 1)
                values[key] = value.strip('"')
        require(values.get("ID") in {"ubuntu", "debian", "fedora", "arch"}, "Unsupported integration distro.")
        return values["ID"]

    def write_private(self, path, content):
        with path.open("x", encoding="utf-8") as output:
            os.fchmod(output.fileno(), 0o600)
            os.fchown(output.fileno(), self.user.pw_uid, self.user.pw_gid)
            output.write(content)

    def redact(self, value):
        for secret in [self.password, self.reader_password]:
            for variant in [secret, option_value(secret)[1:-1], json.dumps(secret)[1:-1],
                            secret.replace("\\", "\\\\").replace("'", "\\'"), secret.encode().hex(), secret[:40]]:
                if variant:
                    value = value.replace(variant, "[redacted]")
        return value

    def checked(self, command, **kwargs):
        executable = json.dumps(self.redact(Path(command[0]).name))
        try:
            code, out, err = run(command, env=self.environment, **kwargs)
        except Exception as error:
            detail = json.dumps(self.redact(str(error))[-6000:])
            raise RuntimeError(f"Command {executable} could not complete: {detail}") from None
        if code != 0:
            detail = self.redact(out + "\n" + err).strip()[-6000:] or "No stdout or stderr was produced."
            raise RuntimeError(f"Command {executable} failed ({code}): {json.dumps(detail)}")
        return out

    def failure_logs(self):
        for name in FIXTURE_LOGS:
            try:
                contents = read_fixture_log(self.root, name)
                diagnostic = {"log": name, "output": self.redact(contents).strip()[-6000:] or "Log is empty."}
            except FileNotFoundError:
                diagnostic = {"log": name, "status": "not created"}
            except Exception as error:
                diagnostic = {"log": name, "error": self.redact(str(error))[:1000]}
            print("DIAGNOSTIC " + json.dumps(diagnostic), file=sys.stderr, flush=True)

    def cli(self, *arguments, secret=None, success=True, tty=False, timeout=90):
        code, out, err = run([self.args.binary, "--config", self.config, *arguments],
                             env=self.environment, user=self.user, tty=tty,
                             stdin=(secret + "\n").encode() if secret is not None else None, timeout=timeout)
        require((code == 0) == success, f"CLI {' '.join(arguments[:2])}: unexpected exit {code}: {self.redact(out + err)[-6000:]}")
        return out, err

    def cli_json(self, *arguments, **kwargs):
        out, _ = self.cli(*arguments, **kwargs)
        return json.loads(out)

    def query(self, sql, *, reader=False, success=True, file=None, database=DATABASE):
        query = ["--file", str(file)] if file else [sql]
        return self.cli_json("query", *query, "--execute", "--host", "127.0.0.1", "--port", str(self.port),
                             "--user", "fxi_ci_reader" if reader else "fxi_ci_admin", "--database", database,
                             "--password-stdin", secret=self.reader_password if reader else self.password, success=success)

    def passed(self, name):
        self.results["checks"].append(name)
        print(f"PASS {name}", flush=True)

    def backup_diagnostics(self):
        require(sys.platform == "linux" and os.environ.get("GITHUB_ACTIONS") == "true"
                and os.environ.get("RUNNER_ENVIRONMENT") == "github-hosted"
                and os.environ.get("FXI_DISPOSABLE_MARIADB") == ACK,
                "Backup diagnostics require disposable Linux CI.")
        require(self.root.parent == Path("/tmp") and self.root.name.startswith("fxi-mariadb-integration-")
                and self.root.resolve() == self.root and not self.root.is_symlink(), "Invalid diagnostic directory.")
        require((self.datadir / ".fxi-ci-owned").read_text() == os.environ["GITHUB_RUN_ID"], "Unowned diagnostic database.")
        client = shutil.which("mariadb-dump") or shutil.which("mysqldump")
        require(bool(client), "No dump client for diagnostics.")
        defaults = self.root / "dump-diagnostic.cnf"
        self.write_private(defaults, "[client]\nuser=fxi_ci_admin\npassword=" + option_value(self.password) + "\n")
        try:
            probes = [
                ("version", [client, "--no-defaults", "--version"]),
                ("fixture dump", [client, f"--defaults-file={defaults}", "--protocol=tcp", "--host=127.0.0.1",
                                  f"--port={self.port}", "--hex-blob", "--default-character-set=utf8mb4",
                                  "--single-transaction", "--quick", "--routines", "--triggers", "--events", DATABASE]),
            ]
            for label, command in probes:
                code, out, err = run(command, env=self.environment, user=self.user, timeout=30)
                # Never print dump stdout (SQL), argv or option-file contents.
                diagnostic = self.redact(err if label == "fixture dump" else out + err)[:6000]
                print("DIAGNOSTIC " + json.dumps({"probe": label, "exit": code, "output": diagnostic}),
                      file=sys.stderr, flush=True)
        finally:
            defaults.unlink()

    def backup(self):
        try:
            return self.cli_json("database", "backup", "--output", str(self.root), "--host", "127.0.0.1",
                                 "--port", str(self.port), "--user", "fxi_ci_admin", "--database", DATABASE,
                                 "--password-stdin", secret=self.password)
        except Exception:
            try:
                self.backup_diagnostics()
            except Exception as error:
                print("DIAGNOSTIC " + json.dumps({"error": self.redact(str(error))[:6000]}), file=sys.stderr, flush=True)
            raise

    def catalog(self):
        packages = self.cli_json("database", "packages")
        require(not packages["error"], f"Package catalog: {packages['error']}")
        candidate = packages["latestVersion"]
        require(bool(candidate), "No distro candidate was returned.")
        series = self.cli_json("database", "versions")
        require(bool(series), "No install series was returned.")
        versions = [release["version"] for item in series
                    for release in self.cli_json("database", "versions", item["series"])]
        require(candidate in versions, "Candidate is missing from install choices.")
        self.results["candidate"] = candidate
        self.passed("CLI package detection and version catalog")
        return candidate

    def install(self, candidate):
        require(not self.cli_json("database", "status")["installed"], "New-install test requires an image without MariaDB server packages.")
        require(not self.datadir.exists() or not any(self.datadir.iterdir()), "Refusing existing default database data.")
        self.cli("database", "install", success=False)
        self.cli("database", "install", "--version", "9999.9999.9999", "--yes", success=False)
        require(not self.cli_json("database", "status")["installed"], "Rejected install unexpectedly installed a server.")
        self.cli("database", "install", "--version", candidate, "--yes", tty=True, timeout=600)
        require(self.cli_json("database", "status")["installed"], "Installed server was not detected.")
        self.passed("app exact-version new install and invalid-choice rejection")
        if self.args.systemd:
            self.cli("database", "stop", "--yes", tty=True)
        require(not any_database_process(), "Package started an unmanaged daemon; discard this container.")

    def initialize(self):
        self.datadir.mkdir(mode=0o700, exist_ok=True)
        require(self.datadir.resolve() == self.datadir, "Refusing symlinked data directory.")
        for base, directories, files in os.walk(self.datadir, followlinks=False):
            for name in ["", *directories, *files]:
                path = Path(base) / name
                require(not path.is_symlink(), "Unexpected symlink in disposable initial database.")
                os.chown(path, self.user.pw_uid, self.user.pw_gid)
        if not (self.datadir / "mysql").is_dir():
            self.bootstrap()
        self.write_private(self.datadir / ".fxi-ci-owned", os.environ["GITHUB_RUN_ID"])
        if self.args.systemd:
            self.configure_systemd()
        self.start()
        escape = lambda value: value.replace("\\", "\\\\").replace("'", "\\'")
        sql = (f"SET SESSION sql_mode=''; CREATE DATABASE {DATABASE}; "
               f"CREATE USER 'fxi_ci_admin'@'127.0.0.1' IDENTIFIED BY '{escape(self.password)}'; "
               "GRANT ALL PRIVILEGES ON *.* TO 'fxi_ci_admin'@'127.0.0.1'; "
               f"CREATE USER 'fxi_ci_reader'@'127.0.0.1' IDENTIFIED BY '{self.reader_password}'; "
               f"GRANT SELECT ON {DATABASE}.* TO 'fxi_ci_reader'@'127.0.0.1';")
        self.checked(["mariadb", "--no-defaults", "--protocol=SOCKET", f"--socket={self.socket}", "--user=root"], stdin=sql.encode())
        require(self.query("SELECT @@datadir;")["rows"] == [[str(self.datadir) + "/"]], "Connected to the wrong data directory.")

    def bootstrap(self):
        log = self.root / "bootstrap.log"
        self.write_private(log, "")
        command = ["mariadb-install-db", "--no-defaults"]
        if self.results["distro"] == "arch":
            command.append("--basedir=/usr")
        command.extend([f"--datadir={self.datadir}", "--auth-root-authentication-method=normal",
                        "--skip-test-db", f"--log-error={log}"])
        self.checked(command, user=self.user, timeout=120)

    def daemon_arguments(self):
        binary = shutil.which("mariadbd")
        require(bool(binary), "mariadbd is not installed.")
        return [binary, "--no-defaults", f"--datadir={self.datadir}", f"--socket={self.socket}",
                f"--pid-file={self.root / 'db.pid'}", "--bind-address=127.0.0.1", f"--port={self.port}",
                "--skip-name-resolve", "--innodb-buffer-pool-size=64M", "--max-connections=20",
                f"--log-error={self.root / 'server.log'}"]

    def configure_systemd(self):
        require(Path("/proc/1/comm").read_text().strip() == "systemd", "--systemd requires a real container systemd PID 1.")
        directory = Path("/run/systemd/system/mariadb.service.d")
        require(not directory.exists(), "Refusing existing runtime service overrides.")
        directory.mkdir(parents=True)
        unit = ("[Service]\nType=simple\nRestart=no\nUser=fxi-db-ci\nGroup=fxi-db-ci\n"
                "PrivateTmp=false\nExecStartPre=\nExecStartPost=\nExecStop=\nExecStopPost=\nExecStart=\n"
                "ExecStart=" + " ".join(self.daemon_arguments()) + "\n"
                f"ReadWritePaths={self.root} {self.datadir}\nTimeoutStartSec=60\nTimeoutStopSec=30\n")
        (directory / "90-fxi-ci.conf").write_text(unit)
        self.checked(["systemctl", "daemon-reload"], timeout=30)
        self.systemd_configured = True

    def start(self):
        if self.args.systemd:
            self.cli("database", "start", tty=True)
        else:
            self.daemon_log = (self.root / "daemon-output.log").open("ab")
            self.daemon = subprocess.Popen(self.daemon_arguments(), env=self.environment,
                                            user=self.user.pw_uid, group=self.user.pw_gid, extra_groups=[],
                                            stdin=subprocess.DEVNULL, stdout=self.daemon_log, stderr=self.daemon_log,
                                            start_new_session=True)
        deadline = time.monotonic() + 45
        while time.monotonic() < deadline:
            code, _, _ = run(["mariadb-admin", "--no-defaults", "--protocol=SOCKET", f"--socket={self.socket}",
                              "--connect-timeout=1", "ping"], env=self.environment, timeout=3)
            if code == 0:
                return
            time.sleep(0.2)
        raise RuntimeError("Test database did not become ready within 45 seconds.")

    def stop(self):
        if self.args.systemd and self.systemd_configured:
            self.cli("database", "stop", "--yes", tty=True)
        elif self.daemon is not None:
            self.daemon.terminate()
            try:
                self.daemon.wait(timeout=30)
            except subprocess.TimeoutExpired:
                self.daemon.kill()
                self.daemon.wait(timeout=5)
                raise RuntimeError("Fixture daemon did not shut down cleanly.")
            finally:
                self.daemon = None
                self.daemon_log.close()
                self.daemon_log = None

    def exercise(self):
        self.query("CREATE TABLE items (id INT PRIMARY KEY, label VARCHAR(80)) ENGINE=InnoDB; INSERT INTO items VALUES (1,'preserve-me'),(2,CONVERT(0xE4B8ADE69687 USING utf8mb4));")
        rows = self.query("SELECT id, label FROM items ORDER BY id;")["rows"]
        require(len(rows) == 2 and rows[0] == ["1", "preserve-me"], "Query result mismatch.")
        self.cli("query", "SELECT 1", "--execute", "--host", "127.0.0.1", "--port", str(self.port),
                 "--user", "fxi_ci_admin", "--password-stdin", secret="wrong-password", success=False)
        require(self.query("SELECT id, label FROM items ORDER BY id;", reader=True)["rows"] == rows, "Reader cannot read fixture.")
        require(not self.query("DELETE FROM items;", reader=True, success=False)["success"], "Read-only account unexpectedly wrote data.")
        self.passed("TCP login, escaped password, wrong-password rejection and read-only grants")
        backup = self.backup()
        dump = Path(backup["path"])
        require(dump.resolve().parent == self.root and not dump.is_symlink(), "Backup escaped the fixture.")
        require(stat.S_IMODE(dump.stat().st_mode) == 0o600 and dump.stat().st_size > 0, "Backup is empty or not private.")
        self.query("CREATE DATABASE fxi_ci_import;")
        require(dump.stat().st_size < 10 * 1024 * 1024, "Unexpectedly large fixture dump.")
        self.query(None, file=dump, database="fxi_ci_import")
        require(self.query("SELECT id, label FROM items ORDER BY id;", database="fxi_ci_import")["rows"] == rows, "SQL file round-trip mismatch.")
        self.passed("app backup permissions and SQL-file restore round-trip")
        if self.args.restore_test_binary:
            descriptor = self.root / "fixture.json"
            self.write_private(descriptor, json.dumps({"host": "127.0.0.1", "port": self.port,
                               "username": "fxi_ci_admin", "password": self.password,
                               "database": DATABASE, "dataPath": str(self.datadir)}))
            self.environment["FXI_MARIADB_FIXTURE"] = str(descriptor)
            result = self.checked([self.args.restore_test_binary, "services::mariadb::linux::integration::live_backup_restore",
                                   "--exact", "--ignored", "--nocapture", "--test-threads=1"], user=self.user, timeout=180)
            require("1 passed; 0 failed" in result, "Restore test was not compiled or did not run.")
            descriptor.unlink()
            self.passed("native immutable restore, checksum and ownership safeguards")
        else:
            self.results["limits"].append("Native owned-restore test not run; supply --restore-test-binary. SQL-file import is not the owned-restore workflow.")
        self.stop()
        self.start()
        if self.args.systemd:
            self.cli("database", "restart", "--yes", tty=True)
            require(self.cli_json("database", "status")["running"], "Restarted systemd service is not running.")
            self.passed("app real systemd start, stop and restart with isolated unit override")
        else:
            for action in [("start",), ("stop", "--yes"), ("restart", "--yes")]:
                self.cli("database", *action, success=False)
            self.results["limits"].append("No systemd PID 1: app service actions tested for safe refusal only. Daemon restart is fixture-managed.")
        require(self.query("SELECT id, label FROM items ORDER BY id;")["rows"] == rows, "Restart lost fixture data.")
        self.passed("login and fixture rows survive daemon restart")

    def preserve_uninstall(self, candidate):
        self.cli("database", "update", "--yes", tty=True, timeout=600)
        self.results["limits"].append("Update exercised against the same candidate; no cross-version package upgrade was tested.")
        self.stop()
        hashes = self.data_hashes()
        require(any(name.endswith(".ibd") for name in hashes), "No database table files to verify.")
        self.cli("database", "uninstall", "--yes", tty=True, timeout=600)
        require(not self.cli_json("database", "status")["installed"], "MariaDB server still detected after uninstall.")
        require(self.data_hashes() == hashes, "Uninstall changed or removed preserved database files.")
        out, err = self.cli("database", "install", "--version", candidate, "--yes", success=False)
        require("Existing /var/lib/mysql" in out + err, "Reinstall did not reject retained data for the expected reason.")
        require(self.data_hashes() == hashes, "Rejected reinstall changed database files.")
        self.passed("app uninstall preserves every database file; preserved-data reinstall fails closed")

    def data_hashes(self):
        result = {}
        for path in sorted(self.datadir.rglob("*")):
            require(not path.is_symlink(), "Symlink appeared in owned data directory.")
            if path.is_file():
                digest = hashlib.sha256()
                with path.open("rb") as source:
                    for chunk in iter(lambda: source.read(1024 * 1024), b""):
                        digest.update(chunk)
                result[str(path.relative_to(self.datadir))] = digest.hexdigest()
        return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--packages", action="store_true", help="Actually install and remove MariaDB in a fresh image.")
    parser.add_argument("--systemd", action="store_true", help="Test app service controls against real systemd PID 1.")
    parser.add_argument("--restore-test-binary", type=Path, help="Compiled Linux desktop lib-test executable, not cargo.")
    args = parser.parse_args()
    check_context()
    if args.systemd:
        require(Path("/proc/1/comm").read_text().strip() == "systemd", "--systemd requires real systemd PID 1 before any package changes.")
    args.binary = args.binary.resolve(strict=True)
    if args.restore_test_binary:
        args.restore_test_binary = args.restore_test_binary.resolve(strict=True)
    fixture = Fixture(args)
    try:
        candidate = fixture.catalog()
        if args.packages:
            fixture.install(candidate)
        else:
            require(fixture.cli_json("database", "status")["installed"], "Fixture mode needs distro MariaDB server/client packages preinstalled.")
            fixture.results["limits"].append("Package installation and removal not exercised in fixture mode.")
        fixture.initialize()
        fixture.exercise()
        if args.packages:
            fixture.preserve_uninstall(candidate)
        print(json.dumps(fixture.results, indent=2), flush=True)
    except Exception as error:
        fixture.failure_logs()
        print("FAIL " + fixture.redact(str(error)), file=sys.stderr, flush=True)
        raise SystemExit(1) from None
    finally:
        if fixture.daemon is not None:
            fixture.stop()
        elif fixture.systemd_configured and any_database_process():
            fixture.stop()


if __name__ == "__main__":
    main()
