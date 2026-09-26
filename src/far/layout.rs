//! Where everything in the far view sits: the composition of T01 seen from the
//! player spawn (2, 0, 14) facing -Z. Pure data, so tests can check it.
//!
//! - The galaxy's core is upper left: azimuth -21°, 28° up.
//! - The station fills the upper-right third: azimuth 30°, 640 m out, its
//!   platform 152 m up, so the whole of it shows from spawn, from the rock's tip
//!   (3° up) to the flèche (32° up) and from its left sail (14°) to its right
//!   one (46°). Its waterfalls fall to the horizon.
//! - The ringed planet sits low in the middle of the view below and left of the
//!   station: azimuth 2°, 12° up, 800 m out, about 13° across (26° with rings).
//! - Eighteen floating islands ring the arena 125–240 m out, every direction
//!   has some and the spawn view has seven; eleven pour waterfalls, the small
//!   high islets don't.
//! - Five ships fly four loops round the station: a wide orbit above the ring
//!   walkway, a high lap weaving between the spires and the sails, a sweep past
//!   the planet, and a high loop on the right.

use super::galaxy::sky_point;
use bevy::prelude::*;

/// The player spawn's eye (feet (2, 0, 14) + 1.6 m), the reference viewpoint.
pub const SPAWN_EYE: Vec3 = Vec3::new(2.0, 1.6, 14.0);
/// The station's bearing and horizontal distance from the spawn eye, and the
/// height of its platform (the model's `Platform` point).
pub const STATION_AZIMUTH: f32 = 30.0;
pub const STATION_DISTANCE: f32 = 640.0;
pub const STATION_PLATFORM_Y: f32 = 152.0;

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
            let dir = sky_point(STATION_AZIMUTH, 0.0);
            Vec3::new(
                SPAWN_EYE.x + dir.x * STATION_DISTANCE,
                STATION_PLATFORM_Y,
                SPAWN_EYE.z + dir.z * STATION_DISTANCE,
            )
        };
        let planet_at = SPAWN_EYE + sky_point(2.0, 12.0) * 800.0;
        let piece = |model, anchor, position: Vec3, scale| FarPiece {
            model,
            anchor,
            position,
            yaw: facing_arena(position),
            scale,
        };
        // (model, azimuth from the arena centre, distance, top height, scale,
        // waterfall). The first seven are in the spawn view.
        let islands = [
            ("far_island_a", -42.0, 150.0, 20.0, 1.1, true),
            ("far_island_b", -24.0, 190.0, 26.0, 1.1, true),
            ("far_island_c", -8.0, 165.0, 36.0, 0.7, false),
            ("far_island_a", 12.0, 175.0, 16.0, 0.9, true),
            ("far_island_b", 38.0, 150.0, 24.0, 1.1, true),
            ("far_island_c", 24.0, 205.0, 46.0, 0.55, false),
            ("far_island_c", -58.0, 125.0, 40.0, 0.55, false),
            ("far_island_a", 75.0, 160.0, 24.0, 1.15, true),
            ("far_island_c", 100.0, 140.0, 36.0, 0.6, false),
            ("far_island_b", 125.0, 180.0, 28.0, 1.15, true),
            ("far_island_a", 160.0, 150.0, 30.0, 1.0, true),
            ("far_island_c", 182.0, 200.0, 50.0, 0.6, false),
            ("far_island_b", -160.0, 170.0, 28.0, 1.1, true),
            ("far_island_a", -125.0, 150.0, 34.0, 0.9, true),
            ("far_island_c", -95.0, 170.0, 24.0, 0.85, true),
            ("far_island_a", -78.0, 215.0, 54.0, 0.5, false),
            ("far_island_c", 142.0, 235.0, 62.0, 0.55, false),
            ("far_island_c", 58.0, 240.0, 72.0, 0.5, false),
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
                lap_s: 10.0,
                starts: vec![0.1, 0.6],
                scale: 1.3,
            },
            // High, between the inner and outer sails and behind the flèche.
            ShipSpec {
                points: orbit(Vec3::new(0.0, 196.0, 2.0), 136.0, 56.0, 8.0, 0.9, 8),
                lap_s: 7.2,
                starts: vec![0.35],
                scale: 1.2,
            },
            // A low sweep out past the planet (the arena's left of the station).
            ShipSpec {
                points: orbit(Vec3::new(235.0, 30.0, 45.0), 140.0, 50.0, 12.0, 2.0, 8),
                lap_s: 7.0,
                starts: vec![0.8],
                scale: 1.3,
            },
            // A loop in front of the station's right side, past its waterfalls.
            ShipSpec {
                points: orbit(Vec3::new(-95.0, -5.0, -170.0), 75.0, 40.0, 10.0, 4.0, 8),
                lap_s: 7.0,
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
    fn islands_surround_the_arena_and_bob_out_of_step() {
        let layout = FarLayout::default();
        let falls = layout.islands.iter().filter(|i| i.waterfall).count();
        assert!(falls >= 4, "{falls} waterfall islands");
        let mut sectors = [0; 4];
        let mut in_spawn_view = 0;
        for island in &layout.islands {
            assert!((1.0..=3.0).contains(&island.bob_amplitude));
            assert!((6.0..=10.0).contains(&island.bob_period));
            let p = island.piece.position;
            let d = Vec2::new(p.x, p.z).length();
            assert!((110.0..260.0).contains(&d), "mid-distance: {d}");
            let az = p.x.atan2(-p.z).to_degrees().rem_euclid(360.0);
            sectors[((az + 45.0) / 90.0) as usize % 4] += 1;
            let from_spawn = (p - SPAWN_EYE).normalize();
            let view_az = from_spawn.x.atan2(-from_spawn.z).to_degrees();
            in_spawn_view += usize::from(view_az.abs() < 42.0);
        }
        assert!(
            sectors.iter().all(|&n| n >= 3),
            "every direction: {sectors:?}"
        );
        assert!(
            in_spawn_view >= 5,
            "{in_spawn_view} islands in the spawn view"
        );
        // Neighbours in the ring bob out of step.
        for pair in layout.islands.windows(2) {
            let d = (pair[0].bob_phase - pair[1].bob_phase).rem_euclid(std::f32::consts::TAU);
            assert!((0.5..std::f32::consts::TAU - 0.5).contains(&d), "{d}");
        }
    }

    #[test]
    fn the_whole_station_fits_the_upper_right_third_from_spawn() {
        // The station's extent in model space (sails to flèche, rock tip).
        let (half_width, above, below) = (183.0f32, 257.0f32, 118.0f32);
        let d = STATION_DISTANCE;
        let right = STATION_AZIMUTH + (half_width / d).atan().to_degrees();
        let left = STATION_AZIMUTH - (half_width / d).atan().to_degrees();
        let top = ((STATION_PLATFORM_Y + above - SPAWN_EYE.y) / d)
            .atan()
            .to_degrees();
        let bottom = ((STATION_PLATFORM_Y - below - SPAWN_EYE.y) / d)
            .atan()
            .to_degrees();
        // The spawn view is ±47° wide and ±35° tall.
        assert!(left > 10.0 && right < 47.0, "{left}°..{right}°");
        assert!(bottom > 2.0 && top < 35.0, "{bottom}°..{top}°");
    }
}
