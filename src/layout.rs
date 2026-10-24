//! Split-pane tree for a single tab.

use eframe::egui::{Rect, pos2};

pub type PaneId = u64;

/// Direction in which a split divides its area.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    /// Children side by side (vertical divider), like iTerm2 "Split Vertically".
    Horizontal,
    /// Children stacked (horizontal divider), like iTerm2 "Split Horizontally".
    Vertical,
}

#[derive(Debug)]
pub enum Node {
    Leaf(PaneId),
    Split { axis: Axis, ratio: f32, first: Box<Node>, second: Box<Node> },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl Node {
    /// Replaces the leaf `target` with a split holding `target` and `new`.
    pub fn split(&mut self, target: PaneId, new: PaneId, axis: Axis) -> bool {
        if matches!(self, Node::Leaf(id) if *id == target) {
            *self = Node::Split {
                axis,
                ratio: 0.5,
                first: Box::new(Node::Leaf(target)),
                second: Box::new(Node::Leaf(new)),
            };
            return true;
        }
        match self {
            Node::Leaf(_) => false,
            Node::Split { first, second, .. } => first.split(target, new, axis) || second.split(target, new, axis),
        }
    }

    /// Returns the tree without `target`, collapsing the split that contained it.
    /// `None` means the tree is now empty.
    pub fn remove(self, target: PaneId) -> Option<Node> {
        match self {
            Node::Leaf(id) if id == target => None,
            Node::Leaf(id) => Some(Node::Leaf(id)),
            Node::Split { axis, ratio, first, second } => match (first.remove(target), second.remove(target)) {
                (Some(a), Some(b)) => Some(Node::Split { axis, ratio, first: Box::new(a), second: Box::new(b) }),
                (Some(n), None) | (None, Some(n)) => Some(n),
                (None, None) => None,
            },
        }
    }

    pub fn panes(&self, out: &mut Vec<PaneId>) {
        match self {
            Node::Leaf(id) => out.push(*id),
            Node::Split { first, second, .. } => {
                first.panes(out);
                second.panes(out);
            }
        }
    }

    pub fn first_pane(&self) -> PaneId {
        match self {
            Node::Leaf(id) => *id,
            Node::Split { first, .. } => first.first_pane(),
        }
    }
}

/// Splits `rect` according to `axis`/`ratio`, leaving a `gap` for the divider.
/// Returns (first, divider, second).
pub fn split_rect(rect: Rect, axis: Axis, ratio: f32, gap: f32) -> (Rect, Rect, Rect) {
    match axis {
        Axis::Horizontal => {
            let x = rect.min.x + (rect.width() - gap) * ratio;
            (
                Rect::from_min_max(rect.min, pos2(x, rect.max.y)),
                Rect::from_min_max(pos2(x, rect.min.y), pos2(x + gap, rect.max.y)),
                Rect::from_min_max(pos2(x + gap, rect.min.y), rect.max),
            )
        }
        Axis::Vertical => {
            let y = rect.min.y + (rect.height() - gap) * ratio;
            (
                Rect::from_min_max(rect.min, pos2(rect.max.x, y)),
                Rect::from_min_max(pos2(rect.min.x, y), pos2(rect.max.x, y + gap)),
                Rect::from_min_max(pos2(rect.min.x, y + gap), rect.max),
            )
        }
    }
}

/// Finds the pane adjacent to `from` in `dir`, using the rects from the last layout.
pub fn neighbor(rects: &[(PaneId, Rect)], from: PaneId, dir: Direction) -> Option<PaneId> {
    let (_, cur) = rects.iter().find(|(id, _)| *id == from)?;
    let center = cur.center();
    rects
        .iter()
        .filter(|(id, _)| *id != from)
        .filter(|(_, r)| match dir {
            Direction::Left => r.max.x <= cur.min.x + 1.0 && overlaps(r.y_range(), cur.y_range()),
            Direction::Right => r.min.x >= cur.max.x - 1.0 && overlaps(r.y_range(), cur.y_range()),
            Direction::Up => r.max.y <= cur.min.y + 1.0 && overlaps(r.x_range(), cur.x_range()),
            Direction::Down => r.min.y >= cur.max.y - 1.0 && overlaps(r.x_range(), cur.x_range()),
        })
        .min_by(|(_, a), (_, b)| {
            let da = a.distance_sq_to_pos(center);
            let db = b.distance_sq_to_pos(center);
            da.total_cmp(&db)
        })
        .map(|(id, _)| *id)
}

fn overlaps(a: eframe::egui::Rangef, b: eframe::egui::Rangef) -> bool {
    a.min < b.max && b.min < a.max
}
