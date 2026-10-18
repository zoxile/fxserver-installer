use super::linux::{
    plans::{self, Distro, Plan},
    runtime,
};
use crate::models::mariadb::{MariaDBRelease, MariaDBSeries};
use std::{collections::BTreeSet, fs, path::Path};

#[derive(Clone, Debug)]
pub(super) struct Catalog {
    pub distro: Distro,
    pub versions: Vec<String>,
    pub installed: Option<String>,
    pub candidate: Option<String>,
    pub repos: Vec<String>,
}

pub(super) fn distro() -> Result<Distro, String> {
    let source = fs::read_to_string("/etc/os-release")
        .or_else(|_| fs::read_to_string("/usr/lib/os-release"))
        .map_err(|e| format!("Cannot read Linux distribution identity: {e}"))?;
    plans::parse_os_release(&source)
}

pub(super) fn catalog() -> Result<Catalog, String> {
    let distro = distro()?;
    match distro {
        Distro::Debian => apt_catalog(),
        Distro::Fedora | Distro::Rhel => rpm_catalog(distro),
        Distro::Arch => arch_catalog(),
    }
}

fn check_apt_trust() -> Result<(), String> {
    let mut sources = vec![Path::new("/etc/apt/sources.list").to_path_buf()];
    if let Ok(entries) = fs::read_dir("/etc/apt/sources.list.d") {
        for entry in entries {
            let path = entry.map_err(|e| e.to_string())?.path();
            if matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("list" | "sources")
            ) {
                sources.push(path);
            }
        }
    }
    for path in sources {
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("Cannot inspect APT trust settings: {e}")),
        };
        if !apt_source_is_secure(&text) {
            return Err("APT sources contain a signature/trust override. Restore signed distro repository verification before using package actions.".into());
        }
    }
    Ok(())
}

fn apt_source_is_secure(source: &str) -> bool {
    source
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .all(|line| {
            let compact = line
                .to_ascii_lowercase()
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>();
            ![
                "trusted",
                "allow-insecure",
                "allow-weak",
                "allow-downgrade-to-insecure",
            ]
            .iter()
            .any(|key| {
                ["=", ":"].iter().any(|separator| {
                    ["yes", "true", "1"]
                        .iter()
                        .any(|value| compact.contains(&format!("{key}{separator}{value}")))
                })
            })
        })
}

fn apt_origins_are_distro(source: &str) -> bool {
    let mut pending = false;
    let mut found = false;
    for line in source.lines().map(str::trim) {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.first().is_some_and(|s| s.parse::<i32>().is_ok())
            && fields.last() == Some(&"Packages")
        {
            if pending {
                return false;
            }
            pending = true;
        } else if pending && line.starts_with("release ") {
            let origin = line["release ".len()..]
                .split(',')
                .find_map(|part| part.trim().strip_prefix("o="));
            if !origin.is_some_and(|o| plans::allowed_repo(Distro::Debian, o)) {
                return false;
            }
            pending = false;
            found = true;
        }
    }
    found && !pending
}

fn apt_catalog() -> Result<Catalog, String> {
    check_apt_trust()?;
    let policy = runtime::checked_read(&Plan::new("apt-cache", &["policy"]))?;
    // Fail closed if a third-party source could shadow the same version. Never edit sources.
    if !apt_origins_are_distro(&policy) {
        return Err("APT catalog must contain only signed Debian/Ubuntu repositories. Third-party or missing repository origins require external package management.".into());
    }
    let listing = runtime::checked_read(&Plan::new("apt-cache", &["madison", "mariadb-server"]))?;
    let versions = parse_apt_versions(&listing);
    let policy = runtime::checked_read(&Plan::new("apt-cache", &["policy", "mariadb-server"]))?;
    let value = |label: &str| {
        policy
            .lines()
            .find_map(|l| l.trim().strip_prefix(label))
            .map(str::trim)
            .filter(|v| plans::valid_package_version(v))
            .map(str::to_string)
    };
    Ok(Catalog {
        distro: Distro::Debian,
        candidate: value("Candidate:").filter(|v| versions.contains(v)),
        installed: value("Installed:"),
        versions,
        repos: vec![],
    })
}

fn parse_apt_versions(source: &str) -> Vec<String> {
    unique(source.lines().filter_map(|line| {
        let fields: Vec<_> = line.split('|').map(str::trim).collect();
        (fields.len() == 3 && fields[0] == "mariadb-server" && fields[2].ends_with("Packages"))
            .then(|| fields[1].to_string())
    }))
}

fn rpm_catalog(distro: Distro) -> Result<Catalog, String> {
    // cacheonly is deliberately read-only: no metadata refresh or module stream changes.
    let listing = runtime::checked_read(&Plan::new(
        "dnf",
        &[
            "--cacheonly",
            "--quiet",
            "repoquery",
            "--available",
            "--qf",
            "%{name}\t%{evr}\t%{repoid}\n",
            "mariadb-server",
        ],
    ))?;
    let (versions, repos) = parse_rpm_versions(distro, &listing);
    let mut repos: BTreeSet<_> = repos.into_iter().collect();
    let enabled = runtime::checked_read(&Plan::new(
        "dnf",
        &["--cacheonly", "--quiet", "repolist", "--enabled"],
    ))?;
    for id in enabled
        .lines()
        .filter_map(|line| line.split_whitespace().next())
    {
        if plans::allowed_repo(distro, id) {
            repos.insert(id.into());
        }
    }
    let installed = installed_version(distro)?;
    let candidate = versions.iter().max_by_key(|v| version_key(v)).cloned();
    Ok(Catalog {
        distro,
        versions,
        repos: repos.into_iter().collect(),
        candidate,
        installed,
    })
}

fn parse_rpm_versions(distro: Distro, source: &str) -> (Vec<String>, Vec<String>) {
    let mut repos = BTreeSet::new();
    let versions = unique(source.lines().filter_map(|line| {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 3
            || fields[0] != "mariadb-server"
            || !plans::allowed_repo(distro, fields[2])
        {
            return None;
        }
        repos.insert(fields[2].to_string());
        Some(fields[1].to_string())
    }));
    (versions, repos.into_iter().collect())
}

pub(super) fn is_upgrade(catalog: &Catalog, target: &str) -> Result<bool, String> {
    let Some(installed) = catalog.installed.as_deref() else {
        return Ok(false);
    };
    match catalog.distro {
        Distro::Debian => Ok(runtime::read(&Plan::new(
            "dpkg",
            &["--compare-versions", target, "gt", installed],
        ))?
        .success),
        Distro::Arch => {
            let result = runtime::checked_read(&Plan::new("vercmp", &[target, installed]))?;
            Ok(result
                .parse::<i32>()
                .map_err(|_| "Invalid pacman version comparison.")?
                > 0)
        }
        Distro::Fedora | Distro::Rhel => {
            let source = runtime::checked_read(&Plan::new(
                "dnf",
                &[
                    "--cacheonly",
                    "--quiet",
                    "repoquery",
                    "--upgrades",
                    "--qf",
                    "%{name}\t%{evr}\t%{repoid}\n",
                    "mariadb-server",
                ],
            ))?;
            Ok(parse_rpm_versions(catalog.distro, &source)
                .0
                .iter()
                .any(|v| v == target))
        }
    }
}

fn arch_catalog() -> Result<Catalog, String> {
    for args in [vec!["SigLevel"], vec!["--repo", "extra", "SigLevel"]] {
        let policy = runtime::checked_read(&Plan::new("pacman-conf", &args))?;
        if !arch_signatures_required(&policy) {
            return Err(
                "Arch package signatures must be Required and TrustedOnly for extra.".into(),
            );
        }
    }
    let source = runtime::checked_read(&Plan::new("pacman", &["-Si", "extra/mariadb"]))?;
    let versions = parse_arch_versions(&source);
    Ok(Catalog {
        distro: Distro::Arch,
        installed: installed_version(Distro::Arch)?,
        candidate: versions.first().cloned(),
        versions,
        repos: vec!["extra".into()],
    })
}

fn parse_arch_versions(source: &str) -> Vec<String> {
    let fields: std::collections::HashMap<_, _> = source
        .lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.trim(), value.trim()))
        .collect();
    if fields.get("Repository") != Some(&"extra") || fields.get("Name") != Some(&"mariadb") {
        return Vec::new();
    }
    unique(fields.get("Version").map(|v| (*v).to_string()).into_iter())
}

fn arch_signatures_required(source: &str) -> bool {
    let tokens: Vec<_> = source.split_whitespace().collect();
    tokens
        .iter()
        .any(|t| matches!(*t, "Required" | "PackageRequired"))
        && !tokens.iter().any(|t| {
            matches!(
                *t,
                "Never"
                    | "Optional"
                    | "PackageNever"
                    | "PackageOptional"
                    | "TrustAll"
                    | "PackageTrustAll"
            )
        })
}

pub(super) fn installed_version(distro: Distro) -> Result<Option<String>, String> {
    let plan = match distro {
        Distro::Debian => Plan::new(
            "dpkg-query",
            &[
                "-W",
                "-f=${db:Status-Status}\t${Version}\n",
                "mariadb-server",
            ],
        ),
        Distro::Fedora | Distro::Rhel => Plan::new(
            "rpm",
            &[
                "-q",
                "--qf",
                "%{EPOCHNUM}:%{VERSION}-%{RELEASE}\n",
                "mariadb-server",
            ],
        ),
        Distro::Arch => Plan::new("pacman", &["-Q", "mariadb"]),
    };
    let output = runtime::read(&plan)?;
    if !output.success {
        return Ok(None);
    }
    let value = match distro {
        Distro::Debian => output.stdout.strip_prefix("installed\t"),
        Distro::Arch => output.stdout.strip_prefix("mariadb "),
        _ => Some(output.stdout.as_str()),
    };
    Ok(value
        .filter(|v| plans::valid_package_version(v))
        .map(str::to_string))
}

fn unique(values: impl Iterator<Item = String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    values
        .filter(|v| {
            plans::valid_package_version(v)
                && plans::upstream_version(v).is_some()
                && seen.insert(v.clone())
        })
        .collect()
}

fn version_key(value: &str) -> Vec<u64> {
    value
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|n| n.parse().ok())
        .collect()
}

impl Catalog {
    pub fn select(&self, requested: Option<&str>) -> Result<String, String> {
        let selected = match requested {
            None => self.candidate.clone(),
            Some(requested) => {
                if !plans::valid_package_version(requested) {
                    return Err("Invalid MariaDB version selection.".into());
                }
                self.versions
                    .iter()
                    .filter(|v| {
                        v.as_str() == requested
                            || plans::upstream_version(v).as_deref() == Some(requested)
                            || plans::series(v).as_deref() == Some(requested)
                    })
                    .max_by_key(|v| version_key(v))
                    .cloned()
            }
        };
        selected.ok_or_else(|| "The selected MariaDB version is not available in the configured signed distro package cache. Refresh packages externally; no repository or module stream will be changed.".into())
    }
}

pub fn list_series() -> Result<Vec<MariaDBSeries>, String> {
    let catalog = catalog()?;
    let series: BTreeSet<_> = catalog
        .versions
        .iter()
        .filter_map(|v| plans::series(v))
        .collect();
    Ok(series
        .into_iter()
        .map(|series| MariaDBSeries {
            series,
            support: "Distribution repository".into(),
            eol: String::new(),
        })
        .collect())
}

pub fn list_releases(series: &str) -> Result<Vec<MariaDBRelease>, String> {
    if series.split('.').count() != 2 || !series.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return Err("Invalid MariaDB series.".into());
    }
    let catalog = catalog()?;
    Ok(catalog
        .versions
        .into_iter()
        .filter(|v| plans::series(v).as_deref() == Some(series))
        .map(|version| MariaDBRelease {
            version,
            date: String::new(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn apt_catalog_does_not_include_installed_only_or_source_packages() {
        assert_eq!(parse_apt_versions("mariadb-server | 1:10.11.6-1 | https://deb.debian.org/debian bookworm/main amd64 Packages\nmariadb-server | 10.5.1 | /var/lib/dpkg/status\nmariadb | 10.11.6 | Sources"), ["1:10.11.6-1"]);
        assert!(apt_origins_are_distro("500 https://deb.debian.org/debian bookworm/main amd64 Packages\n release v=12,o=Debian,a=stable,n=bookworm,l=Debian,c=main,b=amd64"));
        assert!(!apt_origins_are_distro(
            "500 https://example.invalid custom/main amd64 Packages\n release o=Custom"
        ));
    }
    #[test]
    fn signature_bypasses_are_rejected() {
        for source in [
            "deb [trusted=yes] https://example.org stable main",
            "Trusted: yes",
            "Allow-Insecure: yes",
            "Trusted: true",
            "deb [trusted=1] https://example.org stable main",
        ] {
            assert!(!apt_source_is_secure(source));
        }
        assert!(apt_source_is_secure("deb [signed-by=/usr/share/keyrings/debian-archive-keyring.gpg] https://deb.debian.org/debian stable main"));
        assert!(arch_signatures_required(
            "PackageRequired\nDatabaseOptional"
        ));
        assert!(!arch_signatures_required("Required PackageTrustAll"));
    }
    #[test]
    fn rpm_catalog_filters_third_party_repositories_and_keeps_epoch() {
        let (versions, repos) = parse_rpm_versions(Distro::Rhel,
            "mariadb-server\t3:10.5.27-1.el9\trhel-9-for-x86_64-appstream-rpms\nmariadb-server\t99.0.1-1\tcustom\nmysql-server\t8.4.0\tappstream");
        assert_eq!(versions, ["3:10.5.27-1.el9"]);
        assert_eq!(repos, ["rhel-9-for-x86_64-appstream-rpms"]);
    }
    #[test]
    fn selection_never_substitutes_an_unavailable_release() {
        let catalog = Catalog {
            distro: Distro::Debian,
            versions: vec!["1:10.11.6-1".into()],
            installed: None,
            candidate: Some("1:10.11.6-1".into()),
            repos: vec![],
        };
        assert_eq!(catalog.select(Some("10.11")).unwrap(), "1:10.11.6-1");
        assert!(catalog.select(Some("11.4")).is_err());
        assert!(catalog.select(Some("--allow-unauthenticated")).is_err());
    }
    #[test]
    fn arch_queries_only_the_requested_signed_repository_package() {
        assert_eq!(
            parse_arch_versions("Repository : extra\nName : mariadb\nVersion : 11.8.3-1\nDescription : MariaDB server"),
            ["11.8.3-1"]
        );
        assert_eq!(
            parse_arch_versions("Repository : extra\nName : mariadb\nVersion : 1:11.8.3-1"),
            ["1:11.8.3-1"]
        );
        for source in [
            "Repository : custom\nName : mariadb\nVersion : 11.8.3-1",
            "Repository : extra\nName : mysql\nVersion : 8.4.0-1",
            "Repository : extra\nName : mariadb\nVersion : --evil",
        ] {
            assert!(parse_arch_versions(source).is_empty());
        }
    }
}
