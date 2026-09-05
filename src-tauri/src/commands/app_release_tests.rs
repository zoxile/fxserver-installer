use super::*;
use serde_json::json;

fn asset(tag: &str) -> String {
    format!(
        "{RELEASES}/download/{tag}/FXServer.Installer_{}_windows_x64-setup.exe",
        tag.trim_start_matches('v')
    )
}

fn release(tag: &str) -> serde_json::Value {
    json!({ "tag_name": tag, "html_url": format!("{RELEASES}/tag/{tag}"), "draft": false,
        "prerelease": false, "published_at": "2026-09-05T12:00:00Z",
        "assets": [{ "browser_download_url": asset(tag), "state": "uploaded", "size": 12345 }] })
}

#[test]
fn canonical_urls_reject_other_hosts_tags_paths_and_asset_types() {
    assert_eq!(
        tag_from_url(&format!("{RELEASES}/tag/v0.4.0")).as_deref(),
        Some("v0.4.0")
    );
    assert_eq!(
        installer_url(&asset("v0.4.0"), "v0.4.0"),
        Some(asset("v0.4.0"))
    );
    assert_eq!(
        installer_url(
            asset("v0.4.0").trim_start_matches("https://github.com"),
            "v0.4.0"
        ),
        Some(asset("v0.4.0"))
    );
    for value in [
        asset("v0.4.0").replace("github.com", "github.com.evil.test"),
        asset("v0.4.0").replace("github.com", "github.com@evil.test"),
        asset("v0.4.0").replace("zoxile", "another-owner"),
        asset("v0.4.0").replace("https:", "http:"),
        asset("v0.4.0").replace("windows_x64", "windows_arm64"),
        asset("v0.4.0").replace("/v0.4.0/", "/v0.3.2/"),
        format!("{}?download=1", asset("v0.4.0")),
        format!("{}#fragment", asset("v0.4.0")),
        format!("{RELEASES}/download/../download/v0.4.0/installer.exe"),
    ] {
        assert!(installer_url(&value, "v0.4.0").is_none(), "{value}");
    }
    for tag in [
        "v0.4.0-beta.1",
        "v01.2.3",
        "v0.4.0/extra",
        "v0.4.0?x",
        "v0.4.0%2F",
        "0.4",
        "4294967296.0.0",
    ] {
        assert!(version(tag).is_none(), "{tag}");
    }
}

#[test]
fn main_version_or_tag_without_uploaded_installer_cannot_be_advertised() {
    assert!(api_releases(r#"{"version":"0.4.0"}"#, true, &[]).is_err());
    let valid = release("v0.4.0");
    for replacement in [
        json!([]),
        json!([{ "browser_download_url": asset("v0.4.0"), "state": "new", "size": 123 }]),
        json!([{ "browser_download_url": asset("v0.4.0"), "state": "uploaded", "size": 0 }]),
    ] {
        let mut candidate = valid.clone();
        candidate["assets"] = replacement;
        assert!(api_releases(&json!([candidate]).to_string(), true, &[])
            .unwrap()
            .is_empty());
    }
    for (key, value) in [
        ("draft", json!(true)),
        ("published_at", json!(null)),
        ("html_url", json!("https://evil.test/release")),
    ] {
        let mut candidate = valid.clone();
        candidate[key] = value;
        assert!(api_releases(&json!([candidate]).to_string(), true, &[])
            .unwrap()
            .is_empty());
    }
}

#[test]
fn stable_channel_excludes_github_prereleases_and_numeric_policy_betas() {
    let policy = vec!["0.4.0".into()];
    let mut prerelease = release("v0.5.0");
    prerelease["prerelease"] = json!(true);
    let data = json!([prerelease, release("v0.4.0"), release("v0.3.2")]).to_string();
    let stable = api_releases(&data, false, &policy).unwrap();
    assert_eq!(stable.len(), 1);
    assert_eq!(stable[0].version, "0.3.2");
    let beta = api_releases(&data, true, &policy).unwrap();
    assert_eq!(beta.len(), 3);
    assert!(beta[0].prerelease && beta[1].prerelease && !beta[2].prerelease);
    assert!(!allowed("v0.4.0", false, false, &policy));
    assert!(allowed("v0.4.0", true, true, &policy));
}

#[test]
fn atom_only_accepts_direct_canonical_release_links_not_escaped_notes() {
    let body = format!(
        r#"<?xml version="1.0"?><feed xmlns="http://www.w3.org/2005/Atom">
      <entry><link rel="alternate" href="{RELEASES}/tag/v0.4.0"/>
      <content>&lt;link rel="alternate" href="{RELEASES}/tag/v99.0.0"/&gt;</content></entry>
      <entry><link href="https://evil.test/releases/tag/v9.0.0" rel="alternate"/></entry>
      <entry><link href="{RELEASES}/tag/v0.3.2" rel="alternate"/></entry></feed>"#
    );
    assert_eq!(atom_tags(&body).unwrap(), ["v0.4.0", "v0.3.2"]);
    assert!(atom_tags("<!DOCTYPE feed SYSTEM 'file:///secret'><feed/>").is_err());
    assert!(atom_tags("<feed><entry></feed>").is_err());
}

#[test]
fn assets_fragment_requires_real_anchor_and_matching_installer() {
    let url = asset("v0.4.0");
    let body = format!(
        r#"<div><ul><li><a data-turbo="false" href="{}" rel="nofollow"><span>Installer</span></a></li></ul><div data-clipboard-copy-feedback></div></div>"#,
        url.trim_start_matches("https://github.com")
    );
    assert_eq!(asset_link(&body, "v0.4.0").unwrap(), Some(url.clone()));
    assert!(asset_link(&body, "v0.3.2").unwrap().is_none());
    for body in [
        format!("<div>{url}</div>"),
        format!(r#"<!-- <a href="{url}">Download</a> -->"#),
        format!(r#"<div>&lt;a href="{url}"&gt;Download&lt;/a&gt;</div>"#),
        format!(r#"<a href="{url}.sig">Signature</a>"#),
    ] {
        assert!(asset_link(&body, "v0.4.0").unwrap().is_none());
    }
}

#[test]
fn availability_probe_does_not_follow_arbitrary_redirects() {
    assert!(asset_response(200, None, Some(123)));
    assert!(!asset_response(200, None, Some(0)));
    assert!(!asset_response(404, None, Some(123)));
    assert!(asset_response(302, Some("https://release-assets.githubusercontent.com/github-production-release-asset/fixture?signature=value"), None));
    for url in [
        "https://evil.test/file.exe",
        "https://release-assets.githubusercontent.com.evil.test/file.exe",
        "http://release-assets.githubusercontent.com/file.exe",
        "https://user@release-assets.githubusercontent.com/file.exe",
    ] {
        assert!(!asset_response(302, Some(url), None));
    }
}

#[test]
fn response_size_and_request_budget_are_bounded_before_network_access() {
    assert!(read_body(&vec![b'x'; MAX_BODY + 1][..]).is_err());
    assert!(read_body(&[0xff][..]).is_err());
    let mut lookup = Lookup {
        client: Client::builder().redirect(Policy::none()).build().unwrap(),
        deadline: Instant::now(),
        requests: 0,
    };
    assert!(lookup.request(API, false).is_err());
    assert_eq!(lookup.requests, 0);
    lookup.deadline = Instant::now() + Duration::from_secs(20);
    assert!(lookup.request("https://evil.test", false).is_err());
    assert_eq!(lookup.requests, 0);
    lookup.requests = 16;
    assert!(lookup.request(API, false).is_err());
}

#[test]
fn force_and_channel_changes_bypass_success_and_failure_caches() {
    assert!(cache_fresh(Duration::from_secs(59), true, true, false));
    assert!(!cache_fresh(Duration::from_secs(60), true, true, false));
    assert!(cache_fresh(Duration::from_secs(9), true, false, false));
    assert!(!cache_fresh(Duration::from_secs(10), true, false, false));
    for success in [false, true] {
        assert!(!cache_fresh(Duration::ZERO, true, success, true));
        assert!(!cache_fresh(Duration::ZERO, false, success, false));
    }
}

#[test]
#[ignore = "Opt-in read-only checks against public GitHub release metadata and asset HEAD"]
fn live_published_release_lookup() {
    let client = Client::builder()
        .user_agent("FXServer-Installer release-check")
        .no_proxy()
        .connect_timeout(Duration::from_secs(3))
        .redirect(Policy::none())
        .build()
        .unwrap();
    let mut http = Lookup {
        client,
        deadline: Instant::now() + Duration::from_secs(20),
        requests: 0,
    };
    let feed = http
        .body(&format!("{RELEASES}.atom"))
        .expect("Live Atom response");
    let tags = atom_tags(&feed).expect("Live Atom parsing");
    assert!(!tags.is_empty());
    let tag = &tags[0];
    let assets = http
        .body(&format!("{RELEASES}/expanded_assets/{tag}"))
        .expect("Live asset HTML");
    let url = asset_link(&assets, tag)
        .expect("Live HTML parsing")
        .expect("Published installer anchor");
    assert!(
        http.verify_asset(&url),
        "Live HEAD must verify the canonical asset without following its redirect"
    );
    println!(
        "Live Atom: {} entries; asset HTML: {} bytes; verified HEAD: {url}",
        tags.len(),
        assets.len()
    );
    let latest = http
        .request(&format!("{RELEASES}/latest"), true)
        .expect("Live stable redirect");
    let latest_tag = latest
        .headers()
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .and_then(tag_from_url)
        .expect("Canonical stable tag redirect");
    println!(
        "Live stable route: HTTP {} -> {latest_tag}",
        latest.status()
    );
    let policy = vec!["0.4.0".into()];
    for beta in [false, true] {
        let found = lookup(beta, &policy).expect("Published release lookup");
        println!(
            "Live {} lookup: {} -> {}",
            if beta { "beta" } else { "stable" },
            found.version,
            found.installer_url
        );
    }
}
