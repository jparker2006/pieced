//! Pieced: a solo first-person shooter with Fortnite-style building.
//!
//! Gameplay lives in headless-safe plugins ([`app::SimPlugins`]) driven only by
//! [`shared::PlayerIntent`]. Presentation (rendering, audio, HUD, devices) lives in
//! [`app::ClientPlugins`] and only runs in the full game.

// Bevy system signatures routinely trip these two lints.
#![allow(clippy::type_complexity, clippy::too_many_arguments)]

pub mod app;
pub mod arena;
pub mod audio;
pub mod building;
pub mod combat;
pub mod dummy;
pub mod far;
pub mod fx;
pub mod hud;
pub mod input;
pub mod knight;
pub mod look;
pub mod menu;
pub mod models;
pub mod movement;
pub mod native;
pub mod palette;
pub mod perf_knobs;
pub mod player;
pub mod render;
pub mod rng;
pub mod scenario;
pub mod shared;
pub mod sim;
pub mod telemetry;
pub mod tuning;
pub mod viewmodel;
