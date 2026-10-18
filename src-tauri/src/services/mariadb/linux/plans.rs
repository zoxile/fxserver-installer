//! Pure plans: no process creation, repository changes, or privilege acquisition.
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Distro {
    Debian,
    Fedora,
    Rhel,
    Arch,
}

pub fn parse_os_release(source: &str) -> Result<Distro, String> {
    let mut fields = HashMap::new();
    for line in source.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !matches!(key, "ID" | "ID_LIKE") {
            continue;
        }
        let value = if value.starts_with(['\'', '"']) {
            if value.len() < 2 || !value.ends_with(value.chars().next().unwrap()) {
                return Err("Malformed os-release quoting.".into());
            }
            &value[1..value.len() - 1]
        } else {
            value
        };
        if !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b' '))
        {
            return Err("Invalid os-release distribution identifier.".into());
        }
        fields.insert(key, value);
    }
    let classify = |id| match id {
        "debian" | "ubuntu" => Some(Distro::Debian),
        "fedora" => Some(Distro::Fedora),
        "rhel" | "centos" | "rocky" | "almalinux" => Some(Distro::Rhel),
        "arch" => Some(Distro::Arch),
        _ => None,
    };
    fields
        .get("ID")
        .and_then(|id| classify(*id))
        .or_else(|| {
            fields
                .get("ID_LIKE")
                .and_then(|ids| ids.split_whitespace().find_map(classify))
        })
        .ok_or_else(|| {
            "Supported package managers are Debian/Ubuntu APT, Fedora/RHEL DNF, and Arch pacman."
                .into()
        })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub program: &'static str,
    pub args: Vec<String>,
}

impl Plan {
    pub fn new(program: &'static str, args: &[&str]) -> Self {
        Self {
            program,
            args: args.iter().map(|s| (*s).into()).collect(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Install,
    Update,
    Remove,
}

pub fn valid_package_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value.as_bytes()[0].is_ascii_digit()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".:+~_-".contains(&b))
}

pub fn upstream_version(value: &str) -> Option<String> {
    let value = value.split_once(':').map_or(value, |(_, rest)| rest);
    let numeric = value
        .split(|c: char| !c.is_ascii_digit() && c != '.')
        .next()?;
    let parts: Vec<_> = numeric.split('.').collect();
    (parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.len() <= 4))
        .then(|| numeric.into())
}

pub fn series(value: &str) -> Option<String> {
    Some(
        upstream_version(value)?
            .split('.')
            .take(2)
            .collect::<Vec<_>>()
            .join("."),
    )
}

pub fn allowed_repo(distro: Distro, id: &str) -> bool {
    match distro {
        Distro::Fedora => matches!(id, "fedora" | "updates"),
        Distro::Rhel => {
            matches!(id, "baseos" | "appstream" | "BaseOS" | "AppStream")
                || (id.starts_with("rhel-")
                    && id.ends_with("-rpms")
                    && (id.contains("-baseos-") || id.contains("-appstream-"))
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')))
        }
        Distro::Arch => id == "extra",
        Distro::Debian => matches!(id, "Debian" | "Ubuntu"),
    }
}

pub fn package_plan(
    distro: Distro,
    action: Action,
    version: Option<&str>,
    repos: &[String],
) -> Result<Plan, String> {
    if action != Action::Remove
        && !version.is_some_and(|v| valid_package_version(v) && upstream_version(v).is_some())
    {
        return Err("An exact version from the distro package catalog is required.".into());
    }
    let mut plan = match distro {
        Distro::Debian => Plan::new(
            "apt-get",
            &[
                "-o",
                "APT::Get::AllowUnauthenticated=false",
                "-o",
                "Acquire::AllowInsecureRepositories=false",
                "-o",
                "Acquire::AllowDowngradeToInsecureRepositories=false",
                "-o",
                "Acquire::AllowWeakRepositories=false",
                "-o",
                "APT::Get::AutomaticRemove=false",
                "-o",
                "APT::Get::Purge=false",
                "--assume-yes",
                "--no-remove",
            ],
        ),
        Distro::Fedora | Distro::Rhel => {
            if repos.is_empty() || repos.iter().any(|r| !allowed_repo(distro, r)) {
                return Err("No supported signed distro repositories were found.".into());
            }
            let mut p = Plan::new(
                "dnf",
                &[
                    "--assumeyes",
                    "--setopt=gpgcheck=1",
                    "--setopt=localpkg_gpgcheck=1",
                    "--setopt=clean_requirements_on_remove=False",
                ],
            );
            for repo in repos {
                p.args.push(format!("--repo={repo}"));
                p.args.push(format!("--setopt={repo}.gpgcheck=1"));
            }
            p
        }
        Distro::Arch => Plan::new("pacman", &["--noconfirm"]),
    };
    match (distro, action) {
        (Distro::Debian, Action::Remove) => {
            plan.args.retain(|a| a != "--no-remove");
            plan.args.extend([
                "remove".into(),
                "--".into(),
                "mariadb-server".into(),
                "mariadb-server-core".into(),
            ]);
        }
        (Distro::Debian, _) => {
            if action == Action::Update {
                plan.args.push("--only-upgrade".into());
            }
            plan.args.extend([
                "install".into(),
                "--".into(),
                format!("mariadb-server={}", version.unwrap()),
                format!("mariadb-client={}", version.unwrap()),
            ]);
        }
        (Distro::Fedora | Distro::Rhel, Action::Remove) => {
            plan.args
                .extend(["remove".into(), "--".into(), "mariadb-server".into()])
        }
        (Distro::Fedora | Distro::Rhel, _) => plan.args.extend([
            if action == Action::Update {
                "upgrade"
            } else {
                "install"
            }
            .into(),
            "--".into(),
            format!("mariadb-server-{}", version.unwrap()),
            format!("mariadb-{}", version.unwrap()),
        ]),
        (Distro::Arch, Action::Remove) => {
            plan.args
                .extend(["-R".into(), "--".into(), "mariadb".into()])
        }
        // Never perform -Sy (partial upgrade) or silently upgrade the entire OS with -Syu.
        (Distro::Arch, _) => plan.args.extend([
            "-S".into(),
            "--needed".into(),
            "--".into(),
            "extra/mariadb".into(),
            "extra/mariadb-clients".into(),
        ]),
    }
    Ok(plan)
}

pub fn service_unit(name: &str) -> Result<String, String> {
    let name = name.strip_suffix(".service").unwrap_or(name);
    if !matches!(name, "mariadb" | "mysql" | "mysqld") {
        return Err(
            "Only the distro MariaDB service (mariadb, mysql, or mysqld) is supported.".into(),
        );
    }
    Ok(format!("{name}.service"))
}

pub fn service_plan(action: &str, name: &str) -> Result<Plan, String> {
    if !matches!(action, "start" | "stop" | "restart") {
        return Err("Unsupported MariaDB service action.".into());
    }
    Ok(Plan::new(
        "systemctl",
        &["--system", "--no-pager", action, "--", &service_unit(name)?],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn distro_parsing_never_evaluates_shell() {
        for (input, expected) in [
            ("ID=ubuntu", Distro::Debian),
            ("ID=debian", Distro::Debian),
            ("ID=fedora", Distro::Fedora),
            ("ID=rhel\nID_LIKE=\"fedora\"", Distro::Rhel),
            ("ID=rocky\nID_LIKE=\"rhel centos fedora\"", Distro::Rhel),
            ("ID=derivative\nID_LIKE='arch'", Distro::Arch),
        ] {
            assert_eq!(parse_os_release(input).unwrap(), expected);
        }
        for input in [
            "ID=$(touch /tmp/x)",
            "ID=\"ubuntu",
            "ID=unsupported",
            "ID=ubuntu;reboot",
        ] {
            assert!(parse_os_release(input).is_err());
        }
    }
    #[test]
    fn package_plans_preserve_data_and_trust() {
        for distro in [Distro::Debian, Distro::Fedora, Distro::Rhel, Distro::Arch] {
            let repos = vec![if distro == Distro::Fedora {
                "fedora"
            } else {
                "appstream"
            }
            .into()];
            for action in [Action::Install, Action::Update, Action::Remove] {
                let plan =
                    package_plan(distro, action, Some("1:11.4.5-2+deb12u1"), &repos).unwrap();
                for forbidden in [
                    "purge",
                    "autoremove",
                    "--allowerasing",
                    "--nogpgcheck",
                    "-Rns",
                    "-Sy",
                    "-Syu",
                    "sh",
                    "-c",
                ] {
                    assert!(!plan.args.iter().any(|a| a == forbidden));
                }
                assert!(plan.args.iter().any(|a| a == "--"));
            }
        }
    }
    #[test]
    fn versions_and_units_cannot_supply_options_or_shell_code() {
        for bad in [
            "-y",
            "11.4;reboot",
            "11.4$(id)",
            "../payload",
            "11.4\nreboot",
            "",
        ] {
            assert!(package_plan(Distro::Debian, Action::Install, Some(bad), &[]).is_err());
        }
        for bad in [
            "ssh",
            "--all",
            "mariadb*",
            "mariadb@x",
            "../mariadb",
            "mysql;reboot",
        ] {
            assert!(service_plan("start", bad).is_err());
        }
        assert_eq!(
            service_plan("restart", "mariadb").unwrap().args,
            ["--system", "--no-pager", "restart", "--", "mariadb.service"]
        );
        assert!(service_plan("enable", "mariadb").is_err());
    }
    #[test]
    fn debian_removal_includes_daemon_but_keeps_clients_and_data() {
        let plan = package_plan(Distro::Debian, Action::Remove, None, &[]).unwrap();
        let packages = plan.args.split(|arg| arg == "--").next_back().unwrap();
        assert_eq!(packages, ["mariadb-server", "mariadb-server-core"]);
        assert!(plan.args.iter().any(|arg| arg == "APT::Get::Purge=false"));
        assert!(plan
            .args
            .iter()
            .any(|arg| arg == "APT::Get::AutomaticRemove=false"));
    }
    #[test]
    fn parses_distro_versions_and_rejects_unrelated_repos() {
        assert_eq!(
            upstream_version("1:10.11.6-0+deb12u1").as_deref(),
            Some("10.11.6")
        );
        assert_eq!(series("3:10.5.27-1.el9").as_deref(), Some("10.5"));
        assert!(!allowed_repo(Distro::Fedora, "copr:user:db"));
        assert!(!allowed_repo(Distro::Arch, "custom"));
        assert!(allowed_repo(
            Distro::Rhel,
            "rhel-9-for-x86_64-appstream-rpms"
        ));
    }
}
