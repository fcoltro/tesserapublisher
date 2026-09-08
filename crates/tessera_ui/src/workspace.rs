//! Named arrangements of the interface.
//!
//! A layout tool is used for very different jobs by the same person in one
//! afternoon. Setting a long document wants Pages and Styles and no Swatches;
//! finishing artwork for press wants Swatches and Preflight and no Pages. The
//! arrangement that suits one is in the way of the other, and rebuilding it each
//! time is the tax this removes.
//!
//! ## What a workspace remembers, and what it does not
//!
//! It remembers **which panels are open, in what order, and how wide the rail
//! is**. That is the arrangement.
//!
//! It does not remember which *document* was open, where the view was scrolled
//! to, or what was selected. Those belong to the work, not to the way of
//! working — and a workspace that restored them would mean switching from
//! "Layout" to "Prepress" threw away your place on the page.
//!
//! It also does not remember the theme or the panel surface. Those are
//! preferences about how the application looks to *you*, not about which panels
//! this job needs, and folding them in would mean a workspace could turn the
//! lights on and off.

use serde::{Deserialize, Serialize};

use crate::app::TesseraApp;
use crate::view::rail::Dock;

/// One saved arrangement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    /// The panels that are open, in the order the rail shows them.
    ///
    /// Stored as names rather than as the enum, so a workspace saved by an
    /// older build survives a panel being added or renamed: an unknown name is
    /// skipped, and a panel this build has that the workspace does not mention
    /// simply stays shut.
    pub open: Vec<String>,
    /// Whether the rail is expanded or collapsed to its strip.
    pub rail_open: bool,
    /// Where the open panels sit: sides, stacks and tab order.
    ///
    /// Defaulted rather than required, so a workspace saved before panels could
    /// be arranged still applies — it simply lands them in the shipped layout,
    /// which is what it meant when it was saved.
    #[serde(default)]
    pub docking: crate::docking::Docking,
}

impl Workspace {
    /// The arrangement as it stands.
    pub fn capture(name: impl Into<String>, state: &TesseraApp) -> Self {
        Self {
            name: name.into(),
            open: Dock::ALL
                .iter()
                .filter(|dock| dock.is_open(state))
                .map(|dock| dock.title().to_string())
                .collect(),
            rail_open: state.rail_open,
            docking: state.prefs.docking.clone(),
        }
    }

    /// Put the interface into this arrangement.
    ///
    /// Every panel this build has is set, not only the ones named: applying a
    /// workspace has to *close* what it does not ask for, or switching from a
    /// crowded arrangement to a spare one would leave the crowd behind.
    pub fn apply(&self, state: &mut TesseraApp) {
        for dock in Dock::ALL {
            let wanted = self.open.iter().any(|name| name == dock.title());
            dock.set_open(state, wanted);
        }
        state.rail_open = self.rail_open;
        state.prefs.docking = self.docking.clone();
        // The saved layout may name panels this build has dropped, or miss ones
        // it has gained. Reconciling here rather than at draw time means the
        // arrangement is sound the moment it is applied.
        state.prefs.docking.reconcile();
    }

    /// The arrangements Tessera comes with.
    ///
    /// **Named for jobs, not for panels.** "Layout" and "Prepress" are things
    /// somebody is doing; "Pages + Styles" is a list they would have to decode.
    pub fn usual() -> Vec<Workspace> {
        vec![
            Workspace {
                name: "Essentials".to_string(),
                open: vec!["Properties".to_string()],
                rail_open: true,
                docking: crate::docking::Docking::default(),
            },
            Workspace {
                name: "Layout".to_string(),
                open: vec![
                    "Properties".to_string(),
                    "Pages".to_string(),
                    "Layers".to_string(),
                ],
                rail_open: true,
                docking: crate::docking::Docking::default(),
            },
            Workspace {
                name: "Typography".to_string(),
                open: vec!["Properties".to_string(), "Styles".to_string()],
                rail_open: true,
                docking: crate::docking::Docking::default(),
            },
            Workspace {
                name: "Prepress".to_string(),
                open: vec![
                    "Properties".to_string(),
                    "Swatches".to_string(),
                    "Preflight".to_string(),
                ],
                rail_open: true,
                docking: crate::docking::Docking::default(),
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_arrangement_of_panels_survives_switching_away_and_back() {
        // **The milestone's acceptance sentence, as a test.** Arrange the
        // panels, save that as a workspace, switch to another, switch back, and
        // find it as it was. Before workspaces carried the docking they carried
        // only *which* panels were open, so switching back restored the list and
        // lost the layout — which is the half somebody actually arranged.
        use crate::docking::Region;

        let mut state = TesseraApp::headless();
        state.prefs.docking.place("Pages", Region::Left, 0, 0);
        state.prefs.docking.place("Layers", Region::Left, 0, 1);
        let mine = Workspace::capture("Mine", &state);

        Workspace::usual()[0].apply(&mut state);
        assert!(
            state.prefs.docking.stacks(Region::Left).is_empty(),
            "the other workspace kept the left side"
        );

        mine.apply(&mut state);
        let left = state.prefs.docking.stacks(Region::Left);
        assert_eq!(left.len(), 1, "the left side did not come back");
        assert_eq!(
            left[0].panels,
            vec!["Pages".to_string(), "Layers".to_string()]
        );
    }

    #[test]
    fn a_workspace_saved_before_panels_could_be_arranged_still_applies() {
        // It lands them in the shipped layout, which is what it meant when it
        // was saved. Refusing to read it would lose the workspace entirely.
        let older = r#"{"name":"Old","open":["Properties"],"rail_open":true}"#;
        let read: Workspace = serde_json::from_str(older).expect("read");
        assert_eq!(read.docking, crate::docking::Docking::default());

        let mut state = TesseraApp::headless();
        read.apply(&mut state);
        assert!(Dock::Properties.is_open(&state));
    }

    #[test]
    fn applying_a_workspace_closes_what_it_does_not_ask_for() {
        // **The property that makes switching work.** Otherwise going from a
        // crowded arrangement to a spare one leaves the crowd behind, and the
        // spare workspace is only spare the first time it is used.
        let mut state = TesseraApp::headless();
        for dock in Dock::ALL {
            dock.set_open(&mut state, true);
        }

        Workspace::usual()[0].apply(&mut state);
        assert!(Dock::Properties.is_open(&state));
        assert!(!Dock::Pages.is_open(&state), "Pages was left open");
        assert!(!Dock::Preflight.is_open(&state));
    }

    #[test]
    fn an_arrangement_survives_being_captured_and_applied() {
        let mut state = TesseraApp::headless();
        Dock::Swatches.set_open(&mut state, true);
        Dock::Pages.set_open(&mut state, true);
        let saved = Workspace::capture("Mine", &state);

        Workspace::usual()[0].apply(&mut state);
        assert!(!Dock::Swatches.is_open(&state));

        saved.apply(&mut state);
        assert!(Dock::Swatches.is_open(&state), "the arrangement was lost");
        assert!(Dock::Pages.is_open(&state));
    }

    #[test]
    fn a_workspace_naming_a_panel_this_build_does_not_have_is_not_a_failure() {
        // Panels get added and renamed. A workspace saved by an older build
        // must still apply, minus the part this build cannot honour.
        let mut state = TesseraApp::headless();
        let odd = Workspace {
            name: "From the future".to_string(),
            open: vec!["Properties".to_string(), "Sparkles".to_string()],
            rail_open: true,
            docking: crate::docking::Docking::default(),
        };
        odd.apply(&mut state);
        assert!(Dock::Properties.is_open(&state));
    }

    #[test]
    fn a_workspace_does_not_carry_the_theme_or_the_document() {
        // Those belong to the person and to the work. A workspace that restored
        // them would mean switching arrangement turned the lights off and threw
        // away your place on the page.
        let mut state = TesseraApp::headless();
        state.prefs.theme = crate::prefs::ThemeChoice::Light;
        let before = state.active;

        Workspace::usual()[3].apply(&mut state);
        assert_eq!(state.prefs.theme, crate::prefs::ThemeChoice::Light);
        assert_eq!(state.active, before);
    }

    #[test]
    fn the_arrangements_are_named_for_jobs() {
        // "Prepress" is a thing somebody is doing. "Swatches + Preflight" is a
        // list they would have to decode.
        let names: Vec<String> = Workspace::usual().into_iter().map(|w| w.name).collect();
        assert!(names.contains(&"Prepress".to_string()));
        assert!(names.contains(&"Typography".to_string()));
        assert!(!names.iter().any(|n| n.contains('+')));
    }

    #[test]
    fn a_workspace_round_trips_through_json() {
        let saved = &Workspace::usual()[1];
        let text = serde_json::to_string(saved).expect("write");
        let back: Workspace = serde_json::from_str(&text).expect("read");
        assert_eq!(&back, saved);
    }
}
