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
//! three flag encodings, cross-product, and the word list — and, for
//! suggesting, `TRY` (the letters worth trying, commonest first) and `REP`
//! (the language's own list of usual slips); and compounding — `COMPOUNDFLAG`
//! and the begin, middle and end flags, `COMPOUNDMIN`, `ONLYINCOMPOUND` —
//! which is how German and the Nordic languages spell words no list could
//! hold; `COMPOUNDRULE`, patterns of flags a compound's parts must follow,
//! which is how English spells "10th" and "21st"; and `PHONE`, the phonetic rules a few dictionaries carry
//! ([`phonet`]). That is enough for the dictionaries LibreOffice and
//! Firefox ship, which are the ones a person has.
//!
//! [`Dictionary::suggest`] begins with the classic edit-distance-one walk:
//! every replacement from `REP`, then every swap of neighbours, dropped
//! letter, wrong letter and extra letter, then the word split in two —
//! each kept only if the dictionary passes it. Then, as Hunspell does,
//! words two slips away, looked up in the word list directly so the walk
//! stays quick; then, where the dictionary has a `PHONE` table, the words
//! that *sound* like it — "night" for "nite"; and last the words that
//! *look* most like it by their letter pairs and triples — Hunspell's
//! n-gram pass, which is what finds "phone" for "fone" when no single slip
//! explains it.
//!
//! No dictionary is bundled: the word lists are large and licensed each
//! their own way. Tessera looks in its dictionaries folder for the text's
//! language, and says so when there is none.

use std::collections::{HashMap, HashSet};

mod phonet;

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
    /// The letters to try when suggesting, commonest first: the `.aff`'s
    /// `TRY` line, or a–z when it has none.
    try_chars: Vec<char>,
    /// `REP from to`: the slips this language's makers have seen most.
    replacements: Vec<(String, String)>,
    /// The flag a stem carries to stand anywhere in a compound, and the
    /// ones for only its start, middle or end.
    compound_flag: Option<u32>,
    compound_begin: Option<u32>,
    compound_middle: Option<u32>,
    compound_end: Option<u32>,
    /// The shortest part a compound may be made of, in letters.
    compound_min: usize,
    /// A stem that is a word only inside a compound: the German linking
    /// "s" and the like.
    only_in_compound: Option<u32>,
    /// `PHONE`: what letters sound like, for suggesting by ear.
    phone: phonet::Table,
    /// `COMPOUNDRULE`: each a pattern of flags, one per part, a part
    /// repeated (`*`) or optional (`?`).
    compound_rules: Vec<Vec<(u32, Repeat)>>,
}

/// How often one element of a `COMPOUNDRULE` may match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Repeat {
    Once,
    /// `?`
    Maybe,
    /// `*`
    Any,
}

/// The most parts a `COMPOUNDRULE` compound may have: "12345th" is six,
/// and a long run of digits is not a word worth more work.
const MOST_RULE_PARTS: usize = 12;

/// Where a part stands in a compound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    Begin,
    Middle,
    End,
}

/// How many parts a compound may have: enough for any real word, few enough
/// that a long nonsense word cannot make the check crawl.
const MOST_PARTS: usize = 4;

impl Dictionary {
    /// Read the `.aff` and `.dic` texts.
    pub fn parse(aff: &str, dic: &str) -> Self {
        let mut d = Dictionary {
            try_chars: ('a'..='z').collect(),
            compound_min: 3,
            ..Dictionary::default()
        };
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
        if self.check_word(&word) {
            return true;
        }
        let lower = word.to_lowercase();
        if lower != word
            && (self.ignore_case || word.chars().next().is_some_and(char::is_uppercase))
        {
            if self.check_word(&lower) {
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
                if self.check_word(&title) {
                    return true;
                }
            }
        }
        false
    }

    /// What `word` was probably meant to be, likeliest first, at most eight.
    /// Empty for a word that is right, and for one nothing near is.
    ///
    /// Worked on the word lowercased when it began with a capital, and the
    /// answers given back in the word's own case — "Cta" is offered "Cat",
    /// "CTA" is offered "CAT" — except that a suggestion which is a proper
    /// name in the list keeps its own capital.
    pub fn suggest(&self, word: &str) -> Vec<String> {
        let word = word.trim();
        if word.is_empty() || self.check(word) {
            return Vec::new();
        }
        let chars: Vec<char> = word.chars().collect();
        let shouted = chars.len() > 1 && chars.iter().all(|c| !c.is_lowercase());
        let capitalised = chars[0].is_uppercase();
        let base: String = if capitalised {
            word.to_lowercase()
        } else {
            word.to_owned()
        };

        let mut out: Vec<String> = Vec::new();
        let offer = |candidate: String, out: &mut Vec<String>| {
            if out.len() >= 8 || candidate == base || out.contains(&candidate) {
                return;
            }
            // A capitalised word may have meant a name: "pariss" is tried
            // as "Paris" as well as "paris".
            let ok = candidate.split(' ').all(|part| {
                !part.is_empty() && (self.check(part) || (capitalised && self.check(&title(part))))
            });
            if ok {
                out.push(candidate);
            }
        };

        // The language's own list first.
        for (from, to) in &self.replacements {
            let mut at = 0;
            while let Some(found) = base[at..].find(from.as_str()) {
                let i = at + found;
                let candidate = format!("{}{}{}", &base[..i], to, &base[i + from.len()..]);
                offer(candidate, &mut out);
                at = i + from.len().max(1);
            }
        }

        let letters: Vec<char> = base.chars().collect();
        let n = letters.len();
        let with = |letters: &[char]| letters.iter().collect::<String>();

        // Neighbours swapped: "cta".
        for i in 0..n.saturating_sub(1) {
            let mut l = letters.clone();
            l.swap(i, i + 1);
            offer(with(&l), &mut out);
        }
        // A letter dropped: "catt".
        for i in 0..n {
            let mut l = letters.clone();
            l.remove(i);
            offer(with(&l), &mut out);
        }
        // A letter wrong: "cet".
        for i in 0..n {
            for &c in &self.try_chars {
                if c == letters[i] {
                    continue;
                }
                let mut l = letters.clone();
                l[i] = c;
                offer(with(&l), &mut out);
            }
        }
        // A letter missing: "hapy".
        for i in 0..=n {
            for &c in &self.try_chars {
                let mut l = letters.clone();
                l.insert(i, c);
                offer(with(&l), &mut out);
            }
        }
        // Two words run together: "thecat".
        for i in 1..n {
            let (a, b) = (with(&letters[..i]), with(&letters[i..]));
            offer(format!("{a} {b}"), &mut out);
        }

        // Two slips: every word one slip from a word one slip from this,
        // looked up in the list itself rather than through the affixes —
        // the square of the first walk, too many to check the long way.
        // Up to fourteen letters: the walk grows with the square of the
        // word, and a longer one is found by the look-alikes below.
        if out.len() < 8 && (4..=14).contains(&n) {
            let mut near: Vec<String> = self
                .one_slip(&letters)
                .into_iter()
                .flat_map(|once| self.one_slip(&once.chars().collect::<Vec<_>>()))
                .filter(|twice| self.stems.contains_key(twice) && !self.is_compound_only(twice))
                .collect();
            near.sort_by_key(|w| (w.chars().count().abs_diff(n), w.clone()));
            near.dedup();
            for candidate in near {
                offer(candidate, &mut out);
            }
        }

        // What sounds like it, where the dictionary says how words sound.
        if out.len() < 8 && !self.phone.is_empty() {
            for candidate in self.sounds_like(&base) {
                offer(candidate, &mut out);
            }
        }

        // What looks most like it, letter pair by letter pair.
        if out.len() < 8 {
            for candidate in self.look_alikes(&base) {
                offer(candidate, &mut out);
            }
        }

        // Back into the word's own case — where two passes offered one
        // word in two cases, "paris" and "Paris", it is offered once.
        let mut cased: Vec<String> = Vec::with_capacity(out.len());
        for s in out {
            let s = if shouted {
                s.to_uppercase()
            } else if capitalised {
                title(&s)
            } else {
                s
            };
            if !cased.contains(&s) {
                cased.push(s);
            }
        }
        cased
    }

    /// Every string one slip from `letters`: swapped, dropped, wrong or
    /// missing a letter from the `TRY` set.
    fn one_slip(&self, letters: &[char]) -> Vec<String> {
        let with = |l: &[char]| l.iter().collect::<String>();
        let n = letters.len();
        let mut out = Vec::new();
        for i in 0..n.saturating_sub(1) {
            let mut l = letters.to_vec();
            l.swap(i, i + 1);
            out.push(with(&l));
        }
        for i in 0..n {
            let mut l = letters.to_vec();
            l.remove(i);
            out.push(with(&l));
            for &c in &self.try_chars {
                if c != letters[i] {
                    let mut l = letters.to_vec();
                    l[i] = c;
                    out.push(with(&l));
                }
            }
        }
        for i in 0..=n {
            for &c in &self.try_chars {
                let mut l = letters.to_vec();
                l.insert(i, c);
                out.push(with(&l));
            }
        }
        out
    }

    /// The listed words most like `word` by the letters they share, in
    /// pairs and in threes, less what their lengths differ by: Hunspell's
    /// n-gram suggestion, for a word too far from any for slips to reach.
    /// At most four, and only those alike enough to be worth a look.
    fn look_alikes(&self, word: &str) -> Vec<String> {
        fn grams(word: &str, n: usize) -> Vec<String> {
            let chars: Vec<char> = word.chars().collect();
            chars.windows(n).map(|w| w.iter().collect()).collect()
        }
        let score = |candidate: &str| -> i64 {
            let mut shared = 0i64;
            for n in 1..=3 {
                let theirs = grams(candidate, n);
                shared +=
                    grams(word, n).iter().filter(|g| theirs.contains(g)).count() as i64 * n as i64;
            }
            let difference = word.chars().count().abs_diff(candidate.chars().count()) as i64;
            shared - 2 * difference
        };
        let length = word.chars().count();
        // As alike as the word is to itself, less a little for every
        // letter: a looser bar than that offers noise.
        let own = score(word);
        let bar = own * 2 / 5;
        let mut scored: Vec<(i64, &String)> = self
            .stems
            .keys()
            .filter(|s| s.chars().count().abs_diff(length) <= 3 && !self.is_compound_only(s))
            .map(|s| (score(s), s))
            .filter(|(s, _)| *s >= bar)
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
        scored.into_iter().take(4).map(|(_, s)| s.clone()).collect()
    }

    /// The listed words that sound as `word` does, by the `PHONE` table:
    /// the same phonetic code, nearest in length first, at most three.
    fn sounds_like(&self, word: &str) -> Vec<String> {
        let code = self.phone.code(word);
        if code.is_empty() {
            return Vec::new();
        }
        let length = word.chars().count();
        let mut alike: Vec<&String> = self
            .stems
            .keys()
            .filter(|s| s.chars().count().abs_diff(length) <= 4 && !self.is_compound_only(s))
            .filter(|s| self.phone.code(s) == code)
            .collect();
        alike.sort_by_key(|s| (s.chars().count().abs_diff(length), s.as_str()));
        alike.into_iter().take(3).cloned().collect()
    }

    /// A word in the list, a stem with its affixes, or a compound of them.
    fn check_word(&self, word: &str) -> bool {
        self.check_exact(word) || self.compound(word, 0) || self.ruled_compound(word)
    }

    /// Whether `word` is parts in the list whose flags follow one of the
    /// `COMPOUNDRULE` patterns, at least two of them: "10th" is "1" then
    /// "0th" under English's `n*1t`.
    fn ruled_compound(&self, word: &str) -> bool {
        if self.compound_rules.is_empty() || word.chars().count() > 40 {
            return false;
        }
        let bounds: Vec<usize> = word
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(word.len()))
            .collect();
        self.compound_rules
            .iter()
            .any(|rule| self.follows(word, &bounds, 0, rule, 0, 0))
    }

    /// Whether `word` from the `at`th character on follows `rule` from its
    /// `step`th element, `parts` parts having been taken already.
    fn follows(
        &self,
        word: &str,
        bounds: &[usize],
        at: usize,
        rule: &[(u32, Repeat)],
        step: usize,
        parts: usize,
    ) -> bool {
        let end = bounds.len() - 1;
        if at == end {
            return parts >= 2 && rule[step..].iter().all(|(_, r)| *r != Repeat::Once);
        }
        let Some(&(flag, repeat)) = rule.get(step) else {
            return false;
        };
        if parts >= MOST_RULE_PARTS {
            return false;
        }
        // Skipping an element that may be absent.
        if repeat != Repeat::Once && self.follows(word, bounds, at, rule, step + 1, parts) {
            return true;
        }
        let min = self.compound_min.max(1);
        for next in (at + min)..=end {
            let part = &word[bounds[at]..bounds[next]];
            if !self.stem_takes(part, flag) {
                continue;
            }
            let again = if repeat == Repeat::Any {
                step
            } else {
                step + 1
            };
            if self.follows(word, bounds, next, rule, again, parts + 1)
                || (repeat == Repeat::Any
                    && self.follows(word, bounds, next, rule, step + 1, parts + 1))
            {
                return true;
            }
        }
        false
    }

    /// A `COMPOUNDRULE` pattern read into its elements: flags in the
    /// dictionary's own encoding — a long or numeric flag in parentheses —
    /// each perhaps followed by `*` or `?`.
    fn parse_rule(&self, pattern: &str) -> Vec<(u32, Repeat)> {
        let mut out: Vec<(u32, Repeat)> = Vec::new();
        let mut chars = pattern.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '*' | '?' => {
                    if let Some(last) = out.last_mut() {
                        last.1 = if c == '*' { Repeat::Any } else { Repeat::Maybe };
                    }
                }
                '(' => {
                    let inner: String = chars.by_ref().take_while(|c| *c != ')').collect();
                    if let Some(flag) = self.parse_flags(&inner).into_iter().next() {
                        out.push((flag, Repeat::Once));
                    }
                }
                c => {
                    if let Some(flag) = self.parse_flags(&c.to_string()).into_iter().next() {
                        out.push((flag, Repeat::Once));
                    }
                }
            }
        }
        out
    }

    /// Whether a stem is a word only inside a compound.
    fn is_compound_only(&self, stem: &str) -> bool {
        self.only_in_compound
            .is_some_and(|flag| self.stem_takes(stem, flag))
    }

    /// Whether `word` is parts that may stand together, the first of them
    /// at `made` parts in: each at least `compound_min` letters, each a
    /// stem carrying a compound flag for where it stands — a prefix allowed
    /// on the first part, a suffix on the last.
    fn compound(&self, word: &str, made: usize) -> bool {
        if self.compound_flag.is_none() && self.compound_begin.is_none() {
            return false;
        }
        let min = self.compound_min.max(1);
        let bounds: Vec<usize> = word
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(word.len()))
            .collect();
        let letters = bounds.len() - 1;
        if letters < min * 2 || made + 2 > MOST_PARTS {
            return false;
        }
        for &cut in &bounds[min..=letters - min] {
            let (head, tail) = word.split_at(cut);
            let here = if made == 0 {
                Place::Begin
            } else {
                Place::Middle
            };
            if !self.part(head, here) {
                continue;
            }
            if self.part(tail, Place::End) || self.compound(tail, made + 1) {
                return true;
            }
        }
        false
    }

    /// Whether `part` may stand at `place` in a compound.
    fn part(&self, part: &str, place: Place) -> bool {
        let flags: Vec<u32> = [
            self.compound_flag,
            match place {
                Place::Begin => self.compound_begin,
                Place::Middle => self.compound_middle,
                Place::End => self.compound_end,
            },
        ]
        .into_iter()
        .flatten()
        .collect();
        let takes = |stem: &str| flags.iter().any(|f| self.stem_takes(stem, *f));
        if takes(part) {
            return true;
        }
        // An affix only at the compound's own ends: a prefix on its first
        // part, a suffix on its last.
        for (flag, rules) in &self.affixes {
            for rule in rules {
                let allowed = if rule.prefix {
                    place == Place::Begin
                } else {
                    place == Place::End
                };
                if !allowed {
                    continue;
                }
                if let Some(stem) = self.strip_affix(part, rule)
                    && self.stem_takes(&stem, *flag)
                    && takes(&stem)
                {
                    return true;
                }
            }
        }
        false
    }

    fn check_exact(&self, word: &str) -> bool {
        if self.stems.contains_key(word) {
            // A stem may be marked as only ever appearing with an affix
            // (NEEDAFFIX); not read here, so a bare stem is a word — unless
            // it is one only inside a compound.
            return !self.is_compound_only(word);
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
                "TRY" => {
                    if let Some(letters) = parts.next() {
                        self.try_chars = letters.chars().collect();
                    }
                }
                "REP" => {
                    // The first line is the count; the rest are pairs. A
                    // pair's `_` stands for a space, as the format has it.
                    if let (Some(from), Some(to)) = (parts.next(), parts.next()) {
                        self.replacements
                            .push((from.replace('_', " "), to.replace('_', " ")));
                    }
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
                "COMPOUNDFLAG" | "COMPOUNDBEGIN" | "COMPOUNDMIDDLE" | "COMPOUNDEND"
                | "ONLYINCOMPOUND" => {
                    let flag = parts
                        .next()
                        .and_then(|f| self.parse_flags(f).into_iter().next());
                    match key {
                        "COMPOUNDFLAG" => self.compound_flag = flag,
                        "COMPOUNDBEGIN" => self.compound_begin = flag,
                        "COMPOUNDMIDDLE" => self.compound_middle = flag,
                        "COMPOUNDEND" => self.compound_end = flag,
                        _ => self.only_in_compound = flag,
                    }
                }
                // The count line is `PHONE n`; each rule `PHONE search
                // replacement`.
                "PHONE" => {
                    if let (Some(search), Some(replacement)) = (parts.next(), parts.next()) {
                        self.phone.push(search, replacement);
                    }
                }
                // The count line is `COMPOUNDRULE n`; each rule a pattern.
                "COMPOUNDRULE" => {
                    if let Some(pattern) = parts.next()
                        && pattern.parse::<usize>().is_err()
                    {
                        let rule = self.parse_rule(pattern);
                        if !rule.is_empty() {
                            self.compound_rules.push(rule);
                        }
                    }
                }
                "COMPOUNDMIN" => {
                    if let Some(n) = parts.next().and_then(|n| n.parse().ok()) {
                        self.compound_min = n;
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

/// `word` with its first letter capitalised.
fn title(word: &str) -> String {
    let mut c = word.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>())
        .unwrap_or_default()
        + c.as_str()
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
    fn english_ordinals_are_compounds_the_rules_allow() {
        // English's own rules and the entries they read, as SCOWL ships
        // them: "11th" to "19th" under n*1t, the rest under n*mp.
        let aff = "COMPOUNDMIN 1\nONLYINCOMPOUND c\nCOMPOUNDRULE 2\nCOMPOUNDRULE n*1t\nCOMPOUNDRULE n*mp\n";
        let dic = "8\n0/nm\n0th/pt\n1/n1\n1st/p\n1th/tc\n2/nm\n2nd/p\n2th/tc\n";
        let d = Dictionary::parse(aff, dic);
        for word in [
            "10th", "11th", "12th", "21st", "22nd", "100th", "1st", "2nd",
        ] {
            assert!(d.check(word), "{word}");
        }
        for word in ["11st", "1th", "21th", "12nd"] {
            assert!(!d.check(word), "{word}");
        }
    }

    #[test]
    fn compounds_are_words_their_parts_allow() {
        // German's way: parts flagged to stand in a compound, a linking
        // "s" that is a word only inside one, and a suffix on the end.
        let aff = "COMPOUNDFLAG X\nCOMPOUNDMIN 1\nONLYINCOMPOUND O\nSFX N Y 1\nSFX N 0 n .\n";
        let dic = "4\narbeit/X\ns/XO\nzimmer/XN\nhaus/X\n";
        let d = Dictionary::parse(aff, dic);
        assert!(d.check("Arbeitszimmer"), "work + s + room");
        assert!(
            d.check("Arbeitszimmern"),
            "with its ending on the last part"
        );
        assert!(d.check("Hausarbeit"));
        assert!(!d.check("s"), "the linking s is no word alone");
        assert!(!d.check("Arbeitskatze"), "a part the list lacks");
        // Without a compound flag, nothing compounds.
        let plain = Dictionary::parse("", "2\narbeit\nzimmer\n");
        assert!(!plain.check("arbeitzimmer"));
    }

    #[test]
    fn two_slips_and_a_look_alike_are_suggested() {
        let d = Dictionary::parse(
            "TRY abcdefghijklmnopqrstuvwxyz\n",
            "4\nbeautiful\nphone\nnecessary\nthe\n",
        );
        // Two letters wrong at once: no single slip reaches it.
        assert!(d.suggest("beutifull").contains(&"beautiful".to_string()));
        assert!(d.suggest("neccesary").contains(&"necessary".to_string()));
        // Spelled as it sounds: "ph" for "f" is two slips, found either way.
        assert!(d.suggest("fone").contains(&"phone".to_string()));
        // And something like nothing is offered nothing.
        assert!(d.suggest("qqqqzzzz").is_empty());
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

    // --- suggesting ---------------------------------------------------------

    #[test]
    fn a_transposition_a_dropped_letter_and_a_wrong_one_are_suggested() {
        let d = dictionary();
        assert_eq!(d.suggest("cta"), vec!["cat"], "swapped");
        assert_eq!(d.suggest("hapy"), vec!["happy"], "dropped");
        assert_eq!(d.suggest("cet"), vec!["cat"], "wrong letter");
        assert!(
            d.suggest("catt").contains(&"cat".to_string()),
            "extra letter"
        );
    }

    #[test]
    fn suggestions_keep_the_word_s_capitals() {
        let d = dictionary();
        assert_eq!(d.suggest("Cta"), vec!["Cat"]);
        assert_eq!(d.suggest("CTA"), vec!["CAT"]);
        // A name keeps its own capital rather than being shouted.
        assert_eq!(d.suggest("Pariss"), vec!["Paris"]);
    }

    #[test]
    fn an_affixed_form_is_suggested_too() {
        // "unhappines" -> "unhappiness": a suggestion may be a stem plus
        // affixes, because that is what a word is.
        let d = dictionary();
        assert!(
            d.suggest("unhappines").contains(&"unhappiness".to_string()),
            "{:?}",
            d.suggest("unhappines")
        );
    }

    #[test]
    fn two_words_run_together_are_split() {
        let d = dictionary();
        assert!(d.suggest("thecat").contains(&"the cat".to_string()));
    }

    #[test]
    fn a_phonetic_table_suggests_what_sounds_alike() {
        let aff = "PHONE 8
PHONE GH _
PHONE G K
PHONE H H
PHONE I I
                   PHONE N N
PHONE T T
PHONE E$ _
PHONE E E
";
        // One slip from "nite" are "mite", "nice" and "nine", offered
        // first, as Hunspell does; "night" is three away, and is found by
        // how it sounds.
        let dic = "4
night
nine
nice
mite
";
        let with = Dictionary::parse(aff, dic);
        assert_eq!(with.sounds_like("nite"), ["night"]);
        let heard = with.suggest("nite");
        assert_eq!(heard[..3], ["mite", "nice", "nine"], "{heard:?}");
        assert!(heard.contains(&"night".to_owned()), "{heard:?}");
        // With no table, nothing is heard.
        assert!(Dictionary::parse("", dic).sounds_like("nite").is_empty());
    }

    #[test]
    fn a_replacement_table_entry_comes_first() {
        // REP says "ei" is often "ie" — the language's own knowledge of its
        // common mistakes, ahead of the mechanical edits.
        let d = Dictionary::parse(
            "TRY abcdefghijklmnopqrstuvwxyz\nREP 1\nREP ei ie\n",
            "2\nfriend\nfriends\n",
        );
        assert_eq!(
            d.suggest("freind").first().map(String::as_str),
            Some("friend")
        );
    }

    #[test]
    fn a_right_word_and_a_hopeless_one_get_no_suggestions() {
        let d = dictionary();
        assert!(d.suggest("cat").is_empty());
        assert!(d.suggest("xqzvw").is_empty());
    }
}
