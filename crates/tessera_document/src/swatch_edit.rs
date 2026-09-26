//! Renaming a global colour must keep every reference attached to it.
use crate::{
    document::Document,
    nodes::{FrameKind, Swatch},
    paint::Paint,
};
use tessera_color::Color;
use tessera_text::story::{CharacterFormat, ParagraphFormat, Story};

/// Every colour in the document that could name a swatch, handed to `f` in
/// turn: the one walk both a rename and a replacement make, so neither can
/// miss a place the other reaches.
struct Recolour<F: FnMut(&mut Color)> {
    f: F,
}

impl<F: FnMut(&mut Color)> Recolour<F> {
    fn colour(&mut self, colour: &mut Color) {
        (self.f)(colour);
    }
    fn paint(&mut self, paint: &mut Paint) {
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
    fn character(&mut self, format: &mut CharacterFormat) {
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
    fn paragraph(&mut self, format: &mut ParagraphFormat) {
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
    fn story(&mut self, story: &mut Story) {
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
    fn document(&mut self, doc: &mut Document) {
        for frame in doc.frames.values_mut() {
            self.paint(&mut frame.fill);
            if let Some(s) = &mut frame.stroke {
                self.colour(&mut s.color);
            }
            if let Some(s) = &mut frame.shadow {
                self.colour(&mut s.colour);
            }
            if let FrameKind::Table(table) = &mut frame.kind {
                if let Some(s) = &mut table.stroke {
                    self.colour(&mut s.color);
                }
                for cell in table.cells.iter_mut().filter_map(|s| s.cell_mut()) {
                    if let Some(p) = &mut cell.fill {
                        self.paint(p);
                    }
                }
            }
        }
        for style in doc.object_styles.values_mut() {
            if let Some(p) = &mut style.format.fill {
                self.paint(p);
            }
            if let Some(Some(s)) = &mut style.format.stroke {
                self.colour(&mut s.color);
            }
            if let Some(Some(s)) = &mut style.format.shadow {
                self.colour(&mut s.colour);
            }
        }
        for style in doc.character_styles.values_mut() {
            self.character(&mut style.format);
        }
        for style in doc.paragraph_styles.values_mut() {
            self.paragraph(&mut style.format);
        }
        for story in doc.stories.values_mut() {
            self.story(story);
        }
        self.colour(&mut doc.text_default.color);
        for swatch in &mut doc.swatches {
            self.colour(&mut swatch.colour);
        }
    }
}

/// What the colour naming `old` should become, given the tint it named it at.
fn renaming(old: &str, mut to: impl FnMut(f32) -> Color) -> impl FnMut(&mut Color) {
    move |colour| {
        if let Color::Swatch { name, tint } = colour
            && name == old
        {
            *colour = to(*tint);
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
        let new = edited.name.clone();
        let mut rename = renaming(old, |tint| Color::Swatch {
            name: new.clone(),
            tint,
        });
        if old != edited.name {
            Recolour { f: &mut rename }.document(self);
        }
        let mut edited = edited.clone();
        rename(&mut edited.colour);
        self.swatches[index] = edited;
        self.touch();
        true
    }

    /// Remove a swatch and hand everything using it to something else: to
    /// the swatch `with`, at the tint each use named, or — with `None` — to
    /// the colour it stood for, written into each use as a colour of its own.
    ///
    /// [`Document::remove_swatch`] leaves the uses naming nothing, which
    /// draws them in the alarming magenta, and that is the honest thing for
    /// a delete nobody was asked about. This is the delete somebody *was*
    /// asked about, as InDesign asks "replace with": the objects keep a
    /// colour, and the one they keep is the one chosen.
    ///
    /// A spot swatch replaced by its value stays an ink: the uses carry the
    /// spot, plate and all, rather than its process fallback.
    ///
    /// Refused, changing nothing, when the swatch is not there, or `with` is
    /// not there or is the swatch itself.
    pub fn replace_swatch(&mut self, name: &str, with: Option<&str>) -> bool {
        if self.swatch(name).is_none()
            || with.is_some_and(|other| other == name || self.swatch(other).is_none())
        {
            return false;
        }
        // What the swatch stands for, taken before anything is rewritten: a
        // tint of it is this at that tint, since every tint is a step toward
        // the paper and two steps multiply.
        let full = self.resolve_colour(&Color::Swatch {
            name: name.to_owned(),
            tint: 1.0,
        });
        let with = with.map(str::to_owned);
        let mut replace = renaming(name, |tint| match &with {
            Some(other) => Color::Swatch {
                name: other.clone(),
                tint,
            },
            None => full.tinted(tint),
        });
        Recolour { f: &mut replace }.document(self);
        self.swatches.retain(|s| s.name != name);
        self.touch();
        true
    }
}

/// Every place a swatch is named, gathered: what editing it recolours, and
/// what deleting it would leave drawing in the alarming magenta of a colour
/// nobody defined.
///
/// The counterpart of [`Document::edit_swatch`]'s rename, and it walks the
/// same places: a count that looked only at fills and strokes, as the
/// Swatches panel's did, said "0 in use" of a swatch colouring every heading
/// in the book.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SwatchReferences {
    /// Objects whose fill (a gradient's stops included), stroke, shadow, or
    /// table stroke or cells name it, in paint order.
    pub frames: Vec<crate::ids::FrameId>,
    /// Stories whose own formatting names it: a run's colour or its
    /// underline's, a paragraph's rules.
    pub stories: Vec<crate::ids::StoryId>,
    pub paragraph_styles: Vec<tessera_text::story::ParagraphStyleId>,
    pub character_styles: Vec<tessera_text::story::CharacterStyleId>,
    pub object_styles: Vec<crate::ids::ObjectStyleId>,
    /// Other swatches defined from it — its tints — by name.
    pub swatches: Vec<String>,
    /// Whether the document's default text colour is it.
    pub text_default: bool,
}

impl SwatchReferences {
    /// How many places name it, each counted once.
    pub fn count(&self) -> usize {
        self.frames.len()
            + self.stories.len()
            + self.paragraph_styles.len()
            + self.character_styles.len()
            + self.object_styles.len()
            + self.swatches.len()
            + usize::from(self.text_default)
    }

    pub fn is_empty(&self) -> bool {
        self.count() == 0
    }
}

/// Whether a colour is the swatch `name`, at any tint.
fn names(colour: &Color, name: &str) -> bool {
    matches!(colour, Color::Swatch { name: n, .. } if n == name)
}

fn paint_names(paint: &Paint, name: &str) -> bool {
    match paint {
        Paint::Solid(c) => names(c, name),
        Paint::Gradient(g) => g.stops().iter().any(|stop| names(&stop.colour, name)),
    }
}

/// Whether a character format names the swatch: its colour, or its
/// underline's or strikethrough's.
pub fn character_names_swatch(format: &CharacterFormat, name: &str) -> bool {
    format.colour.as_ref().is_some_and(|c| names(c, name))
        || [&format.underline, &format.strikethrough]
            .into_iter()
            .flatten()
            .any(|d| d.colour.as_ref().is_some_and(|c| names(c, name)))
}

/// Whether a paragraph format names the swatch: its character half, or its
/// rules.
pub fn paragraph_names_swatch(format: &ParagraphFormat, name: &str) -> bool {
    character_names_swatch(&format.character, name)
        || [&format.rule_above, &format.rule_below]
            .into_iter()
            .flatten()
            .any(|r| r.colour.as_ref().is_some_and(|c| names(c, name)))
}

fn story_names_swatch(story: &Story, name: &str) -> bool {
    story
        .runs
        .iter()
        .any(|run| character_names_swatch(&run.local, name))
        || story
            .paragraphs
            .iter()
            .any(|p| paragraph_names_swatch(&p.local, name))
        || story.footnotes.iter().any(|f| story_names_swatch(f, name))
}

fn frame_names_swatch(frame: &crate::nodes::Frame, name: &str) -> bool {
    paint_names(&frame.fill, name)
        || frame.stroke.as_ref().is_some_and(|s| names(&s.color, name))
        || frame
            .shadow
            .as_ref()
            .is_some_and(|s| names(&s.colour, name))
        || match &frame.kind {
            FrameKind::Table(table) => {
                table.stroke.as_ref().is_some_and(|s| names(&s.color, name))
                    || table
                        .cells
                        .iter()
                        .filter_map(|slot| slot.cell())
                        .any(|cell| cell.fill.as_ref().is_some_and(|p| paint_names(p, name)))
            }
            _ => false,
        }
}

impl Document {
    /// Every place the swatch `name` is used. See [`SwatchReferences`].
    pub fn swatch_references(&self, name: &str) -> SwatchReferences {
        let order = self.paint_order();
        let mut frames: Vec<crate::ids::FrameId> = self
            .frames
            .iter()
            .filter(|(_, frame)| frame_names_swatch(frame, name))
            .map(|(id, _)| id)
            .collect();
        // Reading order, for going to each; anything paint order does not
        // list goes last rather than being dropped from the count.
        frames.sort_by_key(|id| order.iter().position(|o| o == id).unwrap_or(usize::MAX));
        SwatchReferences {
            frames,
            stories: self
                .stories
                .iter()
                .filter(|(_, story)| story_names_swatch(story, name))
                .map(|(id, _)| id)
                .collect(),
            paragraph_styles: self
                .paragraph_styles
                .iter()
                .filter(|(_, style)| paragraph_names_swatch(&style.format, name))
                .map(|(id, _)| id)
                .collect(),
            character_styles: self
                .character_styles
                .iter()
                .filter(|(_, style)| character_names_swatch(&style.format, name))
                .map(|(id, _)| id)
                .collect(),
            object_styles: self
                .object_style_order
                .iter()
                .copied()
                .filter(|id| {
                    self.object_styles.get(*id).is_some_and(|style| {
                        let f = &style.format;
                        f.fill.as_ref().is_some_and(|p| paint_names(p, name))
                            || matches!(&f.stroke, Some(Some(s)) if names(&s.color, name))
                            || matches!(&f.shadow, Some(Some(s)) if names(&s.colour, name))
                    })
                })
                .collect(),
            swatches: self
                .swatches
                .iter()
                .filter(|s| s.name != name && names(&s.colour, name))
                .map(|s| s.name.clone())
                .collect(),
            text_default: names(&self.text_default.color, name),
        }
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

    #[test]
    fn every_place_a_swatch_is_named_is_found_and_the_rename_leaves_none_behind() {
        // The Swatches panel counted fills and strokes, so a swatch used only
        // by text, a style, a shadow or a tint read as unused: "0 in use"
        // above a delete that would leave all of them magenta.
        let mut doc = Document::new();
        doc.set_swatch(Swatch::new("Brand", Color::BLACK));
        doc.set_swatch(Swatch::new("Brand 50%", reference("Brand")));
        doc.text_default.color = reference("Brand");

        let mut story = Story::new("Text");
        story.runs[0].local.underline = Some(tessera_text::story::Decoration {
            colour: Some(reference("Brand")),
            ..Default::default()
        });
        let story_id = doc.add_story(story);

        let layer = doc.default_layer().expect("a layer");
        let frame = doc.add_frame(
            layer,
            crate::nodes::Frame {
                bounds: tessera_geometry::DocRect {
                    x: 10.0,
                    y: 20.0,
                    width: 100.0,
                    height: 50.0,
                },
                kind: FrameKind::Rectangle,
                transform: tessera_geometry::Transform::IDENTITY,
                fill: Paint::Solid(Color::BLACK),
                stroke: None,
                wrap: crate::nodes::TextWrap::None,
                blend: crate::blending::Blending::PLAIN,
                corners: crate::corners::Corners::SQUARE,
                // Named only by its shadow: the place a fill-and-stroke count
                // missed.
                shadow: Some(crate::shadow::Shadow {
                    colour: reference("Brand"),
                    ..crate::shadow::Shadow::TYPICAL
                }),
                anchor: None,
                style: None,
            },
        );

        let paragraph = doc.add_paragraph_style(tessera_text::story::ParagraphStyle {
            name: "Ruled".into(),
            based_on: None,
            format: ParagraphFormat {
                rule_below: Some(tessera_text::story::ParagraphRule {
                    colour: Some(reference("Brand")),
                    ..Default::default()
                }),
                ..Default::default()
            },
        });
        let character = doc.add_character_style(tessera_text::story::CharacterStyle {
            name: "Coloured".into(),
            based_on: None,
            format: CharacterFormat {
                colour: Some(reference("Brand")),
                ..Default::default()
            },
        });
        let object = doc.add_object_style(crate::object_style::ObjectStyle {
            name: "Stroked".into(),
            based_on: None,
            format: crate::object_style::ObjectFormat {
                stroke: Some(Some(crate::nodes::Stroke::new(reference("Brand"), 1.0))),
                ..Default::default()
            },
        });

        let found = doc.swatch_references("Brand");
        assert_eq!(found.frames, [frame]);
        assert_eq!(found.stories, [story_id]);
        assert_eq!(found.paragraph_styles, [paragraph]);
        assert_eq!(found.character_styles, [character]);
        assert_eq!(found.object_styles, [object]);
        assert_eq!(found.swatches, ["Brand 50%"]);
        assert!(found.text_default);
        assert_eq!(found.count(), 7);

        assert!(doc.edit_swatch("Brand", Swatch::new("House", Color::BLACK)));
        assert!(
            doc.swatch_references("Brand").is_empty(),
            "nothing still names the old one"
        );
        assert_eq!(
            doc.swatch_references("House").count(),
            7,
            "and all of it names the new"
        );
    }

    fn frame_filled(doc: &mut Document, fill: Color) -> crate::ids::FrameId {
        let layer = doc.default_layer().expect("a layer");
        doc.add_frame(
            layer,
            crate::nodes::Frame {
                bounds: tessera_geometry::DocRect {
                    x: 0.0,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                },
                kind: FrameKind::Rectangle,
                transform: tessera_geometry::Transform::IDENTITY,
                fill: Paint::Solid(fill),
                stroke: None,
                wrap: crate::nodes::TextWrap::None,
                blend: crate::blending::Blending::PLAIN,
                corners: crate::corners::Corners::SQUARE,
                shadow: None,
                anchor: None,
                style: None,
            },
        )
    }

    #[test]
    fn deleting_a_swatch_can_hand_its_uses_to_another_at_the_tint_each_named() {
        let mut doc = Document::new();
        doc.set_swatch(Swatch::new("Brand", Color::BLACK));
        doc.set_swatch(Swatch::new("Ink", Color::WHITE));
        let frame = frame_filled(&mut doc, reference("Brand"));
        let mut story = Story::new("Text");
        story.runs[0].local.colour = Some(reference("Brand"));
        let story = doc.add_story(story);

        assert!(doc.replace_swatch("Brand", Some("Ink")));
        assert!(doc.swatch("Brand").is_none());
        assert_eq!(doc.frames[frame].fill, Paint::Solid(reference("Ink")));
        assert_eq!(
            doc.stories[story].runs[0].local.colour,
            Some(reference("Ink"))
        );
        assert!(doc.swatch_references("Brand").is_empty());
    }

    #[test]
    fn deleting_a_swatch_can_keep_its_colour_in_every_use_instead() {
        // Rather than the magenta of a name nobody defines: each use keeps
        // what it looked like, tint and all.
        let cmyk = Color::Cmyk {
            c: 0.0,
            m: 0.8,
            y: 0.6,
            k: 0.0,
            a: 1.0,
        };
        let mut doc = Document::new();
        doc.set_swatch(Swatch::new("Brand", cmyk.clone()));
        doc.set_swatch(Swatch::new("Brand tint", reference("Brand")));
        let at_full = frame_filled(
            &mut doc,
            Color::Swatch {
                name: "Brand".into(),
                tint: 1.0,
            },
        );
        let tinted = frame_filled(&mut doc, reference("Brand"));
        let before = doc.resolve_colour(&reference("Brand"));

        assert!(doc.replace_swatch("Brand", None));
        assert_eq!(doc.frames[at_full].fill, Paint::Solid(cmyk.clone()));
        assert_eq!(doc.frames[tinted].fill, Paint::Solid(before));
        assert_eq!(
            doc.swatch("Brand tint").map(|s| s.colour.clone()),
            Some(cmyk.tinted(0.7)),
            "a tint of it becomes a colour of its own"
        );
    }

    #[test]
    fn a_spot_swatch_replaced_by_its_value_stays_an_ink() {
        let mut doc = Document::new();
        doc.set_swatch(Swatch {
            name: "PANTONE 185 C".into(),
            colour: Color::Cmyk {
                c: 0.0,
                m: 0.9,
                y: 0.8,
                k: 0.0,
                a: 1.0,
            },
            spot: true,
        });
        let frame = frame_filled(&mut doc, reference("PANTONE 185 C"));
        assert!(doc.replace_swatch("PANTONE 185 C", None));
        assert!(
            matches!(
                &doc.frames[frame].fill,
                Paint::Solid(Color::Spot { name, tint, .. })
                    if name == "PANTONE 185 C" && (*tint - 0.7).abs() < 1e-6
            ),
            "still its own plate: {:?}",
            doc.frames[frame].fill
        );
    }

    #[test]
    fn a_replacement_that_is_missing_or_the_swatch_itself_is_refused() {
        let mut doc = Document::new();
        doc.set_swatch(Swatch::new("Brand", Color::BLACK));
        let before = doc.revision();
        assert!(!doc.replace_swatch("Brand", Some("Nothing")));
        assert!(!doc.replace_swatch("Brand", Some("Brand")));
        assert!(!doc.replace_swatch("Nothing", None));
        assert_eq!(doc.revision(), before);
        assert!(doc.swatch("Brand").is_some());
    }
}
