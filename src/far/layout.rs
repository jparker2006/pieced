//! Where everything in the far view sits: the composition of T01 seen from the
//! player spawn (2, 0, 14) facing -Z. Pure data, so tests can check it.
//!
//! - The galaxy's core is upper left: azimuth -21°, 28° up.
//! - The station fills the upper-right quarter: azimuth 30°, 640 m out, its
//!   platform 120 m up, so from spawn its ring spans about 12°–48° of azimuth,
//!   its front rim is about 14° up, its rock hangs to the horizon and its
//!   flèche just reaches the top of a level view (T01, T02): broader than it is
//!   tall, as the targets paint it. Its waterfalls fall past the horizon.
//! - The ringed planet sits low in the middle of the view below and left of the
//!   station: azimuth 4°, 13° up, 800 m out, about 13° across.
//! - Floating islands fill the sky in three depth layers (T01, T03, T11): a near
//!   ring of fourteen 180–245 m out all round the arena (the ones the scenery
//!   wraps in cloud), kept out of the station's quarter; and a deep field of
//!   forty more 300–900 m out, clustered round the station and along the
//!   horizon of every way the gallery looks (azimuth -50° to 120°), from
//!   pebbles to big castle islands, some below the horizon for the view over
//!   the edge (T11). Most pour waterfalls. They all share five meshes, turned
//!   and scaled.
//! - Five ships fly four loops round the station: a wide orbit above the ring
//!   walkway, a high lap across the facade between the spires, a sweep under the
//!   ring out past the planet, and a low loop in front of its right side.

use super::galaxy::sky_point;
use bevy::prelude::*;

/// The player spawn's eye (feet (2, 0, 14) + 1.6 m), the reference viewpoint.
pub const SPAWN_EYE: Vec3 = Vec3::new(2.0, 1.6, 14.0);
/// The station's bearing and horizontal distance from the spawn eye, and the
/// height of its platform (the model's `Platform` point).
pub const STATION_AZIMUTH: f32 = 30.0;
pub const STATION_DISTANCE: f32 = 640.0;
pub const STATION_PLATFORM_Y: f32 = 120.0;

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
    /// The near ring of islands (the scenery wraps these in cloud).
    pub islands: Vec<IslandSpec>,
    /// The deep field of islands beyond the near ring.
    pub distant_islands: Vec<IslandSpec>,
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

/// One island of the table: model, azimuth from the arena centre (degrees),
/// distance (m), top height (m), scale, waterfall.
type IslandRow = (&'static str, f32, f32, f32, f32, bool);

/// The near ring: fourteen islands all round the arena, 175–245 m out, their
/// undersides clear of the horizon from the arena. The first five are in the
/// spawn view.
const NEAR_ISLANDS: [IslandRow; 14] = [
    ("far_island_a", -34.0, 205.0, 52.0, 1.25, true),
    ("far_island_b", -20.0, 235.0, 70.0, 1.0, true),
    ("far_island_c", -8.0, 185.0, 44.0, 0.95, true),
    ("far_island_d", 62.0, 238.0, 44.0, 1.0, true),
    ("far_island_a", 86.0, 212.0, 50.0, 1.1, true),
    ("far_island_b", 110.0, 230.0, 58.0, 1.0, true),
    ("far_island_c", 136.0, 190.0, 60.0, 0.9, true),
    ("far_island_a", 162.0, 220.0, 50.0, 1.1, true),
    ("far_island_d", 188.0, 245.0, 48.0, 0.9, true),
    ("far_island_c", 214.0, 190.0, 62.0, 0.9, true),
    ("far_island_b", 240.0, 225.0, 54.0, 1.0, true),
    ("far_island_a", 266.0, 205.0, 46.0, 1.1, true),
    ("far_island_c", 290.0, 180.0, 58.0, 0.8, false),
    ("far_island_b", 310.0, 215.0, 50.0, 1.05, true),
];

/// The deep field, 300–950 m out: round the station (its flanks and in front
/// of its rock, azimuth 0°–70°), along the horizon every way the gallery
/// looks, and a few below the horizon for the view over the island's edge
/// (T11).
const DISTANT_ISLANDS: [IslandRow; 46] = [
    // Round the station: up its flanks, beside and in front of its rock.
    ("far_island_c", 10.0, 520.0, 250.0, 1.3, true),
    ("far_islet", 2.0, 600.0, 260.0, 2.2, false),
    ("far_island_a", 13.0, 560.0, 60.0, 1.6, true),
    ("far_island_b", 52.0, 520.0, 70.0, 1.5, true),
    ("far_island_d", 58.0, 600.0, 150.0, 1.4, true),
    ("far_islet", 50.0, 580.0, 200.0, 2.2, false),
    ("far_island_c", 62.0, 620.0, 260.0, 1.6, true),
    ("far_island_a", 66.0, 720.0, 110.0, 1.8, true),
    ("far_islet", 26.0, 470.0, 24.0, 1.8, false),
    ("far_islet", 38.0, 500.0, 60.0, 2.0, false),
    ("far_islet", 4.0, 700.0, 330.0, 2.6, false),
    // Mid distance along the spawn view's horizon.
    ("far_island_d", -52.0, 380.0, 64.0, 1.2, true),
    ("far_island_b", -33.0, 430.0, 96.0, 1.3, true),
    ("far_island_a", -20.0, 350.0, 44.0, 1.0, true),
    ("far_islet", -44.0, 320.0, 120.0, 1.6, false),
    ("far_island_c", -6.0, 470.0, 36.0, 1.4, true),
    ("far_islet", 9.0, 360.0, 30.0, 1.8, false),
    ("far_island_c", -2.0, 430.0, 30.0, 1.2, true),
    ("far_island_c", -46.0, 600.0, 150.0, 1.6, true),
    // Far out on the horizon, both sides of the station.
    ("far_island_b", -14.0, 780.0, 60.0, 2.0, true),
    ("far_island_a", -40.0, 820.0, 80.0, 2.2, true),
    ("far_island_d", 76.0, 820.0, 70.0, 1.8, true),
    ("far_islet", -26.0, 900.0, 140.0, 3.0, false),
    ("far_island_c", 88.0, 900.0, 90.0, 2.4, true),
    // Toward the right-hand views (T03, T06, T09, T11).
    ("far_island_b", 76.0, 420.0, 54.0, 1.2, true),
    ("far_island_c", 97.0, 360.0, 84.0, 1.0, true),
    ("far_island_a", 108.0, 470.0, 44.0, 1.4, true),
    ("far_islet", 84.0, 330.0, 110.0, 1.6, false),
    ("far_island_d", 122.0, 560.0, 70.0, 1.5, true),
    ("far_islet", 70.0, 380.0, 128.0, 1.8, false),
    // Below the horizon, seen past the edge (T11).
    ("far_island_d", 58.0, 420.0, -40.0, 1.3, true),
    ("far_island_a", 82.0, 330.0, -30.0, 1.1, true),
    ("far_island_c", 45.0, 360.0, -44.0, 1.0, true),
    ("far_islet", 70.0, 300.0, -18.0, 1.5, false),
    ("far_island_b", 100.0, 520.0, -60.0, 1.4, true),
    // High in the sky, round the galaxy and over the right-hand views.
    ("far_islet", -58.0, 520.0, 280.0, 2.4, false),
    ("far_island_c", -35.0, 700.0, 450.0, 1.8, true),
    ("far_islet", -5.0, 450.0, 230.0, 2.0, false),
    ("far_island_c", 70.0, 480.0, 300.0, 1.4, true),
    ("far_islet", 95.0, 520.0, 330.0, 2.6, false),
    ("far_island_b", -80.0, 600.0, 260.0, 1.6, true),
    // The rest of the sky, sparser.
    ("far_island_a", 150.0, 520.0, 60.0, 1.4, true),
    ("far_island_c", 200.0, 460.0, 80.0, 1.2, true),
    ("far_island_b", 250.0, 540.0, 56.0, 1.4, true),
    ("far_island_d", 300.0, 600.0, 64.0, 1.5, true),
    ("far_islet", 330.0, 400.0, 110.0, 1.8, false),
];

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
        let planet_at = SPAWN_EYE + sky_point(4.0, 13.0) * 800.0;
        let piece = |model, anchor, position: Vec3, scale| FarPiece {
            model,
            anchor,
            position,
            yaw: facing_arena(position),
            scale,
        };
        // Islands, numbered on through both tables so neighbours bob out of
        // step and turn differently.
        let island = |k: usize, &(model, az, dist, height, scale, waterfall): &IslandRow| {
            let k = k as f32;
            let position = around_arena(az, dist, height);
            // Turned up to ±35° off facing the arena, so the shared meshes
            // don't repeat their faces (their waterfalls still pour toward it).
            let turn = ((k * 0.618_034).fract() * 70.0 - 35.0).to_radians();
            IslandSpec {
                piece: FarPiece {
                    yaw: facing_arena(position) + turn,
                    ..piece(model, "Top", position, scale)
                },
                bob_amplitude: 1.2 + (k * 0.618).fract() * 1.6,
                bob_period: 6.2 + (k * 0.382 + 0.2).fract() * 3.5,
                // The golden angle keeps neighbours out of step.
                bob_phase: (k * 2.399_963).rem_euclid(std::f32::consts::TAU),
                waterfall,
            }
        };
        let islands = NEAR_ISLANDS
            .iter()
            .enumerate()
            .map(|(k, row)| island(k, row))
            .collect();
        let distant_islands = DISTANT_ISLANDS
            .iter()
            .enumerate()
            .map(|(k, row)| island(NEAR_ISLANDS.len() + k, row))
            .collect();
        let ships = vec![
            // A wide orbit just above the ring walkway, passing in front.
            ShipSpec {
                points: orbit(Vec3::new(0.0, 84.0, 0.0), 246.0, 214.0, 12.0, 0.0, 8),
                lap_s: 14.5,
                starts: vec![0.1, 0.6],
                scale: 1.4,
            },
            // High across the facade, between the front towers and the wings.
            ShipSpec {
                points: orbit(Vec3::new(0.0, 205.0, -118.0), 150.0, 30.0, 10.0, 0.9, 8),
                lap_s: 7.2,
                starts: vec![0.35],
                scale: 1.3,
            },
            // A low sweep under the ring, out past the planet (the arena's left
            // of the station).
            ShipSpec {
                points: orbit(Vec3::new(300.0, 22.0, 40.0), 110.0, 50.0, 10.0, 2.0, 8),
                lap_s: 6.0,
                starts: vec![0.8],
                scale: 1.4,
            },
            // A loop in front of the station's right side, past its waterfalls.
            ShipSpec {
                points: orbit(Vec3::new(-95.0, -4.0, -190.0), 80.0, 40.0, 10.0, 4.0, 8),
                lap_s: 7.0,
                starts: vec![0.2],
                scale: 1.3,
            },
        ];
        Self {
            galaxy: sky_point(-21.0, 28.0),
            station: piece("station", "Platform", station_at, 1.0),
            planet: piece("planet", "Center", planet_at, 1.0),
            islands,
            distant_islands,
            ships,
            horizon_radius: 1300.0,
        }
    }
}

impl FarLayout {
    /// Every island: the near ring, then the deep field.
    pub fn all_islands(&self) -> impl Iterator<Item = &IslandSpec> {
        self.islands.iter().chain(&self.distant_islands)
    }

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
        // The deep field fills the rest of the spawn view.
        assert!(
            in_spawn_view >= 3,
            "{in_spawn_view} islands in the spawn view"
        );
        // Neighbours in the ring bob out of step.
        for pair in layout.islands.windows(2) {
            let d = (pair[0].bob_phase - pair[1].bob_phase).rem_euclid(std::f32::consts::TAU);
            assert!((0.5..std::f32::consts::TAU - 0.5).contains(&d), "{d}");
        }
    }

    /// The station's sidecar (model space, platform at its `Platform` point).
    fn station_sidecar() -> crate::models::Sidecar {
        crate::models::Sidecar::parse(include_str!("../../assets/models/station.json")).unwrap()
    }

    /// (azimuth, elevation) in degrees of a world point seen from the spawn eye.
    fn seen(p: Vec3) -> (f32, f32) {
        let d = (p - SPAWN_EYE).normalize();
        (d.x.atan2(-d.z).to_degrees(), d.y.asin().to_degrees())
    }

    #[test]
    fn the_station_fills_the_upper_right_quarter_from_spawn() {
        // T01, T02: the ring spans the upper right from about 12° to the
        // view's edge; the rock hangs down to the horizon; the flèche reaches
        // about the top of a level view (±35° tall, ±47° wide), so the whole
        // station is broader than it is tall.
        let side = station_sidecar();
        let platform = side.attach("Platform").unwrap().position();
        let part = |name: &str| side.part(name).unwrap().bounds;
        let d = STATION_DISTANCE;
        let half_width = part("Cathedral").max[0].max(-part("Cathedral").min[0]);
        let left = STATION_AZIMUTH - (half_width / d).atan().to_degrees();
        let right = STATION_AZIMUTH + (half_width / d).atan().to_degrees();
        assert!(
            (8.0..16.0).contains(&left) && right > 44.0,
            "{left}°..{right}°"
        );
        let height = |y: f32| {
            ((STATION_PLATFORM_Y + y - platform.y - SPAWN_EYE.y) / d)
                .atan()
                .to_degrees()
        };
        let (bottom, top) = (
            height(part("Base").min[1]),
            height(part("Cathedral").max[1]),
        );
        assert!((-8.0..3.0).contains(&bottom), "rock down to {bottom}°");
        assert!((26.0..36.0).contains(&top), "spires up to {top}°");
        assert!(top - bottom < right - left, "{bottom}°..{top}° tall");
        let (_, el) = seen(FarLayout::default().station.position);
        assert!((9.0..16.0).contains(&el), "platform {el}° up");
    }

    #[test]
    fn the_island_field_fills_every_gallery_direction() {
        let layout = FarLayout::default();
        let all: Vec<&IslandSpec> = layout.all_islands().collect();
        assert!((40..=60).contains(&all.len()), "{} islands", all.len());
        // Most pour waterfalls (the pebbles have none).
        let falls = all.iter().filter(|i| i.waterfall).count();
        assert!(falls * 3 >= all.len() * 2, "{falls} of {} pour", all.len());
        // A few share each mesh, at many scales.
        for model in [
            "far_island_a",
            "far_island_b",
            "far_island_c",
            "far_island_d",
            "far_islet",
        ] {
            let n = all.iter().filter(|i| i.piece.model == model).count();
            assert!(n >= 5, "{model}: {n}");
        }
        let scales: Vec<f32> = all.iter().map(|i| i.piece.scale).collect();
        let (lo, hi) = scales
            .iter()
            .fold((f32::MAX, 0f32), |(a, b), &s| (a.min(s), b.max(s)));
        assert!(lo < 0.9 && hi > 2.5, "scales {lo}..{hi}");
        // Every way the gallery looks (azimuth -10° to 100°, a level view
        // ±47° wide) has islands in the sky, near and far.
        for look in (-10..=100).step_by(10) {
            let look = look as f32;
            let in_view: Vec<_> = all
                .iter()
                .filter(|i| {
                    let (az, el) = seen(i.piece.position);
                    (az - look).abs() < 40.0 && (-2.0..30.0).contains(&el)
                })
                .collect();
            let far = in_view
                .iter()
                .filter(|i| i.piece.position.xz().length() > 300.0)
                .count();
            assert!(
                in_view.len() >= 8 && far >= 5,
                "looking {look}°: {} islands ({far} far)",
                in_view.len()
            );
        }
        // Denser round the station than anywhere else of the same width.
        let near_station = |az0: f32| {
            all.iter()
                .filter(|i| {
                    let (az, _) = seen(i.piece.position);
                    (az - az0).abs() < 25.0
                })
                .count()
        };
        let station = near_station(STATION_AZIMUTH);
        for az0 in [-150.0, -90.0, 120.0, 180.0] {
            assert!(station > near_station(az0), "{station} vs {az0}°");
        }
        // Some below the horizon, for the view over the edge (T11).
        assert!(
            all.iter()
                .filter(|i| seen(i.piece.position).1 < -2.0)
                .count()
                >= 4
        );
        // Nothing covers the planet's disc, and nothing near sits in front of
        // the galaxy's core.
        let planet = seen(layout.planet.position);
        for island in &all {
            let p = island.piece.position;
            let (az, el) = seen(p);
            if p.distance(SPAWN_EYE) < layout.planet.position.distance(SPAWN_EYE) {
                // The model's top radius (far.py).
                let radius = match island.piece.model {
                    "far_island_a" => 22.0,
                    "far_island_b" => 19.0,
                    "far_island_c" => 11.0,
                    "far_island_d" => 32.0,
                    _ => 5.0,
                };
                let r = (radius * island.piece.scale / p.distance(SPAWN_EYE))
                    .atan()
                    .to_degrees();
                assert!(
                    Vec2::new(az - planet.0, el - planet.1).length() > 6.4 + r,
                    "an island at {az}°, {el}° covers the planet"
                );
            }
            assert!(
                Vec2::new(az + 21.0, el - 28.0).length() > 8.0,
                "{az}°, {el}°"
            );
        }
    }
}
