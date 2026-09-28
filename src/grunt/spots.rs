//! Standing spots (docs/M3-SPEC.md → The grunt, item 3): each grunt picks a
//! spot 10–18 m from the player, scored by range, a spread penalty near other
//! grunts' spots and line of sight. When the player stands up on the build
//! (a floor or ramp one or more levels up), spots on the player's level score
//! a height bonus, so grunts climb the player's ramps and floors after them.
//!
//! Scoring is pure; the brain adds line of sight for the best few candidates
//! (the expensive rays run last, best score first: the Crytek pattern).

use bevy::prelude::*;

/// Other grunts' spots closer than this are all but forbidden (m). Two grunts
/// strafing 1.2 m either side of spots this far apart stay over 3 m apart.
pub const SPREAD_HARD: f32 = 6.2;

/// Spot scoring knobs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpotParams {
    pub range_min: f32,
    pub range_max: f32,
    /// Other grunts' spots closer than this cost a growing penalty (m).
    pub spread_radius: f32,
    /// Closer than this is all but forbidden (m). Two grunts strafing
    /// `STRAFE_RADIUS` around spots this far apart never come within 3 m.
    pub spread_hard: f32,
    /// Score lost per metre of travel from the grunt to the spot.
    pub travel_cost: f32,
    /// Score kept by the current spot (so a grunt doesn't hop between equals).
    pub keep_bonus: f32,
    /// Score for seeing the player from the spot.
    pub los_bonus: f32,
    /// Score for standing on the player's level when the player is up on the build.
    pub height_bonus: f32,
}

impl SpotParams {
    pub fn new(range_min: f32, range_max: f32) -> Self {
        Self {
            range_min,
            range_max,
            spread_radius: 9.0,
            spread_hard: SPREAD_HARD,
            travel_cost: 0.03,
            keep_bonus: 0.3,
            los_bonus: 1.5,
            height_bonus: 0.8,
        }
    }
}

/// A candidate spot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpotCandidate {
    pub pos: Vec3,
    /// On the player's level, up on the build.
    pub elevated: bool,
}

/// Score without line of sight (higher is better).
pub fn score_spot(
    c: SpotCandidate,
    player: Vec3,
    others: &[Vec3],
    from: Vec3,
    current: Option<Vec3>,
    p: &SpotParams,
) -> f32 {
    let d = c.pos.xz().distance(player.xz());
    let mut score = if c.elevated {
        // Up on the build the fight is close: anywhere off the player's tile.
        let min = 2.5;
        if d < min {
            -3.0
        } else if d > p.range_max {
            1.0 - (d - p.range_max) * 0.3
        } else {
            1.0 + p.height_bonus - (d - 6.0).abs() / p.range_max * 0.3
        }
    } else {
        let mid = (p.range_min + p.range_max) / 2.0;
        let half = ((p.range_max - p.range_min) / 2.0).max(0.1);
        if d < p.range_min {
            1.0 - (p.range_min - d) * 0.3
        } else if d > p.range_max {
            1.0 - (d - p.range_max) * 0.3
        } else {
            1.0 - (d - mid).abs() / half * 0.3
        }
    };
    for o in others {
        let gap = o.xz().distance(c.pos.xz());
        if gap < p.spread_hard {
            score -= 5.0;
        }
        if gap < p.spread_radius {
            score -= (p.spread_radius - gap) / p.spread_radius * 2.0;
        }
    }
    score -= from.xz().distance(c.pos.xz()) * p.travel_cost;
    if current.is_some_and(|s| s.distance(c.pos) < 1.0) {
        score += p.keep_bonus;
    }
    score
}

/// Ground candidates on rings around the player: `angles` directions starting
/// at `rotation`, at three radii across the range band.
pub fn ring_candidates(player: Vec3, rotation: f32, angles: usize, p: &SpotParams) -> Vec<Vec3> {
    let radii = [
        p.range_min + 1.0,
        (p.range_min + p.range_max) / 2.0,
        p.range_max - 1.0,
    ];
    let mut out = Vec::with_capacity(angles * radii.len());
    for i in 0..angles {
        let a = rotation + std::f32::consts::TAU * i as f32 / angles as f32;
        let dir = Vec3::new(a.cos(), 0.0, a.sin());
        for r in radii {
            out.push(Vec3::new(player.x, 0.0, player.z) + dir * r);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ground(pos: Vec3) -> SpotCandidate {
        SpotCandidate {
            pos,
            elevated: false,
        }
    }

    #[test]
    fn the_range_band_scores_best() {
        let p = SpotParams::new(10.0, 18.0);
        let s = |d: f32| score_spot(ground(Vec3::X * d), Vec3::ZERO, &[], Vec3::X * d, None, &p);
        assert!(s(14.0) > s(10.5));
        assert!(s(10.5) > s(6.0));
        assert!(s(17.0) > s(24.0));
    }

    #[test]
    fn spots_near_other_grunts_lose() {
        let p = SpotParams::new(10.0, 18.0);
        let c = ground(Vec3::new(14.0, 0.0, 0.0));
        let alone = score_spot(c, Vec3::ZERO, &[], c.pos, None, &p);
        let crowded = score_spot(c, Vec3::ZERO, &[Vec3::new(14.0, 0.0, 2.0)], c.pos, None, &p);
        let near = score_spot(c, Vec3::ZERO, &[Vec3::new(14.0, 0.0, 7.0)], c.pos, None, &p);
        assert!(crowded < alone - 5.0, "inside the hard spread");
        assert!(near < alone && near > crowded);
    }

    #[test]
    fn up_on_the_build_the_players_level_wins() {
        let p = SpotParams::new(10.0, 18.0);
        let player = Vec3::new(0.0, 3.0, 0.0);
        let up = SpotCandidate {
            pos: Vec3::new(4.0, 3.0, 0.0),
            elevated: true,
        };
        let down = ground(Vec3::new(14.0, 0.0, 0.0));
        let from = Vec3::new(10.0, 0.0, 0.0);
        assert!(
            score_spot(up, player, &[], from, None, &p)
                > score_spot(down, player, &[], from, None, &p)
        );
        let on_top = SpotCandidate {
            pos: Vec3::new(1.0, 3.0, 0.0),
            elevated: true,
        };
        assert!(score_spot(on_top, player, &[], from, None, &p) < 0.0);
    }

    #[test]
    fn rings_cover_the_band() {
        let p = SpotParams::new(10.0, 18.0);
        let c = ring_candidates(Vec3::new(1.0, 2.0, 1.0), 0.3, 16, &p);
        assert_eq!(c.len(), 48);
        for pos in c {
            let d = pos.xz().distance(Vec2::ONE);
            assert!((10.0..=18.0).contains(&d));
            assert_eq!(pos.y, 0.0);
        }
    }
}
