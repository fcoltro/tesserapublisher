//! A book: several documents that are one publication.
//!
//! What InDesign's book file is — a list of documents in order, so that the
//! chapters of a long work can be worked on as separate files and still be
//! numbered as one, listed in one contents, and exported as one PDF. The
//! book holds nothing of the documents themselves; it is the list, and
//! what the list means.
//!
//! Saved as JSON beside the documents, with each document's path kept
//! relative to the book when it can be — a book that moves with its folder
//! still finds its chapters.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The file extension a book is saved with.
pub const EXTENSION: &str = "tesserabook";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Book {
    /// The documents, in reading order. Relative paths are from the book's
    /// own folder.
    pub documents: Vec<PathBuf>,
    /// Whether each document's pages are numbered on from the one before,
    /// which is what a book is for.
    #[serde(default = "yes")]
    pub continue_numbering: bool,
}

fn yes() -> bool {
    true
}

/// `document` as a book at `book_path` stores it: relative to the book's
/// folder when it lies under it, as given otherwise.
fn relative_to(book_path: &Path, document: &Path) -> PathBuf {
    match book_path.parent() {
        Some(folder) => document
            .strip_prefix(folder)
            .map(Path::to_path_buf)
            .unwrap_or_else(|_| document.to_path_buf()),
        None => document.to_path_buf(),
    }
}

impl Default for Book {
    fn default() -> Self {
        Self {
            documents: Vec::new(),
            continue_numbering: true,
        }
    }
}

impl Book {
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        serde_json::from_str(&text).map_err(std::io::Error::other)
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        tessera_io::atomic::write_atomic(path, text.as_bytes()).map_err(std::io::Error::other)
    }

    /// Add a document, kept relative to `book_path`'s folder when it lies
    /// under it. Already listed: left where it is.
    pub fn add(&mut self, book_path: &Path, document: &Path) {
        let stored = relative_to(book_path, document);
        if !self.documents.contains(&stored) {
            self.documents.push(stored);
        }
    }

    /// Every document's path, made absolute against `book_path`'s folder.
    pub fn resolved(&self, book_path: &Path) -> Vec<PathBuf> {
        let folder = book_path.parent();
        self.documents
            .iter()
            .map(|p| match folder {
                Some(folder) if p.is_relative() => folder.join(p),
                _ => p.clone(),
            })
            .collect()
    }

    /// Move the document at `from` to stand at `to` in the list as it is
    /// before the move — counted as a drop marker is drawn, so `to` of the
    /// list's length is the end. Whether anything moved.
    pub fn move_to(&mut self, from: usize, to: usize) -> bool {
        if from >= self.documents.len() || to > self.documents.len() {
            return false;
        }
        let at = if to > from { to - 1 } else { to };
        if at == from {
            return false;
        }
        let document = self.documents.remove(from);
        self.documents.insert(at, document);
        true
    }

    /// Point the entry at `index` at another file — a chapter found again
    /// after its folder moved — kept relative as [`Book::add`] keeps one.
    /// Refused when the file is already another entry.
    pub fn locate(&mut self, book_path: &Path, index: usize, document: &Path) -> bool {
        let stored = relative_to(book_path, document);
        if index >= self.documents.len()
            || self
                .documents
                .iter()
                .enumerate()
                .any(|(i, d)| i != index && *d == stored)
        {
            return false;
        }
        self.documents[index] = stored;
        true
    }

    /// Take the entry at `index` out of the book. The file is not touched.
    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.documents.len() {
            return false;
        }
        self.documents.remove(index);
        true
    }

    /// Move the document at `index` one place earlier or later.
    pub fn shift(&mut self, index: usize, later: bool) -> bool {
        let to = if later {
            index + 1
        } else {
            match index.checked_sub(1) {
                Some(to) => to,
                None => return false,
            }
        };
        if index >= self.documents.len() || to >= self.documents.len() {
            return false;
        }
        self.documents.swap(index, to);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_book_keeps_its_chapters_relative_and_finds_them_again() {
        let folder = std::env::temp_dir().join(format!("tessera-book-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let book_path = folder.join("novel.tesserabook");
        let mut book = Book::default();
        book.add(&book_path, &folder.join("one.tsrdf"));
        book.add(&book_path, &folder.join("two.tsrdf"));
        book.add(&book_path, &folder.join("one.tsrdf"));
        assert_eq!(book.documents.len(), 2, "listed once");
        assert!(book.documents[0].is_relative(), "kept relative to the book");
        // A chapter outside the book's folder: kept as given, absolute.
        let elsewhere = folder
            .parent()
            .unwrap()
            .join("elsewhere")
            .join("three.tsrdf");
        book.add(&book_path, &elsewhere);
        assert!(
            book.documents[2].is_absolute(),
            "outside the folder: as given"
        );

        book.save(&book_path).unwrap();
        let back = Book::load(&book_path).unwrap();
        assert_eq!(back, book);
        let found = back.resolved(&book_path);
        assert_eq!(found[0], folder.join("one.tsrdf"));
        assert_eq!(found[2], elsewhere);

        assert!(back.continue_numbering, "a book numbers on by default");
        let _ = std::fs::remove_dir_all(&folder);
    }

    fn abc() -> Book {
        Book {
            documents: vec!["a".into(), "b".into(), "c".into()],
            ..Default::default()
        }
    }

    fn order(book: &Book) -> String {
        book.documents
            .iter()
            .map(|d| d.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn a_chapter_dragged_lands_where_it_was_dropped() {
        let mut book = abc();
        assert!(book.move_to(0, 3), "the first to the end");
        assert_eq!(order(&book), "bca");
        assert!(book.move_to(2, 0), "the last to the front");
        assert_eq!(order(&book), "abc");
        assert!(book.move_to(0, 2), "into the gap before the third");
        assert_eq!(order(&book), "bac");
        assert!(
            !book.move_to(1, 1) && !book.move_to(1, 2),
            "dropped where it was"
        );
        assert!(!book.move_to(5, 0) && !book.move_to(0, 9), "nothing there");
    }

    #[test]
    fn a_missing_chapter_is_found_again_and_a_chapter_taken_out() {
        let folder = std::env::temp_dir().join("tessera-book-locate");
        let book_path = folder.join("b.tesserabook");
        let mut book = abc();
        assert!(book.locate(&book_path, 1, &folder.join("moved").join("b2.tsrdf")));
        assert_eq!(
            book.documents[1],
            PathBuf::from("moved/b2.tsrdf"),
            "kept relative"
        );
        assert!(
            !book.locate(&book_path, 1, &folder.join("a")),
            "not a second entry for a file already listed"
        );
        assert!(!book.locate(&book_path, 7, &folder.join("x")));
        assert!(book.remove(0));
        assert_eq!(book.documents.len(), 2);
        assert!(!book.remove(2));
    }

    #[test]
    fn a_chapter_moves_one_place_and_not_off_the_end() {
        let mut book = Book {
            documents: vec!["a".into(), "b".into(), "c".into()],
            ..Default::default()
        };
        assert!(book.shift(0, true));
        assert_eq!(
            book.documents,
            vec![PathBuf::from("b"), "a".into(), "c".into()]
        );
        assert!(!book.shift(0, false), "nothing before the first");
        assert!(!book.shift(2, true), "nothing after the last");
        assert!(!book.shift(9, true), "nothing there at all");
    }
}
