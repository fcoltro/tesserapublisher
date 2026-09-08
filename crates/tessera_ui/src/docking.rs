//! Where each panel lives: which side, which stack, and in what order.
//!
//! Until now the rail was one column on the right holding every open panel in a
//! fixed order. This is the model underneath dragging a panel to the other side,
//! grouping panels into a tabbed stack, and splitting a side into two.
//!
//! ## Panels are named, not numbered
//!
//! A placement holds a panel's **title**, the same string [`Dock::title`] gives.
//! Numbering them would mean a saved layout from an older build silently
//! reassigns itself when a panel is added in the middle of the list — the layout
//! would still load, and every panel would be in the wrong place. A name that
//! this build does not have is dropped, and a panel this build has that the
//! layout does not mention is appended.
//!
//! ## The invariant that matters
//!
//! **A panel is in exactly one place.** Every operation here goes through
//! [`Docking::place`], which removes the panel from wherever it was before it
//! puts it anywhere new. A model that let a panel be in two stacks would draw it
//! twice, and the second copy would be a panel whose controls edit the same
//! state as the first — which looks like a bug in whatever the panel does rather
//! than a bug in the layout.

use serde::{Deserialize, Serialize};

use crate::view::rail::Dock;

/// Which side of the window a stack is on.
///
/// Two sides, not four. A layout tool's canvas wants to be as tall as the
/// window, so a panel across the bottom costs more than it gives — and the
/// status bar is already there. Floating panels are out of scope by decision,
/// not by omission: see the spec, section 14.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Region {
    Left,
    Right,
}

impl Region {
    pub const ALL: [Region; 2] = [Region::Left, Region::Right];
}

/// One tabbed group of panels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stack {
    /// Panel titles, in tab order.
    pub panels: Vec<String>,
    /// Which tab is showing.
    ///
    /// Clamped by [`Docking::tidy`] rather than trusted: it comes out of a
    /// preferences file, and an index past the end would panic somewhere far
    /// from here.
    pub active: usize,
}

impl Stack {
    fn of(panels: Vec<String>) -> Self {
        Self { panels, active: 0 }
    }

    /// The panel showing, if the stack has one.
    pub fn showing(&self) -> Option<&str> {
        self.panels.get(self.active).map(String::as_str)
    }
}

/// The arrangement of every panel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Docking {
    pub left: Vec<Stack>,
    pub right: Vec<Stack>,
}

impl Default for Docking {
    /// Everything in one stack on the right, in the order the rail listed them.
    ///
    /// One stack rather than one per panel: six panels each in their own
    /// splitter is six splitters, and nobody arranged that on purpose.
    fn default() -> Self {
        Self {
            left: Vec::new(),
            right: vec![Stack::of(
                Dock::ALL.iter().map(|d| d.title().to_string()).collect(),
            )],
        }
    }
}

impl Docking {
    fn side(&self, region: Region) -> &Vec<Stack> {
        match region {
            Region::Left => &self.left,
            Region::Right => &self.right,
        }
    }

    fn side_mut(&mut self, region: Region) -> &mut Vec<Stack> {
        match region {
            Region::Left => &mut self.left,
            Region::Right => &mut self.right,
        }
    }

    /// The stacks on a side, in order.
    pub fn stacks(&self, region: Region) -> &[Stack] {
        self.side(region)
    }

    /// Where a panel is: its side, which stack, and its place in that stack.
    pub fn find(&self, panel: &str) -> Option<(Region, usize, usize)> {
        for region in Region::ALL {
            for (at, stack) in self.side(region).iter().enumerate() {
                if let Some(slot) = stack.panels.iter().position(|p| p == panel) {
                    return Some((region, at, slot));
                }
            }
        }
        None
    }

    /// Take a panel out of wherever it is.
    ///
    /// Private, and the only remover, because a panel taken out and not put back
    /// is a panel that has vanished from the interface with no way to reach it.
    /// [`Docking::place`] is the one caller.
    fn lift(&mut self, panel: &str) {
        for region in Region::ALL {
            for stack in self.side_mut(region).iter_mut() {
                stack.panels.retain(|p| p != panel);
            }
        }
    }

    /// Put a panel into a stack, at a position.
    ///
    /// `stack` past the end of the side makes a **new stack** there, which is
    /// what dropping a panel onto the edge of a region means. `at` past the end
    /// of the stack appends.
    ///
    /// The panel is lifted from wherever it was first, so this can never
    /// duplicate one — see the module note.
    pub fn place(&mut self, panel: &str, region: Region, stack: usize, at: usize) {
        // Read where it was *before* lifting, so a move inside one stack can
        // tell "after myself" from "before myself" once I am gone.
        let was = self.find(panel);
        self.lift(panel);

        // Lifting can empty the stack the panel came from, which shifts every
        // stack after it. Tidy first so `stack` still means what the caller
        // meant, then clamp.
        let shifted = match was {
            Some((from, at_stack, _))
                if from == region
                    && at_stack < stack
                    && self
                        .side(from)
                        .get(at_stack)
                        .is_some_and(|s| s.panels.is_empty()) =>
            {
                stack - 1
            }
            _ => stack,
        };
        self.tidy();

        let side = self.side_mut(region);
        if shifted >= side.len() {
            side.push(Stack::of(vec![panel.to_string()]));
        } else {
            let target = &mut side[shifted];
            let at = at.min(target.panels.len());
            target.panels.insert(at, panel.to_string());
            // Follow the panel that was just moved: somebody who drags a tab
            // wants to see it, and leaving the old tab showing makes the drag
            // look like it did nothing.
            target.active = at;
        }
        self.tidy();
    }

    /// Drop empty stacks and clamp every active index onto a real panel.
    ///
    /// Called after every change rather than trusted to happen: an empty stack
    /// is a splitter with nothing in it, and an active index past the end is a
    /// panel area that draws nothing while its tabs say otherwise.
    pub fn tidy(&mut self) {
        for region in Region::ALL {
            let side = self.side_mut(region);
            side.retain(|stack| !stack.panels.is_empty());
            for stack in side.iter_mut() {
                if stack.active >= stack.panels.len() {
                    stack.active = stack.panels.len().saturating_sub(1);
                }
            }
        }
    }

    /// Reconcile with the panels this build actually has.
    ///
    /// A name this build does not know is dropped; a panel it has that the
    /// layout does not mention is appended to the last stack on the right. Both
    /// halves matter: without the first, a removed panel leaves a tab that
    /// cannot be drawn; without the second, a panel added in a later version is
    /// unreachable for anybody with a saved layout.
    pub fn reconcile(&mut self) {
        let known: Vec<&'static str> = Dock::ALL.iter().map(|d| d.title()).collect();

        for region in Region::ALL {
            for stack in self.side_mut(region).iter_mut() {
                stack.panels.retain(|p| known.contains(&p.as_str()));
            }
        }
        self.tidy();

        for title in known {
            if self.find(title).is_none() {
                let side = self.side_mut(Region::Right);
                match side.last_mut() {
                    Some(stack) => stack.panels.push(title.to_string()),
                    None => side.push(Stack::of(vec![title.to_string()])),
                }
            }
        }
    }

    /// Show a panel: bring its tab to the front of whatever stack holds it.
    pub fn reveal(&mut self, panel: &str) {
        if let Some((region, stack, slot)) = self.find(panel)
            && let Some(stack) = self.side_mut(region).get_mut(stack)
        {
            stack.active = slot;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles() -> Vec<&'static str> {
        Dock::ALL.iter().map(|d| d.title()).collect()
    }

    #[test]
    fn every_panel_starts_somewhere() {
        // A panel with no placement is a panel that cannot be drawn, and the
        // only way to notice is that it is missing.
        let docking = Docking::default();
        for title in titles() {
            assert!(docking.find(title).is_some(), "{title} has no place");
        }
    }

    #[test]
    fn a_panel_is_never_in_two_places() {
        // **The invariant.** Drawn twice, the second copy edits the same state
        // as the first, which looks like a bug in the panel rather than in the
        // layout.
        let mut docking = Docking::default();
        docking.place("Pages", Region::Left, 0, 0);
        docking.place("Pages", Region::Right, 0, 0);
        docking.place("Pages", Region::Left, 9, 0);

        let mut seen = 0;
        for region in Region::ALL {
            for stack in docking.stacks(region) {
                seen += stack.panels.iter().filter(|p| *p == "Pages").count();
            }
        }
        assert_eq!(seen, 1, "Pages is in {seen} places");
    }

    #[test]
    fn dropping_past_the_last_stack_makes_a_new_one() {
        // Which is what dropping a panel on the edge of a region means: a
        // splitter beside what is already there, not a tab in it.
        let mut docking = Docking::default();
        let before = docking.stacks(Region::Right).len();
        docking.place("Pages", Region::Right, 99, 0);
        assert_eq!(docking.stacks(Region::Right).len(), before + 1);
    }

    #[test]
    fn emptying_a_stack_removes_it() {
        // An empty stack is a splitter with nothing in it, taking room from the
        // canvas and offering nothing back.
        let mut docking = Docking {
            left: vec![Stack::of(vec!["Pages".to_string()])],
            right: vec![Stack::of(vec!["Properties".to_string()])],
        };
        docking.place("Pages", Region::Right, 0, 0);
        assert!(
            docking.stacks(Region::Left).is_empty(),
            "an empty stack stayed"
        );
    }

    #[test]
    fn the_active_tab_is_always_a_real_panel() {
        // It comes out of a preferences file. An index past the end would panic
        // somewhere a long way from here.
        let mut docking = Docking {
            left: Vec::new(),
            right: vec![Stack {
                panels: vec!["Properties".to_string(), "Pages".to_string()],
                active: 47,
            }],
        };
        docking.tidy();
        assert_eq!(docking.stacks(Region::Right)[0].active, 1);
        assert!(docking.stacks(Region::Right)[0].showing().is_some());
    }

    #[test]
    fn a_moved_panel_is_the_one_showing() {
        // Somebody who drags a tab wants to see it. Leaving the old tab in front
        // makes the drag look like it did nothing at all.
        let mut docking = Docking::default();
        docking.place("Preflight", Region::Left, 0, 0);
        assert_eq!(docking.stacks(Region::Left)[0].showing(), Some("Preflight"));
    }

    #[test]
    fn reordering_inside_one_stack_does_not_duplicate() {
        // The case the `lift`-then-insert order exists for.
        let mut docking = Docking::default();
        let count = docking.stacks(Region::Right)[0].panels.len();
        docking.place("Properties", Region::Right, 0, 3);

        let stack = &docking.stacks(Region::Right)[0];
        assert_eq!(stack.panels.len(), count, "a panel was gained or lost");
        assert_eq!(
            stack.panels.iter().filter(|p| *p == "Properties").count(),
            1
        );
    }

    #[test]
    fn a_layout_from_another_build_still_works() {
        // A name this build does not have is dropped; a panel it has that the
        // layout does not mention is appended. Without the first, a tab cannot
        // be drawn; without the second, a new panel is unreachable for anybody
        // who has ever saved a layout.
        let mut docking = Docking {
            left: Vec::new(),
            right: vec![Stack::of(vec![
                "Properties".to_string(),
                "Sparkles".to_string(),
            ])],
        };
        docking.reconcile();

        assert!(
            docking.find("Sparkles").is_none(),
            "an unknown panel stayed"
        );
        for title in titles() {
            assert!(docking.find(title).is_some(), "{title} went missing");
        }
    }

    #[test]
    fn reconciling_an_empty_layout_gives_every_panel_a_home() {
        let mut docking = Docking {
            left: Vec::new(),
            right: Vec::new(),
        };
        docking.reconcile();
        for title in titles() {
            assert!(docking.find(title).is_some(), "{title} has no place");
        }
    }

    #[test]
    fn revealing_a_panel_brings_its_tab_forward() {
        let mut docking = Docking::default();
        docking.reveal("Preflight");
        let (region, stack, slot) = docking.find("Preflight").expect("placed");
        assert_eq!(docking.stacks(region)[stack].active, slot);
    }

    #[test]
    fn a_layout_round_trips_through_json() {
        let mut docking = Docking::default();
        docking.place("Pages", Region::Left, 0, 0);
        docking.place("Layers", Region::Left, 0, 1);

        let text = serde_json::to_string(&docking).expect("write");
        let back: Docking = serde_json::from_str(&text).expect("read");
        assert_eq!(back, docking);
    }
}
