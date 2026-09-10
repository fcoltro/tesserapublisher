//! Asking GitHub what the newest release is.
//!
//! The one place in Tessera that reaches the network, and it exists here rather
//! than in `tessera_ui` for two reasons: a library that needs an HTTP client
//! for one request a day is a library carrying a TLS stack for everybody who
//! links it, and the decisions worth testing — which version is newer, whether
//! to ask at all, what to do with an answer nobody can read — are all in
//! [`tessera_ui::update`], which has no network to make its tests flaky.
//!
//! Everything here returns `None` on trouble. A version check is not something
//! anybody asked for, so it must never produce an error message, a retry, or a
//! delay somebody notices.

/// Where the releases are, and what a person is sent to.
const FEED: &str = "https://api.github.com/repos/fcoltro/tesserapublisher/releases/latest";

/// How long to wait before giving up.
///
/// Short on purpose. This runs on a thread of its own, so a long wait would not
/// freeze anything — but a thread still holding a socket open while somebody
/// works for an hour is a thread that will answer about a release that no
/// longer matters, and the next launch will ask again anyway.
const PATIENCE: std::time::Duration = std::time::Duration::from_secs(10);

/// The newest release: its version, and the page to send somebody to.
///
/// Blocking, and meant to be. [`tessera_ui::update::Check::begin`] runs it on a
/// thread and takes the answer whenever it arrives.
pub fn newest() -> Option<(String, String)> {
    let mut response = ureq::get(FEED)
        // GitHub refuses a request with no user agent, and asks that it name
        // the application. Without this the answer is a 403 that looks exactly
        // like being rate-limited.
        .header("User-Agent", "Tessera-Publisher")
        // Pinned, so that a future default the API changes to does not change
        // what the fields below mean.
        .header("Accept", "application/vnd.github+json")
        .config()
        .timeout_global(Some(PATIENCE))
        .build()
        .call()
        .ok()?;

    // Capped explicitly at 64K. A release feed is a few kilobytes; anything
    // larger is not one, and reading an unbounded body into memory on the
    // strength of a URL is how one bad response becomes a crash. Said here
    // rather than left to a default, because the default is not this crate's to
    // rely on.
    let body = response
        .body_mut()
        .with_config()
        .limit(64 * 1024)
        .read_to_string()
        .ok()?;
    read(&body)
}

/// Pull the version and the page out of what the feed said.
///
/// Separate from the request so it can be tested against a real response body
/// without one.
fn read(body: &str) -> Option<(String, String)> {
    let feed: serde_json::Value = serde_json::from_str(body).ok()?;
    let tag = feed.get("tag_name")?.as_str()?.to_string();
    // The release page, and only if the feed gave one. Constructing a URL here
    // would mean guessing — and a notice that sends somebody to a page that
    // does not exist is worse than no notice.
    let page = feed.get("html_url")?.as_str()?.to_string();
    // A draft is not released, and a prerelease is not what somebody running a
    // stable build should be told to install.
    if feed.get("draft").and_then(serde_json::Value::as_bool) == Some(true)
        || feed.get("prerelease").and_then(serde_json::Value::as_bool) == Some(true)
    {
        return None;
    }
    Some((tag, page))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_gives_its_tag_and_its_page() {
        let (tag, page) = read(
            r#"{"tag_name": "v1.4.0",
                "html_url": "https://github.com/fcoltro/tesserapublisher/releases/tag/v1.4.0",
                "draft": false, "prerelease": false}"#,
        )
        .expect("a release");
        assert_eq!(tag, "v1.4.0");
        assert!(page.ends_with("v1.4.0"));
    }

    #[test]
    fn a_prerelease_is_not_offered() {
        // Somebody running a stable build did not ask to test anything.
        assert_eq!(
            read(r#"{"tag_name": "v2.0.0-rc1", "html_url": "https://x", "prerelease": true}"#),
            None
        );
    }

    #[test]
    fn a_draft_is_not_offered() {
        assert_eq!(
            read(r#"{"tag_name": "v2.0.0", "html_url": "https://x", "draft": true}"#),
            None
        );
    }

    #[test]
    fn a_release_with_no_page_is_no_answer() {
        // **Rather than a guessed URL.** A notice that sends somebody to a page
        // that does not exist has cost them a click and told them nothing.
        assert_eq!(read(r#"{"tag_name": "v1.0.0"}"#), None);
    }

    #[test]
    fn nothing_readable_is_no_answer() {
        // A 404 body, an HTML error page from a captive portal, an empty
        // response: all the same, and none of them interrupt anybody.
        for body in ["", "not json", "{}", "[]", r#"{"message": "Not Found"}"#] {
            assert_eq!(read(body), None, "{body:?} was read as a release");
        }
    }
}
