//! Rockbox version info.

use std::time::Duration;

/// Fetch the latest Rockbox release version from the GitHub mirror's tags
/// API (not behind the Anubis bot-wall). Returns e.g. `"4.0"`.
pub fn latest_version() -> Option<String> {
    let resp = ureq::get("https://api.github.com/repos/Rockbox/rockbox/tags?per_page=1")
        .set("User-Agent", "rockbox-tui")
        .timeout(Duration::from_secs(5))
        .call()
        .ok()?;
    let json: serde_json::Value = resp.into_json().ok()?;
    let tag = json.get(0)?.get("name")?.as_str()?;
    Some(clean_tag(tag))
}

/// Turn a Rockbox release tag like `v4.0-final` into a display string `4.0`.
fn clean_tag(tag: &str) -> String {
    tag.trim_start_matches('v').replace("-final", "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_tags() {
        assert_eq!(clean_tag("v4.0-final"), "4.0");
        assert_eq!(clean_tag("v3.15-final"), "3.15");
        assert_eq!(clean_tag("4.0-final"), "4.0");
    }
}
