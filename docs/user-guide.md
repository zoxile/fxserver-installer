# User Guide

This guide explains how to install, build, update, and operate FXServer Installer without depending on the in-app setup flow alone.

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
- Rust stable. The Tauri project currently requires Rust `1.77.2` or newer.
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
4. Runs `npm run check`.
5. Reads the version from `src-tauri/tauri.conf.json`.
6. Creates a version tag if it does not already exist.
7. Builds the Tauri NSIS installer.
8. Publishes a GitHub release with the installer attached.

If the workflow says the tag already exists, bump the version and push again.

## Saved Data And Updates

App updates should preserve user data because app state is stored outside the install directory.

Examples of saved or remembered state:

- App configuration and selected paths.
- Artifact and server path selections.
- Optional JOOAT resolver database in the app data directory.
- RCON password, protected with Windows data protection.

Session-only data:

- MariaDB credentials typed into the app connection card are remembered only while the app is open, unless you explicitly write a generated connection string to a server config file.

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

## Manage Server

Manage Server is the main runtime view. It includes:

- Start, stop, and status controls.
- Optimized live console output.
- RCON command input.
- Secure saved RCON password.
- CPU and RAM history charts.
- Uptime, start time, thread count, and handle count.

If the console is extremely busy, the app batches visible output so the UI remains responsive.

## Resource Manager

Resource Manager scans resources from the configured server resources folder. It reads `fxmanifest.lua` files, detects repository metadata where available, compares versions against GitHub, and lets you update or reinstall resources.

Runtime controls use RCON commands:

```text
start resource_name
stop resource_name
restart resource_name
ensure resource_name
```

The app does not show resource running/stopped state because the workflow does not have a reliable state source. Back up local resource configuration before updating or reinstalling a resource because upstream files may replace local changes.

## Logs

The Logs navigation group contains:

- Server Logs.
- Application Logs.
- Client Logs.

Client Logs read from the local FiveM log folder, commonly:

```text
C:\Users\{User}\AppData\Local\FiveM\FiveM.app\logs
```

Application Logs are the best first place to check when an app operation fails.

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

### A build or install file is locked

Close the visible app window and exit it from the tray before rebuilding, updating, or deleting release files.
