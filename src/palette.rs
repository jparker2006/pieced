//! The art direction's palette (docs/SPEC.md → Art direction). Every slice takes
//! colors from here so the world reads as one design. Values are sRGB.

use bevy::color::Color;

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::srgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
}

// Sky and atmosphere: warm late afternoon.
pub const SKY_ZENITH: Color = rgb(0x5E, 0x9C, 0xE0);
pub const SKY_HORIZON: Color = rgb(0xF6, 0xD3, 0x9C);
/// Pale clear blue between the gold horizon and the zenith (avoids a muddy blend).
pub const SKY_MID: Color = rgb(0xA8, 0xCC, 0xEE);
pub const SUN_DISC: Color = rgb(0xFF, 0xF1, 0xC8);
pub const SUNLIGHT: Color = rgb(0xFF, 0xE6, 0xBF);
pub const FOG: Color = rgb(0xEE, 0xD2, 0xA8);
pub const AMBIENT: Color = rgb(0xC9, 0xD8, 0xF0);
/// Cool skylight fill from above (no shadows): keeps shadowed faces shaped.
pub const SKYLIGHT: Color = rgb(0xB8, 0xCF, 0xF2);
/// Warm-neutral bounce used as the ambient term.
pub const BOUNCE: Color = rgb(0xE6, 0xE0, 0xD8);

// Muted mid-tone world.
pub const GRASS: Color = rgb(0x8C, 0x9E, 0x62);
pub const GRASS_DARK: Color = rgb(0x6F, 0x84, 0x4E);
pub const OLIVE: Color = rgb(0x7A, 0x7F, 0x4C);
pub const SAND: Color = rgb(0xD9, 0xC2, 0x93);
pub const ROCK: Color = rgb(0x7E, 0x82, 0x8C);
pub const ROCK_DARK: Color = rgb(0x5F, 0x63, 0x6E);
pub const FOLIAGE: Color = rgb(0x5E, 0x7D, 0x4A);
pub const TRUNK: Color = rgb(0x6B, 0x4E, 0x38);
pub const FOLIAGE_LIGHT: Color = rgb(0x7D, 0x98, 0x58);
pub const SAGE: Color = rgb(0xA3, 0xAE, 0x86);
pub const DIRT: Color = rgb(0xB4, 0xA2, 0x7E);
pub const ROCK_LIGHT: Color = rgb(0x9C, 0x9E, 0xA6);
pub const ROCK_WARM: Color = rgb(0x8F, 0x87, 0x7D);
pub const MESA: Color = rgb(0xC9, 0x8E, 0x66);
pub const MESA_DARK: Color = rgb(0xA8, 0x70, 0x52);
pub const CLOUD: Color = rgb(0xFF, 0xF8, 0xEE);
pub const CLOUD_SHADE: Color = rgb(0xF0, 0xD4, 0xCB);

// Player-built pieces: warm crafted wood, clearly brighter than the ground.
pub const WOOD: Color = rgb(0xE0, 0xA9, 0x62);
pub const WOOD_LIGHT: Color = rgb(0xF0, 0xC4, 0x84);
pub const WOOD_DARK: Color = rgb(0xB9, 0x7D, 0x3F);
pub const WOOD_TRIM: Color = rgb(0x9A, 0x64, 0x32);
pub const GHOST_VALID: Color = Color::srgba(0.55, 0.85, 1.0, 0.35);
pub const GHOST_INVALID: Color = Color::srgba(1.0, 0.35, 0.3, 0.35);

// The target: one saturated hue used nowhere else, plus its rim light.
pub const TARGET: Color = rgb(0xFF, 0x4F, 0x7B);
pub const TARGET_DARK: Color = rgb(0xC2, 0x2E, 0x5A);
pub const TARGET_RIM: Color = rgb(0xFF, 0xC2, 0xD4);
/// Bullseye rings on the dummy's target plates.
pub const TARGET_LIGHT: Color = rgb(0xFF, 0xE4, 0xEC);

// Guns (viewmodel).
pub const GUN_METAL: Color = rgb(0x3C, 0x41, 0x4A);
pub const GUN_POLYMER: Color = rgb(0x2B, 0x2E, 0x33);
pub const GUN_ACCENT: Color = rgb(0xD8, 0x9A, 0x4E);

// Feedback and UI.
pub const SHIELD: Color = rgb(0x59, 0xB8, 0xFF);
pub const HEALTH: Color = rgb(0x7B, 0xE0, 0x6A);
pub const HIT_WHITE: Color = rgb(0xFF, 0xFF, 0xFF);
pub const HEADSHOT: Color = rgb(0xFF, 0xD2, 0x3F);
pub const MUZZLE: Color = rgb(0xFF, 0xD9, 0x8A);
pub const UI_TEXT: Color = rgb(0xF7, 0xF3, 0xEA);
pub const UI_SHADOW: Color = Color::srgba(0.05, 0.05, 0.08, 0.6);
pub const UI_PANEL: Color = Color::srgba(0.08, 0.09, 0.12, 0.55);
