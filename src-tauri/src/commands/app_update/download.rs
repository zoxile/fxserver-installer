use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use reqwest::{redirect::Policy, Client, Url};
use serde::Deserialize;

use super::super::app_release::{version, AppReleaseInfo};

pub(super) const TARGET: &str = "windows-x86_64";
const RELEASES: &str = "https://github.com/zoxile/fxserver-installer/releases/download";
const MAX_METADATA: usize = 256 * 1024;
const MAX_INSTALLER: usize = 256 * 1024 * 1024;

#[derive(Deserialize)]
struct Manifest {
    version: String,
    platforms: BTreeMap<String, Platform>,
}

#[derive(Deserialize, PartialEq)]
struct Platform {
    url: String,
    signature: String,
}

pub(super) fn newer_release(
    release: &AppReleaseInfo,
    current: &str,
    channel: &str,
) -> Result<bool, String> {
    let latest = version(&release.version).ok_or("The release has an invalid version.")?;
    let installed = version(current).ok_or("This application has an invalid version.")?;
    if release.tag_name != format!("v{}", release.version)
        || release.installer_url != installer_url(&release.version)
        || release.html_url
            != format!(
                "https://github.com/zoxile/fxserver-installer/releases/tag/{}",
                release.tag_name
            )
        || !matches!(channel, "beta" | "stable")
        || channel == "stable" && release.prerelease
    {
        return Err("The release does not match this updater channel or platform.".into());
    }
    Ok(latest > installed)
}

fn installer_url(version: &str) -> String {
    format!("{RELEASES}/v{version}/FXServer.Installer_{version}_windows_x64-setup.exe")
}

fn validate_manifest(value: &serde_json::Value, release: &AppReleaseInfo) -> Result<(), String> {
    let manifest: Manifest =
        serde_json::from_value(value.clone()).map_err(|_| "Invalid update feed format.")?;
    if manifest.version != release.version
        || manifest.platforms.is_empty()
        || manifest.platforms.len() > 2
    {
        return Err("The update feed does not match the selected release.".into());
    }
    let platform = manifest
        .platforms
        .get(TARGET)
        .ok_or("The update feed is missing the Windows NSIS installer.")?;
    if platform.url != release.installer_url
        || platform.url != installer_url(&release.version)
        || platform.signature.trim().is_empty()
        || platform.signature.len() > 4096
        || !platform
            .signature
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"+/=\r\n".contains(&c))
        || manifest.platforms.iter().any(|(key, value)| {
            !matches!(key.as_str(), TARGET | "windows-x86_64-nsis") || value != platform
        })
    {
        return Err("The update feed contains an untrusted installer or missing signature.".into());
    }
    Ok(())
}

fn trusted_download(url: &Url) -> bool {
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.fragment().is_some()
    {
        return false;
    }
    if url.host_str() == Some("release-assets.githubusercontent.com") {
        return true;
    }
    if url.host_str() != Some("github.com") || url.query().is_some() {
        return false;
    }
    let Some((tag, name)) = url
        .as_str()
        .strip_prefix(&format!("{RELEASES}/"))
        .and_then(|tail| tail.split_once('/'))
    else {
        return false;
    };
    let Some(number) = tag.strip_prefix('v') else {
        return false;
    };
    version(number).is_some() && (name == "latest.json" || url.as_str() == installer_url(number))
}

async fn bounded_download(
    url: Url,
    limit: usize,
    timeout: Duration,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<Vec<u8>, String> {
    if !trusted_download(&url) || url.host_str() != Some("github.com") {
        return Err("Untrusted update download endpoint.".into());
    }
    let client = Client::builder()
        .no_proxy()
        .user_agent("FXServer-Installer signed-updater")
        .connect_timeout(Duration::from_secs(10))
        .timeout(timeout)
        .redirect(Policy::custom(|attempt| {
            if attempt.previous().len() > 3 || !trusted_download(attempt.url()) {
                attempt.error("Untrusted update redirect")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|_| "Cannot initialize the update download client.")?;
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|_| "Could not download the update from GitHub. Try again later.")?;
    if !response.status().is_success() {
        return Err(format!(
            "GitHub returned HTTP {} while downloading the update.",
            response.status().as_u16()
        ));
    }
    let total = response.content_length();
    if total.is_some_and(|length| length == 0 || length > limit as u64) {
        return Err("The update download has an invalid or excessive size.".into());
    }
    let mut bytes = Vec::new();
    let mut last_emit = Instant::now();
    progress(0, total);
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "The update download was interrupted. Try downloading again.")?
    {
        append_chunk(&mut bytes, &chunk, limit)?;
        if last_emit.elapsed() >= Duration::from_millis(100) {
            progress(bytes.len() as u64, total);
            last_emit = Instant::now();
        }
    }
    if bytes.is_empty() || total.is_some_and(|length| length != bytes.len() as u64) {
        return Err("The update download is empty or incomplete.".into());
    }
    progress(bytes.len() as u64, total);
    Ok(bytes)
}

fn append_chunk(bytes: &mut Vec<u8>, chunk: &[u8], limit: usize) -> Result<(), String> {
    if chunk.len() > limit.saturating_sub(bytes.len()) {
        return Err("The update download exceeded its size limit.".into());
    }
    let needed = bytes.len() + chunk.len();
    if needed > bytes.capacity() {
        let capacity = bytes.capacity().saturating_mul(2).max(needed).min(limit);
        bytes
            .try_reserve_exact(capacity - bytes.len())
            .map_err(|_| "Not enough memory to download this update.")?;
    }
    bytes.extend_from_slice(chunk);
    Ok(())
}

pub(super) fn manifest_url(release: &AppReleaseInfo) -> Result<Url, String> {
    Url::parse(&format!("{RELEASES}/{}/latest.json", release.tag_name))
        .map_err(|_| "Invalid update metadata URL.".into())
}

pub(super) async fn manifest(release: &AppReleaseInfo) -> Result<serde_json::Value, String> {
    let url = manifest_url(release)?;
    let bytes = bounded_download(url, MAX_METADATA, Duration::from_secs(30), |_, _| {}).await?;
    let value = serde_json::from_slice(&bytes).map_err(|_| {
        "The release does not contain a valid signed updater feed. Install this release manually."
    })?;
    validate_manifest(&value, release)?;
    Ok(value)
}

pub(super) async fn installer(
    url: &Url,
    progress: impl FnMut(u64, Option<u64>),
) -> Result<Vec<u8>, String> {
    bounded_download(
        url.clone(),
        MAX_INSTALLER,
        Duration::from_secs(15 * 60),
        progress,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release() -> AppReleaseInfo {
        AppReleaseInfo {
            version: "0.6.0".into(),
            tag_name: "v0.6.0".into(),
            html_url: "https://github.com/zoxile/fxserver-installer/releases/tag/v0.6.0".into(),
            installer_url: installer_url("0.6.0"),
            prerelease: true,
        }
    }

    fn feed() -> serde_json::Value {
        serde_json::json!({"version": "0.6.0", "platforms": {TARGET: {"url": installer_url("0.6.0"), "signature": "c2lnbmF0dXJl"}}})
    }

    #[test]
    fn release_comparison_rejects_downgrades_cross_channel_and_other_platforms() {
        assert!(newer_release(&release(), "0.5.2", "beta").unwrap());
        assert!(!newer_release(&release(), "0.6.0", "beta").unwrap());
        assert!(!newer_release(&release(), "0.7.0", "beta").unwrap());
        assert!(newer_release(&release(), "0.5.2", "stable").is_err());
        assert!(newer_release(&release(), "0.5.2", "development").is_err());
        let mut wrong = release();
        wrong.installer_url = wrong
            .installer_url
            .replace("windows_x64-setup.exe", "linux_x64.AppImage");
        assert!(newer_release(&wrong, "0.5.2", "beta").is_err());
    }

    #[test]
    fn metadata_requires_the_exact_release_and_signed_nsis_asset() {
        assert!(validate_manifest(&feed(), &release()).is_ok());
        let mut dual = feed();
        dual["platforms"]["windows-x86_64-nsis"] = dual["platforms"][TARGET].clone();
        assert!(validate_manifest(&dual, &release()).is_ok());
        for signature in ["", " ", "bad\\signature", "https://example.com/signature"] {
            let mut value = feed();
            value["platforms"][TARGET]["signature"] = signature.into();
            assert!(validate_manifest(&value, &release()).is_err());
        }
        for url in [
            "https://evil.example/setup.exe".to_string(),
            installer_url("0.5.0"),
            format!("{}?download=1", installer_url("0.6.0")),
        ] {
            let mut value = feed();
            value["platforms"][TARGET]["url"] = url.into();
            assert!(validate_manifest(&value, &release()).is_err());
        }
        let mut value = feed();
        value["version"] = "99.0.0".into();
        assert!(validate_manifest(&value, &release()).is_err());
        value = feed();
        value["platforms"]["linux-x86_64"] = value["platforms"][TARGET].clone();
        assert!(validate_manifest(&value, &release()).is_err());
    }

    #[test]
    fn redirects_cannot_escape_githubs_release_cdn() {
        for url in [
            installer_url("0.6.0"),
            format!("{RELEASES}/v0.6.0/latest.json"),
            "https://release-assets.githubusercontent.com/a?token=example".into(),
        ] {
            assert!(trusted_download(&Url::parse(&url).unwrap()));
        }
        for url in ["http://release-assets.githubusercontent.com/a", "https://release-assets.githubusercontent.com.evil.test/a", "https://user@release-assets.githubusercontent.com/a", "https://release-assets.githubusercontent.com:8443/a", "https://github.com/other/repo/releases/download/v0.6.0/latest.json", "https://127.0.0.1/latest.json", "https://github.com/zoxile/fxserver-installer/releases/download/v0.6.0/latest.json#fragment", "https://github.com/zoxile/fxserver-installer/releases/download/v0.6.0/../latest.json"] {
            assert!(!trusted_download(&Url::parse(url).unwrap()), "{url}");
        }
    }

    #[test]
    fn unknown_length_downloads_cannot_exceed_the_limit() {
        let mut bytes = Vec::new();
        append_chunk(&mut bytes, b"1234", 6).unwrap();
        append_chunk(&mut bytes, b"56", 6).unwrap();
        assert!(append_chunk(&mut bytes, b"7", 6).is_err());
        assert_eq!(bytes, b"123456");
    }
}
