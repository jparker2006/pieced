//! The HUD's node tree, spawned once at startup. Systems in `systems.rs` find the
//! parts they drive through the [`El`] marker.
//!
//! Milestone 2 restyles Milestone 1's layout without moving it (the anchors are
//! in [`anchors`], pinned by a test): rounded cartoon frames with dark ink
//! borders round a slate rim, a crystal-badged shield bar over a heart-badged
//! health bar, the ammo count beside a crystal, hotbar slots showing the
//! Blender-rendered icons with a gold frame on the selected one, and damage
//! numbers in the cartoon font with an ink outline.

use super::{HudTuning, art::UiArt};
use crate::palette;
use bevy::{prelude::*, text::LetterSpacing};

/// Shared text and color styling (the menu uses it too).
pub mod style {
    use crate::palette::{self, cartoon};
    use bevy::{prelude::*, text::LetterSpacing};

    pub const TEXT: Color = palette::UI_TEXT;
    /// Cartoon gold: the selected slot, badges, the menu's trim.
    pub const ACCENT: Color = Color::srgb(1.0, 0.79, 0.23);
    /// Ink: the dark border round every frame and the outline of text.
    pub const INK: Color = Color::srgb(0.035, 0.04, 0.075);
    pub const DANGER: Color = Color::srgb(1.0, 0.36, 0.3);
    /// The slate rim inside a frame's ink border.
    pub const RIM: Color = cartoon::HUD_FRAME;
    /// The dark panel inside a frame (the targets' HUD navy).
    pub const PANEL: Color = Color::srgba(0.086, 0.137, 0.208, 0.9);
    /// Hotbar slots: the targets' dark teal slate.
    pub const SLOT: Color = Color::srgba(0.1, 0.2, 0.2, 0.86);
    /// The empty part of a bar.
    pub const TROUGH: Color = Color::srgba(0.02, 0.035, 0.06, 0.92);
    /// The cyan shield and green health fills.
    pub const SHIELD_FILL: Color = cartoon::HUD_SHIELD;
    pub const HEALTH_FILL: Color = cartoon::HUD_HEALTH;

    /// UI text at `alpha`.
    pub fn dim(alpha: f32) -> Color {
        TEXT.with_alpha(alpha)
    }

    /// A hard ink drop under text: the cartoon "cut-out" look.
    pub fn shadow() -> TextShadow {
        TextShadow {
            offset: Vec2::new(1.0, 2.0),
            color: INK.with_alpha(0.9),
        }
    }

    /// A text node in the HUD font with the ink drop.
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
        (text(value, size, color), LetterSpacing::Px(size * 0.1))
    }

    /// The ink line round a frame.
    pub fn ink(width: f32) -> Outline {
        Outline::new(px(width), px(0), INK)
    }
}

use style::{
    ACCENT, HEALTH_FILL, INK, PANEL, RIM, SHIELD_FILL, SLOT, TEXT, TROUGH, caps, dim, ink, text,
};

/// Parts of the HUD that systems update.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum El {
    Root,
    Crosshair,
    Numbers,
    ShieldFill,
    ShieldTrail,
    ShieldValue,
    HealthFill,
    HealthTrail,
    HealthValue,
    WeaponLabel,
    AmmoCrystal,
    AmmoValue,
    AmmoMax,
    ReloadRow,
    ReloadFill,
    Slot(u8),
    SlotIcon(u8),
    BuildBadge,
    /// The badge's words: BUILD MODE, or EDIT MODE while editing (D44).
    BuildBadgeText,
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

/// A pooled floating damage number: a box whose children are the glyph
/// layers ([`NumberGlyph`]), ink copies under the colored face.
#[derive(Component, Debug, Clone, Default)]
pub struct DamageNumber {
    pub active: bool,
    pub age: f32,
    pub point: Vec3,
    pub jitter: f32,
    pub color: Color,
}

/// One text layer of a damage number: `0..INK_LAYERS` are ink copies (the
/// outline), `INK_LAYERS` is the face.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumberGlyph(pub u8);

/// Ink copies per damage number. Each also draws its text shadow as a second
/// copy, so four entities trace the outline in eight directions.
pub const INK_LAYERS: u8 = 4;

/// Where the ink copies of a damage number sit round its face, in outline
/// widths: (the copy's offset, its shadow's offset from the copy).
pub(super) const INK_OFFSETS: [(Vec2, Vec2); INK_LAYERS as usize] = [
    (Vec2::new(-0.72, -0.72), Vec2::new(1.44, 0.0)),
    (Vec2::new(-0.72, 0.72), Vec2::new(1.44, 0.0)),
    (Vec2::new(-1.0, 0.0), Vec2::new(2.0, 0.0)),
    (Vec2::new(0.0, -1.0), Vec2::new(0.0, 2.0)),
];

/// Damage numbers in the pool. Every text entity they need is spawned once at
/// startup; hits reuse them.
pub const NUMBER_POOL: usize = 24;
pub(super) const NUMBER_BOX: Vec2 = Vec2::new(190.0, 72.0);

/// The tick lengths of the crosshair (px at scale 1).
pub(super) const TICK_LEN: f32 = 7.0;
pub(super) const TICK_WIDTH: f32 = 2.5;

pub(super) const SLOT_KEYS: [&str; 6] = ["1", "2", "Q", "E", "F", "V"];

/// Where each HUD group is anchored (px from the screen edges). Milestone 2
/// restyles the HUD without moving it.
pub mod anchors {
    /// Shield and health bars: (left, bottom).
    pub const STATUS: (f32, f32) = (28.0, 26.0);
    /// Ammo: (right, bottom).
    pub const AMMO: (f32, f32) = (30.0, 20.0);
    /// Hotbar: centered, bottom.
    pub const HOTBAR_BOTTOM: f32 = 20.0;
    /// Combat readout: (right, top).
    pub const READOUT: (f32, f32) = (20.0, 18.0);
    /// Performance overlay: (left, top).
    pub const PERF: (f32, f32) = (16.0, 14.0);
    /// The piece HP bar under the crosshair: (top below the center, width).
    pub const PIECE: (f32, f32) = (34.0, 124.0);
}

pub(super) fn build(app: &mut App) {
    app.add_systems(Startup, spawn_hud);
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

/// A rounded cartoon frame: ink line, slate rim, dark panel.
fn frame(node: Node, rim: f32, radius: f32, ink_width: f32) -> impl Bundle {
    (
        Node {
            border: UiRect::all(px(rim)),
            border_radius: BorderRadius::all(px(radius)),
            ..node
        },
        BackgroundColor(PANEL),
        BorderColor::all(RIM),
        ink(ink_width),
    )
}

fn spawn_hud(
    mut commands: Commands,
    tuning: Option<Res<crate::tuning::Tuning>>,
    art: Option<Res<UiArt>>,
) {
    let hud = tuning.map(|t| t.hud.clone()).unwrap_or_default();
    let art = art.map(|a| a.clone()).unwrap_or_default();
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
            spawn_status(root, &art);
            spawn_ammo(root, &art);
            spawn_hotbar(root, &art);
            spawn_readout(root);
            spawn_perf(root);
            spawn_numbers(root);
        });
}

fn spawn_crosshair(root: &mut ChildSpawnerCommands, _hud: &HudTuning) {
    let edge = || Outline::new(px(1.5), px(0), INK.with_alpha(0.85));
    root.spawn((
        El::Crosshair,
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
                Node {
                    border_radius: BorderRadius::all(px(1)),
                    ..centered(w, h)
                },
                BackgroundColor(TEXT),
                edge(),
                UiTransform::default(),
            ));
        }
        c.spawn((
            El::Dot,
            Node {
                border_radius: BorderRadius::MAX,
                ..centered(3.0, 3.0)
            },
            BackgroundColor(TEXT),
            edge(),
        ));
        c.spawn((
            El::PumpRing,
            Node {
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::MAX,
                ..centered(40.0, 40.0)
            },
            BorderColor::all(TEXT.with_alpha(0.95)),
            edge(),
            Visibility::Hidden,
        ));
        c.spawn((
            El::BuildReticle,
            Node {
                border: UiRect::all(px(2.5)),
                border_radius: BorderRadius::all(px(3)),
                ..centered(15.0, 15.0)
            },
            BorderColor::all(ACCENT),
            edge(),
            UiTransform::from_rotation(Rot2::degrees(45.0)),
            Visibility::Hidden,
        ));
        for i in 0..4u8 {
            c.spawn((
                El::MarkerBar(i),
                Node {
                    border_radius: BorderRadius::all(px(1.5)),
                    ..centered(3.0, 10.0)
                },
                BackgroundColor(palette::HIT_WHITE),
                edge(),
                UiTransform::default(),
                Visibility::Hidden,
            ));
        }
        // The HP bar of the piece under the crosshair.
        let (top, width) = anchors::PIECE;
        c.spawn((
            El::PieceGroup,
            Node {
                position_type: PositionType::Absolute,
                left: px(-width / 2.0),
                top: px(top),
                width: px(width),
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
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
                row.spawn((El::PieceName, caps("WALL", 12.0, TEXT)));
                row.spawn((El::PieceValue, text("200", 12.0, dim(0.9))));
            });
            g.spawn((
                Node {
                    height: px(10),
                    border: UiRect::all(px(1.5)),
                    border_radius: BorderRadius::MAX,
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(TROUGH),
                BorderColor::all(RIM),
                ink(1.5),
            ))
            .with_children(|bar| {
                bar.spawn((
                    El::PieceFill,
                    Node {
                        width: percent(100),
                        height: percent(100),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(ACCENT),
                ));
            });
        });
    });
}

/// One health or shield bar row: a round badge with its icon, then a pill
/// holding the value and the bar.
fn bar_row(
    parent: &mut ChildSpawnerCommands,
    icon: Handle<Image>,
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
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Node {
                    width: px(36),
                    height: px(36),
                    border: UiRect::all(px(3)),
                    border_radius: BorderRadius::MAX,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(PANEL.with_alpha(1.0)),
                BorderColor::all(RIM),
                ink(2.0),
                ZIndex(1),
            ))
            .with_child((
                ImageNode::new(icon),
                Node {
                    width: px(26),
                    height: px(26),
                    ..default()
                },
            ));
            row.spawn(frame(
                Node {
                    margin: UiRect::left(px(-9)),
                    padding: UiRect::new(px(12), px(5), px(3), px(3)),
                    column_gap: px(7),
                    align_items: AlignItems::Center,
                    ..default()
                },
                2.5,
                99.0,
                2.0,
            ))
            .with_children(|pill| {
                pill.spawn(Node {
                    width: px(34),
                    justify_content: JustifyContent::FlexEnd,
                    ..default()
                })
                .with_children(|v| {
                    v.spawn((value, text("100", 17.0, TEXT)));
                });
                pill.spawn((
                    Node {
                        width: px(236),
                        height: px(height),
                        border_radius: BorderRadius::MAX,
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(TROUGH),
                ))
                .with_children(|bar| {
                    let full = || Node {
                        position_type: PositionType::Absolute,
                        left: px(0),
                        top: px(0),
                        width: percent(100),
                        height: percent(100),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    };
                    bar.spawn((
                        trail,
                        full(),
                        BackgroundColor(Color::srgba(1.0, 0.97, 0.92, 0.85)),
                    ));
                    bar.spawn((fill, full(), BackgroundColor(color)))
                        .with_children(|f| {
                            // A cartoon shine along the top of the fill.
                            f.spawn((
                                Node {
                                    position_type: PositionType::Absolute,
                                    left: px(4),
                                    right: px(4),
                                    top: px(2),
                                    height: percent(30),
                                    border_radius: BorderRadius::MAX,
                                    ..default()
                                },
                                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.38)),
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
                            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.22)),
                        ));
                    }
                });
            });
        });
}

fn spawn_status(root: &mut ChildSpawnerCommands, art: &UiArt) {
    let (left, bottom) = anchors::STATUS;
    root.spawn((
        Name::new("Status"),
        Node {
            left: px(left),
            bottom: px(bottom),
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            ..abs()
        },
    ))
    .with_children(|s| {
        bar_row(
            s,
            art.crystal_blue.clone(),
            El::ShieldValue,
            El::ShieldTrail,
            El::ShieldFill,
            SHIELD_FILL,
            16.0,
        );
        bar_row(
            s,
            art.heart.clone(),
            El::HealthValue,
            El::HealthTrail,
            El::HealthFill,
            HEALTH_FILL,
            16.0,
        );
    });
}

fn spawn_ammo(root: &mut ChildSpawnerCommands, art: &UiArt) {
    let (right, bottom) = anchors::AMMO;
    root.spawn((
        Name::new("Ammo"),
        Node {
            right: px(right),
            bottom: px(bottom),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::FlexEnd,
            row_gap: px(4),
            ..abs()
        },
    ))
    .with_children(|a| {
        a.spawn((
            El::WeaponLabel,
            caps("SCAR", 14.0, dim(0.9)),
            Node {
                margin: UiRect::right(px(12)),
                ..default()
            },
        ));
        a.spawn(frame(
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(8),
                padding: UiRect::new(px(10), px(18), px(2), px(2)),
                ..default()
            },
            3.0,
            18.0,
            2.5,
        ))
        .with_children(|panel| {
            panel.spawn((
                El::AmmoCrystal,
                ImageNode::new(art.crystal_blue.clone()),
                Node {
                    width: px(40),
                    height: px(40),
                    ..default()
                },
                UiTransform::default(),
            ));
            panel
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Baseline,
                    column_gap: px(6),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((El::AmmoValue, text("30", 40.0, TEXT)));
                    row.spawn((El::AmmoMax, text("/ 30", 20.0, dim(0.75))));
                });
        });
        a.spawn((
            El::ReloadRow,
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(8),
                height: px(16),
                margin: UiRect::right(px(12)),
                ..default()
            },
            Visibility::Hidden,
        ))
        .with_children(|row| {
            row.spawn(caps("RELOADING", 11.0, ACCENT));
            row.spawn((
                Node {
                    width: px(110),
                    height: px(8),
                    border: UiRect::all(px(1.5)),
                    border_radius: BorderRadius::MAX,
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(TROUGH),
                BorderColor::all(RIM),
                ink(1.5),
            ))
            .with_children(|bar| {
                bar.spawn((
                    El::ReloadFill,
                    Node {
                        width: percent(0),
                        height: percent(100),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(ACCENT),
                ));
            });
        });
    });
}

fn spawn_hotbar(root: &mut ChildSpawnerCommands, art: &UiArt) {
    root.spawn((
        Name::new("Hotbar"),
        Node {
            left: px(0),
            right: px(0),
            bottom: px(anchors::HOTBAR_BOTTOM),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(14),
            ..abs()
        },
    ))
    .with_children(|h| {
        h.spawn((
            El::BuildBadge,
            Node {
                padding: UiRect::axes(px(12), px(3)),
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(ACCENT),
            BorderColor::all(Color::srgb(1.0, 0.93, 0.62)),
            ink(2.0),
            Visibility::Hidden,
        ))
        .with_children(|b| {
            b.spawn((
                El::BuildBadgeText,
                Text::new("BUILD MODE"),
                TextFont::from_font_size(13.0),
                TextColor(INK),
                LetterSpacing::Px(1.4),
            ));
        });
        h.spawn(Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(6),
            ..default()
        })
        .with_children(|row| {
            for (i, key) in SLOT_KEYS.iter().enumerate() {
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
                        width: px(66),
                        height: px(60),
                        border: UiRect::all(px(3)),
                        border_radius: BorderRadius::all(px(11)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(SLOT),
                    BorderColor::all(RIM),
                    ink(2.0),
                    UiTransform::default(),
                ))
                .with_children(|slot| {
                    slot.spawn((
                        El::SlotIcon(i),
                        ImageNode::new(art.slots[i as usize].clone()),
                        Node {
                            width: px(54),
                            height: px(54),
                            ..default()
                        },
                    ));
                    // The key, on a little ink cap over the corner.
                    slot.spawn((
                        Node {
                            left: px(-8),
                            top: px(-9),
                            min_width: px(18),
                            height: px(18),
                            padding: UiRect::horizontal(px(3)),
                            border: UiRect::all(px(1.5)),
                            border_radius: BorderRadius::all(px(6)),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..abs()
                        },
                        BackgroundColor(INK),
                        BorderColor::all(RIM),
                        children![(
                            Text::new(*key),
                            TextFont::from_font_size(12.0),
                            TextColor(TEXT),
                        )],
                    ));
                });
            }
        });
    });
}

fn spawn_readout(root: &mut ChildSpawnerCommands) {
    let (right, top) = anchors::READOUT;
    root.spawn((
        El::Readout,
        Name::new("Combat readout"),
        frame(
            Node {
                right: px(right),
                top: px(top),
                flex_direction: FlexDirection::Row,
                column_gap: px(18),
                padding: UiRect::axes(px(14), px(6)),
                ..abs()
            },
            2.5,
            14.0,
            2.0,
        ),
    ))
    .with_children(|r| {
        for (i, label) in ["TTK", "ACC", "HEAD", "ELIMS"].iter().enumerate() {
            r.spawn(Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|cell| {
                cell.spawn(caps(*label, 10.0, ACCENT));
                cell.spawn((El::ReadoutValue(i as u8), text("-", 18.0, TEXT)));
            });
        }
    });
}

fn spawn_perf(root: &mut ChildSpawnerCommands) {
    let (left, top) = anchors::PERF;
    root.spawn((
        El::Perf,
        Name::new("Performance overlay"),
        frame(
            Node {
                left: px(left),
                top: px(top),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Baseline,
                column_gap: px(12),
                padding: UiRect::axes(px(10), px(4)),
                ..abs()
            },
            2.0,
            12.0,
            2.0,
        ),
        Visibility::Hidden,
    ))
    .with_children(|p| {
        p.spawn((El::PerfFps, text("60 FPS", 15.0, TEXT)));
        p.spawn((El::PerfMs, text("16.7 ms", 13.0, dim(0.8))));
        p.spawn((El::PerfWorst, text("worst 16.7 ms", 13.0, palette::HEALTH)));
    });
}

fn spawn_numbers(root: &mut ChildSpawnerCommands) {
    use super::NumberKind;
    root.spawn((
        El::Numbers,
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
        // Each kind's size is in the pool from the start, holding every digit,
        // so the glyph atlases are filled before the first hit.
        let kinds = [
            NumberKind::Body,
            NumberKind::Headshot,
            NumberKind::Structure,
        ];
        for k in 0..NUMBER_POOL {
            let size = kinds[k % kinds.len()].size();
            n.spawn((
                DamageNumber::default(),
                Node {
                    left: px(0),
                    top: px(0),
                    width: px(NUMBER_BOX.x),
                    height: px(NUMBER_BOX.y),
                    ..abs()
                },
                UiTransform::default(),
                Visibility::Hidden,
            ))
            .with_children(|glyphs| {
                for layer in 0..=INK_LAYERS {
                    let face = layer == INK_LAYERS;
                    glyphs.spawn((
                        NumberGlyph(layer),
                        Node {
                            left: px(0),
                            top: px(0),
                            width: percent(100),
                            ..abs()
                        },
                        Text::new("0123456789"),
                        TextFont::from_font_size(size),
                        TextColor(if face { TEXT } else { INK }),
                        TextLayout::justify(Justify::Center),
                        TextShadow {
                            offset: Vec2::ZERO,
                            color: INK,
                        },
                        UiTransform::default(),
                    ));
                }
            });
        }
    });
}
