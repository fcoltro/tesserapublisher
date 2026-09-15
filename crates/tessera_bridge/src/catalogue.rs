//! Every command, read off the source that defines it.
//!
//! `Command` is the whole mutation surface of the application — the A6
//! invariant — and its doc comments are the best description of each
//! change anybody has written. So the catalogue a model reads **is that
//! source**: `command.rs` is embedded in the binary at compile time and
//! parsed once, into a name, a doc, and a list of typed fields per variant.
//! A command added to the enum is reachable, described, the moment it is
//! written; nothing here has to be kept in step by hand. The source is
//! embedded, not read from disk, because the binary is installed on
//! machines that have no source tree.
//!
//! The parser knows rustfmt's shape of an enum and nothing more: a `///`
//! line is doc, a line at four spaces beginning with a capital is a variant,
//! and a variant is a unit, a tuple, or a struct with its fields at eight
//! spaces. A test holds it to the real file.

use serde_json::{Value, json};
use std::sync::OnceLock;

/// The text of `tessera_ui/src/command.rs`, as compiled.
const SOURCE: &str = include_str!("../../tessera_ui/src/command.rs");

/// One variant of `Command`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    pub name: String,
    pub doc: String,
    pub shape: Shape,
}

/// How a variant carries its arguments — which decides the JSON it takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shape {
    /// `Undo`: no arguments. Sent as the bare name.
    Unit,
    /// `AddRectangle(DocRect)`: one value, sent as it is.
    Newtype(String),
    /// `AddPath(DocRect, BezPath)`: several values, sent as an array.
    Tuple(Vec<String>),
    /// `SetText { id, text }`: named fields, sent as an object.
    Struct(Vec<Field>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub ty: String,
    pub doc: String,
}

/// The catalogue, parsed on first use.
pub fn variants() -> &'static [Variant] {
    static ALL: OnceLock<Vec<Variant>> = OnceLock::new();
    ALL.get_or_init(|| parse(SOURCE))
}

/// The variant called `name`.
pub fn variant(name: &str) -> Option<&'static Variant> {
    variants().iter().find(|v| v.name == name)
}

/// Read the `Command` enum out of the module's source.
fn parse(source: &str) -> Vec<Variant> {
    let mut lines = source
        .lines()
        .skip_while(|l| !l.starts_with("pub enum Command {"))
        .skip(1);
    let mut out = Vec::new();
    let mut doc: Vec<String> = Vec::new();
    while let Some(line) = lines.next() {
        if line == "}" {
            break;
        }
        let trimmed = line.trim();
        if let Some(text) = trimmed.strip_prefix("///") {
            doc.push(text.trim().to_owned());
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with("#[") || trimmed.starts_with("//") {
            continue;
        }
        // A variant line: four spaces, a capital.
        let Some(rest) = line.strip_prefix("    ") else {
            continue;
        };
        if rest.starts_with(' ') || !rest.starts_with(|c: char| c.is_ascii_uppercase()) {
            continue;
        }
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        let after = &rest[name.len()..];
        let shape = if after.starts_with(',') {
            Shape::Unit
        } else if let Some(inner) = after.strip_prefix('(') {
            let inner = inner.trim_end_matches(',').trim_end_matches(')');
            let types = split_top_level(inner);
            match types.as_slice() {
                [one] => Shape::Newtype(one.clone()),
                many => Shape::Tuple(many.to_vec()),
            }
        } else if after.starts_with(" {") {
            let mut fields = Vec::new();
            let mut field_doc: Vec<String> = Vec::new();
            for line in lines.by_ref() {
                let t = line.trim();
                if t == "}," || t == "}" {
                    break;
                }
                if let Some(text) = t.strip_prefix("///") {
                    field_doc.push(text.trim().to_owned());
                    continue;
                }
                if t.starts_with("#[") || t.is_empty() {
                    continue;
                }
                if let Some((fname, ty)) = t.split_once(':') {
                    fields.push(Field {
                        name: fname.trim().to_owned(),
                        ty: ty.trim().trim_end_matches(',').to_owned(),
                        doc: std::mem::take(&mut field_doc).join(" "),
                    });
                }
            }
            Shape::Struct(fields)
        } else {
            Shape::Unit
        };
        out.push(Variant {
            name,
            doc: std::mem::take(&mut doc).join(" "),
            shape,
        });
    }
    out
}

/// Split `a, b<c, d>, e` at the commas that are not inside brackets.
fn split_top_level(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for c in s.chars() {
        match c {
            '<' | '(' => depth += 1,
            '>' | ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(current.trim().to_owned());
                current.clear();
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_owned());
    }
    out
}

/// Whether a Rust type is one of the document's ids, which a model is
/// handed as a number and sends back as one.
pub fn is_id_type(ty: &str) -> bool {
    let bare = ty
        .strip_prefix("Option<")
        .and_then(|t| t.strip_suffix('>'))
        .unwrap_or(ty);
    bare.rsplit("::").next().is_some_and(|t| t.ends_with("Id"))
}

/// A field's type in the words a model reads: the JSON it should send.
pub fn describe_type(ty: &str) -> String {
    let (optional, bare) = match ty.strip_prefix("Option<").and_then(|t| t.strip_suffix('>')) {
        Some(inner) => (true, inner),
        None => (false, ty),
    };
    let short = bare.rsplit("::").next().unwrap_or(bare);
    let base = match short {
        "f64" | "f32" => "number".to_owned(),
        "usize" | "u32" | "u16" | "u8" | "i32" => "integer".to_owned(),
        "bool" => "boolean".to_owned(),
        "String" | "PathBuf" => "string".to_owned(),
        "Range<usize>" => "{start, end} byte offsets".to_owned(),
        "DocRect" => "{x, y, width, height} in points".to_owned(),
        "Insets" => "{top, bottom, left, right} in points".to_owned(),
        "Transform" => "[a, b, c, d, e, f] affine".to_owned(),
        "BezPath" => "kurbo path as JSON".to_owned(),
        t if t.ends_with("Id") => format!("integer ({t} from describe_document)"),
        t => format!("{t} object; see describe_shapes"),
    };
    if optional {
        format!("{base}, or null")
    } else {
        base
    }
}

/// The catalogue as the model reads it.
pub fn listing() -> Vec<Value> {
    variants()
        .iter()
        .map(|v| {
            let (kind, arguments): (&str, Value) = match &v.shape {
                Shape::Unit => ("none", Value::Null),
                Shape::Newtype(t) => ("value", json!(describe_type(t))),
                Shape::Tuple(ts) => (
                    "array",
                    json!(ts.iter().map(|t| describe_type(t)).collect::<Vec<_>>()),
                ),
                Shape::Struct(fields) => (
                    "object",
                    json!(
                        fields
                            .iter()
                            .map(|f| {
                                json!({
                                    "name": f.name,
                                    "type": describe_type(&f.ty),
                                    "doc": f.doc,
                                })
                            })
                            .collect::<Vec<_>>()
                    ),
                ),
            };
            json!({
                "name": v.name,
                "doc": v.doc,
                "arguments": kind,
                "fields": arguments,
            })
        })
        .collect()
}

/// A document id as the JSON the command layer deserialises.
fn key_json(n: u64) -> Value {
    serde_json::to_value(slotmap::KeyData::from_ffi(n)).unwrap_or(Value::Null)
}

/// Turn a model's `arguments` for `variant` into the JSON `Command`
/// deserialises: the externally tagged form serde uses, with every id
/// field's number turned into the key the document keeps.
pub fn command_json(variant: &Variant, arguments: Value) -> Result<Value, String> {
    let payload = match &variant.shape {
        Shape::Unit => return Ok(Value::String(variant.name.clone())),
        Shape::Newtype(ty) => translate(ty, arguments)?,
        Shape::Tuple(types) => {
            let Value::Array(items) = arguments else {
                return Err(format!(
                    "{} takes an array of {} values",
                    variant.name,
                    types.len()
                ));
            };
            if items.len() != types.len() {
                return Err(format!(
                    "{} takes {} values, not {}",
                    variant.name,
                    types.len(),
                    items.len()
                ));
            }
            Value::Array(
                types
                    .iter()
                    .zip(items)
                    .map(|(t, v)| translate(t, v))
                    .collect::<Result<Vec<_>, _>>()?,
            )
        }
        Shape::Struct(fields) => {
            let Value::Object(mut given) = arguments else {
                return Err(format!("{} takes an object of fields", variant.name));
            };
            let mut out = serde_json::Map::new();
            for f in fields {
                let Some(v) = given.remove(&f.name) else {
                    if f.ty.starts_with("Option<") {
                        out.insert(f.name.clone(), Value::Null);
                        continue;
                    }
                    return Err(format!("{} needs a field {:?}", variant.name, f.name));
                };
                out.insert(f.name.clone(), translate(&f.ty, v)?);
            }
            if let Some(extra) = given.keys().next() {
                return Err(format!("{} has no field {extra:?}", variant.name));
            }
            Value::Object(out)
        }
    };
    Ok(json!({ &variant.name: payload }))
}

/// An id number becomes a key; anything else passes through.
fn translate(ty: &str, value: Value) -> Result<Value, String> {
    if !is_id_type(ty) {
        return Ok(value);
    }
    match value {
        Value::Null if ty.starts_with("Option<") => Ok(Value::Null),
        Value::Number(n) => n
            .as_u64()
            .map(key_json)
            .ok_or_else(|| format!("an id must be a whole number, not {n}")),
        // Already a key, as describe_shapes would show one.
        Value::Object(o) if o.contains_key("idx") => Ok(Value::Object(o)),
        other => Err(format!("{ty} takes an id number, not {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_ui::command::Command;

    #[test]
    fn the_whole_enum_is_read_with_its_docs() {
        let all = variants();
        assert!(all.len() > 100, "{} variants", all.len());
        let names: Vec<&str> = all.iter().map(|v| v.name.as_str()).collect();
        for expected in [
            "AddRectangle",
            "SetText",
            "Undo",
            "AddPage",
            "SetTransforms",
            "AddPath",
        ] {
            assert!(
                names.contains(&expected),
                "{expected} missing from {names:?}"
            );
        }
        let add_page = variant("AddPage").unwrap();
        assert_eq!(add_page.shape, Shape::Unit);
        assert!(
            add_page
                .doc
                .starts_with("Add a page at the end of the document.")
        );
        let set_text = variant("SetText").unwrap();
        match &set_text.shape {
            Shape::Struct(fields) => {
                assert_eq!(fields[0].name, "id");
                assert_eq!(fields[0].ty, "FrameId");
                assert_eq!(fields[1].ty, "String");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            variant("AddPath").unwrap().shape,
            Shape::Tuple(vec!["DocRect".into(), "kurbo::BezPath".into()])
        );
        assert_eq!(
            variant("AddRectangle").unwrap().shape,
            Shape::Newtype("DocRect".into())
        );
    }

    #[test]
    fn every_parsed_name_is_a_variant_serde_knows() {
        // Not "unknown variant": a wrong payload fails differently, and a
        // name the enum does not have fails with exactly those words. This
        // is what holds the parser to the file.
        for v in variants() {
            let probe = json!({ &v.name: {} });
            let error = serde_json::from_value::<Command>(probe)
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            assert!(!error.contains("unknown variant"), "{}: {error}", v.name);
        }
        assert!(
            serde_json::from_value::<Command>(json!({ "MakeCoffee": {} }))
                .unwrap_err()
                .to_string()
                .contains("unknown variant")
        );
    }

    #[test]
    fn arguments_become_the_command_and_ids_become_keys() {
        let set_text = variant("SetText").unwrap();
        let value = command_json(set_text, json!({ "id": 4294967297u64, "text": "hi" })).unwrap();
        let command: Command = serde_json::from_value(value).expect("a command");
        match command {
            Command::SetText { id, text } => {
                assert_eq!(text, "hi");
                assert_eq!(slotmap::Key::data(&id).as_ffi(), 4294967297);
            }
            other => panic!("{other:?}"),
        }
        // A unit, a newtype, a tuple.
        let undo: Command =
            serde_json::from_value(command_json(variant("Undo").unwrap(), Value::Null).unwrap())
                .unwrap();
        assert!(matches!(undo, Command::Undo));
        let rect = command_json(
            variant("AddRectangle").unwrap(),
            json!({ "x": 1, "y": 2, "width": 3, "height": 4 }),
        )
        .unwrap();
        assert!(matches!(
            serde_json::from_value::<Command>(rect).unwrap(),
            Command::AddRectangle(_)
        ));
        // Missing and extra fields are named.
        assert!(
            command_json(set_text, json!({ "id": 1 }))
                .unwrap_err()
                .contains("text")
        );
        assert!(
            command_json(set_text, json!({ "id": 1, "text": "", "bogus": 1 }))
                .unwrap_err()
                .contains("bogus")
        );
        // An optional id may be left out.
        let v = variant("SetParagraphStyleOf").unwrap();
        let out =
            command_json(v, json!({ "story": 1, "range": { "start": 0, "end": 1 } })).unwrap();
        assert!(serde_json::from_value::<Command>(out).is_ok());
    }

    #[test]
    fn types_are_described_in_a_model_s_words() {
        assert_eq!(describe_type("f64"), "number");
        assert_eq!(
            describe_type("Option<FrameId>"),
            "integer (FrameId from describe_document), or null"
        );
        assert!(describe_type("tessera_document::nodes::TextLayout").contains("describe_shapes"));
        assert!(is_id_type("Option<tessera_document::ids::ObjectStyleId>"));
        assert!(!is_id_type("String"));
    }
}
