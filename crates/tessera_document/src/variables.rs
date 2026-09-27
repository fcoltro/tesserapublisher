//! Text variables: what a document defines for its markers to read as.
//!
//! The built-in markers — the page number, the section marker — need nothing
//! defined; the document knows its own pages. A **text variable** is one the
//! person defines: a running header that reads the nearest heading, or a piece
//! of text used in forty places that should change in all of them at once.
//! Each is referred to from a story by [`tessera_text::variables::Marker::Variable`]
//! carrying its index in [`crate::Document::variables`].
//!
//! ## A running header is read off the page
//!
//! "Running header (paragraph style)" is InDesign's name and its mechanism: the
//! variable names a paragraph style, and on each page reads as the first (or
//! last) paragraph in that style *laid out on that page*. That answer is not
//! known until the page's own text has been laid out — which is why the layout
//! crate resolves a page's own frames before the parent items it inherits,
//! even though the parent items paint underneath. The value is a fact about
//! the layout, and the model only says how to find it.

use serde::{Deserialize, Serialize};

use tessera_text::story::ParagraphStyleId;

/// Which paragraph a running header takes from a page with several.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Which {
    /// The first on the page: the section a verso is in.
    #[default]
    First,
    /// The last on the page: the section a recto reads on to.
    Last,
}

/// What a variable reads as.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VariableKind {
    /// The same text everywhere it is used.
    Custom(String),
    /// The first or last paragraph in `style` on the page.
    RunningHeader {
        style: ParagraphStyleId,
        which: Which,
    },
    /// The document's file name, as [`FileFacts::path`] has it: with its
    /// folder, its extension, both or neither. Nothing until it is saved.
    FileName { folder: bool, extension: bool },
    /// A date from [`FileFacts`], written as `format` says (see
    /// [`Stamp::format`]). Nothing where the date is not known — a document
    /// made before dates were kept has no creation date, and says so by
    /// saying nothing rather than guessing one.
    Date { of: DateOf, format: String },
    /// The number of the last page, of the document or of the section the
    /// page is in — "page 3 of 12".
    LastPageNumber { scope: PageScope },
    /// The document's chapter number, as its numbering options write it
    /// ([`crate::sections::Chapter`]).
    ChapterNumber,
}

/// Which of the document's dates a [`VariableKind::Date`] reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DateOf {
    /// When the document was first made.
    Creation,
    /// When it was last saved.
    Modification,
    /// When it is printed or exported: today, on screen.
    #[default]
    Output,
}

/// How far a [`VariableKind::LastPageNumber`] looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PageScope {
    /// The last page of the section the page is in.
    Section,
    /// The last page of the document.
    #[default]
    Document,
}

/// The date format a new date variable starts with: "27 September 2026".
pub const DEFAULT_DATE_FORMAT: &str = "d MMMM yyyy";

/// A moment as the calendar and clock on the wall showed it, where it was
/// taken — no time zone, because what a footer prints is the wall's date.
///
/// Made by the application, which is the one place that reads the clock:
/// the model and the layout only carry and format it, so a layout is the
/// same whenever it is run and a test never depends on today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Stamp {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

impl Stamp {
    /// `2026-09-27T20:05:09`, as `meta.json` keeps it.
    pub fn iso(self) -> String {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }

    /// Read what [`Stamp::iso`] wrote. Anything after the seconds — an
    /// offset, a fraction — is ignored, and anything that is not a date is
    /// `None`: an empty field from a file saved before dates were kept.
    pub fn from_iso(text: &str) -> Option<Self> {
        fn field<T: std::str::FromStr>(text: &str, range: std::ops::Range<usize>) -> Option<T> {
            text.get(range)?.parse().ok()
        }
        let small = |range| field::<u8>(text, range);
        let stamp = Self {
            year: field(text, 0..4)?,
            month: small(5..7)?,
            day: small(8..10)?,
            hour: small(11..13).unwrap_or(0),
            minute: small(14..16).unwrap_or(0),
            second: small(17..19).unwrap_or(0),
        };
        ((1..=12).contains(&stamp.month) && (1..=31).contains(&stamp.day)).then_some(stamp)
    }

    /// Written as `pattern` says, with InDesign's letters: `d` `dd` the
    /// day, `M` `MM` `MMM` `MMMM` the month as 9, 09, Sep, September, `yy`
    /// `yyyy` the year, `H` `HH` the hour of 24, `h` `hh` of 12, `mm` the
    /// minute, `ss` the second, `a` AM or PM. Text in single quotes is
    /// written as it is, `''` is a quote, and any other character is itself.
    pub fn format(self, pattern: &str) -> String {
        let chars: Vec<char> = pattern.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c == '\'' {
                if chars.get(i + 1) == Some(&'\'') {
                    out.push('\'');
                    i += 2;
                    continue;
                }
                i += 1;
                while i < chars.len() {
                    if chars[i] == '\'' {
                        // `''` inside quotes is a quote; one alone ends them.
                        if chars.get(i + 1) == Some(&'\'') {
                            out.push('\'');
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    out.push(chars[i]);
                    i += 1;
                }
                i += 1;
                continue;
            }
            let run = chars[i..].iter().take_while(|&&x| x == c).count();
            i += run;
            let hour12 = match self.hour % 12 {
                0 => 12,
                h => h,
            };
            let month = MONTHS[usize::from(self.month.clamp(1, 12) - 1)];
            match (c, run) {
                ('d', 1) => out.push_str(&self.day.to_string()),
                ('d', _) => out.push_str(&format!("{:02}", self.day)),
                ('M', 1) => out.push_str(&self.month.to_string()),
                ('M', 2) => out.push_str(&format!("{:02}", self.month)),
                ('M', 3) => out.push_str(&month[..3]),
                ('M', _) => out.push_str(month),
                ('y', 1 | 2) => out.push_str(&format!("{:02}", self.year.rem_euclid(100))),
                ('y', _) => out.push_str(&self.year.to_string()),
                ('H', 1) => out.push_str(&self.hour.to_string()),
                ('H', _) => out.push_str(&format!("{:02}", self.hour)),
                ('h', 1) => out.push_str(&hour12.to_string()),
                ('h', _) => out.push_str(&format!("{hour12:02}")),
                ('m', _) => out.push_str(&format!("{:02}", self.minute)),
                ('s', _) => out.push_str(&format!("{:02}", self.second)),
                ('a', _) => out.push_str(if self.hour < 12 { "AM" } else { "PM" }),
                _ => out.extend(std::iter::repeat_n(c, run)),
            }
        }
        out
    }
}

/// What the document knows about its file, which is not part of the
/// document: never written into `document.json`. The application fills it
/// — the path when the document is opened or saved as, the dates from
/// `meta.json` and the clock — and the layout reads it for the file name
/// and date variables.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileFacts {
    /// Where the document is saved; `None` until it is.
    pub path: Option<std::path::PathBuf>,
    /// When it was first made. Written into `meta.json` at every save, so
    /// it survives the file being copied, which a file system's own
    /// creation time does not.
    pub created: Option<Stamp>,
    /// When it was last saved.
    pub modified: Option<Stamp>,
    /// Today, for the output date: moved on by the application while the
    /// document is open, and just before it is printed or exported.
    pub output: Option<Stamp>,
}

impl FileFacts {
    /// What a [`VariableKind::FileName`] reads as.
    pub fn file_name(&self, folder: bool, extension: bool) -> String {
        let Some(path) = &self.path else {
            return String::new();
        };
        let name = if extension {
            path.file_name()
        } else {
            path.file_stem()
        };
        let name = name
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match path
            .parent()
            .filter(|p| folder && !p.as_os_str().is_empty())
        {
            Some(parent) => parent.join(name).to_string_lossy().into_owned(),
            None => name,
        }
    }

    /// The date `of` names, if it is known.
    pub fn date(&self, of: DateOf) -> Option<Stamp> {
        match of {
            DateOf::Creation => self.created,
            DateOf::Modification => self.modified,
            DateOf::Output => self.output,
        }
    }
}

/// A named variable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextVariable {
    pub name: String,
    pub kind: VariableKind,
}

impl TextVariable {
    pub fn custom(name: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: VariableKind::Custom(text.into()),
        }
    }

    pub fn running_header(name: impl Into<String>, style: ParagraphStyleId, which: Which) -> Self {
        Self {
            name: name.into(),
            kind: VariableKind::RunningHeader { style, which },
        }
    }

    pub fn file_name(name: impl Into<String>, folder: bool, extension: bool) -> Self {
        Self {
            name: name.into(),
            kind: VariableKind::FileName { folder, extension },
        }
    }

    pub fn date(name: impl Into<String>, of: DateOf, format: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: VariableKind::Date {
                of,
                format: format.into(),
            },
        }
    }

    pub fn chapter_number(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: VariableKind::ChapterNumber,
        }
    }

    pub fn last_page_number(name: impl Into<String>, scope: PageScope) -> Self {
        Self {
            name: name.into(),
            kind: VariableKind::LastPageNumber { scope },
        }
    }
}

/// The most variables a document can hold: a marker carries its index in one
/// byte of code point, and this is how many that byte can name.
pub const MOST_VARIABLES: usize = 256;

#[cfg(test)]
mod tests {
    use super::*;

    const EVENING: Stamp = Stamp {
        year: 2026,
        month: 9,
        day: 7,
        hour: 20,
        minute: 5,
        second: 9,
    };

    #[test]
    fn a_date_is_written_in_indesign_s_letters() {
        for (pattern, written) in [
            (DEFAULT_DATE_FORMAT, "7 September 2026"),
            ("dd/MM/yy", "07/09/26"),
            ("MMM d, yyyy", "Sep 7, 2026"),
            ("M/d/yyyy h:mm a", "9/7/2026 8:05 PM"),
            ("HH:mm:ss", "20:05:09"),
            ("'Printed' d MMMM", "Printed 7 September"),
            ("d 'o''clock'", "7 o'clock"),
            ("yyyy-MM-dd", "2026-09-07"),
        ] {
            assert_eq!(EVENING.format(pattern), written, "{pattern}");
        }
        let midnight = Stamp { hour: 0, ..EVENING };
        assert_eq!(midnight.format("h a"), "12 AM", "midnight is twelve");
    }

    #[test]
    fn a_stamp_survives_meta_json_and_an_empty_field_is_no_date() {
        assert_eq!(Stamp::from_iso(&EVENING.iso()), Some(EVENING));
        assert_eq!(
            Stamp::from_iso("2026-09-07T20:05:09+03:00"),
            Some(EVENING),
            "an offset is read past"
        );
        for nothing in ["", "yesterday", "2026-13-01T00:00:00", "2026-09"] {
            assert_eq!(Stamp::from_iso(nothing), None, "{nothing:?}");
        }
    }

    #[test]
    fn a_file_name_with_and_without_its_folder_and_extension() {
        let facts = FileFacts {
            path: Some(std::path::Path::new("books").join("Autumn.tsrdf")),
            ..FileFacts::default()
        };
        assert_eq!(facts.file_name(false, false), "Autumn");
        assert_eq!(facts.file_name(false, true), "Autumn.tsrdf");
        assert_eq!(
            facts.file_name(true, true),
            std::path::Path::new("books")
                .join("Autumn.tsrdf")
                .to_string_lossy()
        );
        assert_eq!(FileFacts::default().file_name(true, true), "", "unsaved");
    }
}
