//! Colours and styles: `Resources/Graphic.xml` and `Resources/Styles.xml`.
//!
//! IDML names a colour by its `Self` — `Color/C=100 M=0 Y=0 K=0` — and a
//! style the same way, and everything else in the package points at those
//! names. So both are read first, into maps from the name to what Tessera
//! made of it, and every later reference is one lookup.

use std::collections::HashMap;

use roxmltree::Node;
use tessera_color::Color;
use tessera_document::document::Document;
use tessera_text::story::{
    Alignment, Case, CharacterFormat, CharacterStyle, CharacterStyleId, Decoration, KeepOptions,
    KeepTogether, Kerning, ListFormat, ListKind, ParagraphFormat, ParagraphStyle, ParagraphStyleId,
    TabAlignment, TabStop,
};

use crate::xml::{attr, attr_f32, attr_f64, child, children, numbers, property};

/// Every colour the package defines, by its `Self` — and every gradient,
/// which a fill may name in the colour's place.
#[derive(Debug, Default)]
pub(crate) struct Colours {
    by_name: HashMap<String, Color>,
    gradients: HashMap<String, tessera_document::paint::Gradient>,
}

impl Colours {
    pub(crate) fn read(graphic: Node) -> Self {
        let mut by_name = HashMap::new();
        for node in graphic
            .descendants()
            .filter(|n| n.tag_name().name() == "Color")
        {
            let (Some(name), Some(space), Some(values)) = (
                attr(node, "Self"),
                attr(node, "Space"),
                attr(node, "ColorValue").map(numbers),
            ) else {
                continue;
            };
            let colour = match (space, values.as_slice()) {
                ("CMYK", [c, m, y, k]) => Color::Cmyk {
                    c: (*c / 100.0) as f32,
                    m: (*m / 100.0) as f32,
                    y: (*y / 100.0) as f32,
                    k: (*k / 100.0) as f32,
                    a: 1.0,
                },
                ("RGB", [r, g, b]) => Color::Rgb {
                    r: (*r / 255.0) as f32,
                    g: (*g / 255.0) as f32,
                    b: (*b / 255.0) as f32,
                    a: 1.0,
                },
                ("LAB", [l, a, b]) => Color::Lab {
                    l: *l as f32,
                    a: *a as f32,
                    b: *b as f32,
                    alpha: 1.0,
                },
                _ => continue,
            };
            by_name.insert(name.to_owned(), colour);
        }
        // Gradients after the colours, since their stops name colours. The
        // angle is not the gradient's: InDesign puts it on the object that
        // is filled, as Tessera's ramp does, so it is read there.
        let mut gradients = HashMap::new();
        for node in graphic
            .descendants()
            .filter(|n| n.tag_name().name() == "Gradient")
        {
            let Some(name) = attr(node, "Self") else {
                continue;
            };
            let ramp = match attr(node, "Type") {
                Some("Radial") => tessera_document::paint::Ramp::Radial,
                _ => tessera_document::paint::Ramp::Linear { angle: 0.0 },
            };
            let stops: Vec<tessera_document::paint::Stop> = node
                .children()
                .filter(|n| n.is_element() && n.tag_name().name() == "GradientStop")
                .filter_map(|stop| {
                    let colour = by_name.get(attr(stop, "StopColor")?)?.clone();
                    let at = (attr_f64(stop, "Location").unwrap_or(0.0) / 100.0).clamp(0.0, 1.0);
                    Some(tessera_document::paint::Stop {
                        at: at as f32,
                        colour,
                    })
                })
                .collect();
            if stops.is_empty() {
                continue;
            }
            gradients.insert(
                name.to_owned(),
                tessera_document::paint::Gradient::new(ramp, stops),
            );
        }
        Self { by_name, gradients }
    }

    /// What a fill reference paints: a colour, tinted as asked, or a
    /// gradient turned to `angle` — InDesign's `GradientFillAngle`, counter-
    /// clockwise in a y-up world, so Tessera's clockwise ramp takes its
    /// negative. `None` for `Swatch/None` and anything unknown.
    pub(crate) fn paint(
        &self,
        reference: Option<&str>,
        tint: Option<f64>,
        angle: Option<f64>,
    ) -> Option<tessera_document::paint::Paint> {
        let reference = reference?;
        if let Some(gradient) = self.gradients.get(reference) {
            let mut gradient = gradient.clone();
            if let (tessera_document::paint::Ramp::Linear { .. }, Some(angle)) =
                (gradient.ramp, angle)
            {
                gradient.ramp = tessera_document::paint::Ramp::Linear { angle: -angle };
            }
            return Some(tessera_document::paint::Paint::Gradient(gradient));
        }
        let colour = self.by_name.get(reference)?.clone();
        Some(tessera_document::paint::Paint::Solid(super::tinted(
            colour, tint,
        )))
    }

    /// What a `FillColor="Color/..."` reference means. `None` for
    /// `Swatch/None` and anything unknown, which draws nothing — the honest
    /// reading of a colour that is not there.
    pub(crate) fn get(&self, reference: Option<&str>) -> Option<Color> {
        let reference = reference?;
        self.by_name.get(reference).cloned()
    }

    /// The name a swatch panel would show: the part after `Color/`.
    pub(crate) fn swatches(&self) -> impl Iterator<Item = (String, Color)> + '_ {
        self.by_name.iter().filter_map(|(name, colour)| {
            let shown = name.strip_prefix("Color/")?;
            if shown.starts_with("$ID/") {
                return None; // InDesign's own: Registration, Paper, None
            }
            Some((shown.to_owned(), colour.clone()))
        })
    }
}

/// The package's styles, added to the document, by their `Self`.
#[derive(Debug, Default)]
pub(crate) struct Styles {
    pub(crate) paragraph: HashMap<String, ParagraphStyleId>,
    pub(crate) character: HashMap<String, CharacterStyleId>,
    pub(crate) object: HashMap<String, tessera_document::ids::ObjectStyleId>,
}

/// An object's effects as IDML writes them, in a `TransparencySetting`
/// child: opacity and blend mode, a drop shadow, and a gradient feather.
/// Absent, the object is plain, casts none and does not fade.
pub(crate) fn effects(
    node: Node,
    colours: &Colours,
) -> (
    tessera_document::blending::Blending,
    Option<tessera_document::shadow::Shadow>,
    Option<tessera_document::feather::GradientFeather>,
) {
    use tessera_document::blending::{BlendMode, Blending};
    use tessera_document::shadow::Shadow;
    let Some(setting) = child(node, "TransparencySetting") else {
        return (Blending::PLAIN, None, None);
    };
    let mut blend = Blending::PLAIN;
    if let Some(blending) = child(setting, "BlendingSetting") {
        blend.opacity =
            (attr_f64(blending, "Opacity").unwrap_or(100.0) / 100.0).clamp(0.0, 1.0) as f32;
        blend.mode = match attr(blending, "BlendMode") {
            Some("Multiply") => BlendMode::Multiply,
            Some("Screen") => BlendMode::Screen,
            Some("Overlay") => BlendMode::Overlay,
            // The rest InDesign has — darken, lighten, hue and so on — are
            // not modelled; painted over rather than lost, since the object
            // is worth more than its mode.
            _ => BlendMode::Normal,
        };
    }
    let shadow = child(setting, "DropShadowSetting")
        .filter(|s| attr(*s, "Mode") == Some("Drop"))
        .map(|s| {
            let opacity = (attr_f64(s, "Opacity").unwrap_or(75.0) / 100.0).clamp(0.0, 1.0) as f32;
            // InDesign's own default effect colour is [Black], the black
            // plate alone.
            let colour = colours
                .get(attr(s, "EffectColor"))
                .unwrap_or(Color::BLACK_INK);
            let colour = match colour {
                Color::Rgb { r, g, b, .. } => Color::Rgb {
                    r,
                    g,
                    b,
                    a: opacity,
                },
                Color::Cmyk { c, m, y, k, .. } => Color::Cmyk {
                    c,
                    m,
                    y,
                    k,
                    a: opacity,
                },
                Color::Lab { l, a, b, .. } => Color::Lab {
                    l,
                    a,
                    b,
                    alpha: opacity,
                },
                other => other,
            };
            Shadow {
                offset: (
                    attr_f64(s, "XOffset").unwrap_or(Shadow::TYPICAL.offset.0),
                    attr_f64(s, "YOffset").unwrap_or(Shadow::TYPICAL.offset.1),
                ),
                // InDesign's Size is the blur's reach; near enough its sigma.
                blur: attr_f64(s, "Size").unwrap_or(Shadow::TYPICAL.blur),
                colour,
            }
        });
    (blend, shadow, feather(setting))
}

/// A `GradientFeatherSetting` that is applied: its ramp, and its
/// `OpacityGradientStop`s as percentages along it. InDesign's ramp has a
/// start point and a length as well as an angle; the angle is kept and the
/// ramp runs across the whole object, as a gradient fill's does here.
fn feather(setting: Node) -> Option<tessera_document::feather::GradientFeather> {
    use tessera_document::feather::{FeatherStop, GradientFeather};
    use tessera_document::paint::Ramp;
    let s =
        child(setting, "GradientFeatherSetting").filter(|s| attr(*s, "Applied") == Some("true"))?;
    let ramp = match attr(s, "Type") {
        Some("Radial") => Ramp::Radial,
        _ => Ramp::Linear {
            // InDesign measures counter-clockwise; this model clockwise,
            // because its y axis points down.
            angle: -attr_f64(s, "Angle").unwrap_or(0.0),
        },
    };
    let stops = s
        .descendants()
        .filter(|n| n.tag_name().name() == "OpacityGradientStop")
        .map(|n| FeatherStop {
            at: (attr_f64(n, "Location").unwrap_or(0.0) / 100.0).clamp(0.0, 1.0) as f32,
            opacity: (attr_f64(n, "Opacity").unwrap_or(100.0) / 100.0).clamp(0.0, 1.0) as f32,
        })
        .collect();
    Some(GradientFeather::new(ramp, stops))
}

impl Styles {
    /// Read every style into `doc`. Two passes, because a style may be based
    /// on one declared after it.
    pub(crate) fn read(styles: Node, doc: &mut Document, colours: &Colours) -> Self {
        let mut out = Self::default();
        let mut paragraph_parents: Vec<(ParagraphStyleId, String)> = Vec::new();
        let mut character_parents: Vec<(CharacterStyleId, String)> = Vec::new();

        for node in styles
            .descendants()
            .filter(|n| n.tag_name().name() == "CharacterStyle")
        {
            let Some(name) = attr(node, "Self") else {
                continue;
            };
            if is_root(name) {
                continue;
            }
            let id = doc.add_character_style(CharacterStyle {
                name: shown_name(attr(node, "Name").unwrap_or(name)),
                based_on: None,
                format: character_format(node, colours, None),
            });
            out.character.insert(name.to_owned(), id);
            if let Some(parent) = property(node, "BasedOn") {
                character_parents.push((id, parent.to_owned()));
            }
        }
        for node in styles
            .descendants()
            .filter(|n| n.tag_name().name() == "ParagraphStyle")
        {
            let Some(name) = attr(node, "Self") else {
                continue;
            };
            if is_root(name) {
                continue;
            }
            let mut format = paragraph_format(node, colours);
            automatic_rules(node, &out.character, &mut format);
            let id = doc.add_paragraph_style(ParagraphStyle {
                name: shown_name(attr(node, "Name").unwrap_or(name)),
                based_on: None,
                format,
            });
            out.paragraph.insert(name.to_owned(), id);
            if let Some(parent) = property(node, "BasedOn") {
                paragraph_parents.push((id, parent.to_owned()));
            }
        }

        // Object styles: what the style states of fill, stroke and effects.
        // InDesign's `[None]` and `[Normal ...]` roots are the document's own
        // defaults and are not added.
        let mut object_parents: Vec<(tessera_document::ids::ObjectStyleId, String)> = Vec::new();
        for node in styles
            .descendants()
            .filter(|n| n.tag_name().name() == "ObjectStyle")
        {
            let Some(name) = attr(node, "Self") else {
                continue;
            };
            if is_root(name) {
                continue;
            }
            let fill = colours.paint(
                attr(node, "FillColor"),
                attr_f64(node, "FillTint"),
                attr_f64(node, "GradientFillAngle"),
            );
            let stroke = match (
                colours.get(attr(node, "StrokeColor")),
                attr_f64(node, "StrokeWeight"),
            ) {
                (Some(colour), Some(weight)) if weight > 0.0 => {
                    Some(Some(tessera_document::nodes::Stroke::new(
                        super::tinted(colour, attr_f64(node, "StrokeTint")),
                        weight,
                    )))
                }
                (None, _) if attr(node, "StrokeColor").is_some() => Some(None),
                _ => None,
            };
            let (blend, shadow, feather) = effects(node, colours);
            let states_effects = child(node, "TransparencySetting").is_some();
            let id = doc.add_object_style(tessera_document::object_style::ObjectStyle {
                name: shown_name(attr(node, "Name").unwrap_or(name)),
                based_on: None,
                format: tessera_document::object_style::ObjectFormat {
                    fill,
                    stroke,
                    blend: states_effects.then_some(blend),
                    shadow: states_effects.then_some(shadow),
                    feather: states_effects.then_some(feather),
                    wrap: None,
                },
            });
            out.object.insert(name.to_owned(), id);
            if let Some(parent) = property(node, "BasedOn") {
                object_parents.push((id, parent.to_owned()));
            }
        }
        for (id, parent) in object_parents {
            if let (Some(parent), Some(style)) =
                (out.object.get(&parent), doc.object_styles.get_mut(id))
            {
                style.based_on = Some(*parent);
            }
        }

        for (id, parent) in character_parents {
            if let (Some(parent), Some(style)) =
                (out.character.get(&parent), doc.character_styles.get_mut(id))
            {
                style.based_on = Some(*parent);
            }
        }
        for (id, parent) in paragraph_parents {
            if let (Some(parent), Some(style)) =
                (out.paragraph.get(&parent), doc.paragraph_styles.get_mut(id))
            {
                style.based_on = Some(*parent);
            }
        }
        doc.touch();
        out
    }
}

/// InDesign's own roots — `[No paragraph style]`, `[Basic Paragraph]`,
/// `[No character style]` — which are Tessera's document default and not
/// styles to add.
fn is_root(name: &str) -> bool {
    name.contains("$ID/")
}

fn shown_name(name: &str) -> String {
    name.strip_prefix("$ID/").unwrap_or(name).to_owned()
}

/// Character attributes as IDML writes them on a style, a range, or a
/// paragraph style's character part.
///
/// `size` is what the range would inherit, when known, so a leading in points
/// can become a multiple of it.
#[allow(clippy::field_reassign_with_default)]
pub(crate) fn character_format(
    node: Node,
    colours: &Colours,
    size: Option<f32>,
) -> CharacterFormat {
    let mut f = CharacterFormat::default();
    f.family = property(node, "AppliedFont").map(str::to_owned);
    f.size = attr_f32(node, "PointSize");
    if let Some(style) = attr(node, "FontStyle") {
        let lower = style.to_ascii_lowercase();
        if lower.contains("bold") || lower.contains("black") || lower.contains("heavy") {
            f.weight = Some(700);
        } else if lower.contains("semibold") || lower.contains("medium") {
            f.weight = Some(600);
        } else if lower.contains("light") {
            f.weight = Some(300);
        } else if lower == "regular" || lower == "roman" {
            f.weight = Some(400);
        }
        if lower.contains("italic") || lower.contains("oblique") {
            f.italic = Some(true);
        } else if lower == "regular" || lower == "roman" {
            f.italic = Some(false);
        }
    }
    f.tracking = attr_f32(node, "Tracking");
    // InDesign's three: Metrics, Optical, and "$ID/manual", which is metrics
    // with the pairs kerned by hand — and the hand kerns come with the runs.
    f.kerning = match attr(node, "KerningMethod") {
        Some("Optical") => Some(Kerning::Optical),
        Some("Metrics") | Some("$ID/manual") => Some(Kerning::Metrics),
        _ => None,
    };
    f.baseline_shift = attr_f32(node, "BaselineShift");
    f.case = match attr(node, "Capitalization") {
        Some("AllCaps") => Some(Case::Upper),
        Some("SmallCaps") | Some("CapToSmallCap") => Some(Case::SmallCaps),
        Some("Normal") => Some(Case::Normal),
        _ => None,
    };
    if attr(node, "Underline") == Some("true") {
        f.underline = Some(Decoration::default());
    }
    if attr(node, "StrikeThru") == Some("true") {
        f.strikethrough = Some(Decoration::default());
    }
    if let Some(colour) = colours.get(attr(node, "FillColor")) {
        f.colour = Some(colour);
    }
    if let Some(leading) = property(node, "Leading").and_then(|l| l.trim().parse::<f32>().ok()) {
        let base = f.size.or(size).unwrap_or(12.0);
        if base > 0.0 {
            f.line_height = Some(leading / base);
        }
    }
    match attr(node, "Ligatures") {
        Some("true") => f.ligatures = Some(true),
        Some("false") => f.ligatures = Some(false),
        _ => {}
    }
    if let Some(language) = attr(node, "AppliedLanguage") {
        f.language = language_code(language);
    }
    f
}

/// "$ID/English: USA" → "en", "$ID/German: Reformed" → "de".
fn language_code(applied: &str) -> Option<String> {
    let name = applied.strip_prefix("$ID/").unwrap_or(applied);
    let name = name.split(':').next()?.trim().to_ascii_lowercase();
    let code = match name.as_str() {
        "english" | "english uk" | "english usa" => "en",
        "german" => "de",
        "french" => "fr",
        "spanish" => "es",
        "italian" => "it",
        "portuguese" => "pt",
        "dutch" => "nl",
        "swedish" => "sv",
        "danish" => "da",
        "norwegian" => "nb",
        "finnish" => "fi",
        "polish" => "pl",
        "czech" => "cs",
        "hungarian" => "hu",
        "russian" => "ru",
        "turkish" => "tr",
        "catalan" => "ca",
        "greek" => "el",
        _ => return None,
    };
    Some(code.to_owned())
}

/// Paragraph attributes, with the character part folded in.
#[allow(clippy::field_reassign_with_default)]
pub(crate) fn paragraph_format(node: Node, colours: &Colours) -> ParagraphFormat {
    let mut f = ParagraphFormat::default();
    f.alignment = match attr(node, "Justification") {
        Some("LeftAlign") => Some(Alignment::Left),
        Some("CenterAlign") => Some(Alignment::Centre),
        Some("RightAlign") => Some(Alignment::Right),
        Some("LeftJustified")
        | Some("RightJustified")
        | Some("CenterJustified")
        | Some("FullyJustified") => Some(Alignment::Justify),
        _ => None,
    };
    f.indent_left = attr_f32(node, "LeftIndent");
    f.indent_right = attr_f32(node, "RightIndent");
    f.indent_first = attr_f32(node, "FirstLineIndent");
    f.space_before = attr_f32(node, "SpaceBefore");
    f.space_after = attr_f32(node, "SpaceAfter");
    f.hyphenate = match attr(node, "Hyphenation") {
        Some("true") => Some(true),
        Some("false") => Some(false),
        _ => None,
    };
    f.drop_cap_lines = attr_f32(node, "DropCapLines")
        .map(|n| n as u8)
        .filter(|n| *n > 1);
    f.drop_cap_characters = attr_f32(node, "DropCapCharacters").map(|n| n as u8);
    f.column_span = column_span(node);

    let with_next = attr_f32(node, "KeepWithNext").is_some_and(|n| n > 0.0);
    let together = if attr(node, "KeepLinesTogether") == Some("true") {
        if attr(node, "KeepAllLinesTogether") == Some("true") {
            KeepTogether::All
        } else {
            KeepTogether::Ends {
                start: attr_f32(node, "KeepFirstLines").map_or(2, |n| n as u8),
                end: attr_f32(node, "KeepLastLines").map_or(2, |n| n as u8),
            }
        }
    } else {
        KeepTogether::Off
    };
    if with_next || together != KeepTogether::Off {
        f.keep = Some(KeepOptions {
            with_next,
            together,
        });
    }

    f.list = match attr(node, "BulletsAndNumberingListType") {
        Some("BulletList") => Some(ListFormat {
            kind: ListKind::Bullet,
            ..ListFormat::default()
        }),
        Some("NumberedList") => Some(ListFormat {
            kind: ListKind::Number,
            ..ListFormat::default()
        }),
        Some("NoList") => Some(ListFormat {
            kind: ListKind::None,
            ..ListFormat::default()
        }),
        _ => None,
    };

    f.tab_stops = tab_stops(node);
    f.character = character_format(node, colours, None);
    f
}

/// `<TabList type="list"><ListItem type="record">…` into tab stops.
fn tab_stops(node: Node) -> Option<Vec<TabStop>> {
    let properties = child(node, "Properties")?;
    let list = child(properties, "TabList")?;
    let stops: Vec<TabStop> = children(list, "ListItem")
        .filter_map(|item| {
            let position = child(item, "Position")?
                .text()?
                .trim()
                .parse::<f32>()
                .ok()?;
            let alignment = match child(item, "Alignment").and_then(|a| a.text()) {
                Some("CenterAlign") => TabAlignment::Centre,
                Some("RightAlign") => TabAlignment::Right,
                Some("CharacterAlign") => TabAlignment::Decimal,
                _ => TabAlignment::Left,
            };
            let leader = child(item, "Leader")
                .and_then(|l| l.text())
                .and_then(|l| l.chars().next());
            Some(TabStop {
                position,
                alignment,
                leader,
            })
        })
        .collect();
    if stops.is_empty() { None } else { Some(stops) }
}

/// Span Columns, as InDesign writes it on a paragraph or its style: across
/// so many columns ("All" or a number) or split into them, with the space
/// kept around it. Absent is unsaid; `SingleColumn` says one column.
fn column_span(node: Node) -> Option<tessera_text::story::ColumnSpan> {
    use tessera_text::story::ColumnSpan;
    let count = || match attr(node, "SpanSplitColumnCount") {
        Some("All") | None => 0,
        Some(n) => n.trim().parse::<u8>().unwrap_or(0),
    };
    let before = attr_f32(node, "SpanColumnMinSpaceBefore").unwrap_or(0.0);
    let after = attr_f32(node, "SpanColumnMinSpaceAfter").unwrap_or(0.0);
    match attr(node, "SpanColumnType")? {
        "SingleColumn" => Some(ColumnSpan::Single),
        "SpanColumns" => Some(ColumnSpan::Span {
            columns: count(),
            space_before: before,
            space_after: after,
        }),
        "SplitColumns" => Some(ColumnSpan::Split {
            columns: count().max(2),
            gutter: attr_f32(node, "SplitColumnInsideGutter").unwrap_or(12.0),
            space_before: before,
            space_after: after,
        }),
        _ => None,
    }
}

/// A paragraph style's nested, line and GREP styles, as InDesign lists them
/// in its properties, each naming its character style by `Self`. A rule
/// naming a style that was not read, or a delimiter this has no word for, is
/// left out rather than guessed.
fn automatic_rules(
    node: Node,
    characters: &HashMap<String, CharacterStyleId>,
    format: &mut ParagraphFormat,
) {
    use tessera_text::automatic::{Delimiter, GrepStyle, LineStyle, NestedStyle};
    let Some(properties) = child(node, "Properties") else {
        return;
    };
    let records = |list: &str| -> Vec<Node> {
        child(properties, list)
            .map(|l| children(l, "ListItem").collect())
            .unwrap_or_default()
    };
    let text = |item: Node, name: &str| -> Option<String> {
        child(item, name).and_then(|n| n.text()).map(str::to_owned)
    };
    // `[No character style]` is InDesign's [None].
    let style = |item: Node| -> Result<Option<CharacterStyleId>, ()> {
        match text(item, "AppliedCharacterStyle") {
            None => Ok(None),
            Some(name) if name.ends_with("[No character style]") => Ok(None),
            Some(name) => characters.get(&name).copied().map(Some).ok_or(()),
        }
    };
    let count = |item: Node, name: &str| -> u16 {
        text(item, name)
            .and_then(|t| t.trim().parse::<u16>().ok())
            .unwrap_or(1)
            .max(1)
    };

    let nested: Vec<NestedStyle> = records("AllNestedStyles")
        .into_iter()
        .filter_map(|item| {
            let delimiter = match text(item, "Delimiter")?.as_str() {
                "Sentence" => Delimiter::Sentences,
                "AnyWord" => Delimiter::Words,
                "AnyCharacter" => Delimiter::Characters,
                "Letters" => Delimiter::Letters,
                "Digits" => Delimiter::Digits,
                "Tabs" => Delimiter::Tabs,
                "EmSpace" => Delimiter::AnyOf("\u{2003}".to_owned()),
                "EnSpace" => Delimiter::AnyOf("\u{2002}".to_owned()),
                "NonbreakingSpace" => Delimiter::AnyOf("\u{00A0}".to_owned()),
                // A typed delimiter is the characters themselves; InDesign's
                // own names are longer than one character and begin capital.
                typed if typed.chars().count() <= 4 => Delimiter::AnyOf(typed.to_owned()),
                _ => return None,
            };
            Some(NestedStyle {
                style: style(item).ok()?,
                through: text(item, "Inclusive").as_deref() != Some("false"),
                count: count(item, "Repetition"),
                delimiter,
            })
        })
        .collect();
    let lines: Vec<LineStyle> = records("AllLineStyles")
        .into_iter()
        .filter_map(|item| {
            Some(LineStyle {
                style: style(item).ok()?,
                lines: count(item, "LineCount"),
            })
        })
        .collect();
    let grep: Vec<GrepStyle> = records("AllGREPStyles")
        .into_iter()
        .filter_map(|item| {
            Some(GrepStyle {
                style: style(item).ok()??,
                pattern: text(item, "GrepExpression")?,
            })
        })
        .collect();
    format.nested = (!nested.is_empty()).then_some(nested);
    format.line_styles = (!lines.is_empty()).then_some(lines);
    format.grep = (!grep.is_empty()).then_some(grep);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_and_split_columns_are_read_from_a_paragraph() {
        use tessera_text::story::ColumnSpan;
        let graphic = roxmltree::Document::parse("<Graphic/>").unwrap();
        let colours = Colours::read(graphic.root_element());
        let span = roxmltree::Document::parse(
            r#"<ParagraphStyleRange SpanColumnType="SpanColumns" SpanSplitColumnCount="All" SpanColumnMinSpaceAfter="9"/>"#,
        )
        .unwrap();
        assert_eq!(
            paragraph_format(span.root_element(), &colours).column_span,
            Some(ColumnSpan::Span {
                columns: 0,
                space_before: 0.0,
                space_after: 9.0
            })
        );
        let split = roxmltree::Document::parse(
            r#"<ParagraphStyleRange SpanColumnType="SplitColumns" SpanSplitColumnCount="3" SplitColumnInsideGutter="6"/>"#,
        )
        .unwrap();
        assert_eq!(
            paragraph_format(split.root_element(), &colours).column_span,
            Some(ColumnSpan::Split {
                columns: 3,
                gutter: 6.0,
                space_before: 0.0,
                space_after: 0.0
            })
        );
    }

    #[test]
    fn a_paragraph_style_s_nested_line_and_grep_styles_are_read() {
        use tessera_text::automatic::Delimiter;
        let mut doc = Document::new();
        let graphic = roxmltree::Document::parse("<Graphic/>").unwrap();
        let colours = Colours::read(graphic.root_element());
        let xml = r#"<Styles>
          <RootCharacterStyleGroup>
            <CharacterStyle Self="CharacterStyle/Bold" Name="Bold" FontStyle="Bold"/>
            <CharacterStyle Self="CharacterStyle/Caps" Name="Caps"/>
          </RootCharacterStyleGroup>
          <RootParagraphStyleGroup>
            <ParagraphStyle Self="ParagraphStyle/Body" Name="Body"><Properties>
              <AllNestedStyles type="list">
                <ListItem type="record">
                  <AppliedCharacterStyle type="object">CharacterStyle/Bold</AppliedCharacterStyle>
                  <Delimiter type="string">:</Delimiter>
                  <Repetition type="long">1</Repetition>
                  <Inclusive type="boolean">true</Inclusive>
                </ListItem>
                <ListItem type="record">
                  <AppliedCharacterStyle type="object">CharacterStyle/$ID/[No character style]</AppliedCharacterStyle>
                  <Delimiter type="enumeration">AnyWord</Delimiter>
                  <Repetition type="long">2</Repetition>
                  <Inclusive type="boolean">false</Inclusive>
                </ListItem>
              </AllNestedStyles>
              <AllLineStyles type="list">
                <ListItem type="record">
                  <AppliedCharacterStyle type="object">CharacterStyle/Caps</AppliedCharacterStyle>
                  <LineCount type="long">1</LineCount>
                </ListItem>
              </AllLineStyles>
              <AllGREPStyles type="list">
                <ListItem type="record">
                  <AppliedCharacterStyle type="object">CharacterStyle/Bold</AppliedCharacterStyle>
                  <GrepExpression type="string">\d+%</GrepExpression>
                </ListItem>
              </AllGREPStyles>
            </Properties></ParagraphStyle>
          </RootParagraphStyleGroup>
        </Styles>"#;
        let parsed = roxmltree::Document::parse(xml).unwrap();
        let styles = Styles::read(parsed.root_element(), &mut doc, &colours);
        let bold = styles.character["CharacterStyle/Bold"];
        let caps = styles.character["CharacterStyle/Caps"];
        let body = &doc.paragraph_styles[styles.paragraph["ParagraphStyle/Body"]].format;

        let nested = body.nested.as_ref().expect("nested");
        assert_eq!(nested.len(), 2);
        assert_eq!(nested[0].style, Some(bold));
        assert_eq!(nested[0].delimiter, Delimiter::AnyOf(":".into()));
        assert!(nested[0].through);
        assert_eq!(nested[1].style, None, "[No character style] is none");
        assert_eq!((nested[1].count, nested[1].through), (2, false));
        assert_eq!(nested[1].delimiter, Delimiter::Words);

        let lines = body.line_styles.as_ref().expect("lines");
        assert_eq!((lines[0].style, lines[0].lines), (Some(caps), 1));
        let grep = body.grep.as_ref().expect("grep");
        assert_eq!((grep[0].style, grep[0].pattern.as_str()), (bold, r"\d+%"));
    }

    #[test]
    fn an_applied_gradient_feather_is_read_with_its_stops() {
        use tessera_document::paint::Ramp;
        let graphic = roxmltree::Document::parse("<Graphic/>").unwrap();
        let colours = Colours::read(graphic.root_element());
        let item = roxmltree::Document::parse(
            r#"<Rectangle><TransparencySetting><GradientFeatherSetting Applied="true" Type="Linear" Angle="90"><OpacityGradientStop Opacity="100" Location="0"/><OpacityGradientStop Opacity="0" Location="80"/></GradientFeatherSetting></TransparencySetting></Rectangle>"#,
        )
        .unwrap();
        let (_, _, feather) = effects(item.root_element(), &colours);
        let feather = feather.expect("a feather");
        // Counter-clockwise in InDesign, clockwise here.
        assert_eq!(feather.ramp, Ramp::Linear { angle: -90.0 });
        let stops = feather.stops();
        assert_eq!((stops[0].at, stops[0].opacity), (0.0, 1.0));
        assert_eq!((stops[1].at, stops[1].opacity), (0.8, 0.0));
    }

    #[test]
    fn a_gradient_feather_not_applied_is_no_feather() {
        let graphic = roxmltree::Document::parse("<Graphic/>").unwrap();
        let colours = Colours::read(graphic.root_element());
        let item = roxmltree::Document::parse(
            r#"<Rectangle><TransparencySetting><GradientFeatherSetting Applied="false"/></TransparencySetting></Rectangle>"#,
        )
        .unwrap();
        assert!(effects(item.root_element(), &colours).2.is_none());
    }

    #[test]
    fn a_drop_shadow_with_no_colour_named_is_black_ink() {
        // InDesign's own default effect colour is [Black]: the black plate,
        // at the shadow's opacity.
        let graphic = roxmltree::Document::parse("<Graphic/>").unwrap();
        let colours = Colours::read(graphic.root_element());
        let item = roxmltree::Document::parse(
            r#"<Rectangle><TransparencySetting><DropShadowSetting Mode="Drop" Opacity="40"/></TransparencySetting></Rectangle>"#,
        )
        .unwrap();
        let (_, shadow, _) = effects(item.root_element(), &colours);
        assert_eq!(
            shadow.expect("a shadow").colour,
            Color::Cmyk {
                c: 0.0,
                m: 0.0,
                y: 0.0,
                k: 1.0,
                a: 0.4,
            }
        );
    }
}
