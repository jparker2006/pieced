//! Where everything in the far view sits: the composition of T01 seen from the
//! player spawn (2, 0, 14) facing -Z. Pure data, so tests can check it.
//!
//! - The galaxy's core is upper left: azimuth -21°, 28° up.
//! - The station is front right: azimuth 33°, 470 m out, its platform 125 m up,
//!   so its tallest spires reach the top of the view and its waterfalls fall to
//!   the horizon.
//! - The ringed planet sits low between them: azimuth 7°, 15° up, 1050 m out.
//! - Six waterfall islands ring the arena 230–340 m out, with smaller islets
//!   (waterfalls hidden) scattered between and above them.
//! - Five ships fly four loops round the station: a wide orbit above the ring
//!   walkway, a high lap weaving between the spires and the sails, a sweep past
//!   the planet, and a high loop on the right.

use super::galaxy::sky_point;
use bevy::prelude::*;

/// The player spawn's eye (feet (2, 0, 14) + 1.6 m), the reference viewpoint.
pub const SPAWN_EYE: Vec3 = Vec3::new(2.0, 1.6, 14.0);

/// A far model and where its reference attach point goes.
#[derive(Debug, Clone, PartialEq)]
pub struct FarPiece {
    pub model: &'static str,
    /// The attach point placed at `position` (`Platform`, `Top`, `Center`).
    pub anchor: &'static str,
    pub position: Vec3,
    /// Turn about +Y (radians); models face their -Z, so `facing_arena` turns
    /// them toward the arena centre.
    pub yaw: f32,
    pub scale: f32,
}

/// A far island: a model, its bob and whether its waterfall shows.
#[derive(Debug, Clone, PartialEq)]
pub struct IslandSpec {
    pub piece: FarPiece,
    pub bob_amplitude: f32,
    pub bob_period: f32,
    pub bob_phase: f32,
    pub waterfall: bool,
}

/// A ship's loop, in the station's frame (its -Z faces the arena, +X is the
/// arena's left as seen from spawn, +Y up from its platform).
#[derive(Debug, Clone, PartialEq)]
pub struct ShipSpec {
    pub points: Vec<Vec3>,
    pub lap_s: f32,
    /// Starting positions as fractions of a lap: one ship each.
    pub starts: Vec<f32>,
    pub scale: f32,
}

/// The whole far view.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct FarLayout {
    /// World direction of the galaxy's core.
    pub galaxy: Vec3,
    pub station: FarPiece,
    pub planet: FarPiece,
    pub islands: Vec<IslandSpec>,
    pub ships: Vec<ShipSpec>,
    /// Radius (m) of the soft horizon glow band around the arena.
    pub horizon_radius: f32,
}

/// Yaw that turns a model at `position` to face the arena centre.
pub fn facing_arena(position: Vec3) -> f32 {
    let to_centre = Vec3::new(-position.x, 0.0, -position.z);
    // Transform::looking_to's -Z → direction: yaw = atan2(-dx, -dz).
    (-to_centre.x).atan2(-to_centre.z)
}

/// A point `distance` m from the arena centre on `azimuth_deg`, `height` m up.
pub fn around_arena(azimuth_deg: f32, distance: f32, height: f32) -> Vec3 {
    let a = azimuth_deg.to_radians();
    Vec3::new(a.sin() * distance, height, -a.cos() * distance)
}

/// An elliptical loop of `n` points: centre, radii along x and z, and a
/// vertical wobble (amplitude, phase).
pub fn orbit(
    centre: Vec3,
    rx: f32,
    rz: f32,
    wobble: f32,
    wobble_phase: f32,
    n: usize,
) -> Vec<Vec3> {
    (0..n)
        .map(|i| {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            centre
                + Vec3::new(
                    a.cos() * rx,
                    wobble * (a + wobble_phase).sin(),
                    a.sin() * rz,
                )
        })
        .collect()
}

impl Default for FarLayout {
    fn default() -> Self {
        let station_at = {
            let dir = sky_point(33.0, 0.0);
            Vec3::new(
                SPAWN_EYE.x + dir.x * 470.0,
                125.0,
                SPAWN_EYE.z + dir.z * 470.0,
            )
        };
        let planet_at = SPAWN_EYE + sky_point(7.0, 15.0) * 1050.0;
        let piece = |model, anchor, position: Vec3, scale| FarPiece {
            model,
            anchor,
            position,
            yaw: facing_arena(position),
            scale,
        };
        // (model, azimuth, distance, top height, scale, waterfall)
        let islands = [
            ("far_island_a", -52.0, 230.0, 32.0, 1.15, true),
            ("far_island_b", -25.0, 300.0, 48.0, 1.0, true),
            ("far_island_a", -3.0, 340.0, 22.0, 0.9, true),
            ("far_island_c", 16.0, 250.0, 30.0, 1.25, true),
            ("far_island_b", 62.0, 240.0, 40.0, 1.15, true),
            ("far_island_a", 178.0, 270.0, 30.0, 1.2, true),
            ("far_island_c", -78.0, 200.0, 58.0, 0.55, false),
            ("far_island_c", -38.0, 195.0, 80.0, 0.45, false),
            ("far_island_a", 7.0, 265.0, 72.0, 0.4, false),
            ("far_island_c", 46.0, 205.0, 94.0, 0.5, false),
            ("far_island_b", 100.0, 230.0, 42.0, 0.5, false),
            ("far_island_c", 135.0, 260.0, 56.0, 0.6, false),
            ("far_island_c", -115.0, 240.0, 46.0, 0.6, false),
            ("far_island_a", -160.0, 230.0, 64.0, 0.5, false),
            ("far_island_c", 152.0, 300.0, 20.0, 0.7, false),
        ];
        let islands = islands
            .iter()
            .enumerate()
            .map(|(i, &(model, az, dist, height, scale, waterfall))| {
                let k = i as f32;
                IslandSpec {
                    piece: piece(model, "Top", around_arena(az, dist, height), scale),
                    bob_amplitude: 1.2 + (k * 0.618).fract() * 1.6,
                    bob_period: 6.2 + (k * 0.382 + 0.2).fract() * 3.5,
                    // The golden angle keeps neighbours out of step.
                    bob_phase: (k * 2.399_963).rem_euclid(std::f32::consts::TAU),
                    waterfall,
                }
            })
            .collect();
        let ships = vec![
            // A wide orbit just above the ring walkway, passing in front.
            ShipSpec {
                points: orbit(Vec3::new(0.0, 74.0, 5.0), 178.0, 132.0, 10.0, 0.0, 8),
                lap_s: 13.0,
                starts: vec![0.1, 0.6],
                scale: 1.3,
            },
            // High, between the inner and outer sails and behind the flèche.
            ShipSpec {
                points: orbit(Vec3::new(0.0, 196.0, 2.0), 136.0, 56.0, 8.0, 0.9, 8),
                lap_s: 9.0,
                starts: vec![0.35],
                scale: 1.2,
            },
            // A low sweep out past the planet (the arena's left of the station).
            ShipSpec {
                points: orbit(Vec3::new(235.0, 30.0, 45.0), 140.0, 50.0, 12.0, 2.0, 8),
                lap_s: 8.5,
                starts: vec![0.8],
                scale: 1.3,
            },
            // A loop in front of the station's right side, past its waterfalls.
            ShipSpec {
                points: orbit(Vec3::new(-95.0, -5.0, -170.0), 75.0, 40.0, 10.0, 4.0, 8),
                lap_s: 9.0,
                starts: vec![0.2],
                scale: 1.2,
            },
        ];
        Self {
            galaxy: sky_point(-21.0, 28.0),
            station: piece("station", "Platform", station_at, 1.0),
            planet: piece("planet", "Center", planet_at, 1.0),
            islands,
            ships,
            horizon_radius: 1300.0,
        }
    }
}

impl FarLayout {
    /// The station's frame: ship loops are in it.
    pub fn station_frame(&self) -> Transform {
        Transform::from_translation(self.station.position)
            .with_rotation(Quat::from_rotation_y(self.station.yaw))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facing_arena_points_minus_z_at_the_centre() {
        for p in [
            Vec3::new(200.0, 50.0, -300.0),
            Vec3::new(-150.0, 0.0, 90.0),
            Vec3::new(0.0, 10.0, 250.0),
        ] {
            let t = Transform::from_translation(p)
                .with_rotation(Quat::from_rotation_y(facing_arena(p)));
            let to_centre = Vec3::new(-p.x, 0.0, -p.z).normalize();
            assert!(t.forward().dot(to_centre) > 0.9999, "{p}");
        }
    }

    #[test]
    fn the_composition_matches_t01_from_spawn() {
        let layout = FarLayout::default();
        let look = |p: Vec3| {
            let d = (p - SPAWN_EYE).normalize();
            (d.x.atan2(-d.z).to_degrees(), d.y.asin().to_degrees())
        };
        let (az, el) = look(layout.station.position);
        assert!(
            (25.0..40.0).contains(&az) && (10.0..30.0).contains(&el),
            "station {az} {el}"
        );
        let (az, el) = look(layout.planet.position);
        assert!(
            (-5.0..15.0).contains(&az) && (5.0..20.0).contains(&el),
            "planet {az} {el}"
        );
        let g = layout.galaxy;
        assert!(
            g.x < 0.0 && g.z < 0.0 && g.y > 0.3,
            "galaxy upper left: {g}"
        );
        // Everything stays inside the camera's far plane (1500 m).
        for p in [layout.station.position, layout.planet.position] {
            assert!(p.distance(SPAWN_EYE) + 250.0 < crate::render::FAR_PLANE);
        }
        assert!(layout.horizon_radius < crate::render::FAR_PLANE - 150.0);
    }

    #[test]
    fn four_to_six_waterfall_islands_bob_out_of_step() {
        let layout = FarLayout::default();
        let falls: Vec<_> = layout.islands.iter().filter(|i| i.waterfall).collect();
        assert!((4..=6).contains(&falls.len()));
        for island in &layout.islands {
            assert!((1.0..=3.0).contains(&island.bob_amplitude));
            assert!((6.0..=10.0).contains(&island.bob_period));
            let d = Vec2::new(island.piece.position.x, island.piece.position.z).length();
            assert!((150.0..400.0).contains(&d), "mid-distance: {d}");
        }
        let mut phases: Vec<f32> = falls.iter().map(|i| i.bob_phase).collect();
        phases.sort_by(f32::total_cmp);
        assert!(phases.windows(2).all(|w| w[1] - w[0] > 0.3), "{phases:?}");
    }
}
