//! Offscreen review of editing and the cone (D43, D44): runs the real client
//! look with no window and writes PNGs of edited walls (a window, a door, an
//! arch, a triangle and a pillar), the edit grid mid-selection, a cone capping
//! a 1×1 box, the edited cone roofs, and half ramps.
//!
//! It needs a GPU, so it is ignored by default. Run it by hand:
//!
//! ```sh
//! PIECED_EDIT_OUT=/some/dir cargo test --locked --test edit_offscreen -- --ignored --nocapture
//! ```

use avian3d::prelude::PhysicsPlugins;
use bevy::{
    log::LogPlugin,
    prelude::*,
    render::{
        RenderPlugin,
        pipelined_rendering::PipelinedRenderingPlugin,
        settings::{Backends, RenderCreation, WgpuSettings},
        view::screenshot::{Screenshot, save_to_disk},
    },
    window::ExitCondition,
    winit::WinitPlugin,
};
use pieced::{
    app::{BootPlugin, SimPlugins},
    arena::visuals::ArenaVisualsPlugin,
    building::{
        EditDrag, EditMode, EditSession, PieceEdit, PieceSlot, clear_pieces, edit::tile_quad,
        edit_piece, place_piece, visuals::BuildingVisualsPlugin,
    },
    fx::FxPlugin,
    look::LookPlugin,
    models::ModelsPlugin,
    render::{RenderSetupPlugin, WorldTarget},
    scenario::gallery::GalleryCamera,
    shared::{AppState, Facing, GridCell, LookAngles, Player, PlayerIntent, PreviousFeet},
    viewmodel::ViewmodelPlugin,
};
use std::path::PathBuf;

fn app() -> App {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .build()
            .disable::<WinitPlugin>()
            .disable::<PipelinedRenderingPlugin>()
            .disable::<bevy::audio::AudioPlugin>()
            .disable::<LogPlugin>()
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                close_when_requested: false,
                ..default()
            })
            .set(RenderPlugin {
                render_creation: RenderCreation::Automatic(Box::new(WgpuSettings {
                    backends: Some(Backends::METAL),
                    ..default()
                })),
                synchronous_pipeline_compilation: true,
                ..default()
            }),
    )
    .add_plugins(PhysicsPlugins::default())
    .add_plugins(SimPlugins)
    .add_plugins((
        RenderSetupPlugin,
        LookPlugin,
        ModelsPlugin,
        ArenaVisualsPlugin,
        BuildingVisualsPlugin,
        ViewmodelPlugin,
        FxPlugin,
        BootPlugin,
    ));
    app.finish();
    app.cleanup();
    app
}

fn frames(app: &mut App, n: usize) {
    for _ in 0..n {
        app.update();
    }
}

fn capture(app: &mut App, path: PathBuf) {
    let _ = std::fs::remove_file(&path);
    let image = app.world().resource::<WorldTarget>().image.clone();
    app.world_mut()
        .spawn(Screenshot::image(image))
        .observe(save_to_disk(path.clone()));
    for _ in 0..30 {
        app.update();
        if path.exists() {
            app.update();
            return;
        }
    }
    panic!("no screenshot at {}", path.display());
}

fn look_from(app: &mut App, eye: Vec3, at: Vec3) {
    app.insert_resource(GalleryCamera(
        Transform::from_translation(eye).looking_at(at, Vec3::Y),
    ));
    frames(app, 6);
}

fn cell(x: i32, z: i32, level: i32) -> GridCell {
    GridCell::new(x, z, level)
}

fn put(app: &mut App, slot: PieceSlot, edit: PieceEdit) -> Entity {
    let e = place_piece(app.world_mut(), slot).expect("placed");
    if edit.is_edited() {
        assert!(edit_piece(app.world_mut(), e, edit));
    }
    e
}

fn player(app: &mut App) -> Entity {
    let world = app.world_mut();
    let mut q = world.query_filtered::<Entity, With<Player>>();
    q.single(world).unwrap()
}

#[test]
#[ignore = "needs a GPU: run by hand to review editing and the cone"]
fn render_edits_offscreen() {
    let out = std::env::var("PIECED_EDIT_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("pieced-edit"));
    std::fs::create_dir_all(&out).unwrap();
    let mut app = app();
    let mut boot = 0;
    while *app.world().resource::<State<AppState>>().get() == AppState::Boot {
        app.update();
        boot += 1;
        assert!(boot < 200, "Boot never ended");
    }
    println!("Boot ended after {boot} frames");
    frames(&mut app, 10);
    clear_pieces(app.world_mut());
    // Park the player out of the way, looking at nothing in particular.
    let p = player(&mut app);
    app.world_mut().get_mut::<Transform>(p).unwrap().translation =
        cell(1, 11, 0).base_center();

    // A row of edited walls along z line 8, seen from the south.
    let walls = [
        (3, PieceEdit::of(&[4])),
        (4, PieceEdit::of(&[4, 7])),
        (5, PieceEdit::of(&[4, 6, 7, 8])),
        (6, PieceEdit::of(&[0, 1, 3])),
        (7, PieceEdit::of(&[1, 2, 4, 5, 7, 8])),
        (8, PieceEdit::of(&[0, 1, 2])),
    ];
    for (x, e) in walls {
        put(&mut app, PieceSlot::wall(cell(x, 8, 0), Facing::North), e);
    }
    frames(&mut app, 20);
    let row = cell(4, 8, 0).base_center();
    look_from(&mut app, row + Vec3::new(1.5, 2.2, 9.0), row + Vec3::new(1.5, 1.4, -2.0));
    capture(&mut app, out.join("01-walls-window-door-arch.png"));
    let right = cell(7, 8, 0).base_center();
    look_from(&mut app, right + Vec3::new(0.0, 2.0, 8.0), right + Vec3::new(0.0, 1.3, -2.0));
    capture(&mut app, out.join("02-walls-triangle-pillar-midwall.png"));
    look_from(&mut app, row + Vec3::new(-3.0, 1.6, -4.0), row + Vec3::new(1.0, 1.2, 0.0));
    capture(&mut app, out.join("03-walls-back-side.png"));

    // The edit grid mid-selection: dragging a door on a fresh wall.
    let slot = PieceSlot::wall(cell(4, 5, 0), Facing::North);
    let wall = put(&mut app, slot, PieceEdit::FULL);
    let feet = cell(4, 5, 0).base_center() + Vec3::Z * 0.5;
    let tile7 = slot.transform().transform_point(
        tile_quad(slot.kind, 7).iter().copied().sum::<Vec3>() / 4.0,
    );
    {
        let world = app.world_mut();
        world.get_mut::<Transform>(p).unwrap().translation = feet;
        world.get_mut::<PreviousFeet>(p).unwrap().0 = feet;
        let d = (tile7 - (feet + Vec3::Y * 1.62)).normalize();
        *world.get_mut::<LookAngles>(p).unwrap() = LookAngles {
            yaw: (-d.x).atan2(-d.z),
            pitch: d.y.asin(),
        };
        // Held: the drag stays open.
        world.get_mut::<PlayerIntent>(p).unwrap().fire = true;
        world.get_mut::<EditMode>(p).unwrap().session = Some(EditSession {
            piece: wall,
            slot,
            base: PieceEdit::FULL,
            hovered: Some(7),
            drag: Some(EditDrag {
                paint: true,
                tiles: PieceEdit::of(&[4, 7]).tiles,
                path: [4, 7, 0, 0],
                len: 2,
            }),
        });
    }
    frames(&mut app, 4);
    let c = slot.transform().translation;
    look_from(&mut app, c + Vec3::new(2.2, 0.4, 4.2), c + Vec3::new(0.0, -0.1, 0.0));
    capture(&mut app, out.join("04-edit-grid-mid-selection.png"));
    // An invalid selection turns red.
    app.world_mut()
        .get_mut::<EditMode>(p)
        .unwrap()
        .session
        .as_mut()
        .unwrap()
        .drag = Some(EditDrag {
        paint: true,
        tiles: PieceEdit::of(&[0, 4, 8]).tiles,
        path: [0, 4, 8, 0],
        len: 3,
    });
    // Aim at 8 so the hovered tile doesn't grow the drag.
    frames(&mut app, 2);
    capture(&mut app, out.join("05-edit-grid-invalid.png"));
    {
        let world = app.world_mut();
        world.get_mut::<EditMode>(p).unwrap().session = None;
        world.get_mut::<PlayerIntent>(p).unwrap().fire = false;
        world.get_mut::<Transform>(p).unwrap().translation = cell(1, 11, 0).base_center();
    }

    // A cone capping a 1×1 box, and the four edited roofs beside it.
    let bx = cell(4, 2, 0);
    for f in Facing::ALL {
        put(&mut app, PieceSlot::wall(bx, f), PieceEdit::FULL);
    }
    put(&mut app, PieceSlot::cone(cell(4, 2, 1)), PieceEdit::FULL);
    for (x, e) in [
        (6, PieceEdit::of(&[0])),
        (7, PieceEdit::of(&[0, 1])),
        (8, PieceEdit::of(&[0, 3])),
        (9, PieceEdit::of(&[0, 1, 2])),
    ] {
        put(&mut app, PieceSlot::cone(cell(x, 2, 0)), e);
    }
    frames(&mut app, 20);
    let b = bx.base_center();
    look_from(&mut app, b + Vec3::new(6.0, 5.5, 8.0), b + Vec3::new(0.0, 2.2, 0.0));
    capture(&mut app, out.join("06-cone-on-a-box.png"));
    let roofs = cell(7, 2, 0).base_center() + Vec3::X * 2.0;
    look_from(&mut app, roofs + Vec3::new(-2.0, 5.0, 10.0), roofs + Vec3::new(0.0, 0.4, 0.0));
    capture(&mut app, out.join("07-cone-edits.png"));
    // From inside the box, looking up at the cone's underside.
    look_from(&mut app, b + Vec3::new(0.8, 1.4, 1.2), b + Vec3::new(-0.2, 3.0, -0.5));
    capture(&mut app, out.join("08-cone-from-inside-the-box.png"));

    // Half ramps and edited floors.
    put(&mut app, PieceSlot::ramp(cell(8, 5, 0), Facing::North), PieceEdit::path(2, 0));
    put(&mut app, PieceSlot::ramp(cell(9, 5, 0), Facing::North), PieceEdit::path(3, 2));
    put(&mut app, PieceSlot::floor(cell(6, 5, 1)), PieceEdit::of(&[0, 1]));
    put(&mut app, PieceSlot::floor(cell(7, 5, 1)), PieceEdit::of(&[3]));
    frames(&mut app, 20);
    let h = cell(8, 5, 0).base_center();
    look_from(&mut app, h + Vec3::new(-2.0, 5.0, 9.0), h + Vec3::new(-1.0, 1.0, 0.0));
    capture(&mut app, out.join("09-half-ramps-and-floors.png"));
    println!("wrote PNGs to {}", out.display());
}
