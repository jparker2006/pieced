//! Grunt navigation: a hand-rolled A* over the build grid (docs/M3-SPEC.md →
//! The grunt, item 4; docs/research/bot-ai.md → Tactics on the build grid).
//!
//! A node is a cell, a level and a surface: the ground (level 0), a floor, or a
//! ramp. The successor function reads the live [`PieceMap`] on every search, so
//! nothing is ever rebuilt:
//! - a wall on an edge blocks it (a wall edited into a centred door doesn't);
//! - a ramp links level n (its low end) to level n + 1 (its high end), and
//!   only along its facing, so direction matters;
//! - floors add walkable nodes one or more levels up, and you can drop off
//!   their open edges onto whatever is below;
//! - cones, edited floors and edited ramps are not walked;
//! - the arena props (D29 rocks and stumps) block the ground moves that pass
//!   through them, and the arena bounds end the grid.
//!
//! Everything here is pure: tests build a [`PieceMap`] and call it directly.

use crate::{
    arena::ARENA_PROPS,
    building::{EdgeKey, PieceMap, SlotKey, ramp_surface_height},
    shared::{ARENA_CELLS, Facing, GridCell, LEVEL_HEIGHT, MAX_LEVELS},
};
use bevy::prelude::*;
use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap},
};

/// What a node stands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Surface {
    /// The island's grass (level 0), or a floor laid on it.
    Ground,
    /// A floor one or more levels up.
    Floor,
    /// A ramp rising toward its facing.
    Ramp(Facing),
}

/// One walkable place on the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NavNode {
    pub x: i32,
    pub z: i32,
    pub level: i32,
    pub surface: Surface,
}

impl NavNode {
    pub fn cell(&self) -> GridCell {
        GridCell::new(self.x, self.z, self.level)
    }

    pub fn is_ground(&self) -> bool {
        self.surface == Surface::Ground
    }

    /// The level a character standing here is on: a ramp counts as its top
    /// level from its middle up.
    pub fn standing_level(&self, feet: Vec3) -> i32 {
        match self.surface {
            Surface::Ramp(_) if feet.y >= (self.level as f32 + 0.5) * LEVEL_HEIGHT => {
                self.level + 1
            }
            _ => self.level,
        }
    }

    /// The node's reference point: the cell's centre, at the height of the
    /// walking surface there.
    pub fn center(&self) -> Vec3 {
        let base = self.cell().base_center();
        match self.surface {
            Surface::Ramp(_) => base + Vec3::Y * (LEVEL_HEIGHT / 2.0),
            _ => base,
        }
    }

    /// Height of the walking surface at a world XZ point inside the cell.
    pub fn surface_height(&self, xz: Vec2) -> f32 {
        let base = self.cell().base_center();
        match self.surface {
            Surface::Ramp(facing) => {
                let along = (xz - base.xz()).dot(facing.vector().xz());
                base.y + ramp_surface_height(-along)
            }
            _ => base.y,
        }
    }
}

/// A prop's footprint on the ground, as a circle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PropCircle {
    pub center: Vec2,
    pub radius: f32,
}

/// The arena's props (D29) as circles.
pub fn arena_prop_circles() -> Vec<PropCircle> {
    ARENA_PROPS
        .iter()
        .map(|p| PropCircle {
            center: p.position.xz(),
            radius: p.kind.footprint_radius(),
        })
        .collect()
}

/// A character's body clearance from props when walking between node centres (m).
pub const PROP_MARGIN: f32 = 0.45;

/// Whether the segment `a`→`b` passes within `radius` of `c`.
pub fn segment_near(a: Vec2, b: Vec2, c: Vec2, radius: f32) -> bool {
    let ab = b - a;
    let t = if ab.length_squared() > 1e-8 {
        ((c - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a + ab * t).distance_squared(c) < radius * radius
}

fn in_grid(x: i32, z: i32) -> bool {
    (0..ARENA_CELLS).contains(&x) && (0..ARENA_CELLS).contains(&z)
}

/// Result of one A* search.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// Start to goal, both included. `None` when unreachable or over the cap.
    pub path: Option<Vec<NavNode>>,
    pub expansions: u32,
}

/// A point to walk to, and the node it's on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Waypoint {
    pub pos: Vec3,
    pub node: NavNode,
}

/// Waypoints for a planned path: every node's centre after the start, with the
/// goal node's centre replaced by the exact goal point.
pub fn waypoints(path: &[NavNode], goal: Vec3) -> Vec<Waypoint> {
    let mut out: Vec<Waypoint> = path
        .iter()
        .skip(1)
        .map(|n| Waypoint {
            pos: n.center(),
            node: *n,
        })
        .collect();
    match (out.last_mut(), path.last()) {
        (Some(last), _) => last.pos = goal,
        (None, Some(node)) => out.push(Waypoint {
            pos: goal,
            node: *node,
        }),
        (None, None) => {}
    }
    out
}

/// The live navigation graph: the piece map and the props.
pub struct NavGrid<'a> {
    pub map: &'a PieceMap,
    pub props: &'a [PropCircle],
}

impl<'a> NavGrid<'a> {
    pub fn new(map: &'a PieceMap, props: &'a [PropCircle]) -> Self {
        Self { map, props }
    }

    /// Whether a character can walk across the `facing` edge of `cell` at its
    /// level: no wall, or a wall edited open in its middle column (a centred
    /// door or arch), since grunts cross edges at their middle.
    pub fn edge_open(&self, cell: GridCell, facing: Facing) -> bool {
        match self.map.get(SlotKey::Wall(EdgeKey::of(cell, facing))) {
            None => true,
            Some(e) => e.edit.has(4) && e.edit.has(7),
        }
    }

    /// The ground or floor node at `level` in (x, z), if one can be stood on.
    pub fn surface_node(&self, x: i32, z: i32, level: i32) -> Option<NavNode> {
        if !in_grid(x, z) || !(0..MAX_LEVELS).contains(&level) {
            return None;
        }
        let cell = GridCell::new(x, z, level);
        if self.map.get(SlotKey::Ramp(cell)).is_some() || self.map.get(SlotKey::Cone(cell)).is_some()
        {
            return None;
        }
        let surface = if level == 0 {
            Surface::Ground
        } else {
            match self.map.get(SlotKey::Floor(cell)) {
                Some(e) if !e.edit.is_edited() => Surface::Floor,
                _ => return None,
            }
        };
        Some(NavNode {
            x,
            z,
            level,
            surface,
        })
    }

    /// The ramp node at `level` in (x, z), if its ramp is whole and not capped
    /// by a floor at its top.
    pub fn ramp_node(&self, x: i32, z: i32, level: i32) -> Option<NavNode> {
        if !in_grid(x, z) || !(0..MAX_LEVELS).contains(&level) {
            return None;
        }
        let cell = GridCell::new(x, z, level);
        let entry = self.map.get(SlotKey::Ramp(cell))?;
        if entry.edit.is_edited()
            || self
                .map
                .get(SlotKey::Floor(GridCell::new(x, z, level + 1)))
                .is_some()
        {
            return None;
        }
        Some(NavNode {
            x,
            z,
            level,
            surface: Surface::Ramp(entry.slot.facing),
        })
    }

    /// Whether `node` still exists in the live map.
    pub fn node_exists(&self, node: NavNode) -> bool {
        let live = match node.surface {
            Surface::Ramp(_) => self.ramp_node(node.x, node.z, node.level),
            _ => self.surface_node(node.x, node.z, node.level),
        };
        live == Some(node)
    }

    /// Whether a ground node's centre is clear of every prop.
    pub fn clear_of_props(&self, p: Vec2, margin: f32) -> bool {
        self.props
            .iter()
            .all(|c| p.distance_squared(c.center) >= (c.radius + margin).powi(2))
    }

    /// Whether `a`→`b` keeps clear of every prop. A prop whose margin already
    /// holds `a` is ignored, so a character pressed against a rock (or spawned
    /// beside one) can always walk away from it.
    fn segment_clear_of_props(&self, a: Vec2, b: Vec2) -> bool {
        self.props.iter().all(|c| {
            let r = c.radius + PROP_MARGIN;
            a.distance_squared(c.center) < r * r || !segment_near(a, b, c.center, r)
        })
    }

    /// The node a character with its feet at `pos` stands on (or is jumping
    /// above): the highest surface in its cell at most 0.6 m above the feet.
    pub fn localize(&self, pos: Vec3) -> Option<NavNode> {
        let cell = GridCell::containing(pos);
        let (x, z) = (
            cell.x.clamp(0, ARENA_CELLS - 1),
            cell.z.clamp(0, ARENA_CELLS - 1),
        );
        let mut best: Option<(f32, NavNode)> = None;
        let mut lowest: Option<(f32, NavNode)> = None;
        for level in 0..MAX_LEVELS {
            if level as f32 * LEVEL_HEIGHT > pos.y + LEVEL_HEIGHT {
                break;
            }
            for node in [self.surface_node(x, z, level), self.ramp_node(x, z, level)]
                .into_iter()
                .flatten()
            {
                let h = node.surface_height(pos.xz());
                if h <= pos.y + 0.6 && best.is_none_or(|(bh, _)| h > bh) {
                    best = Some((h, node));
                }
                if lowest.is_none_or(|(lh, _)| h < lh) {
                    lowest = Some((h, node));
                }
            }
        }
        best.or(lowest).map(|(_, n)| n)
    }

    /// Every node reachable in one move from `node`, with its cost. `from`
    /// overrides the node's centre as the start of ground moves (the planner's
    /// start is wherever the grunt stands, perhaps beside a prop).
    pub fn successors(&self, node: NavNode, from: Option<Vec3>, out: &mut Vec<(NavNode, f32)>) {
        out.clear();
        let here = from.unwrap_or_else(|| node.center());
        let cell = node.cell();
        let mut push = |this: &Self, next: NavNode, extra: f32| {
            let to = next.center();
            // Props stand on the grass: only moves at level 0 can touch them.
            if node.level == 0
                && next.level == 0
                && !this.segment_clear_of_props(here.xz(), to.xz())
            {
                return;
            }
            out.push((next, here.distance(to) + extra));
        };
        match node.surface {
            Surface::Ground | Surface::Floor => {
                let l = node.level;
                for f in Facing::ALL {
                    let o = f.offset();
                    let (bx, bz) = (node.x + o.x, node.z + o.y);
                    if !in_grid(bx, bz) || !self.edge_open(cell, f) {
                        continue;
                    }
                    if let Some(next) = self.surface_node(bx, bz, l) {
                        push(self, next, 0.0);
                        continue;
                    }
                    // Onto a ramp's low end, climbing along `f`.
                    if let Some(next) = self.ramp_node(bx, bz, l)
                        && next.surface == Surface::Ramp(f)
                    {
                        push(self, next, 0.0);
                        continue;
                    }
                    // Onto the high end of a ramp that falls away from us.
                    if l >= 1
                        && let Some(next) = self.ramp_node(bx, bz, l - 1)
                        && next.surface == Surface::Ramp(f.opposite())
                    {
                        push(self, next, 0.0);
                        continue;
                    }
                    // Off an open floor edge, down to the highest surface below.
                    if node.surface == Surface::Floor
                        && let Some(next) = self.drop_target(bx, bz, l)
                    {
                        push(self, next, 0.5);
                    }
                }
                // Diagonals, when both orthogonal routes are open.
                for (f1, f2) in [
                    (Facing::North, Facing::East),
                    (Facing::East, Facing::South),
                    (Facing::South, Facing::West),
                    (Facing::West, Facing::North),
                ] {
                    let (o1, o2) = (f1.offset(), f2.offset());
                    let a = GridCell::new(node.x + o1.x, node.z + o1.y, l);
                    let b = GridCell::new(node.x + o2.x, node.z + o2.y, l);
                    let (dx, dz) = (node.x + o1.x + o2.x, node.z + o1.y + o2.y);
                    let open = self.surface_node(a.x, a.z, l).is_some()
                        && self.surface_node(b.x, b.z, l).is_some()
                        && self.edge_open(cell, f1)
                        && self.edge_open(cell, f2)
                        && self.edge_open(a, f2)
                        && self.edge_open(b, f1);
                    if open && let Some(next) = self.surface_node(dx, dz, l) {
                        push(self, next, 0.0);
                    }
                }
            }
            Surface::Ramp(f) => {
                let l = node.level;
                // Down off the low end.
                let lo = f.opposite().offset();
                let (ax, az) = (node.x + lo.x, node.z + lo.y);
                if self.edge_open(cell, f.opposite()) {
                    if let Some(next) = self.surface_node(ax, az, l) {
                        push(self, next, 0.0);
                    } else if l >= 1
                        && let Some(next) = self.ramp_node(ax, az, l - 1)
                        && next.surface == Surface::Ramp(f)
                    {
                        push(self, next, 0.0);
                    }
                }
                // Up off the high end, at the next level.
                let hi = f.offset();
                let (cx, cz) = (node.x + hi.x, node.z + hi.y);
                let top = GridCell::new(node.x, node.z, l + 1);
                if l + 1 < MAX_LEVELS && self.edge_open(top, f) {
                    if let Some(next) = self.surface_node(cx, cz, l + 1) {
                        push(self, next, 0.0);
                    } else if let Some(next) = self.ramp_node(cx, cz, l + 1)
                        && next.surface == Surface::Ramp(f)
                    {
                        push(self, next, 0.0);
                    } else if let Some(next) = self.ramp_node(cx, cz, l)
                        && next.surface == Surface::Ramp(f.opposite())
                    {
                        push(self, next, 0.0);
                    }
                }
            }
        }
    }

    /// Where a character stepping off a floor at `level` into (x, z) lands: the
    /// highest ground or floor below, if the fall is clear of ramps, cones and
    /// other floors.
    fn drop_target(&self, x: i32, z: i32, level: i32) -> Option<NavNode> {
        for k in (0..level).rev() {
            let cell = GridCell::new(x, z, k + 1);
            if self.map.get(SlotKey::Floor(cell)).is_some()
                || self.map.get(SlotKey::Cone(cell)).is_some()
                || self
                    .map
                    .get(SlotKey::Ramp(GridCell::new(x, z, k)))
                    .is_some()
            {
                return None;
            }
            if let Some(node) = self.surface_node(x, z, k) {
                return Some(node);
            }
        }
        None
    }

    /// Whether `b` is one move from `a` in the live map.
    pub fn is_move(&self, a: NavNode, b: NavNode) -> bool {
        let mut out = Vec::new();
        self.successors(a, None, &mut out);
        out.iter().any(|(n, _)| *n == b)
    }

    /// Whether every move along `path` still exists.
    pub fn path_valid(&self, path: &[NavNode]) -> bool {
        path.first().is_none_or(|n| self.node_exists(*n))
            && path.windows(2).all(|w| self.is_move(w[0], w[1]))
    }

    /// A* from `start` (standing at `start_pos`) to `goal`, expanding at most
    /// `max_expansions` nodes.
    pub fn plan(
        &self,
        start: NavNode,
        start_pos: Vec3,
        goal: NavNode,
        max_expansions: u32,
    ) -> Plan {
        if start == goal {
            return Plan {
                path: Some(vec![start]),
                expansions: 0,
            };
        }
        let goal_center = goal.center();
        let h = |n: &NavNode| n.center().distance(goal_center);
        // Costs in millimetres keep the heap's ordering total and deterministic.
        let mm = |c: f32| (c * 1000.0).round() as u64;
        let mut open: BinaryHeap<Reverse<(u64, u64, NavNode)>> = BinaryHeap::new();
        let mut best: HashMap<NavNode, (f32, Option<NavNode>)> = HashMap::new();
        let mut counter = 0u64;
        best.insert(start, (0.0, None));
        open.push(Reverse((mm(h(&start)), counter, start)));
        let mut expansions = 0u32;
        let mut next = Vec::with_capacity(12);
        while let Some(Reverse((_, _, node))) = open.pop() {
            if node == goal {
                let mut path = vec![goal];
                let mut cur = goal;
                while let Some((_, Some(parent))) = best.get(&cur) {
                    path.push(*parent);
                    cur = *parent;
                }
                path.reverse();
                return Plan {
                    path: Some(path),
                    expansions,
                };
            }
            if expansions >= max_expansions {
                break;
            }
            expansions += 1;
            let g = best.get(&node).map_or(0.0, |b| b.0);
            let from = (node == start).then_some(start_pos);
            self.successors(node, from, &mut next);
            for &(succ, cost) in &next {
                let ng = g + cost;
                if best.get(&succ).is_none_or(|b| ng + 1e-4 < b.0) {
                    best.insert(succ, (ng, Some(node)));
                    counter += 1;
                    open.push(Reverse((mm(ng + h(&succ)), counter, succ)));
                }
            }
        }
        Plan {
            path: None,
            expansions,
        }
    }

    /// Whether a ground cell at (x, z) can be stood in.
    fn ground_ok(&self, x: i32, z: i32) -> bool {
        self.surface_node(x, z, 0).is_some()
    }

    /// Whether a body `half_width` wide can walk straight from `a` to `b` on
    /// the ground: every cell on the way is open ground, no wall edge is
    /// crossed and no prop is touched.
    pub fn ground_segment_clear(&self, a: Vec3, b: Vec3, half_width: f32) -> bool {
        let (a, b) = (a.xz(), b.xz());
        let d = b - a;
        let len = d.length();
        let side = if len > 1e-4 {
            Vec2::new(-d.y, d.x) / len
        } else {
            Vec2::ZERO
        };
        let steps = (len / 0.5).ceil().max(1.0) as i32;
        let offsets: &[f32] = if half_width > 0.0 {
            &[-1.0, 0.0, 1.0]
        } else {
            &[0.0]
        };
        for &o in offsets {
            let shift = side * o * half_width;
            let mut prev: Option<GridCell> = None;
            for i in 0..=steps {
                let p = a + d * (i as f32 / steps as f32) + shift;
                let cell = GridCell::containing(Vec3::new(p.x, 0.0, p.y));
                if !self.ground_ok(cell.x, cell.z) || !self.clear_of_props(p, 0.1) {
                    return false;
                }
                if let Some(pc) = prev
                    && pc != cell
                    && !self.cells_connected(pc, cell)
                {
                    return false;
                }
                prev = Some(cell);
            }
        }
        true
    }

    /// Two ground cells that touch (sides or corners) with no wall between.
    fn cells_connected(&self, a: GridCell, b: GridCell) -> bool {
        let (dx, dz) = (b.x - a.x, b.z - a.z);
        let fx = match dx {
            1 => Some(Facing::East),
            -1 => Some(Facing::West),
            _ => None,
        };
        let fz = match dz {
            1 => Some(Facing::South),
            -1 => Some(Facing::North),
            _ => None,
        };
        match (fx, fz) {
            (Some(f), None) | (None, Some(f)) => self.edge_open(a, f),
            (Some(fx), Some(fz)) => {
                let via_x = GridCell::new(a.x + dx, a.z, 0);
                let via_z = GridCell::new(a.x, a.z + dz, 0);
                self.ground_ok(via_x.x, via_x.z)
                    && self.ground_ok(via_z.x, via_z.z)
                    && self.edge_open(a, fx)
                    && self.edge_open(a, fz)
                    && self.edge_open(via_x, fz)
                    && self.edge_open(via_z, fx)
            }
            (None, None) => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::building::PieceSlot;

    fn map_with(slots: &[PieceSlot]) -> PieceMap {
        let mut map = PieceMap::default();
        for s in slots {
            map.insert(*s, Entity::PLACEHOLDER);
        }
        map
    }

    fn ground(x: i32, z: i32) -> NavNode {
        NavNode {
            x,
            z,
            level: 0,
            surface: Surface::Ground,
        }
    }

    #[test]
    fn open_ground_plans_a_straight_diagonal_line() {
        let map = PieceMap::default();
        let nav = NavGrid::new(&map, &[]);
        let plan = nav.plan(ground(1, 1), ground(1, 1).center(), ground(5, 5), 2000);
        let path = plan.path.expect("reachable");
        assert_eq!(path.len(), 5, "four diagonal steps: {path:?}");
        assert_eq!(path.last(), Some(&ground(5, 5)));
    }

    #[test]
    fn walls_block_edges_and_force_a_detour() {
        // A wall run across column 3, rows 0..=10, on the east side.
        let walls: Vec<PieceSlot> = (0..=10)
            .map(|z| PieceSlot::wall(GridCell::new(3, z, 0), Facing::East))
            .collect();
        let map = map_with(&walls);
        let nav = NavGrid::new(&map, &[]);
        assert!(!nav.is_move(ground(3, 5), ground(4, 5)));
        let path = nav
            .plan(ground(3, 5), ground(3, 5).center(), ground(4, 5), 2000)
            .path
            .expect("around the end of the run");
        assert!(path.iter().any(|n| n.z == 11), "goes round the end: {path:?}");
        // Fully sealed: unreachable, and the search stops.
        let mut sealed = walls.clone();
        sealed.push(PieceSlot::wall(GridCell::new(3, 11, 0), Facing::East));
        let map = map_with(&sealed);
        let nav = NavGrid::new(&map, &[]);
        let plan = nav.plan(ground(3, 5), ground(3, 5).center(), ground(4, 5), 2000);
        assert!(plan.path.is_none());
    }

    #[test]
    fn a_ramp_links_its_low_end_to_the_floor_at_its_top_only_along_its_facing() {
        // Ramp in (5, 6) rising north to a floor at (5, 5) level 1.
        let map = map_with(&[
            PieceSlot::ramp(GridCell::new(5, 6, 0), Facing::North),
            PieceSlot::floor(GridCell::new(5, 5, 1)),
        ]);
        let nav = NavGrid::new(&map, &[]);
        let ramp = nav.ramp_node(5, 6, 0).unwrap();
        let floor = nav.surface_node(5, 5, 1).unwrap();
        assert!(nav.is_move(ground(5, 7), ramp), "low end from the south");
        assert!(!nav.is_move(ground(4, 6), ramp), "not from the side");
        assert!(!nav.is_move(ground(5, 5), ramp), "not up the high face");
        assert!(nav.is_move(ramp, floor));
        assert!(nav.is_move(floor, ramp), "and back down");
        let path = nav
            .plan(ground(5, 10), ground(5, 10).center(), floor, 2000)
            .path
            .expect("up the ramp");
        assert!(path.contains(&ramp));
        assert_eq!(path.last(), Some(&floor));
        // Walling the ramp's foot blocks the climb.
        let map = map_with(&[
            PieceSlot::ramp(GridCell::new(5, 6, 0), Facing::North),
            PieceSlot::floor(GridCell::new(5, 5, 1)),
            PieceSlot::wall(GridCell::new(5, 6, 0), Facing::South),
        ]);
        let nav = NavGrid::new(&map, &[]);
        assert!(
            nav.plan(ground(5, 10), ground(5, 10).center(), floor, 2000)
                .path
                .is_none()
        );
    }

    #[test]
    fn floors_drop_down_and_ramp_chains_climb_levels() {
        let map = map_with(&[
            PieceSlot::ramp(GridCell::new(2, 8, 0), Facing::North),
            PieceSlot::ramp(GridCell::new(2, 7, 1), Facing::North),
            PieceSlot::floor(GridCell::new(2, 6, 2)),
        ]);
        let nav = NavGrid::new(&map, &[]);
        let top = nav.surface_node(2, 6, 2).unwrap();
        let path = nav
            .plan(ground(2, 11), ground(2, 11).center(), top, 2000)
            .path
            .expect("two ramps up");
        assert_eq!(path.iter().filter(|n| !n.is_ground()).count(), 3);
        // Off the floor's west edge, straight down to the grass.
        assert!(nav.is_move(top, ground(1, 6)));
        let down = nav
            .plan(top, top.center(), ground(8, 6), 2000)
            .path
            .expect("drops off");
        assert!(down.len() >= 2);
    }

    #[test]
    fn props_block_ground_moves_through_them() {
        let map = PieceMap::default();
        let rock = PropCircle {
            center: ground(4, 4).center().xz(),
            radius: 1.0,
        };
        let props = [rock];
        let nav = NavGrid::new(&map, &props);
        assert!(!nav.is_move(ground(3, 4), ground(4, 4)));
        let path = nav
            .plan(ground(3, 4), ground(3, 4).center(), ground(5, 4), 2000)
            .path
            .expect("around the rock");
        assert!(!path.contains(&ground(4, 4)));
        assert!(!nav.ground_segment_clear(ground(3, 4).center(), ground(5, 4).center(), 0.4));
        assert!(nav.ground_segment_clear(ground(3, 2).center(), ground(5, 2).center(), 0.4));
        // Standing against the rock, a grunt can still plan its way off it.
        let beside = rock.center + Vec2::new(0.8, 0.3);
        let start = Vec3::new(beside.x, 0.0, beside.y);
        let from = nav.localize(start).unwrap();
        assert!(
            nav.plan(from, start, ground(8, 8), 2000).path.is_some(),
            "walks away from a prop it's pressed against"
        );
    }

    #[test]
    fn the_expansion_cap_stops_long_searches() {
        let map = PieceMap::default();
        let nav = NavGrid::new(&map, &[]);
        let plan = nav.plan(ground(0, 0), ground(0, 0).center(), ground(11, 11), 3);
        assert!(plan.path.is_none());
        assert_eq!(plan.expansions, 3);
    }

    #[test]
    fn localize_finds_ramps_floors_and_the_ground() {
        let map = map_with(&[
            PieceSlot::ramp(GridCell::new(5, 6, 0), Facing::North),
            PieceSlot::floor(GridCell::new(5, 5, 1)),
        ]);
        let nav = NavGrid::new(&map, &[]);
        let floor = nav.surface_node(5, 5, 1).unwrap();
        let ramp = nav.ramp_node(5, 6, 0).unwrap();
        assert_eq!(nav.localize(floor.center()), Some(floor));
        assert_eq!(nav.localize(floor.center() + Vec3::Y * 1.0), Some(floor));
        assert_eq!(nav.localize(ramp.center()), Some(ramp));
        // Under the floor is the grass.
        assert_eq!(nav.localize(ground(5, 5).center()), Some(ground(5, 5)));
        assert_eq!(ramp.standing_level(ramp.center() + Vec3::Y * 0.1), 1);
    }

    #[test]
    fn waypoints_end_at_the_goal_point() {
        let path = [ground(1, 1), ground(2, 1), ground(3, 1)];
        let goal = Vec3::new(1.0, 0.0, 2.0);
        let w = waypoints(&path, goal);
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].pos, ground(2, 1).center());
        assert_eq!(w[1].pos, goal);
        assert_eq!(waypoints(&path[..1], goal)[0].pos, goal);
    }
}
