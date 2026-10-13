//! Where Chinese and Japanese text may break a line, and where kinsoku
//! forbids it.
//!
//! Chinese and Japanese put no spaces between words, so a line may end
//! between any two characters — except that some may not begin a line
//! (closing brackets and punctuation, small kana, the prolonged sound mark)
//! and some may not end one (opening brackets). Those are the kinsoku
//! rules, and InDesign applies them by default. Korean is set word by word,
//! at its spaces, as Latin text is, so hangul takes no breaks between its
//! letters here.
//!
//! The character ranges and the kinsoku sets are DesignCraft's
//! (`crates/compose/src/breaker.rs` and `lib.rs`), used under its MIT
//! licence; see `ATTRIBUTION.md`.

/// Whether `c` is set as an ideograph is: Han, kana, CJK punctuation and
/// symbols, fullwidth forms. Hangul is not, because Korean breaks at spaces.
pub fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x2E80..=0x2FFF        // radicals, ideographic description
        | 0x3000..=0x30FF      // CJK punctuation, hiragana, katakana
        | 0x3100..=0x312F      // bopomofo
        | 0x31A0..=0x31FF      // bopomofo extended, strokes, katakana extension
        | 0x3200..=0x33FF      // enclosed and compatibility
        | 0x3400..=0x4DBF      // Han extension A
        | 0x4E00..=0x9FFF      // Han
        | 0xF900..=0xFAFF      // compatibility ideographs
        | 0xFE10..=0xFE1F      // vertical forms
        | 0xFE30..=0xFE4F      // compatibility forms
        | 0xFF01..=0xFF60      // fullwidth forms
        | 0xFFE0..=0xFFE6
        | 0x20000..=0x3FFFF) // Han extensions B and on
}

/// Characters a line may not begin with.
const NO_START: &str = "、。，．・：；？！ー）」』】〕〉》｝］〙〗ぁぃぅぇぉっゃゅょゎァィゥェォッャュョヮヵヶ々〻ゝゞヽヾ!),.:;?]}";
/// Characters a line may not end with.
const NO_END: &str = "（「『【〔〈《｛［〘〖([{";

/// Whether kinsoku lets a line end between `a` and `b`.
pub fn kinsoku_allows(a: char, b: char) -> bool {
    !NO_START.contains(b) && !NO_END.contains(a)
}

/// Whether a line may end between `a` and `b` with no space between them:
/// one of them is set as an ideograph, and kinsoku does not forbid it.
pub fn break_between(a: char, b: char) -> bool {
    (is_cjk(a) || is_cjk(b)) && kinsoku_allows(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ideographs_break_anywhere_but_kinsoku_holds() {
        assert!(break_between('日', '本'), "between two ideographs");
        assert!(
            !break_between('本', '。'),
            "a full stop does not start a line"
        );
        assert!(
            !break_between('「', '本'),
            "an opening bracket does not end one"
        );
        assert!(!break_between('カ', 'ッ'), "nor a small kana start one");
        assert!(!break_between('a', 'b'), "Latin breaks at spaces");
        assert!(!break_between('한', '국'), "and so does Korean");
    }
}
