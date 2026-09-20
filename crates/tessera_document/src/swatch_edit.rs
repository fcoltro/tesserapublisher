//! Renaming a global colour must keep every reference attached to it.
use crate::{
    document::Document,
    nodes::{FrameKind, Swatch},
    paint::Paint,
};
use tessera_color::Color;
use tessera_text::story::{CharacterFormat, ParagraphFormat, Story};

struct Rename<'a> {
    old: &'a str,
    new: &'a str,
}

impl Rename<'_> {
    fn colour(&self, colour: &mut Color) {
        if let Color::Swatch { name, .. } = colour
            && name == self.old
        {
            *name = self.new.to_owned();
        }
    }
    fn paint(&self, paint: &mut Paint) {
        match paint {
            Paint::Solid(c) => self.colour(c),
            Paint::Gradient(g) => {
                let mut stops = g.stops().to_vec();
                for stop in &mut stops {
                    self.colour(&mut stop.colour);
                }
                g.set_stops(stops);
            }
        }
    }
    fn character(&self, format: &mut CharacterFormat) {
        if let Some(c) = &mut format.colour {
            self.colour(c);
        }
        for decoration in [&mut format.underline, &mut format.strikethrough]
            .into_iter()
            .flatten()
        {
            if let Some(c) = &mut decoration.colour {
                self.colour(c);
            }
        }
    }
    fn paragraph(&self, format: &mut ParagraphFormat) {
        self.character(&mut format.character);
        for rule in [&mut format.rule_above, &mut format.rule_below]
            .into_iter()
            .flatten()
        {
            if let Some(c) = &mut rule.colour {
                self.colour(c);
            }
        }
    }
    fn story(&self, story: &mut Story) {
        for run in &mut story.runs {
            self.character(&mut run.local);
        }
        for paragraph in &mut story.paragraphs {
            self.paragraph(&mut paragraph.local);
        }
        for footnote in &mut story.footnotes {
            self.story(footnote);
        }
    }
}

impl Document {
    /// Atomically edit a swatch, preserving its list position and references.
    /// Reject missing sources, empty names and collisions without changing data.
    pub fn edit_swatch(&mut self, old: &str, edited: Swatch) -> bool {
        let Some(index) = self.swatches.iter().position(|s| s.name == old) else {
            return false;
        };
        if edited.name.trim().is_empty()
            || (edited.name != old && self.swatch(&edited.name).is_some())
        {
            return false;
        }
        let rename = Rename {
            old,
            new: &edited.name,
        };
        if old != edited.name {
            for frame in self.frames.values_mut() {
                rename.paint(&mut frame.fill);
                if let Some(s) = &mut frame.stroke {
                    rename.colour(&mut s.color);
                }
                if let Some(s) = &mut frame.shadow {
                    rename.colour(&mut s.colour);
                }
                if let FrameKind::Table(table) = &mut frame.kind {
                    if let Some(s) = &mut table.stroke {
                        rename.colour(&mut s.color);
                    }
                    for cell in table.cells.iter_mut().filter_map(|s| s.cell_mut()) {
                        if let Some(p) = &mut cell.fill {
                            rename.paint(p);
                        }
                    }
                }
            }
            for style in self.object_styles.values_mut() {
                if let Some(p) = &mut style.format.fill {
                    rename.paint(p);
                }
                if let Some(Some(s)) = &mut style.format.stroke {
                    rename.colour(&mut s.color);
                }
                if let Some(Some(s)) = &mut style.format.shadow {
                    rename.colour(&mut s.colour);
                }
            }
            for style in self.character_styles.values_mut() {
                rename.character(&mut style.format);
            }
            for style in self.paragraph_styles.values_mut() {
                rename.paragraph(&mut style.format);
            }
            for story in self.stories.values_mut() {
                rename.story(story);
            }
            rename.colour(&mut self.text_default.color);
            for swatch in &mut self.swatches {
                rename.colour(&mut swatch.colour);
            }
        }
        let mut edited = edited.clone();
        rename.colour(&mut edited.colour);
        self.swatches[index] = edited;
        self.touch();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(name: &str) -> Color {
        Color::Swatch {
            name: name.into(),
            tint: 0.7,
        }
    }

    #[test]
    fn rename_updates_aliases_gradients_styles_and_nested_text() {
        let mut doc = Document::new();
        doc.set_swatch(Swatch::new("Old", Color::BLACK));
        doc.set_swatch(Swatch::new("Alias", reference("Old")));
        doc.text_default.color = reference("Old");
        let mut story = Story::new("Text");
        story.runs[0].local.colour = Some(reference("Old"));
        story.footnotes.push(story.clone());
        let story_id = doc.add_story(story);
        let style_id = doc.object_styles.insert(crate::object_style::ObjectStyle {
            name: "Object".into(),
            based_on: None,
            format: crate::object_style::ObjectFormat {
                fill: Some(Paint::Gradient(crate::paint::Gradient::new(
                    crate::paint::Ramp::Radial,
                    vec![
                        crate::paint::Stop {
                            at: 0.0,
                            colour: reference("Old"),
                        },
                        crate::paint::Stop {
                            at: 1.0,
                            colour: Color::BLACK,
                        },
                    ],
                ))),
                ..Default::default()
            },
        });
        assert!(doc.edit_swatch("Old", Swatch::new("New", Color::BLACK)));
        assert_eq!(doc.swatches[0].name, "New");
        assert_eq!(doc.swatches[1].colour, reference("New"));
        assert_eq!(doc.text_default.color, reference("New"));
        assert_eq!(
            doc.stories[story_id].runs[0].local.colour,
            Some(reference("New"))
        );
        assert_eq!(
            doc.stories[story_id].footnotes[0].runs[0].local.colour,
            Some(reference("New"))
        );
        let Some(Paint::Gradient(gradient)) = &doc.object_styles[style_id].format.fill else {
            panic!("gradient lost")
        };
        assert_eq!(gradient.stops()[0].colour, reference("New"));
    }

    #[test]
    fn invalid_names_never_overwrite_another_swatch() {
        let mut doc = Document::new();
        doc.set_swatch(Swatch::new("First", Color::BLACK));
        doc.set_swatch(Swatch::new("Second", Color::WHITE));
        let before = doc.swatches.clone();
        for (old, new) in [("First", "Second"), ("First", "  "), ("Missing", "New")] {
            assert!(!doc.edit_swatch(old, Swatch::new(new, Color::BLACK)));
            assert_eq!(doc.swatches, before);
        }
    }
}
