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
        let stored = match book_path.parent() {
            Some(folder) => document
                .strip_prefix(folder)
                .map(Path::to_path_buf)
                .unwrap_or_else(|_| document.to_path_buf()),
            None => document.to_path_buf(),
        };
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
