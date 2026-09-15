//! What the objects a command takes look like as JSON.
//!
//! The catalogue names a field's type — `TextLayout`, `Paint` — and this
//! shows it: an example value for each, serialised by the same code that
//! reads it back, so an example is right by construction and a model can
//! copy it and change what it means to. Enums are shown as every variant.
//!
//! Ids inside these objects (a style's `based_on`, a contents level's
//! `style`) appear as the document keeps them, `{"idx", "version"}`, which
//! `describe_document` reports alongside each number as `key`.

use serde_json::{Value, json};
use tessera_color::Color;
use tessera_document::nodes::{Axis, Guide, Insets, Stroke, Swatch, TextLayout, TextWrap, WrapTo};
use tessera_document::paint::Paint;

/// An example of every type a command's field may name.
pub fn all() -> Value {
    let mut out = serde_json::Map::new();
    let mut put = |name: &str, value: Value| {
        out.insert(name.to_owned(), value);
    };
    let ex = |v: &dyn erased::Ser| v.to_json();

    put(
        "Color",
        json!({
            "examples": [
                ex(&Color::Rgb { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }),
                ex(&Color::Cmyk { c: 0.0, m: 1.0, y: 1.0, k: 0.0, a: 1.0 }),
            ],
            "doc": "Channels 0 to 1. Rgb or Cmyk, each with an alpha `a`.",
        }),
    );
    put(
        "Paint",
        json!({
            "examples": [ex(&Paint::default()), ex(&Paint::Solid(Color::Rgb { r: 0.2, g: 0.4, b: 0.8, a: 1.0 }))],
            "doc": "What fills a shape: Solid(colour), or Gradient with a ramp (Linear {angle} or Radial) and stops.",
        }),
    );
    put(
        "Stroke",
        json!({
            "example": ex(&Stroke::new(Color::Rgb { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }, 1.0)),
            "doc": "align is Center, Inside or Outside; cap Butt, Round or Square; join Miter, Round or Bevel.",
        }),
    );
    put(
        "Insets",
        json!({ "example": ex(&Insets::default()), "doc": "Points, top/bottom/left/right." }),
    );
    put(
        "DocRect",
        json!({ "example": { "x": 0.0, "y": 0.0, "width": 100.0, "height": 50.0 }, "doc": "Points from the document's top-left." }),
    );
    put(
        "Transform",
        json!({ "example": [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], "doc": "Affine [a, b, c, d, e, f]; identity shown." }),
    );
    put(
        "Range<usize>",
        json!({ "example": { "start": 0, "end": 5 }, "doc": "Byte offsets into a story's text." }),
    );
    put(
        "TextLayout",
        json!({ "example": ex(&TextLayout::default()), "doc": "A text frame's columns, gutter, insets, vertical justification (Top, Centre, Bottom, Justify), grid lock, and next/previous for threading." }),
    );
    put(
        "TextWrap",
        json!({
            "examples": [
                ex(&TextWrap::None),
                ex(&TextWrap::Bounds { standoff: Insets::default(), sides: WrapTo::Largest }),
                ex(&TextWrap::Contour { standoff: 4.0, sides: WrapTo::Both }),
                ex(&TextWrap::Jump),
            ],
            "doc": "sides: Largest, Both, Left or Right.",
        }),
    );
    put(
        "ParagraphFormat",
        json!({
            "example": ex(&tessera_text::story::ParagraphFormat::default()),
            "doc": "Every field optional; only the ones given change. alignment: Left, Centre, Right, Justify. `character` carries the character formatting the paragraph imposes.",
        }),
    );
    put(
        "CharacterFormat",
        json!({
            "example": ex(&tessera_text::story::CharacterFormat::default()),
            "doc": "Every field optional. size in points, line_height a multiple of size, weight 100..900, tracking in 1/1000 em, language a code like \"en\".",
        }),
    );
    put(
        "ParagraphStyle",
        json!({ "example": ex(&tessera_text::story::ParagraphStyle::default()), "doc": "name, based_on (a key or null), format: ParagraphFormat." }),
    );
    put(
        "CharacterStyle",
        json!({ "example": ex(&tessera_text::story::CharacterStyle::default()) }),
    );
    put(
        "DocumentSetup",
        json!({ "example": ex(&tessera_document::nodes::DocumentSetup::default()) }),
    );
    put(
        "FootnoteOptions",
        json!({ "example": ex(&tessera_document::footnotes::FootnoteOptions::default()) }),
    );
    put(
        "Contents",
        json!({ "example": ex(&tessera_document::contents::Contents::default()), "doc": "A table-of-contents recipe: title, levels each naming a paragraph style key." }),
    );
    put(
        "Index",
        json!({ "example": ex(&tessera_document::contents::Index::default()) }),
    );
    put(
        "Corners",
        json!({ "example": ex(&tessera_document::corners::Corners::default()) }),
    );
    put(
        "ObjectFormat",
        json!({ "example": ex(&tessera_document::object_style::ObjectFormat::default()) }),
    );
    put(
        "Blending",
        json!({ "example": ex(&tessera_document::blending::Blending::default()), "doc": "opacity 0..1 and a blend mode." }),
    );
    put(
        "Shadow",
        json!({ "example": ex(&tessera_document::shadow::Shadow::default()) }),
    );
    put(
        "Swatch",
        json!({ "example": ex(&Swatch::new("Brand red", Color::Rgb { r: 1.0, g: 0.0, b: 0.0, a: 1.0 })) }),
    );
    put(
        "Guide",
        json!({
            "example": ex(&Guide { axis: Axis::Vertical, position: 100.0, locked: false }),
            "doc": "axis Horizontal or Vertical; position in points.",
        }),
    );
    put(
        "Section",
        json!({
            "example": { "first": { "idx": 1, "version": 1 }, "start": 1, "style": "Arabic", "prefix": "" },
            "doc": "first: a page key; style Arabic, LowerAlpha, UpperAlpha, LowerRoman or UpperRoman.",
        }),
    );
    put(
        "TextVariable",
        json!({
            "examples": [
                ex(&tessera_document::variables::TextVariable::custom("Issue", "No. 12")),
                { "name": "Header", "kind": { "RunningHeader": { "style": { "idx": 1, "version": 1 }, "which": "First" } } },
            ],
        }),
    );
    put(
        "ZMove",
        json!({ "values": ["Forward", "Backward", "ToFront", "ToBack"] }),
    );
    put(
        "Edge",
        json!({ "values": ["Left", "HCentre", "Right", "Top", "VCentre", "Bottom"] }),
    );
    put(
        "AlignTo",
        json!({ "values": ["Selection", "Margins", "Page", "Spread"] }),
    );
    put(
        "Fit",
        json!({ "values": ["Stretch", "Proportionally", "FillProportionally", "Centre"] }),
    );
    put("Axis", json!({ "values": ["Horizontal", "Vertical"] }));
    put(
        "Anchor",
        json!({ "values": ["TopLeft", "TopCentre", "TopRight", "MiddleLeft", "Centre", "MiddleRight", "BottomLeft", "BottomCentre", "BottomRight"], "doc": "The reference point a transform resolves about." }),
    );
    put(
        "BezPath",
        json!({ "doc": "kurbo's serde form: a list of path elements such as {\"MoveTo\": [x, y]}, {\"LineTo\": [x, y]}, {\"CurveTo\": [[x1,y1],[x2,y2],[x,y]]}, \"ClosePath\"." }),
    );
    Value::Object(out)
}

/// Type erasure for "anything serde can write", so the table above stays
/// one line per type.
mod erased {
    pub trait Ser {
        fn to_json(&self) -> serde_json::Value;
    }
    impl<T: serde::Serialize> Ser for T {
        fn to_json(&self) -> serde_json::Value {
            serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_example_reads_back_as_its_type() {
        // An example a model copies has to be one the command layer accepts.
        let shapes = all();
        let get = |name: &str| shapes[name]["example"].clone();
        serde_json::from_value::<TextLayout>(get("TextLayout")).unwrap();
        serde_json::from_value::<tessera_text::story::ParagraphFormat>(get("ParagraphFormat"))
            .unwrap();
        serde_json::from_value::<tessera_text::story::CharacterFormat>(get("CharacterFormat"))
            .unwrap();
        serde_json::from_value::<Stroke>(get("Stroke")).unwrap();
        serde_json::from_value::<tessera_geometry::DocRect>(get("DocRect")).unwrap();
        serde_json::from_value::<tessera_geometry::Transform>(get("Transform")).unwrap();
        serde_json::from_value::<Guide>(get("Guide")).unwrap();
        serde_json::from_value::<tessera_document::sections::Section>(get("Section")).unwrap();
        for v in shapes["TextWrap"]["examples"].as_array().unwrap() {
            serde_json::from_value::<TextWrap>(v.clone()).unwrap();
        }
        for v in shapes["TextVariable"]["examples"].as_array().unwrap() {
            serde_json::from_value::<tessera_document::variables::TextVariable>(v.clone()).unwrap();
        }
        for v in shapes["ZMove"]["values"].as_array().unwrap() {
            serde_json::from_value::<tessera_document::document::ZMove>(v.clone()).unwrap();
        }
        for v in shapes["Anchor"]["values"].as_array().unwrap() {
            serde_json::from_value::<tessera_geometry::Anchor>(v.clone()).unwrap();
        }
        for v in shapes["Fit"]["values"].as_array().unwrap() {
            serde_json::from_value::<tessera_document::graphic::Fit>(v.clone()).unwrap();
        }
    }
}
