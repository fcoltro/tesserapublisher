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

/// Now, as seconds since the epoch.
///
/// Zero if the clock is set before 1970, which is a machine whose clock cannot
/// be trusted to measure a day anyway — and zero makes a check due, which
/// errs towards asking rather than towards silently never asking again.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// A check running on a thread of its own, and what it found.
///
/// **On a thread, because the alternative is a frozen window.** A release feed
/// is a request to a server that may be slow, unreachable, or behind a captive
/// portal that answers eventually; any of those blocking the frame would make
/// the application appear to hang on launch, which is a worse first impression
/// than being a version behind.
///
/// The notice outlives the answer: `found` is kept so the bar can stay up until
/// somebody deals with it, rather than appearing for the one frame the answer
/// arrived in.
#[derive(Default)]
pub struct Check {
    /// The thread's answer, while it is still coming.
    from: Option<std::sync::mpsc::Receiver<Found>>,
    /// What it found, once.
    found: Option<Found>,
    /// Whether the notice has been dismissed.
    ///
    /// Not written to preferences. `Checking::seen` already records the version
    /// that was found, so a dismissal that outlived the session would need to
    /// agree with it about *which* version was dismissed — two places holding
    /// one fact. Within a session this is enough; across sessions the daily
    /// interval is what stops it being a nag.
    dismissed: bool,
}

impl Check {
    /// Start a check, if one is due.
    ///
    /// `fetch` returns the newest version and where to get it. It is supplied
    /// by the caller rather than written here so that this crate needs no HTTP
    /// client for one request a day — and so that everything that can be wrong
    /// in a way nobody notices stays in a module with tests.
    ///
    /// Nothing is started when a check is not due, which includes the case of
    /// it being switched off. That is the switch doing what it says.
    pub fn begin<F>(checking: &Checking, now: u64, fetch: F) -> Check
    where
        F: FnOnce() -> Option<(String, String)> + Send + 'static,
    {
        if !checking.due(now) {
            return Check::default();
        }
        let running = Version::running();
        let (to, from) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let found = match fetch() {
                Some((latest, url)) => compare(&latest, &url, running),
                None => Found::Unknown,
            };
            // The receiver is gone if the application closed while the request
            // was in flight. A check nobody is waiting for is not a failure.
            let _ = to.send(found);
        });
        Check {
            from: Some(from),
            found: None,
            dismissed: false,
        }
    }

    /// Take the answer if it has arrived, and record that a check happened.
    ///
    /// Returns whether `checking` changed, so the caller knows whether the
    /// preferences are worth writing. Called every frame; does nothing on
    /// almost all of them.
    pub fn settle(&mut self, checking: &mut Checking, now: u64) -> bool {
        use std::sync::mpsc::TryRecvError;
        let Some(from) = self.from.as_ref() else {
            return false;
        };
        let found = match from.try_recv() {
            Ok(found) => found,
            Err(TryRecvError::Empty) => return false,
            // The thread went away without answering. That is `Unknown` — and
            // recording it stops this polling a dead channel forever.
            Err(TryRecvError::Disconnected) => Found::Unknown,
        };
        self.from = None;
        checking.checked(now, &found);
        self.found = Some(found);
        true
    }

    /// The newer version to tell somebody about, if there is one they have not
    /// dismissed.
    pub fn newer(&self) -> Option<(Version, &str)> {
        if self.dismissed {
            return None;
        }
        match self.found.as_ref()? {
            Found::Newer { version, url } => Some((*version, url)),
            _ => None,
        }
    }

    /// Put the notice away for this session.
    pub fn dismiss(&mut self) {
        self.dismissed = true;
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

    /// Poll until the check settles, or give up.
    ///
    /// **A deadline rather than a number of attempts.** `try_recv` is fast
    /// enough that ten thousand of them finish before the operating system has
    /// scheduled the thread at all, so the first version of this failed on a
    /// fast machine and would have passed on a slow one — exactly backwards for
    /// a test about a thread.
    fn settled(check: &mut Check, checking: &mut Checking, now: u64) -> bool {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if check.settle(checking, now) {
                return true;
            }
            std::thread::yield_now();
        }
        false
    }

    #[test]
    fn a_check_that_is_not_due_starts_no_thread() {
        // **The switch has to actually do something.** A preference stored and
        // never read is the failure this whole wiring exists to fix, so the
        // off position is checked here rather than assumed.
        let off = Checking {
            enabled: false,
            ..Default::default()
        };
        // Long past the interval, so the *only* reason nothing happens is the
        // switch. A `now` inside the first day would have passed this test
        // whether the switch worked or not.
        let now = BETWEEN_CHECKS.as_secs() * 10;
        let mut check = Check::begin(&off, now, || {
            panic!("a check was made with the setting switched off")
        });
        let mut checking = off.clone();
        assert!(!check.settle(&mut checking, now));
        assert_eq!(checking, off);
        assert!(check.newer().is_none());
    }

    #[test]
    fn a_newer_version_arrives_and_is_remembered() {
        let mut checking = Checking {
            enabled: true,
            ..Default::default()
        };
        // Past the interval. A fresh `Checking` has `last_checked` at zero, so
        // a `now` inside the first day is *not* due — which is the module being
        // right and cost this test one run to notice.
        let now = BETWEEN_CHECKS.as_secs() + 9_000;
        let mut check = Check::begin(&checking, now, || {
            Some(("99.0.0".to_string(), "https://example/releases".to_string()))
        });

        assert!(
            settled(&mut check, &mut checking, now),
            "the answer never arrived"
        );

        assert_eq!(checking.last_checked, now);
        assert_eq!(checking.seen.as_deref(), Some("99.0.0"));
        let (version, url) = check.newer().expect("a newer version");
        assert_eq!(version, v(99, 0, 0));
        assert_eq!(url, "https://example/releases");

        check.dismiss();
        assert!(
            check.newer().is_none(),
            "a dismissed notice came back in the same session"
        );
    }

    #[test]
    fn a_fetch_that_answers_nothing_is_unknown_and_still_counts() {
        let mut checking = Checking {
            enabled: true,
            ..Default::default()
        };
        let now = BETWEEN_CHECKS.as_secs() + 7_000;
        let mut check = Check::begin(&checking, now, || None);
        assert!(
            settled(&mut check, &mut checking, now),
            "the answer never arrived"
        );
        assert_eq!(checking.last_checked, now);
        assert!(check.newer().is_none());
        // And it stops asking. A settled check that kept returning `true` would
        // write the preferences every frame for the rest of the session.
        assert!(!check.settle(&mut checking, now + 1_000));
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
