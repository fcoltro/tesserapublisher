//! What is wrong with this document, before it costs money to find out.
//!
//! **A crate rather than a module, and the boundary is the point.** Preflight
//! must be independent of the GPU: it runs while the document changes, it runs
//! on a machine with no adapter, and one day it runs in a command-line packager
//! with no window at all. A dependency list that cannot reach `vello` proves
//! that in a way no comment can. Moving `effective_ppi` out of the renderer was
//! the first thing this crate asked for, and it was right to ask — how big a
//! picture is against how big it is drawn is arithmetic about a document, not
//! about a frame buffer.
//!
//! ## Errors and warnings are a real distinction
//!
//! An **error** means the job comes back wrong: text nobody will read because it
//! is not there, a picture that prints as a grey box. A **warning** means
//! somebody should look: 200ppi artwork is fine on newsprint and not fine on a
//! brochure.
//!
//! The line is not taste. It is whether a printer, following the file exactly,
//! produces something the customer did not intend. Everything on the wrong side
//! of that line is an error however common, and everything on the right side is
//! a warning however alarming it looks.
//!
//! ## Nothing here guesses
//!
//! A preflight that reports problems a document does not have is a preflight
//! people switch off — and then it is worth nothing on the day it was right. So
//! every rule answers a question with a definite answer, and where a question
//! cannot be answered definitely the rule is absent rather than approximate.

pub mod fonts;
pub mod rules;

use tessera_document::ids::{FrameId, PageId};

/// How much attention a problem deserves.
///
/// Ordered so sorting puts errors first, which is the order a panel shows them
/// in and the order somebody works through them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// The job comes back wrong.
    Error,
    /// Somebody should look at this before it goes.
    Warning,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Error => "Error",
            Severity::Warning => "Warning",
        }
    }
}

/// Which rule found a problem.
///
/// Named rather than free text, so a panel can group by it, a preset can silence
/// one, and a test can ask for exactly the rule it is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rule {
    /// A story longer than the frames it flows through.
    OversetText,
    /// A placed file that is not where the document says it is.
    MissingLink,
    /// A placed file that has changed on disk since it was placed.
    ModifiedLink,
    /// Artwork reproduced below the resolution asked for.
    LowResolution,
    /// A colour in a space the chosen press cannot print.
    ColourSpaceMismatch,
    /// An object over a page edge that stops short of the bleed.
    OutsideBleed,
    /// A reference to a named colour the document no longer defines.
    UnresolvedSwatch,
    /// No press has been chosen, so nothing can be checked against one.
    NoOutputIntent,
    /// Type in a family this machine cannot resolve.
    MissingFont,
}

impl Rule {
    pub const ALL: [Rule; 9] = [
        Rule::OversetText,
        Rule::MissingLink,
        Rule::ModifiedLink,
        Rule::LowResolution,
        Rule::ColourSpaceMismatch,
        Rule::OutsideBleed,
        Rule::UnresolvedSwatch,
        Rule::NoOutputIntent,
        Rule::MissingFont,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Rule::OversetText => "Overset text",
            Rule::MissingLink => "Missing link",
            Rule::ModifiedLink => "Modified link",
            Rule::LowResolution => "Low resolution",
            Rule::ColourSpaceMismatch => "Colour space",
            Rule::OutsideBleed => "Outside the bleed",
            Rule::UnresolvedSwatch => "Unresolved swatch",
            Rule::NoOutputIntent => "No output intent",
            Rule::MissingFont => "Missing font",
        }
    }

    /// Whether this rule, when it fires, means the job comes back wrong.
    ///
    /// Stated on the rule rather than decided where it fires, so one rule cannot
    /// be an error in one place and a warning in another.
    pub fn severity(self) -> Severity {
        match self {
            // Text that is not printed is text nobody reads. A missing picture
            // prints as nothing. An unresolved swatch prints in the alarming
            // magenta that exists to be noticed. All three come back wrong.
            // A missing font is the same kind of wrong as a missing
            // picture: the type is set in whatever the shaper falls back to, so
            // the copy fits differently, breaks differently, and comes back
            // looking like somebody else's job. It is worse than a missing
            // picture in one way \— a missing picture prints as nothing and is
            // noticed, and a substituted face prints as type.
            Rule::OversetText | Rule::MissingLink | Rule::UnresolvedSwatch | Rule::MissingFont => {
                Severity::Error
            }

            // These all need a person. A modified link might be the newer
            // artwork somebody meant to place; 200ppi is fine on newsprint; an
            // RGB object might be going to a digital press; an object short of
            // the bleed might be exactly where it was put on purpose.
            Rule::ModifiedLink
            | Rule::LowResolution
            | Rule::ColourSpaceMismatch
            | Rule::OutsideBleed
            | Rule::NoOutputIntent => Severity::Warning,
        }
    }
}

/// What a problem is about, so a panel can jump to it.
///
/// **Click-to-jump is not a nicety.** Forty problems with no locations is a list
/// somebody reads and then has to find everything in twice, and that is how
/// preflight comes to be skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Where {
    Frame(FrameId),
    Page(PageId),
    /// About the document as a whole. Nothing to jump to, and saying so beats
    /// jumping somewhere arbitrary.
    Document,
}

/// One thing that is wrong.
#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub rule: Rule,
    /// What to tell the person, in their terms.
    ///
    /// A sentence about *their document*, not a rule name with values
    /// interpolated. "Bridge.jpg is 137 ppi where 300 was asked for" tells
    /// somebody what to do; "LowResolution: 137 < 300" tells them what the
    /// program noticed.
    pub message: String,
    pub at: Where,
}

impl Problem {
    pub fn severity(&self) -> Severity {
        self.rule.severity()
    }
}

/// What preflight is checking against.
///
/// Passed in rather than read from preferences, because this crate must not
/// depend on the interface — and because a packager or a build step will want to
/// check against a printer's numbers rather than the ones on whatever screen
/// somebody happens to be sitting at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    /// Artwork below this effective resolution is reported.
    pub minimum_ppi: f64,
    /// How far past a page edge an object must reach to count as bleeding.
    ///
    /// Objects that stop short of this are reported. Zero switches the rule off,
    /// which is what a document with no bleed wants: every object over an edge
    /// would otherwise be reported and the rule would be noise.
    pub bleed: f64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            minimum_ppi: 300.0,
            bleed: 0.0,
        }
    }
}

/// Everything wrong with a document.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    pub problems: Vec<Problem>,
}

impl Report {
    pub fn errors(&self) -> usize {
        self.count(Severity::Error)
    }

    pub fn warnings(&self) -> usize {
        self.count(Severity::Warning)
    }

    fn count(&self, severity: Severity) -> usize {
        self.problems
            .iter()
            .filter(|p| p.severity() == severity)
            .count()
    }

    /// Whether the document is fit to send.
    ///
    /// Warnings do not stop it. A document with warnings and no errors is one
    /// somebody has looked at and decided about, and a preflight that refused it
    /// would be a preflight people work around.
    pub fn is_clear(&self) -> bool {
        self.errors() == 0
    }

    /// A one-line summary for the status bar.
    ///
    /// "No problems" is worth saying out loud: an empty space says "not checked"
    /// as loudly as it says "nothing wrong", and those are very different things
    /// to tell somebody about to send a job.
    pub fn summary(&self) -> String {
        match (self.errors(), self.warnings()) {
            (0, 0) => "No problems".to_string(),
            (0, w) => format!("{w} warning{}", plural(w)),
            (e, 0) => format!("{e} error{}", plural(e)),
            (e, w) => format!("{e} error{}, {w} warning{}", plural(e), plural(w)),
        }
    }

    /// Errors first, then warnings, and within each the order they were found.
    ///
    /// A stable sort, so two runs over an unchanged document give the same list
    /// in the same order — a panel whose rows moved between frames would be one
    /// nobody could click.
    pub fn sorted(mut self) -> Self {
        self.problems.sort_by_key(|p| p.severity());
        self
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_problem(rule: Rule) -> Problem {
        Problem {
            rule,
            message: "something".to_string(),
            at: Where::Document,
        }
    }

    #[test]
    fn a_clean_document_says_so_rather_than_saying_nothing() {
        assert_eq!(Report::default().summary(), "No problems");
        assert!(Report::default().is_clear());
    }

    #[test]
    fn warnings_do_not_stop_a_document_going_out() {
        let report = Report {
            problems: vec![a_problem(Rule::LowResolution)],
        };
        assert!(report.is_clear());
        assert_eq!(report.summary(), "1 warning");
    }

    #[test]
    fn errors_do() {
        let report = Report {
            problems: vec![a_problem(Rule::OversetText)],
        };
        assert!(!report.is_clear());
        assert_eq!(report.summary(), "1 error");
    }

    #[test]
    fn the_summary_counts_both_and_gets_its_plurals_right() {
        let report = Report {
            problems: vec![
                a_problem(Rule::OversetText),
                a_problem(Rule::MissingLink),
                a_problem(Rule::LowResolution),
            ],
        };
        assert_eq!(report.summary(), "2 errors, 1 warning");
    }

    #[test]
    fn errors_sort_above_warnings_and_the_rest_keeps_its_order() {
        // Stable, so an unchanged document gives the same list in the same
        // order. A panel whose rows moved between frames would be unclickable.
        let report = Report {
            problems: vec![
                a_problem(Rule::LowResolution),
                a_problem(Rule::OversetText),
                a_problem(Rule::ModifiedLink),
                a_problem(Rule::MissingLink),
            ],
        }
        .sorted();

        let order: Vec<Rule> = report.problems.iter().map(|p| p.rule).collect();
        assert_eq!(
            order,
            vec![
                Rule::OversetText,
                Rule::MissingLink,
                Rule::LowResolution,
                Rule::ModifiedLink
            ]
        );
    }

    #[test]
    fn a_rule_has_one_severity_wherever_it_fires() {
        // Stated on the rule rather than decided at each site, so one rule
        // cannot be an error in one place and a warning in another.
        assert_eq!(Rule::OversetText.severity(), Severity::Error);
        assert_eq!(Rule::LowResolution.severity(), Severity::Warning);
    }

    #[test]
    fn the_things_that_come_back_wrong_are_errors_and_the_rest_are_not() {
        // The line: would a printer following this file exactly produce
        // something the customer did not intend?
        for rule in [Rule::OversetText, Rule::MissingLink, Rule::UnresolvedSwatch] {
            assert_eq!(rule.severity(), Severity::Error, "{}", rule.title());
        }
        for rule in [
            Rule::ModifiedLink,
            Rule::LowResolution,
            Rule::ColourSpaceMismatch,
            Rule::OutsideBleed,
            Rule::NoOutputIntent,
        ] {
            assert_eq!(rule.severity(), Severity::Warning, "{}", rule.title());
        }
    }

    #[test]
    fn every_rule_has_a_title_of_its_own() {
        // Distinct, not merely present. Two rules under one title are two
        // problems a reader cannot tell apart, and counting them to eight was a
        // proxy that broke the moment a ninth rule arrived while saying nothing
        // about whether the eight were distinguishable.
        let mut titles: Vec<&str> = Rule::ALL.iter().map(|r| r.title()).collect();
        for title in &titles {
            assert!(!title.is_empty());
        }
        titles.sort_unstable();
        let total = titles.len();
        titles.dedup();
        assert_eq!(titles.len(), total, "two rules share a title");
    }
}
