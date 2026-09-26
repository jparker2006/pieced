//! The HUD's and menus' art, compiled into the binary (the game runs from
//! outside the repo, like the models and shaders):
//!
//! - the cartoon font (`assets/fonts/`, the one open-license file), loaded into
//!   Bevy's *default* font handle, so every UI text in the game uses it without
//!   naming it;
//! - the Blender-rendered icons and the PIECED logo (`assets/ui/`, made by
//!   `art/blender/assets/icons.py` and `logo.py`).
//!
//! Everything is decoded once while the plugins build, before the first frame:
//! nothing here is allocated per frame or per hit.

use bevy::{
    asset::RenderAssetUsages,
    image::{CompressedImageFormats, ImageSampler, ImageType},
    prelude::*,
};

/// The HUD font's file name under `assets/fonts/`.
pub const FONT_FILE: &str = "LilitaOne-Regular.ttf";
/// Its family name (what `FontSource::Family` would use).
pub const FONT_FAMILY: &str = "Lilita One";
pub const FONT_BYTES: &[u8] = include_bytes!("../../assets/fonts/LilitaOne-Regular.ttf");

macro_rules! ui_images {
    ($($path:literal),* $(,)?) => {
        &[$(($path, include_bytes!(concat!("../../assets/ui/", $path)))),*]
    };
}

/// Every file under `assets/ui/`, by its path there (a test keeps this and the
/// folder in step).
pub const UI_IMAGES: &[(&str, &[u8])] = ui_images![
    "icons/rifle.png",
    "icons/pump.png",
    "icons/wall_brick.png",
    "icons/ramp_plank.png",
    "icons/floor_plank.png",
    "icons/crystal_blue.png",
    "icons/crystal_violet.png",
    "icons/heart.png",
    "logo.png",
];

/// Handles to the decoded UI images (default handles in apps without an
/// image store, e.g. headless tests).
#[derive(Resource, Debug, Clone, Default)]
pub struct UiArt {
    /// Hotbar icons in slot order: rifle, pump, wall, ramp, floor.
    pub slots: [Handle<Image>; 5],
    pub crystal_blue: Handle<Image>,
    pub crystal_violet: Handle<Image>,
    pub heart: Handle<Image>,
    pub logo: Handle<Image>,
    /// The logo's size in pixels (its aspect ratio sizes its nodes).
    pub logo_size: UVec2,
}

/// The embedded bytes of a UI image.
pub fn ui_image_bytes(path: &str) -> Option<&'static [u8]> {
    UI_IMAGES.iter().find(|(p, _)| *p == path).map(|(_, b)| *b)
}

/// Decodes an embedded PNG (sRGB, linear filtering).
pub fn decode_png(bytes: &[u8]) -> Result<Image, String> {
    Image::from_buffer(
        bytes,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::linear(),
        RenderAssetUsages::default(),
    )
    .map_err(|e| e.to_string())
}

/// Loads the font and the images once; later calls do nothing.
pub fn install(app: &mut App) {
    if app.world().contains_resource::<UiArt>() {
        return;
    }
    let world = app.world_mut();
    if let Some(mut fonts) = world.get_resource_mut::<Assets<Font>>() {
        // Replacing the default font before any text is laid out: every text
        // that doesn't name a font (all of the HUD and the menus) uses it.
        if let Err(e) = fonts.insert(AssetId::default(), Font::from_bytes(FONT_BYTES.to_vec())) {
            warn!("HUD font not installed: {e}");
        }
    }
    let art = match world.get_resource_mut::<Assets<Image>>() {
        Some(mut images) => {
            let mut load = |path: &str| -> (Handle<Image>, UVec2) {
                let bytes = ui_image_bytes(path).expect("every UI image is embedded");
                match decode_png(bytes) {
                    Ok(image) => {
                        let size = image.size();
                        (images.add(image), size)
                    }
                    Err(e) => {
                        warn!("UI image {path} not decoded: {e}");
                        (Handle::default(), UVec2::ONE)
                    }
                }
            };
            let (logo, logo_size) = load("logo.png");
            UiArt {
                slots: [
                    load("icons/rifle.png").0,
                    load("icons/pump.png").0,
                    load("icons/wall_brick.png").0,
                    load("icons/ramp_plank.png").0,
                    load("icons/floor_plank.png").0,
                ],
                crystal_blue: load("icons/crystal_blue.png").0,
                crystal_violet: load("icons/crystal_violet.png").0,
                heart: load("icons/heart.png").0,
                logo,
                logo_size,
            }
        }
        None => UiArt {
            logo_size: UVec2::new(1200, 520),
            ..default()
        },
    };
    world.insert_resource(art);
}
