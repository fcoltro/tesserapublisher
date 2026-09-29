//! A document as an EPUB 3 book: InDesign's File ▸ Export ▸ EPUB
//! (Reflowable).
//!
//! An EPUB is web pages in a ZIP with a list of what is in it: the same
//! content [`crate::export`] writes, split into chapters at every
//! first-level heading so a reader is never handed one enormous file, a
//! table of contents made from the headings, and the package file that
//! names every part and the order to read them in.
//!
//! **Fonts are not put in the book.** A licence to set type is not a
//! licence to hand the font on — the reasoning Package follows — so the
//! reader's own fonts set it, in the styles' families where it has them.

use std::collections::HashMap;
use std::io::Write as _;

use tessera_document::document::Document;

use crate::text::escape;
use crate::{Options, STYLESHEET, render};

/// What goes on the book's cover and in its catalogue entry.
#[derive(Debug, Clone, Default)]
pub struct EpubOptions {
    /// The content's options: pictures' format and resolution. Its title is
    /// the book's when `title` is empty.
    pub content: Options,
    pub title: String,
    pub author: String,
    /// A unique identifier: an ISBN, or a UUID when the book has none.
    pub identifier: String,
    /// When the book was made, as `YYYY-MM-DDThh:mm:ssZ`.
    pub modified: String,
    /// A cover picture, as its file's bytes and its extension.
    pub cover: Option<(Vec<u8>, String)>,
}

/// The file name a chapter is written as.
fn chapter_file(n: usize) -> String {
    format!("chapter-{n}.xhtml")
}

/// The parts of `body` a chapter each: a new one at every first-level
/// heading, and what comes before the first heading a chapter of its own.
fn chapters(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    for line in body.split_inclusive('\n') {
        if line.starts_with("<h1") && !current.trim().is_empty() {
            out.push(std::mem::take(&mut current));
        }
        current.push_str(line);
    }
    if !current.trim().is_empty() || out.is_empty() {
        out.push(current);
    }
    out
}

/// Every `id="…"` in `html`.
fn ids(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find(" id=\"") {
        let from = at + 5;
        let Some(len) = rest[from..].find('"') else {
            break;
        };
        out.push(rest[from..from + len].to_owned());
        rest = &rest[from + len..];
    }
    out
}

/// `href="#id"` pointing into another chapter, made to point there.
fn relink(html: &str, here: &str, home: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = rest.find("href=\"#") {
        let from = at + 7;
        out.push_str(&rest[..at]);
        let len = rest[from..].find('"').unwrap_or(0);
        let id = &rest[from..from + len];
        match home.get(id) {
            Some(file) if file != here => out.push_str(&format!("href=\"{file}#{id}")),
            _ => out.push_str(&format!("href=\"#{id}")),
        }
        rest = &rest[from + len..];
    }
    out.push_str(rest);
    out
}

/// A content document: XHTML, as EPUB requires, around `body`.
fn xhtml(title: &str, language: &str, body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE html>\n\
         <html xmlns=\"http://www.w3.org/1999/xhtml\" \
         xmlns:epub=\"http://www.idpf.org/2007/ops\" lang=\"{lang}\" xml:lang=\"{lang}\">\n\
         <head>\n<meta charset=\"utf-8\" />\n<title>{title}</title>\n\
         <link rel=\"stylesheet\" href=\"{STYLESHEET}\" />\n</head>\n<body>\n{body}</body>\n</html>\n",
        lang = escape(language),
        title = escape(title),
    )
}

/// The media type of a file the book holds, by its extension.
fn media_type(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("") {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "css" => "text/css",
        _ => "application/xhtml+xml",
    }
}

/// Export `doc` as a reflowable EPUB 3, its bytes ready to be written.
pub fn export(doc: &Document, options: &EpubOptions) -> Result<Vec<u8>, String> {
    let rendered = render(doc, &options.content);
    let language = crate::text::document_language(doc);
    let title = if options.title.trim().is_empty() {
        options
            .content
            .title
            .clone()
            .unwrap_or_else(|| doc.file_facts().file_name(false, false))
    } else {
        options.title.clone()
    };
    let title = if title.trim().is_empty() {
        "Untitled".to_owned()
    } else {
        title
    };

    // The chapters, and which chapter each id is in.
    let parts = chapters(&rendered.body);
    let mut home: HashMap<String, String> = HashMap::new();
    for (n, part) in parts.iter().enumerate() {
        for id in ids(part) {
            home.entry(id).or_insert_with(|| chapter_file(n + 1));
        }
    }
    let documents: Vec<(String, String)> = parts
        .iter()
        .enumerate()
        .map(|(n, part)| {
            let file = chapter_file(n + 1);
            let body = relink(part, &file, &home);
            (file, xhtml(&title, &language, &body))
        })
        .collect();

    // The table of contents: the headings, nested by level.
    let mut nav = String::new();
    let mut depth = 0usize;
    for (level, id, words) in &rendered.headings {
        let level = (*level).max(1);
        let Some(file) = home.get(id) else { continue };
        while depth < level {
            nav.push_str("<ol>");
            depth += 1;
        }
        while depth > level {
            nav.push_str("</li></ol>");
            depth -= 1;
        }
        if nav.ends_with("</a>") {
            nav.push_str("</li>");
        }
        nav.push_str(&format!("<li><a href=\"{file}#{id}\">{words}</a>"));
    }
    if depth > 0 {
        nav.push_str("</li>");
        for _ in 1..depth {
            nav.push_str("</ol></li>");
        }
        nav.push_str("</ol>");
    } else {
        // A book with no headings lists its first chapter, as every
        // reader needs a way in.
        nav.push_str(&format!(
            "<ol><li><a href=\"{}\">{}</a></li></ol>",
            chapter_file(1),
            escape(&title)
        ));
    }
    let nav = xhtml(
        &title,
        &language,
        &format!("<nav epub:type=\"toc\" id=\"toc\">\n<h1>Contents</h1>\n{nav}\n</nav>\n"),
    );

    // The package: what the book is, what is in it, and the reading order.
    let mut manifest = String::new();
    manifest.push_str(
        "<item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n",
    );
    manifest.push_str(&format!(
        "<item id=\"css\" href=\"{STYLESHEET}\" media-type=\"text/css\"/>\n"
    ));
    let mut spine = String::new();
    for (n, (file, _)) in documents.iter().enumerate() {
        manifest.push_str(&format!(
            "<item id=\"c{}\" href=\"{file}\" media-type=\"application/xhtml+xml\"/>\n",
            n + 1
        ));
        spine.push_str(&format!("<itemref idref=\"c{}\"/>\n", n + 1));
    }
    for (n, (name, _)) in rendered.files.iter().enumerate() {
        manifest.push_str(&format!(
            "<item id=\"i{}\" href=\"{}\" media-type=\"{}\"/>\n",
            n + 1,
            escape(name),
            media_type(name)
        ));
    }
    let cover = options
        .cover
        .as_ref()
        .map(|(bytes, extension)| (format!("images/cover.{extension}"), bytes));
    if let Some((name, _)) = &cover {
        manifest.push_str(&format!(
            "<item id=\"cover\" href=\"{name}\" media-type=\"{}\" properties=\"cover-image\"/>\n",
            media_type(name)
        ));
    }
    let identifier = if options.identifier.trim().is_empty() {
        format!("urn:tessera:{}", crate::css::slug(&title))
    } else {
        options.identifier.clone()
    };
    let modified = if options.modified.trim().is_empty() {
        "2000-01-01T00:00:00Z".to_owned()
    } else {
        options.modified.clone()
    };
    let author = if options.author.trim().is_empty() {
        String::new()
    } else {
        format!("<dc:creator>{}</dc:creator>\n", escape(&options.author))
    };
    let package = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"uid\" xml:lang=\"{lang}\">\n\
         <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n\
         <dc:identifier id=\"uid\">{id}</dc:identifier>\n\
         <dc:title>{title}</dc:title>\n\
         <dc:language>{lang}</dc:language>\n\
         {author}\
         <meta property=\"dcterms:modified\">{modified}</meta>\n\
         </metadata>\n<manifest>\n{manifest}</manifest>\n<spine>\n{spine}</spine>\n</package>\n",
        lang = escape(&language),
        id = escape(&identifier),
        title = escape(&title),
        modified = escape(&modified),
    );
    let container = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
        <container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\n\
        <rootfiles><rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles>\n\
        </container>\n";

    // The ZIP: `mimetype` first and stored as it is, which is how a reader
    // knows what it has before it unpacks anything.
    let fail = |e: zip::result::ZipError| format!("Could not write the book: {e}");
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let add = |zip: &mut zip::ZipWriter<std::io::Cursor<Vec<u8>>>,
               name: &str,
               bytes: &[u8],
               options: zip::write::SimpleFileOptions|
     -> Result<(), String> {
        zip.start_file(name, options).map_err(fail)?;
        zip.write_all(bytes)
            .map_err(|e| format!("Could not write the book: {e}"))
    };
    add(&mut zip, "mimetype", b"application/epub+zip", stored)?;
    add(
        &mut zip,
        "META-INF/container.xml",
        container.as_bytes(),
        deflated,
    )?;
    add(&mut zip, "OEBPS/content.opf", package.as_bytes(), deflated)?;
    add(&mut zip, "OEBPS/nav.xhtml", nav.as_bytes(), deflated)?;
    add(
        &mut zip,
        &format!("OEBPS/{STYLESHEET}"),
        rendered.stylesheet.as_bytes(),
        deflated,
    )?;
    for (file, document) in &documents {
        add(
            &mut zip,
            &format!("OEBPS/{file}"),
            document.as_bytes(),
            deflated,
        )?;
    }
    for (name, bytes) in &rendered.files {
        add(&mut zip, &format!("OEBPS/{name}"), bytes, stored)?;
    }
    if let Some((name, bytes)) = &cover {
        add(&mut zip, &format!("OEBPS/{name}"), bytes, stored)?;
    }
    let bytes = zip.finish().map_err(fail)?.into_inner();
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_splits_at_its_first_level_headings() {
        let body =
            "<p>Front</p>\n<h1 id=\"h-1\">One</h1>\n<p>a</p>\n<h1 id=\"h-2\">Two</h1>\n<p>b</p>\n";
        let parts = chapters(body);
        assert_eq!(parts.len(), 3);
        assert!(parts[1].starts_with("<h1 id=\"h-1\">"));
        // Nothing before the first heading: no empty chapter.
        assert_eq!(chapters("<h1 id=\"h-1\">One</h1>\n<p>a</p>\n").len(), 1);
    }

    #[test]
    fn a_link_into_another_chapter_names_it() {
        let home: HashMap<String, String> = [
            ("a-x".to_owned(), "chapter-2.xhtml".to_owned()),
            ("fn-1".to_owned(), "chapter-1.xhtml".to_owned()),
        ]
        .into();
        let html = "<a href=\"#a-x\">x</a> <a href=\"#fn-1\">1</a> <a href=\"https://e\">e</a>";
        assert_eq!(
            relink(html, "chapter-1.xhtml", &home),
            "<a href=\"chapter-2.xhtml#a-x\">x</a> <a href=\"#fn-1\">1</a> <a href=\"https://e\">e</a>"
        );
    }
}
