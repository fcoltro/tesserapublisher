//! Every mutation is covered by exactly one undo entry.
//!
//! Undo integrity, the command palette and any later scripting rest on changes
//! going through `Command`, and that erodes silently: one stray edit and undo
//! has a hole in it that nothing reports.
//!
//! But it is not absolute, and pretending otherwise would be the kind of rule
//! that gets muted rather than kept. An interactive gesture — dragging a
//! frame, typing into one — must write to the document on every pointer move
//! or keystroke, or nothing would be visible until the mouse came up. Those
//! writes are legitimate **because the gesture brackets them**: it records one
//! history snapshot when it begins, or restores and reapplies once through a
//! `Command` when it ends. Milestone 1 states the same rule from the other
//! side: every gesture records exactly one undo entry, on completion.
//!
//! So the line this test holds is: **a direct mutation outside the command
//! layer must be deliberate and must say why.** It is excused only by the
//! marker below, on or just above the line.
//!
//! Rust can restrict a method to a crate but not to one sibling module, so the
//! compiler cannot hold this. Reading the crate's own source is an unusual way
//! to write a test and it is the honest tool for the job — the alternative is
//! an invariant enforced by intention, which is the kind that quietly stops
//! being true.
//!
//! **If this fails, route the change through a `Command` variant.** Reach for
//! the marker only when the write really is inside a bracketed gesture.

use std::path::Path;

/// Files that *are* the command layer.
const PERMITTED: &[&str] = &["command.rs", "open_document.rs"];

/// What excuses a direct mutation: a bracketed interactive gesture.
const MARKER: &str = "undo-bracketed:";

/// How many lines above the mutation the marker may sit.
const MARKER_REACH: usize = 6;

#[test]
fn every_direct_mutation_is_marked_as_a_bracketed_gesture() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();

    visit(&src, &mut |path, contents| {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if PERMITTED.contains(&name.as_str()) {
            return;
        }

        let lines: Vec<&str> = contents.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            // Test modules are conventionally last in a file, and a test
            // building a document is not the interface editing behind undo's
            // back. Everything from here down is fixtures.
            if line.trim() == "#[cfg(test)]" {
                break;
            }
            if !line.contains("document_mut(") || line.trim_start().starts_with("///") {
                continue;
            }

            let from = i.saturating_sub(MARKER_REACH);
            let excused = lines[from..=i].iter().any(|l| l.contains(MARKER));
            if !excused {
                offenders.push(format!("{}:{}: {}", path.display(), i + 1, line.trim()));
            }
        }
    });

    assert!(
        offenders.is_empty(),
        "these edit the document outside the command layer with no \
         `{MARKER}` marker, so undo cannot account for them:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn the_guard_reacts_to_an_unmarked_mutation() {
    // A test that can only pass is not a test.
    let unmarked = "    state.active_mut().document_mut().add_frame(layer, frame);";
    let marked = "    // undo-bracketed: the drag recorded its entry on press.\n\
                  \x20   state.active_mut().document_mut().frame_mut(id);";

    assert_eq!(count_offenders(unmarked), 1, "an unmarked write must count");
    assert_eq!(count_offenders(marked), 0, "a marked write must not");
}

/// The same rule the scan applies, over an arbitrary snippet.
fn count_offenders(source: &str) -> usize {
    let lines: Vec<&str> = source.lines().collect();
    let mut n = 0;
    for (i, line) in lines.iter().enumerate() {
        if !line.contains("document_mut(") || line.trim_start().starts_with("///") {
            continue;
        }
        let from = i.saturating_sub(MARKER_REACH);
        if !lines[from..=i].iter().any(|l| l.contains(MARKER)) {
            n += 1;
        }
    }
    n
}

fn visit(dir: &Path, f: &mut impl FnMut(&Path, &str)) {
    for entry in std::fs::read_dir(dir).expect("src is readable") {
        let path = entry.expect("entry is readable").path();
        if path.is_dir() {
            visit(&path, f);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let contents = std::fs::read_to_string(&path).expect("file is readable");
            f(&path, &contents);
        }
    }
}
