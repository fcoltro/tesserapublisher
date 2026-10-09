//! A family replaced everywhere it is named, as InDesign's Find Font changes
//! one: what a missing font is fixed with.

use crate::document::Document;
use tessera_text::story::Story;

impl Document {
    /// Set everything named in the family `from` in `to` instead: the
    /// document's default, the styles, and every run, paragraph and footnote
    /// that names it. How many places changed.
    ///
    /// Everywhere at once, because a family is missing from the machine and
    /// not from one frame: replacing it where one frame names it would leave
    /// the style the rest of the book is set in still asking for it.
    pub fn replace_family(&mut self, from: &str, to: &str) -> usize {
        if from == to {
            return 0;
        }
        let mut changed = 0;
        let mut swap = |family: &mut Option<String>| {
            if family.as_deref() == Some(from) {
                *family = Some(to.to_owned());
                changed += 1;
            }
        };
        for style in self.character_styles.values_mut() {
            swap(&mut style.format.family);
        }
        for style in self.paragraph_styles.values_mut() {
            swap(&mut style.format.character.family);
        }
        for story in self.stories.values_mut() {
            story_families(story, &mut swap);
        }
        if self.text_default.family == from {
            self.text_default.family = to.to_owned();
            changed += 1;
        }
        if changed > 0 {
            self.touch();
        }
        changed
    }

    /// Every family the document names, and in how many places — the
    /// default, each style, each run, paragraph and footnote that asks for
    /// it — sorted by name. What Find Font lists.
    ///
    /// Counted where [`Document::replace_family`] changes, so the number
    /// beside a family is the number of places a replacement will touch.
    pub fn families_used(&self) -> Vec<(String, usize)> {
        let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
        let mut count = |family: &Option<String>| {
            if let Some(family) = family {
                *counts.entry(family.clone()).or_default() += 1;
            }
        };
        for style in self.character_styles.values() {
            count(&style.format.family);
        }
        for style in self.paragraph_styles.values() {
            count(&style.format.character.family);
        }
        for story in self.stories.values() {
            story_families_read(story, &mut count);
        }
        count(&Some(self.text_default.family.clone()));
        let mut used: Vec<(String, usize)> = counts.into_iter().collect();
        used.sort_by_key(|(name, _)| name.to_lowercase());
        used
    }
}

fn story_families_read(story: &Story, count: &mut impl FnMut(&Option<String>)) {
    for run in &story.runs {
        count(&run.local.family);
    }
    for paragraph in &story.paragraphs {
        count(&paragraph.local.character.family);
    }
    for footnote in &story.footnotes {
        story_families_read(footnote, count);
    }
}

fn story_families(story: &mut Story, swap: &mut impl FnMut(&mut Option<String>)) {
    for run in &mut story.runs {
        swap(&mut run.local.family);
    }
    for paragraph in &mut story.paragraphs {
        swap(&mut paragraph.local.character.family);
    }
    for footnote in &mut story.footnotes {
        story_families(footnote, swap);
    }
}

#[cfg(test)]
mod tests {
    use tessera_text::story::{CharacterFormat, CharacterStyle, Story};

    use crate::document::Document;

    fn in_family(family: &str) -> CharacterFormat {
        CharacterFormat {
            family: Some(family.to_owned()),
            ..Default::default()
        }
    }

    #[test]
    fn a_family_is_replaced_wherever_it_is_named_and_nowhere_else() {
        let mut doc = Document::new();
        doc.text_default.family = "Gone Serif".into();
        doc.add_character_style(CharacterStyle {
            name: "Emphasis".into(),
            format: in_family("Gone Serif"),
            ..Default::default()
        });
        let mut story = Story::new("One two three");
        story.apply_character_format(0..3, &in_family("Gone Serif"));
        story.apply_character_format(4..7, &in_family("Kept Sans"));
        let mut note = Story::new("A note");
        note.apply_character_format(0..6, &in_family("Gone Serif"));
        story.footnotes.push(note);
        let id = doc.add_story(story);
        let revision = doc.revision();
        assert_eq!(
            doc.families_used(),
            [("Gone Serif".to_string(), 4), ("Kept Sans".to_string(), 1)],
            "counted where a replacement will reach"
        );

        let changed = doc.replace_family("Gone Serif", "Found Serif");
        assert_eq!(changed, 4, "the default, the style, a run, the footnote");
        assert!(doc.revision() > revision);
        assert_eq!(doc.text_default.family, "Found Serif");
        let families: Vec<Option<&str>> = doc.stories[id]
            .runs
            .iter()
            .map(|r| r.local.family.as_deref())
            .collect();
        assert!(families.contains(&Some("Found Serif")));
        assert!(
            families.contains(&Some("Kept Sans")),
            "another family stays"
        );
        assert!(!families.contains(&Some("Gone Serif")));
        assert_eq!(
            doc.stories[id].footnotes[0].runs[0].local.family.as_deref(),
            Some("Found Serif")
        );
    }

    #[test]
    fn a_family_nothing_names_changes_nothing() {
        let mut doc = Document::new();
        let revision = doc.revision();
        assert_eq!(doc.replace_family("Nowhere", "Somewhere"), 0);
        assert_eq!(doc.revision(), revision, "not an edit");
        let own = doc.text_default.family.clone();
        assert_eq!(doc.replace_family(&own, &own), 0);
    }
}
