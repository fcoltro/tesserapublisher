//! A document as a web page: InDesign's File ▸ Export ▸ HTML.
//!
//! **The words, not the page.** A web page reflows to whatever window it is
//! read in, so what goes out is the document's content in reading order —
//! its stories, pictures and tables — with its paragraph and character
//! styles as CSS classes, not a picture of each page. Where a page number
//! would go there is no page, so page-number markers go out as nothing and a
//! cross-reference reads as the paragraph it points at.
//!
//! **Reading order** is the page order, and on a page top to bottom, then
//! left to right — InDesign's "based on page layout". A story threaded
//! through several frames goes out once, where its first frame is; an object
//! anchored in text goes out where its marker is.

mod css;
mod order;
mod pictures;
mod text;

use tessera_document::document::Document;

pub use order::Block;
pub use pictures::ImageFormat;

/// What an export is asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    /// The page's title; the document's file name when `None`.
    pub title: Option<String>,
    /// Whether the styles go inside the page, in a `<style>` element, rather
    /// than in a stylesheet beside it that the page links to.
    pub inline_css: bool,
    /// How finely pictures are rendered, in pixels an inch of the page.
    pub ppi: f64,
    pub images: ImageFormat,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            title: None,
            inline_css: false,
            // Sharp on a high-density screen at the size the page shows it.
            ppi: 150.0,
            images: ImageFormat::Automatic,
        }
    }
}

/// The folder the pictures go in, beside the page.
pub const IMAGES: &str = "images";

/// What an export made: the page, its stylesheet, and the files it links
/// to, each with the path the page names it by.
#[derive(Debug, Clone, Default)]
pub struct Exported {
    pub html: String,
    /// Empty when the styles went inside the page.
    pub css: String,
    pub files: Vec<(String, Vec<u8>)>,
}

/// The name the stylesheet is linked by.
pub const STYLESHEET: &str = "style.css";

/// Export `doc` as one web page.
pub fn export(doc: &Document, options: &Options) -> Exported {
    let classes = css::Classes::of(doc);
    let mut body = String::new();
    let mut writer = text::Writer::new(doc, &classes, options);
    for block in order::reading_order(doc) {
        writer.block(&block, &mut body);
    }
    let stylesheet = classes.stylesheet(doc);

    let title = options
        .title
        .clone()
        .unwrap_or_else(|| doc.file_facts().file_name(false, false));
    let title = if title.trim().is_empty() {
        "Untitled".to_owned()
    } else {
        title
    };
    let language = text::document_language(doc);
    let mut html = String::new();
    html.push_str("<!DOCTYPE html>\n");
    html.push_str(&format!(
        "<html lang=\"{}\">\n<head>\n",
        text::escape(&language)
    ));
    html.push_str("<meta charset=\"utf-8\">\n");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    html.push_str(&format!("<title>{}</title>\n", text::escape(&title)));
    let css = if options.inline_css {
        html.push_str("<style>\n");
        html.push_str(&stylesheet);
        html.push_str("</style>\n");
        String::new()
    } else {
        html.push_str(&format!(
            "<link rel=\"stylesheet\" href=\"{STYLESHEET}\">\n"
        ));
        stylesheet
    };
    html.push_str("</head>\n<body>\n");
    html.push_str(&body);
    html.push_str("</body>\n</html>\n");
    Exported {
        html,
        css,
        files: writer.into_files(),
    }
}
