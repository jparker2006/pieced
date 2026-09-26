//! `--knobs k=v,...`: debug switches for performance experiments. They turn
//! expensive features off one at a time so their cost shows up in frame timing
//! (Metal has no per-pass GPU timestamps in Bevy). Never used in normal play.
//!
//! Milestone 1 knobs: `shadows=off`, `msaa=1|2|4`, `viewmodel=off`,
//! `scale=<f32>`, `fog=off`, `sky=off`, `vmmsaa=1|2|4` (viewmodel camera only).
//! Since Milestone 2 the world has no shadow maps and no fog, so `shadows` and
//! `fog` are accepted but change nothing.
//!
//! Milestone 2 knobs (read through [`crate::look::LookSettings`]):
//! `outline=mod|hull|off`, `far=off`, `halos=off`, `particles=<cap>`,
//! `skyrot=off` (field only; the sky slice reads it) and `blobs=off`.
//! `outline=mod` also adds `bevy_mod_outline`'s plugin at startup, so its fixed
//! cost is only paid when it is being measured.

use crate::{look::OutlineBackend, tuning::Tuning, viewmodel::ViewmodelCamera};
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
    pub outline: Option<OutlineBackend>,
    pub far: Option<bool>,
    pub halos: Option<bool>,
    /// Particle pool cap (overrides `FeedbackTuning::max_particles` at startup).
    pub particles: Option<u32>,
    pub skyrot: Option<bool>,
    pub blobs: Option<bool>,
}

impl PerfKnobs {
    pub fn from_args(args: &[String]) -> Option<Self> {
        let raw = args
            .iter()
            .position(|a| a == "--knobs")
            .and_then(|i| args.get(i + 1))?;
        Some(Self::parse(raw))
    }

    /// Parses `k=v,k=v,...`. A bare key means "on"; unknown keys are reported
    /// and ignored.
    pub fn parse(raw: &str) -> Self {
        let mut knobs = PerfKnobs {
            raw: raw.to_string(),
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
                "outline" => {
                    knobs.outline = OutlineBackend::parse(value);
                    if knobs.outline.is_none() {
                        eprintln!("unknown outline mode '{value}' (mod|hull|off)");
                    }
                }
                "far" => knobs.far = Some(on),
                "halos" => knobs.halos = Some(on),
                "particles" => knobs.particles = value.parse().ok(),
                "skyrot" => knobs.skyrot = Some(on),
                "blobs" => knobs.blobs = Some(on),
                other => eprintln!("unknown knob '{other}'"),
            }
        }
        knobs
    }
}

pub struct PerfKnobsPlugin;

impl Plugin for PerfKnobsPlugin {
    fn build(&self, app: &mut App) {
        // The particle pools are sized in Startup, so the cap has to land first.
        let cap = app
            .world()
            .get_resource::<PerfKnobs>()
            .and_then(|k| k.particles);
        if let (Some(cap), Some(mut tuning)) = (cap, app.world_mut().get_resource_mut::<Tuning>()) {
            tuning.feedback.max_particles = cap;
        }
        app.add_systems(Update, apply_knobs.run_if(resource_exists::<PerfKnobs>));
    }
}

/// Milestone 1 knobs that act directly. MSAA and every Milestone 2 knob act
/// through [`crate::look::LookSettings`].
fn apply_knobs(
    knobs: Res<PerfKnobs>,
    mut commands: Commands,
    mut tuning: ResMut<Tuning>,
    mut lights: Query<&mut DirectionalLight>,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_milestone_2_knobs() {
        let raw = "outline=mod,far=off,halos=off,particles=120,skyrot=off,blobs=off";
        let k = PerfKnobs::parse(raw);
        assert_eq!(k.outline, Some(OutlineBackend::Mod));
        assert_eq!(k.far, Some(false));
        assert_eq!(k.halos, Some(false));
        assert_eq!(k.particles, Some(120));
        assert_eq!(k.skyrot, Some(false));
        assert_eq!(k.blobs, Some(false));
        assert_eq!(k.raw, raw);

        assert_eq!(
            PerfKnobs::parse("outline=hull").outline,
            Some(OutlineBackend::Hull)
        );
        assert_eq!(
            PerfKnobs::parse("outline=off").outline,
            Some(OutlineBackend::Off)
        );
        // A bare key means on; bad values are ignored.
        assert_eq!(PerfKnobs::parse("far").far, Some(true));
        assert_eq!(PerfKnobs::parse("outline=wobbly").outline, None);
        assert_eq!(PerfKnobs::parse("particles=lots").particles, None);
    }

    #[test]
    fn keeps_milestone_1_knobs() {
        let k =
            PerfKnobs::parse("shadows=off,msaa=2,vmmsaa=1,viewmodel=off,scale=0.8,fog=off,sky=off");
        assert_eq!(k.shadows, Some(false));
        assert_eq!(k.msaa, Some(2));
        assert_eq!(k.viewmodel_msaa, Some(1));
        assert_eq!(k.viewmodel, Some(false));
        assert_eq!(k.scale, Some(0.8));
        assert_eq!(k.fog, Some(false));
        assert_eq!(k.sky, Some(false));
        assert_eq!(k.outline, None);
    }

    #[test]
    fn knobs_come_from_the_command_line() {
        let args: Vec<String> = ["pieced", "--scenario", "perf", "--knobs", "blobs=off"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(PerfKnobs::from_args(&args).unwrap().blobs, Some(false));
        assert!(PerfKnobs::from_args(&args[..3]).is_none());
    }

    #[test]
    fn particle_cap_lands_before_startup() {
        let mut app = App::new();
        app.init_resource::<Tuning>()
            .insert_resource(PerfKnobs::parse("particles=64"))
            .add_plugins(PerfKnobsPlugin);
        assert_eq!(app.world().resource::<Tuning>().feedback.max_particles, 64);
    }
}
