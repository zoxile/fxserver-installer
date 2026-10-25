import copy
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("firewall_ci", ROOT / "scripts/ci/firewall_integration.py")
fixture = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(fixture)


class FirewallIntegrationGuards(unittest.TestCase):
    def setUp(self):
        self.environment = {
            "GITHUB_ACTIONS": "true",
            "RUNNER_ENVIRONMENT": "github-hosted",
            "GITHUB_RUN_ID": "123",
            "FXI_DISPOSABLE_FIREWALL": fixture.ACK,
        }
        self.mounts = {"/opt/fxins": "/checkout/fxins", "/opt/firewall-test.py": "/checkout/test.py"}
        self.container = {
            "HostConfig": {
                "NetworkMode": "none", "Privileged": False,
                "PidMode": "", "UTSMode": "", "IpcMode": "private", "CgroupnsMode": "private",
                "CapDrop": ["ALL"], "CapAdd": list(fixture.CAPABILITIES),
                "SecurityOpt": None, "Devices": [], "DeviceRequests": None,
                "PortBindings": {}, "PublishAllPorts": False,
                "PidsLimit": 128, "Memory": 512 * 1024 * 1024,
            },
            "Config": {"User": "0:0"},
            "Mounts": [{"Type": "bind", "RW": False, "Destination": dest, "Source": source}
                       for dest, source in self.mounts.items()],
        }

    def test_requires_every_opt_in_on_linux(self):
        fixture.require_ci(self.environment, "linux")
        for key in self.environment:
            with self.subTest(missing=key):
                values = self.environment.copy()
                del values[key]
                with self.assertRaises(RuntimeError):
                    fixture.require_ci(values, "linux")
        for key, value in [("RUNNER_ENVIRONMENT", "self-hosted"), ("GITHUB_RUN_ID", "0"),
                           ("GITHUB_RUN_ID", "1;cmd"), ("GITHUB_ACTIONS", "false")]:
            with self.subTest(key=key, value=value), self.assertRaises(RuntimeError):
                fixture.require_ci({**self.environment, key: value}, "linux")
        for platform in ["win32", "darwin"]:
            with self.assertRaises(RuntimeError):
                fixture.require_ci(self.environment, platform)

    def test_launcher_rejects_before_any_system_call(self):
        with patch.dict(fixture.os.environ, {}, clear=True), patch.object(fixture, "run") as run:
            with self.assertRaises(RuntimeError):
                fixture.launch("does-not-exist")
            run.assert_not_called()

    def test_inside_rejects_before_any_system_call(self):
        with patch.dict(fixture.os.environ, {}, clear=True), patch.object(fixture, "run") as run:
            with self.assertRaises(RuntimeError):
                fixture.inside()
            run.assert_not_called()

    def test_only_expected_container_is_accepted(self):
        fixture.validate_container(self.container, self.mounts)
        self.container["HostConfig"]["CapAdd"] = ["CAP_" + name for name in fixture.CAPABILITIES]
        fixture.validate_container(self.container, self.mounts)

    def test_dangerous_docker_options_fail_closed(self):
        variants = {
            "NetworkMode": ["host", "bridge", "container:other"],
            "Privileged": [True, None], "PidMode": ["host"], "UTSMode": ["host"],
            "IpcMode": ["host", "shareable"], "CgroupnsMode": ["host"],
            "CapDrop": [[], ["NET_RAW"]], "CapAdd": [list(fixture.CAPABILITIES) + ["SYS_ADMIN"], ["ALL"]],
            "SecurityOpt": [["apparmor=unconfined"], ["seccomp=unconfined"]],
            "Devices": [[{"PathOnHost": "/dev/sda"}]], "DeviceRequests": [[{"Count": -1}]],
            "PortBindings": [{"30120/tcp": [{"HostPort": "30120"}]}], "PublishAllPorts": [True],
            "PidsLimit": [0, -1, 1024], "Memory": [0],
        }
        for field, values in variants.items():
            for value in values:
                with self.subTest(field=field, value=value):
                    data = copy.deepcopy(self.container)
                    data["HostConfig"][field] = value
                    with self.assertRaises(RuntimeError):
                        fixture.validate_container(data, self.mounts)

    def test_writable_host_mounts_and_extra_mounts_fail_closed(self):
        for field, value in [("RW", True), ("Type", "volume"), ("Source", "/etc/ufw"),
                             ("Destination", "/var/run/docker.sock")]:
            data = copy.deepcopy(self.container)
            data["Mounts"][0][field] = value
            with self.subTest(field=field), self.assertRaises(RuntimeError):
                fixture.validate_container(data, self.mounts)
        data = copy.deepcopy(self.container)
        data["Mounts"].append({"Destination": "/etc/ufw", "Source": "/etc/ufw", "RW": True, "Type": "bind"})
        with self.assertRaises(RuntimeError):
            fixture.validate_container(data, self.mounts)

    def test_namespace_proof_must_differ_from_host(self):
        fixture.validate_namespaces("net:[1]", "pid:[2]", "net:[3]", "pid:[4]", ["lo"])
        for args in [
            ("net:[1]", "pid:[2]", "net:[1]", "pid:[4]", ["lo"]),
            ("net:[1]", "pid:[2]", "net:[3]", "pid:[2]", ["lo"]),
            (None, "pid:[2]", "net:[3]", "pid:[4]", ["lo"]),
            ("net:[1]", "pid:[2]", "net:[3]", "pid:[4]", ["lo", "eth0"]),
        ]:
            with self.subTest(args=args), self.assertRaises(RuntimeError):
                fixture.validate_namespaces(*args)

    def test_only_private_overlay_or_tmpfs_is_writable(self):
        mounts = "10 1 0:1 / / rw - overlay overlay rw\n11 10 0:2 / /tmp rw - tmpfs tmpfs rw"
        for path in ["/etc/ufw", "/etc/default/ufw", "/tmp", "/home/fxi-fw-ci"]:
            fixture.private_mount(mounts, path)
        for extra in [
            "12 10 8:1 /host-ufw /etc/ufw rw - ext4 /dev/sda rw",
            "12 10 8:1 / /etc rw - ext4 /dev/sda rw",
            "12 10 0:2 /other /etc/ufw rw - overlay overlay rw",
        ]:
            with self.subTest(extra=extra), self.assertRaises(RuntimeError):
                fixture.private_mount(mounts + "\n" + extra, "/etc/ufw")

    def test_kernel_assertion_requires_source_port_for_outbound(self):
        rules = "\n".join(
            f"-A {chain} -p {protocol} -m {protocol} {option} {port} -j ACCEPT"
            for chain, option in [("ufw-user-input", "--dport"), ("ufw-user-output", "--sport")]
            for protocol, port in [("tcp", 30142), ("udp", 30143)]
        )
        fixture.assert_kernel_ports(rules, 30142, 30143, family=4)
        with self.assertRaises(RuntimeError):
            fixture.assert_kernel_ports(rules.replace("--sport", "--dport"), 30142, 30143, family=4)
        with self.assertRaises(RuntimeError):
            fixture.assert_kernel_ports(rules.replace("ACCEPT", "DROP"), 30142, 30143, family=4)

    def test_kernel_assertion_checks_ipv6_chains_separately(self):
        rules = "\n".join(
            f"-A {chain} -p {protocol} -m {protocol} {option} {port} -j ACCEPT"
            for chain, option in [("ufw6-user-input", "--dport"), ("ufw6-user-output", "--sport")]
            for protocol, port in [("tcp", 30142), ("udp", 30143)]
        )
        fixture.assert_kernel_ports(rules, 30142, 30143, family=6)
        with self.assertRaisesRegex(RuntimeError, "IPv4 inbound tcp"):
            fixture.assert_kernel_ports(rules, 30142, 30143, family=4)
        with self.assertRaisesRegex(RuntimeError, "IPv6 inbound tcp"):
            fixture.assert_kernel_ports(rules.replace("ufw6-", "ufw-"), 30142, 30143, family=6)
        for wrong in [rules.replace("--sport", "--dport"), rules.replace("ACCEPT", "DROP")]:
            with self.assertRaises(RuntimeError):
                fixture.assert_kernel_ports(wrong, 30142, 30143, family=6)

    def test_workflow_runs_explicit_fixture_and_restricts_sudo(self):
        workflow = (ROOT / ".github/workflows/linux-checks.yml").read_text()
        self.assertIn("FXI_DISPOSABLE_FIREWALL: " + fixture.ACK, workflow)
        self.assertIn("python3 scripts/ci/firewall_integration.py --binary src-tauri/target/debug/fxins", workflow)
        dockerfile = (ROOT / "scripts/ci/firewall.Dockerfile").read_text()
        self.assertIn("NOPASSWD: /opt/fxins --fxins-firewall-helper *", dockerfile)
        self.assertNotIn("NOPASSWD: ALL", dockerfile)
        source = (ROOT / "scripts/ci/firewall_integration.py").read_text()
        self.assertNotIn("shell=True", source)
        self.assertIn("stdin=subprocess.PIPE if input_bytes is not None else subprocess.DEVNULL", source)
        self.assertIn("user=USER_ID, timeout=150", source)


if __name__ == "__main__":
    unittest.main()
