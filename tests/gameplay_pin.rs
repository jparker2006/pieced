//! Milestone 4 changes no gameplay numbers (D117). This pins every gameplay
//! section of `Tuning`, plus the hitboxes and the grid, to their values at
//! M3's close (`d722340`), stored in `tests/fixtures/gameplay-pin.json`.
//!
//! A failure here means a gameplay number moved. That needs Jake's OK: if he
//! gave it, regenerate the fixture with `PIECED_PIN_WRITE=1` and say so in the
//! report.

use pieced::shared::{ARENA_CELLS, CELL_SIZE, LEVEL_HEIGHT, MAX_LEVELS, TICK_HZ};
use pieced::tuning::Tuning;
use serde_json::{Value, json};

fn gameplay_numbers() -> Value {
    let t = Tuning::default();
    json!({
        "movement": t.movement,
        "building": t.building,
        "combat": t.combat,
        "dummy": t.dummy,
        "grunt": t.grunt,
        "orb": t.orb,
        "waves": t.waves,
        "hitboxes": {
            "body_radius": pieced::player::BODY_RADIUS,
            "body_bottom": pieced::player::BODY_BOTTOM,
            "body_top": pieced::player::BODY_TOP,
            "head_center": pieced::player::HEAD_CENTER,
            "head_radius": pieced::player::HEAD_RADIUS,
        },
        "grid": {
            "cell_size": CELL_SIZE,
            "level_height": LEVEL_HEIGHT,
            "arena_cells": ARENA_CELLS,
            "max_levels": MAX_LEVELS,
            "tick_hz": TICK_HZ,
        },
    })
}

#[test]
fn gameplay_numbers_match_the_m3_pin() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gameplay-pin.json");
    let text = serde_json::to_string_pretty(&gameplay_numbers()).unwrap() + "\n";
    if std::env::var_os("PIECED_PIN_WRITE").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &text).unwrap();
    }
    // Both sides go through the same text parse, so float formatting can't differ.
    let now: Value = serde_json::from_str(&text).unwrap();
    let pinned: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("gameplay pin fixture"))
            .unwrap();
    assert_eq!(
        now, pinned,
        "a gameplay number changed (D117): that needs Jake's OK before the pin moves"
    );
}
