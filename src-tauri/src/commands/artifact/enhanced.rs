//! Enhanced URL validation and wire format adapted from Huntercorlett's fork (53f1834).
//! Discovery reads only the official page's matching host data; no cached fallback.

use crate::models::artifact::{ArtifactPlatform, EnhancedArtifactBuild, EnhancedArtifactCatalog};
use reqwest::{blocking::Client, redirect::Policy, Url};
use std::time::Duration;

use super::download_page;

const ARTIFACT_HOST: &str = "downloads.cfx-services.net";

pub(super) fn client(timeout: Duration) -> Result<Client, String> {
    Client::builder()
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(15))
        .redirect(Policy::none())
        .user_agent("fxserver-installer")
        .build()
        .map_err(|error| error.to_string())
}

pub(super) fn validate_enhanced_url(
    value: &str,
    platform: ArtifactPlatform,
) -> Result<String, String> {
    let artifact_file = platform.enhanced_archive();
    let invalid = || {
        format!("Use the official Enhanced {} URL: https://{ARTIFACT_HOST}/prod/<build-id>/{artifact_file}", platform.as_str())
    };
    let raw = value.trim();
    let url = Url::parse(raw).map_err(|_| invalid())?;
    if url.scheme() != "https"
        || url.host_str() != Some(ARTIFACT_HOST)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid());
    }
    let segments: Vec<_> = url.path().split('/').collect();
    let id = match segments.as_slice() {
        ["", "prod", id, file] if *file == artifact_file && is_build_id(id) => *id,
        _ => return Err(invalid()),
    };
    // Reject URL parser normalization of dot segments, ports, escapes, or backslashes.
    if raw != format!("https://{ARTIFACT_HOST}/prod/{id}/{artifact_file}") {
        return Err(invalid());
    }
    Ok(id.to_ascii_lowercase())
}

fn is_build_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
}

fn parse_catalog(
    data: &serde_json::Value,
    platform: ArtifactPlatform,
) -> Result<EnhancedArtifactCatalog, String> {
    let items = data
        .pointer(&format!("/props/pageProps/enhanced/{}", platform.as_str()))
        .and_then(|value| value.as_array())
        .filter(|items| !items.is_empty() && items.len() <= 32)
        .ok_or_else(|| {
            format!(
                "No Enhanced {} builds are currently published in the official download data.",
                platform.as_str()
            )
        })?;
    let mut builds: Vec<EnhancedArtifactBuild> = Vec::new();
    for item in items {
        let download_url = item
            .get("downloadURL")
            .and_then(|value| value.as_str())
            .ok_or("Missing Enhanced download URL.")?;
        let version = validate_enhanced_url(download_url, platform)?;
        if !builds.iter().any(|build| build.version == version) {
            builds.push(EnhancedArtifactBuild {
                version,
                download_url: download_url.into(),
            });
        }
    }
    Ok(EnhancedArtifactCatalog {
        builds,
        source_url: download_page::page(platform, true),
        warning: None,
    })
}

pub(super) fn load_catalog() -> Result<EnhancedArtifactCatalog, String> {
    let platform = ArtifactPlatform::host()?;
    parse_catalog(
        &download_page::load(&download_page::page(platform, true))?,
        platform,
    )
}

pub(super) fn validate_published_download(value: &str) -> Result<String, String> {
    let platform = ArtifactPlatform::host()?;
    let version = validate_enhanced_url(value, platform)?;
    // Never infer Linux availability by substituting a filename in a Windows URL.
    if platform == ArtifactPlatform::Linux
        && !load_catalog()?
            .builds
            .iter()
            .any(|build| build.version == version && build.download_url == value.trim())
    {
        return Err(
            "This Enhanced Linux build is not currently published in the official download data."
                .into(),
        );
    }
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;
    const WINDOWS: ArtifactPlatform = ArtifactPlatform::Windows;
    const URL: &str = "https://downloads.cfx-services.net/prod/00000000-0000-0000-0000-000000000001/cfx-server_win_x64.zip";
    #[test]
    fn strict_official_urls() {
        assert!(validate_enhanced_url(URL, WINDOWS).is_ok());
        for bad in [
            URL.replace("https:", "http:"),
            URL.replace(ARTIFACT_HOST, "evil.example"),
            format!("{URL}?x=1"),
            URL.replace("/prod/", "/x/../prod/"),
            URL.replace(ARTIFACT_HOST, &format!("{ARTIFACT_HOST}:443")),
            URL.replace("/prod/", "/prod%2f"),
            URL.replace("/prod/", "/prod\\"),
        ] {
            assert!(validate_enhanced_url(&bad, WINDOWS).is_err(), "{bad}");
        }
    }
    #[test]
    fn only_structured_windows_releases_no_stale_fallback() {
        let data = serde_json::json!({"props":{"pageProps":{"enhanced":{"windows":[{"downloadURL":URL},{"downloadURL":URL}],"linux":[{"downloadURL":"https://evil.example"}]}}}});
        let page =
            format!("<script id=\"__NEXT_DATA__\" type=\"application/json\">{data}</script>");
        assert_eq!(
            parse_catalog(&download_page::parse(&page).unwrap(), WINDOWS)
                .unwrap()
                .builds
                .len(),
            1
        );
        assert!(parse_catalog(&serde_json::json!({}), WINDOWS).is_err());
    }

    #[test]
    #[ignore = "contacts the official Cfx Enhanced download page"]
    fn live_official_enhanced_catalog() {
        let catalog = load_catalog().unwrap();
        assert!(!catalog.builds.is_empty());
        for build in &catalog.builds {
            assert_eq!(
                validate_enhanced_url(&build.download_url, ArtifactPlatform::host().unwrap())
                    .unwrap(),
                build.version
            );
        }
        println!(
            "Live Enhanced catalog: {} published Windows build(s).",
            catalog.builds.len()
        );
    }

    #[test]
    fn linux_requires_an_explicit_matching_official_release() {
        let linux = ArtifactPlatform::Linux;
        let linux_url = URL.replace(WINDOWS.enhanced_archive(), linux.enhanced_archive());
        assert!(validate_enhanced_url(URL, linux).is_err());
        assert!(validate_enhanced_url(&linux_url, WINDOWS).is_err());
        let mut data = serde_json::json!({"props":{"pageProps":{"enhanced":{"windows":[{"downloadURL":URL}]}}}});
        assert!(parse_catalog(&data, linux).is_err());
        data["props"]["pageProps"]["enhanced"]["linux"] =
            serde_json::json!([{ "downloadURL": URL }]);
        assert!(parse_catalog(&data, linux).is_err());
        data["props"]["pageProps"]["enhanced"]["linux"] =
            serde_json::json!([{ "downloadURL": linux_url }]);
        let catalog = parse_catalog(&data, linux).unwrap();
        assert_eq!(catalog.builds[0].download_url, linux_url);
        assert!(catalog.source_url.ends_with("os=linux"));
    }
}
