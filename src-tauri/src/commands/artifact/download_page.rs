use std::{io::Read, time::Duration};

use serde_json::Value;

pub(super) const LEGACY_PAGE: &str =
    "https://docs.fivem.net/docs/server-download/?platform=legacy&os=windows";
pub(super) const ENHANCED_PAGE: &str =
    "https://docs.fivem.net/docs/server-download/?platform=enhanced&os=windows";
const MAX_PAGE: u64 = 4 * 1024 * 1024;

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
    let mut bytes = Vec::new();
    response
        .take(MAX_PAGE + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Could not read official download data: {error}"))?;
    if bytes.len() as u64 > MAX_PAGE {
        return Err("Official download page exceeded the size limit.".into());
    }
    parse(std::str::from_utf8(&bytes).map_err(|_| "Official download page is not UTF-8.")?)
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
