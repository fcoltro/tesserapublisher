//! Workbooks — Excel's `.xlsx`, `.xlsm`, `.xlsb` and `.xls`, and
//! OpenDocument's `.ods` — read as data, for a merge and for a table.
//!
//! The **first worksheet** is read, its first row naming the fields, and made
//! a grid through the same code a delimited file goes through
//! ([`crate::delimited::from_rows`]), so a blank name, a repeated one and a
//! ragged row are dealt with, and said, the same way. A workbook of several
//! sheets says which was read and that the others were not.
//!
//! A cell is read as its **value**, not as the sheet formats it: 4.5 rather
//! than "£4.50", since the format is the spreadsheet's and the words on the
//! page are the layout's. A whole number reads without a decimal point, a
//! date as the date — `2026-09-27`, or with its time when it has one — and a
//! cell showing an error (`#REF!`) reads as nothing and is counted.

use std::path::Path;

use calamine::{Data as Cell, Reader};

use crate::delimited::{Data, DataError};

/// The extensions a workbook comes with.
pub const EXTENSIONS: [&str; 5] = ["xlsx", "xlsm", "xlsb", "xls", "ods"];

/// Whether `path` names a workbook rather than a delimited file.
pub fn is_workbook(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// A data file of either kind, by its extension.
pub fn read_any(path: &Path) -> Result<Data, DataError> {
    if is_workbook(path) {
        read_path(path)
    } else {
        crate::delimited::read_path(path)
    }
}

/// A workbook's first worksheet, as data.
pub fn read_path(path: &Path) -> Result<Data, DataError> {
    let mut book =
        calamine::open_workbook_auto(path).map_err(|e| DataError::Workbook(e.to_string()))?;
    let sheets = book.sheet_names();
    let first = sheets.first().cloned().ok_or(DataError::Empty)?;
    let range = book
        .worksheet_range(&first)
        .map_err(|e| DataError::Workbook(e.to_string()))?;
    let mut errors = 0usize;
    let rows: Vec<Vec<String>> = range
        .rows()
        .map(|row| {
            row.iter()
                .map(|cell| {
                    if matches!(cell, Cell::Error(_)) {
                        errors += 1;
                    }
                    text(cell)
                })
                .collect()
        })
        .collect();
    let mut data = crate::delimited::from_rows(rows)?;
    if sheets.len() > 1 {
        data.notes.push(format!(
            "The workbook's first sheet, \"{first}\", was read; its other {} w{} not.",
            sheets.len() - 1,
            if sheets.len() == 2 { "as" } else { "ere" }
        ));
    }
    if errors > 0 {
        data.notes.push(format!(
            "{errors} cell{} showed an error in the workbook and read as nothing.",
            if errors == 1 { "" } else { "s" }
        ));
    }
    Ok(data)
}

/// A cell's value as words.
fn text(cell: &Cell) -> String {
    match cell {
        Cell::Empty | Cell::Error(_) => String::new(),
        Cell::String(s) | Cell::DateTimeIso(s) | Cell::DurationIso(s) => s.clone(),
        Cell::Int(n) => n.to_string(),
        Cell::Float(f) => number(*f),
        Cell::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_owned(),
        Cell::DateTime(d) => excel_date(d.as_f64()),
    }
}

/// A number as a spreadsheet shows it by default: a whole number without a
/// point, anything else as short as it can be written exactly.
fn number(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{f:.0}")
    } else {
        f.to_string()
    }
}

/// An Excel date — days since 30 December 1899, the fraction the time of
/// day — as `2026-09-27`, or `2026-09-27 14:30` when it has a time.
fn excel_date(serial: f64) -> String {
    let days = serial.floor() as i64;
    // Howard Hinnant's days-to-civil, from 1970; the epoch is 25 569 days
    // after Excel's.
    let z = days - 25_569 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let minutes = ((serial - serial.floor()) * 1_440.0).round() as i64;
    if minutes == 0 {
        format!("{year:04}-{month:02}-{day:02}")
    } else {
        format!(
            "{year:04}-{month:02}-{day:02} {:02}:{:02}",
            minutes / 60,
            minutes % 60
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A workbook written by hand: the parts `.xlsx` needs and no more,
    /// its cells inline so no shared-string table is needed.
    fn workbook(path: &Path, sheets: &[&str], rows: &[&[&str]]) {
        let file = std::fs::File::create(path).expect("create");
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        let mut put = |name: &str, body: String| {
            zip.start_file(name, options).expect("entry");
            zip.write_all(body.as_bytes()).expect("write");
        };
        let mut types = String::from(
            r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#,
        );
        let mut listed = String::new();
        let mut rels = String::new();
        for (i, name) in sheets.iter().enumerate() {
            let n = i + 1;
            types.push_str(&format!(r#"<Override PartName="/xl/worksheets/sheet{n}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#));
            listed.push_str(&format!(
                r#"<sheet name="{name}" sheetId="{n}" r:id="rId{n}"/>"#
            ));
            rels.push_str(&format!(r#"<Relationship Id="rId{n}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{n}.xml"/>"#));
        }
        types.push_str("</Types>");
        put("[Content_Types].xml", types);
        put("_rels/.rels", r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#.into());
        put(
            "xl/workbook.xml",
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>{listed}</sheets></workbook>"#
            ),
        );
        put(
            "xl/_rels/workbook.xml.rels",
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rels}</Relationships>"#
            ),
        );
        for n in 1..=sheets.len() {
            let mut data = String::new();
            for (r, row) in rows.iter().enumerate() {
                data.push_str(&format!(r#"<row r="{}">"#, r + 1));
                for (c, value) in row.iter().enumerate() {
                    let at = format!("{}{}", char::from(b'A' + c as u8), r + 1);
                    if let Some(number) = value.strip_prefix('=') {
                        data.push_str(&format!(r#"<c r="{at}"><v>{number}</v></c>"#));
                    } else {
                        data.push_str(&format!(
                            r#"<c r="{at}" t="inlineStr"><is><t>{value}</t></is></c>"#
                        ));
                    }
                }
                data.push_str("</row>");
            }
            put(
                &format!("xl/worksheets/sheet{n}.xml"),
                format!(
                    r#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>{data}</sheetData></worksheet>"#
                ),
            );
        }
        zip.finish().expect("finish");
    }

    #[test]
    fn a_workbook_s_first_sheet_is_data_its_numbers_as_a_sheet_shows_them() {
        let path = std::env::temp_dir().join(format!("tessera-book-{}.xlsx", std::process::id()));
        workbook(
            &path,
            &["Prices", "Notes"],
            &[
                &["Item", "Price", "Stock"],
                &["Tea", "=2.5", "=40"],
                &["Café", "=12", "=0"],
            ],
        );
        assert!(is_workbook(&path));
        let data = read_any(&path).expect("read");
        let names: Vec<&str> = data.fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["Item", "Price", "Stock"]);
        assert_eq!(data.value(0, "Price"), Some("2.5"));
        assert_eq!(
            data.value(1, "Price"),
            Some("12"),
            "a whole number, no point"
        );
        assert_eq!(data.value(1, "Item"), Some("Café"));
        assert!(
            data.notes.iter().any(|n| n.contains("\"Prices\"")),
            "the other sheet is said: {:?}",
            data.notes
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn an_excel_date_is_the_day_it_names() {
        assert_eq!(excel_date(45_292.0), "2024-01-01");
        assert_eq!(excel_date(46_292.5), "2026-09-27 12:00");
        assert_eq!(excel_date(1.0), "1899-12-31");
    }

    #[test]
    fn a_file_that_is_not_a_workbook_says_so() {
        let path = std::env::temp_dir().join(format!("tessera-not-{}.xlsx", std::process::id()));
        std::fs::write(&path, b"not a zip").expect("write");
        assert!(matches!(read_any(&path), Err(DataError::Workbook(_))));
        let _ = std::fs::remove_file(path);
    }
}
