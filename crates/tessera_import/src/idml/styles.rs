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

use crate::xml::{attr, attr_f32, child, children, numbers, property};

/// Every colour the package defines, by its `Self`.
#[derive(Debug, Default)]
pub(crate) struct Colours {
    by_name: HashMap<String, Color>,
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
        Self { by_name }
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
            let id = doc.add_paragraph_style(ParagraphStyle {
                name: shown_name(attr(node, "Name").unwrap_or(name)),
                based_on: None,
                format: paragraph_format(node, colours),
            });
            out.paragraph.insert(name.to_owned(), id);
            if let Some(parent) = property(node, "BasedOn") {
                paragraph_parents.push((id, parent.to_owned()));
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
