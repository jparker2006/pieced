//! The gun and glove viewmodel models (docs/M2-SPEC.md → Guns and gloves, Asset
//! pipeline): the rifle's and pump's named parts and attach points exist where the
//! viewmodel code needs them, the muzzle points forward (Bevy -Z), the iron sights
//! make a level sight line, the crystals sit in their sockets, and one pair of
//! gloves fits both guns' grips.

use bevy::prelude::*;
use pieced::models::{EMBEDDED_MODELS, Sidecar};

fn sidecar(name: &str) -> Sidecar {
    let model = EMBEDDED_MODELS
        .iter()
        .find(|m| m.name == name)
        .unwrap_or_else(|| panic!("{name} is not embedded"));
    Sidecar::parse(model.sidecar).unwrap()
}

fn attach(side: &Sidecar, name: &str) -> Vec3 {
    side.attach(name)
        .unwrap_or_else(|| panic!("{}: no attach point {name}", side.name))
        .position()
}

fn contains(bounds: &pieced::models::Bounds, p: Vec3, tolerance: f32) -> bool {
    p.cmpge(bounds.min() - tolerance).all() && p.cmple(bounds.max() + tolerance).all()
}

const GUNS: [&str; 2] = ["rifle", "pump"];
const GUN_ATTACH: [&str; 6] = [
    "MuzzleTip",
    "CrystalSocket",
    "GripR",
    "GripL",
    "Sight",
    "SightFront",
];

#[test]
fn guns_have_their_named_parts_and_attach_points() {
    let rifle = sidecar("rifle");
    for part in ["Body", "Stock", "Chamber", "Crystal", "Mag", "Muzzle"] {
        assert!(rifle.part(part).is_some(), "rifle: no part {part}");
    }
    let pump = sidecar("pump");
    for part in [
        "Body", "Stock", "Crystal", "Rings", "PumpGrip", "Shard", "Muzzle",
    ] {
        assert!(pump.part(part).is_some(), "pump: no part {part}");
    }
    for name in GUNS {
        let side = sidecar(name);
        assert_eq!(side.kind, "gun");
        for a in GUN_ATTACH {
            attach(&side, a);
        }
    }
    let gloves = sidecar("gloves");
    assert_eq!(gloves.kind, "gloves");
    for part in ["GloveR", "GloveL"] {
        assert!(gloves.part(part).is_some(), "gloves: no part {part}");
    }
}

/// The spec pins the rifle's muzzle pointing forward: the muzzle tip is the
/// model's front-most point, on the barrel axis, facing Bevy -Z.
#[test]
fn muzzles_point_forward() {
    for name in GUNS {
        let side = sidecar(name);
        let tip = attach(&side, "MuzzleTip");
        assert!(
            (tip.z - side.bounds.min().z).abs() < 0.01,
            "{name}: the muzzle tip ({tip}) is the front of the model ({:?})",
            side.bounds
        );
        let muzzle = side.part("Muzzle").unwrap().bounds;
        assert!(
            (tip.xy() - muzzle.center().xy()).length() < 0.005,
            "{name}: the muzzle tip is on the muzzle's axis"
        );
        let forward = side.attach("MuzzleTip").unwrap().transform().forward();
        assert!(
            forward.dot(Vec3::NEG_Z) > 0.999,
            "{name}: the muzzle faces -Z ({forward})"
        );
        // The stock is at the back, behind the grip.
        let stock = side.part("Stock").unwrap().bounds;
        assert!(
            stock.max().z > attach(&side, "GripR").z,
            "{name}: stock behind grip"
        );
        assert!(
            stock.min().z > tip.z + 0.4,
            "{name}: stock far behind the muzzle"
        );
    }
}

/// Aim-down-sights looks through the rear notch at the front sight: both lie on a
/// level line along the gun's centre, above the barrel, the front one ahead.
#[test]
fn iron_sights_make_a_level_sight_line() {
    for name in GUNS {
        let side = sidecar(name);
        let (rear, front) = (attach(&side, "Sight"), attach(&side, "SightFront"));
        assert!(
            rear.x.abs() < 1e-4 && front.x.abs() < 1e-4,
            "{name}: sights centred"
        );
        assert!(
            (rear.y - front.y).abs() < 1e-3,
            "{name}: sight line is level"
        );
        assert!(
            front.z < rear.z - 0.3,
            "{name}: front sight well ahead of the rear"
        );
        assert!(
            rear.y > attach(&side, "MuzzleTip").y + 0.05,
            "{name}: sight line above the bore"
        );
    }
}

#[test]
fn crystals_sit_in_their_sockets() {
    for name in GUNS {
        let side = sidecar(name);
        let socket = attach(&side, "CrystalSocket");
        let crystal = side.part("Crystal").unwrap().bounds;
        assert!(
            (crystal.center() - socket).length() < 0.01,
            "{name}: the crystal is centred on its socket"
        );
        assert!(
            side.part("Crystal")
                .unwrap()
                .colors
                .iter()
                .any(|c| c.starts_with("crystal_")),
            "{name}: crystal colour"
        );
    }
    let rifle = sidecar("rifle");
    let chamber = rifle.part("Chamber").unwrap().bounds;
    let crystal = rifle.part("Crystal").unwrap().bounds;
    assert!(
        contains(&chamber, crystal.min(), 0.0) && contains(&chamber, crystal.max(), 0.0),
        "rifle: the crystal is inside the glass chamber"
    );
    let pump = sidecar("pump");
    let rings = pump.part("Rings").unwrap().bounds;
    let socket = attach(&pump, "CrystalSocket");
    assert!(
        (rings.center() - socket).length() < 0.02,
        "pump: the rings wrap the crystal"
    );
    assert!(
        pump.part("Crystal").unwrap().bounds.size().z > 0.1,
        "pump: a long crystal"
    );
}

/// One pair of gloves fits both guns: the pistol grips are the same shape and
/// angle, and the pump's left hand rides on the sliding pump grip.
#[test]
fn one_pair_of_gloves_fits_both_guns() {
    let (rifle, pump) = (sidecar("rifle"), sidecar("pump"));
    let rot =
        |side: &Sidecar, a: &str| Quat::from_array(side.attach(a).unwrap().rotation).normalize();
    assert!(
        rot(&rifle, "GripR").abs_diff_eq(rot(&pump, "GripR"), 1e-5),
        "same pistol grip angle"
    );
    let up = rot(&rifle, "GripR") * Vec3::Y;
    assert!(
        up.y > 0.9 && up.z < -0.1,
        "the grip rakes back as it goes down, so up it leans forward ({up})"
    );
    assert_eq!(
        pump.attach("GripL").unwrap().parent.as_deref(),
        Some("PumpGrip"),
        "the pump's left hand moves with the pump grip"
    );
    let grip = pump.part("PumpGrip").unwrap().bounds;
    assert!(contains(&grip, attach(&pump, "GripL"), 0.0));
    // The left hand holds in front of the right, under the gun's centre line.
    for side in [&rifle, &pump] {
        let (l, r) = (attach(side, "GripL"), attach(side, "GripR"));
        assert!(
            l.z < r.z - 0.2,
            "{}: left hand ahead of the right",
            side.name
        );
        assert!(
            l.y < attach(side, "CrystalSocket").y,
            "{}: under the gun",
            side.name
        );
    }
}
