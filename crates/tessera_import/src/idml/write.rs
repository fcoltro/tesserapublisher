//! IDML written: a Tessera document as a package InDesign opens.
//!
//! The same package the importer reads, written the other way: the spine
//! (`designmap.xml`) naming the resources, the parent spreads, the spreads
//! and the stories; colours and gradients in `Resources/Graphic.xml`; styles
//! in `Resources/Styles.xml`; the page size in `Resources/Preferences.xml`.
//!
//! ## Where things are
//!
//! Each spread's space has its origin at the centre of its pages, as
//! InDesign's does, and each page carries the translation into it. An item's
//! path points are in its own space, from its top-left corner, and its
//! `ItemTransform` carries it — turn and all — into the spread's: what IDML
//! means by an item, and what the importer reads back.
//!
//! ## What is not written
//!
//! The same rule as the importer's: **what cannot be carried is said.**
//! Tables, text on a path, objects anchored in text, rounded corners, gradient
//! feathers and hyperlinks are listed in [`Written::dropped`] rather than left
//! out quietly.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Write as _;

use tessera_color::Color;
use tessera_document::document::Document;
use tessera_document::ids::{FrameId, LayerId, PageId, SpreadId, StoryId};
use tessera_document::nodes::{Frame, FrameKind, TextWrap, VerticalJustify, WrapTo};
use tessera_document::paint::{Paint, Ramp};
use tessera_geometry::{DocPoint, DocRect};
use tessera_text::story::{
    Alignment, Case, CharacterFormat, CharacterStyleId, ColumnSpan, KeepTogether, Kerning,
    ListKind, ParagraphFormat, ParagraphStyleId, Story, TabAlignment,
};
use tessera_text::variables::Marker;

use crate::Dropped;

/// A package, and what it could not carry.
#[derive(Debug)]
pub struct Written {
    pub bytes: Vec<u8>,
    pub dropped: Dropped,
}

const DOM: &str = "8.0";
const PKG: &str = "http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging";
const HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";
const NO_CHARACTER_STYLE: &str = "CharacterStyle/$ID/[No character style]";
const NORMAL_PARAGRAPH_STYLE: &str = "ParagraphStyle/$ID/NormalParagraphStyle";

/// Text for an attribute or an element, escaped.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' => out.push('\t'),
            // XML 1.0 has no place for the other control characters.
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out
}

/// A number as IDML writes one: no more decimals than it needs.
fn n(v: f64) -> String {
    let v = if v.abs() < 1e-9 { 0.0 } else { v };
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".to_owned()
    } else {
        s.to_owned()
    }
}

/// `file:` URI for a path, as InDesign writes a link.
fn uri(path: &std::path::Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut encoded = String::new();
    for c in text.chars() {
        match c {
            ' ' => encoded.push_str("%20"),
            '%' => encoded.push_str("%25"),
            '#' => encoded.push_str("%23"),
            '?' => encoded.push_str("%3F"),
            c => encoded.push(c),
        }
    }
    if encoded.len() > 1 && encoded.as_bytes()[1] == b':' {
        format!("file:/{encoded}")
    } else {
        format!("file://{encoded}")
    }
}

/// A path point: its left handle, its anchor and its right handle.
type PathPoint = [(f64, f64); 3];

/// A subpath's points, and whether it is open.
type Subpath = (Vec<PathPoint>, bool);

/// Every colour and gradient the package names, written once each.
#[derive(Default)]
struct Palette {
    /// `Self` of each colour written, by its canonical values.
    unnamed: HashMap<String, String>,
    /// The `<Color>` and `<Gradient>` elements, in the order made.
    elements: String,
    count: usize,
    alpha_noted: bool,
}

impl Palette {
    fn next(&mut self, prefix: &str) -> String {
        self.count += 1;
        format!("{prefix}/u{:x}", 0x100 + self.count)
    }

    /// `Space` and `ColorValue` for a concrete colour.
    fn space(colour: &Color) -> (&'static str, String) {
        match colour {
            Color::Cmyk { c, m, y, k, .. } => (
                "CMYK",
                format!(
                    "{} {} {} {}",
                    n(f64::from(*c) * 100.0),
                    n(f64::from(*m) * 100.0),
                    n(f64::from(*y) * 100.0),
                    n(f64::from(*k) * 100.0)
                ),
            ),
            Color::Lab { l, a, b, .. } => (
                "LAB",
                format!(
                    "{} {} {}",
                    n(f64::from(*l)),
                    n(f64::from(*a)),
                    n(f64::from(*b))
                ),
            ),
            Color::Spot { fallback, .. } => Self::space(fallback),
            other => {
                let [r, g, b, _] = other.to_rgb_f32();
                (
                    "RGB",
                    format!(
                        "{} {} {}",
                        n(f64::from(r) * 255.0),
                        n(f64::from(g) * 255.0),
                        n(f64::from(b) * 255.0)
                    ),
                )
            }
        }
    }

    fn colour_element(self_: &str, name: &str, model: &str, colour: &Color) -> String {
        let (space, values) = Self::space(colour);
        format!(
            "  <Color Self=\"{}\" Model=\"{model}\" Space=\"{space}\" ColorValue=\"{values}\" \
             ColorOverride=\"Normal\" AlternateSpace=\"NoAlternateColor\" AlternateColorValue=\"\" \
             Name=\"{}\" ColorEditable=\"true\" ColorRemovable=\"true\" Visible=\"true\" \
             SwatchCreatorID=\"7937\"/>\n",
            esc(self_),
            esc(name)
        )
    }

    /// A named swatch.
    fn swatch(&mut self, name: &str, colour: &Color, spot: bool) {
        let model = if spot || matches!(colour, Color::Spot { .. }) {
            "Spot"
        } else {
            "Process"
        };
        let element = Self::colour_element(&format!("Color/{name}"), name, model, colour);
        self.elements.push_str(&element);
    }

    /// What a colour reference is: its `Self` and a tint, `None` for none.
    fn colour(
        &mut self,
        doc: &Document,
        colour: &Color,
        dropped: &mut Dropped,
    ) -> Option<(String, Option<f64>)> {
        match colour {
            Color::Swatch { name, tint } if doc.swatch(name).is_some() => {
                let tint = (*tint < 1.0).then(|| f64::from(*tint) * 100.0);
                Some((format!("Color/{name}"), tint))
            }
            Color::Spot { name, tint, .. } => {
                if doc.swatch(name).is_none() && !self.unnamed.contains_key(&format!("spot:{name}"))
                {
                    let element =
                        Self::colour_element(&format!("Color/{name}"), name, "Spot", colour);
                    self.elements.push_str(&element);
                    self.unnamed
                        .insert(format!("spot:{name}"), format!("Color/{name}"));
                }
                let tint = (*tint < 1.0).then(|| f64::from(*tint) * 100.0);
                Some((format!("Color/{name}"), tint))
            }
            Color::Swatch { .. } => self.colour(doc, &doc.resolve_colour(colour), dropped),
            concrete => {
                let alpha = concrete.to_rgb_f32()[3];
                if alpha <= 0.0 {
                    return None;
                }
                if alpha < 1.0 && !self.alpha_noted {
                    dropped.note("see-through colours were written opaque");
                    self.alpha_noted = true;
                }
                if *concrete == Color::BLACK_INK {
                    return Some(("Color/Black".to_owned(), None));
                }
                let (space, values) = Self::space(concrete);
                let key = format!("{space}:{values}");
                if let Some(found) = self.unnamed.get(&key) {
                    return Some((found.clone(), None));
                }
                let self_ = self.next("Color");
                // `$ID/` is InDesign's mark of a colour with no swatch.
                let element = Self::colour_element(&self_, "$ID/", "Process", concrete);
                self.elements.push_str(&element);
                self.unnamed.insert(key, self_.clone());
                Some((self_, None))
            }
        }
    }

    /// Attributes for a fill or a stroke: `FillColor`, its tint and, for a
    /// gradient, its angle.
    fn paint(
        &mut self,
        doc: &Document,
        paint: &Paint,
        which: &str,
        dropped: &mut Dropped,
    ) -> String {
        match paint {
            Paint::Solid(colour) => match self.colour(doc, colour, dropped) {
                Some((self_, tint)) => {
                    let mut out = format!(" {which}Color=\"{}\"", esc(&self_));
                    if let Some(t) = tint {
                        let _ = write!(out, " {which}Tint=\"{}\"", n(t));
                    }
                    out
                }
                None => format!(" {which}Color=\"Swatch/None\""),
            },
            Paint::Gradient(gradient) => {
                let self_ = self.next("Gradient");
                let mut stops = String::new();
                for (i, stop) in gradient.stops().iter().enumerate() {
                    let colour = doc.resolve_colour(&stop.colour);
                    let reference = self
                        .colour(doc, &colour, dropped)
                        .map(|(s, _)| s)
                        .unwrap_or_else(|| "Color/Paper".to_owned());
                    let _ = writeln!(
                        stops,
                        "    <GradientStop Self=\"{}Stop{i}\" StopColor=\"{}\" Location=\"{}\" Midpoint=\"50\"/>",
                        esc(&self_),
                        esc(&reference),
                        n(f64::from(stop.at) * 100.0)
                    );
                }
                let (kind, angle) = match gradient.ramp {
                    Ramp::Radial => ("Radial", None),
                    Ramp::Linear { angle } => ("Linear", Some(-angle)),
                };
                let _ = write!(
                    self.elements,
                    "  <Gradient Self=\"{}\" Type=\"{kind}\" Name=\"$ID/\" ColorEditable=\"true\" \
                     ColorRemovable=\"true\" Visible=\"true\" SwatchCreatorID=\"7937\">\n{stops}  </Gradient>\n",
                    esc(&self_)
                );
                let mut out = format!(" {which}Color=\"{}\"", esc(&self_));
                if let Some(angle) = angle {
                    let _ = write!(out, " Gradient{which}Angle=\"{}\"", n(angle));
                }
                out
            }
        }
    }
}

/// The style names a package gives, by id.
#[derive(Default)]
struct StyleNames {
    paragraph: HashMap<ParagraphStyleId, String>,
    character: HashMap<CharacterStyleId, String>,
    object: HashMap<tessera_document::ids::ObjectStyleId, String>,
}

/// A name unique among `taken`, with a number after it if it has to.
fn unique(name: &str, taken: &mut Vec<String>) -> String {
    let base = if name.trim().is_empty() {
        "Style".to_owned()
    } else {
        name.replace("$ID/", "")
    };
    let mut candidate = base.clone();
    let mut n = 2;
    while taken.contains(&candidate) {
        candidate = format!("{base} {n}");
        n += 1;
    }
    taken.push(candidate.clone());
    candidate
}

/// InDesign's face name for a weight and slant.
fn font_style(weight: u16, italic: bool) -> String {
    let base = match weight {
        0..=150 => "Thin",
        151..=250 => "ExtraLight",
        251..=350 => "Light",
        351..=450 => "Regular",
        451..=550 => "Medium",
        551..=650 => "Semibold",
        651..=750 => "Bold",
        751..=850 => "ExtraBold",
        _ => "Black",
    };
    match (base, italic) {
        ("Regular", true) => "Italic".to_owned(),
        (base, true) => format!("{base} Italic"),
        (base, false) => base.to_owned(),
    }
}

fn language_name(code: &str) -> Option<&'static str> {
    Some(match code.split(['-', '_']).next()? {
        "en" => "$ID/English: USA",
        "de" => "$ID/German: Reformed",
        "fr" => "$ID/French",
        "es" => "$ID/Spanish",
        "it" => "$ID/Italian",
        "pt" => "$ID/Portuguese",
        "nl" => "$ID/Dutch",
        "sv" => "$ID/Swedish",
        "da" => "$ID/Danish",
        "nb" | "no" => "$ID/Norwegian: Bokmal",
        "fi" => "$ID/Finnish",
        "pl" => "$ID/Polish",
        "cs" => "$ID/Czech",
        "hu" => "$ID/Hungarian",
        "ru" => "$ID/Russian",
        "tr" => "$ID/Turkish",
        "ca" => "$ID/Catalan",
        "el" => "$ID/Greek",
        _ => return None,
    })
}

struct Writer<'d> {
    doc: &'d Document,
    palette: Palette,
    names: StyleNames,
    dropped: Dropped,
    /// IDML `Self` for every frame and story written.
    frames: HashMap<FrameId, String>,
    stories: HashMap<StoryId, String>,
    layers: HashMap<LayerId, String>,
    pages: HashMap<PageId, String>,
    /// Frame before each threaded frame.
    previous: HashMap<FrameId, FrameId>,
    fonts: Vec<(String, Vec<String>)>,
    count: usize,
    masters_self: HashMap<tessera_document::ids::MasterId, String>,
}

impl<'d> Writer<'d> {
    fn id(&mut self) -> String {
        self.count += 1;
        format!("u{:x}", 0x1000 + self.count)
    }

    fn note_font(&mut self, family: &str, style: &str) {
        match self.fonts.iter_mut().find(|(f, _)| f == family) {
            Some((_, styles)) => {
                if !styles.iter().any(|s| s == style) {
                    styles.push(style.to_owned());
                }
            }
            None => self.fonts.push((family.to_owned(), vec![style.to_owned()])),
        }
    }

    /// Character attributes and properties. `size` is the size the text is
    /// set at, which a leading — a multiple here, points in IDML — needs.
    fn character(&mut self, f: &CharacterFormat, size: f32) -> (String, String) {
        let mut attrs = String::new();
        let mut props = String::new();
        if let Some(family) = &f.family {
            let _ = write!(
                props,
                "<AppliedFont type=\"string\">{}</AppliedFont>",
                esc(family)
            );
        }
        if f.weight.is_some() || f.italic.is_some() {
            let style = font_style(f.weight.unwrap_or(400), f.italic.unwrap_or(false));
            if let Some(family) = &f.family {
                let family = family.clone();
                self.note_font(&family, &style);
            }
            let _ = write!(attrs, " FontStyle=\"{}\"", esc(&style));
        }
        if let Some(s) = f.size {
            let _ = write!(attrs, " PointSize=\"{}\"", n(f64::from(s)));
        }
        if let Some(lh) = f.line_height {
            // Absolute in IDML: the multiple of the size it is set at, and
            // the size said beside it, so it reads back as the same multiple.
            if f.size.is_none() {
                let _ = write!(attrs, " PointSize=\"{}\"", n(f64::from(size)));
            }
            let _ = write!(
                props,
                "<Leading type=\"unit\">{}</Leading>",
                n(f64::from(lh * f.size.unwrap_or(size)))
            );
        }
        if let Some(t) = f.tracking {
            let _ = write!(attrs, " Tracking=\"{}\"", n(f64::from(t)));
        }
        if let Some(b) = f.baseline_shift {
            let _ = write!(attrs, " BaselineShift=\"{}\"", n(f64::from(b)));
        }
        if let Some(case) = f.case {
            let said = match case {
                Case::Upper => "AllCaps",
                Case::SmallCaps => "SmallCaps",
                _ => "Normal",
            };
            let _ = write!(attrs, " Capitalization=\"{said}\"");
        }
        if let Some(u) = &f.underline {
            let _ = write!(attrs, " Underline=\"{}\"", u.on);
        }
        if let Some(s) = &f.strikethrough {
            let _ = write!(attrs, " StrikeThru=\"{}\"", s.on);
        }
        if let Some(colour) = &f.colour {
            attrs.push_str(&self.palette.paint(
                self.doc,
                &Paint::Solid(colour.clone()),
                "Fill",
                &mut self.dropped,
            ));
        }
        if let Some(l) = f.ligatures {
            let _ = write!(attrs, " Ligatures=\"{l}\"");
        }
        if let Some(k) = f.kerning {
            let said = match k {
                Kerning::Optical => "Optical",
                _ => "Metrics",
            };
            let _ = write!(attrs, " KerningMethod=\"{said}\"");
        }
        if let Some(name) = f.language.as_deref().and_then(language_name) {
            let _ = write!(attrs, " AppliedLanguage=\"{}\"", esc(name));
        }
        if f.link.is_some() {
            self.dropped.note("hyperlinks (their words were kept)");
        }
        (attrs, props)
    }

    /// Paragraph attributes and properties, the character part included.
    fn paragraph(&mut self, f: &ParagraphFormat, size: f32) -> (String, String) {
        let mut attrs = String::new();
        let mut props = String::new();
        if let Some(a) = f.alignment {
            let said = match a {
                Alignment::Left => "LeftAlign",
                Alignment::Centre => "CenterAlign",
                Alignment::Right => "RightAlign",
                Alignment::Justify => "LeftJustified",
            };
            let _ = write!(attrs, " Justification=\"{said}\"");
        }
        for (value, name) in [
            (f.indent_left, "LeftIndent"),
            (f.indent_right, "RightIndent"),
            (f.indent_first, "FirstLineIndent"),
            (f.space_before, "SpaceBefore"),
            (f.space_after, "SpaceAfter"),
        ] {
            if let Some(v) = value {
                let _ = write!(attrs, " {name}=\"{}\"", n(f64::from(v)));
            }
        }
        if let Some(h) = f.hyphenate {
            let _ = write!(attrs, " Hyphenation=\"{h}\"");
        }
        if let Some(l) = f.drop_cap_lines {
            let _ = write!(attrs, " DropCapLines=\"{l}\"");
        }
        if let Some(c) = f.drop_cap_characters {
            let _ = write!(attrs, " DropCapCharacters=\"{c}\"");
        }
        if let Some(keep) = f.keep {
            let _ = write!(
                attrs,
                " KeepWithNext=\"{}\"",
                if keep.with_next { 1 } else { 0 }
            );
            match keep.together {
                KeepTogether::Off => attrs.push_str(" KeepLinesTogether=\"false\""),
                KeepTogether::All => {
                    attrs.push_str(" KeepLinesTogether=\"true\" KeepAllLinesTogether=\"true\"")
                }
                KeepTogether::Ends { start, end } => {
                    let _ = write!(
                        attrs,
                        " KeepLinesTogether=\"true\" KeepAllLinesTogether=\"false\" \
                         KeepFirstLines=\"{start}\" KeepLastLines=\"{end}\""
                    );
                }
            }
        }
        if let Some(list) = &f.list {
            let said = match list.kind {
                ListKind::Bullet => "BulletList",
                ListKind::Number => "NumberedList",
                _ => "NoList",
            };
            let _ = write!(attrs, " BulletsAndNumberingListType=\"{said}\"");
        }
        if let Some(span) = f.column_span {
            match span {
                ColumnSpan::Single => attrs.push_str(" SpanColumnType=\"SingleColumn\""),
                ColumnSpan::Span {
                    columns,
                    space_before,
                    space_after,
                } => {
                    let count = if columns == 0 {
                        "All".to_owned()
                    } else {
                        columns.to_string()
                    };
                    let _ = write!(
                        attrs,
                        " SpanColumnType=\"SpanColumns\" SpanSplitColumnCount=\"{count}\" \
                         SpanColumnMinSpaceBefore=\"{}\" SpanColumnMinSpaceAfter=\"{}\"",
                        n(f64::from(space_before)),
                        n(f64::from(space_after))
                    );
                }
                ColumnSpan::Split {
                    columns,
                    gutter,
                    space_before,
                    space_after,
                } => {
                    let _ = write!(
                        attrs,
                        " SpanColumnType=\"SplitColumns\" SpanSplitColumnCount=\"{columns}\" \
                         SplitColumnInsideGutter=\"{}\" SpanColumnMinSpaceBefore=\"{}\" \
                         SpanColumnMinSpaceAfter=\"{}\"",
                        n(f64::from(gutter)),
                        n(f64::from(space_before)),
                        n(f64::from(space_after))
                    );
                }
            }
        }
        if let Some(stops) = &f.tab_stops {
            props.push_str("<TabList type=\"list\">");
            for stop in stops {
                let alignment = match stop.alignment {
                    TabAlignment::Centre => "CenterAlign",
                    TabAlignment::Right => "RightAlign",
                    TabAlignment::Decimal => "CharacterAlign",
                    _ => "LeftAlign",
                };
                let _ = write!(
                    props,
                    "<ListItem type=\"record\"><Alignment type=\"enumeration\">{alignment}</Alignment>\
                     <Position type=\"unit\">{}</Position>",
                    n(f64::from(stop.position))
                );
                if let Some(leader) = stop.leader {
                    let _ = write!(
                        props,
                        "<Leader type=\"string\">{}</Leader>",
                        esc(&leader.to_string())
                    );
                }
                props.push_str("</ListItem>");
            }
            props.push_str("</TabList>");
        }
        if f.rule_above.is_some() || f.rule_below.is_some() {
            self.dropped.note("paragraph rules");
        }
        let (a, p) = self.character(&f.character, size);
        attrs.push_str(&a);
        props.push_str(&p);
        (attrs, props)
    }

    /// A paragraph style's nested, line and GREP styles, as properties.
    fn automatic(&self, f: &ParagraphFormat) -> String {
        use tessera_text::automatic::Delimiter;
        let mut out = String::new();
        let style_ref = |id: Option<CharacterStyleId>| -> String {
            id.and_then(|id| self.names.character.get(&id))
                .cloned()
                .unwrap_or_else(|| NO_CHARACTER_STYLE.to_owned())
        };
        if let Some(rules) = &f.nested {
            out.push_str("<AllNestedStyles type=\"list\">");
            for rule in rules {
                let delimiter = match &rule.delimiter {
                    Delimiter::Sentences => "Sentence".to_owned(),
                    Delimiter::Words => "AnyWord".to_owned(),
                    Delimiter::Characters => "AnyCharacter".to_owned(),
                    Delimiter::Letters => "Letters".to_owned(),
                    Delimiter::Digits => "Digits".to_owned(),
                    Delimiter::Tabs => "Tabs".to_owned(),
                    Delimiter::AnyOf(chars) => chars.clone(),
                };
                let kind = if matches!(rule.delimiter, Delimiter::AnyOf(_)) {
                    "string"
                } else {
                    "enumeration"
                };
                let _ = write!(
                    out,
                    "<ListItem type=\"record\"><AppliedCharacterStyle type=\"object\">{}</AppliedCharacterStyle>\
                     <Delimiter type=\"{kind}\">{}</Delimiter><Repetition type=\"long\">{}</Repetition>\
                     <Inclusive type=\"boolean\">{}</Inclusive></ListItem>",
                    esc(&style_ref(rule.style)),
                    esc(&delimiter),
                    rule.count,
                    rule.through
                );
            }
            out.push_str("</AllNestedStyles>");
        }
        if let Some(rules) = &f.line_styles {
            out.push_str("<AllLineStyles type=\"list\">");
            for rule in rules {
                let _ = write!(
                    out,
                    "<ListItem type=\"record\"><AppliedCharacterStyle type=\"object\">{}</AppliedCharacterStyle>\
                     <LineCount type=\"long\">{}</LineCount></ListItem>",
                    esc(&style_ref(rule.style)),
                    rule.lines
                );
            }
            out.push_str("</AllLineStyles>");
        }
        if let Some(rules) = &f.grep {
            out.push_str("<AllGREPStyles type=\"list\">");
            for rule in rules {
                let _ = write!(
                    out,
                    "<ListItem type=\"record\"><AppliedCharacterStyle type=\"object\">{}</AppliedCharacterStyle>\
                     <GrepExpression type=\"string\">{}</GrepExpression></ListItem>",
                    esc(&style_ref(Some(rule.style))),
                    esc(&rule.pattern)
                );
            }
            out.push_str("</AllGREPStyles>");
        }
        out
    }

    fn element(tag: &str, attrs: &str, props: &str, inner: &str) -> String {
        if props.is_empty() && inner.is_empty() {
            format!("<{tag}{attrs}/>")
        } else if props.is_empty() {
            format!("<{tag}{attrs}>{inner}</{tag}>")
        } else {
            format!("<{tag}{attrs}><Properties>{props}</Properties>{inner}</{tag}>")
        }
    }

    fn styles(&mut self) -> String {
        let doc = self.doc;
        // Names first, so a style based on one written after it can say so.
        let mut taken = Vec::new();
        for (id, style) in doc.character_styles.iter() {
            let name = unique(&style.name, &mut taken);
            self.names
                .character
                .insert(id, format!("CharacterStyle/{name}"));
        }
        let mut taken = Vec::new();
        for (id, style) in doc.paragraph_styles.iter() {
            let name = unique(&style.name, &mut taken);
            self.names
                .paragraph
                .insert(id, format!("ParagraphStyle/{name}"));
        }
        let mut taken = Vec::new();
        for (id, style) in doc.object_styles.iter() {
            let name = unique(&style.name, &mut taken);
            self.names.object.insert(id, format!("ObjectStyle/{name}"));
        }
        let size = doc.text_default.size;

        let mut out = format!("{HEAD}<idPkg:Styles xmlns:idPkg=\"{PKG}\" DOMVersion=\"{DOM}\">\n");
        out.push_str("<RootCharacterStyleGroup Self=\"u79\">\n");
        let _ = writeln!(
            out,
            "  <CharacterStyle Self=\"{NO_CHARACTER_STYLE}\" Imported=\"false\" Name=\"$ID/[No character style]\"/>"
        );
        for (id, style) in doc.character_styles.iter() {
            let self_ = self.names.character[&id].clone();
            let (attrs, mut props) = self.character(&style.format, size);
            if let Some(parent) = style.based_on.and_then(|p| self.names.character.get(&p)) {
                let _ = write!(props, "<BasedOn type=\"object\">{}</BasedOn>", esc(parent));
            }
            let shown = self_.trim_start_matches("CharacterStyle/").to_owned();
            let _ = writeln!(
                out,
                "  {}",
                Self::element(
                    "CharacterStyle",
                    &format!(" Self=\"{}\" Name=\"{}\"{attrs}", esc(&self_), esc(&shown)),
                    &props,
                    ""
                )
            );
        }
        out.push_str("</RootCharacterStyleGroup>\n<RootParagraphStyleGroup Self=\"u78\">\n");
        out.push_str(
            "  <ParagraphStyle Self=\"ParagraphStyle/$ID/[No paragraph style]\" Name=\"$ID/[No paragraph style]\"/>\n",
        );
        // [Basic Paragraph]: the document's own text, which unstyled
        // paragraphs are set in.
        let basic = CharacterFormat {
            family: Some(doc.text_default.family.clone()),
            size: Some(doc.text_default.size),
            line_height: Some(doc.text_default.line_height),
            colour: Some(doc.text_default.color.clone()),
            ..CharacterFormat::default()
        };
        let (attrs, props) = self.character(&basic, size);
        let _ = writeln!(
            out,
            "  {}",
            Self::element(
                "ParagraphStyle",
                &format!(
                    " Self=\"{NORMAL_PARAGRAPH_STYLE}\" Name=\"$ID/NormalParagraphStyle\"{attrs}"
                ),
                &props,
                ""
            )
        );
        for (id, style) in doc.paragraph_styles.iter() {
            let self_ = self.names.paragraph[&id].clone();
            let (attrs, mut props) = self.paragraph(&style.format, size);
            props.push_str(&self.automatic(&style.format));
            let parent = style
                .based_on
                .and_then(|p| self.names.paragraph.get(&p))
                .cloned()
                .unwrap_or_else(|| NORMAL_PARAGRAPH_STYLE.to_owned());
            let _ = write!(props, "<BasedOn type=\"object\">{}</BasedOn>", esc(&parent));
            let shown = self_.trim_start_matches("ParagraphStyle/").to_owned();
            let _ = writeln!(
                out,
                "  {}",
                Self::element(
                    "ParagraphStyle",
                    &format!(" Self=\"{}\" Name=\"{}\"{attrs}", esc(&self_), esc(&shown)),
                    &props,
                    ""
                )
            );
        }
        out.push_str("</RootParagraphStyleGroup>\n");
        out.push_str(
            "<RootCellStyleGroup Self=\"u7a\"><CellStyle Self=\"CellStyle/$ID/[None]\" Name=\"$ID/[None]\"/></RootCellStyleGroup>\n\
             <RootTableStyleGroup Self=\"u7b\"><TableStyle Self=\"TableStyle/$ID/[No table style]\" Name=\"$ID/[No table style]\"/></RootTableStyleGroup>\n",
        );
        out.push_str("<RootObjectStyleGroup Self=\"u7c\">\n  <ObjectStyle Self=\"ObjectStyle/$ID/[None]\" Name=\"$ID/[None]\"/>\n");
        for (id, style) in doc.object_styles.iter() {
            let self_ = self.names.object[&id].clone();
            let mut attrs = format!(
                " Self=\"{}\" Name=\"{}\"",
                esc(&self_),
                esc(self_.trim_start_matches("ObjectStyle/"))
            );
            if let Some(fill) = &style.format.fill {
                attrs.push_str(&self.palette.paint(doc, fill, "Fill", &mut self.dropped));
            }
            match &style.format.stroke {
                Some(Some(stroke)) => {
                    attrs.push_str(&self.palette.paint(
                        doc,
                        &Paint::Solid(stroke.color.clone()),
                        "Stroke",
                        &mut self.dropped,
                    ));
                    let _ = write!(attrs, " StrokeWeight=\"{}\"", n(stroke.width));
                }
                Some(None) => attrs.push_str(" StrokeColor=\"Swatch/None\""),
                None => {}
            }
            let _ = writeln!(out, "  <ObjectStyle{attrs}/>");
        }
        out.push_str("</RootObjectStyleGroup>\n</idPkg:Styles>\n");
        out
    }

    /// A story's paragraphs and runs as ranges.
    fn story_body(&mut self, story: &Story, footnote: bool) -> String {
        let doc = self.doc;
        let mut out = String::new();
        let text = &story.text;
        let mut footnotes = story.footnotes.iter();
        for para in &story.paragraphs {
            let size = story
                .runs
                .iter()
                .find(|r| r.range.contains(&para.range.start))
                .map(|r| story.resolve_run(r, doc))
                .and_then(|f| f.size)
                .unwrap_or(doc.text_default.size);
            let style = para
                .style
                .and_then(|s| self.names.paragraph.get(&s))
                .cloned()
                .unwrap_or_else(|| NORMAL_PARAGRAPH_STYLE.to_owned());
            let (attrs, props) = self.paragraph(&para.local, size);
            let _ = write!(
                out,
                "<ParagraphStyleRange AppliedParagraphStyle=\"{}\"{attrs}>",
                esc(&style)
            );
            if !props.is_empty() {
                let _ = write!(out, "<Properties>{props}</Properties>");
            }
            let mut wrote_any = false;
            for run in &story.runs {
                let start = run.range.start.max(para.range.start);
                let end = run.range.end.min(para.range.end);
                if start >= end && !(para.range.is_empty() && run.range.contains(&para.range.start))
                {
                    continue;
                }
                wrote_any = true;
                let run_size = story
                    .resolve_run(run, doc)
                    .size
                    .unwrap_or(doc.text_default.size);
                let style = run
                    .style
                    .and_then(|s| self.names.character.get(&s))
                    .cloned()
                    .unwrap_or_else(|| NO_CHARACTER_STYLE.to_owned());
                let (attrs, props) = self.character(&run.local, run_size);
                let _ = write!(
                    out,
                    "<CharacterStyleRange AppliedCharacterStyle=\"{}\"{attrs}>",
                    esc(&style)
                );
                if !props.is_empty() {
                    let _ = write!(out, "<Properties>{props}</Properties>");
                }
                let piece = text.get(start..end).unwrap_or("");
                let mut content = String::new();
                let flush = |content: &mut String, out: &mut String| {
                    if !content.is_empty() {
                        let _ = write!(out, "<Content>{}</Content>", esc(content));
                        content.clear();
                    }
                };
                for c in piece.chars() {
                    if c == '\n' {
                        flush(&mut content, &mut out);
                        out.push_str("<Br/>");
                        continue;
                    }
                    if c == tessera_document::anchored::MARKER {
                        self.dropped
                            .note("objects anchored in text (their places were kept empty)");
                        continue;
                    }
                    match Marker::of(c) {
                        None => content.push(c),
                        Some(marker) => {
                            flush(&mut content, &mut out);
                            let ace = match marker {
                                Marker::PageNumber => Some(18),
                                Marker::SectionMarker => Some(19),
                                Marker::NextPageNumber => Some(16),
                                Marker::PreviousPageNumber => Some(17),
                                Marker::FootnoteNumber if footnote => Some(4),
                                _ => None,
                            };
                            if let Some(code) = ace {
                                let _ = write!(out, "<Content><?ACE {code}?></Content>");
                            } else if marker == Marker::FootnoteReference {
                                if let Some(note) = footnotes.next() {
                                    let body = self.story_body(note, true);
                                    let _ = write!(out, "<Footnote>{body}</Footnote>");
                                }
                            } else {
                                self.dropped.note(
                                    "index entries, notes, text anchors, cross-references and \
                                     text variables in the text",
                                );
                            }
                        }
                    }
                }
                flush(&mut content, &mut out);
                out.push_str("</CharacterStyleRange>");
            }
            if !wrote_any {
                let _ = write!(
                    out,
                    "<CharacterStyleRange AppliedCharacterStyle=\"{NO_CHARACTER_STYLE}\"/>"
                );
            }
            out.push_str("</ParagraphStyleRange>");
        }
        out
    }

    fn story(&mut self, id: StoryId) -> Option<(String, String)> {
        let story = self.doc.story(id)?;
        let self_ = self.stories.get(&id)?.clone();
        let body = if story.text.is_empty() {
            format!(
                "<ParagraphStyleRange AppliedParagraphStyle=\"{NORMAL_PARAGRAPH_STYLE}\">\
                 <CharacterStyleRange AppliedCharacterStyle=\"{NO_CHARACTER_STYLE}\"/></ParagraphStyleRange>"
            )
        } else {
            self.story_body(story, false)
        };
        let xml = format!(
            "{HEAD}<idPkg:Story xmlns:idPkg=\"{PKG}\" DOMVersion=\"{DOM}\">\n\
             <Story Self=\"{self_}\" AppliedTOCStyle=\"n\" TrackChanges=\"false\" StoryTitle=\"$ID/\" AppliedNamedGrid=\"n\">\n\
             <StoryPreference OpticalMarginAlignment=\"false\" OpticalMarginSize=\"12\" FrameType=\"TextFrameType\" \
             StoryOrientation=\"Horizontal\" StoryDirection=\"LeftToRightDirection\"/>\n\
             <InCopyExportOption IncludeGraphicProxies=\"true\" IncludeAllResources=\"false\"/>\n\
             {body}\n</Story>\n</idPkg:Story>\n"
        );
        Some((self_, xml))
    }

    /// Path points from a frame-local path: each subpath, and whether it is
    /// open.
    fn points(path: &kurbo::BezPath) -> Vec<Subpath> {
        use kurbo::PathEl;
        let mut out: Vec<Subpath> = Vec::new();
        let mut current: Vec<PathPoint> = Vec::new();
        let mut last = (0.0, 0.0);
        let flush = |current: &mut Vec<PathPoint>, out: &mut Vec<Subpath>, closed: bool| {
            if current.is_empty() {
                return;
            }
            let mut points = std::mem::take(current);
            // A closing curve that ends on its start: one point, not two.
            if closed && points.len() > 1 {
                let first = points[0][1];
                let end = points[points.len() - 1][1];
                if (first.0 - end.0).abs() < 1e-9 && (first.1 - end.1).abs() < 1e-9 {
                    let last = points.pop().expect("more than one");
                    points[0][0] = last[0];
                }
            }
            out.push((points, !closed));
        };
        for el in path.elements() {
            match *el {
                PathEl::MoveTo(p) => {
                    flush(&mut current, &mut out, false);
                    last = (p.x, p.y);
                    current.push([last, last, last]);
                }
                PathEl::LineTo(p) => {
                    last = (p.x, p.y);
                    current.push([last, last, last]);
                }
                PathEl::QuadTo(c, p) => {
                    let c1 = (
                        last.0 + 2.0 / 3.0 * (c.x - last.0),
                        last.1 + 2.0 / 3.0 * (c.y - last.1),
                    );
                    let c2 = (p.x + 2.0 / 3.0 * (c.x - p.x), p.y + 2.0 / 3.0 * (c.y - p.y));
                    if let Some(prev) = current.last_mut() {
                        prev[2] = c1;
                    }
                    last = (p.x, p.y);
                    current.push([c2, last, last]);
                }
                PathEl::CurveTo(c1, c2, p) => {
                    if let Some(prev) = current.last_mut() {
                        prev[2] = (c1.x, c1.y);
                    }
                    last = (p.x, p.y);
                    current.push([(c2.x, c2.y), last, last]);
                }
                PathEl::ClosePath => flush(&mut current, &mut out, true),
            }
        }
        flush(&mut current, &mut out, false);
        out
    }

    fn geometry(paths: &[Subpath]) -> String {
        let mut out = String::from("<PathGeometry>");
        for (points, open) in paths {
            let _ = write!(
                out,
                "<GeometryPathType PathOpen=\"{open}\"><PathPointArray>"
            );
            for [left, anchor, right] in points {
                let _ = write!(
                    out,
                    "<PathPointType Anchor=\"{} {}\" LeftDirection=\"{} {}\" RightDirection=\"{} {}\"/>",
                    n(anchor.0),
                    n(anchor.1),
                    n(left.0),
                    n(left.1),
                    n(right.0),
                    n(right.1)
                );
            }
            out.push_str("</PathPointArray></GeometryPathType>");
        }
        out.push_str("</PathGeometry>");
        out
    }

    fn rectangle(w: f64, h: f64) -> Vec<Subpath> {
        let p = |x: f64, y: f64| [(x, y), (x, y), (x, y)];
        vec![(vec![p(0.0, 0.0), p(0.0, h), p(w, h), p(w, 0.0)], false)]
    }

    fn oval(w: f64, h: f64) -> Vec<Subpath> {
        const K: f64 = 0.552_284_749_8;
        let (cx, cy, rx, ry) = (w / 2.0, h / 2.0, w / 2.0, h / 2.0);
        vec![(
            vec![
                [(cx - K * rx, 0.0), (cx, 0.0), (cx + K * rx, 0.0)],
                [(w, cy - K * ry), (w, cy), (w, cy + K * ry)],
                [(cx + K * rx, h), (cx, h), (cx - K * rx, h)],
                [(0.0, cy + K * ry), (0.0, cy), (0.0, cy - K * ry)],
            ],
            false,
        )]
    }

    /// One frame and, for a group, its members, in spread space whose origin
    /// is `so` in the document.
    fn item(&mut self, id: FrameId, so: DocPoint) -> String {
        let doc = self.doc;
        let Some(frame) = doc.frame(id) else {
            return String::new();
        };
        if frame.anchor.is_some() {
            self.dropped
                .note("objects anchored in text (their places were kept empty)");
            return String::new();
        }
        let self_ = self.frames.get(&id).cloned().unwrap_or_else(|| self.id());
        let layer = doc
            .layer_ids()
            .find(|l| doc.layers.get(*l).is_some_and(|l| l.frames.contains(&id)))
            .and_then(|l| self.layers.get(&l))
            .cloned();

        if let FrameKind::Group(members) = &frame.kind {
            let mut inner = String::new();
            for member in members.clone() {
                inner.push_str(&self.item(member, so));
            }
            let layer = layer
                .map(|l| format!(" ItemLayer=\"{l}\""))
                .unwrap_or_default();
            return format!(
                "<Group Self=\"{self_}\"{layer} ItemTransform=\"1 0 0 1 0 0\" Visible=\"{}\" Locked=\"{}\">{inner}</Group>\n",
                !frame.hidden, frame.locked
            );
        }

        // The item's own space: its bounds' top-left at the origin, carried
        // by its turn into the spread's.
        let b = frame.bounds;
        let [a, bb, c, d, e, f] = frame.transform.coefficients;
        let (ex, fy) = (
            e + a * so.x + c * so.y - so.x,
            f + bb * so.x + d * so.y - so.y,
        );
        let (ox, oy) = (b.x - so.x, b.y - so.y);
        let tx = a * ox + c * oy + ex;
        let ty = bb * ox + d * oy + fy;
        let transform = format!("{} {} {} {} {} {}", n(a), n(bb), n(c), n(d), n(tx), n(ty));

        let mut attrs = format!(" Self=\"{self_}\"");
        if let Some(layer) = &layer {
            let _ = write!(attrs, " ItemLayer=\"{layer}\"");
        }
        let _ = write!(
            attrs,
            " Visible=\"{}\" Locked=\"{}\" ItemTransform=\"{transform}\"",
            !frame.hidden, frame.locked
        );
        attrs.push_str(
            &self
                .palette
                .paint(doc, &frame.fill, "Fill", &mut self.dropped),
        );
        match &frame.stroke {
            Some(stroke) if stroke.width > 0.0 => {
                attrs.push_str(&self.palette.paint(
                    doc,
                    &Paint::Solid(stroke.color.clone()),
                    "Stroke",
                    &mut self.dropped,
                ));
                let _ = write!(
                    attrs,
                    " StrokeWeight=\"{}\" StrokeType=\"StrokeStyle/$ID/Solid\"",
                    n(stroke.width)
                );
            }
            _ => attrs.push_str(" StrokeColor=\"Swatch/None\" StrokeWeight=\"0\""),
        }
        if frame.overprint.fill {
            attrs.push_str(" OverprintFill=\"true\"");
        }
        if frame.overprint.stroke {
            attrs.push_str(" OverprintStroke=\"true\"");
        }
        if let Some(style) = frame.style.and_then(|s| self.names.object.get(&s)) {
            let _ = write!(attrs, " AppliedObjectStyle=\"{}\"", esc(style));
        }
        if !frame.corners.is_square() {
            self.dropped
                .note("rounded and fancy corners (written square)");
        }
        if frame.feather.is_some() {
            self.dropped.note("gradient feathers");
        }

        let (tag, paths, mut inner) = match &frame.kind {
            FrameKind::Rectangle => (
                "Rectangle",
                Self::rectangle(b.width, b.height),
                String::new(),
            ),
            FrameKind::Ellipse => ("Oval", Self::oval(b.width, b.height), String::new()),
            FrameKind::Path(path) => {
                let paths = Self::points(path);
                let line = paths.len() == 1 && paths[0].1 && paths[0].0.len() == 2;
                (
                    if line { "GraphicLine" } else { "Polygon" },
                    paths,
                    String::new(),
                )
            }
            FrameKind::Graphic { placed } => {
                let mut inner = String::new();
                if let Some(placed) = placed
                    && let Some(link) = doc.links.get(placed.link)
                {
                    let is_pdf = link.path.extension().is_some_and(|e| {
                        e.eq_ignore_ascii_case("pdf") || e.eq_ignore_ascii_case("ai")
                    });
                    let kind = if is_pdf { "PDF" } else { "Image" };
                    let [ia, ib, ic, id_, ie, if_] = placed.inner.coefficients;
                    let pdf = if is_pdf {
                        let crop = match link.pdf.crop {
                            tessera_document::links::PdfBox::Art => "CropArt",
                            tessera_document::links::PdfBox::Trim => "CropTrim",
                            tessera_document::links::PdfBox::Bleed => "CropBleed",
                            tessera_document::links::PdfBox::Media => "CropMedia",
                            _ => "CropContentVisibleLayers",
                        };
                        format!(
                            "<PDFAttribute PageNumber=\"{}\" PDFCrop=\"{crop}\" TransparentBackground=\"true\"/>",
                            link.pdf.page + 1
                        )
                    } else {
                        String::new()
                    };
                    let image_id = self.id();
                    let link_id = self.id();
                    let _ = write!(
                        inner,
                        "<{kind} Self=\"{image_id}\" ItemTransform=\"{} {} {} {} {} {}\">\
                         <Properties><GraphicBounds Left=\"0\" Top=\"0\" Right=\"{}\" Bottom=\"{}\"/></Properties>\
                         {pdf}<Link Self=\"{link_id}\" LinkResourceURI=\"{}\" StoredState=\"Normal\"/></{kind}>",
                        n(ia),
                        n(ib),
                        n(ic),
                        n(id_),
                        n(ie),
                        n(if_),
                        n(link.natural.0),
                        n(link.natural.1),
                        esc(&uri(&link.path))
                    );
                }
                ("Rectangle", Self::rectangle(b.width, b.height), inner)
            }
            FrameKind::Text { story, layout } => {
                let story_self = self
                    .stories
                    .get(story)
                    .cloned()
                    .unwrap_or_else(|| "n".to_owned());
                let next = doc
                    .next_in_thread(id)
                    .and_then(|f| self.frames.get(&f))
                    .cloned()
                    .unwrap_or_else(|| "n".to_owned());
                let previous = self
                    .previous
                    .get(&id)
                    .and_then(|f| self.frames.get(f))
                    .cloned()
                    .unwrap_or_else(|| "n".to_owned());
                let _ = write!(
                    attrs,
                    " ParentStory=\"{story_self}\" PreviousTextFrame=\"{previous}\" NextTextFrame=\"{next}\" ContentType=\"TextType\""
                );
                let vertical = match layout.vertical {
                    VerticalJustify::Centre => "CenterAlign",
                    VerticalJustify::Bottom => "BottomAlign",
                    VerticalJustify::Justify => "JustifyAlign",
                    _ => "TopAlign",
                };
                let mut pref = format!(
                    " TextColumnCount=\"{}\" TextColumnGutter=\"{}\" VerticalJustification=\"{vertical}\" VerticalBalanceColumns=\"{}\"",
                    layout.columns.max(1),
                    n(layout.gutter),
                    layout.balance
                );
                if let Some(auto) = layout.auto_size {
                    use tessera_document::nodes::AutoGrow;
                    use tessera_geometry::Anchor;
                    let grow = match auto.grow {
                        AutoGrow::Height => "HeightOnly",
                        AutoGrow::Width => "WidthOnly",
                        AutoGrow::Both => "HeightAndWidth",
                    };
                    let from = match auto.from {
                        Anchor::TopLeft => "TopLeftPoint",
                        Anchor::TopRight => "TopRightPoint",
                        Anchor::MiddleLeft => "LeftCenterPoint",
                        Anchor::Centre => "CenterPoint",
                        Anchor::MiddleRight => "RightCenterPoint",
                        Anchor::BottomLeft => "BottomLeftPoint",
                        Anchor::BottomCentre => "BottomCenterPoint",
                        Anchor::BottomRight => "BottomRightPoint",
                        _ => "TopCenterPoint",
                    };
                    let _ = write!(
                        pref,
                        " AutoSizingType=\"{grow}\" AutoSizingReferencePoint=\"{from}\""
                    );
                    if let Some(h) = auto.min_height {
                        let _ = write!(
                            pref,
                            " UseMinimumHeightForAutoSizing=\"true\" MinimumHeightForAutoSizing=\"{}\"",
                            n(h)
                        );
                    }
                    if let Some(w) = auto.min_width {
                        let _ = write!(
                            pref,
                            " UseMinimumWidthForAutoSizing=\"true\" MinimumWidthForAutoSizing=\"{}\"",
                            n(w)
                        );
                    }
                }
                let i = layout.inset;
                let inner = format!(
                    "<TextFramePreference{pref}><Properties><InsetSpacing type=\"list\">\
                     <ListItem type=\"unit\">{}</ListItem><ListItem type=\"unit\">{}</ListItem>\
                     <ListItem type=\"unit\">{}</ListItem><ListItem type=\"unit\">{}</ListItem>\
                     </InsetSpacing></Properties></TextFramePreference>",
                    n(i.top),
                    n(i.left),
                    n(i.bottom),
                    n(i.right)
                );
                ("TextFrame", Self::rectangle(b.width, b.height), inner)
            }
            FrameKind::Table(_) | FrameKind::TablePart { .. } => {
                self.dropped.note("tables");
                return String::new();
            }
            FrameKind::Group(_) => unreachable!("groups are written above"),
        };
        if doc.path_texts.contains_key(id) {
            self.dropped.note("text on a path");
        }

        // Effects.
        let mut transparency = String::new();
        if !frame.blend.is_plain() {
            use tessera_document::blending::BlendMode;
            let mode = match frame.blend.mode {
                BlendMode::Multiply => "Multiply",
                BlendMode::Screen => "Screen",
                BlendMode::Overlay => "Overlay",
                _ => "Normal",
            };
            let _ = write!(
                transparency,
                "<BlendingSetting BlendMode=\"{mode}\" Opacity=\"{}\"/>",
                n(f64::from(frame.blend.opacity) * 100.0)
            );
        }
        if let Some(shadow) = &frame.shadow {
            let opacity = shadow.colour.to_rgb_f32()[3];
            let colour = self
                .palette
                .colour(
                    doc,
                    &opaque(&doc.resolve_colour(&shadow.colour)),
                    &mut self.dropped,
                )
                .map(|(s, _)| s)
                .unwrap_or_else(|| "Color/Black".to_owned());
            let _ = write!(
                transparency,
                "<DropShadowSetting Mode=\"Drop\" EffectColor=\"{}\" Opacity=\"{}\" XOffset=\"{}\" YOffset=\"{}\" Size=\"{}\"/>",
                esc(&colour),
                n(f64::from(opacity) * 100.0),
                n(shadow.offset.0),
                n(shadow.offset.1),
                n(shadow.blur)
            );
        }
        if !transparency.is_empty() {
            inner.insert_str(
                0,
                &format!("<TransparencySetting>{transparency}</TransparencySetting>"),
            );
        }
        inner.insert_str(0, &wrap(&frame.wrap));

        format!(
            "<{tag}{attrs}><Properties>{}</Properties>{inner}</{tag}>\n",
            Self::geometry(&paths)
        )
    }

    /// A spread or a parent spread, with its pages and its items.
    fn spread(&mut self, spread: SpreadId, master: Option<(&str, &str)>) -> String {
        let doc = self.doc;
        let pages = doc.pages_of(spread);
        let rects: Vec<DocRect> = pages
            .iter()
            .filter_map(|p| doc.pages.get(*p).map(|p| p.bounds))
            .collect();
        let (x0, y0, x1, y1) = rects.iter().fold(
            (f64::MAX, f64::MAX, f64::MIN, f64::MIN),
            |(x0, y0, x1, y1), r| {
                (
                    x0.min(r.x),
                    y0.min(r.y),
                    x1.max(r.x + r.width),
                    y1.max(r.y + r.height),
                )
            },
        );
        let so = if rects.is_empty() {
            DocPoint { x: 0.0, y: 0.0 }
        } else {
            DocPoint {
                x: (x0 + x1) / 2.0,
                y: (y0 + y1) / 2.0,
            }
        };
        let m = doc.setup.margins;
        let mut body = String::new();
        for page in &pages {
            let Some(p) = doc.pages.get(*page) else {
                continue;
            };
            let self_ = self.pages.get(page).cloned().unwrap_or_else(|| self.id());
            let name = if master.is_some() {
                "A".to_owned()
            } else {
                doc.page_label(*page).unwrap_or_else(|| "1".to_owned())
            };
            let applied = p
                .master
                .and_then(|mp| {
                    doc.master_ids()
                        .find(|mid| doc.pages_of_master(*mid).contains(&mp))
                })
                .and_then(|mid| self.masters_self.get(&mid))
                .cloned()
                .unwrap_or_else(|| "n".to_owned());
            let _ = writeln!(
                body,
                "<Page Self=\"{self_}\" Name=\"{}\" AppliedMaster=\"{applied}\" GeometricBounds=\"0 0 {} {}\" \
                 ItemTransform=\"1 0 0 1 {} {}\" OverrideList=\"\" MasterPageTransform=\"1 0 0 1 0 0\">\
                 <MarginPreference ColumnCount=\"1\" ColumnGutter=\"12\" Top=\"{}\" Bottom=\"{}\" Left=\"{}\" Right=\"{}\" \
                 ColumnDirection=\"Horizontal\" ColumnsPositions=\"0 {}\"/></Page>",
                esc(&name),
                n(p.bounds.height),
                n(p.bounds.width),
                n(p.bounds.x - so.x),
                n(p.bounds.y - so.y),
                n(m.top),
                n(m.bottom),
                n(m.inside),
                n(m.outside),
                n(p.bounds.width - m.inside - m.outside)
            );
        }
        // Every top-level object whose page is one of these, back to front.
        let order: Vec<FrameId> = doc
            .layer_ids()
            .filter_map(|l| doc.layers.get(l))
            .flat_map(|l| l.frames.iter().copied())
            .filter(|f| doc.page_of_frame(*f).is_some_and(|p| pages.contains(&p)))
            .collect();
        for frame in order {
            body.push_str(&self.item(frame, so));
        }
        let self_ = self.id();
        match master {
            Some((master_self, name)) => format!(
                "{HEAD}<idPkg:MasterSpread xmlns:idPkg=\"{PKG}\" DOMVersion=\"{DOM}\">\n\
                 <MasterSpread Self=\"{master_self}\" Name=\"{}\" NamePrefix=\"{}\" BaseName=\"{}\" \
                 PageCount=\"{}\" ItemTransform=\"1 0 0 1 0 0\" ShowMasterItems=\"true\">\n{body}</MasterSpread>\n</idPkg:MasterSpread>\n",
                esc(name),
                esc(name.split('-').next().unwrap_or("A")),
                esc(name.split_once('-').map_or(name, |(_, b)| b)),
                pages.len()
            ),
            None => format!(
                "{HEAD}<idPkg:Spread xmlns:idPkg=\"{PKG}\" DOMVersion=\"{DOM}\">\n\
                 <Spread Self=\"{self_}\" PageCount=\"{}\" BindingLocation=\"{}\" AllowPageShuffle=\"true\" \
                 ItemTransform=\"1 0 0 1 0 0\" ShowMasterItems=\"true\" PageTransitionType=\"None\">\n{body}</Spread>\n</idPkg:Spread>\n",
                pages.len(),
                if pages.len() > 1 { 1 } else { 0 }
            ),
        }
    }
}

/// A colour at full strength, for an effect whose opacity is said apart.
fn opaque(colour: &Color) -> Color {
    match colour.clone() {
        Color::Rgb { r, g, b, .. } => Color::Rgb { r, g, b, a: 1.0 },
        Color::Cmyk { c, m, y, k, .. } => Color::Cmyk { c, m, y, k, a: 1.0 },
        Color::Lab { l, a, b, .. } => Color::Lab {
            l,
            a,
            b,
            alpha: 1.0,
        },
        other => other,
    }
}

fn wrap(wrap: &TextWrap) -> String {
    let side = |sides: &WrapTo| match sides {
        WrapTo::Both => "BothSides",
        WrapTo::Left => "LeftSide",
        WrapTo::Right => "RightSide",
        WrapTo::TowardsSpine => "SideTowardsSpine",
        WrapTo::AwayFromSpine => "SideAwayFromSpine",
        _ => "LargestArea",
    };
    let offset = |i: &tessera_document::nodes::Insets| {
        format!(
            "<Properties><TextWrapOffset Top=\"{}\" Left=\"{}\" Bottom=\"{}\" Right=\"{}\"/></Properties>",
            n(i.top),
            n(i.left),
            n(i.bottom),
            n(i.right)
        )
    };
    match wrap {
        TextWrap::None => String::new(),
        TextWrap::Bounds { standoff, sides } => format!(
            "<TextWrapPreference TextWrapMode=\"BoundingBoxTextWrap\" TextWrapSide=\"{}\">{}</TextWrapPreference>",
            side(sides),
            offset(standoff)
        ),
        TextWrap::Contour { standoff, sides } => {
            let all = tessera_document::nodes::Insets {
                top: *standoff,
                left: *standoff,
                bottom: *standoff,
                right: *standoff,
            };
            format!(
                "<TextWrapPreference TextWrapMode=\"Contour\" TextWrapSide=\"{}\">{}</TextWrapPreference>",
                side(sides),
                offset(&all)
            )
        }
        TextWrap::Jump => "<TextWrapPreference TextWrapMode=\"JumpObjectTextWrap\"/>".to_owned(),
        #[allow(unreachable_patterns)]
        _ => String::new(),
    }
}

/// The document as an IDML package.
pub fn write(doc: &Document) -> Result<Written, String> {
    let mut w = Writer {
        doc,
        palette: Palette::default(),
        names: StyleNames::default(),
        dropped: Dropped::default(),
        frames: HashMap::new(),
        stories: HashMap::new(),
        layers: HashMap::new(),
        pages: HashMap::new(),
        previous: HashMap::new(),
        fonts: Vec::new(),
        count: 0,
        masters_self: HashMap::new(),
    };

    // Every name an item may point at, made before anything is written.
    let frame_ids: Vec<FrameId> = doc.frames.keys().collect();
    for id in &frame_ids {
        let name = w.id();
        w.frames.insert(*id, name);
        if let Some(next) = doc.next_in_thread(*id) {
            w.previous.insert(next, *id);
        }
    }
    let mut used_stories: Vec<StoryId> = Vec::new();
    for id in &frame_ids {
        if let Some(Frame {
            kind: FrameKind::Text { story, .. },
            anchor: None,
            ..
        }) = doc.frame(*id)
            && !used_stories.contains(story)
        {
            used_stories.push(*story);
        }
    }
    for story in &used_stories {
        let name = w.id();
        w.stories.insert(*story, name);
    }
    let layer_ids: Vec<LayerId> = doc.layer_ids().collect();
    for layer in &layer_ids {
        let name = w.id();
        w.layers.insert(*layer, name);
    }
    for page in doc.pages.keys() {
        let name = w.id();
        w.pages.insert(page, name);
    }
    let master_ids: Vec<_> = doc.master_ids().collect();
    for master in &master_ids {
        let name = w.id();
        w.masters_self.insert(*master, name);
    }

    // Swatches first, so a reference to one finds it.
    for swatch in &doc.swatches {
        let colour = doc.resolve_colour(&swatch.colour);
        if swatch.name == "Black" {
            continue;
        }
        w.palette.swatch(&swatch.name, &colour, swatch.spot);
    }
    // Colour groups, each naming its swatches.
    let mut colour_groups = String::new();
    for (i, group) in doc.swatch_groups().iter().enumerate() {
        let _ = write!(
            colour_groups,
            "<ColorGroup Self=\"ColorGroup/u{i}\" Name=\"{}\" IsStandard=\"false\">",
            esc(group)
        );
        for (j, swatch) in doc
            .swatches
            .iter()
            .filter(|s| s.group.as_ref() == Some(group))
            .enumerate()
        {
            let _ = write!(
                colour_groups,
                "<ColorGroupSwatch Self=\"ColorGroup/u{i}Swatch{j}\" SwatchItemRef=\"Color/{}\"/>",
                esc(&swatch.name)
            );
        }
        colour_groups.push_str("</ColorGroup>\n");
    }

    let styles = w.styles();
    let mut stories = Vec::new();
    for story in &used_stories {
        if let Some(written) = w.story(*story) {
            stories.push(written);
        }
    }
    let mut masters = Vec::new();
    for master in &master_ids {
        let Some(m) = doc.masters.get(*master) else {
            continue;
        };
        let self_ = w.masters_self[master].clone();
        let xml = w.spread(m.spread, Some((&self_, &m.name)));
        masters.push((self_, xml));
    }
    let mut spreads = Vec::new();
    for (i, spread) in doc.spread_ids().collect::<Vec<_>>().into_iter().enumerate() {
        spreads.push((i, w.spread(spread, None)));
    }

    let graphic = format!(
        "{HEAD}<idPkg:Graphic xmlns:idPkg=\"{PKG}\" DOMVersion=\"{DOM}\">\n\
         <Color Self=\"Color/Black\" Model=\"Process\" Space=\"CMYK\" ColorValue=\"0 0 0 100\" ColorOverride=\"Specialblack\" \
         AlternateSpace=\"NoAlternateColor\" AlternateColorValue=\"\" Name=\"Black\" ColorEditable=\"false\" ColorRemovable=\"false\" \
         Visible=\"true\" SwatchCreatorID=\"7937\"/>\n\
         <Color Self=\"Color/Paper\" Model=\"Process\" Space=\"CMYK\" ColorValue=\"0 0 0 0\" ColorOverride=\"Specialpaper\" \
         AlternateSpace=\"NoAlternateColor\" AlternateColorValue=\"\" Name=\"Paper\" ColorEditable=\"true\" ColorRemovable=\"false\" \
         Visible=\"true\" SwatchCreatorID=\"7937\"/>\n\
         <Color Self=\"Color/Registration\" Model=\"Registration\" Space=\"CMYK\" ColorValue=\"100 100 100 100\" \
         ColorOverride=\"Specialregistration\" AlternateSpace=\"NoAlternateColor\" AlternateColorValue=\"\" Name=\"Registration\" \
         ColorEditable=\"false\" ColorRemovable=\"false\" Visible=\"true\" SwatchCreatorID=\"7937\"/>\n\
         {}\
         <Ink Self=\"Ink/$ID/Process Cyan\" Name=\"$ID/Process Cyan\" Angle=\"75\" ConvertToProcess=\"false\" Frequency=\"70\" \
         NeutralDensity=\"0.61\" PrintInk=\"true\" TrapOrder=\"1\" InkType=\"Normal\"/>\n\
         <Ink Self=\"Ink/$ID/Process Magenta\" Name=\"$ID/Process Magenta\" Angle=\"15\" ConvertToProcess=\"false\" Frequency=\"70\" \
         NeutralDensity=\"0.76\" PrintInk=\"true\" TrapOrder=\"2\" InkType=\"Normal\"/>\n\
         <Ink Self=\"Ink/$ID/Process Yellow\" Name=\"$ID/Process Yellow\" Angle=\"0\" ConvertToProcess=\"false\" Frequency=\"70\" \
         NeutralDensity=\"0.16\" PrintInk=\"true\" TrapOrder=\"3\" InkType=\"Normal\"/>\n\
         <Ink Self=\"Ink/$ID/Process Black\" Name=\"$ID/Process Black\" Angle=\"45\" ConvertToProcess=\"false\" Frequency=\"70\" \
         NeutralDensity=\"1.7\" PrintInk=\"true\" TrapOrder=\"4\" InkType=\"Normal\"/>\n\
         <Swatch Self=\"Swatch/None\" Name=\"None\" ColorEditable=\"false\" ColorRemovable=\"false\" Visible=\"true\" SwatchCreatorID=\"7937\"/>\n\
         <StrokeStyle Self=\"StrokeStyle/$ID/Solid\" Name=\"$ID/Solid\"/>\n\
         {colour_groups}</idPkg:Graphic>\n",
        w.palette.elements
    );

    let mut fonts = format!("{HEAD}<idPkg:Fonts xmlns:idPkg=\"{PKG}\" DOMVersion=\"{DOM}\">\n");
    for (i, (family, faces)) in w.fonts.iter().enumerate() {
        let _ = write!(
            fonts,
            "<FontFamily Self=\"di{i}\" Name=\"{}\">",
            esc(family)
        );
        for (j, face) in faces.iter().enumerate() {
            let _ = write!(
                fonts,
                "<Font Self=\"di{i}Font{j}\" FontFamily=\"{}\" Name=\"{} {}\" PostScriptName=\"$ID/\" \
                 Status=\"NotAvailable\" FontStyleName=\"{}\" FontType=\"OpenTypeCFF\" WritingScript=\"0\"/>",
                esc(family),
                esc(family),
                esc(face),
                esc(face)
            );
        }
        fonts.push_str("</FontFamily>\n");
    }
    fonts.push_str("</idPkg:Fonts>\n");

    let s = doc.setup;
    let first = doc
        .page_ids()
        .next()
        .and_then(|p| doc.pages.get(p))
        .map(|p| p.bounds);
    let (pw, ph) = first.map_or((612.0, 792.0), |b| (b.width, b.height));
    let preferences = format!(
        "{HEAD}<idPkg:Preferences xmlns:idPkg=\"{PKG}\" DOMVersion=\"{DOM}\">\n\
         <DocumentPreference PageHeight=\"{}\" PageWidth=\"{}\" PagesPerDocument=\"{}\" FacingPages=\"{}\" \
         DocumentBleedTopOffset=\"{}\" DocumentBleedBottomOffset=\"{}\" DocumentBleedInsideOrLeftOffset=\"{}\" \
         DocumentBleedOutsideOrRightOffset=\"{}\" DocumentBleedUniformSize=\"false\" SlugBottomOffset=\"{}\" \
         SlugTopOffset=\"{}\" SlugInsideOrLeftOffset=\"{}\" SlugRightOrOutsideOffset=\"{}\" DocumentSlugUniformSize=\"false\" \
         PreserveLayoutWhenShuffling=\"true\" AllowPageShuffle=\"true\" OverprintBlack=\"true\" PageBinding=\"LeftToRight\" \
         ColumnDirection=\"Horizontal\" Intent=\"PrintIntent\"/>\n\
         <MarginPreference ColumnCount=\"1\" ColumnGutter=\"12\" Top=\"{}\" Bottom=\"{}\" Left=\"{}\" Right=\"{}\" \
         ColumnDirection=\"Horizontal\" ColumnsPositions=\"0 {}\"/>\n\
         </idPkg:Preferences>\n",
        n(ph),
        n(pw),
        doc.page_ids().count(),
        s.facing_pages,
        n(s.bleed.top),
        n(s.bleed.bottom),
        n(s.bleed.left),
        n(s.bleed.right),
        n(s.slug.bottom),
        n(s.slug.top),
        n(s.slug.left),
        n(s.slug.right),
        n(s.margins.top),
        n(s.margins.bottom),
        n(s.margins.inside),
        n(s.margins.outside),
        n(pw - s.margins.inside - s.margins.outside)
    );

    // The spine.
    let story_list: Vec<&str> = stories.iter().map(|(s, _)| s.as_str()).collect();
    let mut spine = format!(
        "{HEAD}<?aid style=\"50\" type=\"document\" readerVersion=\"6.0\" featureSet=\"257\" product=\"8.0(370)\" ?>\n\
         <Document xmlns:idPkg=\"{PKG}\" DOMVersion=\"{DOM}\" Self=\"d\" StoryList=\"{}\" ZeroPoint=\"0 0\" \
         ActiveLayer=\"{}\" CMYKProfile=\"$ID/\" RGBProfile=\"$ID/\" SolidColorIntent=\"UseColorSettings\" \
         AfterBlendingIntent=\"UseColorSettings\" DefaultImageIntent=\"UseColorSettings\" RGBPolicy=\"PreserveEmbeddedProfiles\" \
         CMYKPolicy=\"CombinationOfPreserveAndSafeCmyk\" AccurateLABSpots=\"false\">\n\
         <idPkg:Graphic src=\"Resources/Graphic.xml\"/>\n\
         <idPkg:Fonts src=\"Resources/Fonts.xml\"/>\n\
         <idPkg:Styles src=\"Resources/Styles.xml\"/>\n\
         <idPkg:Preferences src=\"Resources/Preferences.xml\"/>\n\
         <idPkg:Tags src=\"XML/Tags.xml\"/>\n",
        story_list.join(" "),
        layer_ids
            .last()
            .and_then(|l| w.layers.get(l))
            .cloned()
            .unwrap_or_default()
    );
    // InDesign lists the top layer first.
    for layer in layer_ids.iter().rev() {
        let Some(l) = doc.layers.get(*layer) else {
            continue;
        };
        let _ = writeln!(
            spine,
            "<Layer Self=\"{}\" Name=\"{}\" Visible=\"{}\" Locked=\"{}\" IgnoreWrap=\"false\" ShowGuides=\"true\" \
             LockGuides=\"false\" UI=\"true\" Expendable=\"true\" Printable=\"true\">\
             <Properties><LayerColor type=\"enumeration\">LightBlue</LayerColor></Properties></Layer>",
            w.layers[layer],
            esc(&l.name),
            l.visible,
            l.locked
        );
    }
    for (self_, _) in &masters {
        let _ = writeln!(
            spine,
            "<idPkg:MasterSpread src=\"MasterSpreads/MasterSpread_{self_}.xml\"/>"
        );
    }
    for (i, _) in &spreads {
        let _ = writeln!(spine, "<idPkg:Spread src=\"Spreads/Spread_{i}.xml\"/>");
    }
    // Sections: the implied first, unless the document says otherwise, and
    // each one it has.
    let page_order: Vec<PageId> = doc.page_ids().collect();
    let mut sections: Vec<(usize, &tessera_document::sections::Section)> = doc
        .sections
        .iter()
        .filter_map(|s| {
            page_order
                .iter()
                .position(|p| *p == s.first)
                .map(|i| (i, s))
        })
        .collect();
    sections.sort_by_key(|(i, _)| *i);
    if sections.first().is_none_or(|(i, _)| *i != 0)
        && let Some(first) = page_order.first()
    {
        let length = sections.first().map_or(page_order.len(), |(i, _)| *i);
        let _ = writeln!(
            spine,
            "<Section Self=\"usec0\" Length=\"{length}\" Name=\"\" ContinueNumbering=\"false\" IncludeSectionPrefix=\"false\" \
             PageNumberStyle=\"Arabic\" PageStart=\"{}\" SectionPrefix=\"\" PageNumberStart=\"1\" Marker=\"\"/>",
            w.pages[first]
        );
    }
    for (k, (at, section)) in sections.iter().enumerate() {
        let next = sections.get(k + 1).map_or(page_order.len(), |(i, _)| *i);
        let style = match section.style {
            tessera_text::story::Numbering::LowerRoman => "LowerRoman",
            tessera_text::story::Numbering::UpperRoman => "UpperRoman",
            tessera_text::story::Numbering::LowerAlpha => "LowerLetters",
            tessera_text::story::Numbering::UpperAlpha => "UpperLetters",
            _ => "Arabic",
        };
        let start = section
            .start
            .map(|s| format!(" PageNumberStart=\"{s}\""))
            .unwrap_or_default();
        let _ = writeln!(
            spine,
            "<Section Self=\"usec{}\" Length=\"{}\" Name=\"\" ContinueNumbering=\"{}\" IncludeSectionPrefix=\"{}\" \
             PageNumberStyle=\"{style}\" PageStart=\"{}\" SectionPrefix=\"{}\"{start} Marker=\"{}\"/>",
            k + 1,
            next - at,
            section.start.is_none(),
            section.include_prefix,
            w.pages[&section.first],
            esc(&section.prefix),
            esc(&section.marker)
        );
    }
    spine.push_str("<idPkg:BackingStory src=\"XML/BackingStory.xml\"/>\n");
    for (self_, _) in &stories {
        let _ = writeln!(spine, "<idPkg:Story src=\"Stories/Story_{self_}.xml\"/>");
    }
    spine.push_str("</Document>\n");

    let tags = format!(
        "{HEAD}<idPkg:Tags xmlns:idPkg=\"{PKG}\" DOMVersion=\"{DOM}\">\n\
         <XMLTag Self=\"XMLTag/Root\" Name=\"Root\"><Properties><TagColor type=\"enumeration\">LightBlue</TagColor></Properties></XMLTag>\n\
         </idPkg:Tags>\n"
    );
    let backing = format!(
        "{HEAD}<idPkg:BackingStory xmlns:idPkg=\"{PKG}\" DOMVersion=\"{DOM}\">\n\
         <XmlStory Self=\"ubacking\" AppliedTOCStyle=\"n\" TrackChanges=\"false\" StoryTitle=\"$ID/\" AppliedNamedGrid=\"n\">\
         <ParagraphStyleRange AppliedParagraphStyle=\"{NORMAL_PARAGRAPH_STYLE}\">\
         <CharacterStyleRange AppliedCharacterStyle=\"{NO_CHARACTER_STYLE}\"/></ParagraphStyleRange></XmlStory>\n\
         </idPkg:BackingStory>\n"
    );
    let container = format!(
        "{HEAD}<container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\
         <rootfiles><rootfile full-path=\"designmap.xml\" media-type=\"text/xml\"/></rootfiles></container>\n"
    );

    // The package. `mimetype` first and stored, as the format requires.
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut put = |name: &str, bytes: &[u8], options| -> Result<(), String> {
        zip.start_file(name, options).map_err(|e| e.to_string())?;
        zip.write_all(bytes).map_err(|e| e.to_string())
    };
    put(
        "mimetype",
        b"application/vnd.adobe.indesign-idml-package",
        stored,
    )?;
    put("META-INF/container.xml", container.as_bytes(), deflated)?;
    put("designmap.xml", spine.as_bytes(), deflated)?;
    put("Resources/Graphic.xml", graphic.as_bytes(), deflated)?;
    put("Resources/Fonts.xml", fonts.as_bytes(), deflated)?;
    put("Resources/Styles.xml", styles.as_bytes(), deflated)?;
    put(
        "Resources/Preferences.xml",
        preferences.as_bytes(),
        deflated,
    )?;
    put("XML/Tags.xml", tags.as_bytes(), deflated)?;
    put("XML/BackingStory.xml", backing.as_bytes(), deflated)?;
    for (self_, xml) in &masters {
        put(
            &format!("MasterSpreads/MasterSpread_{self_}.xml"),
            xml.as_bytes(),
            deflated,
        )?;
    }
    for (i, xml) in &spreads {
        put(&format!("Spreads/Spread_{i}.xml"), xml.as_bytes(), deflated)?;
    }
    for (self_, xml) in &stories {
        put(
            &format!("Stories/Story_{self_}.xml"),
            xml.as_bytes(),
            deflated,
        )?;
    }
    let bytes = zip.finish().map_err(|e| e.to_string())?.into_inner();
    Ok(Written {
        bytes,
        dropped: w.dropped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_document::nodes::Swatch;
    use tessera_geometry::Transform;
    use tessera_text::story::{CharacterStyle, ParagraphStyle};

    fn frame(bounds: DocRect, kind: FrameKind) -> Frame {
        Frame {
            bounds,
            kind,
            transform: Transform::IDENTITY,
            fill: Paint::Solid(Color::Rgb {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            }),
            stroke: None,
            wrap: TextWrap::None,
            blend: tessera_document::blending::Blending::PLAIN,
            corners: tessera_document::corners::Corners::SQUARE,
            shadow: None,
            feather: None,
            anchor: None,
            style: None,
            hidden: false,
            locked: false,
            overprint: Default::default(),
        }
    }

    fn rect(x: f64, y: f64, width: f64, height: f64) -> DocRect {
        DocRect {
            x,
            y,
            width,
            height,
        }
    }

    /// Two pages; a styled story threaded through two frames; a swatch-filled,
    /// stroked rectangle; a turned ellipse; an open path; a placed picture.
    fn a_document() -> (Document, Vec<FrameId>) {
        let mut doc = Document::new();
        doc.add_page();
        let layer = doc.default_layer().expect("layer");
        let pages: Vec<PageId> = doc.page_ids().collect();
        let p0 = doc.pages[pages[0]].bounds;
        let p1 = doc.pages[pages[1]].bounds;

        doc.set_swatch(Swatch {
            group: Some("Brand".into()),
            name: "Brand red".into(),
            colour: Color::Cmyk {
                c: 0.0,
                m: 0.9,
                y: 0.8,
                k: 0.05,
                a: 1.0,
            },
            spot: false,
        });
        let emphasis = doc.add_character_style(CharacterStyle {
            name: "Emphasis".into(),
            based_on: None,
            format: CharacterFormat {
                italic: Some(true),
                ..CharacterFormat::default()
            },
        });
        let body = doc.add_paragraph_style(ParagraphStyle {
            name: "Body".into(),
            format: ParagraphFormat {
                space_after: Some(6.0),
                character: CharacterFormat {
                    size: Some(10.0),
                    line_height: Some(1.3),
                    ..CharacterFormat::default()
                },
                ..ParagraphFormat::default()
            },
            ..ParagraphStyle::default()
        });
        let heading = doc.add_paragraph_style(ParagraphStyle {
            name: "Heading".into(),
            based_on: Some(body),
            format: ParagraphFormat {
                alignment: Some(Alignment::Centre),
                character: CharacterFormat {
                    size: Some(18.0),
                    weight: Some(700),
                    ..CharacterFormat::default()
                },
                ..ParagraphFormat::default()
            },
        });
        let page = Marker::PageNumber.character();
        let text = format!("A heading\nSome body with emphasis & more, page {page}.");
        let mut story = Story::new(text.clone());
        story.set_paragraph_style(0..1, Some(heading));
        let second = text.find("Some").expect("there");
        story.set_paragraph_style(second..second + 1, Some(body));
        let word = text.find("emphasis").expect("there");
        story.set_character_style(word..word + "emphasis".len(), Some(emphasis));
        let story = doc.add_story(story);

        let mut one = frame(
            rect(p0.x + 36.0, p0.y + 36.0, 200.0, 100.0),
            FrameKind::text(story),
        );
        if let FrameKind::Text { layout, .. } = &mut one.kind {
            layout.columns = 2;
            layout.gutter = 10.0;
        }
        let one = doc.add_frame(layer, one);
        let two = doc.add_frame(
            layer,
            frame(
                rect(p1.x + 36.0, p1.y + 36.0, 200.0, 100.0),
                FrameKind::text(story),
            ),
        );
        doc.thread(one, two);

        let mut boxed = frame(
            rect(p0.x + 300.0, p0.y + 300.0, 80.0, 40.0),
            FrameKind::Rectangle,
        );
        boxed.fill = Paint::Solid(Color::Swatch {
            name: "Brand red".into(),
            tint: 1.0,
        });
        boxed.stroke = Some(tessera_document::nodes::Stroke::new(Color::BLACK_INK, 2.0));
        let boxed = doc.add_frame(layer, boxed);

        // Turned 30° about its own centre.
        let b = rect(p0.x + 100.0, p0.y + 400.0, 120.0, 60.0);
        let mut oval = frame(b, FrameKind::Ellipse);
        oval.transform = Transform::rotate_about(
            30.0,
            tessera_geometry::DocPoint {
                x: b.x + b.width / 2.0,
                y: b.y + b.height / 2.0,
            },
        );
        oval.fill = Paint::Solid(Color::Rgb {
            r: 0.2,
            g: 0.4,
            b: 0.8,
            a: 1.0,
        });
        let oval = doc.add_frame(layer, oval);

        // A curve whose frame is the box it reaches, from its corner.
        let mut path = kurbo::BezPath::new();
        path.move_to((0.0, 0.0));
        path.curve_to((20.0, -10.0), (40.0, 10.0), (60.0, 0.0));
        let reach = kurbo::Shape::bounding_box(&path);
        path.apply_affine(kurbo::Affine::translate((-reach.x0, -reach.y0)));
        let mut line = frame(
            rect(p1.x + 50.0, p1.y + 300.0, reach.width(), reach.height()),
            FrameKind::Path(path),
        );
        line.stroke = Some(tessera_document::nodes::Stroke::new(Color::BLACK_INK, 1.0));
        let line = doc.add_frame(layer, line);

        let link = doc.add_link(tessera_document::links::Link::new(
            "/pictures/a photo.jpg",
            (300.0, 200.0),
        ));
        let picture = doc.add_frame(
            layer,
            frame(
                rect(p1.x + 300.0, p1.y + 400.0, 150.0, 100.0),
                FrameKind::Graphic {
                    placed: Some(tessera_document::graphic::Placement {
                        link,
                        inner: Transform::scale_about(
                            0.5,
                            0.5,
                            tessera_geometry::DocPoint { x: 0.0, y: 0.0 },
                        ),
                    }),
                },
            ),
        );
        (doc, vec![one, two, boxed, oval, line, picture])
    }

    fn corners(doc: &Document, id: FrameId) -> Vec<(f64, f64)> {
        let mut c: Vec<(f64, f64)> = doc
            .frame(id)
            .expect("frame")
            .corners()
            .iter()
            .map(|p| ((p.x * 100.0).round() / 100.0, (p.y * 100.0).round() / 100.0))
            .collect();
        c.sort_by(|a, b| a.partial_cmp(b).expect("numbers"));
        c
    }

    #[test]
    fn a_document_written_as_idml_reads_back_as_itself() {
        let (doc, ids) = a_document();
        let written = write(&doc).expect("written");
        let back = crate::idml::import_bytes(written.bytes, std::path::Path::new("x.idml"))
            .expect("read back")
            .document;

        assert_eq!(back.page_ids().count(), 2);
        let size = |d: &Document| {
            let p = d.pages[d.page_ids().next().expect("page")].bounds;
            (p.width, p.height)
        };
        assert_eq!(size(&back), size(&doc));

        // Every object, where it was, turned as it was.
        let mut want: Vec<Vec<(f64, f64)>> = ids.iter().map(|id| corners(&doc, *id)).collect();
        let mut got: Vec<Vec<(f64, f64)>> = back
            .top_level_order()
            .into_iter()
            .map(|id| corners(&back, id))
            .collect();
        want.sort_by(|a, b| a.partial_cmp(b).expect("numbers"));
        got.sort_by(|a, b| a.partial_cmp(b).expect("numbers"));
        assert_eq!(got, want);

        // The story, its styles, and the thread.
        let texts: Vec<&Frame> = back
            .frames
            .values()
            .filter(|f| matches!(f.kind, FrameKind::Text { .. }))
            .collect();
        assert_eq!(texts.len(), 2);
        let FrameKind::Text { story, layout } = &texts[0].kind else {
            unreachable!()
        };
        let story = back.story(*story).expect("story");
        let page = Marker::PageNumber.character();
        assert_eq!(
            story.text,
            format!("A heading\nSome body with emphasis & more, page {page}.")
        );
        let names: Vec<&str> = back
            .paragraph_styles
            .values()
            .map(|s| s.name.as_str())
            .collect();
        assert!(
            names.contains(&"Body") && names.contains(&"Heading"),
            "{names:?}"
        );
        let heading = back
            .paragraph_styles
            .values()
            .find(|s| s.name == "Heading")
            .expect("heading");
        assert_eq!(heading.format.character.size, Some(18.0));
        assert_eq!(heading.format.character.weight, Some(700));
        assert_eq!(heading.format.alignment, Some(Alignment::Centre));
        let based = heading.based_on.and_then(|p| back.paragraph_styles.get(p));
        assert_eq!(based.map(|s| s.name.as_str()), Some("Body"));
        let body = back
            .paragraph_styles
            .values()
            .find(|s| s.name == "Body")
            .expect("body");
        let lh = body.format.character.line_height.expect("leading");
        assert!((lh - 1.3).abs() < 1e-4, "leading as a multiple: {lh}");
        let word = story.text.find("emphasis").expect("there");
        let run = story
            .runs
            .iter()
            .find(|r| r.range.contains(&word))
            .expect("run");
        let style = run.style.and_then(|s| back.character_styles.get(s));
        assert_eq!(style.map(|s| s.name.as_str()), Some("Emphasis"));
        assert_eq!(style.and_then(|s| s.format.italic), Some(true));
        assert_eq!(layout.columns, 2);
        let threaded = back
            .frames
            .keys()
            .filter(|f| back.next_in_thread(*f).is_some())
            .count();
        assert_eq!(threaded, 1, "one frame threads into the other");

        // The swatch, named, and the unnamed colour not made one.
        let swatches: Vec<&str> = back.swatches.iter().map(|s| s.name.as_str()).collect();
        assert!(swatches.contains(&"Brand red"), "{swatches:?}");
        assert_eq!(
            back.swatch("Brand red").and_then(|s| s.group.as_deref()),
            Some("Brand"),
            "its colour group came with it"
        );
        assert!(
            !swatches.iter().any(|s| s.starts_with('u')),
            "an object's own colour is not a swatch: {swatches:?}"
        );

        // The picture, linked where it was.
        let link = back.links.values().next().expect("a link");
        assert_eq!(link.path, std::path::PathBuf::from("/pictures/a photo.jpg"));
    }

    #[test]
    fn every_part_of_the_package_is_well_formed_and_the_mimetype_comes_first() {
        let (mut doc, _) = a_document();
        let master = doc.add_master("A-Parent");
        let first = doc.page_ids().next().expect("page");
        doc.apply_master(first, Some(master));
        let written = write(&doc).expect("written");
        let mut zip =
            zip::ZipArchive::new(std::io::Cursor::new(written.bytes.clone())).expect("a zip");
        assert_eq!(zip.by_index(0).expect("first").name(), "mimetype");
        let names: Vec<String> = zip.file_names().map(str::to_owned).collect();
        assert!(names.iter().any(|n| n.starts_with("MasterSpreads/")));
        for name in names.iter().filter(|n| n.ends_with(".xml")) {
            let mut text = String::new();
            std::io::Read::read_to_string(&mut zip.by_name(name).expect("entry"), &mut text)
                .expect("text");
            roxmltree::Document::parse(&text)
                .unwrap_or_else(|e| panic!("{name} is not well-formed: {e}"));
        }
        let back = crate::idml::import_bytes(written.bytes, std::path::Path::new("x.idml"))
            .expect("read back")
            .document;
        let parents: Vec<String> = back
            .master_ids()
            .filter_map(|m| back.masters.get(m).map(|m| m.name.clone()))
            .collect();
        assert!(parents.iter().any(|n| n == "A-Parent"), "{parents:?}");
        let first = back.page_ids().next().expect("page");
        assert!(
            back.pages[first].master.is_some(),
            "the page keeps its parent"
        );
    }

    #[test]
    fn what_cannot_be_written_is_said() {
        let (mut doc, _) = a_document();
        let layer = doc.default_layer().expect("layer");
        let table = tessera_document::table::new(2, 2, 100.0, || doc.add_story(Story::default()));
        doc.add_frame(
            layer,
            frame(rect(0.0, 0.0, 100.0, 40.0), FrameKind::Table(table)),
        );
        let written = write(&doc).expect("written");
        assert!(written.dropped.0.iter().any(|d| d.contains("tables")));
    }
}
