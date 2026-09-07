//! Shortcuts, described once.
//!
//! Until now a shortcut was written down **twice**: `actions.rs` carried
//! `Some("Ctrl+N")` as a label, and the accelerator handler separately matched
//! `pressed(cmd, Key::N)`. Two descriptions of one fact, and the usual
//! consequence — they could disagree, and no test could tell, because a menu
//! showing the wrong chord looks exactly like a menu showing the right one.
//!
//! It is also why remapping was impossible. Changing the string changed the
//! label and nothing else.
//!
//! Here the written chord is the only description. The label is it, and the key
//! the handler matches is it parsed. A malformed chord is a test failure
//! ([`every_shortcut_parses`]) rather than a shortcut that silently never fires.
//!
//! ## "Ctrl" is written, the platform's key is meant
//!
//! Every chord in the table is written with `Ctrl`, and parses to
//! [`egui::Modifiers::COMMAND`] — which is Ctrl on Windows and Linux and ⌘ on
//! macOS. [`Chord::label`] shows whichever one this machine actually has. A Mac
//! user pressing ⌘S is not doing something different from a Windows user
//! pressing Ctrl+S, and two entries in the table would say they were.

use std::collections::BTreeMap;
use std::fmt;

use egui::{Key, Modifiers};

/// A chord: some modifiers and a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub modifiers: Modifiers,
    pub key: Key,
}

impl Chord {
    /// Read a chord as it is written in the action table: `Ctrl+Shift+S`.
    ///
    /// Returns `None` for anything unrecognised rather than guessing. A chord
    /// nobody can parse must not become a chord that half works.
    pub fn parse(text: &str) -> Option<Chord> {
        let mut modifiers = Modifiers::NONE;
        let mut key = None;

        for part in text.split('+') {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }
            match part.to_ascii_lowercase().as_str() {
                // The platform's own command modifier, not literally Control.
                "ctrl" | "cmd" | "command" => modifiers |= Modifiers::COMMAND,
                "shift" => modifiers |= Modifiers::SHIFT,
                "alt" | "option" => modifiers |= Modifiers::ALT,
                _ => {
                    // Two keys in one chord is a chord nobody could press.
                    if key.is_some() {
                        return None;
                    }
                    key = Some(named_key(part)?);
                }
            }
        }

        Some(Chord {
            modifiers,
            key: key?,
        })
    }

    /// How this chord is written down, for the table and for a settings page.
    ///
    /// Always `Ctrl`, never the platform's symbol: this is the canonical form
    /// that [`Chord::parse`] reads back. [`Chord::label`] is the one people see.
    pub fn written(&self) -> String {
        let mut out = String::new();
        if self.modifiers.command {
            out.push_str("Ctrl+");
        }
        if self.modifiers.alt {
            out.push_str("Alt+");
        }
        if self.modifiers.shift {
            out.push_str("Shift+");
        }
        out.push_str(key_name(self.key));
        out
    }

    /// How this chord reads to the person at this machine.
    ///
    /// ⌘ on macOS and `Ctrl` elsewhere, because it is the same key and telling a
    /// Mac user to press Ctrl+S is telling them to press the wrong thing.
    pub fn label(&self) -> String {
        let mut out = String::new();
        if self.modifiers.command {
            out.push_str(if cfg!(target_os = "macos") {
                "\u{2318}"
            } else {
                "Ctrl+"
            });
        }
        if self.modifiers.alt {
            out.push_str(if cfg!(target_os = "macos") {
                "\u{2325}"
            } else {
                "Alt+"
            });
        }
        if self.modifiers.shift {
            out.push_str(if cfg!(target_os = "macos") {
                "\u{21e7}"
            } else {
                "Shift+"
            });
        }
        out.push_str(key_name(self.key));
        out
    }

    /// Whether this chord has a modifier at all.
    ///
    /// A bare key belongs to whatever has the keyboard: `T` with a caret live is
    /// the letter T, not the type tool. The handler uses this to decide what to
    /// suppress while text is being edited.
    pub fn is_bare(&self) -> bool {
        !self.modifiers.command && !self.modifiers.alt && !self.modifiers.ctrl
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.written())
    }
}

/// The keys a chord may name, and what they are called.
///
/// One table read both ways, so a key that can be written can always be read
/// back. Two tables would drift, which is the bug this whole module exists to
/// remove.
const KEYS: &[(&str, Key)] = &[
    ("A", Key::A),
    ("B", Key::B),
    ("C", Key::C),
    ("D", Key::D),
    ("E", Key::E),
    ("F", Key::F),
    ("G", Key::G),
    ("H", Key::H),
    ("I", Key::I),
    ("J", Key::J),
    ("K", Key::K),
    ("L", Key::L),
    ("M", Key::M),
    ("N", Key::N),
    ("O", Key::O),
    ("P", Key::P),
    ("Q", Key::Q),
    ("R", Key::R),
    ("S", Key::S),
    ("T", Key::T),
    ("U", Key::U),
    ("V", Key::V),
    ("W", Key::W),
    ("X", Key::X),
    ("Y", Key::Y),
    ("Z", Key::Z),
    ("0", Key::Num0),
    ("1", Key::Num1),
    ("2", Key::Num2),
    ("3", Key::Num3),
    ("4", Key::Num4),
    ("5", Key::Num5),
    ("6", Key::Num6),
    ("7", Key::Num7),
    ("8", Key::Num8),
    ("9", Key::Num9),
    ("F1", Key::F1),
    ("F2", Key::F2),
    ("F3", Key::F3),
    ("F4", Key::F4),
    ("F5", Key::F5),
    ("F6", Key::F6),
    ("F7", Key::F7),
    ("F8", Key::F8),
    ("F9", Key::F9),
    ("F10", Key::F10),
    ("F11", Key::F11),
    ("F12", Key::F12),
    ("Del", Key::Delete),
    ("Backspace", Key::Backspace),
    ("Enter", Key::Enter),
    ("Esc", Key::Escape),
    ("Tab", Key::Tab),
    ("Space", Key::Space),
    ("[", Key::OpenBracket),
    ("]", Key::CloseBracket),
    ("=", Key::Equals),
    ("-", Key::Minus),
    (",", Key::Comma),
    (".", Key::Period),
    ("/", Key::Slash),
    ("\\", Key::Backslash),
    (";", Key::Semicolon),
    ("'", Key::Quote),
    ("`", Key::Backtick),
    ("Left", Key::ArrowLeft),
    ("Right", Key::ArrowRight),
    ("Up", Key::ArrowUp),
    ("Down", Key::ArrowDown),
    ("Home", Key::Home),
    ("End", Key::End),
    ("PageUp", Key::PageUp),
    ("PageDown", Key::PageDown),
];

fn named_key(name: &str) -> Option<Key> {
    KEYS.iter()
        .find(|(written, _)| written.eq_ignore_ascii_case(name))
        .map(|(_, key)| *key)
}

fn key_name(key: Key) -> &'static str {
    KEYS.iter()
        .find(|(_, k)| *k == key)
        .map(|(written, _)| *written)
        // Unreachable through `parse`, which only produces keys from this table.
        // A key from anywhere else shows as its egui name rather than as
        // nothing: a blank shortcut column is harder to diagnose than an odd one.
        .unwrap_or_else(|| key.name())
}

/// The chords in force: the table's, with anybody's changes on top.
///
/// Held as written text rather than as parsed chords, because this is what goes
/// into the preferences file and a file somebody has edited by hand should say
/// `Ctrl+Shift+K`, not a pair of integers.
///
/// Keyed on the action's **name**, which is what a settings page shows and what
/// somebody would recognise in the file. The alternative, keying on the `Run`
/// value, survives a rename — but produces a preferences file nobody can read,
/// and renaming an action is rarer than reading a settings file.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Bindings {
    /// Only what differs from the table. **Not every shortcut.**
    ///
    /// A file holding all sixty would freeze this build's defaults into it: the
    /// next version's improved chord for an action nobody remapped would never
    /// arrive, because the file would already have an answer.
    ///
    /// An empty string means *removed*, which `None` cannot express in a map
    /// that only holds what changed.
    changed: BTreeMap<String, String>,
}

impl Bindings {
    /// The chord for an action, allowing for a change to it.
    pub fn chord(&self, action: &crate::actions::Action) -> Option<Chord> {
        match self.changed.get(action.name) {
            Some(text) if text.is_empty() => None,
            Some(text) => Chord::parse(text),
            None => action.shortcut.and_then(Chord::parse),
        }
    }

    /// Give an action a chord, or take its chord away with `None`.
    ///
    /// Setting the table's own chord back **removes the change** rather than
    /// recording it. Otherwise a person who tried a chord and thought better of
    /// it would carry a permanent override that happened to agree.
    pub fn set(&mut self, action: &crate::actions::Action, chord: Option<Chord>) {
        let default = action.shortcut.and_then(Chord::parse);
        if chord == default {
            self.changed.remove(action.name);
            return;
        }
        self.changed.insert(
            action.name.to_string(),
            chord.map(|c| c.written()).unwrap_or_default(),
        );
    }

    /// Whether this action's chord has been changed from the table's.
    pub fn is_changed(&self, action: &crate::actions::Action) -> bool {
        self.changed.contains_key(action.name)
    }

    /// Put every chord back the way this build ships them.
    pub fn clear(&mut self) {
        self.changed.clear();
    }

    /// Whether anything has been changed at all.
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty()
    }

    /// The other action already using a chord, if any.
    ///
    /// **Two actions on one chord is not an error, it is a question**, and the
    /// answer depends on the actions: `Ctrl+A` means select-all everywhere and
    /// nothing here would want it twice, but a chord that only fires with a
    /// caret live and one that only fires with frames selected can share. This
    /// reports the clash and lets the settings page say so; it does not refuse.
    pub fn clash(&self, chord: Chord, unlike: &crate::actions::Action) -> Option<&'static str> {
        crate::actions::all()
            .iter()
            .filter(|other| other.name != unlike.name)
            .find(|other| self.chord(other) == Some(chord))
            .map(|other| other.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shortcut_parses() {
        // **The test that makes one description possible.** Before this, a
        // shortcut string was a label and nothing checked it; now the handler
        // reads it, so a typo is a shortcut that never fires. Silently.
        for action in crate::actions::all() {
            if let Some(text) = action.shortcut {
                assert!(
                    Chord::parse(text).is_some(),
                    "{}: cannot parse shortcut {text:?}",
                    action.name
                );
            }
        }
    }

    #[test]
    fn a_chord_round_trips_through_its_written_form() {
        // The written form is what goes in the preferences file. If it did not
        // read back, somebody's remapped shortcut would survive until they
        // quit.
        for text in [
            "Ctrl+N",
            "Ctrl+Shift+S",
            "Ctrl+Alt+Shift+P",
            "Del",
            "F11",
            "Ctrl+]",
        ] {
            let chord = Chord::parse(text).expect(text);
            let back = Chord::parse(&chord.written()).expect("written form");
            assert_eq!(chord, back, "{text} did not survive");
        }
    }

    #[test]
    fn ctrl_means_the_platforms_own_command_key() {
        // Not literally Control. One entry in the table serves both platforms,
        // which is what stops a Mac build showing chords nobody can press.
        let chord = Chord::parse("Ctrl+S").expect("parse");
        assert_eq!(chord.modifiers, Modifiers::COMMAND);
    }

    #[test]
    fn nonsense_is_refused_rather_than_guessed() {
        // A chord nobody can parse must not become a chord that half works.
        for text in ["", "Ctrl+", "+S", "Ctrl+Wingding", "Ctrl+S+N", "Shift"] {
            assert!(Chord::parse(text).is_none(), "{text:?} was accepted");
        }
    }

    #[test]
    fn only_changes_are_stored() {
        // A file holding all sixty would freeze this build's defaults into it,
        // and the next version's better chord would never arrive.
        let save = crate::actions::all()
            .iter()
            .find(|a| a.name == "Save")
            .expect("Save");

        let mut bindings = Bindings::default();
        assert!(bindings.is_empty());
        assert_eq!(bindings.chord(save), Chord::parse("Ctrl+S"));

        bindings.set(save, Chord::parse("Ctrl+Alt+W"));
        assert_eq!(bindings.chord(save), Chord::parse("Ctrl+Alt+W"));
        assert!(bindings.is_changed(save));

        // Set back to the table's own chord: the change goes away rather than
        // being recorded as an override that happens to agree.
        bindings.set(save, Chord::parse("Ctrl+S"));
        assert!(bindings.is_empty(), "an agreeing override was kept");
    }

    #[test]
    fn a_shortcut_can_be_taken_away() {
        // `None` in a map that only holds changes cannot say "removed", so an
        // empty string does. Without it, removing a chord would silently mean
        // "use the default".
        let save = crate::actions::all()
            .iter()
            .find(|a| a.name == "Save")
            .expect("Save");

        let mut bindings = Bindings::default();
        bindings.set(save, None);
        assert_eq!(bindings.chord(save), None, "the chord came back");
        assert!(!bindings.is_empty());
    }

    #[test]
    fn a_clash_is_reported_against_the_other_action() {
        let save = crate::actions::all()
            .iter()
            .find(|a| a.name == "Save")
            .expect("Save");

        let bindings = Bindings::default();
        // Ctrl+N is New document's in the shipped table.
        let clash = bindings.clash(Chord::parse("Ctrl+N").expect("parse"), save);
        assert_eq!(clash, Some("New document"));

        // And an action does not clash with itself.
        assert_eq!(
            bindings.clash(Chord::parse("Ctrl+S").expect("parse"), save),
            None
        );
    }

    #[test]
    fn the_shipped_table_has_no_clashes() {
        // Two actions on one chord means one of them cannot be reached by
        // keyboard at all, and which one depends on the order of an `if` chain.
        let bindings = Bindings::default();
        for action in crate::actions::all() {
            if let Some(chord) = bindings.chord(action) {
                assert_eq!(
                    bindings.clash(chord, action),
                    None,
                    "{} shares {} with another action",
                    action.name,
                    chord.written()
                );
            }
        }
    }

    #[test]
    fn bindings_round_trip_through_json() {
        let save = crate::actions::all()
            .iter()
            .find(|a| a.name == "Save")
            .expect("Save");

        let mut bindings = Bindings::default();
        bindings.set(save, Chord::parse("Ctrl+Alt+W"));

        let text = serde_json::to_string(&bindings).expect("write");
        let back: Bindings = serde_json::from_str(&text).expect("read");
        assert_eq!(back, bindings);
        assert_eq!(back.chord(save), Chord::parse("Ctrl+Alt+W"));
    }
}
