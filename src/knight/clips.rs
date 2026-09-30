//! The knight's authored clips (docs/M4-SPEC.md → Chunk 4, D104):
//! keyframed in Blender on his named rigid parts
//! (`art/blender/assets/knight_anim.py`), exported in `knight.glb` and played
//! through one shared [`AnimationGraph`] ([`KnightGraph`]).
//!
//! **The graph.** An additive node at the root: its first child is a blend
//! of the full-body clips (idle, run, strafes, wind-up, beam landing,
//! victory hop, void fall, deaths, the menu's hero pose), whose weights are
//! normalised; the four short flinches are added on top of it. Every knight
//! shares the graph and the clips; each has its own [`AnimationPlayer`] (on
//! the model's `knight` node) whose clips are all started paused. The pure
//! [`ClipMix`] (from [`KnightAnim`](super::KnightAnim)) sets each clip's
//! weight and time every animation step, so the clips play on the knight's
//! own clock: hitstop, the gallery freeze and the 30 Hz LOD of far knights
//! hold them like the springs.
//!
//! **Layering.** The clips pose the joints first (`AnimationSystems`); the
//! procedural springs (squash, hit wobble, the take, the hop, the hat) are
//! written on top of them afterwards ([`super::write_pose`] composes onto the
//! animated transforms). The hitboxes never move: only the figure's model
//! nodes are animation targets.

use crate::models::ModelLibrary;
use bevy::{animation::AnimationTargetId, prelude::*};

/// The authored clips, in [`KnightClip::ALL`] order. Names match the glTF
/// animations (`knight_anim.py`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KnightClip {
    Idle,
    Run,
    /// Side steps toward his own left, or right.
    StrafeL,
    StrafeR,
    /// The wand wind-up: exactly the gameplay wind-up (0.4 s).
    WindUp,
    BeamLand,
    VictoryHop,
    VoidFall,
    DeathBack,
    DeathSpin,
    DeathCrumple,
    /// The main menu's pose (M4-V8).
    Hero,
    FlinchHead,
    FlinchChest,
    /// A hit on his own left, or right.
    FlinchL,
    FlinchR,
}

/// How many clips there are.
pub const CLIP_COUNT: usize = 16;

/// The run and strafe cycles' authored length (s): one stride of both boots.
pub const RUN_CYCLE: f32 = 0.6;
/// The wind-up clip's length (s): the gameplay wind-up (`GruntTuning::windup`).
pub const WINDUP_SECONDS: f32 = 0.4;
/// A flinch's length (s).
pub const FLINCH_SECONDS: f32 = 0.3;
/// The beam landing's length (s), and how long it fades out at its end.
pub const LAND_SECONDS: f32 = 0.5;
pub const LAND_FADE: f32 = 0.15;
/// The wind-up and the celebration fade in over this long (s).
pub const CLIP_FADE_IN: f32 = 0.08;

impl KnightClip {
    pub const ALL: [KnightClip; CLIP_COUNT] = [
        Self::Idle,
        Self::Run,
        Self::StrafeL,
        Self::StrafeR,
        Self::WindUp,
        Self::BeamLand,
        Self::VictoryHop,
        Self::VoidFall,
        Self::DeathBack,
        Self::DeathSpin,
        Self::DeathCrumple,
        Self::Hero,
        Self::FlinchHead,
        Self::FlinchChest,
        Self::FlinchL,
        Self::FlinchR,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    /// The glTF animation's name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Run => "Run",
            Self::StrafeL => "StrafeL",
            Self::StrafeR => "StrafeR",
            Self::WindUp => "WindUp",
            Self::BeamLand => "BeamLand",
            Self::VictoryHop => "VictoryHop",
            Self::VoidFall => "VoidFall",
            Self::DeathBack => "DeathBack",
            Self::DeathSpin => "DeathSpin",
            Self::DeathCrumple => "DeathCrumple",
            Self::Hero => "Hero",
            Self::FlinchHead => "FlinchHead",
            Self::FlinchChest => "FlinchChest",
            Self::FlinchL => "FlinchL",
            Self::FlinchR => "FlinchR",
        }
    }

    /// The authored length (s), as `knight_anim.py` keys it (a test checks
    /// the glb against it).
    pub fn duration(self) -> f32 {
        match self {
            Self::Idle => 2.0,
            Self::Run | Self::StrafeL | Self::StrafeR => RUN_CYCLE,
            Self::WindUp => WINDUP_SECONDS,
            Self::BeamLand => LAND_SECONDS,
            Self::VictoryHop => 0.5,
            Self::VoidFall => 0.6,
            Self::DeathBack | Self::DeathSpin | Self::DeathCrumple => super::KO_TIME,
            Self::Hero => 2.4,
            Self::FlinchHead | Self::FlinchChest | Self::FlinchL | Self::FlinchR => FLINCH_SECONDS,
        }
    }

    /// Added on top of the full-body blend (rotations only).
    pub fn is_additive(self) -> bool {
        matches!(
            self,
            Self::FlinchHead | Self::FlinchChest | Self::FlinchL | Self::FlinchR
        )
    }

    /// Cycles (played round and round).
    pub fn looping(self) -> bool {
        matches!(
            self,
            Self::Idle
                | Self::Run
                | Self::StrafeL
                | Self::StrafeR
                | Self::VictoryHop
                | Self::VoidFall
                | Self::Hero
        )
    }

    /// Played while he can be shot, so held to the hitbox fit (every 50 ms,
    /// the body within 10 cm of its capsule, the helmet within D121's 9 cm of
    /// the head sphere). Out of play: the deaths, the void fall (flung off,
    /// the figure tumbles) and the menu's pose.
    pub fn in_play(self) -> bool {
        !matches!(
            self,
            Self::VoidFall | Self::DeathBack | Self::DeathSpin | Self::DeathCrumple | Self::Hero
        )
    }

    /// Parts allowed out of the fit in this clip: the wand arm reaching
    /// forward in the wind-up (the orb leaves 0.98 m in front of him).
    pub fn fit_exempt(self) -> &'static [&'static str] {
        match self {
            Self::WindUp => &["GauntletR"],
            _ => &[],
        }
    }
}

/// Every clip's weight and time (s) for one animation step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipMix {
    pub weight: [f32; CLIP_COUNT],
    pub time: [f32; CLIP_COUNT],
}

impl Default for ClipMix {
    /// Standing idle at its start.
    fn default() -> Self {
        let mut mix = Self {
            weight: [0.0; CLIP_COUNT],
            time: [0.0; CLIP_COUNT],
        };
        mix.weight[KnightClip::Idle.index()] = 1.0;
        mix
    }
}

impl ClipMix {
    pub fn weight(&self, clip: KnightClip) -> f32 {
        self.weight[clip.index()]
    }

    pub fn time(&self, clip: KnightClip) -> f32 {
        self.time[clip.index()]
    }

    pub fn set(&mut self, clip: KnightClip, weight: f32, time: f32) {
        self.weight[clip.index()] = weight;
        self.time[clip.index()] = time;
    }

    /// Fades the full-body clips to `1 - k` and plays `clip` at `k` over them.
    pub fn over(&mut self, clip: KnightClip, k: f32, time: f32) {
        let k = k.clamp(0.0, 1.0);
        if k <= 0.0 {
            return;
        }
        for c in KnightClip::ALL.iter().filter(|c| !c.is_additive()) {
            self.weight[c.index()] *= 1.0 - k;
        }
        self.set(clip, k, time);
    }

    /// The full-body clips' total weight (1 when normalised).
    pub fn body_weight(&self) -> f32 {
        KnightClip::ALL
            .iter()
            .filter(|c| !c.is_additive())
            .map(|c| self.weight(*c))
            .sum()
    }
}

/// The joints the clips pose (every full-body clip keys all of their
/// rotations, and the torso's translation). The hat's pivots stay
/// procedural.
pub const CLIP_JOINTS: [&str; 8] = [
    "Torso",
    "PivotLegL",
    "PivotLegR",
    "PivotArmL",
    "PivotArmR",
    "PivotHead",
    "PivotCape",
    "PivotRobe",
];

/// The knights' shared graph and each clip's node in it (by
/// [`KnightClip::ALL`] order). Built once the knight model's clips exist.
#[derive(Resource, Debug, Clone)]
pub struct KnightGraph {
    pub graph: Handle<AnimationGraph>,
    pub nodes: [AnimationNodeIndex; CLIP_COUNT],
}

impl KnightGraph {
    pub fn node(&self, clip: KnightClip) -> AnimationNodeIndex {
        self.nodes[clip.index()]
    }

    /// Readies a knight's player: every clip started, paused, at weight 0
    /// (the mixer drives them from then on).
    pub fn start(&self, player: &mut AnimationPlayer) {
        for node in self.nodes {
            player.start(node).pause().set_weight(0.0);
        }
    }

    /// Writes a mix onto a knight's player (weights and seek times; nothing
    /// is allocated).
    pub fn drive(&self, player: &mut AnimationPlayer, mix: &ClipMix) {
        for (i, node) in self.nodes.iter().enumerate() {
            if let Some(active) = player.animation_mut(*node) {
                active.set_weight(mix.weight[i]).set_seek_time(mix.time[i]);
            }
        }
    }
}

/// Builds [`KnightGraph`] once the model library holds the knight's clips
/// (every clip [`KnightClip::ALL`] names must be there).
pub fn build_knight_graph(
    library: Option<Res<ModelLibrary>>,
    built: Option<Res<KnightGraph>>,
    graphs: Option<ResMut<Assets<AnimationGraph>>>,
    mut commands: Commands,
) {
    let (Some(library), None, Some(mut graphs)) = (library, built, graphs) else {
        return;
    };
    let Some(model) = library.get(super::KNIGHT_MODEL) else {
        return;
    };
    if model.clips.is_empty() {
        return;
    }
    let mut graph = AnimationGraph::new();
    let add = graph.add_additive_blend(1.0, graph.root);
    let body = graph.add_blend(1.0, add);
    let mut nodes = Vec::with_capacity(CLIP_COUNT);
    for clip in KnightClip::ALL {
        let Some((_, handle)) = model.clips.iter().find(|(n, _)| n == clip.name()) else {
            error!(
                "knight: the model has no `{}` clip; not animating",
                clip.name()
            );
            return;
        };
        let parent = if clip.is_additive() { add } else { body };
        nodes.push(graph.add_clip(handle.clone(), 1.0, parent));
    }
    commands.insert_resource(KnightGraph {
        graph: graphs.add(graph),
        nodes: nodes.try_into().expect("CLIP_COUNT clips"),
    });
}

/// The animation target id of a knight joint (the path of names from the
/// model's `knight` node), as the glTF loader gives it.
pub fn joint_target(joint: &str) -> AnimationTargetId {
    let path: &[&str] = match joint {
        "Torso" => &["knight", "Torso"],
        "PivotLegL" | "PivotLegR" => &["knight", joint],
        _ => &["knight", "Torso", joint],
    };
    AnimationTargetId::from_iter(path.iter())
}
