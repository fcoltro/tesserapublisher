//! Spelling, against a Hunspell dictionary.
//!
//! A dictionary is two files: `lang.dic`, a word per line with the affix
//! flags it may take, and `lang.aff`, the prefixes and suffixes those flags
//! stand for. `unhappiness` is not in the list; `happy/UY` is, `U` says
//! `un-` may go in front, `Y` says `-y` may become `-iness`. That is the
//! whole trick, and it is why a 50,000-word file covers a language.
//!
//! This reads the part of the format that decides whether a word is spelled
//! right: `PFX` and `SFX` rules with their strip, add and condition, the
//! three flag encodings, cross-product, and the word list. It does not read
//! compounding, replacement tables, phonetic tables, or anything else that
//! only matters for *suggesting* — suggestions here are the person's own
//! typing. What it does read is enough for the dictionaries LibreOffice and
//! Firefox ship, which are the ones a person has.
//!
//! No dictionary is bundled: the word lists are large and licensed each
//! their own way. Tessera looks in its dictionaries folder for the text's
//! language, and says so when there is none.

use std::collections::{HashMap, HashSet};

/// How flags are written after the slash in the word list and on rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum FlagKind {
    /// One character each: `happy/UY`.
    #[default]
    Short,
    /// Two characters each: `happy/UnYs`.
    Long,
    /// Decimal numbers, comma-separated: `happy/12,34`.
    Num,
}

/// A prefix or suffix rule.
#[derive(Debug, Clone)]
struct Affix {
    prefix: bool,
    cross: bool,
    /// What to strip from the stem before adding; empty for nothing.
    strip: String,
    /// What to add; empty (written `0`) for nothing.
    add: String,
    /// The condition on the stem, as a regex-like pattern over its end (for
    /// suffixes) or start (for prefixes): `[^aeiou]y`, `.`.
    condition: Vec<Cond>,
}

#[derive(Debug, Clone)]
enum Cond {
    Any,
    One(char),
    In(Vec<char>),
    NotIn(Vec<char>),
}

/// A loaded dictionary: the stems with their flags, and the rules.
#[derive(Debug, Default)]
pub struct Dictionary {
    flags: FlagKind,
    /// Stem → the flags it takes.
    stems: HashMap<String, Vec<u32>>,
    /// Flag → its affix rules.
    affixes: HashMap<u32, Vec<Affix>>,
    /// Words the person added, checked before anything else.
    added: HashSet<String>,
    ignore_case: bool,
}

impl Dictionary {
    /// Read the `.aff` and `.dic` texts.
    pub fn parse(aff: &str, dic: &str) -> Self {
        let mut d = Dictionary::default();
        d.read_aff(aff);
        d.read_dic(dic);
        d
    }

    /// Add a word the person vouches for.
    pub fn add(&mut self, word: &str) {
        self.added.insert(word.trim().to_lowercase());
    }

    pub fn is_empty(&self) -> bool {
        self.stems.is_empty()
    }

    /// Whether `word` is spelled right: in the list, or a stem plus the
    /// affixes its flags allow. A capitalised word is also tried lowercase
    /// — "The" at a sentence's start — and an all-capitals word both ways.
    pub fn check(&self, word: &str) -> bool {
        let word =
            word.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '\u{2019}');
        if word.is_empty() || word.chars().all(|c| c.is_numeric()) {
            return true;
        }
        let word = word.replace('\u{2019}', "'");
        if self.added.contains(&word.to_lowercase()) {
            return true;
        }
        if self.check_exact(&word) {
            return true;
        }
        let lower = word.to_lowercase();
        if lower != word
            && (self.ignore_case || word.chars().next().is_some_and(char::is_uppercase))
        {
            if self.check_exact(&lower) {
                return true;
            }
            // "MacArthur" typed as "Macarthur" is not allowed, but "THE"
            // is "The" is "the": all capitals may be any case.
            if word.chars().all(|c| !c.is_lowercase()) {
                let mut chars = lower.chars();
                let title: String = chars
                    .next()
                    .map(|c| c.to_uppercase().collect::<String>())
                    .unwrap_or_default()
                    + chars.as_str();
                if self.check_exact(&title) {
                    return true;
                }
            }
        }
        false
    }

    fn check_exact(&self, word: &str) -> bool {
        if let Some(flags) = self.stems.get(word) {
            // A stem may be marked as only ever appearing with an affix
            // (NEEDAFFIX); not read here, so a bare stem is a word.
            let _ = flags;
            return true;
        }
        // One suffix, one prefix, or both (cross product).
        for (flag, rules) in &self.affixes {
            for rule in rules {
                let Some(stem) = self.strip_affix(word, rule) else {
                    continue;
                };
                if self.stem_takes(&stem, *flag) {
                    return true;
                }
                // With a prefix as well, when this suffix allows it.
                if !rule.prefix && rule.cross {
                    for (pflag, prules) in &self.affixes {
                        for prule in prules.iter().filter(|p| p.prefix && p.cross) {
                            if let Some(bare) = self.strip_affix(&stem, prule)
                                && self.stem_takes(&bare, *flag)
                                && self.stem_takes(&bare, *pflag)
                            {
                                return true;
                            }
                        }
                    }
                }
            }
        }
        false
    }

    fn stem_takes(&self, stem: &str, flag: u32) -> bool {
        self.stems
            .get(stem)
            .is_some_and(|flags| flags.contains(&flag))
    }

    /// `word` with `rule`'s addition taken off and its strip put back, if
    /// the rule could have produced `word` from that stem.
    fn strip_affix(&self, word: &str, rule: &Affix) -> Option<String> {
        let stem = if rule.prefix {
            let rest = word.strip_prefix(rule.add.as_str())?;
            format!("{}{}", rule.strip, rest)
        } else {
            let rest = word.strip_suffix(rule.add.as_str())?;
            format!("{}{}", rest, rule.strip)
        };
        if stem.is_empty() || !condition_holds(&stem, &rule.condition, rule.prefix) {
            return None;
        }
        Some(stem)
    }

    fn read_aff(&mut self, aff: &str) {
        let mut pending: HashMap<u32, (bool, bool)> = HashMap::new();
        for line in aff.lines() {
            let line = line.trim();
            let mut parts = line.split_whitespace();
            let Some(key) = parts.next() else { continue };
            match key {
                "FLAG" => {
                    self.flags = match parts.next() {
                        Some("long") => FlagKind::Long,
                        Some("num") => FlagKind::Num,
                        _ => FlagKind::Short,
                    };
                }
                "PFX" | "SFX" => {
                    let prefix = key == "PFX";
                    let Some(flag) = parts
                        .next()
                        .and_then(|f| self.parse_flags(f).into_iter().next())
                    else {
                        continue;
                    };
                    let rest: Vec<&str> = parts.collect();
                    match rest.as_slice() {
                        // The header: PFX flag cross count
                        [cross, _count] if *cross == "Y" || *cross == "N" => {
                            pending.insert(flag, (prefix, *cross == "Y"));
                        }
                        // A rule: PFX flag strip add [condition] [morph…]
                        [strip, add, more @ ..] => {
                            let cross = pending.get(&flag).is_some_and(|(_, c)| *c);
                            let strip = if *strip == "0" {
                                String::new()
                            } else {
                                (*strip).to_owned()
                            };
                            // The addition may carry a continuation flag
                            // after a slash; not read.
                            let add = add.split('/').next().unwrap_or("");
                            let add = if add == "0" {
                                String::new()
                            } else {
                                add.to_owned()
                            };
                            let condition = more
                                .first()
                                .map(|c| parse_condition(c))
                                .unwrap_or_else(|| vec![Cond::Any]);
                            self.affixes.entry(flag).or_default().push(Affix {
                                prefix,
                                cross,
                                strip,
                                add,
                                condition,
                            });
                        }
                        _ => {}
                    }
                }
                "IGNORECASE" | "CHECKSHARPS" => {}
                _ => {}
            }
        }
    }

    fn read_dic(&mut self, dic: &str) {
        let mut lines = dic.lines();
        // The first line is the count, when it is a number.
        if let Some(first) = lines.next()
            && first.trim().parse::<usize>().is_err()
        {
            self.read_dic_line(first);
        }
        for line in lines {
            self.read_dic_line(line);
        }
    }

    fn read_dic_line(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return;
        }
        // "word/FLAGS morph…": the morphological fields after a tab or
        // space are not read.
        let entry = line.split(['\t', ' ']).next().unwrap_or(line);
        let (word, flags) = match entry.split_once('/') {
            Some((w, f)) => (w, self.parse_flags(f)),
            None => (entry, Vec::new()),
        };
        // An escaped slash in the word itself ("1/2") is rare; left alone.
        self.stems.entry(word.to_owned()).or_default().extend(flags);
    }

    fn parse_flags(&self, text: &str) -> Vec<u32> {
        match self.flags {
            FlagKind::Short => text.chars().map(u32::from).collect(),
            FlagKind::Long => {
                let chars: Vec<char> = text.chars().collect();
                chars
                    .chunks(2)
                    .map(|pair| {
                        let a = u32::from(pair[0]);
                        let b = pair.get(1).map_or(0, |c| u32::from(*c));
                        (a << 16) | b
                    })
                    .collect()
            }
            FlagKind::Num => text
                .split(',')
                .filter_map(|n| n.trim().parse().ok())
                .collect(),
        }
    }
}

/// `[^aeiou]y` → not-a-vowel, then `y`. A `.` is anything.
fn parse_condition(text: &str) -> Vec<Cond> {
    if text == "." {
        return vec![Cond::Any];
    }
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '.' => out.push(Cond::Any),
            '[' => {
                let negated = chars.peek() == Some(&'^');
                if negated {
                    chars.next();
                }
                let mut set = Vec::new();
                for c in chars.by_ref() {
                    if c == ']' {
                        break;
                    }
                    set.push(c);
                }
                out.push(if negated {
                    Cond::NotIn(set)
                } else {
                    Cond::In(set)
                });
            }
            other => out.push(Cond::One(other)),
        }
    }
    out
}

/// Whether the condition matches the end (suffix) or start (prefix) of
/// `stem`.
fn condition_holds(stem: &str, condition: &[Cond], prefix: bool) -> bool {
    let chars: Vec<char> = stem.chars().collect();
    if condition.len() > chars.len() {
        return false;
    }
    let window: &[char] = if prefix {
        &chars[..condition.len()]
    } else {
        &chars[chars.len() - condition.len()..]
    };
    window.iter().zip(condition).all(|(c, cond)| match cond {
        Cond::Any => true,
        Cond::One(x) => c == x,
        Cond::In(set) => set.contains(c),
        Cond::NotIn(set) => !set.contains(c),
    })
}

/// The words of `text`, each with its byte range, for checking one by one.
///
/// A word is a run of letters, digits and apostrophes; everything else
/// separates. Markers and control characters separate too, so a page number
/// is never a "word".
pub fn words(text: &str) -> Vec<(std::ops::Range<usize>, &str)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in text.char_indices() {
        let in_word = c.is_alphanumeric() || c == '\'' || c == '\u{2019}';
        match (in_word, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push((s..i, &text[s..i]));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s..text.len(), &text[s..]));
    }
    // Apostrophes alone are not words.
    out.retain(|(_, w)| w.chars().any(char::is_alphanumeric));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const AFF: &str = "\
SET UTF-8
TRY esianrtolcdugmphbyfvkwzESIANRTOLCDUGMPHBYFVKWZ'

PFX U Y 1
PFX U 0 un .

SFX Y Y 2
SFX Y y iness [^aeiou]y
SFX Y 0 ness [aeiou]y

SFX S Y 2
SFX S y ies [^aeiou]y
SFX S 0 s [^y]
";
    const DIC: &str = "\
4
happy/UYS
cat/S
the
Paris
";

    fn dictionary() -> Dictionary {
        Dictionary::parse(AFF, DIC)
    }

    #[test]
    fn a_listed_word_is_right_and_a_made_up_one_is_wrong() {
        let d = dictionary();
        assert!(d.check("cat"));
        assert!(d.check("the"));
        assert!(!d.check("teh"));
        assert!(!d.check("cta"));
    }

    #[test]
    fn affixes_build_the_words_the_list_leaves_out() {
        let d = dictionary();
        assert!(d.check("cats"), "S: -s");
        assert!(d.check("happiness"), "Y: y -> iness");
        assert!(d.check("unhappy"), "U: un-");
        assert!(d.check("unhappiness"), "U and Y crossed");
        assert!(!d.check("happys"), "the S rule wants no y before -s");
        assert!(d.check("happies"), "but y -> ies");
        assert!(!d.check("uncat"), "cat takes no U");
    }

    #[test]
    fn capitals_are_forgiven_where_a_sentence_begins_and_kept_where_a_name_does() {
        let d = dictionary();
        assert!(d.check("The"));
        assert!(d.check("THE"));
        assert!(d.check("Paris"));
        assert!(!d.check("paris"), "a name is a name");
        assert!(d.check("PARIS"));
    }

    #[test]
    fn punctuation_and_numbers_are_not_misspellings() {
        let d = dictionary();
        assert!(d.check("cat."));
        assert!(d.check("\u{201C}cats\u{201D}"));
        assert!(d.check("1984"));
        assert!(d.check(""));
    }

    #[test]
    fn added_words_are_right_from_then_on() {
        let mut d = dictionary();
        assert!(!d.check("Tessera"));
        d.add("Tessera");
        assert!(d.check("Tessera"));
        assert!(d.check("tessera"));
    }

    #[test]
    fn the_words_of_a_text_come_with_their_places() {
        let found = words("The cat's hat, 2 of them\u{2014}now.");
        let list: Vec<&str> = found.iter().map(|(_, w)| *w).collect();
        assert_eq!(list, ["The", "cat's", "hat", "2", "of", "them", "now"]);
        assert_eq!(found[1].0, 4..9);
    }

    #[test]
    fn long_and_numeric_flags_are_read() {
        let d = Dictionary::parse("FLAG long\nSFX Ab Y 1\nSFX Ab 0 s .\n", "1\ndog/Ab\n");
        assert!(d.check("dogs"));
        let d = Dictionary::parse("FLAG num\nSFX 12 Y 1\nSFX 12 0 s .\n", "1\ndog/12\n");
        assert!(d.check("dogs"));
        assert!(!d.check("dogss"));
    }
}
