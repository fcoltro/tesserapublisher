//! Styles as CSS: a class for each paragraph and character style, and a
//! `style` attribute for what was set by hand.

use std::collections::{HashMap, HashSet};

use tessera_color::Color;
use tessera_document::document::Document;
use tessera_text::story::{
    Alignment, Case, CharacterFormat, CharacterStyleId, ParagraphFormat, ParagraphStyleId, Styles,
};

/// The class each style goes out as.
pub struct Classes {
    pub paragraph: HashMap<ParagraphStyleId, String>,
    pub character: HashMap<CharacterStyleId, String>,
}

impl Classes {
    /// A class for every style, named after it: "Body text" is `p-body-text`,
    /// a character style `c-…`; two styles whose names come out alike are
    /// told apart by a number.
    pub fn of(doc: &Document) -> Self {
        let mut taken = HashSet::new();
        let mut name = |prefix: &str, style: &str| {
            let base = format!("{prefix}-{}", slug(style));
            let mut name = base.clone();
            let mut n = 2;
            while !taken.insert(name.clone()) {
                name = format!("{base}-{n}");
                n += 1;
            }
            name
        };
        let paragraph = doc
            .paragraph_styles
            .iter()
            .map(|(id, s)| (id, name("p", &s.name)))
            .collect();
        let character = doc
            .character_styles
            .iter()
            .map(|(id, s)| (id, name("c", &s.name)))
            .collect();
        Self {
            paragraph,
            character,
        }
    }

    /// The stylesheet: the document's own defaults on the body, then every
    /// style with all it says, its based-on chain folded in.
    pub fn stylesheet(&self, doc: &Document) -> String {
        let mut out = String::new();
        let base = declarations(doc, &doc.document_default());
        out.push_str(&rule("body", &base));
        out.push_str(&rule("p, h1, h2, h3, li", &["margin: 0".to_owned()]));
        let mut paragraphs: Vec<_> = self.paragraph.iter().collect();
        paragraphs.sort_by(|a, b| a.1.cmp(b.1));
        for (id, class) in paragraphs {
            let format = doc.paragraph_chain(*id);
            let mut all = paragraph_declarations(doc, &format);
            all.extend(declarations(doc, &format.character));
            out.push_str(&class_rule(class, &all));
        }
        let mut characters: Vec<_> = self.character.iter().collect();
        characters.sort_by(|a, b| a.1.cmp(b.1));
        for (id, class) in characters {
            let format = doc.character_chain(*id);
            out.push_str(&class_rule(class, &declarations(doc, &format)));
        }
        out.push_str(&rule(".table", &["border-collapse: collapse".to_owned()]));
        out.push_str(&rule(
            ".footnotes",
            &[
                "margin-top: 1.5em".to_owned(),
                "font-size: 0.85em".to_owned(),
            ],
        ));
        out
    }
}

/// A style's class, written even when the style says nothing of its own:
/// the class is on the words, and the stylesheet is where a person edits it.
fn class_rule(class: &str, declarations: &[String]) -> String {
    if declarations.is_empty() {
        return format!(".{class} {{\n}}\n");
    }
    rule(&format!(".{class}"), declarations)
}

fn rule(selector: &str, declarations: &[String]) -> String {
    if declarations.is_empty() {
        return String::new();
    }
    format!("{selector} {{\n  {};\n}}\n", declarations.join(";\n  "))
}

/// A name as a class: lower case, letters and digits, runs of anything else
/// a single hyphen.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for c in name.chars() {
        if c.is_alphanumeric() {
            if gap && !out.is_empty() {
                out.push('-');
            }
            out.extend(c.to_lowercase());
            gap = false;
        } else {
            gap = true;
        }
    }
    if out.is_empty() {
        "style".to_owned()
    } else {
        out
    }
}

/// A colour as CSS, through the document's swatches.
pub fn colour(doc: &Document, colour: &Color) -> String {
    let [r, g, b, a] = doc.resolve_colour(colour).to_rgb_f32();
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    if a >= 1.0 {
        format!("#{:02x}{:02x}{:02x}", byte(r), byte(g), byte(b))
    } else {
        format!(
            "rgba({}, {}, {}, {:.3})",
            byte(r),
            byte(g),
            byte(b),
            a.clamp(0.0, 1.0)
        )
    }
}

/// What a character format says, as CSS declarations; what it leaves
/// unsaid, nothing.
pub fn declarations(doc: &Document, f: &CharacterFormat) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(family) = &f.family {
        // A generic family is a keyword, not a name: quoted, a browser
        // looks for a font called "serif" and falls back to its default.
        const GENERIC: [&str; 6] = [
            "serif",
            "sans-serif",
            "monospace",
            "cursive",
            "fantasy",
            "system-ui",
        ];
        if GENERIC.contains(&family.trim()) {
            out.push(format!("font-family: {}", family.trim()));
        } else {
            out.push(format!("font-family: \"{}\"", family.replace('"', "")));
        }
    }
    if let Some(size) = f.size {
        out.push(format!("font-size: {}pt", number(size)));
    }
    if let Some(weight) = f.weight {
        out.push(format!("font-weight: {weight}"));
    }
    if let Some(italic) = f.italic {
        out.push(format!(
            "font-style: {}",
            if italic { "italic" } else { "normal" }
        ));
    }
    if let Some(tracking) = f.tracking {
        // Thousandths of an em.
        out.push(format!("letter-spacing: {}em", number(tracking / 1000.0)));
    }
    match f.case {
        Some(Case::Upper) => out.push("text-transform: uppercase".to_owned()),
        Some(Case::Lower) => out.push("text-transform: lowercase".to_owned()),
        Some(Case::SmallCaps) => out.push("font-variant-caps: small-caps".to_owned()),
        Some(Case::Normal) => out.push("text-transform: none".to_owned()),
        None => {}
    }
    if let Some(shift) = f.baseline_shift
        && shift != 0.0
    {
        out.push(format!("vertical-align: {}pt", number(shift)));
    }
    if let Some(leading) = f.line_height {
        // A multiple of the type size, which is what CSS's bare number is.
        out.push(format!("line-height: {}", number(leading)));
    }
    if let Some(c) = &f.colour {
        out.push(format!("color: {}", colour(doc, c)));
    }
    let under = f.underline.as_ref().is_some_and(|d| d.on);
    let through = f.strikethrough.as_ref().is_some_and(|d| d.on);
    if f.underline.is_some() || f.strikethrough.is_some() {
        let lines: Vec<&str> = [(under, "underline"), (through, "line-through")]
            .into_iter()
            .filter(|(on, _)| *on)
            .map(|(_, l)| l)
            .collect();
        out.push(format!(
            "text-decoration: {}",
            if lines.is_empty() {
                "none".to_owned()
            } else {
                lines.join(" ")
            }
        ));
    }
    out
}

/// What a paragraph format says about the paragraph, as CSS.
pub fn paragraph_declarations(_doc: &Document, f: &ParagraphFormat) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(align) = f.alignment {
        out.push(format!(
            "text-align: {}",
            match align {
                Alignment::Left => "left",
                Alignment::Centre => "center",
                Alignment::Right => "right",
                Alignment::Justify => "justify",
            }
        ));
    }
    for (value, property) in [
        (f.indent_left, "margin-left"),
        (f.indent_right, "margin-right"),
        (f.indent_first, "text-indent"),
        (f.space_before, "margin-top"),
        (f.space_after, "margin-bottom"),
    ] {
        if let Some(v) = value {
            out.push(format!("{property}: {}pt", number(v)));
        }
    }
    if f.hyphenate == Some(true) {
        out.push("hyphens: auto".to_owned());
    }
    out
}

/// A number as CSS writes it: no trailing zeros, no "-0".
pub fn number(v: f32) -> String {
    let s = format!("{:.3}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".to_owned()
    } else {
        s.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_style_name_becomes_a_class_name() {
        assert_eq!(slug("Body text"), "body-text");
        assert_eq!(slug("  Heading 1 — main "), "heading-1-main");
        assert_eq!(slug("---"), "style");
    }

    #[test]
    fn a_character_format_says_what_it_sets() {
        let doc = Document::new();
        let f = CharacterFormat {
            family: Some("Minion Pro".into()),
            size: Some(10.5),
            weight: Some(700),
            italic: Some(true),
            tracking: Some(50.0),
            ..Default::default()
        };
        assert_eq!(
            declarations(&doc, &f),
            vec![
                "font-family: \"Minion Pro\"",
                "font-size: 10.5pt",
                "font-weight: 700",
                "font-style: italic",
                "letter-spacing: 0.05em",
            ]
        );
        assert!(declarations(&doc, &CharacterFormat::default()).is_empty());
    }
}
