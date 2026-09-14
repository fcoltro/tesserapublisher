//! Check spelling.
//!
//! The classic box: one unknown word at a time, in a sentence of context,
//! with Ignore, Ignore all, Add to dictionary, and Change to whatever was
//! typed. It walks the story being edited, or every story in the document
//! when nothing is, in reading order of the stories as the document holds
//! them.
//!
//! ## Where the words come from
//!
//! Hunspell dictionaries in Tessera's dictionaries folder — beside the
//! preferences file — named by language: `en.dic` and `en.aff`, `de.dic`
//! and `de.aff`. LibreOffice's and Firefox's are that format, and there is
//! no dictionary bundled, because the word lists are large and each has its
//! own licence. A text whose language has no dictionary is said so, not
//! checked against English.
//!
//! Words added go to `user.dic` in the same folder, one per line, and are
//! read back for every language.

use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;

use tessera_document::ids::StoryId;
use tessera_text::spell::{Dictionary, words};

use crate::app::TesseraApp;
use crate::command::{Command, apply};
use crate::theme::Theme;

/// The dictionaries loaded this session, by language code.
#[derive(Default)]
pub struct Dictionaries {
    loaded: HashMap<String, Option<Dictionary>>,
    /// The person's own words, kept across languages.
    added: Vec<String>,
}

impl std::fmt::Debug for Dictionaries {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dictionaries")
            .field("languages", &self.loaded.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl Dictionaries {
    /// Where the `.dic` and `.aff` files live.
    pub fn folder() -> Option<PathBuf> {
        crate::prefs::Preferences::directory().map(|d| d.join("dictionaries"))
    }

    fn user_list() -> Option<PathBuf> {
        Self::folder().map(|d| d.join("user.dic"))
    }

    /// The dictionary for `language`, loading it the first time it is asked
    /// for. `None` when the folder has no files for it.
    pub fn get(&mut self, language: &str) -> Option<&Dictionary> {
        let language = language.to_ascii_lowercase();
        if !self.loaded.contains_key(&language) {
            let loaded = Self::load(&language).map(|mut d| {
                for word in &self.added {
                    d.add(word);
                }
                d
            });
            self.loaded.insert(language.clone(), loaded);
        }
        self.loaded.get(&language).and_then(|d| d.as_ref())
    }

    fn load(language: &str) -> Option<Dictionary> {
        let folder = Self::folder()?;
        // "en" may be shipped as en.dic or en_US.dic; take the exact name
        // first, then anything starting with it.
        let candidates: Vec<PathBuf> = std::fs::read_dir(&folder)
            .ok()?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "dic"))
            .filter(|p| {
                p.file_stem().and_then(|s| s.to_str()).is_some_and(|s| {
                    let s = s.to_ascii_lowercase();
                    s == language && s != "user" || s.starts_with(&format!("{language}_"))
                })
            })
            .collect();
        let dic = candidates
            .iter()
            .find(|p| {
                p.file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.eq_ignore_ascii_case(language))
            })
            .or_else(|| candidates.first())?;
        let aff = dic.with_extension("aff");
        let dic_text = std::fs::read_to_string(dic).ok()?;
        let aff_text = std::fs::read_to_string(&aff).unwrap_or_default();
        let dictionary = Dictionary::parse(&aff_text, &dic_text);
        (!dictionary.is_empty()).then_some(dictionary)
    }

    /// Use `dictionary` for `language` without reading the folder — what a
    /// test does, and what a future "choose a dictionary file" would.
    pub fn insert(&mut self, language: &str, dictionary: Dictionary) {
        self.loaded
            .insert(language.to_ascii_lowercase(), Some(dictionary));
    }

    /// Read the person's own words once.
    pub fn load_user_words(&mut self) {
        if let Some(path) = Self::user_list()
            && let Ok(text) = std::fs::read_to_string(path)
        {
            self.added = text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_owned)
                .collect();
        }
    }

    /// Vouch for `word` from now on, in every language, and remember it.
    pub fn add(&mut self, word: &str) {
        let word = word.trim().to_owned();
        if word.is_empty() || self.added.contains(&word) {
            return;
        }
        self.added.push(word.clone());
        for d in self.loaded.values_mut().flatten() {
            d.add(&word);
        }
        if let Some(path) = Self::user_list() {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&path, self.added.join("\n") + "\n");
        }
    }
}

/// One unknown word, where it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub story: StoryId,
    pub range: Range<usize>,
    pub word: String,
    /// The sentence around it, for the box.
    pub context: String,
    pub language: String,
}

#[derive(Debug, Default)]
pub struct SpellingWindow {
    pub open: bool,
    /// The stories to walk, and how far the walk has got.
    stories: Vec<StoryId>,
    at: Option<(usize, usize)>,
    pub current: Option<Finding>,
    pub replacement: String,
    ignored: Vec<String>,
    /// Said once, when a language has no dictionary.
    pub missing: Vec<String>,
    pub finished: bool,
    pub checked: usize,
}

impl SpellingWindow {
    /// Start over: the story being edited, or every story.
    pub fn open(&mut self, state: &TesseraApp) {
        let editing = state.active().editing.as_ref().and_then(|(id, _)| {
            crate::view::viewport::editing_story(state, *id, state.active().editing_cell)
        });
        self.stories = match editing {
            Some(story) => vec![story],
            None => state.active().document().stories.keys().collect(),
        };
        self.at = Some((0, 0));
        self.current = None;
        self.replacement.clear();
        self.ignored.clear();
        self.missing.clear();
        self.finished = false;
        self.checked = 0;
        self.open = true;
    }

    /// The next unknown word from where the walk stopped, or none.
    fn next(&mut self, state: &mut TesseraApp) -> Option<Finding> {
        let (mut si, mut offset) = self.at?;
        while si < self.stories.len() {
            let id = self.stories[si];
            let Some(story) = state.active().document().story(id).cloned() else {
                si += 1;
                offset = 0;
                continue;
            };
            // The language of every word, read before the dictionaries are
            // borrowed to check them: the two live on the same state.
            let languages: Vec<(Range<usize>, String)> = words(&story.text)
                .into_iter()
                .map(|(range, _)| {
                    let language = story
                        .common_format(range.clone(), state.active().document())
                        .language
                        .unwrap_or_else(|| "en".to_owned());
                    (range, language)
                })
                .collect();
            for ((range, word), (_, language)) in words(&story.text).into_iter().zip(languages) {
                if range.start < offset {
                    continue;
                }
                if tessera_text::variables::Marker::of(word.chars().next().unwrap_or(' ')).is_some()
                {
                    continue;
                }
                if self.ignored.iter().any(|w| w.eq_ignore_ascii_case(word)) {
                    continue;
                }
                let known = match state.dictionaries.get(&language) {
                    Some(d) => d.check(word),
                    None => {
                        if !self.missing.contains(&language) {
                            self.missing.push(language.clone());
                        }
                        true
                    }
                };
                self.checked += 1;
                if !known {
                    self.at = Some((si, range.end));
                    return Some(Finding {
                        story: id,
                        range: range.clone(),
                        word: word.to_owned(),
                        context: context_of(&story.text, &range),
                        language,
                    });
                }
            }
            si += 1;
            offset = 0;
        }
        self.at = None;
        None
    }
}

/// The sentence-ish stretch around a word: up to forty characters either
/// side, cut at word edges.
fn context_of(text: &str, range: &Range<usize>) -> String {
    let start = text[..range.start]
        .char_indices()
        .rev()
        .take(40)
        .last()
        .map_or(range.start, |(i, _)| i);
    let end = text[range.end..]
        .char_indices()
        .take(40)
        .last()
        .map_or(range.end, |(i, c)| range.end + i + c.len_utf8());
    let before = text[start..range.start].replace('\n', " ");
    let after = text[range.end..end].replace('\n', " ");
    // An ellipsis only where the text goes on past what is shown.
    let lead = if start > 0 { "\u{2026}" } else { "" };
    let tail = if end < text.len() { "\u{2026}" } else { "" };
    format!(
        "{lead}{}[{}]{}{tail}",
        before.trim_start(),
        &text[range.clone()],
        after.trim_end()
    )
}

pub fn show(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.spelling.open {
        return;
    }
    // Walk to the first finding when the box has just opened.
    if state.spelling.current.is_none() && !state.spelling.finished {
        let mut window = std::mem::take(&mut state.spelling);
        window.current = window.next(state);
        if window.current.is_none() {
            window.finished = true;
        } else if let Some(f) = &window.current {
            window.replacement = f.word.clone();
        }
        state.spelling = window;
    }

    let mut action: Option<Action> = None;
    let mut close = false;
    let missing = state.spelling.missing.clone();
    let finished = state.spelling.finished;
    let checked = state.spelling.checked;
    let current = state.spelling.current.clone();
    let mut replacement = state.spelling.replacement.clone();

    let response = egui::Modal::new(egui::Id::new("check-spelling"))
        .frame(super::dialog_frame(ctx))
        .show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(340.0, 520.0));
            ui.heading("Check spelling");
            ui.add_space(Theme::space_2());
            if !missing.is_empty() {
                let folder = Dictionaries::folder()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                ui.colored_label(
                    Theme::text_muted(),
                    format!(
                        "No dictionary for {}. Put a Hunspell .dic and .aff named by the language in {folder}",
                        missing.join(", ")
                    ),
                );
                ui.add_space(Theme::space_1());
            }
            match &current {
                Some(finding) => {
                    ui.label(egui::RichText::new(&finding.word).strong().size(18.0));
                    ui.colored_label(Theme::text_muted(), &finding.context);
                    ui.add_space(Theme::space_1());
                    crate::view::panels::field(ui, "Change to", |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut replacement)
                                .desired_width(f32::INFINITY),
                        );
                    });
                    ui.add_space(Theme::space_2());
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .add_enabled(
                                !replacement.trim().is_empty() && replacement != finding.word,
                                super::primary_button("Change"),
                            )
                            .clicked()
                        {
                            action = Some(Action::Change);
                        }
                        if ui.button("Ignore").clicked() {
                            action = Some(Action::Ignore);
                        }
                        if ui.button("Ignore all").clicked() {
                            action = Some(Action::IgnoreAll);
                        }
                        if ui.button("Add to dictionary").clicked() {
                            action = Some(Action::Add);
                        }
                    });
                }
                None if finished => {
                    ui.label(format!(
                        "Done. {checked} word{} checked.",
                        if checked == 1 { "" } else { "s" }
                    ));
                }
                None => {}
            }
            ui.add_space(Theme::space_2());
            if ui.button(if finished { "Close" } else { "Stop" }).clicked() {
                close = true;
            }
        });
    state.spelling.replacement = replacement;
    if response.should_close() || close {
        state.spelling.open = false;
        state.spelling.current = None;
        return;
    }

    if let (Some(action), Some(finding)) = (action, current) {
        match action {
            Action::Change => {
                let to = state.spelling.replacement.trim().to_owned();
                // The edit closes any editing session, as Find and Change
                // does: the buffer holds its own copy of the story.
                state.active_mut().editing = None;
                apply(
                    state,
                    Command::ReplaceMatches {
                        edits: vec![(finding.story, finding.range.clone(), to.clone())],
                    },
                );
                // The walk resumes after the replacement, whose length may
                // differ from the word's.
                state.spelling.at = state
                    .spelling
                    .at
                    .map(|(si, _)| (si, finding.range.start + to.len()));
            }
            Action::Ignore => {}
            Action::IgnoreAll => state.spelling.ignored.push(finding.word.clone()),
            Action::Add => {
                state.dictionaries.add(&finding.word);
            }
        }
        let mut window = std::mem::take(&mut state.spelling);
        window.current = window.next(state);
        match &window.current {
            Some(f) => window.replacement = f.word.clone(),
            None => window.finished = true,
        }
        state.spelling = window;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Change,
    Ignore,
    IgnoreAll,
    Add,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_document::nodes::FrameKind;
    use tessera_geometry::DocRect;

    #[test]
    fn the_context_brackets_the_word() {
        let text = "The quick brown fox jumps over the lazy dog.";
        assert!(context_of(text, &(16..19)).contains("[fox]"));
    }

    fn a_document_saying(text: &str) -> (TesseraApp, StoryId) {
        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::AddTextFrame(DocRect {
                x: 20.0,
                y: 20.0,
                width: 300.0,
                height: 100.0,
            }),
        );
        let id = state.active().selection.single().expect("selected");
        apply(
            &mut state,
            Command::SetText {
                id,
                text: text.to_string(),
            },
        );
        let FrameKind::Text { story, .. } = state.active().document().frame(id).unwrap().kind
        else {
            panic!()
        };
        state.dictionaries.insert(
            "en",
            Dictionary::parse(
                "SFX S Y 1\nSFX S 0 s .\n",
                "6\nthe\ncat/S\nsat\nset/S\ntype\ndoes\n",
            ),
        );
        (state, story)
    }

    #[test]
    fn the_walk_finds_the_unknown_word_and_changing_it_moves_on() {
        let (mut state, story) = a_document_saying("The cta sat. The cats sat.");
        let mut window = SpellingWindow::default();
        window.open(&state);
        let first = window.next(&mut state).expect("one unknown word");
        assert_eq!(first.word, "cta");
        assert_eq!(first.range, 4..7);
        assert_eq!(first.language, "en");
        assert!(first.context.contains("[cta]"));

        apply(
            &mut state,
            Command::ReplaceMatches {
                edits: vec![(story, first.range.clone(), "cat".into())],
            },
        );
        window.at = window.at.map(|(si, _)| (si, first.range.start + 3));
        assert_eq!(window.next(&mut state), None, "nothing else is wrong");
        assert_eq!(
            state.active().document().story(story).unwrap().text,
            "The cat sat. The cats sat."
        );
        assert!(window.missing.is_empty());
    }

    #[test]
    fn ignore_all_and_add_both_silence_a_word() {
        let (mut state, _) = a_document_saying("Tessera sets type. Tessera does.");
        let mut window = SpellingWindow::default();
        window.open(&state);
        let first = window.next(&mut state).expect("unknown");
        assert_eq!(first.word, "Tessera");
        window.ignored.push("Tessera".into());
        assert_eq!(window.next(&mut state), None, "ignored everywhere");

        let (mut state, _) = a_document_saying("Tessera sets type. Tessera does.");
        state.dictionaries.add("Tessera");
        let mut window = SpellingWindow::default();
        window.open(&state);
        assert_eq!(window.next(&mut state), None, "added to the dictionary");
    }

    #[test]
    fn a_language_with_no_dictionary_is_said_and_not_checked() {
        let (mut state, story) = a_document_saying("Guten Tag");
        apply(
            &mut state,
            Command::SetCharacterFormat {
                story,
                range: 0..9,
                format: tessera_text::story::CharacterFormat {
                    language: Some("de".into()),
                    ..Default::default()
                },
            },
        );
        let mut window = SpellingWindow::default();
        window.open(&state);
        assert_eq!(window.next(&mut state), None);
        assert_eq!(window.missing, vec!["de".to_owned()]);
    }
}
