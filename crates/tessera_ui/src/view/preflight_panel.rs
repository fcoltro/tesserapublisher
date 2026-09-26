//! The preflight panel, and the indicator that makes it worth having.
//!
//! **Click-to-jump is the whole feature, and the fix beside it is the rest.**
//! A list of forty problems that does not say where they are is a list
//! somebody reads and then has to find everything in twice — and that is how
//! preflight comes to be skipped, which is worse than not having it, because
//! it was trusted right up until it was ignored.
//!
//! So the panel says first whether the job can go, then lists each problem
//! under the rule that found it, one row per object with the page it is on,
//! and a click on a row selects the object and brings it into view. Next and
//! Previous walk the list the way InDesign's preflight walks its own. The row
//! gone to opens out with what fixes it, where there is something to press:
//! a frame fitted to its text, a file relinked or read again, a press chosen,
//! a missing font or colour name handed to one that exists.
//!
//! A row about the document as a whole says so and goes nowhere, which is
//! honest: jumping somewhere arbitrary to seem responsive is worse than
//! staying put.

use std::collections::HashSet;

use egui::{Color32, Rect, Sense, Ui, Vec2};

use tessera_document::ids::{FrameId, LinkId};
use tessera_document::nodes::FrameKind;
use tessera_preflight::{Checks, Problem, Report, Rule, Severity, Subject, Where};

use super::{panel_ui, style_ui};
use crate::app::TesseraApp;
use crate::command::{Command, apply};
use crate::icons::Icon;
use crate::theme::Theme;

/// A problem's row: what it is on, and what is wrong with it.
const ROW: f32 = 40.0;

/// A rule's heading.
const HEADING: f32 = 30.0;

/// Problems a rule lists before it is asked for the rest. Four hundred RGB
/// fills are one decision about colour, and a list of four hundred rows is a
/// list nobody scrolls to the end of.
const FIRST: usize = 50;

/// Where the text of a row starts, past its icon; what a rule's explanation
/// and a row's fixes line up with.
const INDENT: f32 = 30.0;

/// What the panel is showing, kept between frames.
#[derive(Debug, Clone, Default)]
pub struct View {
    /// Only errors, only warnings, or everything.
    pub filter: Option<Severity>,
    /// The problem last gone to, which Next and Previous count from: its
    /// rule, its place and its sentence, which together tell apart two
    /// problems about the document as a whole.
    pub current: Option<(Rule, Where, String)>,
    /// Rules whose problems are folded away under their heading.
    pub folded: HashSet<Rule>,
    /// Rules listing every problem, past the first [`FIRST`].
    pub whole: HashSet<Rule>,
    /// Whether the list of checks is open.
    pub checks: bool,
    /// Bring the current problem's row into view on the next frame: set by
    /// Next and Previous, which can move it to where the list is not showing.
    scroll: bool,
}

/// What a click in the panel asked for, done once the panel is drawn.
#[derive(Debug, Clone, PartialEq)]
enum Ask {
    GoTo(Rule, Where, String),
    Fold(Rule),
    Whole(Rule),
    Fix(Fix),
}

/// A repair a problem offers.
#[derive(Debug, Clone, PartialEq)]
enum Fix {
    FitFrame(FrameId),
    Relink(LinkId),
    Update(LinkId),
    UpdateAll(Vec<LinkId>),
    FindMissing,
    ShowInLinks(LinkId),
    ChoosePress,
    ReplaceFamily { from: String, to: String },
    RepointSwatch { from: String, to: String },
}

/// The section, as it sits in the rail.
pub fn docked(ui: &mut Ui, state: &mut TesseraApp) {
    // Read out before anything is drawn: the report borrows the application,
    // and acting on a row needs it mutably.
    let report = crate::preflight::Preflight::report(state).clone();
    let checks = crate::preflight::checks(state);

    verdict(ui, state, &report, checks);
    if state.preflight.view.checks {
        checks_card(ui, state, checks);
    }
    if report.problems.is_empty() {
        return;
    }

    filters(ui, state, &report);
    let filter = state.preflight.view.filter;
    let shown: Vec<&Problem> = report
        .problems
        .iter()
        .filter(|p| filter.is_none_or(|s| p.severity() == s))
        .collect();
    let sections = sections(&shown);
    // Walked in the order the list shows them in, rule by rule.
    let order: Vec<&Problem> = sections
        .iter()
        .flat_map(|(_, p)| p.iter().copied())
        .collect();
    let mut ask = walk(ui, state, &order);
    ui.add_space(4.0);

    egui::ScrollArea::vertical()
        .id_salt("preflight-list")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for (rule, problems) in &sections {
                if let Some(asked) = section(ui, state, *rule, problems) {
                    ask = Some(asked);
                }
            }
        });

    if let Some(ask) = ask {
        run(state, ask);
    }
}

/// The problems under the rule that found each, rules in the order the report
/// has them — errors first — and each rule's problems in the order found.
///
/// Grouped, because ten low-resolution images are one decision about
/// resolution rather than ten separate discoveries.
fn sections<'a>(problems: &[&'a Problem]) -> Vec<(Rule, Vec<&'a Problem>)> {
    let mut out: Vec<(Rule, Vec<&Problem>)> = Vec::new();
    for problem in problems {
        match out.iter_mut().find(|(rule, _)| *rule == problem.rule) {
            Some((_, list)) => list.push(problem),
            None => out.push((problem.rule, vec![problem])),
        }
    }
    out
}

fn key(problem: &Problem) -> (Rule, Where, String) {
    (problem.rule, problem.at, problem.message.clone())
}

fn count(n: usize, what: &str) -> String {
    if n == 1 {
        format!("1 {what}")
    } else {
        format!("{n} {what}s")
    }
}

// --- the top of the panel --------------------------------------------------------

/// What it comes to: whether the job can go, in a word and a sentence.
fn verdict(ui: &mut Ui, state: &mut TesseraApp, report: &Report, checks: Checks) {
    let (errors, warnings) = (report.errors(), report.warnings());
    let (icon, tint, title) = verdict_words(errors, warnings);
    let mut detail = match (errors, warnings) {
        // "No problems" here means none of the kinds these checks look for,
        // and a green light would claim more than that.
        (0, 0) => "Nothing the checks look for is wrong. A look through the proof is \
                   still owed."
            .to_string(),
        (0, w) => format!("{} to look at before it goes.", count(w, "warning")),
        (e, 0) => format!(
            "{} to fix: as it is, it comes back wrong.",
            count(e, "error")
        ),
        (e, w) => format!(
            "{} to fix, and {} to look at.",
            count(e, "error"),
            count(w, "warning")
        ),
    };
    let off = checks.off();
    if off > 0 {
        detail.push_str(&format!(
            " {} switched off.",
            if off == 1 {
                "1 check is".to_string()
            } else {
                format!("{off} checks are")
            }
        ));
    }

    style_ui::card(ui, None, |ui| {
        ui.horizontal(|ui| {
            let (mark, _) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::hover());
            crate::icons::paint_rotated(ui.painter(), mark.shrink(3.0), icon, tint, 0.0);
            ui.vertical(|ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(title)
                            .font(style_ui::heading_font(15.0))
                            .color(Theme::text_primary()),
                    )
                    .wrap()
                    .selectable(false),
                );
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(detail)
                            .size(Theme::TYPE_SM)
                            .color(Theme::text_muted()),
                    )
                    .wrap()
                    .selectable(false),
                );
            });
        });
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if panel_ui::action(ui, Icon::RotateCw, "Check again")
                .on_hover_text(
                    "Read the linked files again. Everything else is checked as the \
                     document changes.",
                )
                .clicked()
            {
                // The disk itself, this moment: what "Check again" promises.
                for link in state.active().document().links.values() {
                    tessera_io::seen::look_now(&link.path);
                }
                state.preflight.recheck();
                state.links.recheck();
            }
            let on = Rule::ALL.len() - off;
            let label = format!("Checks {on} of {}", Rule::ALL.len());
            if panel_ui::action(ui, Icon::Properties, &label)
                .on_hover_text("Choose what preflight looks for")
                .clicked()
            {
                state.preflight.view.checks = !state.preflight.view.checks;
            }
        });
    });
}

/// The mark, its colour and the word for a count of errors and warnings.
fn verdict_words(errors: usize, warnings: usize) -> (Icon, Color32, &'static str) {
    match (errors, warnings) {
        (0, 0) => (Icon::Preflight, Theme::text_muted(), "No problems found"),
        (0, _) => (Icon::WarningMark, Theme::accent(), "Ready, with warnings"),
        _ => (Icon::ErrorMark, Theme::error(), "Not ready to print"),
    }
}

/// Which checks run, and what they check against.
fn checks_card(ui: &mut Ui, state: &mut TesseraApp, checks: Checks) {
    style_ui::card(ui, Some("Checks"), |ui| {
        for rule in Rule::ALL {
            let mut on = checks.on(rule);
            ui.horizontal(|ui| {
                let changed = ui
                    .checkbox(&mut on, rule.title())
                    .on_hover_text(rule.why())
                    .changed();
                if changed {
                    crate::preflight::set_check(state, rule, on);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (mark, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                    let severity = rule.severity();
                    crate::icons::paint(
                        ui.painter(),
                        mark,
                        marker(severity),
                        severity_colour(severity),
                    );
                });
            });
        }
        ui.add_space(6.0);
        crate::view::panels::field(ui, "Resolution", |ui| {
            let mut ppi = state.prefs.minimum_ppi;
            ui.add(
                egui::DragValue::new(&mut ppi)
                    .speed(1.0)
                    .range(24.0..=1200.0)
                    .suffix(" ppi"),
            )
            .on_hover_text("300 for offset litho, 150 for newsprint, 72 for a screen PDF");
            state.prefs.minimum_ppi = ppi;
        });
        let bleed = bleed(state);
        let said = if bleed > 0.0 {
            format!(
                "The bleed is {}, from the document's setup.",
                state.prefs.unit.format(bleed)
            )
        } else {
            "The document has no bleed, so nothing is checked against one.".to_string()
        };
        panel_ui::hint(ui, &said);
        if checks.off() > 0 {
            ui.add_space(4.0);
            if ui.button("Turn every check on").clicked() {
                state.prefs.preflight_off.clear();
            }
        }
    });
}

/// The document's bleed, the smallest edge of it: what the check measures
/// against.
fn bleed(state: &TesseraApp) -> f64 {
    let b = state.active().document().setup.bleed;
    b.top.min(b.bottom).min(b.left).min(b.right).max(0.0)
}

/// The counts, each a way to see only those problems.
fn filters(ui: &mut Ui, state: &mut TesseraApp, report: &Report) {
    let (errors, warnings) = (report.errors(), report.warnings());
    let view = &mut state.preflight.view;
    // A filter whose problems are all fixed shows everything again rather
    // than an empty list.
    if view
        .filter
        .is_some_and(|s| report.problems.iter().all(|p| p.severity() != s))
    {
        view.filter = None;
    }
    if errors == 0 || warnings == 0 {
        // One kind only: a filter would show what is shown already.
        return;
    }
    let mut choice = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::splat(4.0);
        for (filter, text, tint) in [
            (
                None,
                format!("All {}", errors + warnings),
                Theme::text_primary(),
            ),
            (
                Some(Severity::Error),
                count(errors, "error"),
                Theme::error(),
            ),
            (
                Some(Severity::Warning),
                count(warnings, "warning"),
                Theme::accent(),
            ),
        ] {
            let on = view.filter == filter;
            if super::links::chip(ui, &text, tint, on).clicked() {
                // A second click on a count shows everything again.
                choice = Some(if on { None } else { filter });
            }
        }
    });
    if let Some(filter) = choice {
        view.filter = filter;
    }
    ui.add_space(2.0);
}

/// Previous and Next, through every problem listed, and where in them the
/// one gone to is.
fn walk(ui: &mut Ui, state: &mut TesseraApp, order: &[&Problem]) -> Option<Ask> {
    let n = order.len();
    if n == 0 {
        return None;
    }
    let at = state
        .preflight
        .view
        .current
        .as_ref()
        .and_then(|c| order.iter().position(|p| key(p) == *c));
    let mut go = None;
    ui.horizontal_wrapped(|ui| {
        if panel_ui::action(ui, Icon::ChevronLeft, "Previous")
            .on_hover_text("Go to the problem before")
            .clicked()
        {
            go = Some(at.map_or(n - 1, |i| (i + n - 1) % n));
        }
        if panel_ui::action(ui, Icon::ChevronRight, "Next")
            .on_hover_text("Go to the next problem")
            .clicked()
        {
            go = Some(at.map_or(0, |i| (i + 1) % n));
        }
        let place = match at {
            Some(i) => format!("{} of {n}", i + 1),
            None => count(n, "problem"),
        };
        ui.label(
            egui::RichText::new(place)
                .size(Theme::TYPE_SM)
                .color(Theme::text_muted()),
        );
    });
    let problem = order[go?];
    let view = &mut state.preflight.view;
    view.folded.remove(&problem.rule);
    // Past the rows a rule lists to begin with, the rest are listed so the
    // one gone to can be seen.
    let in_rule = order
        .iter()
        .filter(|p| p.rule == problem.rule)
        .position(|p| std::ptr::eq(*p, problem))
        .unwrap_or(0);
    if in_rule >= FIRST {
        view.whole.insert(problem.rule);
    }
    view.scroll = true;
    let (rule, at, message) = key(problem);
    Some(Ask::GoTo(rule, at, message))
}

// --- the list ----------------------------------------------------------------------

/// A rule: its heading, why it matters, what fixes all of it at once, and a
/// row for each problem it found.
fn section(ui: &mut Ui, state: &mut TesseraApp, rule: Rule, problems: &[&Problem]) -> Option<Ask> {
    let mut ask = None;
    let folded = state.preflight.view.folded.contains(&rule);
    let severity = rule.severity();

    ui.add_space(4.0);
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), HEADING), Sense::click());
    let painter = ui.painter_at(rect);
    if response.hovered() {
        painter.rect_filled(rect, 6.0, Theme::hover_bg());
    }
    let fold = Rect::from_center_size(
        egui::pos2(rect.left() + 10.0, rect.center().y),
        Vec2::splat(Theme::ICON_SIZE - 4.0),
    );
    crate::icons::paint_rotated(
        &painter,
        fold,
        Icon::ChevronRight,
        Theme::text_muted(),
        if folded { 0.0 } else { 90.0 },
    );
    let mark = Rect::from_center_size(
        egui::pos2(rect.left() + 30.0, rect.center().y),
        Vec2::splat(16.0),
    );
    crate::icons::paint(&painter, mark, marker(severity), severity_colour(severity));
    painter.text(
        egui::pos2(rect.left() + 44.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        rule.title(),
        style_ui::heading_font(Theme::TYPE_SM + 1.0),
        Theme::text_primary(),
    );
    // How many, in a pill at the right.
    let n = problems.len().to_string();
    let galley = painter.layout_no_wrap(
        n.clone(),
        egui::FontId::proportional(Theme::TYPE_SM),
        severity_colour(severity),
    );
    let pill = Rect::from_center_size(
        egui::pos2(
            rect.right() - 8.0 - (galley.size().x + 12.0) / 2.0,
            rect.center().y,
        ),
        Vec2::new(galley.size().x + 12.0, 18.0),
    );
    painter.rect_filled(pill, 9.0, Theme::panel_bg_alt());
    painter.galley(
        pill.center() - galley.size() / 2.0,
        galley,
        severity_colour(severity),
    );
    let response = crate::icons::reads_as(
        response,
        format!("{}, {n}", rule.title()),
        egui::WidgetType::CollapsingHeader,
        Some(!folded),
    )
    .on_hover_text(rule.why());
    if response.clicked() {
        ask = Some(Ask::Fold(rule));
    }
    if folded {
        return ask;
    }

    // What fixes every problem it found at once, the numbers it checks
    // against, and — under the rule being worked through, where it is being
    // read, rather than under every heading at once — why it matters.
    let wide = section_fixes(rule, problems);
    let facts = facts(state, rule);
    let working = state
        .preflight
        .view
        .current
        .as_ref()
        .is_some_and(|c| problems.iter().any(|p| key(p) == *c));
    // Nothing to say is no room taken: an empty block between a heading and
    // its rows reads as a missing line.
    let said = working || facts.is_some() || !wide.is_empty();
    if said {
        ui.horizontal(|ui| {
            ui.add_space(INDENT + 14.0);
            ui.vertical(|ui| {
                if working {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(rule.why())
                                .size(Theme::TYPE_SM)
                                .color(Theme::text_muted()),
                        )
                        .wrap()
                        .selectable(false),
                    );
                }
                if let Some(facts) = facts {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(facts)
                                .size(Theme::TYPE_SM)
                                .color(Theme::text_muted()),
                        )
                        .wrap()
                        .selectable(false),
                    );
                }
                if !wide.is_empty() {
                    ui.add_space(2.0);
                    ui.horizontal_wrapped(|ui| {
                        for (icon, label, hover, fix) in wide {
                            if panel_ui::action(ui, icon, label)
                                .on_hover_text(hover)
                                .clicked()
                            {
                                ask = Some(Ask::Fix(fix));
                            }
                        }
                    });
                }
            });
        });
        ui.add_space(2.0);
    }

    let whole = state.preflight.view.whole.contains(&rule);
    let listed = if whole {
        problems.len()
    } else {
        problems.len().min(FIRST)
    };
    for problem in &problems[..listed] {
        if let Some(asked) = problem_row(ui, state, problem) {
            ask = Some(asked);
        }
    }
    if listed < problems.len() {
        ui.horizontal(|ui| {
            ui.add_space(INDENT);
            if ui
                .button(format!("Show {} more", problems.len() - listed))
                .clicked()
            {
                ask = Some(Ask::Whole(rule));
            }
        });
    }
    ask
}

/// A line of numbers a rule checks against, said in the document's terms.
fn facts(state: &TesseraApp, rule: Rule) -> Option<String> {
    match rule {
        Rule::LowResolution => Some(format!(
            "Asked for: {:.0} ppi or more.",
            state.prefs.minimum_ppi
        )),
        Rule::OutsideBleed => Some(format!(
            "The bleed is {}.",
            state.prefs.unit.format(bleed(state))
        )),
        _ => None,
    }
}

/// What fixes every problem a rule found at once.
fn section_fixes(
    rule: Rule,
    problems: &[&Problem],
) -> Vec<(Icon, &'static str, &'static str, Fix)> {
    let mut links: Vec<LinkId> = Vec::new();
    for problem in problems {
        if let Subject::Link(link) = problem.subject
            && !links.contains(&link)
        {
            links.push(link);
        }
    }
    match rule {
        Rule::MissingLink => vec![(
            Icon::Link2,
            "Find missing…",
            "Choose a folder: every missing file found in it, or in the folders inside \
             it, is relinked there in one step",
            Fix::FindMissing,
        )],
        Rule::ModifiedLink if links.len() > 1 => vec![(
            Icon::RotateCw,
            "Update all",
            "Read every changed file again, in one step",
            Fix::UpdateAll(links),
        )],
        _ => Vec::new(),
    }
}

/// What fixes one problem, as buttons: the font and colour replacements,
/// which need a choice, are drawn as menus beside them.
fn fixes(
    problem: &Problem,
    doc: &tessera_document::document::Document,
) -> Vec<(Icon, &'static str, &'static str, Fix)> {
    match (problem.rule, &problem.subject, problem.at) {
        (Rule::OversetText, _, Where::Frame(id))
            if doc
                .frame(id)
                .is_some_and(|f| matches!(f.kind, FrameKind::Text { .. })) =>
        {
            vec![(
                Icon::ScaleY,
                "Fit frame to text",
                "Make the frame just tall enough to hold the rest of its text",
                Fix::FitFrame(id),
            )]
        }
        (Rule::MissingLink, Subject::Link(link), _) => vec![
            (
                Icon::PlaceImage,
                "Relink…",
                "Choose the file this should point at. Every frame showing it follows.",
                Fix::Relink(*link),
            ),
            (
                Icon::Link2,
                "Show in Links",
                "See the file in the Links panel",
                Fix::ShowInLinks(*link),
            ),
        ],
        (Rule::ModifiedLink, Subject::Link(link), _) => vec![
            (
                Icon::RotateCw,
                "Update",
                "Read the changed file again",
                Fix::Update(*link),
            ),
            (
                Icon::Link2,
                "Show in Links",
                "See the file in the Links panel",
                Fix::ShowInLinks(*link),
            ),
        ],
        (Rule::LowResolution, Subject::Link(link), _) => vec![(
            Icon::Link2,
            "Show in Links",
            "See its pixels, and every place it is used at what resolution",
            Fix::ShowInLinks(*link),
        )],
        (Rule::NoOutputIntent, ..) => vec![(
            Icon::Palette,
            "Choose a press…",
            "Choose the ICC profile of the press this will print on",
            Fix::ChoosePress,
        )],
        _ => Vec::new(),
    }
}

/// One problem: the object it is on, the page, what is wrong; opened out
/// with its fixes when it is the one gone to.
fn problem_row(ui: &mut Ui, state: &mut TesseraApp, problem: &Problem) -> Option<Ask> {
    let mut ask = None;
    let doc = state.active().document();
    let (icon, title) = match problem.at {
        Where::Frame(id) => doc
            .frame(id)
            .map(|frame| super::layers::describe(doc, frame))
            .unwrap_or((Icon::Rectangle, "An object".to_string())),
        Where::Page(page) => (
            Icon::Pages,
            doc.page_label(page)
                .map_or("A page".to_string(), |folio| format!("Page {folio}")),
        ),
        Where::Document => match &problem.subject {
            Subject::Family(family) => (Icon::Text, family.clone()),
            Subject::Swatch(name) => (Icon::Swatches, format!("\u{201c}{name}\u{201d}")),
            _ => (Icon::Properties, "The document".to_string()),
        },
    };
    let place = match problem.at {
        Where::Frame(id) => Some(super::links::spot(doc, id).short()),
        Where::Page(_) | Where::Document => None,
    };
    let buttons = fixes(problem, doc);
    let this = key(problem);
    let current = state.preflight.view.current.as_ref() == Some(&this);
    // A problem about the document goes nowhere when clicked, so what fixes
    // it is shown without asking.
    let open = current || problem.at == Where::Document;

    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW), Sense::click());
    let painter = ui.painter_at(rect);
    if current {
        painter.rect_filled(rect, 6.0, Theme::accent_soft());
    } else if response.hovered() {
        painter.rect_filled(rect, 6.0, Theme::hover_bg());
    }
    let glyph = Rect::from_center_size(
        egui::pos2(rect.left() + INDENT / 2.0 + 14.0, rect.top() + 13.0),
        Vec2::splat(Theme::ICON_SIZE - 2.0),
    );
    crate::icons::paint(&painter, glyph, icon, Theme::text_muted());

    let left = rect.left() + INDENT + 14.0;
    let right = rect.right() - 8.0;
    let small = egui::FontId::proportional(Theme::TYPE_SM);
    let mut room = right - left;
    if let Some(place) = place {
        let place = painter.layout_no_wrap(place, small.clone(), Theme::text_muted());
        room -= place.size().x + 8.0;
        painter.galley(
            egui::pos2(right - place.size().x, rect.top() + 7.0),
            place,
            Theme::text_muted(),
        );
    }
    let mut job = egui::text::LayoutJob::simple_singleline(
        title.clone(),
        egui::TextStyle::Body.resolve(ui.style()),
        Theme::text_primary(),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(room.max(12.0));
    let name = painter.layout_job(job);
    painter.galley(
        egui::pos2(left, rect.top() + 5.0),
        name,
        Theme::text_primary(),
    );
    let mut job = egui::text::LayoutJob::simple_singleline(
        problem.message.clone(),
        small,
        Theme::text_muted(),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width((right - left).max(12.0));
    let line = painter.layout_job(job);
    painter.galley(
        egui::pos2(left, rect.bottom() - 6.0 - line.size().y),
        line,
        Theme::text_muted(),
    );

    let goes = problem.at != Where::Document;
    let response = crate::icons::reads_as(
        response,
        title,
        egui::WidgetType::SelectableLabel,
        Some(current),
    )
    .on_hover_text(if goes {
        format!("{}\nClick to go to it", problem.message)
    } else {
        format!("{}\nAbout the document as a whole", problem.message)
    });
    if current && state.preflight.view.scroll {
        response.scroll_to_me(Some(egui::Align::Center));
        state.preflight.view.scroll = false;
    }
    if response.clicked() {
        let (rule, at, message) = this.clone();
        ask = Some(Ask::GoTo(rule, at, message));
    }
    response.context_menu(|ui| {
        if goes && ui.button("Go to").clicked() {
            let (rule, at, message) = this.clone();
            ask = Some(Ask::GoTo(rule, at, message));
            ui.close();
        }
        for (_, label, _, fix) in &buttons {
            if ui.button(*label).clicked() {
                ask = Some(Ask::Fix(fix.clone()));
                ui.close();
            }
        }
    });

    if open {
        let chosen = ui
            .horizontal_wrapped(|ui| {
                ui.add_space(INDENT + 14.0);
                let mut chosen = None;
                for (icon, label, hover, fix) in &buttons {
                    if panel_ui::action(ui, *icon, label)
                        .on_hover_text(*hover)
                        .clicked()
                    {
                        chosen = Some(fix.clone());
                    }
                }
                match (&problem.rule, &problem.subject) {
                    (Rule::MissingFont, Subject::Family(family)) => {
                        if let Some(to) = family_menu(ui, state, family) {
                            chosen = Some(Fix::ReplaceFamily {
                                from: family.clone(),
                                to,
                            });
                        }
                    }
                    (Rule::UnresolvedSwatch, Subject::Swatch(name)) => {
                        if let Some(to) = swatch_menu(ui, state, name) {
                            chosen = Some(Fix::RepointSwatch {
                                from: name.clone(),
                                to,
                            });
                        }
                    }
                    _ => {}
                }
                chosen
            })
            .inner;
        if let Some(fix) = chosen {
            ask = Some(Ask::Fix(fix));
        }
        ui.add_space(4.0);
    }
    ask
}

/// "Replace with…": every family this machine can set, the ones used lately
/// first.
fn family_menu(ui: &mut Ui, state: &mut TesseraApp, missing: &str) -> Option<String> {
    let mut chosen = None;
    let recent: Vec<String> = state
        .recent_fonts
        .iter()
        .filter(|f| *f != missing)
        .cloned()
        .collect();
    let response = egui::ComboBox::from_id_salt(("preflight-family", missing))
        .selected_text("Replace with…")
        .width(160.0)
        .height(320.0)
        .show_ui(ui, |ui| {
            if !recent.is_empty() {
                style_ui::overline(ui, "Recent");
                for family in &recent {
                    if ui.selectable_label(false, family).clicked() {
                        chosen = Some(family.clone());
                    }
                }
                ui.separator();
            }
            for family in state.shaper.families() {
                if family != missing && ui.selectable_label(false, family).clicked() {
                    chosen = Some(family.clone());
                }
            }
        })
        .response;
    crate::icons::reads_as(
        response,
        format!("Replace {missing} with"),
        egui::WidgetType::ComboBox,
        None,
    )
    .on_hover_text("Set everything in this font in another, everywhere it is named");
    chosen
}

/// "Replace with…": the swatches the document defines.
fn swatch_menu(ui: &mut Ui, state: &TesseraApp, missing: &str) -> Option<String> {
    let names: Vec<String> = state
        .active()
        .document()
        .swatches
        .iter()
        .map(|s| s.name.clone())
        .collect();
    if names.is_empty() {
        panel_ui::hint(ui, "Define a swatch to hand its uses to.");
        return None;
    }
    let mut chosen = None;
    let response = egui::ComboBox::from_id_salt(("preflight-swatch", missing))
        .selected_text("Replace with…")
        .width(160.0)
        .show_ui(ui, |ui| {
            for name in &names {
                if ui.selectable_label(false, name).clicked() {
                    chosen = Some(name.clone());
                }
            }
        })
        .response;
    crate::icons::reads_as(
        response,
        format!("Replace {missing} with"),
        egui::WidgetType::ComboBox,
        None,
    )
    .on_hover_text(
        "Colour everything that names it with a swatch the document has, at the same tint",
    );
    chosen
}

/// Do what the panel was asked.
fn run(state: &mut TesseraApp, ask: Ask) {
    match ask {
        Ask::GoTo(rule, at, message) => {
            state.preflight.view.current = Some((rule, at, message));
            go_to(state, at);
        }
        Ask::Fold(rule) => {
            let folded = &mut state.preflight.view.folded;
            if !folded.remove(&rule) {
                folded.insert(rule);
            }
        }
        Ask::Whole(rule) => {
            state.preflight.view.whole.insert(rule);
        }
        Ask::Fix(fix) => mend(state, fix),
    }
}

fn mend(state: &mut TesseraApp, fix: Fix) {
    match fix {
        Fix::FitFrame(id) => apply(state, Command::FitFrameToText { id }),
        Fix::Relink(link) => {
            if let Some(path) = crate::file_ops::pick_artwork() {
                apply(state, Command::Relink { link, path });
                state.links.recheck();
            }
        }
        Fix::Update(link) => {
            apply(state, Command::UpdateLink { link });
            state.links.recheck();
        }
        Fix::UpdateAll(links) => {
            apply(state, Command::UpdateLinks { links });
            state.links.recheck();
        }
        Fix::FindMissing => super::links::find_missing(state),
        Fix::ShowInLinks(link) => super::links::show(state, link),
        Fix::ChoosePress => crate::file_ops::choose_output_intent(state),
        Fix::ReplaceFamily { from, to } => {
            super::panels::remember_font(&mut state.recent_fonts, &to);
            apply(state, Command::ReplaceFamily { from, to });
        }
        Fix::RepointSwatch { from, to } => apply(state, Command::RepointSwatch { from, to }),
    }
}

/// Bring a problem into view: its object selected and shown — on its parent
/// page, when that is where it is — or its page turned to.
///
/// Both select and show, and neither alone. Selecting without scrolling
/// leaves the offender off screen — which is very often *why* it was not
/// noticed. Scrolling without selecting puts somebody in the right place with
/// no idea which object was meant.
fn go_to(state: &mut TesseraApp, at: Where) {
    match at {
        Where::Frame(id) => crate::view::styles::reveal_object(state, id),
        Where::Page(page) => crate::view::pages::turn_to(state, page),
        Where::Document => {}
    }
}

/// The status-bar indicator.
///
/// **The point of the whole feature.** A person who has to open a panel to find
/// out whether their document is sendable will open it once, at the beginning,
/// and never again. One word in the corner is what makes preflight something
/// that runs rather than something that is run.
pub fn indicator(ui: &mut Ui, state: &mut TesseraApp) {
    let report = crate::preflight::Preflight::report(state);
    let (errors, warnings) = (report.errors(), report.warnings());
    let summary = report.summary();
    let off = crate::preflight::checks(state).off();
    let tint = colour_for(errors, warnings);
    let icon = match (errors, warnings) {
        (0, 0) => Icon::Preflight,
        (0, _) => Icon::WarningMark,
        _ => Icon::ErrorMark,
    };

    // Drawn as one piece, the mark and the words, so it reads the same in
    // whichever direction the status bar lays its right-hand side out.
    let galley = ui.painter().layout_no_wrap(
        summary.clone(),
        egui::FontId::proportional(Theme::TYPE_SM),
        tint,
    );
    let size = Vec2::new(14.0 + 5.0 + galley.size().x, galley.size().y.max(14.0));
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let mark = Rect::from_center_size(
        egui::pos2(rect.left() + 7.0, rect.center().y),
        Vec2::splat(14.0),
    );
    crate::icons::paint(ui.painter(), mark, icon, tint);
    ui.painter().galley(
        egui::pos2(rect.left() + 19.0, rect.center().y - galley.size().y / 2.0),
        galley,
        tint,
    );
    let hover = if off > 0 {
        format!(
            "Preflight. Click to open the panel.\n{} switched off.",
            if off == 1 {
                "1 check is".to_string()
            } else {
                format!("{off} checks are")
            }
        )
    } else {
        "Preflight. Click to open the panel.".to_string()
    };
    let response =
        crate::icons::named(response, format!("Preflight: {summary}")).on_hover_text(hover);

    if response.clicked() {
        state.preflight.open = true;
        state.rail_open = true;
        state.prefs.docking.reveal("Preflight");
    }
}

/// What colour to say it in.
///
/// Green is deliberately **not** used for "no problems". A green light invites
/// somebody to stop reading, and "no problems" here means "no problems this
/// checks for" — the hand check is still owed. Muted says it without claiming
/// more than was established.
fn colour_for(errors: usize, warnings: usize) -> Color32 {
    if errors > 0 {
        Theme::error()
    } else if warnings > 0 {
        Theme::accent()
    } else {
        Theme::text_muted()
    }
}

fn severity_colour(severity: Severity) -> Color32 {
    match severity {
        Severity::Error => Theme::error(),
        Severity::Warning => Theme::accent(),
    }
}

/// A shape, not only a colour.
///
/// Roughly one man in twelve cannot tell the red from the amber, and a panel
/// that says "this one is worse" only in hue says it to eleven of them.
fn marker(severity: Severity) -> Icon {
    match severity {
        Severity::Error => Icon::ErrorMark,
        Severity::Warning => Icon::WarningMark,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_document_is_not_reported_in_green() {
        // A green light invites somebody to stop reading, and "no problems"
        // means "no problems this checks for".
        assert_eq!(colour_for(0, 0), Theme::text_muted());
    }

    #[test]
    fn errors_outrank_warnings_in_the_indicator() {
        assert_eq!(colour_for(1, 9), Theme::error());
        assert_eq!(colour_for(0, 1), Theme::accent());
    }

    #[test]
    fn severity_is_shown_as_a_shape_as_well_as_a_colour() {
        // One man in twelve cannot tell the red from the amber.
        assert_ne!(marker(Severity::Error), marker(Severity::Warning));
        assert_ne!(
            marker(Severity::Error).paths(),
            marker(Severity::Warning).paths()
        );
    }

    #[test]
    fn the_panel_starts_shut() {
        let state = TesseraApp::headless();
        assert!(!state.preflight.open);
    }

    /// A text frame on the first page, `height` tall, saying `text`.
    fn a_text_frame(state: &mut TesseraApp, y: f64, height: f64, text: &str) -> FrameId {
        let mut b = state.first_page_bounds();
        b.x += 40.0;
        b.y += y;
        b.width = 200.0;
        b.height = height;
        apply(state, Command::AddTextFrame(b));
        let id = state.active().selection.single().expect("selected");
        apply(
            state,
            Command::SetText {
                id,
                text: text.to_string(),
            },
        );
        id
    }

    /// A document with text too long for its frame, a caption in a face
    /// nobody has, and a panel in a colour nobody defined — three errors —
    /// and no press, a warning.
    fn troubled() -> (TesseraApp, FrameId, FrameId, FrameId) {
        let mut state = TesseraApp::headless();
        let long = a_text_frame(
            &mut state,
            40.0,
            30.0,
            &"The harbour wakes before the town does. ".repeat(12),
        );
        let caption = a_text_frame(&mut state, 200.0, 40.0, "A caption");
        let FrameKind::Text { story, .. } = state.active().document().frames[caption].kind else {
            panic!("text")
        };
        apply(
            &mut state,
            Command::SetCharacterFormat {
                story,
                range: 0..9,
                format: tessera_text::story::CharacterFormat {
                    family: Some("Definitely Not Installed Sans".into()),
                    ..Default::default()
                },
            },
        );
        let mut b = state.first_page_bounds();
        b.x += 40.0;
        b.y += 300.0;
        b.width = 60.0;
        b.height = 60.0;
        apply(&mut state, Command::AddRectangle(b));
        let panel = state.active().selection.single().expect("selected");
        apply(
            &mut state,
            Command::SetFill {
                id: panel,
                paint: tessera_document::paint::Paint::Solid(tessera_color::Color::Swatch {
                    name: "Harbour teal".into(),
                    tint: 1.0,
                }),
            },
        );
        state.active_mut().selection.clear();
        (state, long, caption, panel)
    }

    fn rules(state: &mut TesseraApp) -> Vec<Rule> {
        crate::preflight::Preflight::report(state)
            .problems
            .iter()
            .map(|p| p.rule)
            .collect()
    }

    #[test]
    fn problems_are_listed_under_their_rule_errors_first() {
        let (mut state, ..) = troubled();
        let report = crate::preflight::Preflight::report(&mut state).clone();
        let all: Vec<&Problem> = report.problems.iter().collect();
        let listed: Vec<Rule> = sections(&all).iter().map(|(rule, _)| *rule).collect();
        assert_eq!(
            listed,
            [
                Rule::OversetText,
                Rule::UnresolvedSwatch,
                Rule::MissingFont,
                Rule::NoOutputIntent
            ]
        );
    }

    #[test]
    fn fitting_the_frame_to_its_text_clears_the_overset_in_one_step() {
        let (mut state, long, ..) = troubled();
        assert!(rules(&mut state).contains(&Rule::OversetText));
        let depth = state.active().history.undo_depth();
        mend(&mut state, Fix::FitFrame(long));
        assert!(!rules(&mut state).contains(&Rule::OversetText));
        assert!(state.active().document().frames[long].bounds.height > 30.0);
        assert_eq!(state.active().history.undo_depth(), depth + 1);
        apply(&mut state, Command::Undo);
        assert!(rules(&mut state).contains(&Rule::OversetText));
    }

    #[test]
    fn a_missing_font_replaced_is_gone_from_the_report() {
        let (mut state, _, caption, _) = troubled();
        let depth = state.active().history.undo_depth();
        // One this machine can set: the default, which a new document is
        // set in.
        let family = state.active().document().text_default.family.clone();
        mend(
            &mut state,
            Fix::ReplaceFamily {
                from: "Definitely Not Installed Sans".into(),
                to: family.clone(),
            },
        );
        assert!(!rules(&mut state).contains(&Rule::MissingFont));
        assert_eq!(state.active().history.undo_depth(), depth + 1);
        assert_eq!(state.recent_fonts.first(), Some(&family), "remembered");
        let FrameKind::Text { story, .. } = state.active().document().frames[caption].kind else {
            panic!("text")
        };
        assert_eq!(
            state.active().document().stories[story].runs[0]
                .local
                .family
                .as_deref(),
            Some(family.as_str())
        );
    }

    #[test]
    fn a_colour_nobody_defined_handed_to_a_swatch_is_gone_from_the_report() {
        let (mut state, _, _, panel) = troubled();
        apply(
            &mut state,
            Command::SetSwatch(tessera_document::nodes::Swatch::new(
                "Teal",
                tessera_color::Color::BLACK,
            )),
        );
        mend(
            &mut state,
            Fix::RepointSwatch {
                from: "Harbour teal".into(),
                to: "Teal".into(),
            },
        );
        assert!(!rules(&mut state).contains(&Rule::UnresolvedSwatch));
        assert_eq!(
            state.active().document().frames[panel].fill,
            tessera_document::paint::Paint::Solid(tessera_color::Color::Swatch {
                name: "Teal".into(),
                tint: 1.0
            })
        );
    }

    #[test]
    fn each_problem_offers_what_fixes_it() {
        let (state, long, ..) = troubled();
        let doc = state.active().document();
        let problem = |rule, at, subject| Problem {
            rule,
            message: String::new(),
            at,
            subject,
        };
        let labels = |p: &Problem| -> Vec<&str> { fixes(p, doc).iter().map(|f| f.1).collect() };
        assert_eq!(
            labels(&problem(
                Rule::OversetText,
                Where::Frame(long),
                Subject::None
            )),
            ["Fit frame to text"]
        );
        let link = LinkId::default();
        assert_eq!(
            labels(&problem(
                Rule::MissingLink,
                Where::Document,
                Subject::Link(link)
            )),
            ["Relink…", "Show in Links"]
        );
        assert_eq!(
            labels(&problem(
                Rule::ModifiedLink,
                Where::Document,
                Subject::Link(link)
            )),
            ["Update", "Show in Links"]
        );
        assert_eq!(
            labels(&problem(
                Rule::NoOutputIntent,
                Where::Document,
                Subject::None
            )),
            ["Choose a press…"]
        );
        assert!(
            labels(&problem(
                Rule::OutsideBleed,
                Where::Frame(long),
                Subject::None
            ))
            .is_empty()
        );
        let two = [
            &problem(Rule::ModifiedLink, Where::Document, Subject::Link(link)),
            &problem(Rule::ModifiedLink, Where::Document, Subject::Link(link)),
        ];
        assert!(
            section_fixes(Rule::ModifiedLink, &two).is_empty(),
            "one file twice is one Update, not Update all"
        );
    }

    // --- the panel, used -------------------------------------------------------

    fn panel(
        ctx: &egui::Context,
        state: &mut TesseraApp,
        events: Vec<egui::Event>,
    ) -> Vec<(String, egui::Rect)> {
        let output = crate::headless_frame::frame(
            ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(300.0, 1400.0),
                )),
                events,
                ..Default::default()
            },
            |ui| docked(ui, state),
        );
        output
            .platform_output
            .accesskit_update
            .map(|update| {
                update
                    .nodes
                    .iter()
                    .filter_map(|(_, node)| {
                        let b = node.bounds()?;
                        Some((
                            node.label()?.to_string(),
                            egui::Rect::from_min_max(
                                egui::pos2(b.x0 as f32, b.y0 as f32),
                                egui::pos2(b.x1 as f32, b.y1 as f32),
                            ),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn a_panel() -> egui::Context {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        ctx.enable_accesskit();
        ctx
    }

    fn labels(ctx: &egui::Context, state: &mut TesseraApp) -> Vec<String> {
        panel(ctx, state, Vec::new());
        panel(ctx, state, Vec::new())
            .into_iter()
            .map(|(name, _)| name)
            .collect()
    }

    fn click(ctx: &egui::Context, state: &mut TesseraApp, label: &str) {
        panel(ctx, state, Vec::new());
        let nodes = panel(ctx, state, Vec::new());
        let pos = nodes
            .iter()
            .find(|(name, _)| name == label)
            .unwrap_or_else(|| panic!("no {label:?} in {nodes:#?}"))
            .1
            .center();
        for pressed in [true, false] {
            panel(
                ctx,
                state,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
    }

    #[test]
    fn the_verdict_says_whether_it_can_go() {
        assert_eq!(verdict_words(2, 5).2, "Not ready to print");
        assert_eq!(verdict_words(0, 5).2, "Ready, with warnings");
        assert_eq!(verdict_words(0, 0).2, "No problems found");
        assert_eq!(verdict_words(0, 0).1, Theme::text_muted(), "not green");
        assert_ne!(verdict_words(1, 0).0, verdict_words(0, 1).0, "two shapes");
    }

    #[test]
    fn next_walks_the_problems_and_goes_to_each() {
        let (mut state, long, caption, panel) = troubled();
        let ctx = a_panel();
        click(&ctx, &mut state, "Next");
        assert_eq!(state.active().selection.single(), Some(long), "the first");
        assert_eq!(state.reveal, Some(long));
        click(&ctx, &mut state, "Next");
        assert_eq!(state.active().selection.single(), Some(panel));
        click(&ctx, &mut state, "Next");
        assert_eq!(state.active().selection.single(), Some(caption));
        assert_eq!(
            state.preflight.view.current.as_ref().map(|c| c.0),
            Some(Rule::MissingFont),
            "the third"
        );
        click(&ctx, &mut state, "Previous");
        assert_eq!(state.active().selection.single(), Some(panel));
    }

    #[test]
    fn a_row_clicked_is_gone_to_and_opens_out_with_its_fix() {
        let (mut state, long, ..) = troubled();
        let ctx = a_panel();
        assert!(
            !labels(&ctx, &mut state)
                .iter()
                .any(|l| l == "Fit frame to text"),
            "fixes wait for the row to be chosen"
        );
        let title = super::super::layers::describe(
            state.active().document(),
            &state.active().document().frames[long],
        )
        .1;
        click(&ctx, &mut state, &title);
        assert_eq!(state.active().selection.single(), Some(long));
        assert!(
            labels(&ctx, &mut state)
                .iter()
                .any(|l| l == "Fit frame to text")
        );
        click(&ctx, &mut state, "Fit frame to text");
        assert!(!rules(&mut state).contains(&Rule::OversetText));
    }

    #[test]
    fn a_problem_about_the_document_shows_its_fix_unasked() {
        let (mut state, ..) = troubled();
        let ctx = a_panel();
        assert!(
            labels(&ctx, &mut state)
                .iter()
                .any(|l| l == "Choose a press…")
        );
    }

    #[test]
    fn a_rule_folds_away_its_rows() {
        let (mut state, ..) = troubled();
        let ctx = a_panel();
        assert!(labels(&ctx, &mut state).iter().any(|l| l == "Rectangle"));
        click(&ctx, &mut state, "Unresolved swatch, 1");
        assert!(
            state
                .preflight
                .view
                .folded
                .contains(&Rule::UnresolvedSwatch)
        );
        assert!(!labels(&ctx, &mut state).iter().any(|l| l == "Rectangle"));
    }

    #[test]
    fn a_count_shows_only_its_problems() {
        let (mut state, ..) = troubled();
        let ctx = a_panel();
        click(&ctx, &mut state, "1 warning");
        assert_eq!(state.preflight.view.filter, Some(Severity::Warning));
        let shown = labels(&ctx, &mut state);
        assert!(!shown.iter().any(|l| l == "Rectangle"), "{shown:?}");
        assert!(shown.iter().any(|l| l == "The document"));
        click(&ctx, &mut state, "1 warning");
        assert_eq!(state.preflight.view.filter, None);
    }

    #[test]
    fn a_check_switched_off_in_the_panel_is_not_run() {
        let (mut state, ..) = troubled();
        let ctx = a_panel();
        click(&ctx, &mut state, "Checks 9 of 9");
        assert!(state.preflight.view.checks);
        click(&ctx, &mut state, "Missing font");
        assert_eq!(state.prefs.preflight_off, ["missing-font"]);
        assert!(!rules(&mut state).contains(&Rule::MissingFont));
        let shown = labels(&ctx, &mut state);
        assert!(shown.iter().any(|l| l == "Checks 8 of 9"), "{shown:?}");
        click(&ctx, &mut state, "Turn every check on");
        assert!(state.prefs.preflight_off.is_empty());
    }
}
