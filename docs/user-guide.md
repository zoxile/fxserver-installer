# User Guide

This guide explains how to install, build, update, and operate FXServer Installer without depending on the in-app setup flow alone.

The current expansion covers the [artifact browser](#artifact-browser), [resource update planning](#resource-update-planning), [Live Bridge](#live-bridge), [configuration history](#configuration-history), [private clone and migration](#private-clone-and-migration), [Database Browser](#database-browser), [Incident Timeline](#incident-timeline), [guided diagnostics](#guided-diagnostics), and [isolated restore tests](#isolated-restore-tests).

> [!WARNING]
> These additions have not yet been validated against live production servers, resources, or databases. Use disposable fixtures and a dedicated non-production environment first. Keep independent backups before any operation that replaces files or writes database data.

## Supported Platform

FXServer Installer currently targets Windows. Several features depend on Windows-specific behavior, including MariaDB service control, Windows elevation, Windows data protection for the saved RCON password, and common FiveM client log paths.

## Install From A Release

1. Open the project's GitHub Releases page.
2. Download the latest Windows NSIS installer.
3. Run the installer.
4. Launch FXServer Installer from the Start Menu or desktop shortcut.

The installer uses a current-user install mode. Installing a newer version over an older version should update the app without removing saved app data.

Before updating, close the app from the tray as well as the visible window if it is still running in the background.

## Build From Source

Install these prerequisites first:

- Git.
- Node.js LTS and npm.
- Current Rust stable. Use the current toolchain for the code and locked dependencies, not just the minimum version declared in the manifest.
- Microsoft Edge WebView2 Runtime.
- Internet access for npm and Cargo dependency downloads.

Clone and build:

```bash
git clone https://github.com/zoxile/fxserver-installer.git
cd fxserver-installer
npm ci
npm run check
npm run tauri build
```

Build output:

```text
src-tauri/target/release/app.exe
src-tauri/target/release/bundle/nsis/
```

Use the NSIS installer in `bundle/nsis` for a normal install. Running `src-tauri/target/release/app.exe` directly is useful for a quick smoke test, but the installer is the expected distribution artifact.

## Development Mode

Run the Tauri development app:

```bash
npm run tauri dev
```

Run frontend and TypeScript checks:

```bash
npm run check
```

Run Rust checks:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

If a development run fails with an elevation error, make sure you are launching the non-elevated dev binary. The production app requests elevation only for actions that need it.

### Fixture And UI Checks

The new fixture and UI harness provides the following validation workflow after `npm ci`:

```bash
npm run check
npm test
cargo test --manifest-path src-tauri/Cargo.toml --locked
npm run build
npx playwright install chromium
npm run test:ui
```

`npm test` runs `scripts/test-*.mjs` with Node's test runner. `npm run test:ui` runs the `scripts/smoke-*.js` scenarios against a local Vite preview of `dist`, using Playwright Chromium and mocked Tauri/server responses. It is not a live FXServer or MariaDB integration test. The runner blocks non-preview network requests unless a scenario provides a mock. Browser failures save screenshots under `output/playwright/`.

Install the matching [Playwright browser binary](https://playwright.dev/docs/browsers) with `npx playwright install chromium` before the first UI run and after browser-version changes. Rebuild the frontend after UI changes. A focused run can select a scenario, for example `npm run test:ui -- smoke-artifact-plan.js`. Passing these checks is not production certification. No real server installation, resource replacement, or database mutation is needed for the fixture/UI checks.

## Version Bumps

Use the included version script to keep the UI package version and Tauri version in sync:

```bash
npm run version:bump -- patch
```

Accepted values:

- `patch`
- `minor`
- `major`
- an explicit version such as `0.2.0`

The script updates `package.json`, `package-lock.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, and `src-tauri/Cargo.lock` when those files are present.

## Publishing A Release

The Windows release workflow runs on pushes to `main`.

The workflow:

1. Checks out the repository.
2. Installs Node.js and Rust.
3. Runs `npm ci`.
4. Runs `npm run check`, `npm test`, and locked Rust tests.
5. Builds the frontend, installs Playwright Chromium, and runs `npm run test:ui`.
6. Reads the version from `src-tauri/tauri.conf.json`.
7. Creates a version tag if it does not already exist.
8. Builds the Tauri NSIS installer.
9. Publishes a GitHub release with the installer attached.

If the workflow says the tag already exists, bump the version and push again.

## Saved Data And Updates

App updates should preserve user data because app state is stored outside the install directory.

Examples of saved or remembered state:

- App configuration and selected paths.
- Artifact and server path selections.
- Optional JOOAT resolver database in the app data directory.
- RCON password, protected with Windows data protection.
- Per-workspace resource pin/ignore preferences and optional Live Bridge connection settings.
- Local Incident Timeline history, bounded to 1,000 events across all workspaces.
- Encrypted per-file configuration history, resource snapshot metadata, backup metadata, and restore-test evidence.
- Live Bridge's encrypted app pairing, when explicitly installed. Its separate server-only token file lives in the installed resource, not in client files or `server.cfg`.

Session-only data:

- MariaDB credentials typed into the app connection card are remembered only while the app is open, unless you explicitly write a generated connection string to a server config file.
- Reviewed resource queues, active apply state, and outcomes survive navigation for the current app session, but are not restored after the app restarts. Pin/ignore preferences do persist.
- Artifact catalog and release-note caches are not an offline archive of downloadable builds or resource files.

Windows-protected secrets and configuration history are tied to the Windows account; copying app data is not a portable credential migration. Ordinary preferences and timeline entries use local app/webview storage and are not protected like DPAPI secrets. Live configuration files, CSV exports, and SQL dumps can contain sensitive plaintext.

Data you should back up yourself:

- MariaDB databases.
- FXServer resources and local resource configuration.
- `server.cfg`, `permissions.cfg`, and other `.cfg` files.
- txAdmin `txData` profiles.

## First Run Wizard

Use First Run Wizard when setting up a machine for the first time. It checks known setup areas and auto-completes steps when it can detect existing work, such as:

- MariaDB installed and running.
- Artifact data already configured.
- FXServer profile/config data already present.
- Other app-managed setup state.

The wizard is a guide, not a lock. You can still open each full panel directly from the sidebar.

## MariaDB Workflow

Use Manage MariaDB to install, update, uninstall, inspect, and manage the database server.

MariaDB installation and updates download the official Windows x64 MSI over HTTPS and verify its SHA-256 checksum before running it. They do not require winget or Microsoft Store, including on Windows Server. Internet access to MariaDB's download service and mirrors is required. The app requests administrator permission when installing binaries or configuring services; it does not need to run elevated for normal use.

Installation stages appear in Manage MariaDB and Application Logs. You can switch tabs during an operation and return to see progress. FXServer start, stop, restart, and RCON commands also run in the background. A restart completes the stop before launching the server again, and overlapping lifecycle actions are rejected.

Recommended order:

1. Back up existing databases.
2. Install or update MariaDB.
3. Confirm the service is running.
4. Validate admin credentials.
5. Create or update an FXServer database user.
6. Use Queries & Files for query work, SQL imports, and backups.

MariaDB uninstall through the app is intended to remove MariaDB Server while preserving the data directory. Still, make a backup before uninstalling or updating.

## Queries & Files

Use Queries & Files for database operations after validating credentials.

The query console includes helpers for common operations. The SQL file runner can execute a selected `.sql` file globally or inside a specific database. Use database scope when importing a framework/database dump that expects tables to be created inside one schema.

Query-console and SQL-file execution accept up to 10 MiB of SQL, retain at most 16 MiB of output per stream, and time out after 30 seconds. These limits keep interactive results bounded; use a reviewed MariaDB client workflow for larger or longer-running imports. A timeout is not a rollback: inspect the database before retrying statements that may already have committed. Managed backup restores use a separate streaming path.

Remote database connections require TLS with server-certificate verification. Configure trusted certificates through MariaDB's normal client option files when required by your server. There is no automatic insecure retry. Explicit localhost or numeric loopback connections retain local compatibility. See [MariaDB client TLS options](https://mariadb.com/docs/server/clients-and-utilities/mariadb-client/mariadb-command-line-client).

## Database Browser

Open **MariaDB > Database Browser**, validate the session connection, and select a database and table. **Rows**, **Columns**, and **Indexes** separate data from schema metadata. Browsing is read-only by default; sorting, paging, and filters do not enable writes.

Rows are fetched in bounded pages of at most 200. Filters are combined with AND, with comparison, literal contains, and SQL NULL operators; at most eight filters are accepted. Empty text is different from SQL NULL. Binary values are displayed as hex. Cell previews are limited to 4,096 characters, and pages with truncated values cannot be used to update or delete rows. Active database writers can change results between pages; this is not a consistent full-database snapshot.

CSV export uses the current filters and sort starting at the first matching row, not just the visible page. It is limited to 5,000 rows and 8 MiB and refuses to overwrite an existing file. SQL NULL is represented as `\N`; empty text is quoted separately. Spreadsheet formula-like values are prefixed with an apostrophe. Exports are not redacted and are not a substitute for a database backup.

### Reviewed Row Editing

1. Back up important data and explicitly enable row editing for the selected table.
2. Choose a single row to update/delete, or prepare an insert. Include only the intended columns and distinguish NULL from empty values.
3. Inspect the statement and values in the preview. Type the exact `database.table` confirmation.
4. Apply while the preview is current, then refresh and verify the result. Previews expire after two minutes and are single-use.

Editing is deliberately restricted to supported InnoDB base tables with at most 32 columns and no triggers, check constraints, or foreign-key relationships, including incoming references. Update/delete require a complete supported primary key. Generated columns, unsupported types, and expression defaults can prevent editing. The backend also needs sufficient metadata visibility to rule out hidden side effects; missing visibility leaves the table read-only. Do not grant broad privileges just to bypass that refusal.

Apply rechecks schema and original-row evidence, uses a transaction, and rolls back unless exactly one row is affected. There is no bulk editor, automatic recovery backup, or undo for committed edits. If a connection fails around commit, the outcome may be uncertain: inspect the current row before retrying. These guards apply to Database Browser, not to arbitrary SQL entered in **Queries & Files**.

## Artifact Browser

Open **Artifacts > Install Artifact** and choose the artifact destination. The recommended-install action remains available. **Official Windows Builds** additionally lists builds from the [official Windows artifact directory](https://runtime.fivem.net/artifacts/fivem/build_server_windows/master/) and annotates them with [JG Scripts issue metadata](https://artifacts.jgscripts.com/).

1. Search by build number or issue text, filter by health or current build, and page through the results in groups of 25.
2. Check the configured current-build marker, recommendation, issue reasons, and fetch timestamps. Refresh requests both the official list and JG metadata; the normal in-session cache lasts 15 minutes.
3. Stop FXServer and txAdmin, including externally launched instances, and back up the artifact destination.
4. Select the build's install action, review the exact build and destination, acknowledge the risks, and confirm installation.

A red **Known issue** badge includes matching JG reports, including reported build ranges. **Healthy (JG)** means the current JG recommendation with no matching report, not that the app tested the build. An unlisted issue is not evidence of health: other builds are **Health unknown**. Failed refreshes can retain cached reports, but cached recommendations are labeled and no longer presented as current healthy evidence. Missing JG metadata leaves health unknown; loading fails if the official list cannot be fetched and no catalog is cached. Downloading still requires internet access.

Installing replaces artifact files in the selected location. Only official Windows download URLs are accepted. There is no managed artifact version-switch or rollback feature, no automatic backup of artifact versions, and no guarantee that an older build will work with your current resources or data.

## FXServer Setup

Use Artifacts to install or inspect FXServer artifacts, then Configure Server to load the txAdmin profile.

The app reads profile configuration from:

```text
txData/{profile}/config.json
```

It expects the server data path in:

```json
{
  "server": {
    "dataPath": "C:/path/to/server/base/"
  }
}
```

Configure Server can load common `.cfg` files, show colored config editing, generate database connection strings, add common values, and warn when RCON setup is incomplete.

For RCON commands, `server.cfg` should include:

```cfg
ensure rconlog
set rcon_password "your-secure-password"
```

Use that password in Manage Server or Resource Manager.

## Configuration History

In **FXServer > Configure Server**, load a profile and `.cfg` file, then open **Configuration History** below the editor. App saves and supported app-generated patches record file versions; this is not a continuous watcher for edits made by other tools.

1. Save or revert the editor's current draft before preparing a restore.
2. Select a historical version. Explicitly reveal configuration contents to inspect its diff; the contents may include passwords or keys.
3. Stop the server, confirm that you reviewed the replacement, and restore the selected file.

Restore requires the current file to match the reviewed content and records the pre-restore state. A changed or missing file must be reloaded and reviewed again. History is per configuration file/profile, not a full-server snapshot, and does not change database data or resource files.

History is encrypted with Windows DPAPI and fails closed if it cannot be read or encrypted. Files are limited to 512 KiB; each file keeps up to 20 versions within 4 MiB, pruning older versions. The overall store is bounded to 64 MiB and 256 history files, so a full store can block a save rather than silently abandon history. Live `.cfg` files remain plaintext. Do not share revealed diffs or screenshots without checking for secrets, and keep independent backups.

## Manage Server

Manage Server is the main runtime view. It includes:

- Start, stop, restart, and status controls.
- Optimized live console output.
- RCON command input.
- Secure saved RCON password.
- CPU and RAM history charts.
- Uptime, start time, thread count, and handle count.

If the console is extremely busy, the app batches visible output so the UI remains responsive.

Start and Restart run preflight checks first. Blocking errors prevent launch; warnings remain visible for review. Port checks are skipped during Restart because the current server still owns its ports. You can also run checks from **FXServer > Diagnostics**, which lists missing dependencies, duplicate resources, configuration references, and RCON warnings. Dynamically generated Lua configuration cannot be fully verified without executing it, so review those warnings manually.

Preflight follows resource directory symlinks and junctions, including linked groups and a linked resources root. It reads manifests and explicit `exec @resource/file.cfg` includes without changing their targets. Cycles and scan limits produce warnings. File-changing workflows retain their separate link protections.

When stopped, **Force start** offers a confirmed, one-time attempt without preflight. Use it after reviewing a false-positive check or when diagnostics cannot complete. It does not repair missing files, free occupied ports, bypass executable/process safeguards, or permanently disable checks. Overrides are recorded in Application Logs.

## Workspaces And Tasks

The first saved workspace, **Default**, adopts your existing settings. Open **Workspaces** to save another server's artifact folder, txData folder, profile, RCON endpoint, and database defaults. Switching requires a stopped server and no running conflicting tasks. Only one FXServer is managed at a time.

A txAdmin profile already selects server configuration and its data directory. A workspace is an optional app-side preset around a profile: it also separates artifact/txData paths, saved database connection defaults, backup schedules, resource update preferences, and bridge settings. Keep one workspace and switch profiles when you do not need that extra separation. Switching profiles within one workspace does not create separate app-side schedules or preferences for each profile.

RCON passwords are encrypted separately for each workspace with Windows data protection. Database passwords are session credentials; saved workspace metadata contains connection defaults but no database password. During MariaDB CLI operations, a short-lived current-user-only option file supplies credentials without putting them in command-line arguments. Normal completion removes it; an abnormal termination or cleanup failure can leave a protected temporary file. Sensitive TXHOST environment values are not included in saved workspace metadata. Removing a workspace removes its entry, backup schedules, and saved RCON password, not its server files, backup files, or databases.

Open **Task Center** from the sidebar to inspect running operations and session history. Navigation remains available during background work. Closing a resource preview does not cancel an update that has already started. File and database writes are not forcibly cancelled; quitting hides the app immediately, stops new scheduled work, and waits for active writes to finish.

Stopped-server guards cover the server managed by this app. Before maintenance, also stop any FXServer or txAdmin instance started elsewhere. Do not race a preview/apply operation with external editors, server launchers, or database writers.

## Private Clone And Migration

Open **Workspaces > Clone & Migration**. **Private clone** creates another local workspace, **Export package** writes a local folder containing a `clone-manifest.json`, and **Import package** uses such a folder to create a new workspace. This is not a cloud upload, public template publisher, or full machine/server image.

1. Select the source server-data folder containing `server.cfg` and `resources`, not the txData root. Load and select the intended resources/configuration files, or select an exported package for import.
2. Choose an existing destination parent and a new folder name. For clone/import, provide a new workspace name and review source and destination FXServer/txAdmin ports for conflicts.
3. Review included files, hashes, sizes, exclusions, and any database plan. Confirm that you have permission to make the private copy and type the exact destination path.
4. Apply the preview within 15 minutes. The app rechecks source evidence and refuses a changed source or existing destination. The original workspace is not rewritten, and no server is started.
5. Inspect the result before use. Install artifacts, configure the new txAdmin profile, supply fresh secrets and database connection settings, and check the new ports. Creating the workspace does not switch to it automatically.

The copy is intentionally incomplete. Artifact binaries and existing txAdmin profiles are not migrated. Hidden files, caches, logs, backups, credentials, database files, ordinary SQL files, executables, and other risky content are excluded. Configuration sanitization removes secret-like settings, endpoint directives, `exec` references, and external paths; selected configs must be reviewed and wired up again. Text that appears sensitive or externally referenced can be excluded wholesale, and opaque binaries outside the supported asset types are omitted. Links/junctions and path escapes are rejected.

The Live Bridge resource, its pairing files, and its configuration entries are excluded. Install a fresh bridge from the app for the new workspace; pairing credentials must not be reused through cloning.

Inspect every exclusion: resources may not run until permitted dependencies or configuration are restored manually. Sanitization is conservative but cannot prove arbitrary content is secret-free or grant redistribution rights. Do not bypass escrow, license, or ownership restrictions. Keep packages private even when a preview succeeds.

### Optional Database Dump

Database migration is opt-in and uses an explicitly selected, reviewed UTF-8 SQL dump, not a live copy of the source database. Dumps are limited to 32 MiB and must pass the constrained isolated-table SQL grammar and secret checks. Unsupported content is refused, not rewritten. Review dumps for personal data as well as credentials before packaging them.

Export stores the accepted dump in the private package. Clone/import can load it only into a newly generated `fxsi_clone_...` database on the reviewed host and port. Confirm that exact generated name and enter target credentials for the session; existing databases are never selected as the target. A successful clone keeps the new database, but does not automatically inject its credentials into copied server configuration.

If migration fails, cleanup is limited to the newly created database whose ownership marker matches. Review the outcome/evidence if cleanup cannot finish; do not delete a similarly named database based on its name alone. Use a dedicated non-production MariaDB instance for initial validation.

## Scheduled Backups And Restore

1. Open **MariaDB > Backups & Restore** and validate the connection.
2. Create a schedule, select a non-system database and an existing output folder, and set its interval and retained backup count.
3. Enable the schedule for the current app session, or leave it paused and use **Run now**.

Schedules run while the app is open or in the tray, not while the app or PC is shut down. Saved schedules reopen paused because passwords are not stored. Validate credentials and enable them again after restarting the app. Missed intervals do not trigger a burst of catch-up backups. Retention only removes verified snapshots owned by that schedule, never unrelated SQL files.

To restore, select an app-managed snapshot and review the target host, database, checksum, and warnings. Enter the requested database name to confirm. The app creates a recovery backup of the target before streaming the selected snapshot into it. Restore can replace tables and data, is not a single transaction, and should be done while FXServer and other database writers are stopped. A failed restore may require restoring the recovery snapshot. Keep an independent, off-machine backup as well.

Regular restore executes trusted SQL and is not a sandbox. Database selection does not contain qualified SQL, routines, triggers, or events that reference another schema. Review the dump and use a database-scoped account. Unlike isolated restore tests, normal backup and restore do not use the 32 MiB restricted-table grammar.

## Isolated Restore Tests

**Test Restore** in **MariaDB > Backups & Restore** is separate from the normal restore action. It verifies a supported app-managed snapshot by importing it into a new temporary database, not the original database. It still performs real writes on the selected MariaDB host, so use a dedicated non-production instance.

1. Select an app-managed snapshot and choose **Test Restore**. The app verifies its checksum and SQL before sending any dump statements.
2. Review the host, port, snapshot checksum, expected tables, and generated `fxsi_restore_test_...` database name. Validate credentials for that target.
3. Type the exact temporary database name and acknowledge automatic cleanup, including on failure. The preview expires after five minutes.
4. Run the test and inspect its saved evidence: timestamps, target, tables verified, import errors, and cleanup status.

Accepted dumps must be UTF-8, no larger than 32 MiB, and within a constrained table-oriented SQL subset. Cross-schema references, database directives, routines, triggers, events, views, generated expressions, external engines, and client commands are refused. The importer executes only the validated original bytes; it does not strip dangerous statements and continue with a modified dump.

The test creates a fresh database and ownership marker, imports the snapshot, checks the expected table inventory and basic table accessibility, then attempts cleanup. Passing is distinct from checksum verification: it means this limited import and cleanup succeeded, not that all rows were compared or that FiveM, migrations, permissions, and resource behavior were tested.

Cleanup drops only the exact generated target with its matching ownership marker. Failed or interrupted tests retain evidence. Use **Review Cleanup**, reconfirm the exact host/port/database, and provide credentials if a database remains. A missing or changed ownership marker blocks deletion; investigate rather than bypass it. Never use a matching name prefix as proof that a database is safe to remove.

## Health And Recovery

Open **FXServer > Health & Recovery** to enable CPU, RAM, or free-disk alerts. Monitoring runs in the backend at five-second intervals, including while the window is hidden. Thresholds must stay exceeded for the configured sustained period; cooldowns prevent repeated notifications. Alerts appear in Application Logs and in dismissible notifications.

Automatic crash recovery is off by default and must be explicitly enabled for the current session. It only restarts a server launched successfully by this app. It waits between attempts and allows at most three attempts per ten-minute window. Manual Stop, workspace switching, and Quit disarm recovery. This is not a Windows service or a replacement for an external uptime monitor.

## Diagnostic Export

Use **FXServer > Diagnostics** to prepare a report. Application and server log excerpts are optional. Inspect the redacted preview before exporting the ZIP; the export uses that exact preview rather than rereading changing files. Previews expire after 15 minutes. Raw configuration files and database dumps are not included by default. Redaction is a safeguard, not a guarantee: review any logs you choose to share for personal information or unusual secret formats.

## Guided Diagnostics

Run **FXServer > Diagnostics** or review the preflight results shown by Start/Restart. Findings provide relevant panel links and concrete checks for paths, profiles, ports, config references, missing dependencies, duplicate resources, RCON, and database connectivity. Read warnings even when they do not block launch.

The only automatic file fix currently offered is a reviewed `ensure rconlog` line in `server.cfg`. It is offered only when the static scan finds one installed `rconlog` and can rule out ambiguous config/dependency references. Review the before/after patch, stop the server, and confirm. Apply rechecks the configuration and resource evidence, records configuration history, and refuses changed evidence or an expired preview. The line takes effect on a subsequent server start; the patch does not start a resource immediately.

Other findings remain guided manual work. The app does not automatically download missing resources, rewrite ports/passwords, start services, or alter firewall rules. Dynamic Lua and runtime behavior cannot be established by this static scan. Configuration previews may contain secrets; inspect them locally and redact screenshots before sharing.

## Live Bridge

**FXServer > Live Bridge** is optional and off by default. It installs a small server-only resource after a stopped-server preview, pairs a local Bearer token, and polls the active workspace while the app is open or in the tray. No remote-host mode is available.

When connected, it supplies real resource states/versions, player server IDs/names/ping, server information, recent resource events, bridge uptime, and scheduler delay. Resource Manager shows runtime badges only for the connected active workspace, and clears them on disconnect. Its normal resource buttons still use RCON; the dedicated bridge page has restricted authenticated resource controls.

Scheduler delay measures lateness of the bridge's JavaScript timer, not per-resource CPU. Bridge uptime begins when the bridge resource starts, not necessarily when FXServer starts. See [Live Bridge installation, operation, and security](live-bridge.md) before enabling it, including token storage, exact removal behavior, and why the endpoint must not be proxied or tunneled.

## Resource Manager

Resource Manager scans resources from the configured server resources folder. It reads `fxmanifest.lua` files, detects repository metadata where available, compares versions against GitHub, and lets you update or reinstall resources.

Runtime controls use RCON commands:

```text
start resource_name
stop resource_name
restart resource_name
ensure resource_name
```

The app does not infer resource running/stopped state from these commands. Actual state badges appear only with a connected Live Bridge for the active workspace; without it, runtime state is unknown. Stop the resource before replacing its files and verify dependencies and connected-player impact yourself.

Update and Re-install prepare a downloaded file preview. Review added, changed, and removed files, and select any additional existing files to protect. Default protected files cannot be deselected, including when a review is queued. Applying creates a verified snapshot, stages the replacement, and checks that local files have not changed since previewing. Configuration changes required by a new resource version may still need manual migration.

Open a resource's snapshot history to restore an earlier file version. Restoring first snapshots the current resource. Snapshots cover resource files, not database migrations or external files. Storage is bounded to 20 snapshots or 10 GiB per resource; explicitly delete unwanted snapshots when the limit is reached. Keep independent backups outside the app.

### Resource Update Planning

Use **Pin installed version** to block app updates/reinstalls while retaining update visibility. **Ignore update checks** excludes a resource from checks and updates. Both preferences persist per workspace and resource path. They do not change files, enforce dependency versions, or stop another tool from updating a resource. Unpin and remove Ignore before preparing an update; a preference read error blocks updates instead of silently discarding saved choices.

1. Check for updates and open an individual update/re-install preview.
2. Expand **Release Notes**, inspect the published release and **Releases** link, and review the downloaded file changes and protected paths. Notes describe the latest published GitHub release; the archive preview uses the selected repository branch and may differ from that release.
3. Choose **Queue Reviewed Update**, or apply that single reviewed preview directly. Up to seven pending reviews can be queued; duplicate pending reviews of one resource are refused.
4. Stop the affected resources, then choose **Apply Reviewed** from **Reviewed Update Queue**. The queue invokes the existing safe preview/apply backend sequentially without downloading or approving replacement previews on your behalf.

Pause takes effect after the current resource finishes. Stop also waits for that operation, then cancels/discards pending reviews. Neither forcibly interrupts file replacement. A failure pauses the queue immediately and records the resource error; successful entries retain their snapshot IDs. Fix and re-review a failed resource separately. **Continue Remaining** requires an explicit action and does not retry the failed entry.

Previews expire 30 minutes after preparation. File changes, expired previews, workspace changes, or newly pinned/ignored resources can invalidate a review. Re-review instead of assuming an old approval applies to new content. Queue state and outcomes survive navigation, but not an app restart; only pin/ignore preferences are persisted. Applying a queue does not stop/restart resources or run database migrations automatically.

## Logs

The Logs navigation group contains:

- Incident Timeline.
- Server Logs.
- Application Logs.
- Client Logs.

Client Logs read from the local FiveM log folder, commonly:

```text
C:\Users\{User}\AppData\Local\FiveM\FiveM.app\logs
```

Application Logs are the best first place to check when an app operation fails.

## Incident Timeline

Open **Logs > Incident Timeline** to correlate app task outcomes, server/app errors, resource/configuration events, restarts, workspace activity, and health alerts. Connected bridge resource events provide additional runtime evidence. Entries include their workspace and time; use workspace, time-range, type, and text filters to narrow an incident, inspect details, and open the related panel. Switch to an entry's workspace before using workspace-specific links.

History is persisted locally and bounded to the latest 1,000 events across all workspaces, not 1,000 per workspace. Clear history explicitly for a selected workspace or all workspaces. Some sources are sampled/batched, bridge events are bounded, and events while the app is closed are not a complete historical feed. If storage fails, new events remain in memory and a persistence warning is shown.

Redaction is best effort, not encryption or a guarantee that unfamiliar secret formats are removed. Review details before sharing. Timeline ordering helps investigation but does not establish causality, identify per-resource CPU consumption, or replace original logs, backups, or a tamper-proof audit system.

## Tools

Tools & Utils includes:

- Command Palette with configurable shortcuts.
- Configurator.
- Profiler viewer.
- JOOAT Resolver & Hasher.
- JSON Formatter.

The JOOAT hasher works immediately. The full offline resolver requires an optional resolver pack. See [JOOAT Resolver Pack](jooat-resolver-pack.md) for pack format and hosting details.

## Troubleshooting

### `index.html` not found when running a release binary

Build the frontend with the Tauri build command:

```bash
npm run tauri build
```

The Tauri config expects the frontend output at `dist`.

### Release workflow skipped publishing

The tag already exists. Bump the version and push again.

### MariaDB connection refused

Check that the MariaDB service is running, the port is correct, networking is enabled if you need TCP connections, and the user host matches how you connect, for example `root@localhost` versus `root@127.0.0.1`.

### RCON command fails

Confirm that FXServer is running, the RCON port is correct, and `server.cfg` includes both `ensure rconlog` and `rcon_password`.

### GitHub update checks return 403

GitHub may be rate limiting anonymous requests. Wait a while and try again. If the repository is private or moved, update the resource manifest repository URL.

### Artifact health is unknown or refresh fails

Inspect the catalog warning and both fetch timestamps. An empty issue list or cached recommendation is not a healthy-build certification. Restore access to the official artifact/JG endpoints and refresh; do not use an unknown status to dismiss reported risks.

### A reviewed action is refused

Check the active workspace, stopped-server requirement, preview expiry, and whether files/schema/rows changed after review. Prepare a new preview instead of bypassing the guard. Queue failures and restore-test cleanup outcomes remain visible for inspection.

### Live Bridge is disconnected or removal is refused

Check the active profile, local FXServer HTTP port, server status, and pairing status. Do not paste the token into logs or a support report. Installation/removal refuses unowned or modified files/configuration; see the [bridge troubleshooting guide](live-bridge.md#troubleshooting).

### A build or install file is locked

Close the visible app window and exit it from the tray before rebuilding, updating, or deleting release files.
