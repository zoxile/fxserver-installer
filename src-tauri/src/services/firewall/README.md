# Firewall backend integration

The public DTOs are in `../firewall.rs`; Tauri wrappers are in
`commands/firewall.rs`. No shared runtime service is required. This uses the
development branch's existing serde, serde_json, log, getrandom, windows-sys and
(on Linux) libc dependencies. The Windows-only main branch must add getrandom 0.3.

## Interface

- `FirewallRequest { backend, tcpPort, udpPort }`: backend is `auto`, `windows`,
  `ufw`, or `firewalld`; ports are nonzero `u16` values. No paths or scripts.
- `inspect_fivem_firewall(request, elevated)`: `false` never prompts; on Linux,
  and on Windows when bulk filter enumeration is denied, it reports
  `requiresElevation` until the user explicitly requests a privileged read.
  `true` asks for one action-scoped approval.
- `configure_fivem_firewall(request)` and `remove_fivem_firewall(request)`:
  inspect again inside the privileged helper, apply only eligible rules, and
  return a fresh snapshot. An error may follow partial changes; inspect before
  retrying. These operations must not be automatically retried.
- `run_firewall_helper_from_args()`: call before normal startup/single-instance
  handling in the desktop executable and both terminal aliases.

Snapshots contain four checks: inbound/outbound TCP/UDP. `state` distinguishes
`owned`, `external`, `defaultAllowed`, `missing`, `blocked`, and `unknown`.
`ownedRulePresent` independently controls removal, including when a separate
block makes the effective check blocked. `canConfigure`, `canRemove`,
`reloadRequired`, and notices should all be respected by callers. A successful
command response is not a claim of Internet reachability.

## Privilege and ownership boundaries

Windows uses ShellExecuteEx `runas` on the app executable, not PowerShell.
Only fixed operations/backends, numeric ports, and an IPC nonce are accepted.
The helper runs an embedded fixed PowerShell program using the system binary
with a sanitized environment. Rules require an exact name, group, description,
port, protocol, direction, scope, and security settings before removal.
Filter collections are read once per store and joined by exact InstanceID/name
and expected CIM class. Missing or duplicate joins fail closed. An unelevated
permission failure returns promptly instead of falling back to slow per-rule
association reads.

Linux launches the app with `sudo -n --` or desktop
`pkexec --disable-internal-agent`. Stdin is always closed, even outside the TUI.
On headless systems, users authenticate with `sudo -v` outside the TUI first.
The main application must remain a normal-user process. IPC uses an abstract
Unix socket with kernel-verified root peer credentials; visible process arguments
are not treated as an authentication secret. Neither platform accepts script or
output-file paths from the caller.

Inspections and individual Linux commands have 20-second limits. A Windows
mutation batch has 60 seconds for up to four writes plus final verification,
with one ownership preflight for the complete selection. All helper work remains
bounded by a 120-second deadline and 1-MiB output limits. The parent waits at most
180 seconds after launching the helper. OS
approval UI can itself remain open awaiting the user. Denying approval makes no
changes. There is no mid-transaction cancellation API: do not show an app Cancel
button after applying starts. A disconnected requester can leave a bounded
in-flight action completing; interruption must be followed by a fresh inspection.
Windows job objects and Linux process groups clean up command descendants on
timeout. The backend never enables, disables, resets, or flushes a firewall.

UFW uses exact generated comments and a parsed matching rule shape. Equivalent
external rules are retained, including duplicates that make ownership ambiguous.
Outbound permissions use the server's local/source port, not the destination port.

Firewalld uses narrowly scoped policies with an exact description and full shape
check. Existing direct/rich rules, explicit blockers, panic mode, unknown formats,
and modified reserved policies fail closed. New policies are saved permanently
and report `reloadRequired`; the app never reloads globally or saves unrelated
runtime changes. After administrator-reviewed activation, existing owned policies
are updated in both runtime and permanent state. Removing access leaves empty
owned policy containers, not permissions. Custom nftables/iptables backends are
unsupported. Firewall managers modified concurrently by another administrator are
not a supported atomic transaction boundary.

## Validation

Fixture/parser/planner tests do not invoke firewall tools:

```text
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features terminal services::firewall --lib
```

For the Windows-only main branch, omit `--no-default-features --features terminal`.

Linux additionally runs ephemeral Unix-socket credential/framing tests, without
sudo or firewall changes. The Windows script test parses the production script
and executes only its pure ownership predicate, with JSON fixtures:

```powershell
powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File src-tauri/src/services/firewall/windows-fixtures.tests.ps1
```

Opt-in Windows schema probe, read-only and never elevated:

```text
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features terminal readonly_windows_firewall_schema_probe --lib -- --ignored
```

Real apply/remove testing belongs in disposable Windows/Linux VMs. This backend
has not been mutation-tested against the workstation firewall.

The ignored `disposable_windows_firewall_roundtrip` test requires
`GITHUB_ACTIONS=true`, `RUNNER_ENVIRONMENT=github-hosted`, `RUNNER_OS=Windows`, a
positive `GITHUB_RUN_ID`, and
`FXI_DISPOSABLE_WINDOWS_FIREWALL=github-hosted-disposable-v1`. It also requires
an already elevated token and never prompts for UAC. Run only that named ignored
test on a fresh GitHub-hosted Windows job. It creates high-port fixture rules,
exercises all four rule shapes, and verifies that external rules, unrelated
app-owned ports, and a deliberately modified rule survive normal app removal.
Cleanup is limited to the generated fixture names with matching markers. No
firewall profile, default policy, or unrelated rule is reset or disabled.

## Official references

- [Windows firewall rule parameters](https://learn.microsoft.com/en-us/powershell/module/netsecurity/new-netfirewallrule)
- [Windows application elevation](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shellexecuteexw)
- [UFW command syntax and defaults](https://manpages.ubuntu.com/manpages/noble/man8/ufw.8.html)
- [Firewalld policies and HOST/ANY direction](https://firewalld.org/documentation/man-pages/firewalld.policies)
- [Firewalld runtime/permanent policy commands](https://firewalld.org/documentation/man-pages/firewall-cmd.html)
