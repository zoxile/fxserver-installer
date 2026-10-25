# Disposable firewall integration

`firewall_integration.py` runs only on a GitHub-hosted Linux runner with
`FXI_DISPOSABLE_FIREWALL=private-network-container-v1`. It builds a fresh Ubuntu
image without a source context, then creates and inspects the container before
starting it. This is not a workstation test.

The container has no network interfaces except loopback, no published ports, no
host networking/PID/IPC/cgroup namespace, no runtime socket, no host devices, and
only two read-only mounts: the executable and fixture script. Root filesystem
changes are disposable. Resource limits bound processes, memory, and runtime.
All default capabilities are dropped. `NET_ADMIN` applies only to the separate
network namespace; `CHOWN`, `DAC_OVERRIDE`, `SETUID`, and `SETGID` let the harness
prepare private files and run the actual app as UID 10001. Neither `SYS_ADMIN`
nor `--privileged` is allowed. Default seccomp/AppArmor confinement remains on.

Before touching UFW, the inner fixture checks its network and PID namespaces
against the launcher's namespace IDs, confirms only loopback exists, verifies
its exact capability boundary, and checks private mounts for all writable paths.
The real `fxins` commands run as the ordinary user with closed stdin. The only
sudoers permission is the executable's typed firewall helper; arbitrary sudo
commands are rejected. The app itself uses `sudo -n`.

The roundtrip checks:

- Read-only inspection does not automatically elevate.
- External allows and default outbound allowance remain distinct from ownership.
- Missing rules use the requested TCP/UDP destination port inbound and local
  source port outbound, verified with real IPv4 and IPv6 kernel rules.
- Repeated setup/removal is idempotent.
- External, unrelated, and modified-marker rules survive app removal.
- Explicit blocks prevent setup without changing rules.

The fixture alone enables UFW and changes its default in this isolated namespace
to exercise the cases above. The application never enables/disables the firewall
or changes default policies. Cleanup discards only the named test container; it
does not restore, flush, or modify the runner's firewall.

Local validation is non-mutating:

```text
python -m unittest discover -s scripts/tests -p test_firewall_integration.py -v
```

Live firewalld policy operations are not covered by this container. Firewalld has
parser/planner fixtures, but still requires a dedicated disposable systemd test
environment for live validation. Do not treat passing UFW tests as firewalld
certification.

The Windows-only guarded roundtrip and read-only probe are documented in
`src-tauri/src/services/firewall/README.md`. They are separate from this Linux
container and never run automatically on a workstation.
