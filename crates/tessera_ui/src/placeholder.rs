//! Type > Fill with Placeholder Text: InDesign's, which pours Latin into a
//! text frame — through its whole thread — until it is exactly full.
//!
//! **Full, not overset.** A frame filled past its end shows the overset mark
//! and says the layout has a problem it does not have; one left short does
//! not show what the column will look like. So the text is cut at the last
//! word the thread can hold, found by laying the story out on a copy of the
//! document: never the frame's area times a guess at the words per square
//! inch, which is wrong the moment the type is larger, the columns narrower
//! or a picture wraps the text away.

use tessera_document::ids::FrameId;
use tessera_document::nodes::FrameKind;

use crate::app::TesseraApp;
use crate::command::{Command, apply};

/// The text poured in, one paragraph each, round again when it runs out.
const LATIN: &[&str] = &[
    "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.",
    "Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum.",
    "Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium doloremque laudantium, totam rem aperiam, eaque ipsa quae ab illo inventore veritatis et quasi architecto beatae vitae dicta sunt explicabo.",
    "Nemo enim ipsam voluptatem quia voluptas sit aspernatur aut odit aut fugit, sed quia consequuntur magni dolores eos qui ratione voluptatem sequi nesciunt. Neque porro quisquam est, qui dolorem ipsum quia dolor sit amet.",
    "At vero eos et accusamus et iusto odio dignissimos ducimus qui blanditiis praesentium voluptatum deleniti atque corrupti quos dolores et quas molestias excepturi sint occaecati cupiditate non provident.",
];

/// The most words a fill will pour: a long thread across many pages, and a
/// bound on the search when the frames have no room to run out of.
const MOST_WORDS: usize = 20_000;

/// The first `n` words of the placeholder text, its paragraphs as paragraphs.
pub fn words(n: usize) -> String {
    let mut out = String::new();
    let mut count = 0;
    'fill: for paragraph in LATIN.iter().cycle() {
        if !out.is_empty() {
            out.push('\n');
        }
        for (i, word) in paragraph.split(' ').enumerate() {
            if count == n {
                break 'fill;
            }
            if i > 0 {
                out.push(' ');
            }
            out.push_str(word);
            count += 1;
        }
        if count == n {
            break;
        }
    }
    // Cut mid-sentence, it still ends as a sentence does.
    let trimmed = out.trim_end_matches([',', ' ']);
    let mut out = trimmed.to_string();
    if !out.is_empty() && !out.ends_with('.') {
        out.push('.');
    }
    out
}

/// Fill the story `frame` shows with as much placeholder text as its thread
/// holds, as one undo step. `false` when `frame` is not a text frame.
pub fn fill(state: &mut TesseraApp, frame: FrameId) -> bool {
    let Some(n) = fitting(state, frame) else {
        return false;
    };
    apply(
        state,
        Command::SetText {
            id: frame,
            text: words(n),
        },
    );
    true
}

/// How many words the thread through `frame` holds without overset.
fn fitting(state: &mut TesseraApp, frame: FrameId) -> Option<usize> {
    let doc = state.active().document();
    let Some(FrameKind::Text { story, .. }) = doc.frame(frame).map(|f| &f.kind) else {
        return None;
    };
    let story = *story;
    let thread = doc.thread_of(frame);
    let base = doc.story(story)?.clone();
    let mut probe = doc.clone();
    let shaper = &mut state.shaper;
    let mut fits = |n: usize| -> bool {
        let mut s = base.clone();
        s.set_text(words(n));
        probe.replace_story_from_edit(story, s);
        let laid = tessera_layout::resolve::resolve(&probe, shaper);
        !laid.items.iter().any(|item| {
            thread.contains(&item.frame)
                && matches!(
                    item.kind,
                    tessera_layout::resolve::ResolvedKind::Text { overset_lines, .. }
                        if overset_lines > 0
                )
        })
    };
    // Out in doubling steps until it no longer fits, then halved back: a
    // dozen or so layouts, however long the thread.
    let mut good = 0;
    let mut bad = 16;
    while fits(bad) {
        good = bad;
        if bad >= MOST_WORDS {
            return Some(MOST_WORDS);
        }
        bad = (bad * 2).min(MOST_WORDS);
    }
    while bad - good > 1 {
        let mid = (good + bad) / 2;
        if fits(mid) {
            good = mid;
        } else {
            bad = mid;
        }
    }
    // Even one word too many for the frame: one word, overset, says what
    // happened better than nothing at all.
    Some(good.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_geometry::DocRect;

    #[test]
    fn the_words_are_counted_and_end_as_a_sentence() {
        assert_eq!(words(2), "Lorem ipsum.");
        assert_eq!(words(5), "Lorem ipsum dolor sit amet.");
        assert_eq!(words(0), "");
        let long = words(200);
        assert_eq!(long.split_whitespace().count(), 200);
        assert!(long.contains('\n'), "paragraphs, not one block");
    }

    #[test]
    fn a_frame_is_filled_exactly_full() {
        let mut state = TesseraApp::headless();
        let page = state.first_page_bounds();
        apply(
            &mut state,
            Command::AddTextFrame(DocRect {
                x: page.x + 36.0,
                y: page.y + 36.0,
                width: 200.0,
                height: 120.0,
            }),
        );
        let id = state.active().selection.single().expect("a frame");
        assert!(fill(&mut state, id));
        let n = fitting(&mut state, id).expect("a text frame");
        assert!(n > 10, "a 200 by 120 frame holds more than {n} words");

        let doc = state.active().document();
        let Some(FrameKind::Text { story, .. }) = doc.frame(id).map(|f| &f.kind) else {
            panic!("a text frame");
        };
        let story = *story;
        let text = doc.story(story).unwrap().text.clone();
        assert_eq!(text.split_whitespace().count(), n);
        // Not overset.
        let laid = state.resolve_active();
        assert!(laid.items.iter().all(|i| !matches!(
            i.kind,
            tessera_layout::resolve::ResolvedKind::Text { overset_lines, .. } if overset_lines > 0
        )));
        // And one undo empties it again.
        apply(&mut state, Command::Undo);
        let doc = state.active().document();
        assert!(doc.story(story).unwrap().text.is_empty());
    }
}
