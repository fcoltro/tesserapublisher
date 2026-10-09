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
    /// Whether the definitions are walked too — the swatches' own colours
    /// and the document's default text colour — or only what is coloured.
    definitions: bool,
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
        if self.definitions {
            self.colour(&mut doc.text_default.color);
            for swatch in &mut doc.swatches {
                self.colour(&mut swatch.colour);
            }
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
            Recolour {
                f: &mut rename,
                definitions: true,
            }
            .document(self);
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
        Recolour {
            f: &mut replace,
            definitions: true,
        }
        .document(self);
        self.swatches.retain(|s| s.name != name);
        self.touch();
        true
    }
}

impl Document {
    /// Hand every use of a colour name the document does not define to a
    /// swatch it does, each at the tint it named: the repair for a swatch
    /// deleted out from under its uses, or a colour pasted in from a document
    /// that had it.
    ///
    /// Refused, changing nothing, when `from` *is* defined — handing a real
    /// swatch's uses on is [`Document::replace_swatch`], which removes it —
    /// or when `to` is not.
    pub fn repoint_swatch(&mut self, from: &str, to: &str) -> bool {
        if self.swatch(from).is_some() || self.swatch(to).is_none() {
            return false;
        }
        let to = to.to_owned();
        let mut replace = renaming(from, |tint| Color::Swatch {
            name: to.clone(),
            tint,
        });
        Recolour {
            f: &mut replace,
            definitions: true,
        }
        .document(self);
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
    /// Every swatch name the document's colours refer to, defined or not, in
    /// the order they are first met.
    ///
    /// Read by the walk a rename makes, so no place a rename would reach is
    /// missed by whoever asks which names are used — the unresolved-swatch
    /// check looked at fills and strokes, and a heading coloured with a
    /// deleted swatch printed magenta without a word. The walk is written
    /// once, to rewrite; it reads a copy.
    pub fn swatch_names_used(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        let mut copy = self.clone();
        Recolour {
            f: |colour: &mut Color| {
                if let Color::Swatch { name, .. } = colour
                    && !names.contains(name)
                {
                    names.push(name.clone());
                }
            },
            definitions: true,
        }
        .document(&mut copy);
        names
    }

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

/// A colour an object, some text or a style uses as itself rather than
/// through a swatch, and which could be named: a process mix or a Lab
/// value, and not a spot (already an ink with a name) or nothing at all.
fn nameable(colour: &Color) -> bool {
    match colour {
        Color::Rgb { a, .. } | Color::Cmyk { a, .. } => *a > 0.0,
        Color::Lab { alpha, .. } => *alpha > 0.0,
        Color::Spot { .. } | Color::Swatch { .. } => false,
    }
}

/// The name InDesign gives a colour it names for you: its numbers, so two
/// people looking at "C=0 M=91 Y=76 K=0" know what it is without opening it.
pub fn colour_name(colour: &Color) -> String {
    let percent = |v: f32| (v * 100.0).round() as i32;
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as i32;
    match colour {
        Color::Cmyk { c, m, y, k, .. } => format!(
            "C={} M={} Y={} K={}",
            percent(*c),
            percent(*m),
            percent(*y),
            percent(*k)
        ),
        Color::Rgb { r, g, b, .. } => format!("R={} G={} B={}", byte(*r), byte(*g), byte(*b)),
        Color::Lab { l, a, b, .. } => format!(
            "L={} a={} b={}",
            l.round() as i32,
            a.round() as i32,
            b.round() as i32
        ),
        Color::Spot { name, .. } | Color::Swatch { name, .. } => name.clone(),
    }
}

/// What bringing swatches in from a file did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SwatchMerge {
    /// Swatches new to the document, under their own names.
    pub added: Vec<String>,
    /// Swatches whose name the document already gave a different colour,
    /// each brought in under the first free name after it: (theirs, ours).
    pub renamed: Vec<(String, String)>,
    /// Swatches the document already had, name, colour and ink alike — or
    /// had under the new name an earlier load gave one.
    pub already: usize,
    /// Of those, the ones found under an earlier load's new name: (theirs,
    /// ours), so a tint of one arriving now follows it there.
    pub renamed_before: Vec<(String, String)>,
}

impl Document {
    /// Every colour something is coloured with, and not the definitions:
    /// a swatch's own colour is what the swatch is, and the default text
    /// colour is a setting — a new document's is a plain black nobody
    /// chose, and naming it would be a swatch nobody asked for.
    fn over_uses(&mut self, f: impl FnMut(&mut Color)) {
        Recolour {
            f,
            definitions: false,
        }
        .document(self);
    }

    /// The colours used as themselves rather than through a swatch, each
    /// once, in the order first met — InDesign's "unnamed colours". A
    /// colour in `leave` is not one: the built-in paper and black are
    /// named already, by the panel.
    pub fn unnamed_colours(&self, leave: &[Color]) -> Vec<Color> {
        let mut found: Vec<Color> = Vec::new();
        let mut copy = self.clone();
        copy.over_uses(|colour: &mut Color| {
            if nameable(colour) && !leave.contains(colour) && !found.contains(colour) {
                found.push(colour.clone());
            }
        });
        found
    }

    /// Name every unnamed colour: a swatch for each, called by its numbers,
    /// and every use of it pointed at the swatch — so editing the swatch
    /// recolours what was using the colour, which is the reason to name it.
    /// The names made, in order.
    pub fn name_unnamed_colours(&mut self, leave: &[Color]) -> Vec<String> {
        let colours = self.unnamed_colours(leave);
        if colours.is_empty() {
            return Vec::new();
        }
        let mut named: Vec<(Color, String)> = Vec::new();
        for colour in colours {
            let stem = colour_name(&colour);
            let name = (1..)
                .map(|n| {
                    if n == 1 {
                        stem.clone()
                    } else {
                        format!("{stem} {n}")
                    }
                })
                .find(|name| {
                    self.swatch(name).is_none() && !named.iter().any(|(_, taken)| taken == name)
                })
                .expect("a free name");
            named.push((colour, name));
        }
        self.over_uses(|colour: &mut Color| {
            if let Some((_, name)) = named.iter().find(|(c, _)| c == colour) {
                *colour = Color::Swatch {
                    name: name.clone(),
                    tint: 1.0,
                };
            }
        });
        let names = named.iter().map(|(_, name)| name.clone()).collect();
        self.swatches.extend(
            named
                .into_iter()
                .map(|(colour, name)| Swatch::new(name, colour)),
        );
        self.touch();
        names
    }

    /// Move the swatch `name` to stand just before `before`, or at the end
    /// with none. By name rather than by index, as the panel chooses by
    /// name: a list filtered to three swatches has its own indexes. Whether
    /// anything moved.
    pub fn move_swatch(&mut self, name: &str, before: Option<&str>) -> bool {
        if before == Some(name) {
            return false;
        }
        let Some(from) = self.swatches.iter().position(|s| s.name == name) else {
            return false;
        };
        let to = match before {
            Some(other) => match self.swatches.iter().position(|s| s.name == other) {
                Some(at) => at,
                None => return false,
            },
            None => self.swatches.len(),
        };
        // Where it lands once it has been taken out of the list.
        let at = if to > from { to - 1 } else { to };
        if at == from {
            return false;
        }
        let swatch = self.swatches.remove(from);
        self.swatches.insert(at, swatch);
        self.touch();
        true
    }

    /// Put the swatches in order of their names, ignoring case, and numbers
    /// in them by value — "Blue 2" before "Blue 10". Whether the order
    /// changed.
    pub fn sort_swatches(&mut self) -> bool {
        let before: Vec<String> = self.swatches.iter().map(|s| s.name.clone()).collect();
        self.swatches.sort_by(|a, b| name_order(&a.name, &b.name));
        let changed = self
            .swatches
            .iter()
            .zip(&before)
            .any(|(s, name)| s.name != *name);
        if changed {
            self.touch();
        }
        changed
    }

    /// Bring swatches in from elsewhere — another document, a swatch
    /// exchange file. One the document has already, alike in name, colour
    /// and ink, is left as it is; one whose name the document gives another
    /// colour comes in under the next free name, and a tint of it follows
    /// it there, so nothing already coloured changes.
    pub fn add_swatches(&mut self, incoming: Vec<Swatch>) -> SwatchMerge {
        let mut merge = SwatchMerge::default();
        for mut swatch in incoming {
            if swatch.name.trim().is_empty() {
                continue;
            }
            if let Color::Swatch { name, .. } = &mut swatch.colour
                && let Some((_, ours)) = merge
                    .renamed
                    .iter()
                    .chain(&merge.renamed_before)
                    .find(|(theirs, _)| theirs == name)
            {
                *name = ours.clone();
            }
            let alike = |here: &Swatch| here.colour == swatch.colour && here.spot == swatch.spot;
            match self.swatch(&swatch.name) {
                Some(here) if alike(here) => {
                    merge.already += 1;
                }
                Some(_) => {
                    let theirs = swatch.name.clone();
                    // Brought in before, under the name it was given then:
                    // the same file loaded twice brings nothing the second
                    // time.
                    let before = (2..)
                        .map(|n| format!("{theirs} {n}"))
                        .map_while(|name| self.swatch(&name))
                        .find(|here| alike(here))
                        .map(|here| here.name.clone());
                    if let Some(ours) = before {
                        merge.already += 1;
                        merge.renamed_before.push((theirs, ours));
                        continue;
                    }
                    let ours = (2..)
                        .map(|n| format!("{theirs} {n}"))
                        .find(|name| self.swatch(name).is_none())
                        .expect("a free name");
                    swatch.name = ours.clone();
                    self.swatches.push(swatch);
                    merge.renamed.push((theirs, ours));
                }
                None => {
                    merge.added.push(swatch.name.clone());
                    self.swatches.push(swatch);
                }
            }
        }
        if !merge.added.is_empty() || !merge.renamed.is_empty() {
            self.touch();
        }
        merge
    }

    /// The swatches as another application can read them: each its own
    /// colour, a tint worked out to the colour it makes, and a spot as its
    /// ink's process stand-in marked as a spot. A swatch naming one that
    /// is not there has no colour to give, and is left out.
    pub fn swatches_for_exchange(&self) -> Vec<Swatch> {
        self.swatches
            .iter()
            .filter_map(|swatch| {
                let resolved = self.resolve_colour(&Color::Swatch {
                    name: swatch.name.clone(),
                    tint: 1.0,
                });
                match resolved {
                    Color::Swatch { .. } => None,
                    // The ink itself is a spot; a tint of it is a mix.
                    Color::Spot { tint, fallback, .. } => Some(Swatch {
                        name: swatch.name.clone(),
                        colour: fallback.tinted(tint),
                        spot: tint >= 1.0,
                    }),
                    colour => Some(Swatch::new(swatch.name.clone(), colour)),
                }
            })
            .collect()
    }
}

/// Names compared as a person reads them: letters without regard to case,
/// and a run of digits as the number it spells — the order
/// [`Document::sort_swatches`] puts swatches in.
pub fn name_order(a: &str, b: &str) -> std::cmp::Ordering {
    fn pieces(s: &str) -> Vec<(bool, String)> {
        let mut out: Vec<(bool, String)> = Vec::new();
        for ch in s.chars() {
            let digit = ch.is_ascii_digit();
            match out.last_mut() {
                Some((d, piece)) if *d == digit => piece.push(ch),
                _ => out.push((digit, ch.to_string())),
            }
        }
        out
    }
    let (pa, pb) = (pieces(a), pieces(b));
    for ((da, sa), (db, sb)) in pa.iter().zip(&pb) {
        let order = if *da && *db {
            let (ta, tb) = (sa.trim_start_matches('0'), sb.trim_start_matches('0'));
            ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb))
        } else {
            sa.to_lowercase().cmp(&sb.to_lowercase())
        };
        if order.is_ne() {
            return order;
        }
    }
    pa.len().cmp(&pb.len()).then_with(|| a.cmp(b))
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

    fn cmyk(c: f32, m: f32, y: f32, k: f32) -> Color {
        Color::Cmyk { c, m, y, k, a: 1.0 }
    }

    fn names(doc: &Document) -> Vec<&str> {
        doc.swatches.iter().map(|s| s.name.as_str()).collect()
    }

    /// A rectangle on the first layer, filled with `fill`.
    fn rectangle(doc: &mut Document, fill: Paint) -> crate::ids::FrameId {
        let layer = doc.default_layer().expect("a layer");
        doc.add_frame(
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
                fill,
                stroke: None,
                wrap: crate::nodes::TextWrap::None,
                blend: crate::blending::Blending::PLAIN,
                corners: crate::corners::Corners::SQUARE,
                shadow: None,
                feather: None,
                anchor: None,
                style: None,
                hidden: false,
                locked: false,
                overprint: Default::default(),
            },
        )
    }

    /// A document with a literal red on a rectangle's fill and on some text,
    /// a literal green in a gradient, black (a built-in) on a stroke, and a
    /// swatch whose own colour is a literal nobody uses.
    fn unnamed_document() -> (Document, crate::ids::FrameId, crate::ids::StoryId) {
        let mut doc = Document::new();
        let red = cmyk(0.0, 0.91, 0.76, 0.0);
        let id = rectangle(&mut doc, Paint::Solid(red.clone()));
        doc.frames[id].stroke = Some(crate::nodes::Stroke::new(cmyk(0.0, 0.0, 0.0, 1.0), 1.0));
        rectangle(
            &mut doc,
            Paint::Gradient(crate::paint::Gradient::new(
                crate::paint::Ramp::Linear { angle: 0.0 },
                vec![
                    crate::paint::Stop {
                        at: 0.0,
                        colour: Color::Rgb {
                            r: 0.2,
                            g: 0.6,
                            b: 0.1,
                            a: 1.0,
                        },
                    },
                    crate::paint::Stop {
                        at: 1.0,
                        colour: Color::Rgb {
                            r: 1.0,
                            g: 1.0,
                            b: 1.0,
                            a: 0.0,
                        },
                    },
                ],
            )),
        );
        let mut story = Story::new("Spring");
        story.runs[0].local.colour = Some(red);
        let story = doc.add_story(story);
        doc.set_swatch(Swatch::new("Defined", cmyk(0.5, 0.0, 0.0, 0.0)));
        (doc, id, story)
    }

    const BUILT_IN: [Color; 1] = [Color::Cmyk {
        c: 0.0,
        m: 0.0,
        y: 0.0,
        k: 1.0,
        a: 1.0,
    }];

    #[test]
    fn the_unnamed_colours_are_the_ones_used_as_themselves() {
        let (doc, ..) = unnamed_document();
        let found = doc.unnamed_colours(&BUILT_IN);
        assert_eq!(
            found,
            vec![
                cmyk(0.0, 0.91, 0.76, 0.0),
                Color::Rgb {
                    r: 0.2,
                    g: 0.6,
                    b: 0.1,
                    a: 1.0
                },
            ],
            "the red once though it is used twice; the black left as built \
             in; a swatch's own colour and a transparent stop not counted"
        );
    }

    #[test]
    fn naming_the_unnamed_colours_points_every_use_at_its_swatch() {
        let (mut doc, id, story) = unnamed_document();
        let made = doc.name_unnamed_colours(&BUILT_IN);
        assert_eq!(made, ["C=0 M=91 Y=76 K=0", "R=51 G=153 B=26"]);
        assert_eq!(
            names(&doc),
            ["Defined", "C=0 M=91 Y=76 K=0", "R=51 G=153 B=26"]
        );
        let named = |name: &str| Color::Swatch {
            name: name.into(),
            tint: 1.0,
        };
        assert_eq!(
            doc.frames[id].fill,
            Paint::Solid(named("C=0 M=91 Y=76 K=0"))
        );
        assert_eq!(
            doc.stories[story].runs[0].local.colour,
            Some(named("C=0 M=91 Y=76 K=0"))
        );
        assert_eq!(
            doc.swatch("C=0 M=91 Y=76 K=0").unwrap().colour,
            cmyk(0.0, 0.91, 0.76, 0.0),
            "the swatch holds the colour, not a reference to itself"
        );
        assert_eq!(doc.frames[id].stroke.as_ref().unwrap().color, BUILT_IN[0]);
        assert!(doc.unnamed_colours(&BUILT_IN).is_empty());
        assert!(doc.name_unnamed_colours(&BUILT_IN).is_empty());
    }

    #[test]
    fn two_colours_that_read_alike_are_given_two_names() {
        let mut doc = Document::new();
        doc.set_swatch(Swatch::new("C=0 M=50 Y=0 K=0", Color::BLACK));
        for m in [0.5, 0.501] {
            rectangle(&mut doc, Paint::Solid(cmyk(0.0, m, 0.0, 0.0)));
        }
        assert_eq!(
            doc.name_unnamed_colours(&[]),
            ["C=0 M=50 Y=0 K=0 2", "C=0 M=50 Y=0 K=0 3"]
        );
    }

    #[test]
    fn a_swatch_moves_before_another_or_to_the_end() {
        let mut doc = Document::new();
        for name in ["A", "B", "C", "D"] {
            doc.set_swatch(Swatch::new(name, Color::BLACK));
        }
        assert!(doc.move_swatch("D", Some("B")));
        assert_eq!(names(&doc), ["A", "D", "B", "C"]);
        assert!(doc.move_swatch("A", Some("C")));
        assert_eq!(names(&doc), ["D", "B", "A", "C"]);
        assert!(doc.move_swatch("D", None));
        assert_eq!(names(&doc), ["B", "A", "C", "D"]);
        let revision = doc.revision();
        // Where it already is, onto itself, and from or to nowhere.
        assert!(!doc.move_swatch("A", Some("C")));
        assert!(!doc.move_swatch("A", Some("A")));
        assert!(!doc.move_swatch("D", None));
        assert!(!doc.move_swatch("Z", None));
        assert!(!doc.move_swatch("A", Some("Z")));
        assert_eq!(names(&doc), ["B", "A", "C", "D"]);
        assert_eq!(doc.revision(), revision);
    }

    #[test]
    fn swatches_sort_as_a_person_reads_their_names() {
        let mut doc = Document::new();
        for name in ["blue 10", "Red", "Blue 2", "apricot", "Blue 1"] {
            doc.set_swatch(Swatch::new(name, Color::BLACK));
        }
        assert!(doc.sort_swatches());
        assert_eq!(
            names(&doc),
            ["apricot", "Blue 1", "Blue 2", "blue 10", "Red"]
        );
        let revision = doc.revision();
        assert!(!doc.sort_swatches(), "sorted already");
        assert_eq!(doc.revision(), revision);
    }

    #[test]
    fn swatches_brought_in_keep_what_is_here_and_rename_what_clashes() {
        let mut doc = Document::new();
        doc.set_swatch(Swatch::new("Brand red", cmyk(0.0, 0.91, 0.76, 0.0)));
        doc.set_swatch(Swatch::new("Sky", cmyk(0.6, 0.0, 0.0, 0.0)));
        let merge = doc.add_swatches(vec![
            // The same in every way: already here.
            Swatch::new("Brand red", cmyk(0.0, 0.91, 0.76, 0.0)),
            // The same name for another colour: renamed.
            Swatch::new("Sky", cmyk(0.9, 0.1, 0.0, 0.0)),
            // A tint of the one renamed follows it.
            Swatch::new(
                "Sky 40%",
                Color::Swatch {
                    name: "Sky".into(),
                    tint: 0.4,
                },
            ),
            Swatch::new("Leaf", cmyk(0.6, 0.0, 1.0, 0.0)),
            Swatch::new("  ", Color::BLACK),
        ]);
        assert_eq!(merge.already, 1);
        assert_eq!(merge.renamed, [("Sky".to_string(), "Sky 2".to_string())]);
        assert_eq!(merge.added, ["Sky 40%", "Leaf"]);
        assert_eq!(
            names(&doc),
            ["Brand red", "Sky", "Sky 2", "Sky 40%", "Leaf"]
        );
        assert_eq!(doc.swatch("Sky").unwrap().colour, cmyk(0.6, 0.0, 0.0, 0.0));
        assert_eq!(
            doc.swatch("Sky 40%").unwrap().colour,
            Color::Swatch {
                name: "Sky 2".into(),
                tint: 0.4
            }
        );
        let revision = doc.revision();
        let again = doc.add_swatches(vec![
            Swatch::new("Leaf", cmyk(0.6, 0.0, 1.0, 0.0)),
            // The clash brought in before, and its tint: found under the
            // name they were given then, not brought a second time.
            Swatch::new("Sky", cmyk(0.9, 0.1, 0.0, 0.0)),
            Swatch::new(
                "Sky 40%",
                Color::Swatch {
                    name: "Sky".into(),
                    tint: 0.4,
                },
            ),
        ]);
        assert_eq!(again.already, 3);
        assert!(again.added.is_empty() && again.renamed.is_empty());
        assert_eq!(doc.revision(), revision, "nothing new, nothing changed");
    }

    #[test]
    fn a_swatch_leaves_as_the_colour_it_makes() {
        let mut doc = Document::new();
        doc.set_swatch(Swatch::new("Red", cmyk(0.0, 1.0, 1.0, 0.0)));
        doc.set_swatch(Swatch::new(
            "Red 40%",
            Color::Swatch {
                name: "Red".into(),
                tint: 0.4,
            },
        ));
        doc.set_swatch(Swatch {
            name: "Ink".into(),
            colour: cmyk(1.0, 0.5, 0.0, 0.0),
            spot: true,
        });
        doc.set_swatch(Swatch::new(
            "Ink 50%",
            Color::Swatch {
                name: "Ink".into(),
                tint: 0.5,
            },
        ));
        doc.set_swatch(Swatch::new(
            "Orphan",
            Color::Swatch {
                name: "Gone".into(),
                tint: 1.0,
            },
        ));
        let out = doc.swatches_for_exchange();
        let by = |name: &str| out.iter().find(|s| s.name == name).cloned();
        assert_eq!(by("Red").unwrap().colour, cmyk(0.0, 1.0, 1.0, 0.0));
        assert_eq!(by("Red 40%").unwrap().colour, cmyk(0.0, 0.4, 0.4, 0.0));
        assert!(by("Ink").unwrap().spot, "the ink is a spot");
        assert_eq!(by("Ink").unwrap().colour, cmyk(1.0, 0.5, 0.0, 0.0));
        let half = by("Ink 50%").unwrap();
        assert!(!half.spot, "a tint of it is a mix");
        assert_eq!(half.colour, cmyk(0.5, 0.25, 0.0, 0.0));
        assert!(by("Orphan").is_none());
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
                feather: None,
                anchor: None,
                style: None,
                hidden: false,
                locked: false,
                overprint: Default::default(),
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
                feather: None,
                anchor: None,
                style: None,
                hidden: false,
                locked: false,
                overprint: Default::default(),
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
    fn a_name_nobody_defines_is_handed_to_a_swatch_that_exists() {
        let mut doc = Document::new();
        doc.set_swatch(Swatch::new("Ink", Color::WHITE));
        let frame = frame_filled(&mut doc, reference("Gone"));
        let mut story = Story::new("Text");
        story.runs[0].local.colour = Some(reference("Gone"));
        let story = doc.add_story(story);
        let names = doc.swatch_names_used();
        assert!(names.contains(&"Gone".to_string()), "{names:?}");

        assert!(doc.repoint_swatch("Gone", "Ink"));
        assert_eq!(
            doc.frames[frame].fill,
            Paint::Solid(reference("Ink")),
            "tint kept"
        );
        assert_eq!(
            doc.stories[story].runs[0].local.colour,
            Some(reference("Ink"))
        );
        assert!(!doc.swatch_names_used().contains(&"Gone".to_string()));
        assert!(doc.swatch("Ink").is_some(), "the swatch handed to stays");
    }

    #[test]
    fn a_defined_swatch_or_one_handed_to_nothing_is_not_repointed() {
        let mut doc = Document::new();
        doc.set_swatch(Swatch::new("Brand", Color::BLACK));
        doc.set_swatch(Swatch::new("Ink", Color::WHITE));
        frame_filled(&mut doc, reference("Brand"));
        frame_filled(&mut doc, reference("Gone"));
        let revision = doc.revision();
        assert!(
            !doc.repoint_swatch("Brand", "Ink"),
            "Brand is defined: handing it on is replace_swatch's"
        );
        assert!(
            !doc.repoint_swatch("Gone", "Also gone"),
            "nothing to hand to"
        );
        assert_eq!(doc.revision(), revision);
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
