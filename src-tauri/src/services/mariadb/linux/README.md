# Linux MariaDB Service Layer

`mariadb/mod.rs` substitutes detection, package discovery, installation, and service
control on Linux. Windows continues using its existing modules. The only added
dependency is target-specific `libc = "0.2"`; no Tauri APIs are required by Linux
service code. `process::CommandNoWindowExt` remains the existing portable no-op.

## Public API and Installer Inputs

Existing signatures and models are preserved:

- `detect::detect_mariadb() -> MariaDBStatus`
- `install::get_package_info() -> MariaDBPackageInfo`
- `install::list_series() -> Result<Vec<MariaDBSeries>, String>`
- `install::list_releases(&str) -> Result<Vec<MariaDBRelease>, String>`
- `install::install_mariadb(MariaDBInstallOptions, &dyn Fn(&str)) -> Result<String, String>`
- `install::{update_mariadb, uninstall_mariadb}(&dyn Fn(&str)) -> Result<String, String>`
- `service::{start_service, stop_service, restart_service}(Option<String>) -> Result<(), String>`

The Linux installer accepts `version: None` for the distro candidate, an available
series, upstream release, or exact package version returned by `list_releases`.
Exact versions retain epoch and distro revision, for example `1:10.11.6-0+deb12u1`.
Release date and EOL fields are empty because local package metadata does not
establish upstream maintenance dates.

The other options must be distro defaults: empty `root_password`, service name
`mariadb` (case insensitive) or `mariadb.service`, port 3306, empty optional
directories/page/buffer fields, and **all booleans false**, including
`optimize_for_transactions`, `use_utf8`, and the Windows optional components.
Unsupported input fails before privilege acquisition. Frontend/CLI callers must
not reuse Windows installer defaults or save the installer password as a verified
database login.

## Privilege and Package Boundaries

Discovery uses read-only local package metadata without elevation or refresh.
Mutations elevate just a fixed system executable with a discrete argument vector:
foreground interactive `sudo -- ...`, or `pkexec --disable-internal-agent ...` in
a graphical session. Missing authentication support fails without falling back to
an elevated app or password-on-stdin. Cancellation/failure is never retried.
Package transactions have no timeout because terminating one mid-write can damage
the package database. Only read-only probes use isolated process groups/deadlines.

- Debian/Ubuntu: APT catalog origins must be Debian/Ubuntu only. Third-party or
  unknown origins and explicit unsigned/trusted source overrides fail closed.
  APT authentication/insecure-repository safeguards are enforced for each action.
- Fedora/RHEL: only already-enabled standard distro repository IDs are selected;
  package GPG checks are forced on. Only the unversioned distro-default
  `mariadb-server` package is supported. Alternate Fedora package names and RHEL
  module-stream changes require external administration. DNF repoquery and readable
  cached metadata are prerequisites.
- Arch: only `extra/mariadb` and `extra/mariadb-clients`; required trusted package
  signatures are checked through pacman-conf. No `-Sy` partial refresh and no
  implicit whole-system `-Syu` operation. Refresh/update the OS externally first.

Trust ultimately belongs to the administrator-maintained package-manager keys,
configuration, and cache. The app does not add keys, repositories, module streams,
or arbitrary download URLs. Package lifecycle scripts can start/restart/stop the
server. Removal never requests purge, autoremove, recursive removal, or data
deletion; dependency resolution remains the distro package manager's responsibility.

## Deliberate Limitations

- Existing nonempty or unreadable `/var/lib/mysql` blocks installation. Therefore
  uninstall preserves data, but reinstall over it is an external administrator
  workflow, not an app-supported automatic reattachment.
- Existing authentication and configuration are never rewritten. No root password
  is set or claimed valid. Current client APIs explicitly use TCP, so a socket-only
  root account needs a separately provisioned login before app queries can work.
- Arch database initialization is reported as a separate administrator task. The
  app never invokes mariadb-install-db against potentially existing data.
- Automatic updates stay within the installed upstream series; migrations and
  downgrades are refused. Package verification is performed after mutations.
- Older distro metapackages can retain versioned server dependencies after removal.
  The app reports this as incomplete removal, without running autoremove or
  guessing which additional packages to delete.
- Service control requires systemd and an identified MariaDB executable behind a
  standard mariadb/mysql/mysqld unit. Custom units and non-systemd/container service
  management are external workflows.
- Credentials require Linux procfs: an exclusive 0600 temporary file is unlinked
  before secrets are written, held open, and read by the client through the parent
  process's `/proc/PID/fd/FD`. No named secret file remains after a crash.
- Linux backups use exclusive 0600 descriptor-relative output, not a result-file
  pathname reopened by the child. Failed partial outputs are retained. Restore
  tests hash, preflight, and import one sealed memfd copy, limited to 32 MiB.
- Automatic backup retention remains disabled on Linux rather than approximating
  Windows delete-by-verified-handle semantics with a pathname race.

The command layer can share storage without duplicate compilation by reexporting
`crate::services::mariadb::backup_storage as storage` instead of another `#[path]`
module declaration. That change belongs to the parent command-layer owner.

## Primary References

- [MariaDB systemd service and package initialization](https://mariadb.com/docs/server/server-management/starting-and-stopping-mariadb/systemd)
- [MariaDB authentication defaults](https://mariadb.com/docs/server/security/user-account-management/authentication-from-mariadb-10-4)
- [MariaDB option-file ordering](https://mariadb.com/docs/server/server-management/install-and-upgrade-mariadb/configuring-mariadb/configuring-mariadb-with-option-files)
- [Debian APT operations and safeguards](https://manpages.debian.org/bookworm/apt/apt-get.8.en.html)
- [Ubuntu distro client packages](https://packages.ubuntu.com/en/mariadb-client)
- [Fedora MariaDB package naming](https://developer.fedoraproject.org/tech/database/mariadb/about.html)
- [RHEL MariaDB installation and stream migration](https://docs.redhat.com/en/documentation/red_hat_enterprise_linux/9/html/configuring_and_using_database_servers/using-mariadb_configuring-and-using-database-servers)
- [DNF command reference](https://dnf.readthedocs.io/en/latest/command_ref.html)
- [DNF5 repoquery output and upgrades](https://dnf5.readthedocs.io/en/latest/commands/repoquery.8.html)
- [Arch pacman operation flags](https://man.archlinux.org/man/pacman.8.en)

Unit tests use fixture text and command-plan inspection, never real package/service
mutations. Linux-only filesystem tests exercise permissions, symlinks, procfs child
access, and immutable restore snapshots.
