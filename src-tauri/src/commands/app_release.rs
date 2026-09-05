use std::{
    io::Read,
    sync::Mutex,
    time::{Duration, Instant},
};

use quick_xml::{events::Event, Reader};
use reqwest::{
    blocking::{Client, Response},
    header::LOCATION,
    redirect::Policy,
    Url,
};
use serde::{Deserialize, Serialize};

const RELEASES: &str = "https://github.com/zoxile/fxserver-installer/releases";
const API: &str = "https://api.github.com/repos/zoxile/fxserver-installer/releases?per_page=10";
const MAX_BODY: usize = 1024 * 1024;
const NO_RELEASE: &str = "Could not verify a published Windows installer for this release channel. Try again later or inspect the project's GitHub releases.";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppReleaseInfo {
    version: String,
    tag_name: String,
    html_url: String,
    installer_url: String,
    prerelease: bool,
}

type Cached = Option<(Instant, bool, Result<AppReleaseInfo, String>)>;
static CACHE: Mutex<Cached> = Mutex::new(None);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleasePolicy {
    beta_versions: Vec<String>,
}

#[tauri::command]
pub async fn fetch_latest_app_release(
    app: tauri::AppHandle,
    force: bool,
) -> Result<AppReleaseInfo, String> {
    let current = app.package_info().version.to_string();
    super::run_blocking(move || {
        let policy: ReleasePolicy =
            serde_json::from_str(include_str!("../../../release-policy.json"))
                .map_err(|_| "Release policy is invalid.".to_string())?;
        let beta = policy.beta_versions.contains(&current);
        let mut cache = CACHE.lock().map_err(|_| "Release check is unavailable.")?;
        if let Some((time, channel, result)) = &*cache {
            if cache_fresh(time.elapsed(), *channel == beta, result.is_ok(), force) {
                return result.clone();
            }
        }
        let result = lookup(beta, &policy.beta_versions);
        *cache = Some((Instant::now(), beta, result.clone()));
        result
    })
    .await
}

fn cache_fresh(age: Duration, same_channel: bool, success: bool, force: bool) -> bool {
    !force && same_channel && age < Duration::from_secs(if success { 60 } else { 10 })
}

fn version(tag: &str) -> Option<[u32; 3]> {
    let parts: Vec<_> = tag.strip_prefix('v').unwrap_or(tag).split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let mut result = [0; 3];
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty()
            || part.len() > 9
            || part.len() > 1 && part.starts_with('0')
            || !part.bytes().all(|c| c.is_ascii_digit())
        {
            return None;
        }
        result[index] = part.parse().ok()?;
    }
    Some(result)
}

fn tag_from_url(value: &str) -> Option<String> {
    let tag = value.strip_prefix(&format!("{RELEASES}/tag/"))?;
    version(tag)?;
    Some(tag.into())
}

fn installer_url(value: &str, tag: &str) -> Option<String> {
    version(tag)?;
    let value = if value.starts_with('/') {
        format!("https://github.com{value}")
    } else {
        value.into()
    };
    let number = tag.strip_prefix('v').unwrap_or(tag);
    let expected =
        format!("{RELEASES}/download/{tag}/FXServer.Installer_{number}_windows_x64-setup.exe");
    (value == expected).then_some(value)
}

fn info(tag: &str, url: String, prerelease: bool) -> AppReleaseInfo {
    AppReleaseInfo {
        version: tag.strip_prefix('v').unwrap_or(tag).into(),
        tag_name: tag.into(),
        html_url: format!("{RELEASES}/tag/{tag}"),
        installer_url: url,
        prerelease,
    }
}

fn allowed(tag: &str, prerelease: bool, beta: bool, policy: &[String]) -> bool {
    version(tag).is_some()
        && (beta
            || !prerelease
                && !policy
                    .iter()
                    .any(|item| item == tag.strip_prefix('v').unwrap_or(tag)))
}

struct Lookup {
    client: Client,
    deadline: Instant,
    requests: usize,
}

impl Lookup {
    fn request(&mut self, url: &str, head: bool) -> Result<Response, String> {
        if !(url == API
            || url == format!("{RELEASES}/latest")
            || url == format!("{RELEASES}.atom")
            || url
                .strip_prefix(&format!("{RELEASES}/expanded_assets/"))
                .is_some_and(|tag| version(tag).is_some())
            || url
                .strip_prefix(&format!("{RELEASES}/download/"))
                .and_then(|part| part.split_once('/'))
                .is_some_and(|(tag, _)| installer_url(url, tag).is_some()))
        {
            return Err("Unexpected release URL.".into());
        }
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() || self.requests >= 16 {
            return Err(NO_RELEASE.into());
        }
        self.requests += 1;
        let request = if head {
            self.client.head(url)
        } else {
            self.client.get(url)
        };
        request
            .timeout(remaining.min(Duration::from_secs(5)))
            .send()
            .map_err(|_| NO_RELEASE.into())
    }

    fn body(&mut self, url: &str) -> Result<String, String> {
        let response = self.request(url, false)?;
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|size| size > MAX_BODY as u64)
        {
            return Err(NO_RELEASE.into());
        }
        read_body(response)
    }

    fn verify_asset(&mut self, url: &str) -> bool {
        self.request(url, true).is_ok_and(|response| {
            asset_response(
                response.status().as_u16(),
                response
                    .headers()
                    .get(LOCATION)
                    .and_then(|v| v.to_str().ok()),
                response.content_length(),
            )
        })
    }
}

fn read_body(reader: impl Read) -> Result<String, String> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_BODY as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| NO_RELEASE)?;
    if bytes.len() > MAX_BODY {
        return Err(NO_RELEASE.into());
    }
    String::from_utf8(bytes).map_err(|_| NO_RELEASE.into())
}

fn asset_response(status: u16, location: Option<&str>, length: Option<u64>) -> bool {
    if status == 200 {
        return length.is_some_and(|size| size > 0);
    }
    if !matches!(status, 302 | 303 | 307 | 308) {
        return false;
    }
    location
        .and_then(|value| Url::parse(value).ok())
        .is_some_and(|url| {
            url.scheme() == "https"
                && url.host_str() == Some("release-assets.githubusercontent.com")
                && url.username().is_empty()
                && url.password().is_none()
                && url.port().is_none()
                && url.fragment().is_none()
        })
}

fn lookup(beta: bool, policy: &[String]) -> Result<AppReleaseInfo, String> {
    let client = Client::builder()
        .user_agent("FXServer-Installer release-check")
        .no_proxy()
        .connect_timeout(Duration::from_secs(3))
        .redirect(Policy::none())
        .build()
        .map_err(|_| NO_RELEASE)?;
    let mut http = Lookup {
        client,
        deadline: Instant::now() + Duration::from_secs(20),
        requests: 0,
    };
    let latest = http
        .request(&format!("{RELEASES}/latest"), true)
        .ok()
        .and_then(|response| {
            if !matches!(response.status().as_u16(), 302 | 303 | 307 | 308) {
                return None;
            }
            response
                .headers()
                .get(LOCATION)
                .and_then(|v| v.to_str().ok())
                .and_then(tag_from_url)
        });
    let mut tags = if beta {
        http.body(&format!("{RELEASES}.atom"))
            .and_then(|body| atom_tags(&body))
            .unwrap_or_default()
    } else {
        vec![]
    };
    if let Some(tag) = &latest {
        tags.push(tag.clone());
    }
    tags.sort_by_key(|tag| std::cmp::Reverse(version(tag)));
    tags.dedup();
    for tag in tags
        .into_iter()
        .filter(|tag| allowed(tag, false, beta, policy))
        .take(4)
    {
        let url = http
            .body(&format!("{RELEASES}/expanded_assets/{tag}"))
            .ok()
            .and_then(|body| asset_link(&body, &tag).ok().flatten());
        if let Some(url) = url.filter(|url| http.verify_asset(url)) {
            let prerelease = latest.as_deref() != Some(&tag)
                || policy.contains(&tag.trim_start_matches('v').to_string());
            return Ok(info(&tag, url, prerelease));
        }
    }
    // The API is a fallback, not a prerequisite for the public HTML/Atom route.
    let mut releases = api_releases(&http.body(API)?, beta, policy)?;
    releases.sort_by_key(|release| std::cmp::Reverse(version(&release.tag_name)));
    releases
        .into_iter()
        .take(4)
        .find(|release| http.verify_asset(&release.installer_url))
        .ok_or_else(|| NO_RELEASE.into())
}

fn atom_tags(body: &str) -> Result<Vec<String>, String> {
    let mut reader = Reader::from_str(body);
    let mut stack = Vec::new();
    let mut tags = Vec::new();
    loop {
        match reader.read_event().map_err(|_| NO_RELEASE)? {
            Event::Start(node) => {
                if stack.len() >= 64 {
                    return Err(NO_RELEASE.into());
                }
                stack.push(node.name().as_ref().to_vec());
            }
            Event::Empty(node)
                if stack == [b"feed".to_vec(), b"entry".to_vec()]
                    && node.name().as_ref() == b"link" =>
            {
                let mut href = None;
                let mut alternate = false;
                for attr in node.attributes() {
                    let attr = attr.map_err(|_| NO_RELEASE)?;
                    if attr.key.as_ref() == b"rel" {
                        alternate = attr.value.as_ref() == b"alternate";
                    }
                    if attr.key.as_ref() == b"href" {
                        href = std::str::from_utf8(&attr.value).ok().and_then(tag_from_url);
                    }
                }
                if alternate {
                    if let Some(tag) = href {
                        if tags.len() < 10 {
                            tags.push(tag);
                        }
                    }
                }
            }
            Event::End(_) => {
                stack.pop();
            }
            Event::DocType(_) => return Err(NO_RELEASE.into()),
            Event::Eof => break,
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err(NO_RELEASE.into());
    }
    Ok(tags)
}

fn asset_link(body: &str, tag: &str) -> Result<Option<String>, String> {
    let mut reader = Reader::from_str(body);
    reader.config_mut().check_end_names = false;
    let mut found = None;
    loop {
        match reader.read_event().map_err(|_| NO_RELEASE)? {
            Event::Start(node) | Event::Empty(node) if node.name().as_ref() == b"a" => {
                for attr in node.html_attributes() {
                    let attr = attr.map_err(|_| NO_RELEASE)?;
                    if attr.key.as_ref() == b"href" {
                        if let Some(url) = std::str::from_utf8(&attr.value)
                            .ok()
                            .and_then(|value| installer_url(value, tag))
                        {
                            found = Some(url);
                        }
                    }
                }
            }
            Event::DocType(_) => return Err(NO_RELEASE.into()),
            Event::Eof => return Ok(found),
            _ => {}
        }
    }
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    html_url: String,
    draft: bool,
    prerelease: bool,
    published_at: Option<String>,
    assets: Vec<ApiAsset>,
}
#[derive(Deserialize)]
struct ApiAsset {
    browser_download_url: String,
    state: String,
    size: u64,
}

fn api_releases(body: &str, beta: bool, policy: &[String]) -> Result<Vec<AppReleaseInfo>, String> {
    let releases: Vec<ApiRelease> = serde_json::from_str(body).map_err(|_| NO_RELEASE)?;
    Ok(releases
        .into_iter()
        .take(10)
        .filter_map(|release| {
            if release.draft
                || release.published_at.as_deref().is_none_or(str::is_empty)
                || !allowed(&release.tag_name, release.prerelease, beta, policy)
                || tag_from_url(&release.html_url).as_deref() != Some(&release.tag_name)
            {
                return None;
            }
            let url = release
                .assets
                .into_iter()
                .filter(|asset| asset.state == "uploaded" && asset.size > 0)
                .find_map(|asset| installer_url(&asset.browser_download_url, &release.tag_name))?;
            let prerelease = release.prerelease
                || policy.contains(&release.tag_name.trim_start_matches('v').to_string());
            Some(info(&release.tag_name, url, prerelease))
        })
        .collect())
}

#[cfg(test)]
#[path = "app_release_tests.rs"]
mod tests;
