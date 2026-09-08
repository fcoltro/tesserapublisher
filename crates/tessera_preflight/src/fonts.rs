//! Which type families a document actually sets type in.
//!
//! Asked by two callers for two reasons — preflight, to find the ones this
//! machine cannot resolve, and packaging, to list what a printer will need — and
//! it lives here so there is **one answer**. Two walks over the same document
//! would drift, and the way they would drift is that one of them would forget
//! the run-local families, which is the half that matters.

use tessera_document::document::Document;
use tessera_document::ids::FrameId;
use tessera_document::nodes::FrameKind;

/// Every font family the document sets type in, sorted.
///
/// From the styles **and** from the runs. A run can name a family the styles
/// never mention — somebody selects a word and picks a face — and a font list
/// that missed those would be a list a printer trusted and was wrong about.
pub fn families(doc: &Document) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut note = |family: Option<&String>| {
        if let Some(name) = family
            && !out.contains(name)
        {
            out.push(name.clone());
        }
    };

    // The document default is always set, so it goes in directly.
    note(Some(&doc.text_default.family));
    for style in doc.character_styles.values() {
        note(style.format.family.as_ref());
    }
    for style in doc.paragraph_styles.values() {
        note(style.format.character.family.as_ref());
    }
    for story in doc.stories.values() {
        for run in &story.runs {
            note(run.local.family.as_ref());
        }
    }

    out.sort();
    out
}

/// The first text frame whose own runs name this family, if any.
///
/// **For jumping to.** A family named only by the document default or by a style
/// belongs to the document rather than to any one frame, and this says so by
/// returning nothing rather than picking the first text frame it can find — a
/// jump to an arbitrary frame is worse than no jump, because it looks like an
/// answer.
pub fn first_frame_using(doc: &Document, family: &str) -> Option<FrameId> {
    doc.paint_order().into_iter().find(|id| {
        let Some(frame) = doc.frame(*id) else {
            return false;
        };
        let FrameKind::Text { story, .. } = frame.kind else {
            return false;
        };
        doc.stories.get(story).is_some_and(|story| {
            story
                .runs
                .iter()
                .any(|run| run.local.family.as_deref() == Some(family))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_document_default_is_always_listed() {
        // It is what every run falls back to, so it is always in use even when
        // nothing names it.
        let doc = Document::new();
        assert!(families(&doc).contains(&doc.text_default.family));
    }

    #[test]
    fn the_list_has_no_repeats() {
        let doc = Document::new();
        let mut seen = families(&doc);
        let total = seen.len();
        seen.dedup();
        assert_eq!(seen.len(), total);
    }

    #[test]
    fn a_family_nothing_names_has_no_frame_to_jump_to() {
        // And says so, rather than offering the first text frame it finds: a
        // jump to an arbitrary frame looks like an answer.
        let doc = Document::new();
        assert_eq!(first_frame_using(&doc, "Nothing At All"), None);
    }
}
