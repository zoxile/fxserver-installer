use std::{
    collections::BTreeMap,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::models::artifact::{
    ArtifactBuild, ArtifactCatalog, ArtifactIssue, ArtifactMetadata, ArtifactPlatform,
};

use super::download_page;

const CACHE_SECONDS: u64 = 15 * 60;
struct CatalogCache {
    catalog: Option<ArtifactCatalog>,
    metadata: Option<(ArtifactMetadata, u64)>,
}
static CACHE: Mutex<CatalogCache> = Mutex::new(CatalogCache {
    catalog: None,
    metadata: None,
});

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub(super) fn load_catalog(refresh: bool) -> Result<ArtifactCatalog, String> {
    let platform = ArtifactPlatform::host()?;
    let mut cache = CACHE
        .lock()
        .map_err(|_| "Artifact catalog cache is unavailable.")?;
    if let Some(cached) = cache
        .catalog
        .as_ref()
        .filter(|cached| !refresh && now().saturating_sub(cached.fetched_at) < CACHE_SECONDS)
    {
        return Ok(cached.clone());
    }
    let builds = match fetch_builds(platform) {
        Ok(builds) => builds,
        Err(error) => {
            return cache
                .catalog
                .as_ref()
                .map(|cached| {
                    let mut stale = cached.clone();
                    stale.stale = true;
                    stale.warning = Some(format!(
                        "Official listing refresh failed. Showing cached builds. {error}"
                    ));
                    stale
                })
                .ok_or(error);
        }
    };
    let warning = match super::fetch_artifact_metadata_blocking() {
        Ok(metadata) => { cache.metadata = Some((metadata, now())); None }
        Err(error) => Some(format!("JG issue metadata refresh failed. Cached reports, when available, are retained; other build health is unknown. {error}")),
    };
    let mut builds = annotate(
        builds,
        cache.metadata.as_ref().map(|(metadata, _)| metadata),
    );
    if warning.is_some() {
        for build in &mut builds {
            if build.issues.is_empty() {
                build.health = "unknown".into();
            }
        }
    }
    let catalog = ArtifactCatalog {
        builds,
        fetched_at: now(),
        metadata_fetched_at: cache.metadata.as_ref().map(|(_, fetched_at)| *fetched_at),
        stale: warning.is_some(),
        warning,
    };
    cache.catalog = Some(catalog.clone());
    Ok(catalog)
}

fn fetch_builds(platform: ArtifactPlatform) -> Result<Vec<ArtifactBuild>, String> {
    parse_catalog(
        &download_page::load(&download_page::page(platform, false))?,
        platform,
    )
}

fn parse_catalog(
    data: &serde_json::Value,
    platform: ArtifactPlatform,
) -> Result<Vec<ArtifactBuild>, String> {
    let mut links = Vec::new();
    for branch in ["recommended", "latest"] {
        let items = data
            .pointer(&format!(
                "/props/pageProps/legacy/{branch}/{}",
                platform.as_str()
            ))
            .and_then(|value| value.as_array())
            .filter(|items| !items.is_empty() && items.len() <= 4096)
            .ok_or_else(|| {
                format!(
                    "Official download data is missing a complete Legacy {} build list.",
                    platform.as_str()
                )
            })?;
        for item in items {
            let link = item
                .get("downloadURL")
                .and_then(|value| value.as_str())
                .ok_or("Official build is missing its download URL.")?;
            links.push(link.to_string());
        }
    }
    parse_links(&links, platform)
}

fn parse_links(links: &[String], platform: ArtifactPlatform) -> Result<Vec<ArtifactBuild>, String> {
    let base = reqwest::Url::parse(platform.legacy_base()).map_err(|error| error.to_string())?;
    let mut builds = BTreeMap::new();
    for link in links {
        let Ok(url) = base.join(link) else { continue };
        let Some(path) = url.as_str().strip_prefix(platform.legacy_base()) else {
            continue;
        };
        let Some((directory, archive)) = path.split_once('/') else {
            continue;
        };
        if archive != platform.legacy_archive()
            && !(platform == ArtifactPlatform::Windows && archive == "server.7z")
        {
            continue;
        }
        let Some((version, hash)) = directory.split_once('-') else {
            continue;
        };
        if !valid_version(version)
            || hash.len() != 40
            || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            continue;
        }
        let download_url = format!(
            "{}{directory}/{}",
            platform.legacy_base(),
            platform.legacy_archive()
        );
        validate_download(&download_url, version, platform)?;
        builds
            .entry(version.parse::<u64>().unwrap())
            .or_insert(ArtifactBuild {
                version: version.to_string(),
                download_url,
                health: "unknown".into(),
                issues: vec![],
                recommended: false,
            });
    }
    if builds.is_empty() {
        return Err(format!(
            "The official listing contained no {} artifact builds.",
            platform.as_str()
        ));
    }
    Ok(builds.into_values().rev().collect())
}

fn valid_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 10
        && version.bytes().all(|byte| byte.is_ascii_digit())
        && version.parse::<u64>().is_ok_and(|value| value > 0)
}

pub(super) fn validate_download(
    value: &str,
    version: &str,
    platform: ArtifactPlatform,
) -> Result<(), String> {
    let invalid = || {
        format!(
            "Choose a valid official {} artifact download and matching build number.",
            platform.as_str()
        )
    };
    if !valid_version(version) {
        return Err(invalid());
    }
    let url = reqwest::Url::parse(value).map_err(|_| invalid())?;
    if url.scheme() != "https"
        || url.host_str() != Some("runtime.fivem.net")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid());
    }
    let path = value
        .strip_prefix(platform.legacy_base())
        .ok_or_else(invalid)?;
    let directory = path
        .strip_suffix(&format!("/{}", platform.legacy_archive()))
        .ok_or_else(invalid)?;
    let (build, hash) = directory.split_once('-').ok_or_else(invalid)?;
    if build != version || hash.len() != 40 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid());
    }
    Ok(())
}

fn issue_matches(version: &str, issue: &ArtifactIssue) -> bool {
    let Ok(version) = version.parse::<u64>() else {
        return false;
    };
    let range: Vec<_> = issue.artifact.split('-').map(str::trim).collect();
    match range.as_slice() {
        [single] => single.parse::<u64>().ok() == Some(version),
        [start, end] => start
            .parse::<u64>()
            .ok()
            .zip(end.parse::<u64>().ok())
            .is_some_and(|(start, end)| start <= version && version <= end),
        _ => false,
    }
}

fn annotate(
    mut builds: Vec<ArtifactBuild>,
    metadata: Option<&ArtifactMetadata>,
) -> Vec<ArtifactBuild> {
    for build in &mut builds {
        if let Some(metadata) = metadata {
            build.recommended = build.version == metadata.recommended_artifact;
            build.issues = metadata
                .broken_artifacts
                .iter()
                .filter(|issue| issue_matches(&build.version, issue))
                .cloned()
                .collect();
            build.health = if !build.issues.is_empty() {
                "known-issue"
            } else if build.recommended {
                "healthy"
            } else {
                "unknown"
            }
            .into();
        }
    }
    builds
}

pub(super) fn validate_install_risk(version: &str, acknowledged: bool) -> Result<(), String> {
    // Recheck reports at install time, including when the browser used cached metadata.
    let metadata = super::fetch_artifact_metadata_blocking();
    let risk = requires_acknowledgement(version, metadata.as_ref().ok());
    if risk && !acknowledged {
        return Err("This build has known issues or unverified health. Refresh the catalog and confirm its warning before installing.".into());
    }
    Ok(())
}

fn requires_acknowledgement(version: &str, metadata: Option<&ArtifactMetadata>) -> bool {
    metadata
        .map(|metadata| {
            version != metadata.recommended_artifact
                || metadata
                    .broken_artifacts
                    .iter()
                    .any(|issue| issue_matches(version, issue))
        })
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    const WINDOWS: ArtifactPlatform = ArtifactPlatform::Windows;
    fn link(version: &str) -> String {
        format!("./{version}-0123456789abcdef0123456789abcdef01234567/server.7z")
    }

    #[test]
    fn new_download_page_includes_latest_and_recommended_windows_builds() {
        let data = serde_json::json!({"props":{"pageProps":{
            "legacy": {
                "recommended": {"windows": [{"downloadURL": link("35245")}]},
                "latest": {"windows": [
                    {"downloadURL": link("35945")},
                    {"downloadURL": link("35245")},
                    {"downloadURL": link("35186")}
                ], "linux": [{"downloadURL": link("99999")}]}
            },
            "enhanced": {"windows": [{"downloadURL": link("88888")}]}
        }}});
        let page = format!(
            "<a href='{}'>Download</a><script id=\"__NEXT_DATA__\">{data}</script>",
            link("35245")
        );
        let builds = parse_catalog(&download_page::parse(&page).unwrap(), WINDOWS).unwrap();
        assert_eq!(
            builds
                .iter()
                .map(|build| build.version.as_str())
                .collect::<Vec<_>>(),
            ["35945", "35245", "35186"]
        );
        assert!(parse_catalog(&serde_json::json!({}), WINDOWS).is_err());
        let mut missing_latest = data.clone();
        missing_latest["props"]["pageProps"]["legacy"]["latest"]["windows"] = serde_json::json!([]);
        assert!(parse_catalog(&missing_latest, WINDOWS).is_err());
    }

    #[test]
    #[ignore = "contacts the official Cfx download page and JG metadata"]
    fn live_official_catalog_has_multiple_builds_and_jg_annotations() {
        let catalog = load_catalog(true).unwrap();
        assert!(catalog.builds.len() > 1);
        assert!(!catalog.stale, "{:?}", catalog.warning);
        assert!(catalog.builds.iter().any(|build| build.recommended));
        for build in &catalog.builds {
            validate_download(
                &build.download_url,
                &build.version,
                ArtifactPlatform::host().unwrap(),
            )
            .unwrap();
        }
        println!(
            "Live catalog: {} official Windows builds, JG metadata loaded.",
            catalog.builds.len()
        );
    }

    #[test]
    fn official_links_are_sorted_deduplicated_and_confined() {
        let builds = parse_links(
            &[
                link("10268"),
                link("8509"),
                link("10268"),
                "..".into(),
                "https://evil.test/123/server.zip".into(),
                "./../build_proot_linux/123/server.zip".into(),
            ],
            WINDOWS,
        )
        .unwrap();
        assert_eq!(
            builds
                .iter()
                .map(|build| build.version.as_str())
                .collect::<Vec<_>>(),
            ["10268", "8509"]
        );
        assert!(builds
            .iter()
            .all(|build| build.download_url.ends_with("/server.zip")));
    }

    #[test]
    fn download_validation_rejects_host_tricks_mismatches_and_paths() {
        let good = format!(
            "{}123-0123456789abcdef0123456789abcdef01234567/server.zip",
            WINDOWS.legacy_base()
        );
        assert!(validate_download(&good, "123", WINDOWS).is_ok());
        for url in [
            good.replace("runtime.fivem.net", "runtime.fivem.net.evil.test"),
            good.replace("https://", "https://evil.test/"),
            format!("{good}?x=1"),
            good.replace("/server.zip", "/../server.zip"),
        ] {
            assert!(validate_download(&url, "123", WINDOWS).is_err(), "{url}");
        }
        assert!(validate_download(&good, "124", WINDOWS).is_err());
        assert!(validate_download(&good, "../../bad", WINDOWS).is_err());
    }

    #[test]
    fn issue_ranges_overlap_and_unknown_is_not_healthy() {
        let metadata = ArtifactMetadata {
            recommended_artifact: "10310".into(),
            windows_download_link: "unused".into(),
            linux_download_link: None,
            broken_artifacts: vec![
                ArtifactIssue {
                    artifact: "10268-10309".into(),
                    reason: "range".into(),
                },
                ArtifactIssue {
                    artifact: "10309".into(),
                    reason: "single".into(),
                },
            ],
        };
        let builds = parse_links(
            &[
                link("10311"),
                link("10310"),
                link("10309"),
                link("10268"),
                link("10267"),
            ],
            WINDOWS,
        )
        .unwrap();
        let result = annotate(builds.clone(), Some(&metadata));
        assert_eq!(
            result
                .iter()
                .map(|build| build.health.as_str())
                .collect::<Vec<_>>(),
            [
                "unknown",
                "healthy",
                "known-issue",
                "known-issue",
                "unknown"
            ]
        );
        assert_eq!(result[2].issues.len(), 2);
        assert!(!requires_acknowledgement("10310", Some(&metadata)));
        assert!(requires_acknowledgement("10311", Some(&metadata)));
        assert!(requires_acknowledgement("10309", Some(&metadata)));
        assert!(requires_acknowledgement("10310", None));
        assert!(annotate(builds, None)
            .iter()
            .all(|build| build.health == "unknown"));
    }

    #[test]
    fn linux_catalog_uses_only_linux_lists_and_tar_urls() {
        let linux = ArtifactPlatform::Linux;
        let linux_link = |version: &str| {
            format!(
                "{}{version}-0123456789abcdef0123456789abcdef01234567/fx.tar.xz",
                linux.legacy_base()
            )
        };
        let data = serde_json::json!({"props":{"pageProps":{"legacy":{
            "recommended":{"linux":[{"downloadURL":linux_link("101")}], "windows":[{"downloadURL":link("999")}]},
            "latest":{"linux":[{"downloadURL":linux_link("102")}], "windows":[{"downloadURL":link("999")}]}
        }}}});
        let builds = parse_catalog(&data, linux).unwrap();
        assert_eq!(
            builds
                .iter()
                .map(|b| b.version.as_str())
                .collect::<Vec<_>>(),
            ["102", "101"]
        );
        for build in builds {
            assert!(validate_download(&build.download_url, &build.version, linux).is_ok());
            assert!(validate_download(&build.download_url, &build.version, WINDOWS).is_err());
        }
        let windows_url = format!(
            "{}101-0123456789abcdef0123456789abcdef01234567/server.zip",
            WINDOWS.legacy_base()
        );
        assert!(validate_download(&windows_url, "101", linux).is_err());
        assert!(parse_links(&[windows_url], linux).is_err());
        let mut missing = data.clone();
        missing["props"]["pageProps"]["legacy"]["latest"]["linux"] = serde_json::json!([]);
        assert!(parse_catalog(&missing, linux).is_err());
    }
}
