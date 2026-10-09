//! Quick Apply: every style the document has, and every command, typed at.
//!
//! InDesign's Edit ▸ Quick Apply, on its key, Ctrl+Enter. Where the command
//! palette (D3) runs commands, this puts a style on whatever is in hand —
//! the text under the caret, the selected objects, the table or the cell
//! being typed in — without going to the panel the style lives in.
//!
//! A query may begin with InDesign's prefixes to look at one kind only:
//! `p:` paragraph styles, `c:` character styles, `o:` object styles, `t:`
//! table styles, `ce:` cell styles, `m:` commands. Enter applies and
//! closes; Alt+Enter applies and clears what was set by hand; Shift+Enter
//! applies and keeps the list open for the next.

use egui::{Key, Ui};
use tessera_document::ids::{CellStyleId, FrameId, ObjectStyleId, TableStyleId};
use tessera_text::story::{CharacterStyleId, ParagraphStyleId};

use crate::actions::{self, Action};
use crate::app::TesseraApp;
use crate::command::{Command, apply};
use crate::theme::Theme;

/// The window's own state. View state; nothing here is document data.
#[derive(Debug, Default)]
pub struct QuickApply {
    pub open: bool,
    pub query: String,
    /// Which row the arrow keys have moved to.
    pub highlighted: usize,
}

impl QuickApply {
    pub fn close(&mut self) {
        self.open = false;
        self.query.clear();
        self.highlighted = 0;
    }
}

/// What a row of the list puts on the page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Entry {
    /// A paragraph style, or `None` for [Basic Paragraph].
    Paragraph(Option<ParagraphStyleId>),
    /// A character style, or `None` for [None].
    Character(Option<CharacterStyleId>),
    Object(ObjectStyleId),
    Table(TableStyleId),
    Cell(CellStyleId),
    Command(actions::Run),
}

/// The kinds a prefix can name, in the order the list shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Paragraph,
    Character,
    Object,
    Table,
    Cell,
    Command,
}

impl Kind {
    /// What a row of the kind says after its name.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Paragraph => "Paragraph style",
            Kind::Character => "Character style",
            Kind::Object => "Object style",
            Kind::Table => "Table style",
            Kind::Cell => "Cell style",
            Kind::Command => "Command",
        }
    }
}

impl Entry {
    pub fn kind(self) -> Kind {
        match self {
            Entry::Paragraph(_) => Kind::Paragraph,
            Entry::Character(_) => Kind::Character,
            Entry::Object(_) => Kind::Object,
            Entry::Table(_) => Kind::Table,
            Entry::Cell(_) => Kind::Cell,
            Entry::Command(_) => Kind::Command,
        }
    }
}

/// A query's prefix, if it has one InDesign knows, and the rest of it.
///
/// `ce:` before `c:`, or a search for cell styles would read as a search
/// for character styles named "e…".
pub fn split_prefix(query: &str) -> (Option<Kind>, &str) {
    let trimmed = query.trim_start();
    let lower = trimmed.to_lowercase();
    for (prefix, kind) in [
        ("ce:", Kind::Cell),
        ("p:", Kind::Paragraph),
        ("c:", Kind::Character),
        ("o:", Kind::Object),
        ("t:", Kind::Table),
        ("m:", Kind::Command),
    ] {
        if lower.starts_with(prefix) {
            return (Some(kind), &trimmed[prefix.len()..]);
        }
    }
    (None, query)
}

/// Every row that matches the query and could be applied now, with its name.
///
/// Only what applies: a character style is no use without text selected,
/// and a list that offers it anyway is a list that does nothing when used.
pub fn entries(state: &TesseraApp, query: &str) -> Vec<(Entry, String)> {
    let (only, needle) = split_prefix(query);
    let doc = state.active().document();
    let mut all: Vec<(Entry, String)> = Vec::new();

    let text = super::panels::text_in_hand(state);
    if text.is_some() {
        all.push((
            Entry::Paragraph(None),
            super::styles::BASIC_PARAGRAPH.to_string(),
        ));
        let mut styles: Vec<_> = doc
            .paragraph_styles
            .iter()
            .map(|(id, s)| (Entry::Paragraph(Some(id)), s.name.clone()))
            .collect();
        styles.sort_by_key(|(_, n)| n.to_lowercase());
        all.extend(styles);
    }
    if text.as_ref().is_some_and(|(_, range)| !range.is_empty()) {
        all.push((Entry::Character(None), "[None]".to_string()));
        let mut styles: Vec<_> = doc
            .character_styles
            .iter()
            .map(|(id, s)| (Entry::Character(Some(id)), s.name.clone()))
            .collect();
        styles.sort_by_key(|(_, n)| n.to_lowercase());
        all.extend(styles);
    }
    if state.active().editing.is_none() && !state.active().selection.is_empty() {
        let mut styles: Vec<_> = doc
            .object_styles
            .iter()
            .map(|(id, s)| (Entry::Object(id), s.name.clone()))
            .collect();
        styles.sort_by_key(|(_, n)| n.to_lowercase());
        all.extend(styles);
    }
    let table = super::table_styles::in_hand(state);
    if table.is_some() {
        let mut styles: Vec<_> = doc
            .table_styles
            .iter()
            .map(|(id, s)| (Entry::Table(id), s.name.clone()))
            .collect();
        styles.sort_by_key(|(_, n)| n.to_lowercase());
        all.extend(styles);
    }
    if table.as_ref().is_some_and(|(_, _, cell)| cell.is_some()) {
        let mut styles: Vec<_> = doc
            .cell_styles
            .iter()
            .map(|(id, s)| (Entry::Cell(id), s.name.clone()))
            .collect();
        styles.sort_by_key(|(_, n)| n.to_lowercase());
        all.extend(styles);
    }
    all.extend(
        actions::all()
            .iter()
            .filter(|a| a.run != actions::Run::QuickApply)
            .filter(|a| actions::enabled(state, a.run))
            .map(|a: &Action| (Entry::Command(a.run), a.name.to_string())),
    );

    all.into_iter()
        .filter(|(entry, _)| only.is_none_or(|k| entry.kind() == k))
        .filter(|(_, name)| actions::matches(needle, name))
        .collect()
}

/// How a row is put on the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    /// Applied, keeping what was set by hand where the style allows it.
    Apply,
    /// Applied, and everything set by hand cleared: Alt+Enter.
    Clearing,
}

/// The commands that put `entry` on what is in hand, as one undo step.
/// `None` when it is a command to run rather than a style to apply, or when
/// nothing it could go on is in hand any more.
pub fn commands_for(state: &TesseraApp, entry: Entry, how: How) -> Option<Command> {
    let clearing = how == How::Clearing;
    let commands = match entry {
        Entry::Paragraph(style) => {
            let (story, range) = super::panels::text_in_hand(state)?;
            let mut commands = vec![Command::SetParagraphStyleOf {
                story,
                range: range.clone(),
                style,
            }];
            if clearing {
                commands.push(Command::ClearParagraphOverrides {
                    story,
                    range: range.clone(),
                });
                commands.push(Command::ClearCharacterOverrides { story, range });
            }
            commands
        }
        Entry::Character(style) => {
            let (story, range) = super::panels::text_in_hand(state)?;
            if range.is_empty() {
                return None;
            }
            let mut commands = vec![Command::SetCharacterStyleOf {
                story,
                range: range.clone(),
                style,
            }];
            if clearing {
                commands.push(Command::ClearCharacterOverrides { story, range });
            }
            commands
        }
        Entry::Object(style) => {
            let frames: Vec<FrameId> = state.active().selection.as_slice().to_vec();
            if frames.is_empty() {
                return None;
            }
            frames
                .into_iter()
                .flat_map(|id| {
                    let mut commands = vec![Command::ApplyObjectStyle { id, style }];
                    if clearing {
                        commands.push(Command::ClearObjectOverrides { id });
                    }
                    commands
                })
                .collect()
        }
        // Applying a table or cell style already clears what was set by
        // hand, so Alt+Enter has nothing more to do for them.
        Entry::Table(style) => {
            let (id, _, _) = super::table_styles::in_hand(state)?;
            vec![Command::ApplyTableStyle {
                id,
                style: Some(style),
            }]
        }
        Entry::Cell(style) => {
            let (id, _, cell) = super::table_styles::in_hand(state)?;
            vec![Command::ApplyCellStyle {
                id,
                cells: vec![cell?],
                style: Some(style),
            }]
        }
        Entry::Command(_) => return None,
    };
    Some(Command::Together(commands))
}

/// Put a row on the page, or run it.
pub fn choose(state: &mut TesseraApp, entry: Entry, how: How) {
    match entry {
        Entry::Command(run) => actions::run(state, run),
        _ => {
            if let Some(command) = commands_for(state, entry, how) {
                apply(state, command);
            }
        }
    }
}

/// Draw the window, and apply whatever it is asked for.
pub fn show(ui: &mut Ui, state: &mut TesseraApp) {
    if !state.quick_apply.open {
        return;
    }
    let ctx = ui.ctx();
    let (up, down, enter) = ctx.input_mut(|i| {
        let enter = if i.consume_key(egui::Modifiers::NONE, Key::Enter) {
            Some((How::Apply, false))
        } else if i.consume_key(egui::Modifiers::ALT, Key::Enter) {
            Some((How::Clearing, false))
        } else if i.consume_key(egui::Modifiers::SHIFT, Key::Enter) {
            Some((How::Apply, true))
        } else {
            None
        };
        (
            i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
            i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
            enter,
        )
    });
    let mut chosen: Option<(Entry, How, bool)> = None;
    let id = egui::Id::new("quick-apply");
    let response = egui::Modal::new(id)
        .area(egui::Modal::default_area(id).anchor(
            egui::Align2::CENTER_TOP,
            egui::vec2(0.0, (ctx.content_rect().height() * 0.15).min(120.0)),
        ))
        .frame(super::dialog_frame(ctx))
        .show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 80.0).clamp(240.0, 520.0));
            ui.horizontal(|ui| {
                ui.heading("Quick apply");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak("Esc to close");
                });
            });
            ui.add_space(Theme::space_2());
            let field = ui.add(
                egui::TextEdit::singleline(&mut state.quick_apply.query)
                    .hint_text("Search styles and commands…  p: c: o: t: ce: m:")
                    .desired_width(f32::INFINITY),
            );
            field.request_focus();
            // Filtered after the field reads this frame's input.
            let matches = entries(state, &state.quick_apply.query);
            if field.changed() {
                state.quick_apply.highlighted = 0;
            }
            let qa = &mut state.quick_apply;
            qa.highlighted = qa.highlighted.min(matches.len().saturating_sub(1));
            if up {
                qa.highlighted = super::palette::moved(qa.highlighted, matches.len(), -1);
            }
            if down {
                qa.highlighted = super::palette::moved(qa.highlighted, matches.len(), 1);
            }
            if let Some((how, stay)) = enter {
                chosen = matches.get(qa.highlighted).map(|(e, _)| (*e, how, stay));
            }
            ui.add_space(Theme::space_2());
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height((ctx.content_rect().height() - 240.0).clamp(100.0, 340.0))
                .show(ui, |ui| {
                    if matches.is_empty() {
                        ui.add_space(Theme::space_3());
                        ui.label("Nothing that applies here matches your search.");
                        ui.weak(
                            "Paragraph and character styles need text in hand; \
                             object styles, a selection; cell styles, a caret in a cell.",
                        );
                    }
                    for (i, (entry, name)) in matches.iter().enumerate() {
                        let selected = i == state.quick_apply.highlighted;
                        let row = ui.add_sized(
                            [ui.available_width(), Theme::row() + Theme::space_1()],
                            egui::Button::new(name.as_str())
                                .selected(selected)
                                .shortcut_text(entry.kind().label()),
                        );
                        if selected && (up || down || field.changed()) {
                            row.scroll_to_me(Some(egui::Align::Center));
                        }
                        if row.clicked() {
                            let how = if ui.input(|i| i.modifiers.alt) {
                                How::Clearing
                            } else {
                                How::Apply
                            };
                            chosen = Some((*entry, how, ui.input(|i| i.modifiers.shift)));
                        }
                    }
                });
            ui.separator();
            ui.weak(format!(
                "{} matches  ·  Enter applies  ·  Alt+Enter clears overrides  ·  \
                 Shift+Enter keeps this open",
                matches.len()
            ));
        });
    if response.should_close() {
        state.quick_apply.close();
    } else if let Some((entry, how, stay)) = chosen {
        // A command may open a window of its own, which this one would sit
        // over, so only a style keeps the list open.
        if !stay || matches!(entry, Entry::Command(_)) {
            state.quick_apply.close();
        }
        choose(state, entry, how);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_geometry::DocRect;
    use tessera_text::story::{ParagraphFormat, ParagraphStyle};

    #[test]
    fn a_prefix_narrows_to_its_kind_and_ce_is_not_c() {
        assert_eq!(split_prefix("p:head"), (Some(Kind::Paragraph), "head"));
        assert_eq!(split_prefix("CE:total"), (Some(Kind::Cell), "total"));
        assert_eq!(split_prefix("c:em"), (Some(Kind::Character), "em"));
        assert_eq!(split_prefix("m:undo"), (Some(Kind::Command), "undo"));
        assert_eq!(split_prefix("heading"), (None, "heading"));
    }

    #[test]
    fn with_nothing_in_hand_only_commands_are_offered() {
        let state = TesseraApp::headless();
        let rows = entries(&state, "");
        assert!(!rows.is_empty());
        assert!(rows.iter().all(|(e, _)| e.kind() == Kind::Command));
        assert!(
            rows.iter()
                .all(|(e, _)| *e != Entry::Command(actions::Run::QuickApply)),
            "it does not offer itself"
        );
    }

    /// A text frame selected, and a paragraph style named "Heading".
    fn with_text() -> (TesseraApp, ParagraphStyleId) {
        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::DefineParagraphStyle(ParagraphStyle {
                name: "Heading".to_string(),
                based_on: None,
                format: ParagraphFormat::default(),
            }),
        );
        let style = state
            .active()
            .document()
            .paragraph_styles
            .keys()
            .last()
            .expect("defined");
        apply(
            &mut state,
            Command::AddTextFrame(DocRect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 100.0,
            }),
        );
        let id = state.active().selection.single().expect("selected");
        apply(
            &mut state,
            Command::SetText {
                id,
                text: "A line of copy".to_string(),
            },
        );
        (state, style)
    }

    #[test]
    fn a_paragraph_style_is_found_by_its_prefix_and_applied_with_enter() {
        let (mut state, style) = with_text();
        let rows = entries(&state, "p:head");
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].0, Entry::Paragraph(Some(style)));

        choose(&mut state, rows[0].0, How::Apply);
        let uses = |state: &TesseraApp| {
            crate::find::uses_of_paragraph_style(state.active().document(), style).len()
        };
        assert_eq!(uses(&state), 1, "the paragraph took the style");
        apply(&mut state, Command::Undo);
        assert_eq!(uses(&state), 0, "and one undo takes it off");
    }

    #[test]
    fn typing_a_name_and_pressing_enter_applies_it_and_closes() {
        let (mut state, style) = with_text();
        state.quick_apply.open = true;
        let ctx = egui::Context::default();
        let _ = crate::headless_frame::frame(&ctx, Default::default(), |ui| show(ui, &mut state));
        let input = egui::RawInput {
            events: vec![
                egui::Event::Text("p:head".into()),
                egui::Event::Key {
                    key: Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        };
        let _ = crate::headless_frame::frame(&ctx, input, |ui| show(ui, &mut state));
        let uses = crate::find::uses_of_paragraph_style(state.active().document(), style);
        assert_eq!(uses.len(), 1);
        assert!(!state.quick_apply.open);
    }

    #[test]
    fn alt_enter_also_clears_what_was_set_by_hand() {
        let (state, style) = with_text();
        let Some(Command::Together(commands)) =
            commands_for(&state, Entry::Paragraph(Some(style)), How::Clearing)
        else {
            panic!("one step");
        };
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, Command::ClearParagraphOverrides { .. }))
        );
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, Command::ClearCharacterOverrides { .. }))
        );
    }

    #[test]
    fn character_styles_wait_for_selected_characters() {
        let (state, _) = with_text();
        // A selected frame formats all its text, which is characters.
        let rows = entries(&state, "c:");
        assert!(rows.iter().any(|(e, _)| *e == Entry::Character(None)));
    }
}
