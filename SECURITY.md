# Security Policy

## Reporting A Vulnerability

Do not publish exploit details, passwords, pairing tokens, connection strings, or unredacted logs in public issues. Use [GitHub's private vulnerability reporting](https://github.com/zoxile/fxserver-installer/security/advisories/new) when it is available. If that option is unavailable, open an issue requesting a private contact method without including sensitive details.

Include the app version, Windows version, affected workflow, reproduction steps using disposable data, and the impact you observed. Give maintainers time to investigate before public disclosure. There is no guaranteed response time or bug bounty.

## Supported Scope

This project builds a Windows desktop app. Security fixes target the latest release; older builds do not have a maintenance guarantee. Version 0.4.0 is a beta with incomplete live-environment validation, not a security certification.

- The app can run installers, operate MariaDB, manage FXServer processes, and replace user-selected files. Only run it for servers and databases you are authorized to administer.
- Resource code and SQL files are executable inputs. Install only trusted resources and review SQL before running it. A preview or checksum does not establish that third-party content is trustworthy.
- Keep independent backups outside the managed server directory before installing, updating, restoring, or migrating data.
- Secrets written into server configuration remain plaintext there. DPAPI protects the app's saved secrets, not every copy of those secrets on disk or against a compromised Windows account.
- MariaDB client operations use temporary option files restricted to the current Windows user. Normal completion removes them; abnormal termination can leave protected files in the Windows temporary directory. Fresh database initialization still uses the official initializer's password argument, and preserved-data password resets use temporary SQL. These operations are not protected against a compromised local account or administrator.
- Remote MariaDB connections require verified TLS. Only explicit localhost or numeric loopback endpoints retain local plaintext compatibility; there is no automatic insecure retry for remote hosts.
- Live Bridge accepts authenticated local requests only. Do not expose its route through a reverse proxy or tunnel, and protect its server-side token file.
- The desktop CSP blocks remote scripts and embedded pages. HTTPS data connections remain allowed for user-configured resolver packs; this is not a sandbox for malicious server resources or local processes.
- GitHub release downloads currently use unsigned Windows installers. Verify the repository and release before running an installer. Checksums, when provided, verify file integrity but do not replace code signing.

## Dependency Checks

Release CI runs `npm audit` and RustSec `cargo audit` against the committed lockfiles and blocks publication for reported vulnerabilities. It also runs type checks, strict Rust linting, native tests, and mocked browser workflows. A clean scan only means those databases reported no known vulnerabilities at the time of the scan.

Tauri's cross-platform dependency graph includes upstream GTK/GLib and Unicode-library maintenance advisories. These remain visible in audit output; they are not suppressed. GTK/GLib is not part of the Windows executable. The project does not claim all upstream maintenance or platform warnings have been resolved, and does not ship a tested Linux build.
