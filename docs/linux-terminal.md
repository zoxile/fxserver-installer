# Linux and Terminal Builds

## Installed Command

`fxins` opens the TUI by default; `fxins tui` is equivalent. Use `fxins run` for a foreground managed server, or `fxins --help` for all subcommands. The existing `fxserver-manager` command remains available as a compatibility binary with the same behavior.

The current-user Windows NSIS installer includes both executables and adds its installation directory to your **user** PATH without elevation. Open a new terminal after installing (restart the terminal application if it still has an old environment), then run `Get-Command fxins` in PowerShell. Uninstall removes only the entry recorded as added by that installation; preexisting entries and unrelated PATH edits are preserved. Ambiguous manually modified entries are left for you to review. Paths containing `;` or `%` cannot be registered safely; the installer reports a warning and the full executable path remains usable.

Linux DEB/RPM packages install `/usr/bin/fxins` and `/usr/bin/fxserver-manager` through the package manager, with no shell-profile edits. Check discovery with `command -v fxins`. An AppImage does **not** install a host PATH command; use the DEB/RPM or the headless source installation below for an installed `fxins`.

Both names retain the existing `com.fxserver.installer` settings namespace and `terminal.json`: `%LOCALAPPDATA%\com.fxserver.installer\terminal.json` on Windows and `$XDG_DATA_HOME/com.fxserver.installer/terminal.json` (default `~/.local/share/com.fxserver.installer/terminal.json`) on Linux. Renaming the command does not migrate, reset, or duplicate settings; `--config` still selects an explicit file.

## Headless Terminal

Run from the repository root with a current stable Rust toolchain. A headless build does not require Node.js, a display server, GTK, or WebKit. Native TLS and archive dependencies still need a C toolchain, OpenSSL development files, pkg-config, and liblzma development files.

On Ubuntu/Debian, install build prerequisites explicitly:

```sh
sudo apt-get update
sudo apt-get install --no-install-recommends build-essential pkg-config libssl-dev liblzma-dev
cargo install --path src-tauri --locked --no-default-features --features terminal --bin fxins --bin fxserver-manager
```

This [Cargo installation](https://doc.rust-lang.org/cargo/commands/cargo-install.html) builds optimized binaries without the desktop stack. It also works on Windows with a stable Rust MSVC toolchain and its C/C++ build prerequisites. Cargo installs commands into its installation root's `bin` directory, normally `~/.cargo/bin` (`%USERPROFILE%\.cargo\bin` on Windows). Ensure that directory is on your user PATH; a custom `--root` needs its own `bin` directory on PATH. Do not run `cargo install` as root. Existing installed binaries may require an explicit `--force` to replace them after reviewing the checkout. Cargo installation does not change the app's settings location.

To build without installing:

```sh
cargo build --manifest-path src-tauri/Cargo.toml --locked --no-default-features --features terminal --bin fxins --bin fxserver-manager
```

The debug binaries are `src-tauri/target/debug/fxins` and `fxserver-manager` (with `.exe` on Windows). Add `--release` to Cargo for optimized binaries under `src-tauri/target/release/`. Substitute the full binary path for `fxins` in the examples below when running an uninstalled build.

Use the binary's `--help` output for the authoritative CLI options. Do not run the desktop or terminal manager as root merely to manage files in a user-owned server directory.

## Terminal Usage

Run the binary with no subcommand, or with `tui`, to open the interactive interface. F1-F9 select Server, Console, Resources, Database, Artifacts, Checks, Settings, Actions, and Config. The minimum size is 50 columns by 14 rows; 100 columns by 30 rows is more comfortable for forms and charts.

Passwords are session-only by default and never written to `terminal.json`. Actions includes explicit remember/load/forget operations using the same OS encryption services as the desktop. Previously remembered credentials load once when the TUI opens, scoped to the configured endpoint and server profile. Linux needs an unlocked Secret Service keyring; failure never falls back to plaintext. Save the non-secret connection settings too so the next session selects the same credential scope. Desktop workspace credentials are not silently imported.

### First Session

1. Run `fxins`, then press F7. Use the arrow keys and Enter to set the artifact folder, txData folder, and connection details. Paths must be absolute.
2. Press `p` to discover and choose a txAdmin profile. An empty txData path can be discovered from the artifact directory. Press `s` to save non-secret settings.
3. Open F6 and press `r` to review preflight checks. Resolve blocking findings before starting the server.
4. Open F1 and press `s` to start, `x` to stop, or `r` to restart. Stop/restart ask for confirmation. Force start is `f`, after reviewing the checks.
5. Open F2 and press Enter to enter an RCON command. Set the RCON password in Settings first. PgUp/PgDn scroll the bounded console; End follows its latest output.

Server history retains up to an hour of samples while this TUI is running. On F1, keys `1`-`5` select 30 seconds, 5 minutes, 10 minutes, 30 minutes, or one hour. CPU and RAM use separate colors; session lows/highs reset for a new managed process. Small terminals show the numeric summary instead of squeezing in unreadable charts.

### Actions And Forms

Press Ctrl+K or F8 to open Actions. Type to filter by group/title, use Up/Down to select, and Enter to open. Selection wraps and scrolls into view. F1-F9 remain available while work is running. In forms, Tab/Shift+Tab or Up/Down select fields, Left/Right changes choices, and Enter reviews or runs the action. Alt+V reveals/hides the active password field; leaving it masks it again. Esc cancels a form or returns from a result. Ctrl+C quits safely; `q` is searchable text while Actions is focused.

Mutations require explicit confirmation. Some higher-risk operations also require a fresh backend preview and its exact token/target. Read the result, return to Actions with Esc, then choose the corresponding Apply action and enter the displayed values. Stale or mismatched previews fail instead of silently regenerating an approval. Ctrl+O reopens the last result without discarding another form; Esc returns to that form. Result pages support arrows, PgUp/PgDn, Home, and End. Forms accept single-line input up to 8 KiB; use the SQL-file runner or Config editor for multiline content.

| Area | Available in the TUI |
| --- | --- |
| MariaDB | Detect installation/service, inspect package versions, install/update/uninstall, service start/stop/restart, validate/change connection, securely remember/load/forget login. Installer messages appear during the operation. |
| Database browser | List databases/tables, inspect columns/indexes, bounded filtered/sorted row pages, export filtered CSV to a new file. F3 then `d` opens a database picker for the current session. |
| Administration | Create accounts, change passwords, delete exact accounts, add database-scoped privileges, inspect grants, preview/apply database or table creation and table maintenance. |
| SQL and diagnostics | F3 Enter opens SQL input; `f` reads a SQL file for review. Actions includes server/database inspection and SQL compatibility checks. SQL runs in the selected database, or global scope if it is empty. |
| Backups | Database or table dumps, schema/data/full modes, optional routines/triggers/events, and isolated restore-test preview/apply/owned-database cleanup. Back up independently before installer or destructive SQL actions. |
| Resources | Manifest scanning and RCON start/stop/restart/ensure, version checks, staged update previews, protected files, snapshots, rollback previews and apply. Resource update and rollback operations require the selected server to be stopped. |
| Artifacts | Host-platform Legacy and Enhanced catalogs and exact-version installation. Enhanced remains early access; health warnings are not a stability guarantee. |
| Live Bridge | Inspect installation, preview/apply installation or removal, fetch live resources/players, send validated resource actions to a reviewed bridge instance. |
| Profiler | Capture a bounded recording from the managed running server, retain its result, export to a new file, discard the retained capture. |
| Configuration | Multiline colored editor, common-value and permission-rule previews, validation, encrypted history and revision-checked restoration. |
| Logs and utilities | Bounded application/server/client log tails, JSON validation/formatting, and lookups in an already installed offline JOOAT resolver database. Client logs remain Windows-only. |

### Editing Configuration

Press F9, then `r` to load the selected profile's cfg files. Select a file and press Enter. The editor uses line numbers, syntax colors, horizontal scrolling, and Unicode-aware cursor handling. Ctrl+S saves, Ctrl+Z undoes, and Ctrl+Y redoes. Esc returns to the file list when the draft is unchanged; with unsaved edits it shows a warning. Ctrl+W explicitly discards the draft. Switching views retains the draft. Files are bounded to 512 KiB and fewer than 65,535 lines.

Saving checks that the original file still matches and retains encrypted history before replacing it. If another tool changed the file, reload and reconcile the changes; the TUI does not overwrite it blindly. On Linux, saving with encrypted history needs the unlocked keyring described above. File contents and explicitly requested log/history views can contain secrets: avoid screen sharing or terminal recording while viewing them.

### Current Boundaries

This is substantial terminal coverage, not a pixel-for-pixel copy of the desktop. The desktop's scheduled backup/maintenance and crash-recovery controllers, workspace cloning, visual profiler analysis, structured Lua configurator, and command-keybinding editor are not run by the TUI. The terminal can query arbitrary reviewed SQL, but its guided database browser is read-only; use explicit SQL or the desktop's guarded row editor for row mutations. The JOOAT database must already be installed. Multiple terminal settings files can be selected with `--config`; this is not automatic desktop workspace synchronization.

Only one mutating/query job runs at a time. Navigation, console collection, and performance sampling remain independent. A running operation must finish before an ordinary quit; the terminal does not promise transactional cancellation of installers or database changes. `fxins run` and TUI server ownership are foreground sessions, not system services. Leaving the session stops its managed FXServer.

Create a non-secret settings example at an absolute path; `init` refuses to overwrite an existing file:

```sh
fxins --config /home/fivem/manager.json init
fxins --config /home/fivem/manager.json
```

Global overrides are `--config`, `--artifact-path`, `--tx-data-path`, and `--profile`. For example:

```sh
fxins \
  --artifact-path /home/fivem/server \
  --tx-data-path /home/fivem/txData \
  --profile default doctor
```

| Command | Behavior |
| --- | --- |
| `--help` | Show options without opening the TUI. Each subcommand also accepts `--help`. |
| `doctor` | Run preflight checks without starting the server. |
| `profiles` | Discover txAdmin profiles. |
| `resources` | List the selected profile's resources. |
| `run` | Start in the foreground; Ctrl+C stops the managed server and its child processes. `--force` bypasses blocking preflight results only after you review them. |
| `artifacts --refresh` | Refresh and list host-platform Legacy builds. |
| `install-artifact VERSION --yes --acknowledge-risk` | Install an exact listed build into the configured artifact directory. Stop the server and back up its files first. |
| `rcon "status"` | Send one command with a hidden password prompt. Optional `--host`, `--port`, and `--password-stdin`. |
| `database-status` | Inspect MariaDB installation and service status without changing it. |
| `database status` | Inspect MariaDB installation and service status; equivalent to `database-status`. |
| `database packages` | Inspect the configured distribution package catalog. |
| `database versions [SERIES]` | List available package versions, optionally for one series. |
| `database install --version V --yes` | Install a selected package with distribution defaults. Linux does not set a root password. |
| `database update --yes` | Request a compatible package update; package scripts may restart MariaDB. |
| `database uninstall --yes` | Remove the server package without purging database files. This is not a backup or a promise that later reinstall will be automatic. |
| `database start` | Start the detected MariaDB service. |
| `database stop --yes` / `database restart --yes` | Explicitly stop or restart the detected service. |
| `database backup --output DIR --database NAME --user USER` | Create a database backup with a hidden password prompt or `--password-stdin`. |
| `query "SELECT 1;"` | Preview SQL only. Add `--execute` to run it with a hidden password prompt. |
| `query --file /home/fivem/reviewed.sql` | Preview a SQL file. Optional connection overrides are `--host`, `--port`, `--user`, `--database`, and `--password-stdin`; execution still requires `--execute`. |

Use hidden prompts interactively. For automation, `--password-stdin` accepts one UTF-8 password line from a trusted secret source; never place passwords in CLI arguments, shell history, or the config file. Commands that change files or execute SQL are not dry runs unless explicitly described above. On Windows, invoke the `.exe` binary from PowerShell with Windows paths; the command names and flags are the same.

`database install` accepts `--password-stdin` for platforms that configure an installer password; do not supply a root password for a Linux distribution-package install. Use `database <command> --help` for the exact options supported by your build.

## Linux MariaDB

Package operations target configured, signed distribution repositories on Debian/Ubuntu, Fedora/RHEL-family, and Arch. They do not add repositories, change module streams, or silently substitute unavailable versions. Update the system package cache externally when needed. Package and systemd actions require administrator authorization for the specific operation, not a root desktop session.

Linux installation supports distribution defaults only: `mariadb` service, port 3306, and the default `/var/lib/mysql` data directory. It does not apply Windows MSI tuning, directory, component, or account options. It does not set or validate a root password, reset existing authentication, or turn socket-only root authentication into a TCP password login. Configure a dedicated MariaDB login separately before using the app's SQL tools.

A non-empty or unreadable `/var/lib/mysql` directory causes installation to be refused. Preserved-data reinstalls require an administrator to verify data-version compatibility and perform the distribution's recovery procedure externally. Updates across MariaDB series are also refused; use an independent backup and a reviewed external migration. Package removal does not purge the database directory, but preserving files is not equivalent to verifying they can be reattached automatically.

Distribution package scripts may initialize or start a new service, and upgrades may restart it. Arch additionally requires an administrator to initialize a **new** database using the distribution's procedure before starting it; this app does not initialize or overwrite preserved data directories. Containers and non-systemd hosts may use the connection/query features but must manage the service externally. These are intentionally narrower semantics than the Windows MSI flow.

## Linux Desktop

Install Node.js 22 and the [official Tauri v2 Linux prerequisites](https://v2.tauri.app/start/prerequisites/#linux), plus the headless prerequisites above. Ubuntu 24.04 is the CI target:

```sh
sudo apt-get install --no-install-recommends \
  libwebkit2gtk-4.1-dev curl wget file libxdo-dev \
  libayatana-appindicator3-dev librsvg2-dev
npm ci
npm run tauri -- dev --config src-tauri/tauri.linux.conf.json
```

For a local desktop build without packaging:

```sh
npm run tauri -- build --no-bundle --config src-tauri/tauri.linux.conf.json
```

Desktop packaging enables both `desktop` and `terminal` Cargo features so the bundle contains the application, `fxins`, and the compatibility binary `fxserver-manager`. The Linux configuration replaces the Windows NSIS bundle targets with `deb`, `rpm`, and `appimage`. The bundled terminal can run without a graphical session but shares the desktop build's library requirements; use the separate headless build above for servers without GTK/WebKit. Building a package is a separate explicit operation; nothing in the Linux check workflow publishes a release. Desktop execution requires a working graphical session. For Fedora/RHEL-family, Arch, or other distributions, use the distribution-specific prerequisites in the official Tauri guide and the same Cargo/npm commands. Ubuntu CI does not establish runtime or packaging support on those distributions.

Closing the Linux desktop window quits the application by default; it does not rely on a tray icon being available in GNOME or another desktop environment. Windows close/tray behavior is unchanged.

Normal quit, Ctrl+C, Stop, SIGTERM, and SIGHUP request graceful shutdown before bounded forced cleanup. A separate Linux supervisor owns the server process group and artifact lock. If the manager crashes or receives SIGKILL, its private socket closes and the supervisor performs the same cleanup. The lock stays held until cleanup finishes. Killing the supervisor itself, power loss, and resources that deliberately detach from the process group are outside this guarantee; use a reviewed systemd/cgroup deployment when stronger containment is needed. A running operation finishes before the TUI worker shuts down.

Desktop and terminal sessions coordinate through private, persistent per-user lock files. On Linux they must share the host's `/tmp` namespace; do not run a second manager for the same server in a container or a systemd service with `PrivateTmp=yes`. Locks coordinate this application's operations, not external database clients or administrators.

## Capabilities

- The desktop queries `get_platform_info` once at startup. Browser previews use `navigator` without launching process or package probes.
- Artifact downloads and official listing links match the host platform. Legacy IPC command names containing `windows` are retained for compatibility; they are not Windows-only operations.
- Linux metadata must contain `linuxDownloadLink`; a missing Linux URL never falls back to a Windows archive. Enhanced installation requires a build returned by the official Linux catalog. An unavailable listing is shown as an error, with installation disabled.
- Linux server and txData fields accept absolute paths such as `/home/fivem/server` and `/home/fivem/txData`. Slow-query imports accept local absolute `.log` paths while rejecting URLs, relative paths, and Windows UNC/device/alternate-stream paths.
- FiveM client-log discovery is Windows-only. Linux still has FXServer logs and application logs.
- Saved database logins, saved RCON passwords, and encrypted configuration history require secure storage. On Linux this needs libsecret's `secret-tool` and an unlocked session keyring with a session D-Bus connection. Install the corresponding package explicitly (for example, `libsecret-tools` on Ubuntu/Debian). A headless SSH session may not provide a keyring; credentials then remain session-only. Encryption errors do not permit plaintext persistence. Restart the app after changing system capabilities.
- MariaDB package support targets Debian/Ubuntu, Fedora/RHEL-family, and Arch distribution catalogs; Windows MSI presets, custom installation directories, and bundled HeidiSQL settings are not offered. Packages and services need explicit administrator authorization. Non-systemd hosts, unavailable catalog versions, and unsupported distributions may require external MariaDB administration. Back up data independently before package actions.
- Linux app updates currently use source builds and project releases, not the Windows installer download.

## Checks

These commands do not install MariaDB, download FXServer, or start managed services:

```sh
npm run check
npm test
node --test src/tests/linux-platform.test.mjs
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo check --manifest-path src-tauri/Cargo.toml --locked --no-default-features --features terminal --all-targets
cargo test --manifest-path src-tauri/Cargo.toml --locked --no-default-features --features terminal
npm run build
cargo check --manifest-path src-tauri/Cargo.toml --locked --no-default-features --features desktop --all-targets
cargo test --manifest-path src-tauri/Cargo.toml --locked --no-default-features --features desktop --lib
```

`.github/workflows/linux-checks.yml` runs on pull requests, manual dispatch, and pushes to `codex/linux-terminal-support`. All jobs use Ubuntu 24.04 x86_64 runners with stable Rust:

| Environment | Automated coverage | Time limit |
| --- | --- | --- |
| Ubuntu 24.04 host | Frontend checks, tests, browser workflows and build; dependency audits; rustfmt and strict Clippy; headless and desktop backend tests; native PTY interaction; isolated Secret Service persistence and missing-key tests; release `deb`, `rpm`, and AppImage contents; native WebKit startup, IPC and navigation under Xvfb. A separate disposable Ubuntu container exercises real MariaDB packages and owned backup/restore safeguards. | 90 minutes; package build limited to 35 minutes. |
| Debian 13 (`debian:13-slim`) | Headless locked all-target build and tests, plus real MariaDB package installation, query, backup/import, and data-preserving removal in a separate disposable container. | 60 minutes; setup limited to 15 minutes, build/tests to 30, and database integration to 15. |
| Fedora 44 (`quay.io/fedora/fedora:44`) | Same headless and isolated database checks. | Same limits. |
| Arch rolling (`archlinux:base`) | Same headless and isolated database checks. | Same limits. |

The three distro jobs use Docker with official [Debian](https://github.com/docker-library/official-images/blob/master/library/debian), [Fedora](https://fedoraproject.org/en/misc/), and [Arch](https://github.com/docker-library/official-images/blob/master/library/archlinux) image references. These tags track updates, not frozen images; each job logs the resolved base-image digest, distro release, and Rust versions. Dependency images are prepared without access to the checkout, installing build prerequisites only (Arch also synchronizes its rolling base). Build/tests run as the runner's unprivileged UID with no Linux capabilities, a read-only container root, checkout, and Rust toolchain, and separate writable Cargo, target, home, and temporary directories. The host Docker socket is not mounted. Dependency downloads use normal container networking; these are not offline tests. No build outputs are shared between distro jobs.

The database containers run actual package changes only inside their disposable filesystems. The harness refuses host database mounts, runs the app as a dedicated unprivileged user, and grants that user narrowly listed package/service commands through container-only sudo rules. It verifies login, bad-password rejection, read-only grants, SQL file round trips, private backups, and byte-for-byte data preservation after uninstall. Ubuntu also exercises the owned-restore implementation against a real database. No workstation database is used.

This matrix runs on the Ubuntu runner's kernel, not native distro kernels. Native GUI and keyring coverage is Ubuntu/Xvfb only. Package extraction is not an installed-package or interactive upgrade test. The database containers have no systemd PID 1, so service controls are checked for safe refusal; real systemd control, polkit prompts, cross-version database upgrades, and preserved-data reattachment still need dedicated integration environments. Debian 12, RHEL derivatives, Wayland, and other architectures are not covered. FXServer lifecycle tests use shell fixtures, not a licensed live game server. No check publishes a release. These are release-readiness checks, not independent certification or a guarantee of vulnerability-free software.

Native desktop screenshots and driver logs are retained as the `native-linux-desktop` workflow artifact for seven days. Failed browser workflows also retain a screenshot, console errors, and visible page text as `browser-failures`. All browser data comes from test fixtures. Consult the latest development-branch run for actual results; adding a check here does not mean it has passed.

Before promoting this branch beyond beta, verify installed-package upgrades, interactive administrator authorization and actual systemd control on disposable target systems, then exercise licensed FXServer startup/shutdown, resource updates, RCON and sustained console traffic. Repeat database recovery with independent backups and representative data. Dependency scans do not cover every logic flaw or the distribution's system libraries; keep OS packages updated. The current Rust audit still reports nine unmaintained transitive dependencies in the desktop stack. These are maintenance risks, not proof of an exploit, and must be reviewed with future Tauri upgrades.

For a mocked desktop regression check, start Vite with `npm run dev -- --host 127.0.0.1 --port 5176 --strictPort`, then run `node src/tests/linux-ui-smoke.mjs`. Set `UI_BASE_URL` to use another server. The check uses Playwright Chromium, captures Windows/Linux desktop and minimum-window screenshots in `output/playwright/`, and never installs packages or starts managed services. Browser emulation is not a real Linux WebKit or keyring test.

## Windows Boundaries

Windows installer publishing remains in the existing Windows workflow. The Windows app keeps its MSI configuration and client-log features. Windows-native process handling, service installation, DPAPI storage, NSIS packaging, and WebView2 behavior still need Windows verification after shared-backend changes; Linux CI cannot validate them.

## GLib Security Backport

Tauri's compatible GTK3 stack uses GLib 0.18.5, which predates the released fix for [RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html). The string iterator passed an immutable pointer reference to a C function that writes through it. Optimized Rust builds can consequently dereference null and crash. This is an upstream memory-safety bug, not evidence that this application was exploited.

The desktop build uses the exact original crate with the upstream two-line fix applied. [Backport provenance and verification](../src-tauri/vendor/glib/BACKPORT.md) records its source, checksum, license and patch. Headless terminal builds do not depend on GLib. CI compares all original source files against the checksummed archive and runs optimized iterator regressions.

Cargo audit skips local path dependencies. `scripts/audit-glib-backport.py` separately audits the original registry version without suppressing scanner findings, accepts only the exact independently verified backport advisory, and fails on additional advisories. Maintenance warnings elsewhere in the Tauri dependency tree remain visible. Recheck this override when upgrading Tauri; remove it only when the compatible dependency chain includes an upstream fixed version and the native tests pass. A local backport adds maintenance responsibility and is not a certification of the whole dependency stack.
