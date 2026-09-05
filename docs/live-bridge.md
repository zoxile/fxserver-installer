# Live Bridge

Live Bridge is an optional, local companion resource for FXServer Installer. It is off by default and is not required for artifact installation, configuration editing, resource updates, or the existing RCON workflow. Open **FXServer > Live Bridge** to inspect, install, pair, and operate it.

> [!WARNING]
> Live production validation has not been performed. Start with a disposable local FXServer workspace. Installing/removing the bridge changes resource files and `server.cfg`; resource controls can affect connected players. Keep independent backups and review every change.

## What It Provides

| Data | Meaning and limits |
| --- | --- |
| Resources | Actual `GetResourceState` results, resource names, and manifest versions. Missing version metadata stays empty. State is not inferred from an accepted command. |
| Players | Current transient server IDs, display names, and ping in milliseconds. IDs can change or be reused; they are not permanent account identifiers. |
| Server information | Hostname, configured game build, OneSync setting, maximum player setting, player count, and resource count. These are a snapshot, not a complete server configuration. |
| Bridge uptime | Time since this bridge resource started. Restarting the resource resets it; it is not necessarily FXServer process uptime. |
| Scheduler delay | Lateness of the bridge's one-second JavaScript timer. This is not actual per-resource CPU, a profiler sample, or attribution of a stall to a particular resource. |
| Resource events | Recent resource start/stop events and accepted bridge resource commands. The bridge retains the latest 100 in memory; its page displays the latest 50. Restarting it resets this buffer. |

The desktop backend polls roughly every three seconds while connection is enabled, including when the app is in the tray. The resource caches snapshots for up to one second. Lists are bounded to 5,000 resources and 512 players; counts can exceed the returned lists. Names/metadata are also bounded. This is a sampled view, not a continuous telemetry archive.

Only a connected snapshot for the active workspace supplies Resource Manager's state badges. Disconnecting clears the snapshot and badges rather than preserving guessed or stale states. An absent badge does not mean a resource is stopped. Bridge resource events can also appear in **Logs > Incident Timeline**, whose separate local history is bounded and is not a complete audit log.

Player IP addresses, license identifiers, authentication tokens, and arbitrary convars are not collected by this bridge. Player names, IDs, and server names can still be personal or identifying data; check screenshots and support reports before sharing them. Use the existing Profiler viewer and appropriate profiler exports for resource-level performance investigation.

## Install And Pair

The desktop app and FXServer must run on the same Windows machine. First configure the active workspace's txData folder/profile in **Configure Server**. The installer resolves the profile's `server.dataPath`; it needs an existing readable `server.cfg` and `resources` directory.

1. Stop the managed FXServer. Also stop any FXServer/txAdmin instance launched outside this app and prevent it from restarting during maintenance.
2. Open **FXServer > Live Bridge**, inspect the resolved installation path, and enter the server's **FXServer HTTP port**, normally `30120`. This is not the txAdmin web port (commonly `40120`) or the MariaDB port.
3. Choose **Install bridge**. Review the file list and exact marked configuration block, then choose **Confirm installation**. The preview expires after ten minutes.
4. Installation creates a new pairing token and enables the workspace's automatic connection preference. Start FXServer through the normal workflow so `server.cfg` starts the bridge resource.
5. Verify **Connected**, the expected server information, and real resource/player data. If the HTTP port changes, update it and choose **Apply connection**.

No manual token entry, extra HTTP listener, firewall exception, public bind address, or remote-host setting is required. The bridge uses FXServer's existing HTTP resource handler. Only the port is configurable; the desktop client always requests `127.0.0.1`.

Connection preferences are per workspace. To stop polling, clear **Connect automatically** and choose **Apply connection**. This disconnects the app but does not remove or stop the installed resource or remove its config block. Re-enable and apply the preference to reconnect. Quitting the app ends its polling; an opted-in workspace can reconnect when the app opens again.

## Files And Configuration

Installation creates this directory under the resolved server data folder:

```text
resources/fxserver_installer_bridge/
  fxmanifest.lua
  server.js
  bridge-token.txt
  .fxsi-bridge.json
```

The manifest declares `server_only 'yes'` and one server script. There are no client/shared scripts and no manifest `files` entry exposing the token. The token is read locally by the server script. See the official [Cfx resource manifest reference](https://docs.fivem.net/docs/scripting-reference/resource-manifest/#server_only) for server-only resource semantics.

The app adds this exact owned block to `server.cfg`, preserving unrelated settings:

```cfg
# BEGIN FXSERVER INSTALLER LIVE BRIDGE
add_ace resource.fxserver_installer_bridge command.start allow
add_ace resource.fxserver_installer_bridge command.stop allow
add_ace resource.fxserver_installer_bridge command.restart allow
add_ace resource.fxserver_installer_bridge command.ensure allow
ensure fxserver_installer_bridge
# END FXSERVER INSTALLER LIVE BRIDGE
```

The token is not written into this block, an ACE, or a replicated convar. The resource receives only these four command permissions, not an unrestricted console grant. Do not broaden the ACE block, add the token to shared/client files, or duplicate the bridge's `ensure` entry elsewhere.

The owner manifest records installed-file hashes and a pairing-key reference. The app's token copy is encrypted with Windows DPAPI in its local application data under `live-bridge/<key-id>.dpapi`. Connection preferences contain no token. Configuration changes use the app's encrypted configuration history; the live configuration and server token file themselves are not encrypted by that history.

## Security Model

There is one supported authentication mode: loopback plus a Bearer token. There is no unauthenticated fallback or remote-control mode.

- The handler checks the request's actual `address` for accepted loopback forms: `127.0.0.1`, `::1`, or IPv4-mapped `::ffff:127.0.0.1`, including supported port notation. It does not trust forwarded-address headers.
- The desktop client uses fixed local HTTP URLs, an `Authorization: Bearer ...` header, no system proxy, no redirects, bounded response size, and short timeouts. The token is never sent in a URL.
- A random 32-byte token is encoded as 64 hexadecimal characters. The resource requires an exact Bearer header and uses a timing-safe comparison. Missing, invalid, or non-loopback authentication is refused.
- Only `GET /snapshot` and `POST /resource` are implemented beneath the resource endpoint. Resource requests are bounded to 1 KiB, have a body timeout, and accept only `start`, `stop`, `restart`, or `ensure` with a restricted resource-name format. The bridge cannot control itself through this endpoint.
- Requests are rate-limited, snapshots are cached, and event/list sizes are bounded. These are defensive limits, not a claim of resistance to every denial-of-service scenario.

The resource handler is hosted by FXServer's existing HTTP service, which may also be reachable for normal server traffic. It is **not a separate socket bound exclusively to loopback**. The security boundary is the handler's source-address check plus token authentication. Do not expose its resource route through a reverse proxy, tunnel, port-forwarding helper, or remote agent that turns external requests into local ones. No bridge-specific inbound firewall opening should be added.

The server needs to read `bridge-token.txt`, so that file is plaintext on disk. Protect the server directory with appropriate Windows filesystem permissions and keep it out of public packages, client downloads, logs, and support bundles. A process or server resource able to read the file can obtain the token; a compromised local machine or server is outside this protection boundary.

The app's DPAPI copy is protected for the Windows user context, not a portable shared secret vault. See [Microsoft's DPAPI documentation](https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptprotectdata). Moving to another Windows account/machine or losing app pairing data can require re-pairing. Do not try to recover pairing by publishing the token or weakening authentication.

The bridge offers no arbitrary console execution, SQL, file-browsing, kick/ban, public web dashboard, or per-resource CPU endpoint. Existing app RCON features are separate and retain their own configuration and trust requirements.

## Resource Controls

Use the buttons beside a resource in **Live Bridge** to start, stop, restart, or ensure it. Stop and Restart require confirmation because connected players may be affected. The bridge validates the action and resource name again on the server; missing resources and attempts to control the bridge itself are refused.

An accepted response means a command was submitted, not that startup succeeded or dependencies are healthy. Wait for a fresh snapshot and inspect server logs. If a timeout leaves the result uncertain, inspect actual state before issuing another action. These controls do not replace resource files, approve update previews, apply SQL migrations, or automatically coordinate dependency restarts.

Resource Manager's existing buttons still use RCON. Its optional bridge badges are a state source, not a change to the RCON transport and not evidence that a resource is safe to update while running.

## Reinstall And Remove

Installation, reinstallation, and removal run under the managed server's lifecycle lock and stopped-state guard. They also recheck the preview's resolved path, `server.cfg` revision, and resource inventory. Stop externally launched instances yourself; the app's manager cannot certify that every external launcher or process is idle.

**Re-install** replaces only a verified app-owned installation and rotates the pairing token. Review and confirm the new preview while the server is stopped, then start FXServer and check connection again. Old token copies no longer authenticate after the new server resource starts.

FXServer Installer 0.4.0 requires bridge protocol 2. Reinstall a bridge from an earlier development build through this page before reconnecting. Resource actions now include the observed server-instance ID, so commands prepared for an old instance are refused after a restart.

To remove:

1. Stop FXServer/txAdmin and open **Live Bridge** for the correct workspace/profile.
2. Choose **Remove bridge**, review the owned files and marked `server.cfg` block, and choose **Confirm removal**.
3. Inspect the result. Normal removal deletes the verified owned resource, removes only its exact config block and local pairing, and disables automatic connection. Other resources and unrelated configuration remain.

An existing unowned folder, extra files, changed file hashes, links/junctions, edited/duplicate config blocks, or unmarked bridge config entries block modification. These refusals preserve user files. Inspect and back up the conflict; do not forge ownership hashes or use a forced recursive delete to bypass the guard.

Changes are staged, but a filesystem/history failure can still leave a warning or preserved recovery files. Read the full outcome and Application Logs, inspect `server.cfg` and the resource folder, and resolve the recorded recovery condition before starting FXServer. Do not assume an error means nothing changed, and do not discard recovery files without checking them.

## Troubleshooting

| Symptom | What to check |
| --- | --- |
| Not installed or wrong path | Select the correct workspace, txData folder, and profile. Verify `server.dataPath`, `server.cfg`, and the existing resources directory. |
| Installed but disconnected | Confirm FXServer is running, the bridge was started by the managed config block, the HTTP port is correct, and automatic connection is applied. Do not use the txAdmin port. |
| Pairing needs repair | Keep the server stopped and use a reviewed Re-install for an otherwise verified owned installation. DPAPI data from another Windows account may be unreadable. |
| Authentication refused | Check for mismatched pairing or a modified token file. Never paste the token into a URL, log, issue, or chat. Do not bypass loopback checks with a proxy. |
| Resource action accepted but state unchanged | Wait for a fresh snapshot, then inspect resource errors/dependencies and bridge ACE permissions in server logs. Acceptance is not success. |
| Snapshot or event history disappears | Disconnect clears live state; bridge restart resets bridge uptime and its event buffer. The view is not a durable history service. |
| Install/remove refused | Stop all relevant processes, inspect ownership/config conflicts, and regenerate an expired or changed preview. Preserve unexpected files for review. |
| Partial-change or recovery warning | Inspect the paths named in the error and Application Logs before restarting or retrying. Verify current files/config rather than assuming rollback completed. |

## Validation Boundary

Node fixtures exercise the bundled script with mocked FXServer APIs; Rust fixtures cover local request/authentication and installation/ownership cases; UI smoke scenarios use mocked desktop/server data. These are the intended checks for changes to the bridge, not evidence of live production compatibility or load testing.

From the repository root after `npm ci`:

```bash
npm test
cargo test --manifest-path src-tauri/Cargo.toml --locked
npm run build
npx playwright install chromium
npm run test:ui
```

The Chromium installation is a prerequisite for the UI harness, not for using the packaged desktop app. Coordinate full checks with other contributors in a shared workspace. Do not install the bridge into an existing user's server or operate on live resources/databases merely to run fixtures. See the [User Guide](user-guide.md#fixture-and-ui-checks) for the harness workflow and [README](../README.md) for the full feature overview.
