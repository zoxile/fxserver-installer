#!/usr/bin/env python3
"""Real UFW rules, only in a private, offline GitHub-hosted Docker container."""
import argparse
import json
import os
from pathlib import Path, PurePosixPath
import re
import secrets
import shlex
import signal
import stat
import subprocess
import sys
import tempfile
import time

ACK = "private-network-container-v1"
CAPABILITIES = {"CHOWN": 0, "DAC_OVERRIDE": 1, "SETGID": 6, "SETUID": 7, "NET_ADMIN": 12}
USER_ID = 10001
OUTPUT_LIMIT = 4 * 1024 * 1024


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def require_ci(environment, platform):
    require(platform == "linux", "Firewall integration runs only in Linux CI.")
    require(environment.get("GITHUB_ACTIONS") == "true", "GitHub Actions is required.")
    require(environment.get("RUNNER_ENVIRONMENT") == "github-hosted", "Self-hosted runners are prohibited.")
    require(re.fullmatch(r"[1-9][0-9]*", environment.get("GITHUB_RUN_ID", "")), "Invalid GitHub run ID.")
    require(environment.get("FXI_DISPOSABLE_FIREWALL") == ACK, "Explicit disposable-firewall acknowledgement required.")


def validate_container(data, expected_mounts):
    config = data["HostConfig"]
    require(config.get("NetworkMode") == "none", "Container must use network=none, never host/container networking.")
    require(config.get("Privileged") is False, "Privileged containers are prohibited.")
    require(config.get("PidMode", "") == "" and config.get("UTSMode", "") == "", "PID and UTS namespaces must be private.")
    require(config.get("IpcMode") == "private" and config.get("CgroupnsMode") == "private", "IPC and cgroup namespaces must be private.")
    normalize = lambda names: {name.removeprefix("CAP_") for name in names or []}
    require(normalize(config.get("CapDrop")) == {"ALL"}, "Drop all default capabilities first.")
    require(normalize(config.get("CapAdd")) == set(CAPABILITIES), "Unexpected capability grant.")
    require(not config.get("SecurityOpt"), "Default seccomp/AppArmor confinement must not be overridden.")
    require(not config.get("Devices") and not config.get("DeviceRequests"), "Host devices must not be exposed.")
    require(not config.get("PortBindings") and not config.get("PublishAllPorts"), "Publishing ports is prohibited.")
    require(config.get("PidsLimit") == 128 and config.get("Memory") == 512 * 1024 * 1024,
            "Container process/memory limits are required.")
    require(data["Config"].get("User") == "0:0", "Only the guarded fixture entrypoint runs as root.")
    mounts = data.get("Mounts", [])
    require(len(mounts) == len(expected_mounts), "Unexpected volume or bind mount.")
    for mount in mounts:
        destination = mount.get("Destination")
        require(mount.get("Type") == "bind" and mount.get("RW") is False
                and destination in expected_mounts and mount.get("Source") == expected_mounts[destination],
                "Only the read-only fixture script and test executable may be mounted.")


def private_mount(mountinfo, path):
    path = PurePosixPath(path)
    parents = []
    for line in mountinfo.splitlines():
        before, after = line.split(" - ", 1)
        fields = before.split()
        mount = PurePosixPath(fields[4])
        if mount == path or mount in path.parents:
            parents.append((len(mount.parts), fields[3], after.split()[0]))
    require(parents, f"No mount information for {path}.")
    _, root, filesystem = max(parents)
    require(root == "/" and filesystem in {"overlay", "tmpfs"},
            f"{path} must be private overlay/tmpfs, never a host bind or persistent volume.")


def validate_namespaces(host_net, host_pid, current_net, current_pid, interfaces):
    require(re.fullmatch(r"net:\[[0-9]+\]", host_net or ""), "Missing host network namespace evidence.")
    require(re.fullmatch(r"pid:\[[0-9]+\]", host_pid or ""), "Missing host PID namespace evidence.")
    require(re.fullmatch(r"net:\[[0-9]+\]", current_net) and current_net != host_net,
            "Refusing to touch the host network namespace.")
    require(re.fullmatch(r"pid:\[[0-9]+\]", current_pid) and current_pid != host_pid,
            "Refusing a shared host PID namespace.")
    require(set(interfaces) == {"lo"}, "Offline container must have only loopback, no host network interfaces.")


def guard_inside():
    require_ci(os.environ, sys.platform)
    require(os.geteuid() == 0, "Only the guarded container fixture must run as root.")
    require(Path("/.dockerenv").is_file(), "A disposable Docker container is required.")
    require(not Path("/var/run/docker.sock").exists() and not Path("/run/podman/podman.sock").exists(),
            "Container runtime sockets must not be mounted.")
    validate_namespaces(os.environ.get("FXI_HOST_NETNS"), os.environ.get("FXI_HOST_PIDNS"),
                        os.readlink("/proc/self/ns/net"), os.readlink("/proc/self/ns/pid"),
                        [p.name for p in Path("/sys/class/net").iterdir()])
    status = dict(line.split(":", 1) for line in Path("/proc/self/status").read_text().splitlines() if ":" in line)
    allowed = sum(1 << bit for bit in CAPABILITIES.values())
    require(int(status["CapBnd"], 16) == allowed and int(status["CapEff"], 16) == allowed,
            "Unexpected capability boundary; SYS_ADMIN/privileged containers are never allowed.")
    mounts = Path("/proc/self/mountinfo").read_text()
    for value in ["/etc/ufw", "/etc/default/ufw", "/run", "/tmp", "/home/fxi-fw-ci"]:
        require(Path(value).resolve() == Path(value), f"Symlinked fixture path: {value}")
        private_mount(mounts, value)


def run(command, *, environment=None, user=None, timeout=120, input_bytes=None, check=True):
    """Bound subprocess lifetime and output; stdin is closed unless feeding a Dockerfile."""
    require(input_bytes is None or len(input_bytes) <= 8192, "Fixture input is limited to 8 KiB.")
    options = {"env": environment, "start_new_session": True}
    if user is not None:
        options.update(user=user, group=user, extra_groups=[])
    with tempfile.TemporaryFile() as out, tempfile.TemporaryFile() as err:
        child = subprocess.Popen([str(item) for item in command], stdout=out, stderr=err,
                                 stdin=subprocess.PIPE if input_bytes is not None else subprocess.DEVNULL, **options)
        deadline = time.monotonic() + timeout
        try:
            if input_bytes is not None:
                child.stdin.write(input_bytes)
                child.stdin.close()
            while child.poll() is None:
                require(time.monotonic() < deadline, "Fixture command timed out; discard the container.")
                require(os.fstat(out.fileno()).st_size + os.fstat(err.fileno()).st_size <= OUTPUT_LIMIT,
                        "Fixture command output limit exceeded.")
                time.sleep(0.03)
            require(os.fstat(out.fileno()).st_size + os.fstat(err.fileno()).st_size <= OUTPUT_LIMIT,
                    "Fixture command output limit exceeded.")
            out.seek(0); err.seek(0)
            result = subprocess.CompletedProcess(command, child.returncode,
                out.read(OUTPUT_LIMIT).decode("utf-8", "replace"), err.read(OUTPUT_LIMIT).decode("utf-8", "replace"))
            if check:
                require(result.returncode == 0, f"{Path(command[0]).name} failed: {json.dumps((result.stdout + result.stderr)[-6000:])}")
            return result
        finally:
            if child.returncode is None:
                os.killpg(child.pid, signal.SIGKILL)
                child.wait(timeout=5)


def launch(binary):
    require_ci(os.environ, sys.platform)
    require(not Path("/.dockerenv").exists(), "Run the launcher on the GitHub-hosted VM, not inside another container.")
    binary = Path(binary).resolve(strict=True)
    require(binary.is_file(), "fxins binary is required.")
    script = Path(__file__).resolve()
    suffix = f"{os.environ['GITHUB_RUN_ID']}-{secrets.token_hex(6)}"
    image = f"fxi-firewall-ufw:{suffix}"
    name = f"fxi-firewall-{suffix}"
    run(["docker", "build", "--tag", image, "-"], timeout=900,
        input_bytes=script.with_name("firewall.Dockerfile").read_bytes())
    environment = {key: os.environ[key] for key in ["GITHUB_ACTIONS", "RUNNER_ENVIRONMENT", "GITHUB_RUN_ID", "FXI_DISPOSABLE_FIREWALL"]}
    environment.update(FXI_HOST_NETNS=os.readlink("/proc/self/ns/net"), FXI_HOST_PIDNS=os.readlink("/proc/self/ns/pid"))
    mounts = {"/opt/fxins": str(binary), "/opt/firewall-test.py": str(script)}
    command = ["docker", "create", "--name", name, "--init", "--user", "0:0", "--network", "none",
               "--ipc", "private", "--cgroupns", "private", "--cap-drop", "ALL", "--pids-limit", "128",
               "--memory", "512m", "--cpus", "2"]
    for capability in CAPABILITIES:
        command += ["--cap-add", capability]
    for key, value in environment.items():
        command += ["--env", f"{key}={value}"]
    for destination, source in mounts.items():
        require("," not in source, "Mount paths may not contain commas.")
        command += ["--mount", f"type=bind,source={source},target={destination},readonly"]
    command.append(image)
    run(command)
    try:
        inspect = json.loads(run(["docker", "inspect", name]).stdout)
        require(len(inspect) == 1, "Expected one disposable container.")
        validate_container(inspect[0], mounts)
        print("Verified private offline network/PID namespaces, constrained capabilities, read-only fixture mounts.", flush=True)
        result = run(["docker", "start", "--attach", name], timeout=420, check=False)
        print(result.stdout, end="", flush=True)
        if result.stderr:
            print(result.stderr, file=sys.stderr, end="", flush=True)
        state = json.loads(run(["docker", "inspect", name]).stdout)[0]["State"]
        require(result.returncode == 0 and state.get("ExitCode") == 0 and not state.get("OOMKilled"),
                "Disposable UFW integration failed.")
    finally:
        run(["docker", "rm", "--force", name], timeout=30, check=False)


def rule_lines(text):
    return sorted(line.strip() for line in text.splitlines() if line.strip().startswith("ufw "))


def assert_kernel_ports(text, tcp, udp):
    for direction, chain, option in [("inbound", "ufw-user-input", "--dport"), ("outbound", "ufw-user-output", "--sport")]:
        for protocol, port in [("tcp", tcp), ("udp", udp)]:
            found = False
            for line in text.splitlines():
                tokens = shlex.split(line)
                if tokens[:2] != ["-A", chain]:
                    continue
                pairs = {tokens[i]: tokens[i + 1] for i in range(len(tokens) - 1) if tokens[i].startswith("-")}
                found |= pairs.get("-p") == protocol and pairs.get(option) == str(port) and pairs.get("-j") == "ACCEPT"
            require(found, f"No real {direction} {protocol} local port {port} accept rule found.")


def inside():
    guard_inside()
    import pwd
    user = pwd.getpwnam("fxi-fw-ci")
    require(user.pw_uid == USER_ID and user.pw_gid == USER_ID, "Unexpected fixture account.")
    executable = Path("/opt/fxins")
    metadata = executable.stat()
    require(stat.S_ISREG(metadata.st_mode) and metadata.st_mode & 0o111 and not metadata.st_mode & 0o6000,
            "Use the real normal-user fxins executable, never a setuid app.")
    environment = {"PATH": "/usr/sbin:/usr/bin:/sbin:/bin", "LC_ALL": "C", "HOME": user.pw_dir,
                   "XDG_DATA_HOME": f"{user.pw_dir}/app-data", "TERM": "dumb"}
    config = Path(user.pw_dir) / "fixture.json"
    with config.open("x", encoding="utf-8") as target:
        os.fchmod(target.fileno(), 0o600)
        os.fchown(target.fileno(), USER_ID, USER_ID)
        target.write("{}")

    def system(*args):
        return run(args, environment=environment, timeout=30).stdout

    def cli(action, tcp=30120, udp=30121, *, elevated=False, yes=False, check=True):
        command = [executable, "--config", config, "firewall", action, "--backend", "ufw", "--tcp-port", str(tcp), "--udp-port", str(udp)]
        if elevated: command.append("--elevated")
        if yes: command.append("--yes")
        response = run(command, environment=environment, user=USER_ID, timeout=150, check=check)
        return json.loads(response.stdout) if check else response

    def states(snapshot):
        return {(row["direction"], row["protocol"]): row["state"] for row in snapshot["checks"]}

    require(system("/usr/bin/id", "-u").strip() == "0", "Harness identity changed.")
    require(run(["/usr/bin/id", "-u"], environment=environment, user=USER_ID).stdout.strip() == str(USER_ID),
            "The app must run as an ordinary user.")
    denied = run(["/usr/bin/sudo", "-n", "--", "/bin/true"], environment=environment, user=USER_ID, check=False)
    require(denied.returncode != 0, "Fixture sudoers must allow only the typed app helper.")
    require("Status: inactive" in system("/usr/sbin/ufw", "status"), "Fresh container unexpectedly has an active firewall.")
    require(not rule_lines(system("/usr/sbin/ufw", "show", "added")), "Refusing a container with existing user rules.")
    # UFW startup must not attempt writes under Docker's read-only /proc/sys mount.
    Path("/etc/ufw/sysctl.conf").write_text("# Disposable network namespace: no sysctl changes.\n")
    system("/usr/sbin/ufw", "--force", "enable")
    system("/usr/sbin/ufw", "allow", "in", "proto", "tcp", "from", "any", "to", "any", "port", "30120", "comment", "fxi-ci-external")
    system("/usr/sbin/ufw", "allow", "in", "proto", "udp", "from", "any", "to", "any", "port", "30200", "comment", "fxi-ci-unrelated")
    baseline = rule_lines(system("/usr/sbin/ufw", "show", "added"))
    require(cli("check")["requiresElevation"], "Normal inspection must not secretly elevate.")
    before = cli("check", elevated=True)
    require(states(before)[("inbound", "tcp")] == "external", "External rule was not recognized.")
    require(all(states(before)[("outbound", protocol)] == "defaultAllowed" for protocol in ["tcp", "udp"]),
            "Default outbound allowance must be distinguished from an owned rule.")
    configured = cli("setup", yes=True)
    require(sum(row["ownedRulePresent"] for row in configured["checks"]) == 1, "Setup replaced an external/default permission.")
    installed = rule_lines(system("/usr/sbin/ufw", "show", "added"))
    cli("setup", yes=True)
    require(rule_lines(system("/usr/sbin/ufw", "show", "added")) == installed, "Setup is not idempotent.")
    cli("remove", yes=True)
    require(rule_lines(system("/usr/sbin/ufw", "show", "added")) == baseline, "Removal changed external rules.")

    system("/usr/sbin/ufw", "default", "deny", "outgoing")
    configured = cli("setup", 30142, 30143, yes=True)
    require(all(row["state"] == "owned" and row["ownedRulePresent"] for row in configured["checks"]),
            "All four explicit directions/protocols must be owned when defaults deny traffic.")
    for tool in ["/usr/sbin/iptables-save", "/usr/sbin/ip6tables-save"]:
        assert_kernel_ports(system(tool), 30142, 30143)
    system("/usr/sbin/ufw", "allow", "in", "proto", "tcp", "from", "any", "to", "any", "port", "30142", "comment", "fxi-ci-modified")
    cli("remove", 30142, 30143, yes=True)
    remaining = rule_lines(system("/usr/sbin/ufw", "show", "added"))
    require(all(line in remaining for line in baseline) and any("fxi-ci-modified" in line for line in remaining),
            "Removal changed an external or modified rule.")
    require(not any("fxins-fw-v1-" in line for line in remaining), "Owned rules were not removed.")
    cli("remove", 30142, 30143, yes=True)
    require(rule_lines(system("/usr/sbin/ufw", "show", "added")) == remaining, "Removal is not idempotent.")
    system("/usr/sbin/ufw", "deny", "in", "proto", "udp", "from", "any", "to", "any", "port", "30143", "comment", "fxi-ci-block")
    blocked = cli("check", 30142, 30143, elevated=True)
    require(states(blocked)[("inbound", "udp")] == "blocked" and not blocked["canConfigure"], "An existing block was ignored.")
    previous = rule_lines(system("/usr/sbin/ufw", "show", "added"))
    require(cli("setup", 30142, 30143, yes=True, check=False).returncode != 0, "Setup must reject a blocker.")
    require(rule_lines(system("/usr/sbin/ufw", "show", "added")) == previous, "Rejected setup still changed rules.")
    print(json.dumps({"backend": "ufw", "normalUser": USER_ID, "network": "private-offline",
        "checks": ["explicit approval", "external/default distinction", "IPv4 and IPv6 local TCP/UDP ports",
                   "source-port output rules", "idempotence", "narrow removal", "modified marker preserved", "blocker refusal"]}), flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default="src-tauri/target/debug/fxins")
    parser.add_argument("--inside", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    try:
        inside() if args.inside else launch(args.binary)
    except Exception as error:
        print("Firewall fixture failed: " + json.dumps(str(error)), file=sys.stderr)
        sys.exit(1)
