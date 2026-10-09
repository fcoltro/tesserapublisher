//! Flex layout: a set of objects arranged in a row or a column, as CSS's
//! flexbox and Figma's auto layout arrange theirs.
//!
//! A one-off arrangement of the selection, not a container that keeps
//! re-flowing its children: InDesign has no such container, and an object
//! that moves on its own when a neighbour changes is a surprise in a page
//! layout. Run it again after a change, as Align and Distribute are run.
//!
//! The objects keep their order along the way they are laid: whatever is
//! leftmost (or topmost, or the reverse) now is first. They are laid inside
//! the box the selection already fills, from its start edge, a gap apart;
//! `justify` places the run along that box and `align` places each object
//! across it. With `wrap`, a run that would leave the box starts a new
//! line, a gap beyond the deepest object of the last.
//!
//! Pure arithmetic over boxes, so it is tested without a document.

use tessera_document::ids::FrameId;
use tessera_geometry::DocRect;

/// Which way the objects are laid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    #[default]
    Right,
    Left,
    Down,
    Up,
}

/// Where along the way they are laid the run sits in its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Justify {
    #[default]
    Start,
    Centre,
    End,
    /// The first at the start, the last at the end, the gaps between equal.
    SpaceBetween,
}

/// Where across the way they are laid each object sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Start,
    Centre,
    End,
}

/// How to arrange.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Flex {
    pub direction: Direction,
    /// Points between one object and the next.
    pub gap: f64,
    pub justify: Justify,
    pub align: Align,
    pub wrap: bool,
}

impl Default for Flex {
    fn default() -> Self {
        Self {
            direction: Direction::Right,
            gap: 12.0,
            justify: Justify::Start,
            align: Align::Start,
            wrap: false,
        }
    }
}

/// How far each object moves, `(id, dx, dy)`, to be arranged by `flex`.
pub fn arrange(items: &[(FrameId, DocRect)], flex: &Flex) -> Vec<(FrameId, f64, f64)> {
    let Some(area) = enclosing(items) else {
        return Vec::new();
    };
    let across = matches!(flex.direction, Direction::Right | Direction::Left);
    let reversed = matches!(flex.direction, Direction::Left | Direction::Up);
    // Along and across, so one arithmetic serves rows and columns.
    let main = |r: &DocRect| {
        if across {
            (r.x, r.width)
        } else {
            (r.y, r.height)
        }
    };
    let cross = |r: &DocRect| {
        if across {
            (r.y, r.height)
        } else {
            (r.x, r.width)
        }
    };
    let (area_start, area_len) = main(&area);
    let (cross_start, cross_len) = cross(&area);

    // In the order they are met going the way they will be laid.
    let mut order: Vec<&(FrameId, DocRect)> = items.iter().collect();
    order.sort_by(|a, b| main(&a.1).0.total_cmp(&main(&b.1).0));
    if reversed {
        order.reverse();
    }

    // Lines: all of them on one, unless wrapping and the box runs out.
    let mut lines: Vec<Vec<&(FrameId, DocRect)>> = vec![Vec::new()];
    let mut used = 0.0;
    for item in order {
        let len = main(&item.1).1;
        let line = lines.last_mut().expect("one line at least");
        let need = if line.is_empty() {
            len
        } else {
            used + flex.gap + len
        };
        if flex.wrap && !line.is_empty() && need > area_len + 1e-9 {
            lines.push(vec![item]);
            used = len;
        } else {
            line.push(item);
            used = need;
        }
    }

    let mut out = Vec::new();
    let mut line_at = cross_start;
    for line in &lines {
        let total: f64 =
            line.iter().map(|i| main(&i.1).1).sum::<f64>() + flex.gap * (line.len() as f64 - 1.0);
        let depth = line.iter().map(|i| cross(&i.1).1).fold(0.0, f64::max);
        // The run's own gap, which only space-between changes.
        let (lead, gap) = match flex.justify {
            Justify::Start => (0.0, flex.gap),
            Justify::Centre => ((area_len - total) / 2.0, flex.gap),
            Justify::End => (area_len - total, flex.gap),
            Justify::SpaceBetween if line.len() > 1 => {
                let lengths: f64 = line.iter().map(|i| main(&i.1).1).sum();
                (0.0, (area_len - lengths) / (line.len() as f64 - 1.0))
            }
            Justify::SpaceBetween => (0.0, flex.gap),
        };
        // Wrapped lines sit within their own depth; a single line within
        // the whole box's.
        let room = if lines.len() > 1 { depth } else { cross_len };
        let mut at = lead;
        for (id, r) in line {
            let (m0, mlen) = main(r);
            let (c0, clen) = cross(r);
            // Laid from the far end when the way runs left or up.
            let to_main = if reversed {
                area_start + area_len - at - mlen
            } else {
                area_start + at
            };
            let to_cross = line_at
                + match flex.align {
                    Align::Start => 0.0,
                    Align::Centre => (room - clen) / 2.0,
                    Align::End => room - clen,
                };
            let (dm, dc) = (to_main - m0, to_cross - c0);
            out.push(if across { (*id, dm, dc) } else { (*id, dc, dm) });
            at += mlen + gap;
        }
        line_at += depth + flex.gap;
    }
    out
}

/// The box around every item.
fn enclosing(items: &[(FrameId, DocRect)]) -> Option<DocRect> {
    let first = items.first()?.1;
    let (mut x0, mut y0) = (first.x, first.y);
    let (mut x1, mut y1) = (first.x + first.width, first.y + first.height);
    for (_, r) in items {
        x0 = x0.min(r.x);
        y0 = y0.min(r.y);
        x1 = x1.max(r.x + r.width);
        y1 = y1.max(r.y + r.height);
    }
    Some(DocRect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxes(rects: &[(f64, f64, f64, f64)]) -> Vec<(FrameId, DocRect)> {
        let mut keys = slotmap::SlotMap::<FrameId, ()>::with_key();
        rects
            .iter()
            .map(|&(x, y, w, h)| {
                (
                    keys.insert(()),
                    DocRect {
                        x,
                        y,
                        width: w,
                        height: h,
                    },
                )
            })
            .collect()
    }

    /// Where each box lands.
    fn landed(items: &[(FrameId, DocRect)], flex: &Flex) -> Vec<(f64, f64)> {
        let moves = arrange(items, flex);
        items
            .iter()
            .map(|(id, r)| {
                let (_, dx, dy) = moves.iter().find(|(i, ..)| i == id).expect("moved");
                (r.x + dx, r.y + dy)
            })
            .collect()
    }

    #[test]
    fn a_row_is_laid_left_to_right_a_gap_apart_in_the_order_they_were() {
        // The second is leftmost, so it goes first.
        let items = boxes(&[
            (100.0, 30.0, 20.0, 10.0),
            (0.0, 0.0, 30.0, 20.0),
            (50.0, 5.0, 10.0, 40.0),
        ]);
        let at = landed(
            &items,
            &Flex {
                gap: 5.0,
                ..Default::default()
            },
        );
        assert_eq!(at, vec![(50.0, 0.0), (0.0, 0.0), (35.0, 0.0)]);
    }

    #[test]
    fn a_column_centred_across() {
        let items = boxes(&[(0.0, 0.0, 40.0, 10.0), (0.0, 50.0, 20.0, 10.0)]);
        let flex = Flex {
            direction: Direction::Down,
            gap: 4.0,
            align: Align::Centre,
            ..Default::default()
        };
        assert_eq!(landed(&items, &flex), vec![(0.0, 0.0), (10.0, 14.0)]);
    }

    #[test]
    fn space_between_puts_the_ends_on_the_box_s_ends() {
        let items = boxes(&[
            (0.0, 0.0, 10.0, 10.0),
            (30.0, 0.0, 10.0, 10.0),
            (90.0, 0.0, 10.0, 10.0),
        ]);
        let flex = Flex {
            justify: Justify::SpaceBetween,
            ..Default::default()
        };
        assert_eq!(
            landed(&items, &flex),
            vec![(0.0, 0.0), (45.0, 0.0), (90.0, 0.0)]
        );
    }

    #[test]
    fn a_row_to_the_left_starts_at_the_right() {
        let items = boxes(&[(0.0, 0.0, 10.0, 10.0), (50.0, 0.0, 10.0, 10.0)]);
        let flex = Flex {
            direction: Direction::Left,
            gap: 2.0,
            ..Default::default()
        };
        // The rightmost is first, against the right edge (60).
        assert_eq!(landed(&items, &flex), vec![(38.0, 0.0), (50.0, 0.0)]);
    }

    #[test]
    fn wrapping_starts_a_new_line_when_the_box_runs_out() {
        // A box 100 wide: three 40-wide objects fit two to a line.
        let items = boxes(&[
            (0.0, 0.0, 40.0, 10.0),
            (60.0, 0.0, 40.0, 20.0),
            (30.0, 30.0, 40.0, 10.0),
        ]);
        let flex = Flex {
            gap: 10.0,
            wrap: true,
            ..Default::default()
        };
        assert_eq!(
            landed(&items, &flex),
            vec![(0.0, 0.0), (0.0, 20.0), (50.0, 0.0)]
        );
    }
}
