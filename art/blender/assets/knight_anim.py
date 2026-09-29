"""The knight's authored clips (docs/M4-SPEC.md → Chunk 4, D104).

Keyframed on the knight's named rigid parts (no skinned skeleton, D20): the
joint pivots `PivotLegL/R`, `PivotArmL/R`, `PivotHead`, `PivotCape`,
`PivotRobe` and the `Torso` (its rotation, and its position for the bob).
The hat's pivots stay procedural (src/knight.rs: the hat pop and the tip's
flop). Every clip is exported into `knight.glb` as one glTF animation,
keyed at `FPS` (a key per frame, so the exported samples are the authored
poses), and `build.py` lists them with their durations in the sidecar's
`clips`. The game plays them through one shared `AnimationGraph` (src/knight/
clips.rs), blended by velocity and state, with the springs on top.

| Clip | Length | Notes |
|---|---|---|
| Idle | 2.0 s loop | breathing: a small dip, the arms easing out, the robe swaying |
| Run | 0.6 s loop | a chibi shuffle-run: short quick steps, pumping fists, a shoulder twist, a dip at each footfall |
| StrafeL, StrafeR | 0.6 s loop | side steps toward his left (+X here) or right, the robe and cape trailing |
| WindUp | exactly 0.4 s | the gameplay wind-up: cock back, then a wide planted stance with the wand thrust forward; its last pose puts the wand's tip on the orb's launch point (`orb::WAND_TIP`) |
| FlinchHead, FlinchChest, FlinchL, FlinchR | 0.3 s | additive (rotations only, rest = identity): layered on whatever he is doing |
| BeamLand | 0.5 s | touchdown from the drop ship's beam: a dip and settle (the squash is the spring's) |
| VictoryHop | 0.5 s loop | D84's celebration: fists pumping, feet kicking back, a wiggle |
| VoidFall | 0.6 s loop | flung into the void: arms windmilling, legs kicking (the figure tumbles too) |
| DeathBack, DeathSpin, DeathCrumple | 0.35 s | the KO beat (`knight::KO_TIME`), while his armor flies off |
| Hero | 2.4 s loop | the main menu's pose (M4-V8): wand raised to the sky, fist on his hip |

**Hitbox fit.** The hitboxes never move with a clip. In every clip he can be
shot in (`FIT_CLIPS`), sampled every 50 ms, the body parts stay inside the
body capsule within 10 cm and the helmet and eyes inside the head sphere
within D121's 9 cm (`check_clips` at build time; `tests/knight.rs` samples the
exported glb the same way). That rules the motion: the torso never leans or
tilts (it would carry the big helmet off the head sphere), it only twists
about its own axis and dips; the head only turns; limbs swing within the
capsule. The wind-up's wand arm reaches out of it by design (the orb leaves
0.98 m in front of him, as in Milestone 3), so `GauntletR` is exempt in
`WindUp`. Deaths, the void fall and the menu pose are out of play and exempt.

Blender space: z up, the knight faces -y, +x is his left. For a limb
hanging below its pivot `pitch(+a)` swings it back and `roll(+a)` toward his
right; for the upright torso and head `pitch(+a)` tips the top forward;
`yaw(+a)` turns his front toward his left.
"""

import math

import bpy
from mathutils import Quaternion, Vector

from assets import knight as K

FPS = 60
JOINTS = ("Torso", "PivotLegL", "PivotLegR", "PivotArmL", "PivotArmR", "PivotHead",
          "PivotCape", "PivotRobe")
FIT_STEP = 0.05  # s between fit samples
BODY_CLIP_TOLERANCE = 0.10

# The wand (src/arena/visuals/wand.rs, src/orb.rs), in Blender space: the right
# shoulder, the grip in his right fist, the wand's rest direction (forward and
# 40 degrees down), its length as drawn, and the orb's launch point.
SHOULDER_R = Vector((-0.2, 0.0, 1.2))
GRIP_R = Vector((-0.266, -0.03, 0.69))
WAND_DIR = Vector((0.0, -math.cos(math.radians(40.0)), -math.sin(math.radians(40.0))))
WAND_LENGTH = 0.42 * 1.35
WAND_TIP = Vector((-0.266, -0.98, 1.34))


def pitch(a):
    return Quaternion((1.0, 0.0, 0.0), a)


def roll(a):
    return Quaternion((0.0, 1.0, 0.0), a)


def yaw(a):
    return Quaternion((0.0, 0.0, 1.0), a)


I = Quaternion()


def smooth(x):
    x = min(max(x, 0.0), 1.0)
    return x * x * (3.0 - 2.0 * x)


def mix(a, b, t):
    return a.slerp(b, min(max(t, 0.0), 1.0))


def out(side, a):
    """A hanging limb swung out to its own side (side +1 = his left, +x)."""
    return roll(-side * a)


class Pose(dict):
    """{joint: Quaternion}, plus `dip` (m, the torso's drop; never up)."""

    def __init__(self, dip=0.0, **rot):
        super().__init__(rot)
        self.dip = dip


# ---------------------------------------------------------------------------
# The clips: pose(t) at t seconds
# ---------------------------------------------------------------------------

def idle(t):
    ph = 2 * math.pi * t  # one breath a second
    breath = 0.5 - 0.5 * math.cos(ph)
    sway = math.sin(ph / 2)
    return Pose(
        dip=-0.008 * breath,
        Torso=yaw(0.035 * sway),
        PivotArmL=out(1, 0.01 + 0.02 * breath) @ pitch(0.03 * sway),
        PivotArmR=out(-1, 0.01 + 0.02 * breath) @ pitch(-0.03 * sway),
        PivotHead=yaw(-0.06 * sway),
        PivotCape=pitch(0.02 + 0.03 * breath),
        PivotRobe=roll(0.03 * sway),
    )


RUN_T = 0.6


def leg_swing(s, fwd=0.12, back=0.3):
    """A boot's swing for s in -1..1: +1 fully forward (limited so the toe
    stays in the capsule), -1 fully back."""
    return pitch(-fwd * s if s > 0 else -back * s)


def run(t):
    ph = 2 * math.pi * t / RUN_T
    s = math.sin(ph)
    flight = abs(s)
    return Pose(
        dip=-0.018 * (1.0 - flight),
        Torso=yaw(0.1 * s),
        PivotLegL=leg_swing(s),
        PivotLegR=leg_swing(-s),
        # Fists pump against the boot on their side, held a little out.
        PivotArmL=out(1, 0.02) @ pitch(0.28 * s if s > 0 else 0.2 * s),
        PivotArmR=out(-1, 0.02) @ pitch(-0.28 * s if s < 0 else -0.2 * s),
        PivotHead=yaw(-0.07 * s) @ pitch(0.012 * math.sin(2 * ph + 0.6)),
        PivotCape=pitch(0.12 + 0.04 * math.sin(2 * ph)),
        PivotRobe=pitch(0.05) @ roll(0.04 * s),
    )


def strafe(side):
    """Side steps toward his left (side +1, +x) or right (-1)."""

    def pose(t):
        ph = 2 * math.pi * t / RUN_T
        s = math.sin(ph)
        lead = 0.1 * max(s, 0.0) - 0.03 * max(-s, 0.0)
        trail = 0.1 * max(-s, 0.0) - 0.03 * max(s, 0.0)
        legs = {1: out(1, lead), -1: out(-1, -trail)} if side > 0 else \
            {-1: out(-1, lead), 1: out(1, -trail)}
        return Pose(
            dip=-0.015 * (1.0 - abs(s)),
            Torso=yaw(0.08 * side),
            PivotLegL=legs[1],
            PivotLegR=legs[-1],
            PivotArmL=out(1, 0.05 if side > 0 else 0.0) @ pitch(0.12 * s),
            PivotArmR=out(-1, 0.05 if side < 0 else 0.0) @ pitch(-0.12 * s),
            PivotHead=yaw(-0.05 * side),
            PivotCape=pitch(0.06) @ roll(0.12 * side),
            PivotRobe=roll(0.06 * side + 0.02 * math.sin(2 * ph)),
        )
    return pose


WINDUP_T = 0.4
COCK_AT = 0.12  # end of the cock-back
THRUST_AT = 0.28  # the wand reaches its aim; held to the release


def wand_tip(arm_world):
    """Where the wand's tip is with his right arm turned by `arm_world`
    (a world rotation about the shoulder) and the torso at rest."""
    return SHOULDER_R + arm_world @ ((GRIP_R - SHOULDER_R) + WAND_DIR * WAND_LENGTH)


def aim_arm(torso):
    """The right arm's local rotation that points the wand's tip at the orb's
    launch point, with the torso turned by `torso` about its axis (the
    shoulder turns with it)."""
    shoulder = K.TORSO_ORIGIN + torso @ (SHOULDER_R - K.TORSO_ORIGIN)
    rest = torso @ ((GRIP_R - SHOULDER_R) + WAND_DIR * WAND_LENGTH)
    world = rest.rotation_difference(WAND_TIP - shoulder) @ torso
    return torso.inverted() @ world


WINDUP_TORSO = yaw(0.1)
WINDUP_AIM = aim_arm(WINDUP_TORSO)


def windup(t):
    cock = smooth(t / COCK_AT)
    thrust = smooth((t - COCK_AT) / (THRUST_AT - COCK_AT))
    stance = smooth(t / THRUST_AT)
    cocked = pitch(0.55) @ out(-1, 0.1)
    arm = mix(mix(I, cocked, cock), WINDUP_AIM, thrust)
    torso = mix(mix(I, yaw(-0.28), cock), WINDUP_TORSO, thrust)
    return Pose(
        dip=-0.015 * stance,
        Torso=torso,
        # Boots planted wide.
        PivotLegL=out(1, 0.1 * stance),
        PivotLegR=out(-1, 0.1 * stance),
        PivotArmR=arm,
        # The off arm flung forward and out for balance.
        PivotArmL=mix(I, pitch(-0.2) @ out(1, 0.03), stance),
        PivotHead=mix(I, yaw(0.18), cock) if thrust <= 0.0 else
        mix(yaw(0.18), yaw(-0.08), thrust),
        PivotCape=pitch(0.08 * stance + 0.1 * thrust * (1 - thrust) * 4),
        PivotRobe=pitch(0.04 * thrust) @ roll(0.03 * stance),
    )


FLINCH_T = 0.3


def envelope(t, T=FLINCH_T, attack=0.04):
    """A flinch's strength: snaps in over `attack`, then eases back to 0 at T
    with a small bounce."""
    if t <= attack:
        return smooth(t / attack)
    u = (t - attack) / (T - attack)
    return (1.0 - u) ** 2 * math.cos(1.6 * math.pi * u * 0.6)


def flinch_head(t):
    e = envelope(t)
    return Pose(PivotHead=yaw(0.4 * e), Torso=yaw(-0.1 * e),
                PivotArmL=out(1, 0.04 * e), PivotArmR=out(-1, 0.04 * e))


def flinch_chest(t):
    e = envelope(t)
    return Pose(PivotArmL=pitch(-0.2 * e),
                PivotArmR=pitch(-0.2 * e),
                PivotRobe=pitch(0.07 * e), PivotCape=pitch(0.12 * e),
                PivotHead=yaw(0.08 * e))


def flinch_side(side):
    """Hit on his left (side +1) or right: that shoulder knocked back."""

    def pose(t):
        e = envelope(t)
        arm = "PivotArmL" if side > 0 else "PivotArmR"
        other = "PivotArmR" if side > 0 else "PivotArmL"
        return Pose(**{
            "Torso": yaw(0.28 * side * e),
            arm: pitch(0.28 * e) @ out(side, 0.03 * e),
            other: pitch(-0.12 * e),
            "PivotHead": yaw(-0.12 * side * e),
            "PivotRobe": roll(-0.04 * side * e),
        })
    return pose


LAND_T = 0.5


def beam_land(t):
    # Touchdown: arms a little out and forward, boots apart, a dip that
    # settles back with a small overshoot.
    k = 1.0 - smooth((t - 0.08) / (LAND_T - 0.08))
    dip = -0.02 * (smooth(t / 0.08) * 0.4 + 0.6) * k
    return Pose(
        dip=dip,
        PivotLegL=out(1, 0.07 * k),
        PivotLegR=out(-1, 0.07 * k),
        PivotArmL=out(1, 0.03 * k) @ pitch(-0.15 * k),
        PivotArmR=out(-1, 0.03 * k) @ pitch(-0.15 * k),
        PivotHead=I,
        PivotCape=pitch(0.18 * k),
        PivotRobe=pitch(-0.05 * k),
        Torso=I,
    )


HOP_T = 0.5


def victory(t):
    ph = 2 * math.pi * t / HOP_T
    s = math.sin(ph)
    return Pose(
        dip=-0.012 * (0.5 + 0.5 * math.cos(ph)),
        Torso=yaw(0.16 * s),
        # Fists pumping in front of his chest, one then the other.
        PivotArmL=pitch(-0.13 - 0.09 * s),
        PivotArmR=pitch(-0.13 + 0.09 * s),
        # Feet kicked back as he hops.
        PivotLegL=pitch(0.25 * (0.5 - 0.5 * math.cos(ph))),
        PivotLegR=pitch(0.25 * (0.5 - 0.5 * math.cos(ph))),
        PivotHead=yaw(-0.2 * s),
        PivotCape=pitch(0.1 + 0.06 * math.cos(ph)),
        PivotRobe=roll(0.05 * s),
    )


VOID_T = 0.6


def void_fall(t):
    ph = 2 * math.pi * t / VOID_T
    return Pose(
        PivotArmL=out(1, 2.2 + 0.5 * math.sin(2 * ph)) @ pitch(0.4 * math.sin(ph)),
        PivotArmR=out(-1, 2.2 + 0.5 * math.sin(2 * ph + 1.5)) @ pitch(-0.4 * math.sin(ph)),
        PivotLegL=pitch(0.6 * math.sin(ph)) @ out(1, 0.25),
        PivotLegR=pitch(-0.6 * math.sin(ph)) @ out(-1, 0.25),
        PivotHead=pitch(-0.25) @ yaw(0.3 * math.sin(ph)),
        PivotCape=pitch(-1.0 + 0.3 * math.sin(2 * ph)),
        PivotRobe=pitch(-0.35 + 0.1 * math.sin(2 * ph)),
        Torso=yaw(0.2 * math.sin(ph)),
    )


DEATH_T = 0.35


def death_back(t):
    k = smooth(t / DEATH_T)
    return Pose(
        Torso=pitch(-1.0 * k),
        PivotArmL=out(1, 1.5 * k) @ pitch(-0.5 * k),
        PivotArmR=out(-1, 1.3 * k) @ pitch(-0.7 * k),
        PivotLegL=pitch(-0.6 * k),
        PivotLegR=pitch(-0.35 * k),
        PivotHead=pitch(-0.35 * k),
        PivotCape=pitch(-0.6 * k),
        PivotRobe=pitch(-0.5 * k),
    )


def death_spin(t):
    k = smooth(t / DEATH_T)
    return Pose(
        dip=-0.12 * k,
        Torso=yaw(2 * math.pi * 0.85 * k),
        PivotArmL=out(1, 1.4 * k),
        PivotArmR=out(-1, 1.4 * k),
        PivotLegL=out(1, 0.35 * k),
        PivotLegR=out(-1, 0.35 * k),
        PivotHead=yaw(0.6 * k) @ roll(0.3 * k),
        PivotCape=pitch(0.8 * k),
        PivotRobe=pitch(0.2 * k),
    )


def death_crumple(t):
    k = smooth(t / DEATH_T)
    return Pose(
        dip=-0.36 * k,
        Torso=pitch(0.45 * k),
        PivotArmL=pitch(-0.7 * k) @ out(1, 0.3 * k),
        PivotArmR=pitch(-0.5 * k) @ out(-1, 0.4 * k),
        PivotLegL=out(1, 0.45 * k),
        PivotLegR=out(-1, 0.45 * k),
        PivotHead=pitch(0.45 * k) @ roll(0.2 * k),
        PivotCape=pitch(-0.3 * k),
        PivotRobe=pitch(-0.2 * k),
    )


HERO_T = 2.4
# Milestone 3's menu pose (src/menu/main_menu.rs), in Blender space: the wand
# arm raised high to the sky, the other fist on his hip, chin up.
HERO_ARM_R = pitch(-(math.radians(70.0) + 0.95)) @ roll(-0.2)


def hero(t):
    ph = 2 * math.pi * t / (HERO_T / 2)  # two breaths a loop
    breath = 0.5 - 0.5 * math.cos(ph)
    return Pose(
        dip=-0.01 * breath,
        Torso=yaw(0.04 * math.sin(ph / 2)),
        PivotArmR=HERO_ARM_R @ pitch(-0.05 * breath),
        PivotArmL=roll(0.5) @ pitch(0.25),
        PivotHead=pitch(-0.12) @ yaw(0.05 * math.sin(ph / 2)),
        PivotLegL=out(1, 0.07),
        PivotLegR=out(-1, 0.1),
        PivotCape=pitch(0.12 + 0.05 * breath) @ roll(0.05),
        PivotRobe=roll(0.025 * math.sin(ph / 2)),
    )


class Clip:
    def __init__(self, name, seconds, pose, additive=False, fit=True, exempt=()):
        self.name = name
        self.frames = round(seconds * FPS)
        self.seconds = seconds
        self.pose = pose
        self.additive = additive
        self.fit = fit
        self.exempt = exempt


CLIPS = [
    Clip("Idle", 2.0, idle),
    Clip("Run", RUN_T, run),
    Clip("StrafeL", RUN_T, strafe(1)),
    Clip("StrafeR", RUN_T, strafe(-1)),
    Clip("WindUp", WINDUP_T, windup, exempt=("GauntletR",)),
    Clip("FlinchHead", FLINCH_T, flinch_head, additive=True),
    Clip("FlinchChest", FLINCH_T, flinch_chest, additive=True),
    Clip("FlinchL", FLINCH_T, flinch_side(1), additive=True),
    Clip("FlinchR", FLINCH_T, flinch_side(-1), additive=True),
    Clip("BeamLand", LAND_T, beam_land),
    Clip("VictoryHop", HOP_T, victory),
    Clip("VoidFall", VOID_T, void_fall, fit=False),
    Clip("DeathBack", DEATH_T, death_back, fit=False),
    Clip("DeathSpin", DEATH_T, death_spin, fit=False),
    Clip("DeathCrumple", DEATH_T, death_crumple, fit=False),
    Clip("Hero", HERO_T, hero, fit=False),
]


# ---------------------------------------------------------------------------
# Applying poses and keying them
# ---------------------------------------------------------------------------

def joints(root):
    by_name = {o.name: o for o in bpy.data.objects}
    out_ = {}
    for name in JOINTS:
        obj = by_name[name]
        obj.rotation_mode = "QUATERNION"
        out_[name] = obj
    return out_


def rest_of(objs):
    return {n: o.location.copy() for n, o in objs.items()}


def apply(objs, rest, pose, base=None):
    """Sets every joint to `pose` (on top of `base`'s rotations for an
    additive clip)."""
    for name, obj in objs.items():
        q = pose.get(name, I)
        if base is not None:
            q = q @ base.get(name, I)
        obj.rotation_quaternion = q
        obj.location = rest[name]
    dip = pose.dip + (base.dip if base is not None else 0.0)
    objs["Torso"].location = rest["Torso"] + Vector((0.0, 0.0, min(dip, 0.0)))


def add_clips(root):
    """Keys every clip as an action on the joints (one slot per joint), each
    on a muted NLA track named after it so the exporter writes one glTF
    animation per clip while the rest pose stays the model's."""
    scene = bpy.context.scene
    scene.render.fps = FPS
    scene.render.fps_base = 1.0
    scene.frame_start = 0
    objs = joints(root)
    rest = rest_of(objs)
    for clip in CLIPS:
        act = bpy.data.actions.new(clip.name)
        act.use_fake_user = True
        keyed = [n for n in JOINTS if not clip.additive or n in clip.pose(0.05)]
        slots = {}
        for name in keyed:
            obj = objs[name]
            obj.animation_data_create()
            slot = act.slots.new(id_type="OBJECT", name=name)
            obj.animation_data.action = act
            obj.animation_data.action_slot = slot
            slots[name] = slot
        for f in range(clip.frames + 1):
            apply(objs, rest, clip.pose(f / FPS))
            for name in keyed:
                objs[name].keyframe_insert("rotation_quaternion", frame=f)
                if name == "Torso" and not clip.additive:
                    objs[name].keyframe_insert("location", frame=f)
        for name in keyed:
            obj = objs[name]
            obj.animation_data.action = None
            track = obj.animation_data.nla_tracks.new()
            track.name = clip.name
            strip = track.strips.new(clip.name, 0, act)
            strip.action_slot = slots[name]
            track.mute = True
    apply(objs, rest, Pose())
    return rest


def keep_channel(anim, node, path):
    """Which exported channels stay (`lib/export.py` drops the rest): the
    joints' rotations, and the torso's translation in the absolute clips.
    Scales are all 1, and the other joints never move."""
    if path == "rotation":
        return True
    if path == "translation" and node == "Torso":
        return not next(c for c in CLIPS if c.name == anim).additive
    return False


# ---------------------------------------------------------------------------
# The sampled hitbox fit
# ---------------------------------------------------------------------------

def sample_times(clip):
    n = int(clip.seconds / FIT_STEP + 1e-6)
    return [k * FIT_STEP for k in range(n + 1)] + [clip.seconds]


def clip_fit(root, objs, rest, clip):
    """{part: (worst overshoot, time)} over the clip's 50 ms samples."""
    worst = {}
    base = idle(0.0) if clip.additive else None
    for t in sample_times(clip):
        apply(objs, rest, clip.pose(min(t, clip.seconds)), base)
        for part, v in K.fit_report(root).items():
            if part not in worst or v > worst[part][0]:
                worst[part] = (v, t)
    apply(objs, rest, Pose())
    return worst


def check_clips(root, rest, report=False):
    objs = joints(root)
    bad = []
    for clip in CLIPS:
        if not clip.fit:
            continue
        for part, (v, t) in sorted(clip_fit(root, objs, rest, clip).items()):
            limit = K.HEAD_TOLERANCE if K.is_head_part(part) else BODY_CLIP_TOLERANCE
            if report:
                print(f"KNIGHT clip {clip.name:11s} {part:10s} {v * 100:+.1f} cm at {t:.2f} s"
                      f"{' (exempt)' if part in clip.exempt else ''}")
            if v > limit and part not in clip.exempt:
                bad.append((clip.name, part, round(v, 3), round(t, 2)))
    if bad:
        raise ValueError(f"knight clips leave the hitboxes: {bad}")
    # The wind-up's last pose puts the wand's tip on the orb's launch point.
    torso = WINDUP_TORSO
    shoulder = K.TORSO_ORIGIN + torso @ (SHOULDER_R - K.TORSO_ORIGIN)
    tip = shoulder + (torso @ WINDUP_AIM) @ ((GRIP_R - SHOULDER_R) + WAND_DIR * WAND_LENGTH)
    miss = (tip - WAND_TIP).length
    if report:
        print(f"KNIGHT clip WindUp wand tip {tuple(round(c, 3) for c in tip)}, "
              f"{miss * 100:.1f} cm from the orb's launch point")
    if miss > 0.03:
        raise ValueError(f"the wind-up's wand tip misses the orb's launch point by {miss:.3f} m")
