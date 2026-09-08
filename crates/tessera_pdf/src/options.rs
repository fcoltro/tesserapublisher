//! What an export is asked to produce.
//!
//! Milestone 0 exported one thing one way. A commercial job needs to say which
//! standard it conforms to, which press it is for, and what marks the guillotine
//! and the press operator need — and those are decisions about *this* export, not
//! about the document. Two exports of one layout, one a proof and one a plate
//! set, differ here and nowhere else.

use tessera_document::intent::OutputIntent;

/// Which PDF standard the file claims to meet.
///
/// **A claim, and one a file is not allowed to make lightly.** Writing
/// `GTS_PDFXVersion` into a file that does not conform is worse than writing
/// nothing: a printer's preflight trusts it, passes the file, and the job fails
/// on the press instead of in the studio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Standard {
    /// An ordinary PDF. Valid, readable, and claiming nothing.
    #[default]
    Plain,
    /// PDF/X-1a:2003. Everything flattened to CMYK and spot; no transparency,
    /// no ICC-tagged RGB. The safe old standard, and still what many presses
    /// ask for.
    X1a,
    /// PDF/X-4:2010. Live transparency and ICC colour allowed, which is what
    /// makes it the one to prefer where a press accepts it.
    X4,
}

impl Standard {
    pub const ALL: [Standard; 3] = [Standard::Plain, Standard::X1a, Standard::X4];

    pub fn label(self) -> &'static str {
        match self {
            Standard::Plain => "PDF",
            Standard::X1a => "PDF/X-1a",
            Standard::X4 => "PDF/X-4",
        }
    }

    /// What goes in `GTS_PDFXVersion`, if anything.
    pub fn version_key(self) -> Option<&'static str> {
        match self {
            Standard::Plain => None,
            Standard::X1a => Some("PDF/X-1a:2003"),
            Standard::X4 => Some("PDF/X-4"),
        }
    }

    /// Whether this standard requires every colour to be device colour.
    ///
    /// X-1a does: no ICC-tagged RGB may survive, so everything is converted
    /// through the output intent. X-4 allows ICC colour, but converting anyway
    /// is what most presses expect and is never wrong — so the difference shows
    /// in what the file *claims*, not in what it contains.
    pub fn needs_device_colour(self) -> bool {
        matches!(self, Standard::X1a)
    }

    /// Whether this standard forbids live transparency.
    ///
    /// X-1a does. Tessera does not flatten, so an X-1a export of a document
    /// using opacity or a blend mode is a claim it cannot honour — and
    /// [`ExportOptions::refusals`] says so rather than writing the claim.
    pub fn forbids_transparency(self) -> bool {
        matches!(self, Standard::X1a)
    }
}

/// What the press and the guillotine need to see outside the trim.
///
/// All of it lives in the bleed area or beyond, which is why a document with no
/// bleed cannot carry marks: there is nowhere to put them that is not on the
/// finished page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Marks {
    /// Corner marks showing where to cut.
    pub crop: bool,
    /// Short marks showing how far the ink is meant to run past the trim.
    pub bleed: bool,
    /// Targets for checking the plates line up.
    pub registration: bool,
    /// A strip of solid and tinted patches for checking density on press.
    pub colour_bar: bool,
    /// How far outside the trim the marks begin, in points.
    ///
    /// Marks that start at the trim would be cut through, which defeats them.
    /// The default clears a typical 3mm bleed.
    pub offset: f64,
}

impl Default for Marks {
    fn default() -> Self {
        Self {
            crop: false,
            bleed: false,
            registration: false,
            colour_bar: false,
            // Just past 3mm, the usual bleed, so a mark is outside the ink
            // rather than sitting on the end of it.
            offset: 10.0,
        }
    }
}

impl Marks {
    /// Everything a commercial printer usually asks for.
    pub fn all() -> Self {
        Self {
            crop: true,
            bleed: true,
            registration: true,
            colour_bar: true,
            ..Self::default()
        }
    }

    pub fn any(&self) -> bool {
        self.crop || self.bleed || self.registration || self.colour_bar
    }

    /// How far past the trim the marks reach, so a media box can hold them.
    pub fn reach(&self) -> f64 {
        if !self.any() {
            return 0.0;
        }
        // The offset, plus the length of the longest mark, plus a little air.
        // A media box that ends exactly at the end of a mark clips its final
        // pixel on half the RIPs in the world.
        self.offset + MARK_LENGTH + 4.0
    }
}

/// How long a crop or bleed mark is drawn, in points.
pub const MARK_LENGTH: f64 = 14.0;

/// Everything an export needs to know beyond the document itself.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExportOptions {
    pub standard: Standard,
    pub marks: Marks,
    /// The press this file is for.
    ///
    /// Carried by value rather than borrowed because an export outlives the
    /// borrow of a document in every caller so far, and a profile is a few
    /// hundred kilobytes copied once per export rather than per object.
    pub intent: Option<OutputIntent>,
}

impl ExportOptions {
    /// Why this export cannot honour what it was asked for.
    ///
    /// **Checked before anything is written**, because the failure mode
    /// otherwise is a file that claims a standard it does not meet — and a
    /// printer's preflight believes the claim, passes it, and the job fails on
    /// press instead of in the studio.
    ///
    /// An empty list means the claim can be made honestly.
    /// `has_artwork` is no longer a reason to refuse anything: placed pictures
    /// are converted through the output intent's profile, so a CMYK export has
    /// no RGB left in it. Kept in the signature because the caller already
    /// computes it and a rule that needs it will arrive before one that does
    /// not — an X-1a document with a spot colour in a picture, for one.
    pub fn refusals(&self, has_transparency: bool, _has_artwork: bool) -> Vec<String> {
        let mut out = Vec::new();

        if self.standard != Standard::Plain && self.intent.is_none() {
            out.push(format!(
                "{} requires an output intent, and this document names no press",
                self.standard.label()
            ));
        }

        if self.standard.forbids_transparency() && has_transparency {
            out.push(format!(
                "{} does not allow transparency, and this document uses it. \
                 Export PDF/X-4, or remove the opacity and blend modes.",
                self.standard.label()
            ));
        }

        if self.marks.any() && self.marks.offset <= 0.0 {
            out.push("Marks at no offset would be cut through by the trim".to_string());
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn an_intent() -> OutputIntent {
        let profile = tessera_color::managed::OutputProfile::screen().expect("a profile");
        OutputIntent {
            description: profile.description().to_string(),
            profile: profile.bytes().to_vec(),
            rendering: tessera_document::intent::Rendering::default(),
        }
    }

    #[test]
    fn a_plain_pdf_claims_nothing_and_so_refuses_nothing() {
        let options = ExportOptions::default();
        assert!(options.standard.version_key().is_none());
        assert!(options.refusals(true, false).is_empty());
    }

    #[test]
    fn a_standard_without_a_press_is_refused() {
        // PDF/X is a promise about *which* press. Without one there is nothing
        // to promise, and writing the claim anyway is the failure this exists
        // to prevent.
        let options = ExportOptions {
            standard: Standard::X4,
            ..Default::default()
        };
        let refused = options.refusals(false, false);
        assert_eq!(refused.len(), 1);
        assert!(refused[0].contains("output intent"));
    }

    #[test]
    fn x1a_refuses_a_document_that_uses_transparency() {
        // Tessera does not flatten. Claiming X-1a for a document with live
        // transparency would be a claim it cannot honour, and a printer's
        // preflight would believe it.
        let options = ExportOptions {
            standard: Standard::X1a,
            intent: Some(an_intent()),
            ..Default::default()
        };
        assert!(
            options
                .refusals(true, false)
                .iter()
                .any(|r| r.contains("transparency"))
        );
        assert!(options.refusals(false, false).is_empty());
    }

    #[test]
    fn x4_allows_transparency_which_is_the_reason_to_prefer_it() {
        let options = ExportOptions {
            standard: Standard::X4,
            intent: Some(an_intent()),
            ..Default::default()
        };
        assert!(options.refusals(true, false).is_empty());
    }

    #[test]
    fn marks_at_no_offset_are_refused() {
        // They would be cut through by the trim, which defeats them.
        let options = ExportOptions {
            marks: Marks {
                offset: 0.0,
                ..Marks::all()
            },
            ..Default::default()
        };
        assert!(!options.refusals(false, false).is_empty());
    }

    #[test]
    fn no_marks_reach_nowhere_and_marks_reach_past_their_offset() {
        assert_eq!(Marks::default().reach(), 0.0);
        let all = Marks::all();
        assert!(
            all.reach() > all.offset + MARK_LENGTH,
            "a media box ending exactly at the end of a mark clips it"
        );
    }

    #[test]
    fn only_x1a_demands_device_colour() {
        // X-4 allows ICC colour, which is what makes it the one to prefer.
        assert!(Standard::X1a.needs_device_colour());
        assert!(!Standard::X4.needs_device_colour());
        assert!(!Standard::Plain.needs_device_colour());
    }

    #[test]
    fn every_standard_has_a_label_and_only_the_plain_one_has_no_version() {
        for standard in Standard::ALL {
            assert!(!standard.label().is_empty());
        }
        assert!(Standard::X1a.version_key().is_some());
        assert!(Standard::X4.version_key().is_some());
        assert!(Standard::Plain.version_key().is_none());
    }
}
