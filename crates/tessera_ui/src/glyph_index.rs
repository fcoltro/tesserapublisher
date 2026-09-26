//! What a face's characters are called and what kind each is: what the
//! Glyphs panel searches and sorts by.
//!
//! A person looking for a character knows it by sight or by a word — the
//! arrow, the section sign, the euro, "ellipsis" — and almost never by its
//! code point, which was all the panel could search by. So each character
//! carries its name from the Unicode standard, read once when a face is shown
//! rather than per frame, and one of a handful of kinds a panel can narrow to.

use unicode_general_category::{GeneralCategory, get_general_category};

/// The kinds a panel narrows its characters to, besides all of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Show {
    #[default]
    All,
    Letters,
    Numbers,
    Punctuation,
    Symbols,
    Arrows,
    Maths,
    Currency,
}

impl Show {
    pub const ALL: [Show; 8] = [
        Show::All,
        Show::Letters,
        Show::Numbers,
        Show::Punctuation,
        Show::Symbols,
        Show::Arrows,
        Show::Maths,
        Show::Currency,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Show::All => "All",
            Show::Letters => "Letters",
            Show::Numbers => "Numbers",
            Show::Punctuation => "Punctuation",
            Show::Symbols => "Symbols",
            Show::Arrows => "Arrows",
            Show::Maths => "Maths",
            Show::Currency => "Currency",
        }
    }
}

/// One character of a face, with what it is called and what kind it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub c: char,
    /// Its name in the standard, in the standard's capitals, or empty for a
    /// character with none — a private-use ornament, most often.
    pub name: String,
    pub kind: Show,
    /// Its code point, written out once for the search.
    hex: String,
}

impl Entry {
    pub fn of(c: char) -> Entry {
        let name = unicode_names2::name(c)
            .map(|n| n.to_string())
            .unwrap_or_default();
        Entry {
            c,
            kind: kind_of(c, &name),
            hex: format!("{:04X}", u32::from(c)),
            name,
        }
    }
}

/// Which kind a character is: by its name for arrows, which the standard
/// files under several categories, and by its category for the rest.
pub fn kind_of(c: char, name: &str) -> Show {
    use GeneralCategory as G;
    if name.contains("ARROW") {
        return Show::Arrows;
    }
    match get_general_category(c) {
        G::UppercaseLetter
        | G::LowercaseLetter
        | G::TitlecaseLetter
        | G::ModifierLetter
        | G::OtherLetter
        | G::NonspacingMark
        | G::SpacingMark
        | G::EnclosingMark => Show::Letters,
        G::DecimalNumber | G::LetterNumber | G::OtherNumber => Show::Numbers,
        G::ConnectorPunctuation
        | G::DashPunctuation
        | G::OpenPunctuation
        | G::ClosePunctuation
        | G::InitialPunctuation
        | G::FinalPunctuation
        | G::OtherPunctuation => Show::Punctuation,
        G::MathSymbol => Show::Maths,
        G::CurrencySymbol => Show::Currency,
        _ => Show::Symbols,
    }
}

/// What kind of character it is, in words, for the line under its name.
pub fn category_words(c: char) -> &'static str {
    use GeneralCategory as G;
    match get_general_category(c) {
        G::UppercaseLetter => "Capital letter",
        G::LowercaseLetter => "Small letter",
        G::TitlecaseLetter => "Title-case letter",
        G::ModifierLetter => "Modifier letter",
        G::OtherLetter => "Letter",
        G::NonspacingMark | G::SpacingMark | G::EnclosingMark => "Combining mark",
        G::DecimalNumber => "Digit",
        G::LetterNumber => "Letter number",
        G::OtherNumber => "Number",
        G::ConnectorPunctuation => "Connector",
        G::DashPunctuation => "Dash",
        G::OpenPunctuation => "Opening bracket",
        G::ClosePunctuation => "Closing bracket",
        G::InitialPunctuation => "Opening quote",
        G::FinalPunctuation => "Closing quote",
        G::OtherPunctuation => "Punctuation",
        G::MathSymbol => "Maths symbol",
        G::CurrencySymbol => "Currency sign",
        G::ModifierSymbol => "Modifier symbol",
        G::PrivateUse => "Private use: this font's own",
        _ => "Symbol",
    }
}

/// A name as a sentence says it: "Horizontal ellipsis", "Latin small letter
/// e with acute", "Latin capital letter A" — the standard's capitals read as
/// shouting, and a letter's own case is the one thing in a name worth
/// keeping.
pub fn spoken(name: &str) -> String {
    if name.is_empty() {
        return "Unnamed character".to_string();
    }
    let capital = name.contains("CAPITAL");
    let part = |word: &str| -> String {
        if word.len() == 1 && word.chars().all(|c| c.is_ascii_alphabetic()) {
            // The letter a name is about, in the case it is.
            if capital {
                word.to_string()
            } else {
                word.to_ascii_lowercase()
            }
        } else if word.chars().any(|c| c.is_ascii_digit()) || word == "CJK" {
            // A code or a number inside a name reads as written.
            word.to_string()
        } else {
            word.to_ascii_lowercase()
        }
    };
    let words: Vec<String> = name
        .split(' ')
        .map(|word| word.split('-').map(part).collect::<Vec<_>>().join("-"))
        .collect();
    let mut sentence = words.join(" ");
    if let Some(first) = sentence.get(0..1) {
        let upper = first.to_ascii_uppercase();
        sentence.replace_range(0..1, &upper);
    }
    sentence
}

/// A search, read once from what was typed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Query {
    /// One character typed or pasted: that character, and nothing else.
    exact: Option<char>,
    /// Words each of which a name must contain.
    words: Vec<String>,
    /// Hex digits a code point must contain: "U+2026", "2026", "20".
    hex: Option<String>,
}

impl Query {
    pub fn read(text: &str) -> Query {
        let text = text.trim();
        let mut chars = text.chars();
        if let (Some(only), None) = (chars.next(), chars.next()) {
            return Query {
                exact: Some(only),
                ..Query::default()
            };
        }
        let code = text
            .trim_start_matches("U+")
            .trim_start_matches("u+")
            .trim_start_matches("0x");
        let hex = (code.len() >= 2 && code.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| code.to_ascii_uppercase());
        Query {
            exact: None,
            words: text.split_whitespace().map(str::to_uppercase).collect(),
            hex,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.exact.is_none() && self.words.is_empty() && self.hex.is_none()
    }

    pub fn matches(&self, entry: &Entry) -> bool {
        if let Some(c) = self.exact {
            return entry.c == c;
        }
        if self.is_empty() {
            return true;
        }
        let by_code = self.hex.as_ref().is_some_and(|hex| entry.hex.contains(hex));
        let by_name = !self.words.is_empty()
            && !entry.name.is_empty()
            && self.words.iter().all(|w| entry.name.contains(w.as_str()));
        by_code || by_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_character_is_known_by_its_name_and_its_kind() {
        let ellipsis = Entry::of('\u{2026}');
        assert_eq!(ellipsis.name, "HORIZONTAL ELLIPSIS");
        assert_eq!(ellipsis.kind, Show::Punctuation);
        assert_eq!(Entry::of('€').kind, Show::Currency);
        assert_eq!(Entry::of('±').kind, Show::Maths);
        assert_eq!(Entry::of('→').kind, Show::Arrows, "an arrow, not maths");
        assert_eq!(Entry::of('½').kind, Show::Numbers);
        assert_eq!(Entry::of('é').kind, Show::Letters);
        assert_eq!(Entry::of('©').kind, Show::Symbols);
        let ornament = Entry::of('\u{E000}');
        assert_eq!(ornament.name, "", "private use has no name");
        assert_eq!(ornament.kind, Show::Symbols);
    }

    #[test]
    fn a_name_is_said_as_a_sentence_keeping_its_letter_s_case() {
        assert_eq!(spoken("HORIZONTAL ELLIPSIS"), "Horizontal ellipsis");
        assert_eq!(
            spoken("LATIN SMALL LETTER E WITH ACUTE"),
            "Latin small letter e with acute"
        );
        assert_eq!(spoken("LATIN CAPITAL LETTER A"), "Latin capital letter A");
        assert_eq!(
            spoken("CJK UNIFIED IDEOGRAPH-4E00"),
            "CJK unified ideograph-4E00"
        );
        assert_eq!(spoken(""), "Unnamed character");
    }

    fn found(query: &str, among: &str) -> String {
        let q = Query::read(query);
        among
            .chars()
            .map(Entry::of)
            .filter(|e| q.matches(e))
            .map(|e| e.c)
            .collect()
    }

    #[test]
    fn a_search_finds_by_words_of_the_name_in_any_order() {
        let among = "—–-…§¶→←↑€$aé";
        assert_eq!(found("dash", among), "—–");
        assert_eq!(found("em dash", among), "—");
        assert_eq!(found("Arrow LEFT", among), "←", "any order, any case");
        assert_eq!(found("euro", among), "€");
        assert_eq!(found("section", among), "§");
        assert_eq!(found("acute", among), "é");
        assert_eq!(found("nothing called this", among), "");
    }

    #[test]
    fn one_character_typed_or_pasted_finds_itself_alone() {
        let among = "aAé€e";
        assert_eq!(found("é", among), "é");
        assert_eq!(found("a", among), "a", "not every name with an A in it");
        assert_eq!(found(" € ", among), "€");
    }

    #[test]
    fn a_code_finds_by_its_digits_as_the_box_did() {
        let among = "…†→a";
        assert_eq!(found("U+2026", among), "…");
        assert_eq!(found("u+2026", among), "…");
        assert_eq!(found("2026", among), "…");
        assert_eq!(found("20", among), "…†", "20 finds U+2026 and U+2020 alike");
        assert_eq!(found("0x2192", among), "→");
        assert!(Query::read("   ").is_empty());
        assert_eq!(found("", among), among, "nothing typed is everything");
    }
}
