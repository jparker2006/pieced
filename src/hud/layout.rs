//! The HUD's node tree, spawned once at startup. Systems in `systems.rs` find the
//! parts they drive through the [`El`] marker.

use super::HudTuning;
use crate::palette;
use bevy::{prelude::*, text::LetterSpacing};

/// Shared text and color styling (the menu uses it too).
pub mod style {
    use crate::palette;
    use bevy::{prelude::*, text::LetterSpacing};

    pub const TEXT: Color = palette::UI_TEXT;
    pub const ACCENT: Color = palette::WOOD_LIGHT;
    pub const INK: Color = Color::srgb(0.09, 0.08, 0.07);
    pub const DANGER: Color = Color::srgb(1.0, 0.36, 0.3);

    /// UI text at `alpha`.
    pub fn dim(alpha: f32) -> Color {
        TEXT.with_alpha(alpha)
    }

    /// A subtle drop shadow for text over the 3D view.
    pub fn shadow() -> TextShadow {
        TextShadow {
            offset: Vec2::new(1.0, 1.5),
            color: palette::UI_SHADOW,
        }
    }

    /// A text node with the HUD's shadow.
    pub fn text(value: impl Into<String>, size: f32, color: Color) -> impl Bundle {
        (
            Text::new(value),
            TextFont::from_font_size(size),
            TextColor(color),
            shadow(),
        )
    }

    /// Small spaced capitals for labels.
    pub fn caps(value: impl Into<String>, size: f32, color: Color) -> impl Bundle {
        (text(value, size, color), LetterSpacing::Px(size * 0.14))
    }
}

use style::{ACCENT, TEXT, caps, dim, text};

/// Parts of the HUD that systems update.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum El {
    Root,
    ShieldFill,
    ShieldTrail,
    ShieldValue,
    HealthFill,
    HealthTrail,
    HealthValue,
    WeaponLabel,
    AmmoValue,
    AmmoMax,
    ReloadRow,
    ReloadFill,
    Slot(u8),
    SlotIcon(u8),
    SlotName(u8),
    BuildBadge,
    PieceGroup,
    PieceFill,
    PieceName,
    PieceValue,
    Readout,
    ReadoutValue(u8),
    Perf,
    PerfFps,
    PerfMs,
    PerfWorst,
    Tick(u8),
    Dot,
    PumpRing,
    BuildReticle,
    MarkerBar(u8),
}

/// A pooled floating damage number.
#[derive(Component, Debug, Clone, Default)]
pub(super) struct DamageNumber {
    pub active: bool,
    pub age: f32,
    pub point: Vec3,
    pub jitter: f32,
    pub color: Color,
}

pub(super) const NUMBER_POOL: usize = 24;
pub(super) const NUMBER_BOX: Vec2 = Vec2::new(160.0, 40.0);

/// The tick lengths of the crosshair (px at scale 1).
pub(super) const TICK_LEN: f32 = 7.0;
pub(super) const TICK_WIDTH: f32 = 2.0;

pub(super) const SLOTS: [(&str, &str); 5] = [
    ("1", "SCAR"),
    ("2", "PUMP"),
    ("Q", "WALL"),
    ("E", "RAMP"),
    ("F", "FLOOR"),
];

pub(super) fn build(app: &mut App) {
    app.add_systems(Startup, spawn_hud);
}

fn outline() -> Outline {
    Outline {
        width: px(1),
        offset: px(0),
        color: Color::srgba(0.0, 0.0, 0.0, 0.5),
    }
}

fn abs() -> Node {
    Node {
        position_type: PositionType::Absolute,
        ..default()
    }
}

/// A node centered on its parent's origin (for the crosshair anchor).
fn centered(w: f32, h: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(-w / 2.0),
        top: px(-h / 2.0),
        width: px(w),
        height: px(h),
        ..default()
    }
}

fn spawn_hud(mut commands: Commands, tuning: Option<Res<crate::tuning::Tuning>>) {
    let hud = tuning.map(|t| t.hud.clone()).unwrap_or_default();
    commands
        .spawn((
            Name::new("HUD"),
            El::Root,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
            GlobalZIndex(10),
        ))
        .with_children(|root| {
            spawn_crosshair(root, &hud);
            spawn_status(root);
            spawn_ammo(root);
            spawn_hotbar(root);
            spawn_readout(root);
            spawn_perf(root);
            spawn_numbers(root);
        });
}

fn spawn_crosshair(root: &mut ChildSpawnerCommands, _hud: &HudTuning) {
    root.spawn((
        Name::new("Crosshair"),
        Node {
            position_type: PositionType::Absolute,
            left: percent(50),
            top: percent(50),
            width: px(0),
            height: px(0),
            ..default()
        },
    ))
    .with_children(|c| {
        for i in 0..4u8 {
            let vertical = i % 2 == 0;
            let (w, h) = if vertical {
                (TICK_WIDTH, TICK_LEN)
            } else {
                (TICK_LEN, TICK_WIDTH)
            };
            c.spawn((
                El::Tick(i),
                centered(w, h),
                BackgroundColor(TEXT),
                outline(),
                UiTransform::default(),
            ));
        }
        c.spawn((
            El::Dot,
            centered(2.0, 2.0),
            BackgroundColor(TEXT),
            outline(),
        ));
        c.spawn((
            El::PumpRing,
            Node {
                border: UiRect::all(px(1.5)),
                border_radius: BorderRadius::MAX,
                ..centered(40.0, 40.0)
            },
            BorderColor::all(TEXT.with_alpha(0.9)),
            outline(),
            Visibility::Hidden,
        ));
        c.spawn((
            El::BuildReticle,
            Node {
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(2)),
                ..centered(14.0, 14.0)
            },
            BorderColor::all(ACCENT),
            outline(),
            UiTransform::from_rotation(Rot2::degrees(45.0)),
            Visibility::Hidden,
        ));
        for i in 0..4u8 {
            c.spawn((
                El::MarkerBar(i),
                centered(2.5, 9.0),
                BackgroundColor(palette::HIT_WHITE),
                Outline {
                    width: px(1),
                    offset: px(0),
                    color: Color::srgba(0.0, 0.0, 0.0, 0.45),
                },
                UiTransform::default(),
                Visibility::Hidden,
            ));
        }
        // The HP bar of the piece under the crosshair.
        c.spawn((
            El::PieceGroup,
            Node {
                position_type: PositionType::Absolute,
                left: px(-62),
                top: px(34),
                width: px(124),
                flex_direction: FlexDirection::Column,
                row_gap: px(3),
                ..default()
            },
            Visibility::Hidden,
        ))
        .with_children(|g| {
            g.spawn(Node {
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            })
            .with_children(|row| {
                row.spawn((El::PieceName, caps("WALL", 11.0, dim(0.85))));
                row.spawn((El::PieceValue, text("200", 11.0, TEXT)));
            });
            g.spawn((
                Node {
                    height: px(5),
                    border_radius: BorderRadius::all(px(2)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
            ))
            .with_children(|bar| {
                bar.spawn((
                    El::PieceFill,
                    Node {
                        width: percent(100),
                        height: percent(100),
                        ..default()
                    },
                    BackgroundColor(ACCENT),
                ));
            });
        });
    });
}

/// One health or shield bar row.
fn bar_row(
    parent: &mut ChildSpawnerCommands,
    icon: impl FnOnce(&mut ChildSpawnerCommands),
    value: El,
    trail: El,
    fill: El,
    color: Color,
    height: f32,
) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(8),
            ..default()
        })
        .with_children(|row| {
            row.spawn(Node {
                width: px(14),
                height: px(14),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(icon);
            row.spawn((Node {
                width: px(40),
                justify_content: JustifyContent::FlexEnd,
                ..default()
            },))
                .with_children(|v| {
                    v.spawn((value, text("100", 19.0, TEXT)));
                });
            row.spawn((
                Node {
                    width: px(236),
                    height: px(height),
                    border_radius: BorderRadius::all(px(3)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.02, 0.03, 0.05, 0.5)),
                BoxShadow::new(
                    Color::srgba(0.0, 0.0, 0.0, 0.25),
                    px(0),
                    px(1),
                    px(0),
                    px(3),
                ),
            ))
            .with_children(|bar| {
                bar.spawn((
                    trail,
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(0),
                        top: px(0),
                        width: percent(100),
                        height: percent(100),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(1.0, 0.97, 0.92, 0.85)),
                ));
                bar.spawn((
                    fill,
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(0),
                        top: px(0),
                        width: percent(100),
                        height: percent(100),
                        ..default()
                    },
                    BackgroundColor(color),
                ))
                .with_children(|f| {
                    // A soft top highlight gives the bar some form.
                    f.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(0),
                            top: px(0),
                            width: percent(100),
                            height: percent(38),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.22)),
                    ));
                });
                for q in 1..4 {
                    bar.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: percent(25 * q),
                            top: px(0),
                            width: px(2),
                            height: percent(100),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.28)),
                    ));
                }
            });
        });
}

fn spawn_status(root: &mut ChildSpawnerCommands) {
    root.spawn((
        Name::new("Status"),
        Node {
            left: px(28),
            bottom: px(26),
            flex_direction: FlexDirection::Column,
            row_gap: px(7),
            ..abs()
        },
    ))
    .with_children(|s| {
        bar_row(
            s,
            |icon| {
                icon.spawn((
                    Node {
                        width: px(10),
                        height: px(10),
                        border_radius: BorderRadius::all(px(2)),
                        ..default()
                    },
                    BackgroundColor(palette::SHIELD),
                    UiTransform::from_rotation(Rot2::degrees(45.0)),
                ));
            },
            El::ShieldValue,
            El::ShieldTrail,
            El::ShieldFill,
            palette::SHIELD,
            12.0,
        );
        bar_row(
            s,
            |icon| {
                icon.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        width: px(12),
                        height: px(4),
                        ..default()
                    },
                    BackgroundColor(palette::HEALTH),
                ));
                icon.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        width: px(4),
                        height: px(12),
                        ..default()
                    },
                    BackgroundColor(palette::HEALTH),
                ));
            },
            El::HealthValue,
            El::HealthTrail,
            El::HealthFill,
            palette::HEALTH,
            16.0,
        );
    });
}

fn spawn_ammo(root: &mut ChildSpawnerCommands) {
    root.spawn((
        Name::new("Ammo"),
        Node {
            right: px(30),
            bottom: px(20),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::FlexEnd,
            row_gap: px(0),
            ..abs()
        },
    ))
    .with_children(|a| {
        a.spawn((El::WeaponLabel, caps("SCAR", 13.0, dim(0.75))));
        a.spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Baseline,
            column_gap: px(6),
            ..default()
        })
        .with_children(|row| {
            row.spawn((El::AmmoValue, text("30", 44.0, TEXT)));
            row.spawn((El::AmmoMax, text("/ 30", 19.0, dim(0.6))));
        });
        a.spawn((
            El::ReloadRow,
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(8),
                height: px(14),
                ..default()
            },
            Visibility::Hidden,
        ))
        .with_children(|row| {
            row.spawn(caps("RELOADING", 10.0, ACCENT));
            row.spawn((
                Node {
                    width: px(110),
                    height: px(4),
                    border_radius: BorderRadius::all(px(2)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
            ))
            .with_children(|bar| {
                bar.spawn((
                    El::ReloadFill,
                    Node {
                        width: percent(0),
                        height: percent(100),
                        ..default()
                    },
                    BackgroundColor(ACCENT),
                ));
            });
        });
    });
}

/// Draws a slot's icon from a few flat shapes.
fn slot_icon(icon: &mut ChildSpawnerCommands, slot: u8) {
    let part = |icon: &mut ChildSpawnerCommands, x: f32, y: f32, w: f32, h: f32, rot: f32| {
        icon.spawn((
            El::SlotIcon(slot),
            Node {
                position_type: PositionType::Absolute,
                left: px(x),
                top: px(y),
                width: px(w),
                height: px(h),
                border_radius: BorderRadius::all(px(1)),
                ..default()
            },
            BackgroundColor(dim(0.8)),
            UiTransform::from_rotation(Rot2::degrees(rot)),
        ));
    };
    match slot {
        // Rifle: stock, receiver, barrel, magazine.
        0 => {
            part(icon, 0.0, 7.0, 7.0, 6.0, 0.0);
            part(icon, 6.0, 6.0, 18.0, 5.0, 0.0);
            part(icon, 24.0, 7.0, 10.0, 2.0, 0.0);
            part(icon, 14.0, 10.0, 4.0, 7.0, 12.0);
        }
        // Pump: stock, receiver, long barrel, fore-end.
        1 => {
            part(icon, 0.0, 7.0, 6.0, 6.0, 0.0);
            part(icon, 5.0, 6.0, 12.0, 5.0, 0.0);
            part(icon, 17.0, 6.0, 17.0, 3.0, 0.0);
            part(icon, 20.0, 9.0, 9.0, 4.0, 0.0);
        }
        // Wall: an upright panel.
        2 => part(icon, 9.0, 1.0, 16.0, 16.0, 0.0),
        // Ramp: a slope.
        3 => part(icon, 3.0, 7.0, 28.0, 5.0, -32.0),
        // Floor: a flat slab.
        _ => part(icon, 5.0, 11.0, 24.0, 5.0, 0.0),
    }
}

fn spawn_hotbar(root: &mut ChildSpawnerCommands) {
    root.spawn((
        Name::new("Hotbar"),
        Node {
            left: px(0),
            right: px(0),
            bottom: px(20),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(8),
            ..abs()
        },
    ))
    .with_children(|h| {
        h.spawn((
            El::BuildBadge,
            Node {
                padding: UiRect::axes(px(10), px(3)),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(ACCENT),
            BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.3), px(0), px(1), px(0), px(4)),
            Visibility::Hidden,
        ))
        .with_children(|b| {
            b.spawn((
                Text::new("BUILD MODE"),
                TextFont::from_font_size(11.0),
                TextColor(style::INK),
                LetterSpacing::Px(1.8),
            ));
        });
        h.spawn(Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(6),
            ..default()
        })
        .with_children(|row| {
            for (i, (key, name)) in SLOTS.iter().enumerate() {
                let i = i as u8;
                if i == 2 {
                    row.spawn(Node {
                        width: px(10),
                        ..default()
                    });
                }
                row.spawn((
                    El::Slot(i),
                    Node {
                        width: px(62),
                        height: px(54),
                        border: UiRect::all(px(2)),
                        border_radius: BorderRadius::all(px(8)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.03, 0.04, 0.06, 0.42)),
                    BorderColor::all(Color::NONE),
                    UiTransform::default(),
                ))
                .with_children(|slot| {
                    slot.spawn((
                        Node {
                            left: px(6),
                            top: px(2),
                            ..abs()
                        },
                        children![text(*key, 11.0, dim(0.7))],
                    ));
                    slot.spawn(Node {
                        width: px(34),
                        height: px(18),
                        margin: UiRect::bottom(px(8)),
                        ..default()
                    })
                    .with_children(|icon| slot_icon(icon, i));
                    slot.spawn((
                        Node {
                            left: px(0),
                            right: px(0),
                            bottom: px(3),
                            justify_content: JustifyContent::Center,
                            ..abs()
                        },
                        children![(El::SlotName(i), caps(*name, 9.0, dim(0.7)))],
                    ));
                });
            }
        });
    });
}

fn spawn_readout(root: &mut ChildSpawnerCommands) {
    root.spawn((
        El::Readout,
        Name::new("Combat readout"),
        Node {
            right: px(20),
            top: px(18),
            flex_direction: FlexDirection::Row,
            column_gap: px(18),
            padding: UiRect::axes(px(14), px(8)),
            border_radius: BorderRadius::all(px(8)),
            ..abs()
        },
        BackgroundColor(palette::UI_PANEL),
    ))
    .with_children(|r| {
        for (i, label) in ["TTK", "ACC", "HEAD", "ELIMS"].iter().enumerate() {
            r.spawn(Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(1),
                ..default()
            })
            .with_children(|cell| {
                cell.spawn(caps(*label, 9.0, dim(0.6)));
                cell.spawn((El::ReadoutValue(i as u8), text("-", 17.0, TEXT)));
            });
        }
    });
}

fn spawn_perf(root: &mut ChildSpawnerCommands) {
    root.spawn((
        El::Perf,
        Name::new("Performance overlay"),
        Node {
            left: px(16),
            top: px(14),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Baseline,
            column_gap: px(12),
            padding: UiRect::axes(px(10), px(5)),
            border_radius: BorderRadius::all(px(6)),
            ..abs()
        },
        BackgroundColor(palette::UI_PANEL),
        Visibility::Hidden,
    ))
    .with_children(|p| {
        p.spawn((El::PerfFps, text("60 FPS", 14.0, TEXT)));
        p.spawn((El::PerfMs, text("16.7 ms", 13.0, dim(0.75))));
        p.spawn((El::PerfWorst, text("worst 16.7 ms", 13.0, palette::HEALTH)));
    });
}

fn spawn_numbers(root: &mut ChildSpawnerCommands) {
    root.spawn((
        Name::new("Damage numbers"),
        Node {
            left: px(0),
            top: px(0),
            width: percent(100),
            height: percent(100),
            ..abs()
        },
    ))
    .with_children(|n| {
        for _ in 0..NUMBER_POOL {
            n.spawn((
                DamageNumber::default(),
                Node {
                    left: px(0),
                    top: px(0),
                    width: px(NUMBER_BOX.x),
                    height: px(NUMBER_BOX.y),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..abs()
                },
                Text::new(""),
                TextFont::from_font_size(26.0),
                TextColor(TEXT),
                TextLayout::justify(Justify::Center),
                TextShadow {
                    offset: Vec2::new(2.0, 2.0),
                    color: Color::srgba(0.0, 0.0, 0.0, 0.8),
                },
                UiTransform::default(),
                Visibility::Hidden,
            ));
        }
    });
}
