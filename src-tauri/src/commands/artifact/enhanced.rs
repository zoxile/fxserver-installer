//! Enhanced URL validation and wire format adapted from Huntercorlett's fork (53f1834).
//! Discovery reads only the official page's structured Windows data; no cached fallback.

use crate::models::artifact::{EnhancedArtifactBuild, EnhancedArtifactCatalog};
use reqwest::{blocking::Client, redirect::Policy, Url};
use std::{io::Read, time::Duration};

pub(super) const ENHANCED_PAGE: &str =
    "https://docs.fivem.net/docs/server-download/?platform=enhanced&os=windows";
const ARTIFACT_HOST: &str = "downloads.cfx-services.net";
const ARTIFACT_FILE: &str = "cfx-server_win_x64.zip";
const MAX_PAGE: u64 = 4 * 1024 * 1024;

pub(super) fn client(timeout: Duration) -> Result<Client, String> {
    Client::builder()
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(15))
        .redirect(Policy::none())
        .user_agent("fxserver-installer")
        .build()
        .map_err(|error| error.to_string())
}

pub(super) fn validate_enhanced_url(value: &str) -> Result<String, String> {
    let invalid = || {
        "Use the official Enhanced Windows URL: https://downloads.cfx-services.net/prod/<build-id>/cfx-server_win_x64.zip".to_string()
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
        ["", "prod", id, file] if *file == ARTIFACT_FILE && is_build_id(id) => *id,
        _ => return Err(invalid()),
    };
    // Reject URL parser normalization of dot segments, ports, escapes, or backslashes.
    if raw != format!("https://{ARTIFACT_HOST}/prod/{id}/{ARTIFACT_FILE}") {
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

fn parse_catalog(page: &str) -> Result<EnhancedArtifactCatalog, String> {
    let start = page.find("id=\"__NEXT_DATA__\"").or_else(|| page.find("id='__NEXT_DATA__'"))
        .ok_or("Official download data was not found. Open the official page and paste its Windows download link.")?;
    let tail = &page[start..];
    let tail = &tail[tail.find('>').ok_or("Invalid official download page.")? + 1..];
    let json = &tail[..tail
        .find("</script>")
        .ok_or("Incomplete official download page.")?];
    let data: serde_json::Value = serde_json::from_str(json)
        .map_err(|error| format!("Invalid official download data: {error}"))?;
    let windows = data
        .pointer("/props/pageProps/enhanced/windows")
        .and_then(|value| value.as_array())
        .filter(|items| !items.is_empty() && items.len() <= 32)
        .ok_or(
            "No Enhanced Windows builds are currently published in the official download data.",
        )?;
    let mut builds: Vec<EnhancedArtifactBuild> = Vec::new();
    for item in windows {
        let download_url = item
            .get("downloadURL")
            .and_then(|value| value.as_str())
            .ok_or("Missing Enhanced download URL.")?;
        let version = validate_enhanced_url(download_url)?;
        if !builds.iter().any(|build| build.version == version) {
            builds.push(EnhancedArtifactBuild {
                version,
                download_url: download_url.into(),
            });
        }
    }
    Ok(EnhancedArtifactCatalog {
        builds,
        source_url: ENHANCED_PAGE.into(),
        warning: None,
    })
}

pub(super) fn load_catalog() -> Result<EnhancedArtifactCatalog, String> {
    let response = client(Duration::from_secs(30))?
        .get(ENHANCED_PAGE)
        .send()
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "Official download page returned {}.",
            response.status()
        ));
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_PAGE + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_PAGE {
        return Err("Official download page exceeded the size limit.".into());
    }
    parse_catalog(std::str::from_utf8(&bytes).map_err(|e| e.to_string())?)
}

#[cfg(test)]
mod tests {
    use super::*;
    const URL: &str = "https://downloads.cfx-services.net/prod/00000000-0000-0000-0000-000000000001/cfx-server_win_x64.zip";
    #[test]
    fn strict_official_urls() {
        assert!(validate_enhanced_url(URL).is_ok());
        for bad in [
            URL.replace("https:", "http:"),
            URL.replace(ARTIFACT_HOST, "evil.example"),
            format!("{URL}?x=1"),
            URL.replace("/prod/", "/x/../prod/"),
            URL.replace(ARTIFACT_HOST, &format!("{ARTIFACT_HOST}:443")),
            URL.replace("/prod/", "/prod%2f"),
            URL.replace("/prod/", "/prod\\"),
        ] {
            assert!(validate_enhanced_url(&bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn only_structured_windows_releases_no_stale_fallback() {
        let data = serde_json::json!({"props":{"pageProps":{"enhanced":{"windows":[{"downloadURL":URL},{"downloadURL":URL}],"linux":[{"downloadURL":"https://evil.example"}]}}}});
        let page =
            format!("<script id=\"__NEXT_DATA__\" type=\"application/json\">{data}</script>");
        assert_eq!(parse_catalog(&page).unwrap().builds.len(), 1);
        assert!(parse_catalog(URL).is_err());
        assert!(parse_catalog("<script id=\"__NEXT_DATA__\">{}</script>").is_err());
    }
}
