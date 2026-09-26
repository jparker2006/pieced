//! Quality presets and perf-knob overrides, resolved once per frame into one
//! [`LookSettings`] resource that every look system reads. Also owns the
//! camera-level color pipeline: `Tonemapping::None` with no debanding on both
//! 3D cameras, and one MSAA setting shared by both.

use crate::{
    perf_knobs::PerfKnobs,
    render::{MainCamera, QualityPreset},
    tuning::Tuning,
    viewmodel::ViewmodelCamera,
};
use bevy::{
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    prelude::*,
};

/// How ink outlines are drawn (`--knobs outline=mod|hull|off`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum OutlineBackend {
    /// `bevy_mod_outline`'s vertex extrusion, drawn in its own passes after the
    /// main pass. Only available when the game was launched with
    /// `--knobs outline=mod` (its plugin is added at startup only then).
    Mod,
    /// Our inverted hull: an inflated copy of the mesh drawn front-face-culled
    /// with [`super::InkMaterial`] inside the main pass. The default.
    #[default]
    Hull,
    /// No outlines.
    Off,
}

impl OutlineBackend {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "mod" => Some(Self::Mod),
            "hull" | "on" => Some(Self::Hull),
            "off" | "0" | "false" => Some(Self::Off),
            _ => None,
        }
    }
}

/// What a quality preset changes in the M2 pipeline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PresetLook {
    pub outline: OutlineBackend,
    /// MSAA sample count for both 3D cameras (1 = off).
    pub msaa_samples: u32,
    /// Render-scale budget: multiplies `GraphicsTuning::max_megapixels`, the cap
    /// on 3D render pixels (Battery keeps the configured cap).
    pub pixel_budget: f32,
    /// Extra grass tufts on the arena floor.
    pub dense_grass: bool,
}

pub fn preset_look(preset: QualityPreset) -> PresetLook {
    match preset {
        QualityPreset::Battery => PresetLook {
            outline: OutlineBackend::Hull,
            msaa_samples: 4,
            pixel_budget: 1.0,
            dense_grass: false,
        },
        QualityPreset::PluggedIn => PresetLook {
            outline: OutlineBackend::Hull,
            msaa_samples: 4,
            pixel_budget: 1.5,
            dense_grass: true,
        },
    }
}

/// `Msaa` for a sample count (anything unexpected means 4×).
pub fn msaa_for(samples: u32) -> Msaa {
    match samples {
        0 | 1 => Msaa::Off,
        2 => Msaa::Sample2,
        8 => Msaa::Sample8,
        _ => Msaa::Sample4,
    }
}

/// Present when `bevy_mod_outline`'s plugin was added (only with
/// `--knobs outline=mod`).
#[derive(Resource, Debug, Default)]
pub struct ModOutlineAvailable;

/// The effective look this frame: the preset, overridden by perf knobs.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct LookSettings {
    pub outline: OutlineBackend,
    pub msaa: Msaa,
    /// Normally equal to `msaa`; only the `vmmsaa` knob splits them.
    pub viewmodel_msaa: Msaa,
    pub far: bool,
    pub halos: bool,
    pub blobs: bool,
    /// Sky rotation on/off (read by the sky slice).
    pub sky_rotation: bool,
    pub dense_grass: bool,
}

impl Default for LookSettings {
    fn default() -> Self {
        resolve_look(QualityPreset::Battery, None, false)
    }
}

/// Preset + knobs → settings. `mod_available` says whether `bevy_mod_outline`
/// is running; without it a `Mod` request falls back to the hull.
pub fn resolve_look(
    preset: QualityPreset,
    knobs: Option<&PerfKnobs>,
    mod_available: bool,
) -> LookSettings {
    let look = preset_look(preset);
    let knob = |f: fn(&PerfKnobs) -> Option<bool>| knobs.and_then(f).unwrap_or(true);
    let mut outline = knobs.and_then(|k| k.outline).unwrap_or(look.outline);
    if outline == OutlineBackend::Mod && !mod_available {
        outline = OutlineBackend::Hull;
    }
    let msaa = msaa_for(knobs.and_then(|k| k.msaa).unwrap_or(look.msaa_samples));
    let viewmodel_msaa = knobs
        .and_then(|k| k.viewmodel_msaa)
        .map(msaa_for)
        .unwrap_or(msaa);
    LookSettings {
        outline,
        msaa,
        viewmodel_msaa,
        far: knob(|k| k.far),
        halos: knob(|k| k.halos),
        blobs: knob(|k| k.blobs),
        sky_rotation: knob(|k| k.skyrot),
        dense_grass: look.dense_grass,
    }
}

pub(crate) fn update_look_settings(
    tuning: Res<Tuning>,
    knobs: Option<Res<PerfKnobs>>,
    available: Option<Res<ModOutlineAvailable>>,
    mut settings: ResMut<LookSettings>,
) {
    let wanted = resolve_look(
        tuning.graphics.preset,
        knobs.as_deref(),
        available.is_some(),
    );
    settings.set_if_neq(wanted);
}

/// Keeps both 3D cameras on the settings' MSAA (only writes on change, so it
/// never forces a pipeline re-specialization by itself).
pub(crate) fn apply_camera_msaa(
    settings: Res<LookSettings>,
    mut cameras: Query<(&mut Msaa, Has<MainCamera>, Has<ViewmodelCamera>)>,
) {
    for (mut msaa, main, viewmodel) in &mut cameras {
        if main {
            msaa.set_if_neq(settings.msaa);
        } else if viewmodel {
            msaa.set_if_neq(settings.viewmodel_msaa);
        }
    }
}

/// Palette colors land on screen as authored: no tone curve and no dither on
/// the world or viewmodel camera. Runs when either camera is spawned, before its
/// first frame is extracted.
pub(crate) fn dress_look_camera(
    add: On<Add, (MainCamera, ViewmodelCamera)>,
    mut commands: Commands,
    settings: Option<Res<LookSettings>>,
    main: Query<(), With<MainCamera>>,
) {
    let settings = settings.map(|s| s.clone()).unwrap_or_default();
    let msaa = if main.contains(add.entity) {
        settings.msaa
    } else {
        settings.viewmodel_msaa
    };
    commands
        .entity(add.entity)
        .insert((Tonemapping::None, DebandDither::Disabled, msaa));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_trade_quality_for_headroom() {
        let battery = preset_look(QualityPreset::Battery);
        let plugged = preset_look(QualityPreset::PluggedIn);
        assert_eq!(battery.outline, OutlineBackend::Hull);
        assert_eq!(battery.msaa_samples, plugged.msaa_samples);
        assert!(battery.pixel_budget < plugged.pixel_budget);
        assert!(!battery.dense_grass && plugged.dense_grass);
        assert_eq!(QualityPreset::default(), QualityPreset::Battery);
    }

    #[test]
    fn knobs_override_the_preset() {
        let knobs = PerfKnobs::parse("outline=off,far=off,halos=off,blobs=off,skyrot=off,msaa=2");
        let s = resolve_look(QualityPreset::Battery, Some(&knobs), false);
        assert_eq!(s.outline, OutlineBackend::Off);
        assert_eq!(s.msaa, Msaa::Sample2);
        assert_eq!(s.viewmodel_msaa, Msaa::Sample2, "both cameras share MSAA");
        assert!(!s.far && !s.halos && !s.blobs && !s.sky_rotation);

        let none = resolve_look(QualityPreset::Battery, None, false);
        assert_eq!(none.outline, OutlineBackend::Hull);
        assert_eq!(none.msaa, Msaa::Sample4);
        assert!(none.far && none.halos && none.blobs && none.sky_rotation);
    }

    #[test]
    fn mod_outlines_need_their_plugin() {
        let knobs = PerfKnobs::parse("outline=mod");
        let without = resolve_look(QualityPreset::Battery, Some(&knobs), false);
        assert_eq!(without.outline, OutlineBackend::Hull);
        let with = resolve_look(QualityPreset::Battery, Some(&knobs), true);
        assert_eq!(with.outline, OutlineBackend::Mod);
    }

    #[test]
    fn viewmodel_msaa_knob_splits_the_cameras() {
        let knobs = PerfKnobs::parse("vmmsaa=1");
        let s = resolve_look(QualityPreset::Battery, Some(&knobs), false);
        assert_eq!(s.msaa, Msaa::Sample4);
        assert_eq!(s.viewmodel_msaa, Msaa::Off);
    }
}
