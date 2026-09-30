//! Phonetic codes, from a dictionary's `PHONE` table.
//!
//! Björn Jacke's *phonet* rules, as aspell has them and Hunspell reads them:
//! each rule says what a run of letters sounds like, and a word's code is
//! what its letters sound like one after another — "night" and "nite" both
//! come out `NIT`. Words with the same code are what a person who spelled
//! by ear may have meant.
//!
//! A rule is a search string and a replacement (`_` for nothing). The
//! search string is letters, then optionally:
//!
//! - `(AB)` — one more letter, any of these;
//! - `-`, as many as there are — the last letters matched are only looked
//!   at, not replaced, and are read again as the next letters;
//! - `<` — the replacement is written back into the word and read again;
//! - a digit — the rule's priority, 5 when none is given, against the rule
//!   that would take over at its last letter;
//! - `^` — only at a word's start; `^^` — and the code starts afresh; `$` —
//!   only at its end.
//!
//! Followed step for step from Hunspell's `phonet.cxx`, quirks included —
//! a letter no rule covers is left out of the code — since the tables are
//! written against it.

use std::collections::HashMap;

/// A dictionary's phonetic rules, in the order it gives them.
#[derive(Debug, Clone, Default)]
pub struct Table {
    rules: Vec<(Vec<char>, Vec<char>)>,
    /// The first rule starting with each letter. A letter's rules are read
    /// from there for as long as they start with it, so a table gives them
    /// together, as every table does.
    first: HashMap<char, usize>,
}

/// Upper case, a letter at a time, keeping any letter whose capital is two
/// ("ß"): the tables are written in capitals and match letter by letter.
fn upper(c: char) -> char {
    let mut up = c.to_uppercase();
    match (up.next(), up.next()) {
        (Some(u), None) => u,
        _ => c,
    }
}

fn is_special(c: char) -> bool {
    "(-<^$".contains(c)
}

fn alpha(c: char) -> bool {
    c != '\0' && c.is_alphabetic()
}

impl Table {
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Add a rule: `PHONE search replacement`.
    pub fn push(&mut self, search: &str, replacement: &str) {
        let search: Vec<char> = search.chars().map(upper).collect();
        let Some(&lead) = search.first() else {
            return;
        };
        let replacement = if replacement == "_" {
            Vec::new()
        } else {
            replacement.chars().map(upper).collect()
        };
        self.first.entry(lead).or_insert(self.rules.len());
        self.rules.push((search, replacement));
    }

    /// Whether rule `n` exists and starts with `c`.
    fn starts(&self, n: usize, c: char) -> bool {
        self.rules.get(n).is_some_and(|r| r.0[0] == c)
    }

    /// What `word` sounds like, by the rules.
    pub fn code(&self, word: &str) -> String {
        let mut word: Vec<char> = word.chars().map(upper).collect();
        let at = |w: &[char], i: usize| w.get(i).copied().unwrap_or('\0');
        let mut target: Vec<char> = Vec::new();
        let mut i = 0usize;
        // Whether the last step was a rule with `<`, whose result is being
        // read again: a second one at once is taken as an ordinary rule, or
        // two could trade letters for ever.
        let mut z = false;
        let mut k = 0usize;
        let mut p0: i32 = -333;

        // Bounded, as a guard against a table that rewrites a word without
        // end; no real word needs a fraction of it.
        let mut steps = 0;
        while at(&word, i) != '\0' && steps < 64 * (word.len() + 1) {
            steps += 1;
            let mut c = at(&word, i);
            let mut z0 = false;
            if let Some(&start) = self.first.get(&c) {
                let mut n = start;
                while self.starts(n, c) {
                    let s = &self.rules[n].0;
                    let sc = |si: usize| s.get(si).copied().unwrap_or('\0');
                    let mut si = 1;
                    k = 1;
                    let mut p = 5;
                    while si < s.len()
                        && at(&word, i + k) == s[si]
                        && !s[si].is_ascii_digit()
                        && !is_special(s[si])
                    {
                        k += 1;
                        si += 1;
                    }
                    if sc(si) == '(' {
                        let close = s[si..].iter().position(|&x| x == ')').map(|p| si + p);
                        let group = &s[si + 1..close.unwrap_or(s.len())];
                        let next = at(&word, i + k);
                        if alpha(next) && group.contains(&next) {
                            k += 1;
                            si = close.map_or(s.len(), |x| x + 1);
                        }
                    }
                    p0 = sc(si) as i32;
                    let k0 = k;
                    while sc(si) == '-' && k > 1 {
                        k -= 1;
                        si += 1;
                    }
                    if sc(si) == '<' {
                        si += 1;
                    }
                    if sc(si).is_ascii_digit() {
                        p = sc(si) as i32 - '0' as i32;
                        si += 1;
                    }
                    if sc(si) == '^' && sc(si + 1) == '^' {
                        si += 1;
                    }
                    let fits = sc(si) == '\0'
                        || (sc(si) == '^'
                            && (i == 0 || !alpha(at(&word, i - 1)))
                            && (sc(si + 1) != '$' || !alpha(at(&word, i + k0))))
                        || (sc(si) == '$'
                            && i > 0
                            && alpha(at(&word, i - 1))
                            && !alpha(at(&word, i + k0)));
                    if !fits {
                        n += 1;
                        continue;
                    }

                    // A rule for the last letter matched, going on from
                    // there, may take over when its priority is at least
                    // this one's.
                    let c0 = at(&word, i + k - 1);
                    if k > 1
                        && p0 != '-' as i32
                        && at(&word, i + k) != '_'
                        && let Some(&start0) = self.first.get(&c0)
                    {
                        let mut n0 = start0;
                        while self.starts(n0, c0) {
                            let s0 = &self.rules[n0].0;
                            let sc0 = |si: usize| s0.get(si).copied().unwrap_or('\0');
                            let mut si = 1;
                            let mut k0 = k;
                            p0 = 5;
                            while si < s0.len()
                                && at(&word, i + k0) == s0[si]
                                && !s0[si].is_ascii_digit()
                                && !is_special(s0[si])
                            {
                                k0 += 1;
                                si += 1;
                            }
                            if sc0(si) == '(' {
                                let close = s0[si..].iter().position(|&x| x == ')').map(|p| si + p);
                                let group = &s0[si + 1..close.unwrap_or(s0.len())];
                                let next = at(&word, i + k0);
                                if alpha(next) && group.contains(&next) {
                                    k0 += 1;
                                    si = close.map_or(s0.len(), |x| x + 1);
                                }
                            }
                            while sc0(si) == '-' {
                                si += 1;
                            }
                            if sc0(si) == '<' {
                                si += 1;
                            }
                            if sc0(si).is_ascii_digit() {
                                p0 = sc0(si) as i32 - '0' as i32;
                                si += 1;
                            }
                            if sc0(si) == '\0' || (sc0(si) == '$' && !alpha(at(&word, i + k0))) {
                                if k0 == k || p0 < p {
                                    // Only a piece of this match, or too
                                    // weak to take over.
                                    n0 += 1;
                                    continue;
                                }
                                break;
                            }
                            n0 += 1;
                        }
                        if p0 >= p && self.starts(n0, c0) {
                            n += 1;
                            continue;
                        }
                    }

                    // The rule fits: replace.
                    let replacement = &self.rules[n].1;
                    let reread = s[1..].contains(&'<');
                    p0 = i32::from(reread);
                    if reread && !z {
                        if let (Some(&last), Some(&first)) = (target.last(), replacement.first())
                            && (last == c || last == first)
                        {
                            target.pop();
                        }
                        z0 = true;
                        z = true;
                        let mut k0 = 0;
                        for &r in replacement {
                            if at(&word, i + k0) == '\0' {
                                break;
                            }
                            word[i + k0] = r;
                            k0 += 1;
                        }
                        if k > k0 {
                            word.drain(i + k0..i + k);
                        }
                    } else {
                        i += k - 1;
                        z = false;
                        let (last, most) = match replacement.split_last() {
                            Some((last, most)) => (*last, most),
                            None => ('\0', &[][..]),
                        };
                        for &r in most {
                            if target.last() != Some(&r) {
                                target.push(r);
                            }
                        }
                        c = last;
                        if s[1..].windows(2).any(|w| w == ['^', '^']) {
                            if c != '\0' {
                                target.push(c);
                            }
                            word.drain(..=i.min(word.len().saturating_sub(1)));
                            i = 0;
                            z0 = true;
                        }
                    }
                    break;
                }
            }
            if !z0 {
                if k != 0 && p0 == 0 && c != '\0' && target.last() != Some(&c) {
                    target.push(c);
                }
                i += 1;
                z = false;
                k = 0;
            }
        }
        target.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(rules: &[(&str, &str)]) -> Table {
        let mut t = Table::default();
        for (search, replacement) in rules {
            t.push(search, replacement);
        }
        t
    }

    #[test]
    fn letters_sound_as_the_rules_say_and_the_rest_are_left_out() {
        let t = table(&[
            ("C(EI)-", "S"),
            ("C", "K"),
            ("I", "I"),
            ("T", "T"),
            ("Y", "_"),
        ]);
        // C before I is S, and the I is read again, as itself.
        assert_eq!(t.code("city"), "SIT");
        assert_eq!(t.code("cit"), "SIT");
        // C before anything else is K; a letter with no rule is dropped.
        assert_eq!(t.code("cat"), "KT");
    }

    #[test]
    fn a_rule_may_hold_at_a_word_s_start_or_end_only() {
        let t = table(&[
            ("KN^", "N"),
            ("K", "K"),
            ("N", "N"),
            ("O", "O"),
            ("E$", "_"),
            ("E", "E"),
        ]);
        assert_eq!(t.code("know"), "NO", "a silent K at the start");
        assert_eq!(t.code("oknok"), "OKNOK", "and only there");
        assert_eq!(t.code("koke"), "KOK", "a silent E at the end");
        assert_eq!(t.code("keko"), "KEKO", "and only there");
    }

    #[test]
    fn a_rule_with_lt_is_read_again_and_double_letters_are_one() {
        let t = table(&[("PH<", "F"), ("F", "F"), ("O", "O"), ("N", "N"), ("T", "T")]);
        assert_eq!(t.code("phone"), t.code("fone"));
        assert_eq!(t.code("fone"), "FON");
        assert_eq!(t.code("otto"), "OTO");
    }

    #[test]
    fn night_and_nite_sound_alike() {
        let t = table(&[
            ("GH", "_"),
            ("G", "K"),
            ("H", "H"),
            ("I", "I"),
            ("N", "N"),
            ("T", "T"),
            ("E$", "_"),
            ("E", "E"),
        ]);
        assert_eq!(t.code("night"), "NIT");
        assert_eq!(t.code("nite"), "NIT");
    }
}
