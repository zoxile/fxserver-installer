# Linux and Terminal Builds

## Headless Terminal

Run from the repository root with a current stable Rust toolchain. A headless build does not require Node.js, a display server, GTK, or WebKit. Native TLS and archive dependencies still need a C toolchain, OpenSSL development files, pkg-config, and liblzma development files.

On Ubuntu/Debian, install build prerequisites explicitly:

```sh
sudo apt-get update
sudo apt-get install --no-install-recommends build-essential pkg-config libssl-dev liblzma-dev
cargo build --manifest-path src-tauri/Cargo.toml --no-default-features --features terminal --bin fxserver-manager
```

The debug binary is `src-tauri/target/debug/fxserver-manager` (`fxserver-manager.exe` on Windows). Add `--release` to Cargo for an optimized binary under `src-tauri/target/release/`.

Use the binary's `--help` output for the authoritative CLI options. Do not run the desktop or terminal manager as root merely to manage files in a user-owned server directory.

## Terminal Usage

Run the binary with no subcommand, or with `tui`, to open the interactive interface. F1-F7 select Server, Console, Resources, Database, Artifacts, Checks, and Settings. TUI passwords remain in memory for the session and are not saved to configuration.

Create a non-secret settings example at an absolute path; `init` refuses to overwrite an existing file:

```sh
./src-tauri/target/debug/fxserver-manager --config /home/fivem/manager.json init
./src-tauri/target/debug/fxserver-manager --config /home/fivem/manager.json
```

Global overrides are `--config`, `--artifact-path`, `--tx-data-path`, and `--profile`. For example:

```sh
./src-tauri/target/debug/fxserver-manager \
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

The Linux configuration enables the `desktop` Cargo feature and replaces the Windows NSIS bundle targets with `deb`, `rpm`, and `appimage`. Building a package is a separate explicit operation; nothing in the Linux check workflow publishes a release. Desktop execution requires a working graphical session. For Fedora/RHEL-family, Arch, or other distributions, use the distribution-specific prerequisites in the official Tauri guide and the same Cargo/npm commands. Ubuntu CI does not establish runtime or packaging support on those distributions.

Closing the Linux desktop window quits the application by default; it does not rely on a tray icon being available in GNOME or another desktop environment. Windows close/tray behavior is unchanged.

Normal quit, Ctrl+C, and Stop clean up the owned Linux process group. Forcibly killing or aborting the manager bypasses that cleanup; Linux does not yet have the Windows external-watchdog equivalent. Resources that deliberately detach from the process group are not contained. This is a development limitation, not a replacement for an administrator-managed service supervisor.

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
| Ubuntu 24.04 host | Frontend checks, tests and build; rustfmt; headless all-target compilation and tests; desktop all-target compilation **and desktop library unit tests**, including command-module filesystem/security safeguards excluded from headless builds. No graphical application is launched. | 60 minutes; desktop tests limited to 25 minutes. |
| Debian 13 (`debian:13-slim`) | Headless `cargo build` and `cargo test`, both with `--locked --no-default-features --features terminal --all-targets`. | 45 minutes; dependency setup limited to 10 minutes and build/tests to 30 minutes. |
| Fedora 44 (`quay.io/fedora/fedora:44`) | Same headless build and tests. | Same 45-minute job and step limits. |
| Arch rolling (`archlinux:base`) | Same headless build and tests. | Same 45-minute job and step limits. |

The three distro jobs use Docker with official [Debian](https://github.com/docker-library/official-images/blob/master/library/debian), [Fedora](https://fedoraproject.org/en/misc/), and [Arch](https://github.com/docker-library/official-images/blob/master/library/archlinux) image references. These tags track updates, not frozen images; each job logs the resolved base-image digest, distro release, and Rust versions. Dependency images are prepared without access to the checkout, installing build prerequisites only (Arch also synchronizes its rolling base). Build/tests run as the runner's unprivileged UID with no Linux capabilities, a read-only container root, checkout, and Rust toolchain, and separate writable Cargo, target, home, and temporary directories. The host Docker socket is not mounted. Dependency downloads use normal container networking; these are not offline tests. No build outputs are shared between distro jobs.

This matrix validates distro userspace compilation and ordinary unit tests on the Ubuntu runner's kernel, not native distro kernels, interactive terminal sessions, GUI/WebKit behavior, keyrings, privilege prompts, systemd integration, or package/release generation. Desktop backend tests run on Ubuntu only. Debian 12, RHEL derivatives, and other architectures are not covered. Ignored integration tests remain ignored: no job installs or starts MariaDB/FXServer, runs application install/service commands, or publishes releases. Passing CI does not establish runtime or packaging support; those workflows still need explicit testing on each intended target.

For a mocked desktop regression check, start Vite with `npm run dev -- --host 127.0.0.1 --port 5176 --strictPort`, then run `node src/tests/linux-ui-smoke.mjs`. Set `UI_BASE_URL` to use another server. The check uses Playwright Chromium, captures Windows/Linux desktop and minimum-window screenshots in `output/playwright/`, and never installs packages or starts managed services. Browser emulation is not a real Linux WebKit or keyring test.

## Windows Boundaries

Windows installer publishing remains in the existing Windows workflow. The Windows app keeps its MSI configuration and client-log features. Windows-native process handling, service installation, DPAPI storage, NSIS packaging, and WebView2 behavior still need Windows verification after shared-backend changes; Linux CI cannot validate them.
