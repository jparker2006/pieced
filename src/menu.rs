//! Slice F — pause menu, settings, the dev tuning panel and settings persistence.
//!
//! Milestone 2: the pause menu wears the PIECED logo and cartoon buttons
//! (`pause.rs`, target T12).
//!
//! - The pause menu ([`AppState::Paused`], cursor free) offers Resume, Settings and
//!   Quit. Settings edit the live [`Tuning`]; every change applies immediately
//!   (window mode and vsync included).
//! - F4 opens the egui tuning panel with every `Tuning` field, grouped by section,
//!   and pauses so the cursor is free. F4 or Esc closes it.
//! - The whole `Tuning` autosaves to [`Tuning::settings_path`] about a second after
//!   changes stop. Scenario runs never save.
//!
//! Milestone 3, chunk 5 (D92, D93):
//!
//! - The **main menu** ([`AppState::Menu`], `main_menu.rs`): the PIECED logo
//!   over the live island with the camera orbiting; Waves (with the best
//!   run), Practice, Settings and Quit. Waves and Practice start in place
//!   (`waves::modes`). The pause menu's Quit becomes "Quit to menu".
//! - The **Controls** page in Settings (`controls.rs`): every action's
//!   binding; click one, press a key or click; Esc cancels; a conflict swaps;
//!   Reset to defaults. The input adapter does the capture (`input.rs` stays
//!   the only device reader).

mod controls;
pub mod main_menu;
mod panel;
mod pause;

pub use controls::{ControlsButton, ControlsStatus, KeyChip};
pub use main_menu::{HeroCamera, LastPlay, MainMenuButton, hero_eye};
pub use pause::PauseButton;

use crate::{render::QualityPreset, scenario::ScenarioRun, shared::AppState, tuning::Tuning};
use bevy::{
    prelude::*,
    window::{MonitorSelection, PresentMode, PrimaryWindow, WindowMode},
};

/// Seconds after the last change before settings are written.
pub const AUTOSAVE_DELAY: f64 = 1.0;

/// Which menu page is showing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MenuPage {
    /// The pause menu's buttons, or (on the main menu) the title screen.
    #[default]
    Main,
    Settings,
    /// Settings → Controls (D93).
    Controls,
}

/// What the menus show. Entering [`AppState::Paused`] opens the menu; entering
/// [`AppState::Menu`] opens the title screen; entering [`AppState::Playing`]
/// closes everything. Scenarios may set these directly to capture the menus
/// without pausing.
#[derive(Resource, Debug, Clone, Default)]
pub struct MenuState {
    pub menu_open: bool,
    pub page: MenuPage,
    pub panel_open: bool,
    /// Closing the tuning panel returns to play (it was opened from play).
    resume_on_close: bool,
    /// The menus belong to the main menu (the title screen is their Main
    /// page), not the pause menu.
    pub title: bool,
}

impl MenuState {
    pub fn menu_visible(&self) -> bool {
        self.menu_open && !self.panel_open
    }

    /// The title screen is showing.
    pub fn title_visible(&self) -> bool {
        self.menu_visible() && self.title && self.page == MenuPage::Main
    }

    /// The pause menu's layer (its buttons, or Settings and Controls over
    /// either menu) is showing.
    pub fn pause_layer_visible(&self) -> bool {
        self.menu_visible() && !(self.title && self.page == MenuPage::Main)
    }

    /// One page back: Controls → Settings → Main.
    pub fn back(&mut self) {
        self.page = match self.page {
            MenuPage::Controls => MenuPage::Settings,
            MenuPage::Settings | MenuPage::Main => MenuPage::Main,
        };
    }

    /// Closes the tuning panel. Returns true when play should resume (the panel
    /// was opened from play rather than from the pause menu).
    pub fn close_panel(&mut self) -> bool {
        let was_open = std::mem::take(&mut self.panel_open);
        was_open && std::mem::take(&mut self.resume_on_close)
    }
}

// ---------------------------------------------------------------------------
// Settings model (pure)
// ---------------------------------------------------------------------------

/// A player-facing setting on the pause menu's Settings page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Setting {
    Sensitivity,
    AdsMultiplier,
    BuildMultiplier,
    Acceleration,
    AimFriction,
    Fov,
    Bloom,
    DamageNumbers,
    CameraShake,
    Volume,
    /// The score's level (M4).
    Music,
    /// The effects' level (M4).
    Effects,
    Mute,
    Quality,
    WindowMode,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettingKind {
    /// Displayed value range and step.
    Slider {
        min: f32,
        max: f32,
        step: f32,
    },
    Toggle,
    Choice(&'static [&'static str]),
}

/// Raw sensitivity is radians per trackpad unit; the menu shows it ×1000.
const SENSITIVITY_DISPLAY: f32 = 1000.0;

impl Setting {
    pub const ALL: [Setting; 15] = [
        Setting::Sensitivity,
        Setting::AdsMultiplier,
        Setting::BuildMultiplier,
        Setting::Acceleration,
        Setting::AimFriction,
        Setting::Fov,
        Setting::Bloom,
        Setting::DamageNumbers,
        Setting::CameraShake,
        Setting::Volume,
        Setting::Music,
        Setting::Effects,
        Setting::Mute,
        Setting::Quality,
        Setting::WindowMode,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Setting::Sensitivity => "Sensitivity",
            Setting::AdsMultiplier => "ADS multiplier (Shift)",
            Setting::BuildMultiplier => "Build multiplier",
            Setting::Acceleration => "Acceleration curve",
            Setting::AimFriction => "Aim friction (rifle)",
            Setting::Fov => "Field of view",
            Setting::Bloom => "Crosshair bloom",
            Setting::DamageNumbers => "Damage numbers",
            Setting::CameraShake => "Camera shake",
            Setting::Volume => "Master volume",
            Setting::Music => "Music",
            Setting::Effects => "Effects",
            Setting::Mute => "Mute",
            Setting::Quality => "Quality",
            Setting::WindowMode => "Window",
        }
    }

    pub fn kind(self) -> SettingKind {
        match self {
            Setting::Sensitivity => SettingKind::Slider {
                min: 0.5,
                max: 10.0,
                step: 0.05,
            },
            Setting::AdsMultiplier => SettingKind::Slider {
                min: 0.2,
                max: 1.5,
                step: 0.05,
            },
            Setting::BuildMultiplier => SettingKind::Slider {
                min: 0.3,
                max: 2.0,
                step: 0.05,
            },
            Setting::Fov => SettingKind::Slider {
                min: 60.0,
                max: 90.0,
                step: 1.0,
            },
            Setting::CameraShake | Setting::Volume | Setting::Music | Setting::Effects => {
                SettingKind::Slider {
                    min: 0.0,
                    max: 1.0,
                    step: 0.05,
                }
            }
            Setting::Acceleration
            | Setting::AimFriction
            | Setting::Bloom
            | Setting::DamageNumbers
            | Setting::Mute => SettingKind::Toggle,
            Setting::Quality => SettingKind::Choice(&["Battery", "Plugged in"]),
            Setting::WindowMode => SettingKind::Choice(&["Fullscreen", "Windowed"]),
        }
    }

    /// The displayed value: slider position, 0/1 for toggles, or the choice index.
    pub fn get(self, t: &Tuning) -> f32 {
        let flag = |b: bool| if b { 1.0 } else { 0.0 };
        match self {
            Setting::Sensitivity => t.look.sensitivity * SENSITIVITY_DISPLAY,
            Setting::AdsMultiplier => t.look.ads_multiplier,
            Setting::BuildMultiplier => t.look.build_multiplier,
            Setting::Acceleration => flag(t.look.accel_enabled),
            Setting::AimFriction => flag(t.combat.aim_friction),
            Setting::Fov => t.look.fov_deg,
            Setting::Bloom => flag(t.hud.show_bloom),
            Setting::DamageNumbers => flag(t.hud.damage_numbers),
            Setting::CameraShake => t.feedback.camera_shake,
            Setting::Volume => t.audio.master_volume,
            Setting::Music => t.audio.music_volume,
            Setting::Effects => t.audio.effects_volume,
            Setting::Mute => flag(t.audio.muted),
            Setting::Quality => match t.graphics.preset {
                QualityPreset::Battery => 0.0,
                QualityPreset::PluggedIn => 1.0,
            },
            Setting::WindowMode => flag(!t.graphics.fullscreen),
        }
    }

    /// Sets the displayed value, clamped to range and snapped to the step.
    pub fn set(self, t: &mut Tuning, value: f32) {
        let value = match self.kind() {
            SettingKind::Slider { min, max, step } => {
                let v = value.clamp(min, max);
                if step > 0.0 {
                    (min + ((v - min) / step).round() * step).clamp(min, max)
                } else {
                    v
                }
            }
            SettingKind::Toggle => {
                if value >= 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
            SettingKind::Choice(options) => value.round().clamp(0.0, (options.len() - 1) as f32),
        };
        let on = value >= 0.5;
        match self {
            Setting::Sensitivity => t.look.sensitivity = value / SENSITIVITY_DISPLAY,
            Setting::AdsMultiplier => t.look.ads_multiplier = value,
            Setting::BuildMultiplier => t.look.build_multiplier = value,
            Setting::Acceleration => t.look.accel_enabled = on,
            Setting::AimFriction => t.combat.aim_friction = on,
            Setting::Fov => t.look.fov_deg = value,
            Setting::Bloom => t.hud.show_bloom = on,
            Setting::DamageNumbers => t.hud.damage_numbers = on,
            Setting::CameraShake => t.feedback.camera_shake = value,
            Setting::Volume => t.audio.master_volume = value,
            Setting::Music => t.audio.music_volume = value,
            Setting::Effects => t.audio.effects_volume = value,
            Setting::Mute => t.audio.muted = on,
            Setting::Quality => {
                t.graphics.preset = if on {
                    QualityPreset::PluggedIn
                } else {
                    QualityPreset::Battery
                }
            }
            Setting::WindowMode => t.graphics.fullscreen = !on,
        }
    }

    /// Slider position in 0..=1.
    pub fn fraction(self, t: &Tuning) -> f32 {
        match self.kind() {
            SettingKind::Slider { min, max, .. } if max > min => {
                ((self.get(t) - min) / (max - min)).clamp(0.0, 1.0)
            }
            _ => self.get(t).clamp(0.0, 1.0),
        }
    }

    /// Sets from a slider position in 0..=1.
    pub fn set_fraction(self, t: &mut Tuning, fraction: f32) {
        if let SettingKind::Slider { min, max, .. } = self.kind() {
            self.set(t, min + (max - min) * fraction.clamp(0.0, 1.0));
        }
    }

    /// The value as the menu prints it.
    pub fn display(self, t: &Tuning) -> String {
        let v = self.get(t);
        match self {
            Setting::Sensitivity => format!("{v:.2}"),
            Setting::AdsMultiplier | Setting::BuildMultiplier => format!("{v:.2}x"),
            Setting::Fov => format!("{v:.0}"),
            Setting::CameraShake | Setting::Volume | Setting::Music | Setting::Effects => {
                format!("{:.0}%", v * 100.0)
            }
            _ => match self.kind() {
                SettingKind::Toggle => if v >= 0.5 { "On" } else { "Off" }.into(),
                SettingKind::Choice(options) => options[v.round() as usize].into(),
                SettingKind::Slider { .. } => format!("{v:.2}"),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Autosave (pure debounce + system)
// ---------------------------------------------------------------------------

/// Fires once, `delay` seconds after the latest [`Debounce::touch`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Debounce {
    pending_since: Option<f64>,
}

impl Debounce {
    pub fn touch(&mut self, now: f64) {
        self.pending_since = Some(now);
    }

    pub fn is_pending(&self) -> bool {
        self.pending_since.is_some()
    }

    /// True once when the quiet period has passed; clears the pending change.
    pub fn fire(&mut self, now: f64, delay: f64) -> bool {
        match self.pending_since {
            Some(t) if now - t >= delay => {
                self.pending_since = None;
                true
            }
            _ => false,
        }
    }

    /// Clears and reports any pending change (for flushing on exit).
    pub fn take(&mut self) -> bool {
        self.pending_since.take().is_some()
    }
}

#[derive(Resource, Debug)]
struct Autosave {
    debounce: Debounce,
    last_saved: Tuning,
    /// Set by the tuning panel's "Save now".
    save_now: bool,
}

fn init_autosave(mut commands: Commands, tuning: Res<Tuning>) {
    commands.insert_resource(Autosave {
        debounce: Debounce::default(),
        last_saved: tuning.clone(),
        save_now: false,
    });
}

fn write_settings(tuning: Tuning, background: bool) {
    let path = Tuning::settings_path();
    let job = move || {
        if let Err(e) = tuning.save(&path) {
            eprintln!("settings not saved to {}: {e}", path.display());
        }
    };
    if background {
        std::thread::spawn(job);
    } else {
        job();
    }
}

fn autosave(
    time: Res<Time<Real>>,
    tuning: Res<Tuning>,
    scenario: Option<Res<ScenarioRun>>,
    mut exits: MessageReader<AppExit>,
    autosave: Option<ResMut<Autosave>>,
) {
    let Some(mut autosave) = autosave else {
        return;
    };
    if scenario.is_some() {
        // Scenarios tweak tuning for their scripts; never persist that.
        return;
    }
    let now = time.elapsed_secs_f64();
    if tuning.is_changed() && *tuning != autosave.last_saved {
        autosave.debounce.touch(now);
    }
    let exiting = exits.read().count() > 0;
    let due = autosave.debounce.fire(now, AUTOSAVE_DELAY) || std::mem::take(&mut autosave.save_now);
    let flush = exiting && autosave.debounce.take();
    if (due || flush) && *tuning != autosave.last_saved {
        autosave.last_saved = tuning.clone();
        write_settings(tuning.clone(), !exiting);
    }
}

// ---------------------------------------------------------------------------
// Live window settings
// ---------------------------------------------------------------------------

fn apply_window_settings(
    tuning: Res<Tuning>,
    mut last: Local<Option<(bool, bool)>>,
    window: Option<Single<&mut Window, With<PrimaryWindow>>>,
) {
    let current = (tuning.graphics.fullscreen, tuning.graphics.vsync);
    let Some(previous) = last.replace(current) else {
        // The window was created from these settings (or `--windowed`).
        return;
    };
    let Some(mut window) = window else {
        return;
    };
    if previous.0 != current.0 {
        window.mode = if current.0 {
            WindowMode::BorderlessFullscreen(MonitorSelection::Primary)
        } else {
            WindowMode::Windowed
        };
    }
    if previous.1 != current.1 {
        window.present_mode = if current.1 {
            PresentMode::Fifo
        } else {
            PresentMode::AutoNoVsync
        };
    }
}

// ---------------------------------------------------------------------------
// State wiring
// ---------------------------------------------------------------------------

fn open_menu(mut menu: ResMut<MenuState>) {
    menu.menu_open = true;
    menu.title = false;
    menu.page = MenuPage::Main;
}

/// Entering the main menu shows the title screen.
fn open_title(mut menu: ResMut<MenuState>) {
    menu.menu_open = true;
    menu.title = true;
    menu.close_panel();
    menu.page = MenuPage::Main;
}

fn close_menus(mut menu: ResMut<MenuState>) {
    menu.menu_open = false;
    menu.title = false;
    menu.close_panel();
    menu.page = MenuPage::Main;
}

/// F4 opens or closes the tuning panel. Opening pauses so the cursor is free.
fn toggle_panel(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<State<AppState>>,
    mut next: ResMut<NextState<AppState>>,
    mut menu: ResMut<MenuState>,
) {
    if !keys.just_pressed(KeyCode::F4) {
        return;
    }
    if menu.panel_open {
        if menu.close_panel() && *state.get() == AppState::Paused {
            next.set(AppState::Playing);
        }
    } else {
        menu.panel_open = true;
        menu.resume_on_close = *state.get() == AppState::Playing;
        if menu.resume_on_close {
            next.set(AppState::Paused);
        }
    }
}

/// The whole menu layer of the game: [`MenuUiPlugin`] plus settings
/// persistence, live window settings and the F4 tuning panel.
pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MenuUiPlugin)
            .add_systems(Startup, init_autosave)
            .add_systems(Update, (toggle_panel, apply_window_settings))
            .add_systems(Last, autosave);
        panel::build(app);
    }
}

/// The menus themselves (native Bevy UI, headless-safe): the main menu, the
/// pause menu, Settings and Controls, and the main menu's orbit camera and
/// Play timing. Never writes the settings file.
pub struct MenuUiPlugin;

impl Plugin for MenuUiPlugin {
    fn build(&self, app: &mut App) {
        // The font, the logo and the crystal (shared with the HUD).
        crate::hud::art::install(app);
        app.init_resource::<MenuState>()
            .init_resource::<crate::input::BindingCapture>()
            .add_systems(OnEnter(AppState::Paused), open_menu)
            .add_systems(OnEnter(AppState::Menu), open_title)
            .add_systems(OnEnter(AppState::Playing), close_menus);
        pause::build(app);
        controls::build(app);
        main_menu::build(app);
    }
}
