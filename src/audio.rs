//! Slice F — the synthesized sound bank and playback.
//!
//! Every sound is generated in code at startup ([`bank`], [`synth`]) into in-memory
//! WAV bytes and loaded as Bevy [`AudioSource`]s. Playback is driven by the gameplay
//! messages: the player's own sounds are non-spatial; everything happening in the
//! world (the dummy's steps, pieces) is spatial, heard through a
//! [`SpatialListener`] on the main camera. A voice cap keeps the mixer cheap.
//!
//! Milestone 2 ("Spellbound") replaced the bank with magical, cartoony cues: spell
//! zaps, bonks and sparkles, brick clunks and plank thocks, glassy shield crashes
//! and a poof-and-slide-whistle elimination. Every cue is mastered to a documented
//! loudness target (see [`bank`]) and mixed by [`Sfx::mix_db`]; hit confirmation
//! sits above the player's own casts and ducks them on the frame it lands.
//!
//! Milestone 4, chunk 3 (D107, D119): an adaptive orchestral score
//! ([`music`]), and richer effects. Every cue is layered (transient, body,
//! tail) and gets a generated room ([`reverb`]) baked in when the bank renders
//! on its background thread, so the rooms cost nothing per frame; repeated
//! cues vary in pitch ([`pitch_spread`]); the guns are layered shots (the
//! rifle a mechanism click, a magic body and a tail; the pump a low whoomp, a
//! chime and a tail). The Music and Effects sliders sit under the master
//! volume.

pub mod ambience;
pub mod bank;
pub mod barks;
pub mod celesta;
pub mod loudness;
pub mod music;
pub mod reverb;
pub mod spatial;
pub mod synth;
pub mod voice;
pub mod wand;

use crate::{
    building::Piece,
    fx::{
        armor::ArmorClattered,
        kills::{Callout, KillConfirmed, KillFeedbackSet, KillFeedbackStats},
    },
    hud::{FrameStartTick, HitFeedbackStats},
    render::MainCamera,
    shared::{
        ActiveTool, DamageDealt, DamageTarget, Eliminated, GameCue, PieceChange, PieceChanged,
        PieceKind, Player, ShotFired, WeaponKind,
    },
    tuning::Tuning,
    viewmodel::{WeaponBeat, WeaponCue},
};
use bevy::{
    audio::{AudioSinkPlayback, SpatialScale, Volume},
    prelude::*,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AudioTuning {
    pub master_volume: f32,
    pub muted: bool,
    /// Most sounds playing at once; lower-priority voices are stolen first.
    pub max_voices: u32,
    /// Distance scale for spatial sounds: full volume within `1 / spatial_scale`
    /// meters, then inverse-square falloff.
    pub spatial_scale: f32,
    pub weapons_volume: f32,
    pub hits_volume: f32,
    pub building_volume: f32,
    pub movement_volume: f32,
    /// Delay from a pump shot to its rack (s).
    pub pump_rack_delay: f32,
    /// Gain on the player's own weapon sounds that start on the same frame as one
    /// of their hit confirmations, so the hit cuts through (0.7 ≈ −3 dB).
    pub hit_duck: f32,
    /// The score's level under the master volume (Settings → Audio → Music;
    /// M4). Silent at 0.
    pub music_volume: f32,
    /// The effects' level under the master volume (Settings → Audio →
    /// Effects; M4). Silent at 0.
    pub effects_volume: f32,
}

impl Default for AudioTuning {
    fn default() -> Self {
        Self {
            master_volume: 0.8,
            muted: false,
            max_voices: 24,
            spatial_scale: 0.08,
            weapons_volume: 1.0,
            hits_volume: 1.0,
            building_volume: 1.0,
            movement_volume: 1.0,
            pump_rack_delay: 0.34,
            hit_duck: 0.7,
            music_volume: 0.8,
            effects_volume: 1.0,
        }
    }
}

impl AudioTuning {
    /// Master gain actually applied (0 when muted).
    pub fn effective_master(&self) -> f32 {
        if self.muted {
            0.0
        } else {
            self.master_volume.clamp(0.0, 1.0)
        }
    }

    /// The effects' gain: master × Effects (exactly 0 when either is 0 or
    /// muted).
    pub fn effects_gain(&self) -> f32 {
        self.effective_master() * self.effects_volume.clamp(0.0, 1.0)
    }

    fn category_gain(&self, category: SfxCategory) -> f32 {
        match category {
            SfxCategory::Weapons => self.weapons_volume,
            SfxCategory::Hits | SfxCategory::Confirm => self.hits_volume,
            SfxCategory::Building => self.building_volume,
            SfxCategory::Movement => self.movement_volume,
            // The run's beats follow the master volume only.
            SfxCategory::Run => 1.0,
            // The knights' voices and the ambience follow Effects.
            SfxCategory::Voice | SfxCategory::Ambience => 1.0,
        }
        .max(0.0)
    }
}

// ---------------------------------------------------------------------------
// The sound bank
// ---------------------------------------------------------------------------

/// Every sound effect in the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sfx {
    /// Rifle spell: a bright falling zap with a sparkle shimmer.
    RifleShot,
    /// Pump spell: "whoomp-zap" with a chime burst.
    PumpShot,
    /// Gold-ring whirr and metallic tick.
    PumpRack,
    /// Crystal clink as the dim crystal pops out.
    RifleMagOut,
    /// Rising hum as the fresh crystal slots in.
    RifleMagIn,
    /// Crystal-shard tink.
    PumpShell,
    /// Soft magical swish.
    WeaponSwitch,
    /// Body hit: bonk plus sparkle.
    HitTick,
    /// Headshot: bonk plus a bright ding.
    HeadshotDing,
    /// Glassy tick.
    ShieldHit,
    /// Glass crash plus chime.
    ShieldBreak,
    /// Cartoon poof plus a slide whistle going down.
    Elimination,
    /// Wall placed: brick clunk.
    BrickPlace,
    /// Floor or ramp placed: wooden thock.
    PlankPlace,
    BrickCrack,
    PlankCrack,
    /// Wall broken: brick crumble.
    BrickBreak,
    /// Floor or ramp broken: wood splinter.
    PlankBreak,
    /// Invalid placement: soft cartoon bwomp.
    Rejected,
    /// Soft grass step.
    Footstep,
    /// Light "boing".
    Jump,
    /// Soft thud.
    Land,
    /// Swish.
    Slide,
    /// A knight's orb leaving his wand: a flame "fwoom" (spatial).
    OrbCast,
    /// An orb passing within 3 m of the player: a doppler whoosh (spatial).
    OrbWhoosh,
    /// An orb hitting the player: a crunchy bonk.
    OrbBonk,
    /// A knight off-screen starting his wind-up: a rising charge from his
    /// direction (D76's off-screen warning).
    WandWarning,
    /// Drinking a shield potion: a glassy gulp and a chime (D80).
    PotionGulp,
    /// A wave cleared: a bright rising jingle as the break starts (D79).
    WaveCleared,
    /// The next wave coming: a two-note horn call.
    WaveStart,
    /// "NEW BEST!" on the results: a little fanfare and a sparkle shower.
    NewBest,
    /// A drop ship's rune circle lighting: a hum rising to the landing (D82).
    ShipHum,
    /// A knight sliding down a drop ship's beam: a falling shimmer.
    ShipBeam,
    /// A knight knocked into the void: a yip and a falling slide whistle (D78).
    VoidYelp,
    /// The kill confirm (M4, D105): a bonk and a bright "cha-ching" of two
    /// coin chimes, over the hit sound.
    KillConfirm,
    /// A headshot rings the helmet: a bright metallic "ding" (M4).
    HelmetDing,
    /// Armor clattering on its first bounce (spatial; at most four at once).
    ArmorClatter,
    /// A multi-kill callout's sting: take 0 "Double!" .. 3 "Rampage!",
    /// rising and growing with the chain.
    MultiKill,
    /// A gun coming up (M4, D106): a cloth swish, the brass seating with a
    /// clack, and the crystal waking with a hum.
    WeaponDraw,
    /// Aiming down sights: a soft leather-and-brass shift with a faint
    /// rising glint.
    AdsIn,
    /// Leaving the sights: the shift, softer and falling.
    AdsOut,
    /// Rifle reload: the glass chamber slides open.
    ChamberOpen,
    /// Rifle reload: the glove grabs a fresh crystal.
    CrystalGrab,
    /// Rifle reload: the crystal clicks into its socket.
    CrystalSlot,
    /// Rifle reload: the chamber snaps shut.
    ChamberShut,
    /// Rifle reload done: the crystal charges up and flashes.
    CrystalCharge,
    /// Pump: the rack pulled back, the rings whirring up.
    RackPull,
    /// Pump: the rack slammed home.
    RackClack,
    /// A knight's "hup!" landing off a drop ship's beam (M4 chunk 6, D123).
    BarkHup,
    /// A knight's taunt on some wind-ups: "nyah-nyah!", "heh-heh-heh!",
    /// "bla-HAH!" (takes).
    BarkTaunt,
    /// A knight's yelp when hit: "ow!", "eep!", "oof!", "yip!" (takes).
    BarkYelp,
    /// The survivors' "hoo-HAH!" in the victory hop.
    BarkHooHah,
    /// A knight's "whaaaa…" as he falls into the void.
    BarkWhaaa,
    /// A knight's armored step on grass, on wood and on brick (spatial).
    KnightStepGrass,
    KnightStepWood,
    KnightStepBrick,
    /// A songbird near the trees (the ambience; spatial, takes).
    Birdsong,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SfxCategory {
    Weapons,
    Hits,
    Building,
    Movement,
    /// The run's own beats: potions, wave clears, the next wave, a new best.
    Run,
    /// Kill confirmation layered over the hits (M4): the kill chime and the
    /// helmet ding. They follow the hits volume.
    Confirm,
    /// The knights' voices (M4 chunk 6): under the guns, over their steps.
    Voice,
    /// The world's ambience one-shots (birdsong).
    Ambience,
}

impl Sfx {
    pub const ALL: [Sfx; 57] = [
        Sfx::RifleShot,
        Sfx::PumpShot,
        Sfx::PumpRack,
        Sfx::RifleMagOut,
        Sfx::RifleMagIn,
        Sfx::PumpShell,
        Sfx::WeaponSwitch,
        Sfx::HitTick,
        Sfx::HeadshotDing,
        Sfx::ShieldHit,
        Sfx::ShieldBreak,
        Sfx::Elimination,
        Sfx::BrickPlace,
        Sfx::PlankPlace,
        Sfx::BrickCrack,
        Sfx::PlankCrack,
        Sfx::BrickBreak,
        Sfx::PlankBreak,
        Sfx::Rejected,
        Sfx::Footstep,
        Sfx::Jump,
        Sfx::Land,
        Sfx::Slide,
        Sfx::OrbCast,
        Sfx::OrbWhoosh,
        Sfx::OrbBonk,
        Sfx::WandWarning,
        Sfx::PotionGulp,
        Sfx::WaveCleared,
        Sfx::WaveStart,
        Sfx::NewBest,
        Sfx::ShipHum,
        Sfx::ShipBeam,
        Sfx::VoidYelp,
        Sfx::KillConfirm,
        Sfx::HelmetDing,
        Sfx::ArmorClatter,
        Sfx::MultiKill,
        Sfx::WeaponDraw,
        Sfx::AdsIn,
        Sfx::AdsOut,
        Sfx::ChamberOpen,
        Sfx::CrystalGrab,
        Sfx::CrystalSlot,
        Sfx::ChamberShut,
        Sfx::CrystalCharge,
        Sfx::RackPull,
        Sfx::RackClack,
        Sfx::BarkHup,
        Sfx::BarkTaunt,
        Sfx::BarkYelp,
        Sfx::BarkHooHah,
        Sfx::BarkWhaaa,
        Sfx::KnightStepGrass,
        Sfx::KnightStepWood,
        Sfx::KnightStepBrick,
        Sfx::Birdsong,
    ];

    /// The cue for a piece event: walls are brick (clunk, crack, crumble); floors
    /// and ramps are wooden planks (thock, crack, splinter).
    pub fn for_piece(kind: PieceKind, change: PieceChange) -> Sfx {
        let brick = matches!(kind, PieceKind::Wall);
        match (change, brick) {
            (PieceChange::Placed, true) => Sfx::BrickPlace,
            (PieceChange::Placed, false) => Sfx::PlankPlace,
            (PieceChange::Cracked(_), true) => Sfx::BrickCrack,
            (PieceChange::Cracked(_), false) => Sfx::PlankCrack,
            (PieceChange::Destroyed, true) => Sfx::BrickBreak,
            (PieceChange::Destroyed, false) => Sfx::PlankBreak,
        }
    }

    /// Renders the sound's first take (mono, [`synth::SAMPLE_RATE`]).
    pub fn synthesize(self) -> Vec<f32> {
        self.synthesize_take(0)
    }

    /// Renders round-robin take `take` (wrapping at [`Sfx::takes`]) as it
    /// plays: the layered design with its room baked in, mastered to the
    /// cue's loudness target.
    pub fn synthesize_take(self, take: u32) -> Vec<f32> {
        let dry = self.dry_take(take);
        let wet = reverb::bake(&dry, &self.room());
        synth::Buffer { samples: wet }.master(self.spec().rms_db)
    }

    /// The room baked into the cue (M4): tight for the player's guns, bright
    /// and short for hits, dry stone for bricks, wooden for planks and
    /// movement, open air for the world's sounds, a small hall for the run's
    /// beats.
    pub fn room(self) -> reverb::Room {
        use Sfx::*;
        match self {
            RifleShot | PumpShot | PumpRack | RifleMagOut | RifleMagIn | PumpShell
            | WeaponSwitch | WeaponDraw | AdsIn | AdsOut | ChamberOpen | CrystalGrab
            | CrystalSlot | ChamberShut | CrystalCharge | RackPull | RackClack => {
                reverb::Room::TIGHT
            }
            HitTick | HeadshotDing | ShieldHit | ShieldBreak | Elimination | KillConfirm
            | HelmetDing => reverb::Room::HIT,
            BrickPlace | BrickCrack | BrickBreak => reverb::Room::STONE,
            PlankPlace | PlankCrack | PlankBreak | Rejected | Footstep | Jump | Land | Slide => {
                reverb::Room::WOOD
            }
            OrbCast | OrbWhoosh | OrbBonk | WandWarning | ShipHum | ShipBeam | VoidYelp
            | ArmorClatter => reverb::Room::WORLD,
            PotionGulp | WaveCleared | WaveStart | NewBest | MultiKill => reverb::Room::HALL,
            BarkHup | BarkTaunt | BarkYelp | BarkHooHah | BarkWhaaa | KnightStepGrass | Birdsong => {
                reverb::Room::WORLD
            }
            KnightStepWood => reverb::Room::WOOD,
            KnightStepBrick => reverb::Room::STONE,
        }
    }

    /// Big hits duck the music for 150 ms (M4): a kill, a headshot, a shield
    /// breaking, the player being hit by an orb.
    pub fn ducks_music(self) -> bool {
        matches!(
            self,
            Sfx::KillConfirm
                | Sfx::HeadshotDing
                | Sfx::ShieldBreak
                | Sfx::Elimination
                | Sfx::OrbBonk
        )
    }

    /// The dry design of take `take` (mastered, before its room).
    pub fn dry_take(self, take: u32) -> Vec<f32> {
        match self {
            Sfx::RifleShot => bank::rifle_shot(take),
            Sfx::PumpShot => bank::pump_shot(),
            Sfx::PumpRack => bank::pump_rack(),
            Sfx::RifleMagOut => bank::rifle_mag_out(),
            Sfx::RifleMagIn => bank::rifle_mag_in(),
            Sfx::PumpShell => bank::pump_shell(),
            Sfx::WeaponSwitch => bank::weapon_switch(),
            Sfx::HitTick => bank::hit_tick(),
            Sfx::HeadshotDing => bank::headshot_ding(),
            Sfx::ShieldHit => bank::shield_hit(),
            Sfx::ShieldBreak => bank::shield_break(),
            Sfx::Elimination => bank::elimination(),
            Sfx::BrickPlace => bank::brick_place(),
            Sfx::PlankPlace => bank::plank_place(),
            Sfx::BrickCrack => bank::brick_crack(),
            Sfx::PlankCrack => bank::plank_crack(),
            Sfx::BrickBreak => bank::brick_break(),
            Sfx::PlankBreak => bank::plank_break(),
            Sfx::Rejected => bank::rejected(),
            Sfx::Footstep => bank::footstep(take),
            Sfx::Jump => bank::jump(),
            Sfx::Land => bank::land(),
            Sfx::Slide => bank::slide(),
            Sfx::OrbCast => bank::orb_cast(),
            Sfx::OrbWhoosh => bank::orb_whoosh(),
            Sfx::OrbBonk => bank::orb_bonk(),
            Sfx::WandWarning => bank::wand_warning(),
            Sfx::PotionGulp => bank::potion_gulp(),
            Sfx::WaveCleared => bank::wave_cleared(),
            Sfx::WaveStart => bank::wave_start(),
            Sfx::NewBest => bank::new_best(),
            Sfx::ShipHum => bank::ship_hum(),
            Sfx::ShipBeam => bank::ship_beam(),
            Sfx::VoidYelp => bank::void_yelp(),
            Sfx::KillConfirm => bank::kill_confirm(),
            Sfx::HelmetDing => bank::helmet_ding(),
            Sfx::ArmorClatter => bank::armor_clatter(take),
            Sfx::MultiKill => bank::multi_kill(take),
            Sfx::WeaponDraw => bank::weapon_draw(),
            Sfx::AdsIn => bank::ads_in(),
            Sfx::AdsOut => bank::ads_out(),
            Sfx::ChamberOpen => bank::chamber_open(),
            Sfx::CrystalGrab => bank::crystal_grab(),
            Sfx::CrystalSlot => bank::crystal_slot(),
            Sfx::ChamberShut => bank::chamber_shut(),
            Sfx::CrystalCharge => bank::crystal_charge(),
            Sfx::RackPull => bank::rack_pull(),
            Sfx::RackClack => bank::rack_clack(),
            Sfx::BarkHup => voice::hup(bank::BARK_HUP.rms_db),
            Sfx::BarkTaunt => voice::taunt(take, bank::BARK_TAUNT.rms_db),
            Sfx::BarkYelp => voice::yelp(take, bank::BARK_YELP.rms_db),
            Sfx::BarkHooHah => voice::hoo_hah(bank::BARK_HOO_HAH.rms_db),
            Sfx::BarkWhaaa => voice::whaaa(bank::BARK_WHAAA.rms_db),
            Sfx::KnightStepGrass => bank::knight_step(bank::StepSurface::Grass, take),
            Sfx::KnightStepWood => bank::knight_step(bank::StepSurface::Wood, take),
            Sfx::KnightStepBrick => bank::knight_step(bank::StepSurface::Brick, take),
            Sfx::Birdsong => bank::birdsong(take),
        }
    }

    /// Round-robin takes in the bank (the fastest-repeating cues get several).
    /// The multi-kill sting's takes are its levels, picked, not cycled.
    pub fn takes(self) -> u32 {
        match self {
            Sfx::RifleShot => bank::RIFLE_VARIANTS,
            Sfx::Footstep => bank::FOOTSTEP_VARIANTS,
            Sfx::ArmorClatter => bank::CLATTER_VARIANTS,
            Sfx::MultiKill => bank::MULTI_KILL_LEVELS,
            Sfx::BarkTaunt => voice::TAUNT_TAKES,
            Sfx::BarkYelp => voice::YELP_TAKES,
            Sfx::KnightStepGrass | Sfx::KnightStepWood | Sfx::KnightStepBrick => {
                bank::KNIGHT_STEP_VARIANTS
            }
            Sfx::Birdsong => bank::BIRDSONG_VARIANTS,
            _ => 1,
        }
    }

    /// The sound's first take as a WAV file.
    pub fn wav(self) -> Vec<u8> {
        synth::encode_wav(&self.synthesize())
    }

    /// Length budget and loudness target (see [`bank`]).
    pub fn spec(self) -> bank::CueSpec {
        use Sfx::*;
        match self {
            RifleShot => bank::RIFLE_CAST,
            PumpShot => bank::PUMP_CAST,
            PumpRack => bank::PUMP_RACK,
            RifleMagOut => bank::RIFLE_MAG_OUT,
            RifleMagIn => bank::RIFLE_MAG_IN,
            PumpShell => bank::PUMP_SHELL,
            WeaponSwitch => bank::WEAPON_SWITCH,
            HitTick => bank::BODY_HIT,
            HeadshotDing => bank::HEADSHOT,
            ShieldHit => bank::SHIELD_HIT,
            ShieldBreak => bank::SHIELD_BREAK,
            Elimination => bank::ELIMINATION,
            BrickPlace => bank::BRICK_PLACE,
            PlankPlace => bank::PLANK_PLACE,
            BrickCrack => bank::BRICK_CRACK,
            PlankCrack => bank::PLANK_CRACK,
            BrickBreak => bank::BRICK_BREAK,
            PlankBreak => bank::PLANK_BREAK,
            Rejected => bank::REJECTED,
            Footstep => bank::FOOTSTEP,
            Jump => bank::JUMP,
            Land => bank::LAND,
            Slide => bank::SLIDE,
            OrbCast => bank::ORB_CAST,
            OrbWhoosh => bank::ORB_WHOOSH,
            OrbBonk => bank::ORB_BONK,
            WandWarning => bank::WAND_WARNING,
            PotionGulp => bank::POTION_GULP,
            WaveCleared => bank::WAVE_CLEARED,
            WaveStart => bank::WAVE_START,
            NewBest => bank::NEW_BEST,
            ShipHum => bank::SHIP_HUM,
            ShipBeam => bank::SHIP_BEAM,
            VoidYelp => bank::VOID_YELP,
            KillConfirm => bank::KILL_CONFIRM,
            HelmetDing => bank::HELMET_DING,
            ArmorClatter => bank::ARMOR_CLATTER,
            MultiKill => bank::MULTI_KILL,
            WeaponDraw => bank::WEAPON_DRAW,
            AdsIn => bank::ADS_IN,
            AdsOut => bank::ADS_OUT,
            ChamberOpen => bank::CHAMBER_OPEN,
            CrystalGrab => bank::CRYSTAL_GRAB,
            CrystalSlot => bank::CRYSTAL_SLOT,
            ChamberShut => bank::CHAMBER_SHUT,
            CrystalCharge => bank::CRYSTAL_CHARGE,
            RackPull => bank::RACK_PULL,
            RackClack => bank::RACK_CLACK,
            BarkHup => bank::BARK_HUP,
            BarkTaunt => bank::BARK_TAUNT,
            BarkYelp => bank::BARK_YELP,
            BarkHooHah => bank::BARK_HOO_HAH,
            BarkWhaaa => bank::BARK_WHAAA,
            KnightStepGrass | KnightStepWood | KnightStepBrick => bank::KNIGHT_STEP,
            Birdsong => bank::BIRDSONG,
        }
    }

    pub fn category(self) -> SfxCategory {
        use Sfx::*;
        match self {
            RifleShot | PumpShot | PumpRack | RifleMagOut | RifleMagIn | PumpShell
            | WeaponSwitch | WeaponDraw | AdsIn | AdsOut | ChamberOpen | CrystalGrab
            | CrystalSlot | ChamberShut | CrystalCharge | RackPull | RackClack => {
                SfxCategory::Weapons
            }
            HitTick | HeadshotDing | ShieldHit | ShieldBreak | Elimination => SfxCategory::Hits,
            BrickPlace | PlankPlace | BrickCrack | PlankCrack | BrickBreak | PlankBreak
            | Rejected => SfxCategory::Building,
            Footstep | Jump | Land | Slide => SfxCategory::Movement,
            // The knights' casts sit with the guns (the weapons volume).
            OrbCast | OrbWhoosh | OrbBonk | WandWarning => SfxCategory::Weapons,
            PotionGulp | WaveCleared | WaveStart | NewBest => SfxCategory::Run,
            // The ships and the void sit with the knights' casts.
            ShipHum | ShipBeam | VoidYelp => SfxCategory::Weapons,
            KillConfirm | HelmetDing => SfxCategory::Confirm,
            // The knights' armor sits with their casts; the callout's sting
            // with the run's beats.
            ArmorClatter => SfxCategory::Weapons,
            MultiKill => SfxCategory::Run,
            BarkHup | BarkTaunt | BarkYelp | BarkHooHah | BarkWhaaa => SfxCategory::Voice,
            KnightStepGrass | KnightStepWood | KnightStepBrick => SfxCategory::Movement,
            Birdsong => SfxCategory::Ambience,
        }
    }

    /// The mix: how loud the cue plays in game (short-term RMS, dBFS) before master
    /// volume, category volume and distance. Hit confirmation sits 4–5 dB above
    /// the rifle (which fires six times a second) and 1–2 dB above the pump, and
    /// the player's own casts also dip by `hit_duck` on the frame a hit lands;
    /// building sits with the rifle; handling sounds and movement sit well under.
    ///
    /// | Cues | Mix (dBFS) |
    /// |---|---|
    /// | headshot, shield break, elimination, new best, kill confirm | −13 |
    /// | wave cleared, helmet ding, multi-kill sting | −14 |
    /// | potion gulp | −15 |
    /// | next wave | −16 |
    /// | body hit, shield hit | −14 |
    /// | pump cast, orb bonk (hitting you), off-screen wand warning | −15 |
    /// | piece breaks | −16 |
    /// | orb whoosh, void yelp | −17 |
    /// | rifle cast, piece places and cracks | −18 |
    /// | orb cast (a knight's fwoom), ship hum, the rack's clack | −19 |
    /// | pump rack, reload, rejected, ship beam, armor clatter, the reload's slot, shut and charge, the rack's pull | −20 |
    /// | pump shell, weapon draw, the chamber opening | −21 |
    /// | weapon switch, ADS in and out, the crystal grab | −22 |
    /// | land, slide | −22 |
    /// | jump | −26 |
    /// | footstep | −27 |
    /// | a knight's whaaa (M4 chunk 6) | −21 |
    /// | a knight's yelp, hup, hoo-hah | −22 |
    /// | a knight's taunt | −23 |
    /// | a knight's step | −26 |
    /// | birdsong | −30 |
    pub fn mix_db(self) -> f32 {
        use Sfx::*;
        match self {
            HeadshotDing | ShieldBreak | Elimination | NewBest | KillConfirm => -13.0,
            HitTick | ShieldHit | WaveCleared | HelmetDing | MultiKill => -14.0,
            PumpShot | OrbBonk | WandWarning | PotionGulp => -15.0,
            BrickBreak | PlankBreak | WaveStart => -16.0,
            OrbWhoosh | VoidYelp => -17.0,
            RifleShot | BrickPlace | PlankPlace | BrickCrack | PlankCrack => -18.0,
            OrbCast | ShipHum => -19.0,
            RackClack => -19.0,
            PumpRack | RifleMagOut | RifleMagIn | Rejected | ShipBeam | ArmorClatter
            | CrystalSlot | ChamberShut | CrystalCharge | RackPull => -20.0,
            PumpShell | WeaponDraw | ChamberOpen => -21.0,
            WeaponSwitch | AdsIn | AdsOut | CrystalGrab | Land | Slide => -22.0,
            Jump => -26.0,
            Footstep => -27.0,
            BarkWhaaa => -21.0,
            BarkYelp | BarkHup | BarkHooHah => -22.0,
            BarkTaunt => -23.0,
            KnightStepGrass | KnightStepWood | KnightStepBrick => -26.0,
            Birdsong => -30.0,
        }
    }

    /// Mix gain before master and category volume: what takes the cue from its
    /// mastered loudness to its [`Sfx::mix_db`].
    pub fn base_volume(self) -> f32 {
        synth::db_to_gain(self.mix_db() - self.spec().rms_db).min(1.0)
    }

    /// Voice-stealing priority: higher survives. Hit confirmation matters most,
    /// with the fairness cues (being hit, the off-screen warning) beside it.
    pub fn priority(self) -> u8 {
        if matches!(self, Sfx::OrbBonk | Sfx::WandWarning) {
            return 3;
        }
        // A knight's steps give way first.
        if matches!(
            self,
            Sfx::KnightStepGrass | Sfx::KnightStepWood | Sfx::KnightStepBrick
        ) {
            return 0;
        }
        match self.category() {
            SfxCategory::Hits => 3,
            // The kill layers ride on their hit (which is protected).
            SfxCategory::Weapons
            | SfxCategory::Building
            | SfxCategory::Run
            | SfxCategory::Confirm => 2,
            SfxCategory::Movement | SfxCategory::Voice => 1,
            SfxCategory::Ambience => 0,
        }
    }
}

/// Gain for a sound starting this frame: the player's own weapon sounds dip to
/// `duck` when one of the player's hit confirmations starts on the same frame
/// (hitscan hits land with their shot), so the bonk and sparkle cut through.
pub fn duck_gain(sfx: Sfx, own: bool, own_hit_this_frame: bool, duck: f32) -> f32 {
    if own && own_hit_this_frame && sfx.category() == SfxCategory::Weapons {
        duck.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

/// Every cue's takes as WAV files, indexed by [`Sfx`] and then take. This is all
/// the synthesis work done at startup (on the `sound-bank` thread).
pub fn render_bank() -> RenderedBank {
    Sfx::ALL
        .iter()
        .map(|sfx| {
            (0..sfx.takes())
                .map(|take| synth::encode_wav(&sfx.synthesize_take(take)))
                .collect()
        })
        .collect()
}

/// Handles to every synthesized sound, indexed by [`Sfx`] and then take.
#[derive(Resource, Debug, Clone)]
pub struct SoundBank {
    handles: Vec<Vec<Handle<AudioSource>>>,
}

impl SoundBank {
    pub fn get(&self, sfx: Sfx) -> Handle<AudioSource> {
        self.take(sfx, 0)
    }

    /// Round-robin take `take` of `sfx` (wraps).
    pub fn take(&self, sfx: Sfx, take: u32) -> Handle<AudioSource> {
        let takes = &self.handles[sfx as usize];
        takes[take as usize % takes.len()].clone()
    }
}

// ---------------------------------------------------------------------------
// Voice management (pure)
// ---------------------------------------------------------------------------

/// What to do with a new sound when the voice cap is reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceDecision<T> {
    Play,
    /// Stop this existing voice, then play.
    Steal(T),
    Drop,
}

/// Picks the voice to steal for a new sound of `priority`: the oldest voice of the
/// lowest priority, if that priority is not higher than the new sound's.
/// `voices` are `(id, priority, started_seconds)`.
pub fn choose_voice<T: Copy>(
    voices: &[(T, u8, f64)],
    max_voices: usize,
    priority: u8,
) -> VoiceDecision<T> {
    if voices.len() < max_voices.max(1) {
        return VoiceDecision::Play;
    }
    let victim = voices
        .iter()
        .min_by(|a, b| a.1.cmp(&b.1).then(a.2.total_cmp(&b.2)));
    match victim {
        Some(&(id, p, _)) if p <= priority => VoiceDecision::Steal(id),
        _ => VoiceDecision::Drop,
    }
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct GameAudioPlugin;

impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        // Synthesis (and the rooms baked into every cue) starts now, on its own
        // thread, so it overlaps the renderer and window setup; the bank is
        // filed the frame it's done and never waited on (launch, W8, M4).
        // The gun's handling sounds follow the viewmodel's animation beats
        // (chunk 2) when it's there; headless apps fall back to the reload
        // and switch cues.
        if app.is_plugin_added::<crate::viewmodel::ViewmodelPlugin>() {
            app.init_resource::<WeaponBeatSounds>();
        }
        app.init_resource::<PlayQueue>()
            .add_message::<KillConfirmed>()
            .add_message::<ArmorClattered>()
            .add_message::<WeaponCue>()
            .insert_resource(SoundBankJob::start())
            .add_systems(Startup, hold_boot_for_the_bank)
            .add_observer(attach_listener);
        music::build(app);
        app.add_systems(
            Update,
            (
                file_sound_bank,
                queue_combat_sounds,
                queue_kill_sounds,
                queue_piece_sounds,
                queue_cue_sounds,
                queue_run_sounds,
                play_queued,
                apply_live_volume,
                music::receive_music,
                music::drive_music,
            )
                .chain()
                .after(KillFeedbackSet),
        )
        // The viewmodel announces its beats in PostUpdate: they play the same
        // frame (Bevy starts sounds after transform propagation).
        .add_systems(
            PostUpdate,
            (queue_weapon_beats, play_queued)
                .chain()
                .after(crate::viewmodel::ViewmodelSet)
                .before(bevy::transform::TransformSystems::Propagate),
        );
        wand::build(app);
        barks::build(app);
        ambience::build(app);
    }
}

/// A playing sound effect (public so the session log can count voices).
#[derive(Component, Debug, Clone, Copy)]
pub struct Voice {
    sfx: Sfx,
    priority: u8,
    started: f64,
    /// Volume before master volume (so master changes apply live).
    level: f32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PlayRequest {
    sfx: Sfx,
    /// World position for spatial sounds; `None` plays it as the player's own.
    at: Option<Vec3>,
    gain: f32,
    speed: f32,
    /// Round-robin take.
    take: u32,
    /// Real time (s) at which to start.
    when: f64,
}

#[derive(Resource)]
pub(crate) struct PlayQueue {
    pending: Vec<PlayRequest>,
    /// An off-screen warning started: stop the knights' barks sounding now
    /// (M4 chunk 6, [`barks`]).
    cut_voices: bool,
    /// Counter for deterministic pitch variation.
    variation: u32,
    /// Next round-robin take per [`Sfx`].
    takes: [u32; Sfx::ALL.len()],
}

// More cues than `Default` covers for arrays (32).
impl Default for PlayQueue {
    fn default() -> Self {
        Self {
            pending: Vec::new(),
            cut_voices: false,
            variation: 0,
            takes: [0; Sfx::ALL.len()],
        }
    }
}

impl Voice {
    /// The cue this voice plays.
    pub fn sfx(&self) -> Sfx {
        self.sfx
    }
}

impl PlayQueue {
    pub(crate) fn push(&mut self, sfx: Sfx, at: Option<Vec3>, now: f64) {
        self.push_with(sfx, at, 1.0, 0.0, now);
    }

    pub(crate) fn push_with(
        &mut self,
        sfx: Sfx,
        at: Option<Vec3>,
        gain: f32,
        delay: f64,
        now: f64,
    ) {
        // Small, deterministic pitch variation so repeats never sound machine-gunned.
        self.variation = self.variation.wrapping_add(1);
        let speed = pitch_variation(sfx, self.variation);
        let take = self.takes[sfx as usize];
        self.takes[sfx as usize] = (take + 1) % sfx.takes().max(1);
        self.pending.push(PlayRequest {
            sfx,
            at,
            gain,
            speed,
            take,
            when: now + delay,
        });
    }

    /// Plays take `take` of `sfx` at playback `speed` (a knight's own pitch).
    pub(crate) fn push_voice(
        &mut self,
        sfx: Sfx,
        at: Option<Vec3>,
        speed: f32,
        take: u32,
        delay: f64,
        now: f64,
    ) {
        self.pending.push(PlayRequest {
            sfx,
            at,
            gain: 1.0,
            speed,
            take: take % sfx.takes().max(1),
            when: now + delay,
        });
    }

    /// Plays a chosen take of `sfx` (the multi-kill sting's level).
    pub(crate) fn push_take(&mut self, sfx: Sfx, at: Option<Vec3>, take: u32, now: f64) {
        self.push(sfx, at, now);
        if let Some(last) = self.pending.last_mut() {
            last.take = take % sfx.takes().max(1);
        }
    }
}

/// How far a cue's pitch varies from play to play (± fraction, M4): steps and
/// the other movement sounds, body and shield hits ±4%, piece placements and
/// the rest of building ±5%, the guns ±3%. The signature cues (the headshot
/// ding, kill confirmation, the run's beats) keep their pitch.
pub fn pitch_spread(sfx: Sfx) -> f32 {
    match sfx {
        Sfx::HitTick | Sfx::ShieldHit => 0.04,
        _ => match sfx.category() {
            SfxCategory::Movement => 0.04,
            SfxCategory::Building => 0.05,
            SfxCategory::Weapons => 0.03,
            SfxCategory::Hits | SfxCategory::Run | SfxCategory::Confirm => 0.0,
            // A knight's own pitch and a yelp's jitter are set by `barks`.
            SfxCategory::Voice => 0.0,
            SfxCategory::Ambience => 0.05,
        },
    }
}

/// The playback speed for the `n`th sound queued: a seeded hash of `n`, spread
/// evenly over ± [`pitch_spread`].
pub fn pitch_variation(sfx: Sfx, n: u32) -> f32 {
    let h = n.wrapping_mul(2_654_435_761) >> 16;
    1.0 + pitch_spread(sfx) * ((h % 1001) as f32 / 500.0 - 1.0)
}

/// Present when the viewmodel drives the gun's handling sounds with its
/// animation beats ([`WeaponCue`], chunk 2): the draw, ADS, every reload beat
/// and the rack.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct WeaponBeatSounds;

/// The layered sound for each of the gun's animation beats (D106).
pub fn beat_sound(beat: WeaponBeat) -> Sfx {
    match beat {
        WeaponBeat::Draw => Sfx::WeaponDraw,
        WeaponBeat::AdsIn => Sfx::AdsIn,
        WeaponBeat::AdsOut => Sfx::AdsOut,
        WeaponBeat::ChamberOpen => Sfx::ChamberOpen,
        WeaponBeat::CrystalPop => Sfx::RifleMagOut,
        WeaponBeat::CrystalGrab => Sfx::CrystalGrab,
        WeaponBeat::CrystalSlot => Sfx::CrystalSlot,
        WeaponBeat::ChamberShut => Sfx::ChamberShut,
        WeaponBeat::CrystalCharged => Sfx::CrystalCharge,
        WeaponBeat::ShardPush => Sfx::PumpShell,
        WeaponBeat::RackPull => Sfx::RackPull,
        WeaponBeat::RackClack => Sfx::RackClack,
    }
}

/// The player's gun, beat by beat, as the viewmodel animates it.
fn queue_weapon_beats(
    time: Res<Time<Real>>,
    beats: Option<Res<WeaponBeatSounds>>,
    mut cues: MessageReader<WeaponCue>,
    mut queue: ResMut<PlayQueue>,
) {
    if beats.is_none() {
        cues.clear();
        return;
    }
    let now = time.elapsed_secs_f64();
    for cue in cues.read() {
        queue.push(beat_sound(cue.beat), None, now);
    }
}

/// The sounds a confirmed kill adds over its hit (M4, D105): the kill chime,
/// and a multi-kill sting at the chain's level (0 double .. 3 rampage).
pub fn kill_cues(kill: &KillConfirmed) -> (Sfx, Option<(Sfx, u32)>) {
    let sting = Callout::for_chain(kill.chain).map(|c| (Sfx::MultiKill, c.level()));
    (Sfx::KillConfirm, sting)
}

/// Every cue's takes as WAV files (see [`render_bank`]).
pub type RenderedBank = Vec<Vec<Vec<u8>>>;

/// The sound bank's synthesis thread (named `sound-bank`), started when the
/// plugin builds.
#[derive(Resource)]
pub struct SoundBankJob {
    thread: std::sync::Mutex<Option<std::thread::JoinHandle<RenderedBank>>>,
    done: std::sync::Mutex<Option<RenderedBank>>,
}

impl SoundBankJob {
    fn start() -> Self {
        let thread = std::thread::Builder::new()
            .name("sound-bank".into())
            .spawn(render_bank)
            .ok();
        Self {
            thread: std::sync::Mutex::new(thread),
            done: std::sync::Mutex::new(None),
        }
    }

    /// The synthesis thread's name (none if it couldn't start).
    pub fn thread_name(&self) -> Option<String> {
        let thread = self.thread.lock().ok()?;
        thread.as_ref()?.thread().name().map(str::to_owned)
    }

    /// Waits for the bank to finish (tests and probes that want it filed on
    /// the next frame).
    pub fn wait(&self) {
        let handle = self.thread.lock().ok().and_then(|mut t| t.take());
        if let Some(bank) = handle.and_then(|h| h.join().ok())
            && let Ok(mut done) = self.done.lock()
        {
            *done = Some(bank);
        }
    }

    /// The bank once the thread is done: `Some(None)` if it failed.
    fn take_if_done(&self) -> Option<Option<RenderedBank>> {
        let running = self
            .thread
            .lock()
            .map(|t| t.as_ref().is_some_and(|h| !h.is_finished()))
            .unwrap_or(false);
        if running {
            return None;
        }
        self.wait();
        Some(self.done.lock().ok().and_then(|mut d| d.take()))
    }
}

/// Play can't start before the bank is filed (it normally is, long before
/// the models and pipelines are ready).
const BANK_GATE: &str = "sound-bank";

fn hold_boot_for_the_bank(gate: Option<ResMut<crate::app::BootGate>>) {
    if let Some(mut gate) = gate {
        gate.hold(BANK_GATE);
    }
}

/// Files the bank the frame its thread finishes; nothing blocks on it. If the
/// thread couldn't start (or panicked), the bank is synthesized here instead.
fn file_sound_bank(
    mut commands: Commands,
    mut sources: ResMut<Assets<AudioSource>>,
    job: Option<Res<SoundBankJob>>,
    gate: Option<ResMut<crate::app::BootGate>>,
) {
    let Some(ready) = job.and_then(|job| job.take_if_done()) else {
        return;
    };
    let bank = ready.unwrap_or_else(render_bank);
    commands.remove_resource::<SoundBankJob>();
    let handles = bank
        .into_iter()
        .map(|takes| {
            takes
                .into_iter()
                .map(|wav| sources.add(AudioSource { bytes: wav.into() }))
                .collect()
        })
        .collect();
    commands.insert_resource(SoundBank { handles });
    if let Some(mut gate) = gate {
        gate.release(BANK_GATE);
    }
}

fn attach_listener(add: On<Add, MainCamera>, mut commands: Commands) {
    commands
        .entity(add.entity)
        .insert(spatial::listener());
}

#[allow(clippy::too_many_arguments)]
fn queue_combat_sounds(
    time: Res<Time<Real>>,
    tuning: Res<Tuning>,
    beats: Option<Res<WeaponBeatSounds>>,
    player: Option<Single<Entity, With<Player>>>,
    start_tick: Option<Res<FrameStartTick>>,
    mut stats: Option<ResMut<HitFeedbackStats>>,
    pieces: Query<&Piece>,
    mut shots: MessageReader<ShotFired>,
    mut damage: MessageReader<DamageDealt>,
    mut eliminated: MessageReader<Eliminated>,
    mut queue: ResMut<PlayQueue>,
) {
    let now = time.elapsed_secs_f64();
    let player = player.map(|p| *p);
    let mine = |e: Option<Entity>| e.is_some() && e == player;
    for shot in shots.read() {
        let at = (!mine(Some(shot.shooter))).then_some(shot.origin);
        match shot.weapon {
            WeaponKind::Rifle => queue.push(Sfx::RifleShot, at, now),
            WeaponKind::Pump => {
                queue.push(Sfx::PumpShot, at, now);
                // The player's own rack follows the viewmodel's beats.
                if at.is_some() || beats.is_none() {
                    let delay = tuning.audio.pump_rack_delay.max(0.0) as f64;
                    queue.push_with(Sfx::PumpRack, at, 1.0, delay, now);
                }
            }
        }
    }
    let frame_start = start_tick.map(|t| t.0).unwrap_or(0);
    let mut piece_knock = false;
    for hit in damage.read() {
        if !mine(hit.source) {
            continue;
        }
        match hit.target_kind {
            DamageTarget::Character => {
                let base = if hit.headshot {
                    Sfx::HeadshotDing
                } else if hit.to_shield > 0.0 && !hit.shield_broke {
                    Sfx::ShieldHit
                } else {
                    Sfx::HitTick
                };
                queue.push(base, None, now);
                // The helmet rings (M4).
                if hit.headshot {
                    queue.push(Sfx::HelmetDing, None, now);
                }
                if hit.shield_broke {
                    queue.push(Sfx::ShieldBreak, None, now);
                }
                if let Some(stats) = stats.as_mut()
                    && hit.tick > frame_start
                {
                    stats.sounds_same_frame += 1;
                }
            }
            DamageTarget::Piece if !piece_knock => {
                // A quiet, quick knock in the piece's material so shooting it has
                // weight. A piece this shot destroyed is already gone; its break
                // sound covers it.
                if let Ok(piece) = pieces.get(hit.target) {
                    piece_knock = true;
                    let knock = Sfx::for_piece(piece.kind, PieceChange::Placed);
                    queue.push_with(knock, Some(hit.point), 0.35, 0.0, now);
                }
            }
            DamageTarget::Piece => {}
        }
    }
    for kill in eliminated.read() {
        if mine(kill.by) {
            queue.push(Sfx::Elimination, None, now);
        }
    }
}

/// The kill chime and multi-kill stings on the kill's frame, and the armor's
/// clatter (already capped at four voices by `fx::armor`).
fn queue_kill_sounds(
    time: Res<Time<Real>>,
    start_tick: Option<Res<FrameStartTick>>,
    mut stats: Option<ResMut<KillFeedbackStats>>,
    mut kills: MessageReader<KillConfirmed>,
    mut clatters: MessageReader<ArmorClattered>,
    mut queue: ResMut<PlayQueue>,
) {
    let now = time.elapsed_secs_f64();
    let frame_start = start_tick.map_or(0, |t| t.0);
    for kill in kills.read() {
        let (chime, sting) = kill_cues(kill);
        queue.push(chime, None, now);
        if let Some((sting, level)) = sting {
            queue.push_take(sting, None, level, now);
        }
        if let Some(stats) = stats.as_mut() {
            stats.sounds_same_frame += u32::from(kill.tick > frame_start);
        }
    }
    for clatter in clatters.read() {
        queue.push(Sfx::ArmorClatter, Some(clatter.at), now);
    }
}

fn queue_piece_sounds(
    time: Res<Time<Real>>,
    mut changes: MessageReader<PieceChanged>,
    mut queue: ResMut<PlayQueue>,
) {
    let now = time.elapsed_secs_f64();
    for change in changes.read() {
        queue.push(
            Sfx::for_piece(change.kind, change.change),
            Some(change.center),
            now,
        );
    }
}

fn queue_cue_sounds(
    time: Res<Time<Real>>,
    beats: Option<Res<WeaponBeatSounds>>,
    player: Option<Single<Entity, With<Player>>>,
    transforms: Query<&Transform>,
    grunts: Query<(), With<crate::grunt::Grunt>>,
    mut cues: MessageReader<GameCue>,
    mut queue: ResMut<PlayQueue>,
) {
    let now = time.elapsed_secs_f64();
    let player = player.map(|p| *p);
    for cue in cues.read() {
        // The knights' own steps follow their run clip ([`barks`]).
        if let GameCue::Footstep { who } = *cue
            && grunts.contains(who)
        {
            continue;
        }
        let (who, sfx) = match *cue {
            GameCue::Jump { who } => (who, Sfx::Jump),
            GameCue::Land { who, speed } => {
                if speed < 2.0 {
                    continue;
                }
                (who, Sfx::Land)
            }
            GameCue::Footstep { who } => (who, Sfx::Footstep),
            GameCue::SlideStart { who } => (who, Sfx::Slide),
            GameCue::ReloadStart { who, weapon } => match weapon {
                WeaponKind::Rifle => (who, Sfx::RifleMagOut),
                // The pump's reload is heard shell by shell.
                WeaponKind::Pump => continue,
            },
            GameCue::ReloadShell { who } => (who, Sfx::PumpShell),
            GameCue::ReloadDone { who, weapon } => match weapon {
                WeaponKind::Rifle => (who, Sfx::RifleMagIn),
                WeaponKind::Pump => (who, Sfx::PumpRack),
            },
            // A gun comes up with its draw (D106); a building piece swishes.
            GameCue::WeaponSwitch { who, tool } => match tool {
                ActiveTool::Weapon(_) => (who, Sfx::WeaponDraw),
                ActiveTool::Build(_) => (who, Sfx::WeaponSwitch),
            },
            GameCue::AdsChanged { who, ads } => (who, if ads { Sfx::AdsIn } else { Sfx::AdsOut }),
            GameCue::PlacementRejected { who } | GameCue::EditRejected { who } => {
                (who, Sfx::Rejected)
            }
            // An edit clicks into place.
            GameCue::PieceEdited { who, .. } => (who, Sfx::WeaponSwitch),
            GameCue::Respawned { .. } => continue,
            // The wand's cast and off-screen warning need the view and the
            // wand tip: `wand::queue_wand_sounds` plays them.
            GameCue::WandWindup { .. } | GameCue::OrbFired { .. } => continue,
            // Drinking a potion gulps and chimes; a drop is only seen.
            GameCue::PotionPicked { who, .. } => (who, Sfx::PotionGulp),
            GameCue::PotionDropped { .. } => continue,
            // The void yelp is voiced with the ships (`waves::ships_visuals`).
            GameCue::VoidFall { .. } => continue,
        };
        let own = Some(who) == player;
        // With the viewmodel, the player's gun handling plays on its beats
        // ([`queue_weapon_beats`]) instead.
        let handling = matches!(
            cue,
            GameCue::ReloadStart { .. }
                | GameCue::ReloadShell { .. }
                | GameCue::ReloadDone { .. }
                | GameCue::AdsChanged { .. }
                | GameCue::WeaponSwitch {
                    tool: ActiveTool::Weapon(_),
                    ..
                }
        );
        if own && handling && beats.is_some() {
            continue;
        }
        if !own && sfx.category() != SfxCategory::Movement {
            // Other characters are heard moving; their gear stays quiet for now.
            continue;
        }
        let at = if own {
            None
        } else {
            match transforms.get(who) {
                Ok(t) => Some(t.translation),
                Err(_) => continue,
            }
        };
        queue.push(sfx, at, now);
    }
}

/// The run's beats from [`RunSummary`](crate::waves::RunSummary): a jingle
/// when a wave is cleared, a horn as the next one starts, and a fanfare when
/// the results show a new best. Once the score's orchestral stings are loaded
/// they play the wave start and the new best instead ([`music`]).
fn queue_run_sounds(
    time: Res<Time<Real>>,
    summary: Option<Res<crate::waves::RunSummary>>,
    music: Option<Res<music::MusicBank>>,
    mut last: Local<Option<(crate::waves::RunPhase, u32)>>,
    mut queue: ResMut<PlayQueue>,
) {
    let Some(summary) = summary else {
        *last = None;
        return;
    };
    let Some((was, was_wave)) = last.replace((summary.phase, summary.wave)) else {
        return;
    };
    let orchestral =
        |sting: music::Sting| music.as_ref().is_some_and(|m| m.is_loaded(sting.track()));
    match run_beat(was, was_wave, &summary) {
        Some(Sfx::WaveStart) if orchestral(music::Sting::RoundStart) => {}
        Some(Sfx::NewBest) if orchestral(music::Sting::NewBest) => {}
        Some(sfx) => queue.push(sfx, None, time.elapsed_secs_f64()),
        None => {}
    }
}

/// The run beat to play when the run goes from `was` (in wave `was_wave`) to
/// `now`, if any.
pub fn run_beat(
    was: crate::waves::RunPhase,
    was_wave: u32,
    now: &crate::waves::RunSummary,
) -> Option<Sfx> {
    use crate::waves::RunPhase::{Break, Fighting, Over};
    match (was, now.phase) {
        (Fighting, Break { .. }) => Some(Sfx::WaveCleared),
        (Break { .. }, Fighting) if now.wave > was_wave => Some(Sfx::WaveStart),
        (Over { .. }, _) => None,
        (_, Over { .. }) if now.new_best => Some(Sfx::NewBest),
        _ => None,
    }
}

/// Scratch lists [`play_queued`] reuses every frame, so playing sounds never
/// allocates once they've grown.
#[derive(Default)]
pub(crate) struct PlayScratch {
    due: Vec<PlayRequest>,
    active: Vec<(Entity, u8, f64)>,
    played_own: Vec<Sfx>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn play_queued(
    mut commands: Commands,
    time: Res<Time<Real>>,
    tuning: Res<Tuning>,
    bank: Option<Res<SoundBank>>,
    mut queue: ResMut<PlayQueue>,
    mut cues: Option<ResMut<music::MusicCues>>,
    mut scratch: Local<PlayScratch>,
    voices: Query<(Entity, &Voice)>,
) {
    // A warning cuts the knights' barks off (D76: it stays clearly audible).
    if std::mem::take(&mut queue.cut_voices) {
        for (entity, voice) in &voices {
            if voice.sfx.category() == SfxCategory::Voice {
                commands.entity(entity).despawn();
            }
        }
    }
    let Some(bank) = bank else {
        queue.pending.clear();
        return;
    };
    let now = time.elapsed_secs_f64();
    let audio = &tuning.audio;
    let gain = audio.effects_gain();
    let PlayScratch {
        due,
        active,
        played_own,
    } = &mut *scratch;
    due.clear();
    queue.pending.retain(|r| {
        if r.when <= now {
            due.push(*r);
            false
        } else {
            true
        }
    });
    if due.is_empty() {
        return;
    }
    // A big hit ducks the score (even with the effects turned down).
    if let Some(cues) = cues.as_mut()
        && due.iter().any(|r| r.sfx.ducks_music())
    {
        cues.big_hit = true;
    }
    if gain <= 0.0 {
        return;
    }
    active.clear();
    active.extend(voices.iter().map(|(e, v)| (e, v.priority, v.started)));
    // One of each non-spatial sound per frame is enough (two hits on one frame
    // shouldn't double the volume).
    played_own.clear();
    let own_hit = due
        .iter()
        .any(|r| r.at.is_none() && r.sfx.category() == SfxCategory::Hits);
    for request in due.iter() {
        if request.at.is_none() {
            if played_own.contains(&request.sfx) {
                continue;
            }
            played_own.push(request.sfx);
        }
        let priority = request.sfx.priority();
        match choose_voice(active, audio.max_voices as usize, priority) {
            VoiceDecision::Play => {}
            VoiceDecision::Steal(victim) => {
                commands.entity(victim).despawn();
                active.retain(|v| v.0 != victim);
            }
            VoiceDecision::Drop => continue,
        }
        let duck = duck_gain(request.sfx, request.at.is_none(), own_hit, audio.hit_duck);
        let level = request.sfx.base_volume()
            * audio.category_gain(request.sfx.category())
            * request.gain
            * duck;
        let mut settings = PlaybackSettings::DESPAWN
            .with_volume(Volume::Linear(level * gain))
            .with_speed(request.speed);
        let mut entity = commands.spawn((
            Name::new("Sfx"),
            AudioPlayer::new(bank.take(request.sfx, request.take)),
            Voice {
                sfx: request.sfx,
                priority,
                started: now,
                level,
            },
        ));
        if let Some(at) = request.at {
            settings = settings
                .with_spatial(true)
                .with_spatial_scale(SpatialScale::new(audio.spatial_scale.max(0.001)));
            entity.insert(Transform::from_translation(at));
        }
        entity.insert(settings);
        active.push((entity.id(), priority, now));
    }
}

/// Master volume, mute and the Effects slider apply to sounds already
/// playing, too.
fn apply_live_volume(
    tuning: Res<Tuning>,
    mut last: Local<Option<f32>>,
    mut sinks: Query<(
        &Voice,
        Option<&mut AudioSink>,
        Option<&mut SpatialAudioSink>,
    )>,
) {
    let master = tuning.audio.effects_gain();
    if *last == Some(master) {
        return;
    }
    *last = Some(master);
    for (voice, sink, spatial) in &mut sinks {
        let volume = Volume::Linear(voice.level * master);
        if let Some(mut sink) = sink {
            sink.set_volume(volume);
        }
        if let Some(mut sink) = spatial {
            sink.set_volume(volume);
        }
    }
}
