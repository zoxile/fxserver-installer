# FXServer Installer

FXServer Installer is a Windows desktop app for setting up, configuring, and maintaining FiveM FXServer workspaces. It combines MariaDB setup, FXServer artifact management, config editing, RCON tools, logs, resource updates, and utility panels in one Tauri app.

The app is built with Svelte, TypeScript, Tauri, Tailwind CSS v4, shadcn-svelte, Vite, Rust, and npm.

Quick note: The main purpose of this app was for me to learn how to build with Tauri and hopefully create something useful along the way. I have really enjoyed making this project, and I hope it helps you too.

> [!WARNING]
> FXServer Installer is a new project in active development. Issues, bugs, UI changes, and breaking changes may happen between releases, so back up important server data before using app features that install, update, uninstall, or rewrite files.
> [!NOTE]
> Windows SmartScreen may warn because this project is new and currently unsigned. If you downloaded it from the official GitHub Releases page, click "More info" and then "Run anyway".

## Documentation

- [User Guide](docs/user-guide.md): Install, build, update, and operate the app without relying on the installer workflow.
- [JOOAT Resolver Pack](docs/jooat-resolver-pack.md): Build and host the optional offline JOOAT hash resolver database.
- [Contributing Guide](CONTRIBUTING.md): How to report issues, open pull requests, and write conventional commits.

## Features

### Home And Onboarding

- Dashboard cards for the main app areas.
- First Run Wizard that checks setup status and auto-completes steps when MariaDB, artifacts, and FXServer data are already available.
- Command palette shortcut hint in the sidebar.

### Manage MariaDB

- Detect MariaDB installation and service status.
- Install, update, and uninstall MariaDB while preserving database data.
- Show installed and recommended versions.
- Show installer progress and important messages in the UI.
- Log MariaDB operations to Application Logs.
- Manage users, hosts, privileges, and database access.
- Warn users to back up databases before install, update, or uninstall actions.

### Queries & Files

- Shared MariaDB connection card with the Manage MariaDB panel.
- Query console with helper snippets for common SELECT, filter, update, delete, and schema tasks.
- SQL file runner with global or database-scoped execution.
- Backup tools for full, database, and table-level exports.

### Artifacts

- Install FXServer artifacts from healthy/recommended artifact metadata provided by [JG Scripts Artifacts DB](https://artifacts.jgscripts.com/).
- Inspect artifact information and known issues.
- Track configured artifact paths inside the app.

### Configure Server

- Read txAdmin profile data from `txData/{profile}/config.json`.
- Resolve the server `dataPath` from the profile configuration.
- Load and edit common `.cfg` files such as `server.cfg`, `permissions.cfg`, `voice.cfg`, `ox.cfg`, and `misc.cfg`.
- Colored `.cfg` editor with line numbers, save, undo, and keyboard shortcuts.
- Helpers for common `server.cfg` values, RCON setup, database connection strings, and permissions.
- Highlight `rcon_password` and warn when `ensure rconlog` is missing.

### Manage Server

- Start, stop, and check FXServer status.
- Optimized console output for busy servers.
- Send RCON commands with a saved RCON password protected by Windows data protection.
- CPU and RAM performance charts with selectable time ranges.
- Uptime, started time, thread count, and handle count in the Performance card.

### Resource Manager

- Scan resources from the configured server resources folder.
- Read `fxmanifest.lua` metadata, local versions, and repository URLs.
- Check GitHub repositories for available updates.
- Update or reinstall resources that expose a repository in their manifest.
- Run `start`, `stop`, `restart`, and `ensure` through RCON.
- Exclude CitizenFX and `[cfx-default]` resources from update checks.
- Avoid runtime state badges because FXServer does not expose a reliable resource-state command through this workflow.

### Logs

- Application Logs for app-side operations.
- Server Logs for FXServer output.
- Client Logs for FiveM client logs from the local FiveM log folder.
- Real-time log following with controls for each log type.

### Tools & Utils

- Command Palette with configurable shortcuts.
- Configurator for structured Lua/config editing.
- Profiler viewer for FXServer profiler exports.
- JOOAT hasher and optional offline resolver database.
- JSON formatter and repair tool.

### Desktop App Behavior

- Current-user Windows installer.
- Close-to-tray behavior for background use.
- Custom titlebar and disabled browser context menu in production builds.
- Hidden child-process consoles for background PowerShell/system calls where possible.

## Quick Start

Download the latest Windows installer from GitHub Releases, run it, and open FXServer Installer from the Start Menu.

For source builds:

```bash
npm ci
npm run check
npm run tauri build
```

The NSIS installer is written under:

```text
src-tauri/target/release/bundle/nsis/
```

For the full setup and build workflow, read the [User Guide](docs/user-guide.md).

## Development

Start the development app:

```bash
npm run tauri dev
```

Run checks:

```bash
npm run check
cargo check --manifest-path src-tauri/Cargo.toml
```

Bump both the frontend package version and Tauri version:

```bash
npm run version:bump -- patch
```

You can also pass `minor`, `major`, or an explicit version such as `0.2.0`.

## Releases

Pushing to `main` runs the Windows release workflow. The workflow checks the app, builds the Tauri NSIS installer, creates a tag from the Tauri version, and publishes a GitHub release unless that tag already exists.

Before publishing a new release:

1. Make the code changes.
2. Run checks locally.
3. Bump the version.
4. Commit with a conventional commit message.
5. Push to `main`.

## Reporting Issues

When opening an issue, include:

- App version.
- Windows version.
- What you were trying to do.
- Exact error text.
- Steps to reproduce.
- Screenshots or short screen recordings when UI behavior is involved.
- Relevant Application Logs, Server Logs, MariaDB installer logs, or FXServer console output.

Do not post secrets such as `rcon_password`, database passwords, CFX keys, or private server IPs. Redact `server.cfg`, `.env`, and log snippets before sharing them.

## Contributing

Contributions are welcome. Read the [Contributing Guide](CONTRIBUTING.md) before opening issues or pull requests. It covers duplicate checks, local setup, verification, pull request expectations, and conventional commit formatting.

Do not commit `node_modules`, build output, generated installers, local logs, secrets, or machine-specific configuration.

## Acknowledgements

- Artifact metadata and artifact install data are powered by [JG Scripts Artifacts DB](https://artifacts.jgscripts.com/). Big thanks to JG Scripts for making artifact data easier to work with.

## Safety Notes

- Back up databases before installing, updating, or uninstalling MariaDB through the app.
- MariaDB uninstall is intended to remove the server application while preserving data, but backups are still the safest recovery path.
- RCON passwords are saved locally with Windows data protection. MariaDB credentials entered in the app are session-only unless written into a config file by the user.
- Review resource updates before applying them, especially resources with local configuration files.

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE) for details.
