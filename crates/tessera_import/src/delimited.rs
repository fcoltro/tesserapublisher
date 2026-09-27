//! Delimited text: comma-separated and tab-separated data.
//!
//! What a data merge reads, and what a table can be made from. A spreadsheet
//! saved as text is the one format every spreadsheet, database and mail tool
//! can write, which is why InDesign's data merge takes it and nothing else.
//!
//! ## What "CSV" means in practice
//!
//! There is a standard (RFC 4180) and there are the files people have:
//!
//! - **Three separators.** A comma; a tab, from "Text (Tab delimited)" and
//!   most databases; and a **semicolon**, which is what Excel writes as CSV
//!   wherever the comma is the decimal mark — most of Europe and South
//!   America. The separator is found from the header row, counting only
//!   outside quotes, rather than assumed from the extension.
//! - **Three encodings.** UTF-8, with or without a byte-order mark; UTF-16,
//!   which is Excel's "Unicode Text"; and Windows-1252, which is what Excel on
//!   Windows writes for a plain "CSV" — so a file that is not UTF-8 is read as
//!   1252 rather than refused, and "Café" survives.
//! - **Quotes.** A field in double quotes may hold the separator, a line
//!   break, and a quote written twice. A quote that is never closed is an
//!   **error naming its row**: read on regardless, every field after it would
//!   land in the wrong column, and the merge would print a wrong page per
//!   record without a word.
//!
//! ## Fields
//!
//! The first row names the fields. A name starting `@` is an **image field**,
//! InDesign's convention: its values are paths to pictures. A blank name is
//! called after its column, and a name used twice gets a number, since a
//! field is found by its name. A row longer than the header keeps its extra
//! values under columns named for their place; a shorter one reads as empty
//! past its end. Blank rows are skipped. Every one of these is said in
//! [`Data::notes`], as this crate says everything it had to change.

use std::path::Path;

/// One column of the data: a name, and whether it holds pictures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// As the header wrote it, less the `@` of an image field.
    pub name: String,
    /// Named with a leading `@`: each value is a path to a picture.
    pub image: bool,
}

/// A data file, read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Data {
    pub fields: Vec<Field>,
    /// One per record, each exactly `fields.len()` long.
    pub records: Vec<Vec<String>>,
    /// What the reader changed to make the file a grid, in words.
    pub notes: Vec<String>,
    /// The separator the file used.
    pub separator: char,
}

impl Data {
    /// The column a field is in, by name.
    pub fn field(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f.name == name)
    }

    /// Record `record`'s value for the field named `name`: `None` when there
    /// is no such record or field.
    pub fn value(&self, record: usize, name: &str) -> Option<&str> {
        let column = self.field(name)?;
        self.records.get(record)?.get(column).map(String::as_str)
    }
}

/// Why a data file could not be read.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DataError {
    #[error("could not read {0}")]
    Read(std::path::PathBuf),
    #[error("the file is empty: there is no header row to name the fields")]
    Empty,
    #[error(
        "a quote opened in row {row} is never closed, so every field after it would be misread"
    )]
    UnclosedQuote { row: usize },
}

/// Read a data file from disk.
pub fn read_path(path: &Path) -> Result<Data, DataError> {
    let bytes = std::fs::read(path).map_err(|_| DataError::Read(path.to_path_buf()))?;
    read(&bytes)
}

/// Read delimited text from its bytes.
pub fn read(bytes: &[u8]) -> Result<Data, DataError> {
    let text = decode(bytes);
    let separator = separator_of(&text);
    let rows = split(&text, separator)?;
    let mut rows = rows.into_iter();
    let Some((_, header)) = rows.next() else {
        return Err(DataError::Empty);
    };

    let mut notes = Vec::new();
    let mut fields = fields_of(&header, &mut notes);
    let mut records: Vec<Vec<String>> = Vec::new();
    let mut blank = 0usize;
    for (row, mut values) in rows {
        if values.iter().all(|v| v.trim().is_empty()) {
            blank += 1;
            continue;
        }
        if values.len() > fields.len() {
            notes.push(format!(
                "Row {row} has {} values and the header names {}: the extra ones are kept \
                 under columns named for their place.",
                values.len(),
                fields.len()
            ));
            while fields.len() < values.len() {
                fields.push(Field {
                    name: format!("Column {}", fields.len() + 1),
                    image: false,
                });
            }
        }
        values.resize(fields.len(), String::new());
        records.push(values);
    }
    // A column added for a long row is owed by every record before it.
    for record in &mut records {
        record.resize(fields.len(), String::new());
    }
    if blank > 0 {
        notes.push(format!(
            "{blank} blank row{} skipped.",
            if blank == 1 { " was" } else { "s were" }
        ));
    }
    Ok(Data {
        fields,
        records,
        notes,
        separator,
    })
}

/// The header's names, made usable: an image field's `@` taken off, a blank
/// named for its column, a repeat numbered.
fn fields_of(header: &[String], notes: &mut Vec<String>) -> Vec<Field> {
    let mut fields: Vec<Field> = Vec::with_capacity(header.len());
    for (i, raw) in header.iter().enumerate() {
        let raw = raw.trim();
        let (image, name) = match raw.strip_prefix('@') {
            Some(rest) => (true, rest.trim()),
            None => (false, raw),
        };
        let mut name = if name.is_empty() {
            notes.push(format!(
                "Column {} has no name in the header, so it is called \"Column {}\".",
                i + 1,
                i + 1
            ));
            format!("Column {}", i + 1)
        } else {
            name.to_owned()
        };
        if fields.iter().any(|f| f.name == name) {
            let base = name.clone();
            let mut n = 2;
            while fields.iter().any(|f| f.name == name) {
                name = format!("{base} ({n})");
                n += 1;
            }
            notes.push(format!(
                "\"{base}\" names two columns; the second is called \"{name}\"."
            ));
        }
        fields.push(Field { name, image });
    }
    fields
}

/// The file's text: by its byte-order mark, as UTF-8 when it is, and as
/// Windows-1252 when it is not.
fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, u16::from_le_bytes);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, u16::from_be_bytes);
    }
    // UTF-16 with no mark: text in Latin letters has a zero in every other
    // byte, which no UTF-8 or 1252 text has.
    if bytes.len() >= 4 && bytes.len().is_multiple_of(2) {
        let odd_zeros = bytes.iter().skip(1).step_by(2).filter(|&&b| b == 0).count();
        let even_zeros = bytes.iter().step_by(2).filter(|&&b| b == 0).count();
        let half = bytes.len() / 2;
        if odd_zeros * 2 > half && even_zeros == 0 {
            return utf16(bytes, u16::from_le_bytes);
        }
        if even_zeros * 2 > half && odd_zeros == 0 {
            return utf16(bytes, u16::from_be_bytes);
        }
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => bytes.iter().map(|&b| windows_1252(b)).collect(),
    }
}

fn utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> String {
    let units: Vec<u16> = bytes.chunks_exact(2).map(|p| unit([p[0], p[1]])).collect();
    String::from_utf16_lossy(&units)
}

/// A Windows-1252 byte as the character it stands for: Latin-1, except the
/// row 0x80–0x9F, where Windows put the curly quotes, the dashes and the euro.
fn windows_1252(byte: u8) -> char {
    const ROW: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž',
        '\u{8F}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9D}',
        'ž', 'Ÿ',
    ];
    match byte {
        0x80..=0x9F => ROW[usize::from(byte - 0x80)],
        _ => char::from(byte),
    }
}

/// The separator the header row uses: whichever of tab, semicolon and comma
/// it has most of outside quotes. A tie goes to the tab, then the comma —
/// a one-column file has none, and reads the same with any.
fn separator_of(text: &str) -> char {
    let mut counts = [('\t', 0usize), (',', 0), (';', 0)];
    let mut quoted = false;
    for c in text.chars() {
        match c {
            '"' => quoted = !quoted,
            '\n' | '\r' if !quoted => break,
            _ if !quoted => {
                if let Some(entry) = counts.iter_mut().find(|(s, _)| *s == c) {
                    entry.1 += 1;
                }
            }
            _ => {}
        }
    }
    counts
        .iter()
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| rank(b.0).cmp(&rank(a.0))))
        .map_or(',', |(s, n)| if *n == 0 { ',' } else { *s })
}

fn rank(separator: char) -> u8 {
    match separator {
        '\t' => 0,
        ',' => 1,
        _ => 2,
    }
}

/// Every row's values, numbered from one as a spreadsheet numbers them.
fn split(text: &str, separator: char) -> Result<Vec<(usize, Vec<String>)>, DataError> {
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    // The spreadsheet row a record began on, for an error to name.
    let mut line = 1usize;
    let mut started_on = 1usize;
    let mut quoted = false;
    // Whether the field so far is a quoted one — its quotes are not text.
    let mut was_quoted = false;
    let mut chars = text.chars().peekable();
    let mut any = false;
    while let Some(c) = chars.next() {
        any = true;
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                '\r' => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    line += 1;
                    field.push('\n');
                }
                '\n' => {
                    line += 1;
                    field.push('\n');
                }
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() && !was_quoted => {
                quoted = true;
                was_quoted = true;
            }
            c if c == separator => {
                row.push(std::mem::take(&mut field));
                was_quoted = false;
            }
            '\r' | '\n' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(std::mem::take(&mut field));
                was_quoted = false;
                rows.push((started_on, std::mem::take(&mut row)));
                line += 1;
                started_on = line;
            }
            _ => field.push(c),
        }
    }
    if quoted {
        return Err(DataError::UnclosedQuote { row: started_on });
    }
    // The last row, when the file does not end with a line break.
    if any && (!field.is_empty() || !row.is_empty() || was_quoted) {
        row.push(field);
        rows.push((started_on, row));
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(data: &Data) -> Vec<&str> {
        data.fields.iter().map(|f| f.name.as_str()).collect()
    }

    #[test]
    fn a_comma_file_with_quotes_line_breaks_and_doubled_quotes() {
        let data = read(
            b"Name,Address,Note\r\n\
              Ana,\"12 Rua Nova,\nLisboa\",\"She said \"\"hi\"\"\"\r\n\
              Bo,Oslo,\r\n",
        )
        .expect("read");
        assert_eq!(data.separator, ',');
        assert_eq!(names(&data), ["Name", "Address", "Note"]);
        assert_eq!(data.records.len(), 2);
        assert_eq!(data.value(0, "Address"), Some("12 Rua Nova,\nLisboa"));
        assert_eq!(data.value(0, "Note"), Some("She said \"hi\""));
        assert_eq!(data.value(1, "Note"), Some(""), "an empty last field");
        assert!(data.notes.is_empty(), "{:?}", data.notes);
    }

    #[test]
    fn tab_and_semicolon_files_are_found_by_their_header() {
        let tabs = read(b"Name\tPrice\nTea\t2,50\n").expect("tabs");
        assert_eq!(tabs.separator, '\t');
        assert_eq!(
            tabs.value(0, "Price"),
            Some("2,50"),
            "a decimal comma stays"
        );
        // Excel in Portugal: semicolons, and decimal commas in the values.
        let semis = read(b"Nome;Pre\xE7o\nCh\xE1;2,50\n").expect("semicolons");
        assert_eq!(semis.separator, ';');
        assert_eq!(
            names(&semis),
            ["Nome", "Preço"],
            "Windows-1252, read as such"
        );
        assert_eq!(semis.value(0, "Preço"), Some("2,50"));
    }

    #[test]
    fn utf16_from_excel_s_unicode_text_with_and_without_a_mark() {
        let text = "Nome\tCidade\r\nJoão\tSão Paulo\r\n";
        let le: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let marked: Vec<u8> = [0xFF, 0xFE].into_iter().chain(le.iter().copied()).collect();
        for bytes in [marked, le] {
            let data = read(&bytes).expect("utf-16");
            assert_eq!(data.value(0, "Cidade"), Some("São Paulo"));
        }
        let be: Vec<u8> = [0xFE, 0xFF]
            .into_iter()
            .chain(text.encode_utf16().flat_map(u16::to_be_bytes))
            .collect();
        assert_eq!(read(&be).unwrap().value(0, "Nome"), Some("João"));
        let bom8: Vec<u8> = [0xEF, 0xBB, 0xBF].into_iter().chain(text.bytes()).collect();
        assert_eq!(
            names(&read(&bom8).unwrap())[0],
            "Nome",
            "the mark is not part of the first name"
        );
    }

    #[test]
    fn an_image_field_a_blank_name_and_a_repeated_one() {
        let data = read(b"Name,@Photo,,Name\nAna,ana.jpg,x,y\n").expect("read");
        assert_eq!(names(&data), ["Name", "Photo", "Column 3", "Name (2)"]);
        assert!(data.fields[1].image && !data.fields[0].image);
        assert_eq!(data.value(0, "Photo"), Some("ana.jpg"));
        assert_eq!(data.notes.len(), 2, "{:?}", data.notes);
    }

    #[test]
    fn ragged_rows_become_a_grid_and_say_so() {
        let data = read(b"A,B\n1\n2,3,4\n\n,\n5,6").expect("read");
        assert_eq!(names(&data), ["A", "B", "Column 3"]);
        assert_eq!(
            data.records,
            [
                vec!["1".to_owned(), String::new(), String::new()],
                vec!["2".into(), "3".into(), "4".into()],
                vec!["5".into(), "6".into(), String::new()],
            ],
            "short padded, long kept, blank skipped, no final line break needed"
        );
        assert_eq!(data.notes.len(), 2, "{:?}", data.notes);
    }

    #[test]
    fn an_unclosed_quote_is_an_error_naming_its_row_and_an_empty_file_has_no_header() {
        assert_eq!(
            read(b"A,B\n1,2\n3,\"never closed\n4,5\n"),
            Err(DataError::UnclosedQuote { row: 3 })
        );
        assert_eq!(read(b""), Err(DataError::Empty));
    }

    #[test]
    fn a_separator_inside_quotes_does_not_decide_the_separator() {
        let data = read(b"\"Last, First\"\tCity\n\"Doe, Jo\"\tRome\n").expect("read");
        assert_eq!(data.separator, '\t');
        assert_eq!(names(&data), ["Last, First", "City"]);
    }
}
