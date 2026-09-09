//! Checking whether a newer version exists.
//!
//! ## It checks, and never installs
//!
//! Downloading and replacing a running application is a different feature with
//! a much longer list of ways to go wrong — a half-written binary, a
//! signature nobody verified, an update that lands mid-save. This tells
//! somebody there is a newer version and where to get it. Deciding to take it
//! stays theirs.
//!
//! ## Nothing here reaches the network
//!
//! Comparing versions and deciding whether to ask are the parts that can be
//! wrong in a way nobody notices, so they live here where they are tested. The
//! fetch is one function the caller supplies, which is also what keeps this
//! crate free of an HTTP client it would otherwise need for one request a week.

use serde::{Deserialize, Serialize};

/// How long between checks.
///
/// A day. Often enough that somebody hears about a fix in the week it ships,
/// rare enough that it is not a request every time the application starts —
/// and a check on every launch is a check that tells a network administrator
/// how often somebody opens their layout tool.
pub const BETWEEN_CHECKS: std::time::Duration = std::time::Duration::from_secs(60 * 60 * 24);

/// A version, as three numbers.
///
/// Parsed rather than compared as text, because "0.10.0" is older than "0.9.0"
/// as a string and newer as a version — and a string comparison would tell
/// everybody on the newest release that they were behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    /// Read `1.2.3`, with or without a leading `v`.
    ///
    /// Anything else is `None` rather than a guess. A version this cannot read
    /// is one it must not compare, and treating an unreadable answer as "you
    /// are up to date" is the safe direction: it says nothing rather than
    /// something wrong.
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.trim().trim_start_matches(['v', 'V']);
        let mut parts = text.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        // A trailing pre-release or build tag is dropped: `1.2.3-rc1` is
        // compared as `1.2.3`, which is the closest true thing this can say
        // without a full semver implementation for one comparison.
        let patch = parts
            .next()
            .unwrap_or("0")
            .split(['-', '+'])
            .next()?
            .parse()
            .ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Version {
            major,
            minor,
            patch,
        })
    }

    /// The version this build is.
    pub fn running() -> Option<Version> {
        Version::parse(env!("CARGO_PKG_VERSION"))
    }
}

/// What a check found.
#[derive(Debug, Clone, PartialEq)]
pub enum Found {
    /// Nothing newer.
    UpToDate,
    /// A newer version, and where it is.
    Newer { version: Version, url: String },
    /// The check could not be made or could not be read.
    ///
    /// **Not an error anybody is shown.** A version check that interrupts
    /// somebody because a server was slow has cost them more than it could ever
    /// have saved them.
    Unknown,
}

/// What the application remembers between checks.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Checking {
    /// Whether to check at all.
    ///
    /// Defaults **on**, and it is the one thing here worth a preference: a
    /// check is a request to a server carrying an implicit "somebody is using
    /// this, now", and anybody who would rather not send that should not have
    /// to find out it is happening.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// When the last check happened, as seconds since the epoch.
    ///
    /// Wall clock rather than an `Instant`, because it has to survive the
    /// application closing — which is the only interval that matters.
    #[serde(default)]
    pub last_checked: u64,
    /// What the last check found, so it can be shown again without asking
    /// again.
    #[serde(default)]
    pub seen: Option<String>,
}

fn yes() -> bool {
    true
}

impl Checking {
    /// Whether a check is due.
    pub fn due(&self, now: u64) -> bool {
        self.enabled && now.saturating_sub(self.last_checked) >= BETWEEN_CHECKS.as_secs()
    }

    /// Record that a check happened, whatever it found.
    ///
    /// **Recorded even when it failed.** A check that only counted successes
    /// would retry every launch while a server was down, which is the moment it
    /// is least welcome.
    pub fn checked(&mut self, now: u64, found: &Found) {
        self.last_checked = now;
        self.seen = match found {
            Found::Newer { version, .. } => Some(format!(
                "{}.{}.{}",
                version.major, version.minor, version.patch
            )),
            _ => None,
        };
    }
}

/// Read what a release feed said.
///
/// Takes the text rather than a URL, so the decision is testable and the fetch
/// belongs to the caller. `latest` is a version string; `url` is where a person
/// would go to get it.
pub fn compare(latest: &str, url: &str, running: Option<Version>) -> Found {
    let (Some(latest), Some(running)) = (Version::parse(latest), running) else {
        return Found::Unknown;
    };
    if latest > running {
        Found::Newer {
            version: latest,
            url: url.to_string(),
        }
    } else {
        Found::UpToDate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(major: u32, minor: u32, patch: u32) -> Version {
        Version {
            major,
            minor,
            patch,
        }
    }

    #[test]
    fn ten_is_newer_than_nine() {
        // **The reason versions are parsed rather than compared as text.**
        // "0.10.0" sorts before "0.9.0" as a string, so a string comparison
        // would tell everybody on the newest release that they were behind —
        // and keep telling them.
        assert!(v(0, 10, 0) > v(0, 9, 0));
        assert!(Version::parse("0.10.0") > Version::parse("0.9.0"));
    }

    #[test]
    fn a_leading_v_is_read_off() {
        // Tags carry it; version strings do not.
        assert_eq!(Version::parse("v1.2.3"), Some(v(1, 2, 3)));
        assert_eq!(Version::parse("1.2.3"), Some(v(1, 2, 3)));
    }

    #[test]
    fn a_pre_release_compares_as_its_release() {
        // The closest true thing this can say without a full semver
        // implementation for one comparison a day.
        assert_eq!(Version::parse("1.2.3-rc1"), Some(v(1, 2, 3)));
    }

    #[test]
    fn something_unreadable_is_not_a_version() {
        // And must not be compared. Treating an unreadable answer as "you are
        // up to date" says nothing rather than something wrong.
        for text in ["", "latest", "1", "1.2.3.4", "one.two.three"] {
            assert_eq!(Version::parse(text), None, "{text:?} was read as a version");
        }
    }

    #[test]
    fn an_unreadable_answer_asks_nobody_anything() {
        assert_eq!(
            compare("latest", "https://example", Some(v(1, 0, 0))),
            Found::Unknown
        );
    }

    #[test]
    fn a_newer_version_is_reported_with_where_to_get_it() {
        // A notice saying "there is a newer version" and not where is a notice
        // that makes somebody go and look.
        let found = compare("1.3.0", "https://example/releases", Some(v(1, 2, 9)));
        match found {
            Found::Newer { version, url } => {
                assert_eq!(version, v(1, 3, 0));
                assert!(!url.is_empty());
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_same_version_is_up_to_date() {
        assert_eq!(
            compare("1.2.3", "https://example", Some(v(1, 2, 3))),
            Found::UpToDate
        );
    }

    #[test]
    fn an_older_release_does_not_ask_anybody_to_downgrade() {
        // Somebody running a build newer than the feed is a developer, and
        // telling them to install something older is telling them to undo
        // their own work.
        assert_eq!(
            compare("1.0.0", "https://example", Some(v(2, 0, 0))),
            Found::UpToDate
        );
    }

    #[test]
    fn a_check_is_not_due_the_moment_after_one_happened() {
        let mut checking = Checking {
            enabled: true,
            ..Default::default()
        };
        checking.checked(1_000_000, &Found::UpToDate);
        assert!(!checking.due(1_000_060));
        assert!(checking.due(1_000_000 + BETWEEN_CHECKS.as_secs()));
    }

    #[test]
    fn a_failed_check_still_counts_as_a_check() {
        // Otherwise it retries every launch while a server is down, which is
        // the moment it is least welcome.
        let mut checking = Checking {
            enabled: true,
            ..Default::default()
        };
        checking.checked(500, &Found::Unknown);
        assert!(!checking.due(600));
    }

    #[test]
    fn switching_it_off_stops_it() {
        let checking = Checking {
            enabled: false,
            last_checked: 0,
            seen: None,
        };
        assert!(!checking.due(u64::MAX / 2));
    }

    #[test]
    fn it_is_on_unless_somebody_turns_it_off() {
        // An opt-in check is one nobody opts into, and the people who most need
        // to hear about a fix are the ones who never open preferences.
        let fresh: Checking = serde_json::from_str("{}").expect("read");
        assert!(fresh.enabled);
    }

    #[test]
    fn this_build_knows_what_version_it_is() {
        // If it does not, every comparison is `Unknown` and the whole thing
        // quietly does nothing.
        assert!(Version::running().is_some());
    }
}
