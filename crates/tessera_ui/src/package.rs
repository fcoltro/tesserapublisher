//! Collecting a job into one folder somebody can hand over.
//!
//! A layout is not a deliverable. It points at photographs on a desktop and
//! fonts in a system folder, and a printer who receives only the file receives a
//! document full of missing links. Packaging is the operation that turns "my
//! document" into "the job".
//!
//! ## What is copied, and what is only reported
//!
//! **Links are copied.** They are the customer's own artwork and there is no
//! question about moving it to the printer who is going to print it.
//!
//! **Fonts are not.** A font is licensed software, and a licence to *set type*
//! is not a licence to redistribute the file — outline embedding in a PDF is
//! explicitly permitted by most foundries and handing over the `.otf` is
//! explicitly not. InDesign copies them and puts a licence warning in the way;
//! Tessera lists them instead, with what a printer would need in order to have
//! them already. The PDF carries subsetted outlines, which is what actually
//! makes the job printable.
//!
//! That is a deliberate difference from InDesign, and it is recorded rather than
//! silent: the summary says which fonts the job uses so the question can be
//! asked, and it says why they are not in the folder.

use std::path::{Path, PathBuf};

use tessera_document::document::Document;

/// What a packaging run produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Packaged {
    pub folder: PathBuf,
    /// Files copied into `Links`.
    pub links: Vec<PathBuf>,
    /// Links that could not be copied, and why.
    ///
    /// **Reported, not fatal.** A job with one missing photograph still needs
    /// packaging — the printer wants everything else, and the studio needs the
    /// list of what to chase.
    pub missing: Vec<String>,
    /// Font families the document sets type in.
    pub fonts: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum PackageError {
    #[error("could not create {0}")]
    Folder(PathBuf),
    #[error("could not write {0}")]
    Write(PathBuf),
}

/// Collect a document, its links and a summary into `folder`.
///
/// The document is written first: if a link copy fails halfway, the folder still
/// holds a readable job rather than a folder of photographs and no layout.
pub fn collect(
    doc: &Document,
    document_name: &str,
    folder: &Path,
    report: &tessera_preflight::Report,
) -> Result<Packaged, PackageError> {
    let links_folder = folder.join("Links");
    std::fs::create_dir_all(&links_folder)
        .map_err(|_| PackageError::Folder(links_folder.clone()))?;

    // The document itself, first.
    let document_path = folder.join(format!("{document_name}.tessera"));
    tessera_document::format::save(doc, &document_path)
        .map_err(|_| PackageError::Write(document_path.clone()))?;

    let mut copied = Vec::new();
    let mut missing = Vec::new();

    for link in doc.links.values() {
        let name = link
            .path
            .file_name()
            .map(|n| n.to_owned())
            .unwrap_or_else(|| link.path.as_os_str().to_owned());
        let target = links_folder.join(&name);

        // Already copied: two frames placing one file is one file, and the
        // second copy would be the same bytes under the same name.
        if target.exists() {
            continue;
        }

        match std::fs::copy(&link.path, &target) {
            Ok(_) => copied.push(target),
            Err(error) => missing.push(format!("{}: {error}", link.path.display())),
        }
    }

    let fonts = families(doc);
    let summary = summarise(doc, document_name, &copied, &missing, &fonts, report);
    let summary_path = folder.join("Instructions.txt");
    std::fs::write(&summary_path, summary).map_err(|_| PackageError::Write(summary_path))?;

    Ok(Packaged {
        folder: folder.to_path_buf(),
        links: copied,
        missing,
        fonts,
    })
}

/// Every font family the document sets type in.
///
/// **`tessera_preflight::fonts`, not a second walk of the same document.** This
/// used to have its own, and two walks over one fact drift: the way they drift
/// is that one of them forgets the run-local families, which is the half a
/// printer would have been misled about.
use tessera_preflight::fonts::families;

/// The note a printer reads first.
///
/// Plain text on purpose. It is opened on a machine nobody here chose, possibly
/// by a person whose job is to check a folder and pass it on, and a format that
/// needs an application is a format that gets skipped.
fn summarise(
    doc: &Document,
    name: &str,
    links: &[PathBuf],
    missing: &[String],
    fonts: &[String],
    report: &tessera_preflight::Report,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("{name}\n"));
    out.push_str(&"=".repeat(name.len()));
    out.push_str("\n\nPacked by Tessera Publisher.\n\n");

    // Dimensions, because the first thing a printer checks is whether the page
    // is the size the job was quoted for.
    out.push_str("Pages\n-----\n");
    let pages = doc.page_ids().count();
    out.push_str(&format!(
        "{pages} page{}\n",
        if pages == 1 { "" } else { "s" }
    ));
    if let Some(first) = doc.page_ids().next()
        && let Some(page) = doc.pages.get(first)
    {
        out.push_str(&format!(
            "Trim {:.1} x {:.1} mm\n",
            page.bounds.width / 72.0 * 25.4,
            page.bounds.height / 72.0 * 25.4
        ));
    }
    let bleed = doc.setup.bleed;
    if bleed.top > 0.0 || bleed.left > 0.0 {
        out.push_str(&format!("Bleed {:.1} mm\n", bleed.top / 72.0 * 25.4));
    } else {
        out.push_str("No bleed set\n");
    }

    out.push_str("\nInks\n----\n");
    match &doc.output_intent {
        Some(intent) => out.push_str(&format!("Prepared for {}\n", intent.description)),
        None => out.push_str(
            "No output intent. Colour has not been converted for any \
             particular press.\n",
        ),
    }
    let spots = spot_names(doc);
    if spots.is_empty() {
        out.push_str("No spot colours\n");
    } else {
        out.push_str(&format!("Spot colours: {}\n", spots.join(", ")));
    }

    out.push_str("\nLinks\n-----\n");
    out.push_str(&format!("{} copied into Links/\n", links.len()));
    if !missing.is_empty() {
        out.push_str("\nCOULD NOT BE COPIED:\n");
        for line in missing {
            out.push_str(&format!("  {line}\n"));
        }
    }

    out.push_str("\nFonts\n-----\n");
    if fonts.is_empty() {
        out.push_str("None used\n");
    } else {
        for family in fonts {
            out.push_str(&format!("  {family}\n"));
        }
    }
    out.push_str(
        "\nFont files are NOT included. A licence to set type is not a licence\n\
         to pass the font on. Outlines are embedded and subsetted in the\n\
         exported PDF, which is what makes the job printable.\n",
    );

    out.push_str("\nPreflight\n---------\n");
    out.push_str(&format!("{}\n", report.summary()));
    for problem in &report.problems {
        out.push_str(&format!(
            "  [{}] {}\n",
            problem.severity().label(),
            problem.message
        ));
    }
    if report.problems.is_empty() {
        out.push_str("Nothing Tessera checks for. A hand check is still worth doing.\n");
    }

    out
}

/// The spot inks the document uses, which decide how many plates a job needs.
fn spot_names(doc: &Document) -> Vec<String> {
    let mut out: Vec<String> = doc
        .swatches
        .iter()
        .filter(|s| s.spot)
        .map(|s| s.name.clone())
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let at = std::env::temp_dir().join(format!("tessera-package-{name}"));
        let _ = std::fs::remove_dir_all(&at);
        at
    }

    #[test]
    fn a_package_holds_the_document_the_links_folder_and_a_summary() {
        let folder = scratch("shape");
        let doc = Document::new();
        let packaged = collect(&doc, "Job", &folder, &Default::default()).expect("packaged");

        assert!(folder.join("Job.tessera").is_file(), "no document");
        assert!(folder.join("Links").is_dir(), "no links folder");
        assert!(folder.join("Instructions.txt").is_file(), "no summary");
        assert_eq!(packaged.links.len(), 0);

        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_link_that_cannot_be_copied_is_reported_rather_than_fatal() {
        // A job with one missing photograph still needs packaging: the printer
        // wants everything else, and the studio needs the list to chase.
        let folder = scratch("missing");
        let mut doc = Document::new();
        doc.add_link(tessera_document::links::Link {
            path: PathBuf::from("nothing-of-this-name.jpg"),
            natural: (100.0, 100.0),
            modified: None,
        });

        let packaged = collect(&doc, "Job", &folder, &Default::default()).expect("packaged");
        assert_eq!(packaged.missing.len(), 1);
        assert!(
            folder.join("Job.tessera").is_file(),
            "the document still went"
        );

        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_link_placed_twice_is_copied_once() {
        let folder = scratch("twice");
        let source = std::env::temp_dir().join("tessera-package-source.txt");
        std::fs::write(&source, b"artwork").expect("write");

        let mut doc = Document::new();
        for _ in 0..2 {
            doc.add_link(tessera_document::links::Link {
                path: source.clone(),
                natural: (10.0, 10.0),
                modified: None,
            });
        }

        let packaged = collect(&doc, "Job", &folder, &Default::default()).expect("packaged");
        assert_eq!(packaged.links.len(), 1, "the same file was copied twice");

        let _ = std::fs::remove_file(&source);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_summary_says_fonts_are_not_included_and_why() {
        // A deliberate difference from InDesign, and one a printer has to know
        // about rather than discover.
        let folder = scratch("fonts");
        let doc = Document::new();
        collect(&doc, "Job", &folder, &Default::default()).expect("packaged");

        let summary =
            std::fs::read_to_string(folder.join("Instructions.txt")).expect("the summary");
        assert!(summary.contains("NOT included"));
        assert!(summary.contains("licence"));
        assert!(
            summary.contains("subsetted"),
            "it must say what does make the job printable"
        );

        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_summary_reports_the_preflight_state() {
        // A folder that says "no problems" when it was never checked is worse
        // than one that says nothing.
        let folder = scratch("preflight");
        let doc = Document::new();
        let report = tessera_preflight::Report {
            problems: vec![tessera_preflight::Problem {
                rule: tessera_preflight::Rule::MissingLink,
                message: "Bridge.jpg is not where the document expects it".to_string(),
                at: tessera_preflight::Where::Document,
            }],
        };
        collect(&doc, "Job", &folder, &report).expect("packaged");

        let summary =
            std::fs::read_to_string(folder.join("Instructions.txt")).expect("the summary");
        assert!(summary.contains("Bridge.jpg"));
        assert!(summary.contains("Error"));

        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_summary_names_the_press_or_says_there_is_none() {
        // The first thing a printer checks after the page size.
        let folder = scratch("press");
        let doc = Document::new();
        collect(&doc, "Job", &folder, &Default::default()).expect("packaged");

        let summary =
            std::fs::read_to_string(folder.join("Instructions.txt")).expect("the summary");
        assert!(summary.contains("No output intent"));

        let _ = std::fs::remove_dir_all(&folder);
    }
}
