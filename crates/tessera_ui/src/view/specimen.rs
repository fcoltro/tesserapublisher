//! A style, set: the preview at the top of the style window.
//!
//! Sample text in the style's own face — the font file this machine resolves
//! its family, weight and slant to, installed in egui through the shared
//! registry — at its size, leading, tracking and colour, aligned and
//! indented as the style says, between two greyed neighbours so its space
//! before and after show. Every change in the window shows here as it is
//! made, without looking away to the page.
//!
//! **A preview, not a proof.** egui lays the sample out, not the page's
//! composer, so kerning, OpenType features, drop caps and hyphenation are
//! left to the page, and the preview says so when asked. What it shows is
//! what a style is mostly for: which face, how big, how spaced, what
//! colour.

use egui::{Color32, FontFamily, FontId, Rect, Sense, Stroke, Ui, Vec2};

use tessera_color::Color;
use tessera_text::story::{
    Alignment, Case, CharacterFormat, CharacterStyleId, ListKind, NoStyles, ParagraphStyleId,
    Story, Styles as _,
};

use super::style_ui::{self, srgb};
use crate::app::TesseraApp;
use crate::theme::Theme;

/// A face asked for by family, weight and slant.
type FaceKey = (Option<String>, u16, bool);

/// The faces the previews have resolved, so a frame that shows the same
/// style shapes nothing.
#[derive(Debug, Clone, Default)]
pub struct Faces {
    resolved: Vec<(FaceKey, Option<tessera_text::FontData>)>,
    /// The sample last found for a style, and the document revision it was
    /// found at: finding it walks every paragraph of every story, which is
    /// not work for every frame of a long book.
    sample: Option<(ParagraphStyleId, u64, String)>,
}

/// How many resolved faces are kept. A few styles' worth.
const KEPT: usize = 16;

/// The largest the sample is drawn, in points: a 72-point display face is
/// scaled down to this, and the caption says by how much.
const LARGEST: f32 = 30.0;

/// How far the sample sits in from the paper's edge.
const MARGIN: f32 = 14.0;

/// The greyed lines standing for the neighbouring paragraphs.
const NEIGHBOUR_PITCH: f32 = 7.0;

const HOW: &str = "Set in the style's own face as this machine has it. Kerning, \
                   OpenType features, drop caps and hyphenation are the page's to show.";

/// The face this machine gives `format`'s family, weight and slant, by the
/// route the page takes: shape two letters in it and read the font off the
/// result.
fn face(state: &mut TesseraApp, format: &CharacterFormat) -> Option<tessera_text::FontData> {
    let key: FaceKey = (
        format.family.clone(),
        format.weight.unwrap_or(400),
        format.italic.unwrap_or(false),
    );
    if let Some((_, face)) = state
        .styles_window
        .faces
        .resolved
        .iter()
        .find(|(k, _)| *k == key)
    {
        return face.clone();
    }
    let mut story = Story::new("Ag");
    story.apply_character_format(
        0..2,
        &CharacterFormat {
            family: key.0.clone(),
            weight: Some(key.1),
            italic: Some(key.2),
            ..CharacterFormat::default()
        },
    );
    let face = state
        .shaper
        .shape(&story, &NoStyles::default(), 1000.0)
        .fonts
        .first()
        .cloned();
    let resolved = &mut state.styles_window.faces.resolved;
    resolved.push((key, face.clone()));
    if resolved.len() > KEPT {
        resolved.remove(0);
    }
    face
}

/// Whether the sample has to be slanted by egui: the style asks for italic
/// and this machine resolves the family's italic to the same upright file
/// as its roman. Without it an italic style previewed upright whenever the
/// family had no italic of its own — the default "sans-serif" among them,
/// on more than one system.
fn slanted(state: &mut TesseraApp, format: &CharacterFormat) -> bool {
    if format.italic != Some(true) {
        return false;
    }
    let upright = CharacterFormat {
        italic: Some(false),
        ..format.clone()
    };
    match (face(state, format), face(state, &upright)) {
        (Some(a), Some(b)) => (a.data.id(), a.index) == (b.data.id(), b.index),
        _ => false,
    }
}

/// The egui family to draw `format` in, or `None` on the frame its face is
/// being installed.
fn family(ui: &Ui, state: &mut TesseraApp, format: &CharacterFormat) -> Option<FontFamily> {
    let face = face(state, format)?;
    crate::ui_fonts::document_face(ui.ctx(), &face).map(|name| FontFamily::Name(name.into()))
}

/// The paragraph style `id`, set between its neighbours.
pub(crate) fn paragraph(ui: &mut Ui, state: &mut TesseraApp, id: ParagraphStyleId) {
    let doc = state.active().document();
    let format = doc.paragraph_chain(id);
    let character = format.character.over(&doc.document_default());
    let colour = ink(state, &character);
    let mut sample = cached_sample(state, id);
    match character.case {
        Some(Case::Upper | Case::SmallCaps) => sample = sample.to_uppercase(),
        Some(Case::Lower) => sample = sample.to_lowercase(),
        _ => {}
    }
    if let Some(list) = &format.list {
        match list.kind {
            ListKind::Bullet => sample = format!("{}  {sample}", list.bullet),
            ListKind::Number => sample = format!("1{}  {sample}", list.suffix),
            ListKind::None => {}
        }
    }
    let size = character.size.unwrap_or(12.0);
    let scale = (LARGEST / size).min(1.0);
    let family = family(ui, state, &character);
    let slant = slanted(state, &character);

    frame(ui, |ui| {
        let width = ui.available_width();
        let measure = width - 2.0 * MARGIN;
        let Some(family) = family else {
            waiting(ui, width);
            return;
        };

        let indent_left = format.indent_left.unwrap_or(0.0) * scale;
        let indent_right = format.indent_right.unwrap_or(0.0) * scale;
        let wrap = (measure - indent_left - indent_right).max(48.0);
        let alignment = format.alignment.unwrap_or(Alignment::Left);
        let job = sample_job(
            &sample,
            &character,
            family,
            colour,
            scale,
            wrap,
            alignment,
            (format.indent_first.unwrap_or(0.0) * scale).max(0.0),
            slant,
        );
        let galley = ui.painter().layout_job(job);

        let rule_height = |rule: &Option<tessera_text::story::ParagraphRule>| {
            rule.as_ref()
                .filter(|r| r.on)
                .map_or(0.0, |r| (r.weight * scale).max(1.0) + 5.0)
        };
        let before = format.space_before.unwrap_or(0.0).max(0.0) * scale;
        let after = format.space_after.unwrap_or(0.0).max(0.0) * scale;
        let height = MARGIN
            + 2.0 * NEIGHBOUR_PITCH
            + 4.0
            + before
            + rule_height(&format.rule_above)
            + galley.size().y
            + rule_height(&format.rule_below)
            + after
            + 4.0
            + 2.0 * NEIGHBOUR_PITCH
            + MARGIN;
        let (paper, response) =
            ui.allocate_exact_size(Vec2::new(width, height.min(220.0)), Sense::hover());
        response.on_hover_text(HOW);
        let painter = ui.painter_at(paper);
        paint_paper(&painter, paper);

        let left = paper.left() + MARGIN;
        let mut y = paper.top() + MARGIN;
        neighbours(&painter, left, measure, &mut y, true);
        y += 4.0 + before;
        if let Some(rule) = format.rule_above.as_ref().filter(|r| r.on) {
            y = paint_rule(
                &painter,
                rule,
                left + indent_left,
                wrap,
                y,
                scale,
                colour,
                state,
            );
        }
        let x = match alignment {
            Alignment::Centre => left + indent_left + wrap / 2.0,
            Alignment::Right => left + indent_left + wrap,
            Alignment::Left | Alignment::Justify => left + indent_left,
        };
        let text_height = galley.size().y;
        painter.galley(egui::pos2(x, y), galley, colour);
        y += text_height;
        if let Some(rule) = format.rule_below.as_ref().filter(|r| r.on) {
            y = paint_rule(
                &painter,
                rule,
                left + indent_left,
                wrap,
                y + 2.0,
                scale,
                colour,
                state,
            );
        }
        y += after + 4.0;
        neighbours(&painter, left, measure, &mut y, false);

        caption(ui, &describe(&character), scale);
    });
}

/// The character style `id`, set in its own words where the document has
/// any, inside the paragraph style it is shown in.
///
/// A character style says only what it changes, so a preview of it alone
/// shows almost nothing: "italic, Ocean" is a style whose size and face are
/// whatever it lands in. So it is shown landed: the first place the
/// document uses it, with the words either side, set in the paragraph style
/// chosen under the preview — by default the one that first use sits in.
pub(crate) fn character(ui: &mut Ui, state: &mut TesseraApp, id: CharacterStyleId) {
    let context = super::styles::shown_in(state, id);
    let (before, words, after) =
        super::styles::first_use_in_context(state, id).unwrap_or_else(|| {
            (
                "Text around it sits in the paragraph's own type, ".to_string(),
                "and these words take the style".to_string(),
                ", and then the sentence goes on.".to_string(),
            )
        });
    let doc = state.active().document();
    let base = match context {
        Some(p) => doc
            .paragraph_chain(p)
            .character
            .over(&doc.document_default()),
        None => doc.document_default(),
    };
    let styled = doc.character_chain(id).over(&base);
    let mut contexts: Vec<(Option<ParagraphStyleId>, String)> =
        vec![(None, super::styles::BASIC_PARAGRAPH.to_string())];
    contexts.extend(
        doc.paragraph_styles
            .iter()
            .map(|(p, style)| (Some(p), style.name.clone())),
    );
    let base_colour = ink(state, &base);
    let styled_colour = ink(state, &styled);
    let largest = base.size.unwrap_or(12.0).max(styled.size.unwrap_or(12.0));
    let scale = (LARGEST / largest).min(1.0);
    let base_family = family(ui, state, &base);
    let styled_family = family(ui, state, &styled);
    let base_slant = slanted(state, &base);
    let styled_slant = slanted(state, &styled);
    let mut chosen = None;

    frame(ui, |ui| {
        let width = ui.available_width();
        let measure = width - 2.0 * MARGIN;
        let (Some(base_family), Some(styled_family)) = (base_family, styled_family) else {
            waiting(ui, width);
            return;
        };
        let mut job = egui::text::LayoutJob::default();
        let plain = text_format(&base, base_family.clone(), base_colour, scale, base_slant);
        job.append(&cased(&before, base.case), 0.0, plain.clone());
        job.append(
            &cased(&words, styled.case),
            0.0,
            text_format(&styled, styled_family, styled_colour, scale, styled_slant),
        );
        job.append(&cased(&after, base.case), 0.0, plain);
        job.wrap.max_width = measure;
        job.wrap.max_rows = 3;
        let galley = ui.painter().layout_job(job);
        let height = 2.0 * MARGIN + galley.size().y;
        let (paper, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
        response.on_hover_text(HOW);
        let painter = ui.painter_at(paper);
        paint_paper(&painter, paper);
        painter.galley(
            egui::pos2(paper.left() + MARGIN, paper.top() + MARGIN),
            galley,
            base_colour,
        );
        caption_with(
            ui,
            &describe(&styled),
            &format!("at {:.0}%", scale * 100.0),
            |ui| {
                let shown = contexts
                    .iter()
                    .find(|(p, _)| *p == context)
                    .map_or(super::styles::BASIC_PARAGRAPH, |(_, name)| name.as_str());
                crate::icons::reads_as(
                    egui::ComboBox::from_id_salt(("character-shown-in", id))
                        .selected_text(
                            egui::RichText::new(shown)
                                .size(Theme::TYPE_SM)
                                .color(Theme::text_primary()),
                        )
                        .truncate()
                        .width(150.0)
                        .show_ui(ui, |ui| {
                            for (p, name) in &contexts {
                                if ui.selectable_label(*p == context, name).clicked() {
                                    chosen = Some(*p);
                                }
                            }
                        })
                        .response,
                    "Shown in",
                    egui::WidgetType::ComboBox,
                    None,
                )
                .on_hover_text(
                    "The paragraph style the sample sits in. What the character \
                 style leaves alone is shown as it is in this paragraph.",
                );
                ui.add(
                    egui::Label::new(
                        egui::RichText::new("Shown in")
                            .size(Theme::TYPE_SM)
                            .color(Theme::text_muted()),
                    )
                    .selectable(false),
                );
            },
        );
    });
    if let Some(p) = chosen {
        state.styles_window.shown_in = Some((id, p));
    }
}

/// Text as a case asks for it to be drawn.
fn cased(text: &str, case: Option<Case>) -> String {
    match case {
        Some(Case::Upper | Case::SmallCaps) => text.to_uppercase(),
        Some(Case::Lower) => text.to_lowercase(),
        _ => text.to_string(),
    }
}

/// The preview's card: a raised ground holding the paper and its caption.
fn frame(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(style_ui::well_fill())
        .stroke(Stroke::new(1.0, Theme::rule()))
        .corner_radius(style_ui::CARD_RADIUS)
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
    ui.add_space(Theme::space_4());
}

/// Paper: white whatever the theme, because the type is being judged as it
/// will print.
fn paint_paper(painter: &egui::Painter, paper: Rect) {
    painter.rect(
        paper,
        6.0,
        Color32::WHITE,
        Stroke::new(1.0, Theme::rule()),
        egui::StrokeKind::Inside,
    );
}

/// The paper, empty, for the frame the face is being installed in.
fn waiting(ui: &mut Ui, width: f32) {
    let (paper, _) = ui.allocate_exact_size(Vec2::new(width, 96.0), Sense::hover());
    paint_paper(ui.painter(), paper);
    ui.painter().text(
        paper.center(),
        egui::Align2::CENTER_CENTER,
        "Loading the face\u{2026}",
        FontId::proportional(Theme::TYPE_MD),
        Color32::from_gray(140),
    );
    ui.ctx().request_repaint();
}

/// Two greyed lines of a neighbouring paragraph, the last one short.
fn neighbours(painter: &egui::Painter, left: f32, measure: f32, y: &mut f32, before: bool) {
    let grey = Color32::from_gray(222);
    let lengths = if before { [1.0, 0.62] } else { [1.0, 0.8] };
    for length in lengths {
        painter.rect_filled(
            Rect::from_min_size(egui::pos2(left, *y + 3.0), Vec2::new(measure * length, 3.0)),
            1.5,
            grey,
        );
        *y += NEIGHBOUR_PITCH;
    }
}

/// A paragraph rule across the measure, in its colour or the text's.
#[allow(clippy::too_many_arguments)]
fn paint_rule(
    painter: &egui::Painter,
    rule: &tessera_text::story::ParagraphRule,
    left: f32,
    width: f32,
    y: f32,
    scale: f32,
    text: Color32,
    state: &TesseraApp,
) -> f32 {
    let weight = (rule.weight * scale).max(1.0);
    let colour = rule.colour.as_ref().map_or(text, |c| {
        srgb(state.active().document().resolve_colour(c).to_rgb_f32())
    });
    let top = y + 2.0;
    painter.rect_filled(
        Rect::from_min_size(
            egui::pos2(left + rule.indent_left * scale, top),
            Vec2::new(
                (width - (rule.indent_left + rule.indent_right) * scale).max(8.0),
                weight,
            ),
        ),
        0.0,
        colour,
    );
    top + weight + 3.0
}

/// The sample as egui lays it out: one run in the style's format.
#[allow(clippy::too_many_arguments)]
fn sample_job(
    sample: &str,
    format: &CharacterFormat,
    family: FontFamily,
    colour: Color32,
    scale: f32,
    wrap: f32,
    alignment: Alignment,
    first_line: f32,
    slant: bool,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        sample,
        first_line,
        text_format(format, family, colour, scale, slant),
    );
    job.wrap.max_width = wrap;
    job.wrap.max_rows = 4;
    job.halign = match alignment {
        Alignment::Centre => egui::Align::Center,
        Alignment::Right => egui::Align::RIGHT,
        Alignment::Left | Alignment::Justify => egui::Align::LEFT,
    };
    job.justify = alignment == Alignment::Justify;
    job
}

/// A character format as egui's: the face, the size, the leading, the
/// tracking, the colour, and the lines under and through.
fn text_format(
    format: &CharacterFormat,
    family: FontFamily,
    colour: Color32,
    scale: f32,
    slant: bool,
) -> egui::TextFormat {
    let size = format.size.unwrap_or(12.0) * scale;
    let line = |decoration: &Option<tessera_text::story::Decoration>| match decoration {
        Some(d) if d.on => Stroke::new(
            d.weight
                .map_or((size / 14.0).max(1.0), |w| (w * scale).max(1.0)),
            colour,
        ),
        _ => Stroke::NONE,
    };
    egui::TextFormat {
        font_id: FontId::new(size, family),
        color: colour,
        extra_letter_spacing: format.tracking.unwrap_or(0.0) / 1000.0 * size,
        line_height: Some(format.line_height.unwrap_or(1.2) * size),
        underline: line(&format.underline),
        strikethrough: line(&format.strikethrough),
        italics: slant,
        ..Default::default()
    }
}

/// The colour the text is drawn in, a swatch followed to its value.
fn ink(state: &TesseraApp, format: &CharacterFormat) -> Color32 {
    let colour = format.colour.clone().unwrap_or(Color::BLACK);
    srgb(
        state
            .active()
            .document()
            .resolve_colour(&colour)
            .to_rgb_f32(),
    )
}

/// "Georgia Bold · 18 pt · leading 1.35×": what the sample is set in.
fn describe(format: &CharacterFormat) -> String {
    let mut face = format
        .family
        .clone()
        .unwrap_or_else(|| "Default".to_string());
    match format.weight.unwrap_or(400) {
        400 => {}
        300 => face.push_str(" Light"),
        500 => face.push_str(" Medium"),
        700 => face.push_str(" Bold"),
        w => face.push_str(&format!(" {w}")),
    }
    if format.italic == Some(true) {
        face.push_str(" Italic");
    }
    let number = |v: f32| {
        let written = format!("{v:.2}");
        written
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    };
    format!(
        "{face} · {} pt · leading {}×",
        number(format.size.unwrap_or(12.0)),
        number(format.line_height.unwrap_or(1.2))
    )
}

/// Under the paper: what it is set in, and at what scale.
fn caption(ui: &mut Ui, said: &str, scale: f32) {
    caption_with(ui, said, &format!("Shown at {:.0}%", scale * 100.0), |_| {});
}

/// As above, with a control of the preview's own before the scale: added
/// right to left, so its last widget sits furthest left.
fn caption_with(ui: &mut Ui, said: &str, scale: &str, control: impl FnOnce(&mut Ui)) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add(
            egui::Label::new(
                egui::RichText::new(said)
                    .size(Theme::TYPE_SM)
                    .color(Theme::text_muted()),
            )
            .truncate()
            .selectable(false),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(scale)
                        .size(Theme::TYPE_SM)
                        .color(Theme::text_muted()),
                )
                .selectable(false),
            );
            control(ui);
        });
    });
}

/// The sample for `id`, found again only when the document has changed.
fn cached_sample(state: &mut TesseraApp, id: ParagraphStyleId) -> String {
    let revision = state.active().document().revision();
    if let Some((for_id, at, sample)) = &state.styles_window.faces.sample
        && *for_id == id
        && *at == revision
    {
        return sample.clone();
    }
    let sample = sample_text(state, id);
    state.styles_window.faces.sample = Some((id, revision, sample.clone()));
    sample
}

/// What to set the sample in: the first paragraph of the document already
/// in the style, so a heading previews as a heading does — or a sentence
/// that says what a preview is, when nothing uses it yet.
fn sample_text(state: &TesseraApp, id: ParagraphStyleId) -> String {
    const MOST: usize = 160;
    let doc = state.active().document();
    for story in doc.stories.values() {
        for range in story.paragraph_ranges() {
            let in_style = story
                .paragraph_run_at(range.start)
                .is_some_and(|run| run.style == Some(id));
            if !in_style {
                continue;
            }
            let text: String = story.text[range]
                .chars()
                .filter(|c| !c.is_control() && tessera_text::variables::Marker::of(*c).is_none())
                .take(MOST)
                .collect();
            let text = text.trim();
            if !text.is_empty() {
                return text.to_string();
            }
        }
    }
    "Every paragraph set in this style looks like this one: the same face, \
     the same size, the same spacing."
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_face_is_resolved_once_and_kept() {
        let mut state = TesseraApp::headless();
        let format = CharacterFormat {
            weight: Some(700),
            ..CharacterFormat::default()
        };
        let first = face(&mut state, &format).expect("a bold face on this machine");
        let again = face(&mut state, &format).expect("the same");
        assert_eq!(
            (first.data.id(), first.index),
            (again.data.id(), again.index)
        );
        assert_eq!(
            state.styles_window.faces.resolved.len(),
            1,
            "the second ask was answered from what the first found"
        );
    }

    #[test]
    fn the_sample_is_the_document_s_own_text_in_the_style() {
        use crate::command::{Command, apply};
        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::DefineParagraphStyle(tessera_text::story::ParagraphStyle {
                name: "Heading".to_string(),
                based_on: None,
                format: Default::default(),
            }),
        );
        let id = state
            .active()
            .document()
            .paragraph_styles
            .keys()
            .next()
            .expect("style");
        assert!(
            sample_text(&state, id).starts_with("Every paragraph"),
            "nothing uses it yet"
        );
        apply(
            &mut state,
            Command::AddTextFrame(tessera_geometry::DocRect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 100.0,
            }),
        );
        let frame = state.active().selection.single().expect("frame");
        apply(
            &mut state,
            Command::SetText {
                id: frame,
                text: "Body first\nThe real heading\n".to_string(),
            },
        );
        let tessera_document::nodes::FrameKind::Text { story, .. } =
            state.active().document().frame(frame).expect("frame").kind
        else {
            panic!("a text frame");
        };
        apply(
            &mut state,
            Command::SetParagraphStyleOf {
                story,
                range: 11..12,
                style: Some(id),
            },
        );
        assert_eq!(sample_text(&state, id), "The real heading");
    }
}
