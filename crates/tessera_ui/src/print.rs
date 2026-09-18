//! Printing, by way of a PDF.
//!
//! Tessera talks to no printer driver. What it does is what every layout
//! application's print path comes down to once the driver is peeled away:
//! the pages a person chose are written as a PDF — the same writer, the same
//! geometry, the same fonts as an export — and handed to the system's own
//! print path, which knows the printers, the trays and the duplexing, and
//! shows its own dialog for them. On Windows that is the shell's *print*
//! verb on the file; on macOS and Linux the file goes to the default
//! printer through `lpr`/`lp`, or opens in the viewer when there is none.
//!
//! Honest about what it is: File ▸ Print… says so in the dialog, and a person
//! who wants the press's own settings still exports and sends the file.

use std::path::{Path, PathBuf};

use tessera_layout::ResolvedDocument;

/// Which pages to print, in the reading order, one-based as a person
/// counts them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pages {
    All,
    /// Inclusive, one-based; a range past the end is cut to the end.
    Range {
        from: usize,
        to: usize,
    },
}

impl Pages {
    /// The zero-based indices of the pages to print, of `count`.
    pub fn indices(self, count: usize) -> Vec<usize> {
        match self {
            Pages::All => (0..count).collect(),
            Pages::Range { from, to } => {
                let from = from.max(1);
                let to = to.min(count);
                if from > to {
                    return Vec::new();
                }
                (from - 1..to).collect()
            }
        }
    }
}

/// `resolved` cut down to `pages`: the other pages gone, with the
/// bookmarks and links that named them, and the rest renumbered. The items
/// stay — the writer puts each on the page its geometry says, and one off
/// every kept page is on none.
pub fn only_pages(resolved: &ResolvedDocument, pages: Pages) -> ResolvedDocument {
    let keep = pages.indices(resolved.pages.len());
    let new_index = |old: usize| keep.iter().position(|k| *k == old);
    let mut out = ResolvedDocument {
        items: Vec::with_capacity(resolved.items.len()),
        pages: keep.iter().map(|k| resolved.pages[*k].clone()).collect(),
        bookmarks: Vec::new(),
    };
    for item in &resolved.items {
        let mut item = item.clone();
        item.links.retain_mut(|link| match &mut link.target {
            tessera_layout::LinkTarget::Page(index) => match new_index(*index) {
                Some(new) => {
                    *index = new;
                    true
                }
                None => false,
            },
            tessera_layout::LinkTarget::Url(_) => true,
        });
        out.items.push(item);
    }
    for bookmark in &resolved.bookmarks {
        if let Some(new) = new_index(bookmark.page) {
            let mut bookmark = bookmark.clone();
            bookmark.page = new;
            out.bookmarks.push(bookmark);
        }
    }
    out
}

/// Where the PDF to print is written: a file of its own in the system's
/// temporary folder, per process, so two windows printing at once do not
/// write over each other.
pub fn spool_path() -> PathBuf {
    std::env::temp_dir().join(format!("tessera-print-{}.pdf", std::process::id()))
}

/// The program and arguments that hand `pdf` to the system's print path
/// on this platform, and what a status line should say about it.
pub fn print_command(pdf: &Path) -> (String, Vec<String>, &'static str) {
    let path = pdf.to_string_lossy().into_owned();
    if cfg!(target_os = "windows") {
        // The shell's print verb: whatever opens PDFs here prints it, with
        // its own dialog or silently, as it is set up to.
        (
            "powershell".into(),
            vec![
                "-NoProfile".into(),
                "-Command".into(),
                format!(
                    "Start-Process -FilePath '{}' -Verb Print",
                    path.replace('\'', "''")
                ),
            ],
            "sent to the system's print path",
        )
    } else if cfg!(target_os = "macos") {
        ("lpr".into(), vec![path], "sent to the default printer")
    } else {
        ("lp".into(), vec![path], "sent to the default printer")
    }
}

/// Hand `pdf` to the system's print path. Returns what happened, for the
/// status line; an error is the system's, in its words.
pub fn send(pdf: &Path) -> Result<&'static str, String> {
    let (program, args, said) = print_command(pdf);
    std::process::Command::new(&program)
        .args(&args)
        .spawn()
        .map(|_| said)
        .map_err(|e| format!("could not run {program}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_range_is_one_based_inclusive_and_cut_to_the_document() {
        assert_eq!(Pages::All.indices(3), vec![0, 1, 2]);
        assert_eq!(Pages::Range { from: 2, to: 3 }.indices(5), vec![1, 2]);
        assert_eq!(
            Pages::Range { from: 0, to: 2 }.indices(5),
            vec![0, 1],
            "0 is 1"
        );
        assert_eq!(
            Pages::Range { from: 4, to: 99 }.indices(5),
            vec![3, 4],
            "cut to the end"
        );
        assert!(
            Pages::Range { from: 3, to: 2 }.indices(5).is_empty(),
            "backwards is nothing"
        );
        assert!(Pages::All.indices(0).is_empty());
    }

    #[test]
    fn only_the_chosen_pages_stay_and_what_named_the_others_goes() {
        use tessera_geometry::DocRect;
        use tessera_layout::resolve::{Bookmark, ResolvedPage};
        let page = |y: f64| ResolvedPage {
            bounds: DocRect {
                x: 0.0,
                y,
                width: 100.0,
                height: 100.0,
            },
            margins: DocRect {
                x: 0.0,
                y,
                width: 100.0,
                height: 100.0,
            },
            bleed: DocRect {
                x: 0.0,
                y,
                width: 100.0,
                height: 100.0,
            },
            slug: DocRect {
                x: 0.0,
                y,
                width: 100.0,
                height: 100.0,
            },
            columns: Vec::new(),
        };
        let resolved = ResolvedDocument {
            items: Vec::new(),
            pages: vec![page(0.0), page(200.0), page(400.0)],
            bookmarks: vec![
                Bookmark {
                    title: "One".into(),
                    page: 0,
                    level: 0,
                },
                Bookmark {
                    title: "Three".into(),
                    page: 2,
                    level: 0,
                },
            ],
        };
        let cut = only_pages(&resolved, Pages::Range { from: 2, to: 3 });
        assert_eq!(cut.pages.len(), 2);
        assert_eq!(cut.pages[0].bounds.y, 200.0);
        assert_eq!(cut.bookmarks.len(), 1, "the bookmark to page one went");
        assert_eq!(cut.bookmarks[0].title, "Three");
        assert_eq!(cut.bookmarks[0].page, 1, "and the rest are renumbered");
    }

    #[test]
    fn the_print_command_names_a_program_this_platform_has() {
        let (program, args, said) = print_command(Path::new("C:/tmp/it's.pdf"));
        assert!(!program.is_empty() && !args.is_empty() && !said.is_empty());
        if cfg!(target_os = "windows") {
            assert_eq!(program, "powershell");
            let command = args.last().unwrap();
            assert!(command.contains("-Verb Print"));
            assert!(
                command.contains("it''s.pdf"),
                "a quote in the path is doubled: {command}"
            );
        }
        assert!(spool_path().to_string_lossy().ends_with(".pdf"));
    }
}
