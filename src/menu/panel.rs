//! The F4 dev tuning panel (bevy_egui). It edits every [`Tuning`] field through
//! its serialized form, so fields other slices add later appear automatically,
//! grouped by section, with reset-to-default buttons per section and group.

use super::{Autosave, MenuState};
use crate::{shared::AppState, tuning::Tuning};
use bevy::prelude::*;
use bevy_egui::{
    EguiContexts, EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass, PrimaryEguiContext, egui,
};
use serde_json::{Number, Value};

pub(super) fn build(app: &mut App) {
    if !app.is_plugin_added::<EguiPlugin>() {
        app.add_plugins(EguiPlugin::default());
    }
    // Egui draws on the native-resolution UI camera, not the offscreen world camera
    // (which would be the automatic choice).
    app.world_mut()
        .resource_mut::<EguiGlobalSettings>()
        .auto_create_primary_context = false;
    app.add_observer(attach_egui_context)
        .add_systems(EguiPrimaryContextPass, tuning_panel);
}

fn attach_egui_context(add: On<Add, IsDefaultUiCamera>, mut commands: Commands) {
    commands.entity(add.entity).insert(PrimaryEguiContext);
}

/// Display order of the top-level sections (others follow alphabetically).
const SECTION_ORDER: [&str; 9] = [
    "look", "movement", "combat", "building", "dummy", "feedback", "hud", "audio", "graphics",
];

/// Known string enums, by JSON path.
fn enum_variants(path: &str) -> Option<&'static [&'static str]> {
    match path {
        "graphics.preset" => Some(&["Battery", "PluggedIn"]),
        _ => None,
    }
}

fn pretty(key: &str) -> String {
    let mut s = key.replace('_', " ");
    if let Some(first) = s.get(0..1) {
        let upper = first.to_uppercase();
        s.replace_range(0..1, &upper);
    }
    s
}

fn tuning_panel(
    mut contexts: EguiContexts,
    state: Res<State<AppState>>,
    mut next: ResMut<NextState<AppState>>,
    mut menu: ResMut<MenuState>,
    mut tuning: ResMut<Tuning>,
    mut autosave: Option<ResMut<Autosave>>,
    mut styled: Local<bool>,
) -> Result {
    if !menu.panel_open {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    if !*styled {
        *styled = true;
        let mut visuals = egui::Visuals::dark();
        visuals.window_corner_radius = egui::CornerRadius::same(10);
        visuals.window_fill = egui::Color32::from_rgba_unmultiplied(18, 20, 26, 242);
        visuals.panel_fill = visuals.window_fill;
        ctx.set_visuals(visuals);
    }

    let original = serde_json::to_value(&*tuning)?;
    let defaults = serde_json::to_value(Tuning::default())?;
    let mut edited = original.clone();
    let mut open = true;
    let mut save_now = false;
    let screen = ctx.content_rect();

    egui::Window::new("Tuning")
        .open(&mut open)
        .anchor(egui::Align2::RIGHT_TOP, [-12.0, 12.0])
        .default_width(390.0)
        .max_height(screen.height() - 24.0)
        .resizable(false)
        .collapsible(false)
        .vscroll(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Reset all").clicked() {
                    edited = defaults.clone();
                }
                if ui.button("Save now").clicked() {
                    save_now = true;
                }
                ui.weak("F4 / Esc to close");
            });
            ui.weak("Edits apply live and autosave after 1 s.");
            ui.separator();
            let Value::Object(sections) = &mut edited else {
                return;
            };
            let mut keys: Vec<String> = sections.keys().cloned().collect();
            keys.sort_by_key(|k| {
                (
                    SECTION_ORDER
                        .iter()
                        .position(|s| s == k)
                        .unwrap_or(SECTION_ORDER.len()),
                    k.clone(),
                )
            });
            for key in keys {
                let default = defaults.get(&key).cloned().unwrap_or(Value::Null);
                if let Some(value) = sections.get_mut(&key) {
                    section(ui, &key, &key, value, &default);
                }
            }
        });

    if !open && menu.close_panel() && *state.get() == AppState::Paused {
        next.set(AppState::Playing);
    }
    if edited != original
        && let Ok(next) = serde_json::from_value::<Tuning>(edited)
        && next != *tuning
    {
        *tuning = next;
    }
    if save_now && let Some(autosave) = autosave.as_mut() {
        autosave.save_now = true;
    }
    Ok(())
}

/// A collapsible group: scalar fields in a grid, nested groups below it.
fn section(ui: &mut egui::Ui, path: &str, key: &str, value: &mut Value, default: &Value) {
    let Value::Object(map) = value else {
        return;
    };
    egui::CollapsingHeader::new(pretty(key))
        .id_salt(path)
        .default_open(path == "look")
        .show(ui, |ui| {
            if ui.small_button("Reset to defaults").clicked() {
                *map = default.as_object().cloned().unwrap_or_default();
            }
            let mut keys: Vec<String> = map.keys().cloned().collect();
            keys.sort();
            egui::Grid::new(format!("{path}-grid"))
                .num_columns(2)
                .striped(true)
                .spacing([12.0, 4.0])
                .show(ui, |ui| {
                    for k in &keys {
                        let field = format!("{path}.{k}");
                        if let Some(v) = map.get_mut(k)
                            && !v.is_object()
                        {
                            ui.label(pretty(k));
                            scalar(ui, &field, v);
                            ui.end_row();
                        }
                    }
                });
            for k in &keys {
                let field = format!("{path}.{k}");
                let d = default.get(k).cloned().unwrap_or(Value::Null);
                if let Some(v) = map.get_mut(k)
                    && v.is_object()
                {
                    section(ui, &field, k, v, &d);
                }
            }
        });
}

fn scalar(ui: &mut egui::Ui, path: &str, value: &mut Value) {
    match value {
        Value::Bool(b) => {
            ui.checkbox(b, "");
        }
        Value::Number(n) => {
            if let Some(mut v) = n.as_u64() {
                if ui.add(egui::DragValue::new(&mut v).speed(0.25)).changed() {
                    *n = Number::from(v);
                }
            } else if let Some(mut v) = n.as_i64() {
                if ui.add(egui::DragValue::new(&mut v).speed(0.25)).changed() {
                    *n = Number::from(v);
                }
            } else if let Some(mut v) = n.as_f64() {
                let magnitude = v.abs().max(1e-3);
                let decimals = if magnitude < 0.01 {
                    5
                } else if magnitude < 1.0 {
                    3
                } else {
                    2
                };
                let drag = egui::DragValue::new(&mut v)
                    .speed(magnitude * 0.01)
                    .max_decimals(decimals);
                if ui.add(drag).changed()
                    && let Some(next) = Number::from_f64(v)
                {
                    *n = next;
                }
            }
        }
        Value::String(s) => {
            if let Some(variants) = enum_variants(path) {
                egui::ComboBox::from_id_salt(path)
                    .selected_text(s.clone())
                    .show_ui(ui, |ui| {
                        for variant in variants {
                            ui.selectable_value(s, (*variant).to_string(), *variant);
                        }
                    });
            } else {
                ui.text_edit_singleline(s);
            }
        }
        Value::Array(items) => {
            ui.vertical(|ui| {
                for (i, item) in items.iter_mut().enumerate() {
                    scalar(ui, &format!("{path}.{i}"), item);
                }
            });
        }
        Value::Null | Value::Object(_) => {
            ui.weak("-");
        }
    }
}
