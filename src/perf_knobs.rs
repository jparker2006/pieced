//! `--knobs k=v,...`: debug switches for performance experiments. They turn
//! expensive features off one at a time so their cost shows up in frame timing
//! (Metal has no per-pass GPU timestamps in Bevy). Never used in normal play.
//!
//! Knobs: `shadows=off`, `msaa=1|2|4`, `viewmodel=off`, `scale=<f32>`,
//! `fog=off`, `sky=off`, `vmmsaa=1|2|4` (viewmodel camera only).

use crate::{render::MainCamera, tuning::Tuning, viewmodel::ViewmodelCamera};
use bevy::{pbr::DistanceFog, prelude::*};

#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct PerfKnobs {
    pub raw: String,
    pub shadows: Option<bool>,
    pub msaa: Option<u32>,
    pub viewmodel: Option<bool>,
    pub scale: Option<f32>,
    pub fog: Option<bool>,
    pub sky: Option<bool>,
    pub viewmodel_msaa: Option<u32>,
}

impl PerfKnobs {
    pub fn from_args(args: &[String]) -> Option<Self> {
        let raw = args
            .iter()
            .position(|a| a == "--knobs")
            .and_then(|i| args.get(i + 1))?
            .clone();
        let mut knobs = PerfKnobs {
            raw: raw.clone(),
            ..default()
        };
        for pair in raw.split(',').filter(|p| !p.is_empty()) {
            let (key, value) = pair.split_once('=').unwrap_or((pair, "on"));
            let on = !matches!(value, "off" | "0" | "false");
            match key {
                "shadows" => knobs.shadows = Some(on),
                "msaa" => knobs.msaa = value.parse().ok(),
                "viewmodel" => knobs.viewmodel = Some(on),
                "scale" => knobs.scale = value.parse().ok(),
                "fog" => knobs.fog = Some(on),
                "sky" => knobs.sky = Some(on),
                "vmmsaa" => knobs.viewmodel_msaa = value.parse().ok(),
                other => eprintln!("unknown knob '{other}'"),
            }
        }
        Some(knobs)
    }
}

pub struct PerfKnobsPlugin;

impl Plugin for PerfKnobsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, apply_knobs.run_if(resource_exists::<PerfKnobs>));
    }
}

fn apply_knobs(
    knobs: Res<PerfKnobs>,
    mut commands: Commands,
    mut tuning: ResMut<Tuning>,
    mut lights: Query<&mut DirectionalLight>,
    mut cameras: Query<(Entity, &mut Msaa, Has<MainCamera>, Has<ViewmodelCamera>)>,
    mut viewmodel: Query<&mut Camera, With<ViewmodelCamera>>,
    fog: Query<Entity, With<DistanceFog>>,
    mut sky: Query<&mut Visibility, With<crate::arena::visuals::SkyDome>>,
) {
    if let Some(scale) = knobs.scale
        && tuning.graphics.render_scale != scale
    {
        tuning.graphics.render_scale = scale;
    }
    if knobs.shadows == Some(false) {
        for mut light in &mut lights {
            if light.shadow_maps_enabled {
                light.shadow_maps_enabled = false;
            }
        }
    }
    if let Some(samples) = knobs.msaa {
        let want = match samples {
            1 => Msaa::Off,
            2 => Msaa::Sample2,
            8 => Msaa::Sample8,
            _ => Msaa::Sample4,
        };
        for (_, mut msaa, main, vm) in &mut cameras {
            if (main || vm) && *msaa != want {
                *msaa = want;
            }
        }
    }
    if let Some(samples) = knobs.viewmodel_msaa {
        let want = match samples {
            1 => Msaa::Off,
            2 => Msaa::Sample2,
            _ => Msaa::Sample4,
        };
        for (_, mut msaa, _, vm) in &mut cameras {
            if vm && *msaa != want {
                *msaa = want;
            }
        }
    }
    if knobs.viewmodel == Some(false) {
        for mut camera in &mut viewmodel {
            if camera.is_active {
                camera.is_active = false;
            }
        }
    }
    if knobs.fog == Some(false) {
        for entity in &fog {
            commands.entity(entity).remove::<DistanceFog>();
        }
    }
    if knobs.sky == Some(false) {
        for mut visibility in &mut sky {
            visibility.set_if_neq(Visibility::Hidden);
        }
    }
    let _ = cameras.iter().count();
}
