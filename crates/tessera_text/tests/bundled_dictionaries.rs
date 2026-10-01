//! The dictionaries the installers ship, read by Tessera's own Hunspell
//! reader: real files, not test fixtures, so a word list that parses into
//! nothing — or that the reader misunderstands — fails here rather than in
//! somebody's document.

use std::path::PathBuf;

use tessera_text::spell::Dictionary;

fn bundled(name: &str) -> Dictionary {
    let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packaging/dictionaries");
    let aff = std::fs::read_to_string(folder.join(format!("{name}.aff"))).expect("aff");
    let dic = std::fs::read_to_string(folder.join(format!("{name}.dic"))).expect("dic");
    Dictionary::parse(&aff, &dic)
}

#[test]
fn american_english_knows_its_words_and_their_forms() {
    let started = std::time::Instant::now();
    let us = bundled("en_US");
    let took = started.elapsed();
    assert!(!us.is_empty());
    for word in [
        "color",
        "organize",
        "center",
        "walked",
        "walking",
        "unhappy",
        "happiness",
        "typography",
        "typeface",
        "10th",
        "21st",
    ] {
        assert!(us.check(word), "en_US does not know {word:?}");
    }
    for word in ["colour", "recieve", "teh", "typograhpy"] {
        assert!(!us.check(word), "en_US knows {word:?}");
    }
    assert!(
        us.suggest("recieve").iter().any(|s| s == "receive"),
        "{:?}",
        us.suggest("recieve")
    );
    // Loaded once per language per session, on first use: a second would
    // be noticed as a stall when typing starts.
    assert!(took.as_secs_f64() < 2.0, "en_US took {took:?} to load");
}

#[test]
fn british_english_spells_the_british_way() {
    let gb = bundled("en_GB");
    for word in ["colour", "organise", "centre", "travelled", "typeface"] {
        assert!(gb.check(word), "en_GB does not know {word:?}");
    }
    for word in ["color", "center"] {
        assert!(!gb.check(word), "en_GB knows {word:?}");
    }
}
