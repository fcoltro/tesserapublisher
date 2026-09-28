//! Data merge: a document that is a template for every record of a data file.
//!
//! InDesign's model, which is the right one: the document names a **data
//! source** — a comma- or tab-separated file — and its text holds **field
//! markers**, one character each, where a record's values go
//! ([`tessera_text::variables::Marker::Field`]). A marker names its field by
//! its place in [`DataSource::fields`], as a variable marker names its
//! variable, so the words around it are ordinary text that every edit, find
//! and change, and copy understands.
//!
//! The file is **linked, not embedded**: the data is the person's to update,
//! and a merge run tomorrow should read tomorrow's list. The field names are
//! kept in the document as well, so the fields still read as themselves —
//! «Name» — when the file is not there, and a missing file is a thing to
//! say rather than a document gone blank.
//!
//! Which record is being previewed is not part of the document: it is set on
//! it from outside, as its file facts are, and the layout reads it.

use serde::{Deserialize, Serialize};

/// One column of the data a document merges.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeField {
    pub name: String,
    /// Its values are paths to pictures: a header named `@Photo`.
    #[serde(default)]
    pub image: bool,
}

/// Where a document's records come from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataSource {
    /// The data file, as it was chosen.
    pub path: std::path::PathBuf,
    /// Its fields in the order the markers count them. A field the file
    /// later loses stays here, reading as nothing, so the markers after it
    /// keep their meaning.
    pub fields: Vec<MergeField>,
}

impl DataSource {
    /// Which of the document's fields `name` is.
    pub fn field(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f.name == name)
    }

    /// The fields a newly read file names, taking the place of this
    /// source's: every field kept where it was, so a marker still reads the
    /// same column by name, and the file's new ones added after.
    pub fn with_fields_of(&self, path: std::path::PathBuf, fields: &[MergeField]) -> Self {
        let mut kept = self.fields.clone();
        for field in fields {
            match kept.iter_mut().find(|f| f.name == field.name) {
                Some(existing) => existing.image = field.image,
                None => kept.push(field.clone()),
            }
        }
        Self { path, fields: kept }
    }
}

/// The most fields a document can hold: a marker carries its index in one
/// byte of code point.
pub const MOST_FIELDS: usize = 256;

/// What each field reads as for one record: its values in the document's
/// field order, found by name in the data's own, so columns moved about in
/// the file still land in the right markers. A field the data lacks reads
/// as nothing.
pub fn values_for(source: &DataSource, data_fields: &[String], record: &[String]) -> Vec<String> {
    source
        .fields
        .iter()
        .map(|field| {
            data_fields
                .iter()
                .position(|name| *name == field.name)
                .and_then(|column| record.get(column))
                .cloned()
                .unwrap_or_default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(name: &str) -> MergeField {
        MergeField {
            name: name.into(),
            image: false,
        }
    }

    #[test]
    fn a_record_s_values_are_found_by_name_whatever_the_file_s_order() {
        let source = DataSource {
            path: "people.csv".into(),
            fields: vec![field("Name"), field("City"), field("Gone")],
        };
        let data_fields = ["City".to_string(), "Name".to_string()];
        let record = ["Lisbon".to_string(), "Ana".to_string()];
        assert_eq!(
            values_for(&source, &data_fields, &record),
            ["Ana", "Lisbon", ""]
        );
    }

    #[test]
    fn a_new_file_keeps_every_field_where_it_was_and_adds_its_own() {
        let source = DataSource {
            path: "old.csv".into(),
            fields: vec![field("Name"), field("City")],
        };
        let next = source.with_fields_of(
            "new.csv".into(),
            &[
                field("City"),
                field("Email"),
                MergeField {
                    name: "Name".into(),
                    image: false,
                },
            ],
        );
        let names: Vec<&str> = next.fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["Name", "City", "Email"]);
        assert_eq!(next.path, std::path::PathBuf::from("new.csv"));
    }
}
