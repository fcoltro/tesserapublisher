//! What profiles are on offer, gathered once.
//!
//! Scanning the machine’s colour directories reads a few dozen files. That is
//! fast, and it is not free: doing it while a menu is open would do it on every
//! frame the menu is open for. So it happens the first time the list is wanted
//! and is kept, and there is a way to ask again for somebody who has just
//! installed a profile.

use tessera_color::profiles::{Installed, Standard};

/// One thing a person can choose.
#[derive(Debug, Clone, PartialEq)]
pub enum Choice {
    /// Built here from published numbers.
    Standard(Standard),
    /// Found on this machine.
    Installed(Installed),
}

impl Choice {
    /// What a menu shows.
    pub fn label(&self) -> String {
        match self {
            Choice::Standard(standard) => standard.label().to_string(),
            Choice::Installed(found) => found.description.clone(),
        }
    }

    /// "CMYK", "RGB" or "Grey".
    pub fn space(&self) -> &'static str {
        match self {
            Choice::Standard(standard) => {
                if standard.is_grey() {
                    "Grey"
                } else {
                    "RGB"
                }
            }
            Choice::Installed(found) => found.space,
        }
    }

    /// The profile’s bytes.
    ///
    /// Built or read at the moment of choosing rather than held for every entry
    /// in the list: a list of forty profiles is forty files, and holding them all
    /// to show their names would be tens of megabytes for a menu.
    pub fn bytes(&self) -> Option<Vec<u8>> {
        match self {
            Choice::Standard(standard) => standard.build(),
            Choice::Installed(found) => std::fs::read(&found.path).ok(),
        }
    }
}

/// The profiles on offer, found once and kept.
#[derive(Default)]
pub struct Catalogue {
    installed: Option<Vec<Installed>>,
}

impl Catalogue {
    /// Everything on offer: the built-in spaces first, then what the machine has.
    ///
    /// Built-ins first because they are always there and are what somebody with
    /// no profiles installed needs; the machine’s own after, because that is
    /// where the CMYK presses are and where the list gets long.
    pub fn choices(&mut self) -> Vec<Choice> {
        let installed = self
            .installed
            .get_or_insert_with(tessera_color::profiles::installed);

        let mut out: Vec<Choice> = Standard::ALL
            .iter()
            .copied()
            .map(Choice::Standard)
            .collect();
        out.extend(installed.iter().cloned().map(Choice::Installed));
        out
    }

    /// Look again, for somebody who has just installed a profile.
    pub fn refresh(&mut self) {
        self.installed = None;
    }

    /// How many profiles the machine turned out to have.
    pub fn installed_count(&mut self) -> usize {
        self.installed
            .get_or_insert_with(tessera_color::profiles::installed)
            .len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_spaces_are_always_on_offer() {
        // Somebody with no profiles installed still has to be able to choose
        // something, or an output intent is unreachable on a fresh machine.
        let mut catalogue = Catalogue::default();
        let choices = catalogue.choices();
        assert!(choices.len() >= Standard::ALL.len());
        assert_eq!(choices[0], Choice::Standard(Standard::Srgb));
    }

    #[test]
    fn every_choice_can_produce_its_bytes() {
        // A list entry that cannot be chosen is worse than no entry.
        let mut catalogue = Catalogue::default();
        for choice in catalogue.choices() {
            assert!(
                choice.bytes().is_some(),
                "{} could not be read",
                choice.label()
            );
            assert!(!choice.label().is_empty());
        }
    }

    #[test]
    fn the_machine_is_scanned_once_rather_than_per_frame() {
        // Reading a few dozen files is fast and not free; doing it while a menu
        // is open would do it on every frame the menu is open for.
        let mut catalogue = Catalogue::default();
        let first = catalogue.choices().len();
        assert!(catalogue.installed.is_some(), "the scan was not kept");
        assert_eq!(catalogue.choices().len(), first);
    }

    #[test]
    fn asking_again_looks_again() {
        let mut catalogue = Catalogue::default();
        catalogue.choices();
        catalogue.refresh();
        assert!(catalogue.installed.is_none());
    }

    #[test]
    fn every_choice_names_a_space_a_person_recognises() {
        let mut catalogue = Catalogue::default();
        for choice in catalogue.choices() {
            assert!(
                ["CMYK", "RGB", "Grey"].contains(&choice.space()),
                "{} is in {}",
                choice.label(),
                choice.space()
            );
        }
    }
}
