use std::{io::Read, time::Duration};

use crate::models::artifact::ArtifactPlatform;
use serde_json::Value;

const MAX_PAGE: u64 = 4 * 1024 * 1024;

pub(super) fn page(platform: ArtifactPlatform, enhanced: bool) -> String {
    format!(
        "https://docs.fivem.net/docs/server-download/?platform={}&os={}",
        if enhanced { "enhanced" } else { "legacy" },
        platform.as_str()
    )
}

pub(super) fn load(url: &str) -> Result<Value, String> {
    let response = super::enhanced::client(Duration::from_secs(30))?
        .get(url)
        .send()
        .map_err(|error| format!("Could not load official download data: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Official download page returned {}.",
            response.status()
        ));
    }
    let bytes = read_bounded(response, MAX_PAGE)?;
    parse(std::str::from_utf8(&bytes).map_err(|_| "Official download page is not UTF-8.")?)
}

pub(super) fn read_bounded(reader: impl Read, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Could not read download data: {error}"))?;
    if bytes.len() as u64 > limit {
        return Err("Download data exceeded the size limit.".into());
    }
    Ok(bytes)
}

pub(super) fn parse(page: &str) -> Result<Value, String> {
    if page.len() as u64 > MAX_PAGE {
        return Err("Official download page exceeded the size limit.".into());
    }
    let start = page
        .find("id=\"__NEXT_DATA__\"")
        .or_else(|| page.find("id='__NEXT_DATA__'"))
        .ok_or(
            "Official download data was not found. Open Official Downloads to check availability.",
        )?;
    let tail = &page[start..];
    let tail = &tail[tail.find('>').ok_or("Invalid official download page.")? + 1..];
    let json = &tail[..tail
        .find("</script>")
        .ok_or("Incomplete official download page.")?];
    serde_json::from_str(json).map_err(|_| "Invalid official download data.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_reader_checks_streamed_size_without_content_length() {
        assert_eq!(read_bounded(&b"1234"[..], 4).unwrap(), b"1234");
        assert!(read_bounded(&b"12345"[..], 4).is_err());
    }

    #[test]
    fn reads_only_bounded_structured_data_not_the_visible_default_link() {
        for quote in ['\'', '"'] {
            let page = format!(
                "<a href='default-only'>Download</a><script id={quote}__NEXT_DATA__{quote} type='application/json'>{{\"props\":{{\"pageProps\":{{}}}}}}</script>"
            );
            assert!(parse(&page).unwrap().pointer("/props/pageProps").is_some());
        }
        for bad in [
            "<a href='default-only'>Download</a>",
            "<script id='__NEXT_DATA__'>{}</script-missing>",
            "<script id='__NEXT_DATA__'>not-json</script>",
        ] {
            assert!(parse(bad).is_err());
        }
        assert!(parse(&" ".repeat(MAX_PAGE as usize + 1)).is_err());
    }
}
