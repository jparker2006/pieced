"""Building pieces: the brick wall, the plank floor, ramp and cone, their crack
stages, the edit tiles edited pieces are drawn from, and the debris chunks a
broken piece bursts into (targets R4-M1, T09, T01, T03-T08).

Every piece is built on the build grid (`src/shared.rs`: 4 m cells, 3 m levels)
in the piece's own local space, exactly as `PieceSlot::transform` places it and
matching the Milestone 1 extents in `src/building/mesh.rs`, so collision is
unchanged. In Blender coordinates (x, y, z) -> Bevy (-x, z, y):

- wall: standing on the origin (pivot on the ground, like every model; Rust
  lowers it 1.5 m onto the wall's centre), 4 m along x, 3 m tall, faces +-y,
  brick faces 0.15 m either side of the centre plane (the collider is 0.2 m);
- floor: centred on the origin, 4 x 4 m, planks along y, top at z = +0.1;
- ramp: base centre on the origin, rising 3 m over 4 m toward -y (its front,
  Bevy -Z), plank tops on the collider's slope;
- cone: base centre on the origin, a 4 x 4 m pyramid 1.5 m tall (half a level),
  plank tops on the collider's faces.

Edits (D44, `src/building/edit.rs`): an edited piece is drawn from shared
meshes. Walls, floors and ramps come as tile sets (`*_tiles`: one named part
per edit tile, cut from the full piece so the tiles together are the piece, with
wooden trims for the opening's edges); an edited cone is one of four roof
shapes (`cone_plank_peak`, `_slope`, `_ridge`, `_tent`) that Rust turns to fit.

Crack stages are separate models (`_crack1` at 66% HP: cartoon cracks; `_crack2`
at 33%: bigger cracks, missing bricks, split planks), so a piece swaps its mesh
when it cracks. Mortar lines are geometry: through-bricks stand 4 cm proud of a
mortar core, so the toon bands light each brick's edges.
"""

import math

import bmesh
from mathutils import Matrix, Vector

from lib import palette, scene, shapes
from lib.registry import Asset

CELL = 4.0
LEVEL = 3.0
HALF_W = CELL / 2
HALF_H = LEVEL / 2

CRACK = "stump_bark_line"  # dark brown ink for cracks and holes

# Tile sets are cut from closed solids, so no box face may be left out while
# one is built (a cut through an open box couldn't be capped).
_KEEP_ALL_FACES = False


# ---------------------------------------------------------------------------
# Local helpers
# ---------------------------------------------------------------------------

def add_box(bm, lo, hi, color, xf=None, drop=()):
    """An axis-aligned box lo..hi (then transformed by `xf` about its centre),
    tagged `color`. `drop` names faces to leave out: '-x', '+x', '-y', '+y',
    '-z', '+z' (hidden faces cost triangles for nothing). Returns the faces."""
    lo, hi = Vector(lo), Vector(hi)
    centre = (lo + hi) / 2
    size = hi - lo
    m = Matrix.Translation(centre)
    if xf is not None:
        m = m @ xf
    m = m @ Matrix.Diagonal((size.x, size.y, size.z, 1.0))
    verts = bmesh.ops.create_cube(bm, size=1.0, matrix=m)["verts"]
    faces = list({f for v in verts for f in v.link_faces})
    rot = (xf.to_3x3() if xf is not None else Matrix.Identity(3))
    if _KEEP_ALL_FACES:
        drop = ()
    keep = []
    dead = []
    for f in faces:
        f.normal_update()
        local = rot.inverted() @ f.normal
        axis = max(range(3), key=lambda i: abs(local[i]))
        name = ("+" if local[axis] > 0 else "-") + "xyz"[axis]
        (dead if name in drop else keep).append(f)
    if dead:
        bmesh.ops.delete(bm, geom=dead, context="FACES_ONLY")
    palette.tag(bm, keep, color)
    return keep


def add_poly(bm, points, color, outward):
    """A flat polygon through `points`, wound to face `outward`."""
    verts = [bm.verts.new(p) for p in points]
    f = bm.faces.new(verts)
    f.normal_update()
    if f.normal.dot(Vector(outward)) < 0:
        f.normal_flip()
    palette.tag(bm, [f], color)
    return f


def crack(bm, at, n, pts, width, color=CRACK):
    """A tapering zig-zag crack of quads lying on a surface: `at(p)` maps each
    2D point of `pts` onto the surface, `n` is the surface's outward normal. The
    crack is lifted a hair off the surface."""
    n = Vector(n).normalized()
    ps = [Vector(at(p)) + n * 0.005 for p in pts]
    segs = len(ps) - 1
    for i in range(segs):
        a, b = ps[i], ps[i + 1]
        d = (b - a).normalized()
        perp = n.cross(d).normalized()
        wa = width * (1.0 - 0.75 * i / segs) / 2
        wb = width * (1.0 - 0.75 * (i + 1) / segs) / 2
        a2, b2 = a - d * wa * 0.8, b + d * wb * 0.8
        quad = [a2 - perp * wa, b2 - perp * wb, b2 + perp * wb, a2 + perp * wa]
        add_poly(bm, quad, color, n)


def nail(bm, centre, up, radius=0.09, height=0.03, sides=6, phase=0.0):
    """A big cartoon nail head: a low faceted dome (a frustum) on a surface.
    `centre` sits on the surface; `up` is the surface normal."""
    up = Vector(up).normalized()
    t = up.orthogonal().normalized()
    b = up.cross(t)
    c = Vector(centre)
    base = []
    top = []
    for k in range(sides):
        a = phase + 2 * math.pi * k / sides
        d = t * math.cos(a) + b * math.sin(a)
        base.append(bm.verts.new(c + d * radius - up * 0.01))
        top.append(bm.verts.new(c + d * radius * 0.55 + up * height))
    faces = shapes.bridge(bm, base, top)
    cap = bm.faces.new(top)
    for f in faces + [cap]:
        f.normal_update()
        centroid = f.calc_center_median()
        if f.normal.dot(centroid - (c - up * 0.02)) < 0:
            f.normal_flip()
    palette.tag(bm, faces + [cap], "nail_head")


def lofted_bar(bm, sections, color):
    """A closed bar through cross-sections: each a list of corner points, wound
    the same way. Adjacent sections are bridged and both ends capped; every
    face is turned to face out of the bar."""
    rings = [[bm.verts.new(p) for p in s] for s in sections]
    sides = []
    for lo, hi in zip(rings, rings[1:]):
        sides += shapes.bridge(bm, lo, hi)
    caps = [bm.faces.new(list(reversed(rings[0]))), bm.faces.new(rings[-1])]
    middle = sum((v.co for r in rings for v in r), Vector()) / sum(len(r) for r in rings)
    for f in sides:
        f.normal_update()
        c = f.calc_center_median()
        if f.normal.dot(c - _nearest_axis_point(rings, c)) < 0:
            f.normal_flip()
    for f in caps:
        f.normal_update()
        if f.normal.dot(f.calc_center_median() - middle) < 0:
            f.normal_flip()
    palette.tag(bm, sides + caps, color)
    return rings, sides + caps


def _nearest_axis_point(rings, p):
    centres = [sum((v.co for v in r), Vector()) / len(r) for r in rings]
    best = centres[0]
    best_d = float("inf")
    for a, b in zip(centres, centres[1:]):
        ab = b - a
        t = max(0.0, min(1.0, (p - a).dot(ab) / max(ab.length_squared, 1e-9)))
        q = a + ab * t
        d = (p - q).length
        if d < best_d:
            best, best_d = q, d
    return best


def finish(root, bm, name="Body"):
    body = scene.make_part(name, shapes.mesh_from_bmesh(bm), root)
    shapes.flat_shading(body)
    return body


# ---------------------------------------------------------------------------
# Brick wall
# ---------------------------------------------------------------------------

COURSES = 8
GAP = 0.055  # mortar joint
FACE = 0.15  # brick faces this far either side of the centre plane
CORE = 0.11  # the mortar core's faces
PITCH_Z = (LEVEL + GAP) / COURSES
BRICKS = 5  # full bricks in an even course
PITCH_X = (CELL + GAP) / BRICKS


def brick_layout(seed):
    """Every brick as (course, index, x0, x1, z0, z1): a running bond of
    `COURSES` courses. Even courses hold 5 full bricks, odd courses 4 full
    bricks and a half brick at each end. Ends alternate flush and 6 cm short,
    so the wall's sides read as bumpy brick edges."""
    r = shapes.rng(seed)
    bricks = []
    for c in range(COURSES):
        z0 = -HALF_H + c * PITCH_Z
        z1 = z0 + PITCH_Z - GAP
        if c == COURSES - 1:
            z1 = HALF_H
        if c % 2 == 0:
            edges = [-HALF_W + k * PITCH_X for k in range(BRICKS + 1)]
        else:
            edges = [-HALF_W] + [-HALF_W + PITCH_X / 2 + k * PITCH_X for k in range(BRICKS)] + [HALF_W + GAP]
        for i in range(len(edges) - 1):
            x0 = edges[i]
            x1 = edges[i + 1] - GAP
            if i == 0 and c % 2 == 0:
                x0 += 0.06
            if i == len(edges) - 2 and c % 2 == 1:
                x1 = HALF_W - 0.06
            elif i == len(edges) - 2:
                x1 = HALF_W
            # Hand-made wobble: a hair shorter or lower, never into a joint.
            x0 += r.uniform(0.0, 0.015)
            x1 -= r.uniform(0.0, 0.015)
            top = z1 - (r.uniform(0.0, 0.045) if c == COURSES - 1 else r.uniform(0.0, 0.012))
            bricks.append((c, i, x0, x1, z0, top))
    return bricks


def brick_tilt(r, course):
    """A tiny tilt in the wall's plane (less on the top and bottom courses, so
    the wall stays inside its cell)."""
    limit = 0.5 if course in (0, COURSES - 1) else 1.3
    return Matrix.Rotation(math.radians(r.uniform(-limit, limit)), 4, "Y")


def wall_bricks(bm, seed, missing=(), shoved=None):
    """Lays the bricks (minus `missing` (course, index) pairs); `shoved` maps
    (course, index) to (push, twist degrees): bricks knocked half out of the
    wall. Returns the holes left by missing bricks."""
    r = shapes.rng(seed + 1)
    shoved = shoved or {}
    holes = []
    for (c, i, x0, x1, z0, z1) in brick_layout(seed):
        tilt = brick_tilt(r, c)
        if (c, i) in missing:
            holes.append((c, x0, x1, z0, z1))
            continue
        xf = tilt
        if (c, i) in shoved:
            push, twist = shoved[(c, i)]
            xf = (Matrix.Translation((0.0, -push, 0.0))
                  @ Matrix.Rotation(math.radians(twist), 4, "Z") @ tilt)
        drop = ("-z",) if c == 0 else ()
        add_box(bm, (x0, -FACE, z0), (x1, FACE, z1), "brick", xf=xf, drop=drop)
    return holes


CORE_TOP = HALF_H - 0.035


def wall_core(bm, bite=False):
    """The mortar core. With `bite`, a stepped notch is knocked out of the top
    left corner (where stage 2's missing corner bricks were)."""
    x0, x1 = -HALF_W + 0.03, HALF_W - 0.03
    if not bite:
        add_box(bm, (x0, -CORE, -HALF_H), (x1, CORE, CORE_TOP), "mortar", drop=("-z",))
        return
    step1 = -HALF_H + 6 * PITCH_Z - GAP / 2  # under course 6
    step2 = -HALF_H + 7 * PITCH_Z - GAP / 2  # under course 7
    add_box(bm, (x0, -CORE, -HALF_H), (x1, CORE, step1), "mortar", drop=("-z",))
    add_box(bm, (-1.26, -CORE, step1), (x1, CORE, step2), "mortar", drop=("-z",))
    add_box(bm, (-0.86, -CORE, step2), (x1, CORE, CORE_TOP), "mortar", drop=("-z",))


def hole_backs(bm, holes):
    """Dark backs in the holes left by missing bricks, on both faces of the
    core (not where the core itself is gone)."""
    for (c, x0, x1, z0, z1) in holes:
        if c >= 6 and x1 < -0.8:
            continue  # the corner bite: open to the sky
        z1 = min(z1, CORE_TOP)
        for s in (-1.0, 1.0):
            y = s * (CORE + 0.004)
            add_poly(bm, [(x0 + 0.02, y, z0 + 0.02), (x1 - 0.02, y, z0 + 0.02),
                          (x1 - 0.02, y, z1 - 0.02), (x0 + 0.02, y, z1 - 0.02)],
                     CRACK, (0.0, s, 0.0))


def wall_cracks(bm, cracks):
    """The same cracks on both faces (through-bricks: each crack stays on the
    one brick it is drawn on). Points are (x, z) on the wall."""
    for s in (-1.0, 1.0):
        n = (0.0, s, 0.0)
        for pts, width in cracks:
            crack(bm, lambda p, s=s: (p[0], s * FACE, p[1]), n, pts, width * 1.4)


# Cracks, each within one brick: (points (x, z), width). Course c spans
# z = -1.5 + 0.382 c .. +0.327; even courses have bricks from x = -1.94, -1.19,
# -0.38, 0.43, 1.24; odd courses from -2.0 (half), -1.59, -0.78, 0.03, 0.84, 1.65.
CRACKS_1 = [
    ([(-1.3, 0.69), (-1.16, 0.6), (-1.24, 0.52), (-1.08, 0.44)], 0.05),  # course 5
    ([(0.4, -0.06), (0.54, -0.14), (0.47, -0.22), (0.62, -0.31)], 0.05),  # course 3
    ([(1.52, 1.08), (1.66, 0.99), (1.6, 0.9), (1.72, 0.82)], 0.045),  # course 6
]
CRACKS_2 = CRACKS_1 + [
    ([(-0.3, 0.33), (-0.14, 0.24), (-0.22, 0.15), (-0.05, 0.06)], 0.055),  # course 4
    ([(1.0, 0.7), (1.16, 0.61), (1.07, 0.52), (1.24, 0.44)], 0.055),  # course 5
    ([(-1.8, -0.44), (-1.64, -0.53), (-1.72, -0.61), (-1.56, -0.69)], 0.05),  # course 2
    ([(-0.1, 1.09), (0.04, 1.0), (-0.04, 0.92), (0.1, 0.83)], 0.045),  # course 6
    ([(0.6, -1.14), (0.75, -1.22), (0.67, -1.31), (0.82, -1.4)], 0.05),  # course 0
]
# (course, index) of bricks knocked out at stage 2: a bite out of the top-left
# corner, a gap in the middle and one low down; two more hang half out.
MISSING_2 = {(7, 0), (7, 1), (6, 0), (4, 3), (3, 4), (1, 1)}
SHOVED_2 = {(5, 5): (0.04, 8.0), (2, 4): (0.03, -6.0)}


def wall_bmesh(stage):
    """The brick wall at a crack stage, standing on the ground."""
    bm = palette.new_bmesh()
    wall_core(bm, bite=stage >= 2)
    missing = MISSING_2 if stage >= 2 else ()
    shoved = SHOVED_2 if stage >= 2 else None
    holes = wall_bricks(bm, 7, missing=missing, shoved=shoved)
    hole_backs(bm, holes)
    if stage == 1:
        wall_cracks(bm, CRACKS_1)
    elif stage >= 2:
        wall_cracks(bm, CRACKS_2)
    # Built around its centre; stand it on the ground.
    bmesh.ops.translate(bm, vec=(0.0, 0.0, HALF_H), verts=bm.verts)
    return bm


def build_wall(root, stage):
    finish(root, wall_bmesh(stage))
    scene.make_attach("Top", root, (0.0, 0.0, LEVEL))


def build_wall_brick(root):
    """Chunky cartoon brick wall: 8 courses of through-bricks on a mortar core."""
    build_wall(root, 0)


def build_wall_brick_crack1(root):
    build_wall(root, 1)


def build_wall_brick_crack2(root):
    build_wall(root, 2)


# ---------------------------------------------------------------------------
# Planks (floor and ramp)
# ---------------------------------------------------------------------------

def plank(bm, frame, length, width, thick, r, bow=0.03, twist=0.012, color="plank",
          span=None, drop=(0.0, 0.0)):
    """A warped plank along the frame's u axis: two segments meeting at a
    slightly sagging middle, so the ends lift ("bent ends"), with a small twist.

    `frame` = (origin, u, v, n): the plank's centre line starts at origin, runs
    along u for `length`, is `width` wide along v and `thick` along -n (its top
    face lies on the plane through origin with normal n). `span` = (a, b)
    builds only that part of the length (a split plank); `drop` = (at a, at b)
    sinks it along -n, linearly (a broken end sagging or prised up)."""
    origin, u, v, n = (Vector(x) for x in frame)
    a, b = span if span is not None else (0.0, length)
    mid = length / 2
    ts = [a, b] if not (a < mid < b) else [a, mid, b]
    end_lift = bow * r.uniform(0.7, 1.2)
    tw = twist * r.uniform(-1.0, 1.0)
    sections = []
    for t in ts:
        f = abs(t - mid) / mid  # 0 in the middle, 1 at the ends
        lift = end_lift * f * f - bow * 0.15
        k = (t - a) / max(b - a, 1e-6)
        lift -= drop[0] * (1 - k) + drop[1] * k
        roll = tw * (t / length - 0.5) * 2
        c = origin + u * t + n * lift
        w = width / 2
        corners = []
        for (sv, sn) in ((-1, 0), (1, 0), (1, -1), (-1, -1)):
            p = c + v * (sv * w) + n * (sn * thick) + n * (sv * roll)
            corners.append(p)
        sections.append(corners)
    _, faces = lofted_bar(bm, sections, color)
    # The sunlit top reads lighter than the plank's edges (targets R4-M1, T09).
    palette.tag(bm, [f for f in faces if f.normal.dot(n) > 0.8], "trunk")

    def top(t, across=0.0):
        """A point on the plank's top face, `t` along it and `across` its width."""
        f = abs(t - mid) / mid
        k = (t - a) / max(b - a, 1e-6)
        lift = end_lift * f * f - bow * 0.15 - (drop[0] * (1 - k) + drop[1] * k)
        roll = tw * (t / length - 0.5) * 2
        sv = across / (width / 2) if width > 0 else 0.0
        return origin + u * t + n * lift + v * across + n * (sv * roll)

    return top


def plank_crack(bm, top, n, pts, w=0.05):
    """A crack along a plank's top face: pts are (along, across) in metres."""
    crack(bm, lambda p: top(p[0], p[1]), n, pts, w)


# Floor: 5 planks along y (Bevy local z), nailed to two beams at the ends.
FLOOR_PLANKS = 5
FLOOR_GAP = 0.045
FLOOR_TOP = 0.1
FLOOR_THICK = 0.1


def floor_frames():
    pitch = (CELL + FLOOR_GAP) / FLOOR_PLANKS
    width = pitch - FLOOR_GAP
    frames = []
    for i in range(FLOOR_PLANKS):
        x = -HALF_W + i * pitch + width / 2
        frames.append(((x, -HALF_W, FLOOR_TOP - 0.01), (0, 1, 0), (1, 0, 0), (0, 0, 1)))
    return frames, width


def lay_planks(bm, frames, width, thick, r, split, split_drops):
    """Lays one plank per frame; plank `split` is snapped in two (the stubs
    sag or prise up by `split_drops`). Returns each plank's list of
    (span, top function)."""
    planks = []
    for i, frame in enumerate(frames):
        if i == split:
            (a_span, a_drop), (b_span, b_drop) = split_drops
            planks.append([(a_span, plank(bm, frame, CELL, width, thick, r, span=a_span, drop=a_drop)),
                           (b_span, plank(bm, frame, CELL, width, thick, r, span=b_span, drop=b_drop))])
        else:
            planks.append([((0.0, CELL), plank(bm, frame, CELL, width, thick, r))])
    return planks


def top_of(planks, i, t, across=0.0):
    """The top of plank i at t (None over a split plank's gap)."""
    for (a, b), top in planks[i]:
        if a <= t <= b:
            return top(t, across)
    return None


def nail_planks(bm, planks, frames, ts, skip, **nail_args):
    for i, (_, _, _, n) in enumerate(frames):
        for t in ts:
            if (i, t) in skip:
                continue  # popped out
            p = top_of(planks, i, t)
            if p is not None:
                nail(bm, p - Vector(n) * 0.004, n, phase=0.3 * i, **nail_args)


def crack_planks(bm, planks, frames, cracks):
    for i, pts in cracks:
        n = frames[i][3]
        (a, b), top = next(((span, top) for span, top in planks[i]
                            if span[0] <= pts[0][0] and pts[-1][0] <= span[1]))
        del a, b
        plank_crack(bm, top, n, pts)


def floor_bmesh(stage):
    bm = palette.new_bmesh()
    r = shapes.rng(31)
    frames, width = floor_frames()
    # Two cross beams under the plank ends (the ceiling of a box shows them).
    for y in (-1.62, 1.62):
        add_box(bm, (-HALF_W + 0.02, y - 0.15, -0.1),
                (HALF_W - 0.02, y + 0.15, FLOOR_TOP - FLOOR_THICK - 0.005),
                "stump_bark", drop=("+z",))
    add_box(bm, (-HALF_W + 0.03, -HALF_W + 0.03, -0.075), (HALF_W - 0.03, HALF_W - 0.03, -0.02),
            "stump_bark")
    split = 2 if stage >= 2 else None
    planks = lay_planks(bm, frames, width, FLOOR_THICK, r, split,
                        (((0.0, 1.55), (0.0, -0.03)), ((2.3, CELL), (0.075, 0.0))))
    skip = {(4, 0.38), (2, CELL - 0.38)} if stage >= 2 else set()
    nail_planks(bm, planks, frames, (0.38, CELL - 0.38), skip)
    if stage >= 1:
        cracks = [
            (1, [(0.9, -0.1), (1.2, 0.05), (1.45, -0.08), (1.75, 0.06)]),
            (3, [(2.6, 0.12), (2.9, -0.02), (3.15, 0.1)]),
        ]
        if stage >= 2:
            cracks += [
                (0, [(1.9, 0.1), (2.25, -0.05), (2.55, 0.08), (2.9, -0.06), (3.1, 0.05)]),
                (4, [(0.9, -0.08), (1.2, 0.08), (1.5, -0.05), (1.8, 0.1)]),
                (3, [(0.7, 0.05), (1.0, -0.1), (1.3, 0.02)]),
            ]
        crack_planks(bm, planks, frames, cracks)
    return bm


def build_floor(root, stage):
    finish(root, floor_bmesh(stage))
    scene.make_attach("Top", root, (0.0, 0.0, FLOOR_TOP))


def build_floor_plank(root):
    """Five warped planks with big nail heads on two cross beams."""
    build_floor(root, 0)


def build_floor_plank_crack1(root):
    build_floor(root, 1)


def build_floor_plank_crack2(root):
    build_floor(root, 2)


# Ramp: 6 planks across the slope on two stringers, a post pair at the high end.
RAMP_PLANKS = 6
RAMP_GAP = 0.05
RAMP_THICK = 0.1
SLOPE_LEN = math.hypot(CELL, LEVEL)
DOWN = Vector((0.0, CELL / SLOPE_LEN, -LEVEL / SLOPE_LEN))  # down the slope (toward +y)
NORMAL = Vector((0.0, LEVEL / SLOPE_LEN, CELL / SLOPE_LEN))  # up out of the slope
TOP_EDGE = Vector((0.0, -HALF_W, LEVEL))  # the slope's high edge (centre)


def ramp_frames():
    # The planks start a little down from the high edge and stop a little short
    # of the foot, so their tilted undersides stay inside the cell.
    start, end = 0.07, SLOPE_LEN - 0.03
    pitch = (end - start + RAMP_GAP) / RAMP_PLANKS
    width = pitch - RAMP_GAP
    frames = []
    for i in range(RAMP_PLANKS):
        s = start + i * pitch + width / 2  # distance down the slope to the centre line
        c = TOP_EDGE + DOWN * s + NORMAL * 0.012
        frames.append(((-HALF_W, c.y, c.z), (1, 0, 0), tuple(DOWN), tuple(NORMAL)))
    return frames, width


def ramp_stringer(bm, x):
    """A beam under the plank ends, following the slope: its profile in (y, z)
    is cut flat at the ground (z = 0), so a ramp built on a floor never pokes
    through it."""
    slope = LEVEL / CELL

    def z_top(y):  # just under the planks
        return slope * (HALF_W - y) - 0.14

    depth = 0.42
    y_tip = HALF_W - 0.14 / slope  # where the top edge meets the ground
    y_foot = HALF_W - (0.14 + depth) / slope  # where the bottom edge meets it
    y_high = -HALF_W + 0.04
    profile = [(y_tip, 0.0), (y_high, z_top(y_high)), (y_high, z_top(y_high) - depth),
               (y_foot, 0.0)]
    lofted_bar(bm, [[(x - 0.1, y, z) for y, z in profile],
                    [(x + 0.1, y, z) for y, z in profile]], "stump_bark")


def ramp_underlay(bm):
    """A dark board along the slope under the planks: the gaps read dark and the
    ramp never looks see-through (its collider is a solid wedge)."""
    hi = TOP_EDGE + DOWN * 0.14 - NORMAL * 0.09
    lo_s = SLOPE_LEN - 0.03
    lo = TOP_EDGE + DOWN * lo_s - NORMAL * 0.09
    if lo.z < 0.0:  # stop at the ground
        k = hi.z / (hi.z - lo.z)
        lo = hi + (lo - hi) * k
    secs = []
    for p in (hi, lo):
        secs.append([(-HALF_W + 0.04, p.y, p.z), (HALF_W - 0.04, p.y, p.z),
                     (HALF_W - 0.04, p.y - NORMAL.y * 0.05, p.z - NORMAL.z * 0.05),
                     (-HALF_W + 0.04, p.y - NORMAL.y * 0.05, p.z - NORMAL.z * 0.05)])
    lofted_bar(bm, secs, "stump_bark")


def ramp_bmesh(stage):
    bm = palette.new_bmesh()
    r = shapes.rng(47)
    frames, width = ramp_frames()
    for x in (-1.72, 1.72):
        ramp_stringer(bm, x)
        # A post under the high end.
        add_box(bm, (x - 0.11, -HALF_W + 0.04, 0.0), (x + 0.11, -HALF_W + 0.26, LEVEL - 0.35),
                "stump_bark", drop=("-z",))
    ramp_underlay(bm)
    split = 2 if stage >= 2 else None
    planks = lay_planks(bm, frames, width, RAMP_THICK, r, split,
                        (((0.0, 1.5), (0.0, -0.035)), ((2.25, CELL), (0.07, 0.0))))
    skip = {(4, 0.28)} if stage >= 2 else set()
    nail_planks(bm, planks, frames, (0.28, CELL - 0.28), skip, radius=0.085, sides=5)
    if stage >= 1:
        cracks = [
            (1, [(0.9, -0.1), (1.2, 0.05), (1.45, -0.08), (1.75, 0.06)]),
            (4, [(2.5, 0.12), (2.8, -0.02), (3.1, 0.1)]),
        ]
        if stage >= 2:
            cracks += [
                (0, [(1.8, 0.1), (2.15, -0.05), (2.45, 0.08), (2.8, -0.06)]),
                (3, [(0.8, -0.08), (1.1, 0.08), (1.4, -0.05), (1.7, 0.1)]),
                (5, [(2.2, 0.05), (2.5, -0.1), (2.8, 0.02)]),
            ]
        crack_planks(bm, planks, frames, cracks)
    return bm


def build_ramp(root, stage):
    finish(root, ramp_bmesh(stage))
    scene.make_attach("Top", root, (0.0, -HALF_W, LEVEL))


def build_ramp_plank(root):
    """Six warped planks across the slope on stringers, with big nail heads."""
    build_ramp(root, 0)


def build_ramp_plank_crack1(root):
    build_ramp(root, 1)


def build_ramp_plank_crack2(root):
    build_ramp(root, 2)


# ---------------------------------------------------------------------------
# Edit tiles
# ---------------------------------------------------------------------------

def clip(bm, planes):
    """Cuts `bm` down to the side of every plane (point, normal) where
    (p - point) . normal <= 0, capping each cut solid with a face of the
    colour beside it (open surfaces such as cracks are just trimmed)."""
    layer = palette.face_layer(bm)
    for co, no in planes:
        co, no = Vector(co), Vector(no).normalized()
        geom = bm.verts[:] + bm.edges[:] + bm.faces[:]
        res = bmesh.ops.bisect_plane(bm, geom=geom, dist=1e-6, plane_co=co, plane_no=no,
                                     clear_outer=True)
        cut = [e for e in res["geom_cut"]
               if isinstance(e, bmesh.types.BMEdge) and e.is_valid and e.is_boundary]
        if not cut:
            continue
        filled = bmesh.ops.holes_fill(bm, edges=cut, sides=0)["faces"]
        for f in filled:
            f.normal_update()
            if f.normal.dot(no) < 0:
                f.normal_flip()
            if f[layer] == 0:
                near = [g for e in f.edges for g in e.link_faces if g is not f and g[layer]]
                if near:
                    f[layer] = near[0][layer]
    # Bisecting leaves nothing loose, but drop stray bits anyway.
    loose = [v for v in bm.verts if not v.link_faces]
    if loose:
        bmesh.ops.delete(bm, geom=loose, context="VERTS")
    return bm


def keep_between(axis, lo, hi, edge_lo, edge_hi):
    """Planes keeping lo <= coordinate `axis` <= hi (none at the piece's own
    outer edges, edge_lo / edge_hi)."""
    planes = []
    n = Vector((0.0, 0.0, 0.0))
    if lo > edge_lo + 1e-4:
        n = Vector((0.0, 0.0, 0.0))
        n[axis] = -1.0
        p = Vector((0.0, 0.0, 0.0))
        p[axis] = lo
        planes.append((p, n))
    if hi < edge_hi - 1e-4:
        n = Vector((0.0, 0.0, 0.0))
        n[axis] = 1.0
        p = Vector((0.0, 0.0, 0.0))
        p[axis] = hi
        planes.append((p, n))
    return planes


def tile_part(root, name, make_bm, planes):
    global _KEEP_ALL_FACES
    _KEEP_ALL_FACES = True
    try:
        bm = make_bm()
    finally:
        _KEEP_ALL_FACES = False
    finish(root, clip(bm, planes), name)


# Wall tiles are numbered like src/building/edit.rs: Bevy columns along the
# wall's +x (Blender -x), rows from the top; a tile is 4/3 x 1 m.
WALL_TILE_W = CELL / 3
WALL_TILE_H = LEVEL / 3


def wall_tile_planes(tile):
    c, r = tile % 3, tile // 3
    x0, x1 = HALF_W - (c + 1) * WALL_TILE_W, HALF_W - c * WALL_TILE_W
    z0, z1 = LEVEL - (r + 1) * WALL_TILE_H, LEVEL - r * WALL_TILE_H
    return keep_between(0, x0, x1, -HALF_W, HALF_W) + keep_between(2, z0, z1, 0.0, LEVEL)


# The diagonals a triangle edit cuts along (Blender x, z): "A" runs from the
# Bevy bottom-left to the top-right corner through tiles 6, 4, 2; "B" from the
# top-left to the bottom-right through tiles 0, 4, 8. "Lo" keeps the side below
# the diagonal, "Hi" the side above.
DIAGONALS = {
    "A": ((HALF_W, 0.0, 0.0), (3.0, 0.0, 4.0), (6, 4, 2)),
    "B": ((-HALF_W, 0.0, 0.0), (-3.0, 0.0, 4.0), (0, 4, 8)),
}


def wall_trim(root, name, length, depth):
    """A wooden frame board along Bevy x (Blender -x), centred on the origin:
    `length` long, 0.12 m across the wall's face, `depth` through it."""
    bm = palette.new_bmesh()
    half = length / 2
    add_box(bm, (-half, -depth / 2, -0.06), (half, depth / 2, 0.06), "trunk")
    finish(root, bm, name)


def build_wall_tiles(root, stage):
    """Every edit tile of the brick wall (`T0`-`T8`), the half tiles a
    diagonal cuts (`A2Lo`...`B8Hi`), and the frame boards Rust lines an
    opening with (`TrimH`, `TrimV`, `TrimD`)."""
    make = lambda: wall_bmesh(stage)
    for t in range(9):
        tile_part(root, f"T{t}", make, wall_tile_planes(t))
    for key, (p0, n, tiles) in DIAGONALS.items():
        for t in tiles:
            for side, sign in (("Lo", 1.0), ("Hi", -1.0)):
                plane = (Vector(p0), Vector(n) * sign)
                tile_part(root, f"{key}{t}{side}", make, wall_tile_planes(t) + [plane])
    wall_trim(root, "TrimH", WALL_TILE_W + 0.12, 0.36)
    wall_trim(root, "TrimV", WALL_TILE_H + 0.12, 0.35)
    # A little short of the corners, so its ends stay inside the wall.
    wall_trim(root, "TrimD", math.hypot(CELL, LEVEL) - 0.2, 0.34)


def floor_tile_planes(tile):
    """Bevy quarter (column from Bevy x, row from Bevy z = Blender y)."""
    c, r = tile % 2, tile // 2
    x0, x1 = (0.0, HALF_W) if c == 0 else (-HALF_W, 0.0)
    y0, y1 = (-HALF_W, 0.0) if r == 0 else (0.0, HALF_W)
    return keep_between(0, x0, x1, -HALF_W, HALF_W) + keep_between(1, y0, y1, -HALF_W, HALF_W)


def build_floor_tiles(root, stage):
    """The plank floor's four quarters (`Q0`-`Q3`) and the beam Rust lays along
    a cut edge (`Trim`, 2 m along Bevy x)."""
    make = lambda: floor_bmesh(stage)
    for t in range(4):
        tile_part(root, f"Q{t}", make, floor_tile_planes(t))
    bm = palette.new_bmesh()
    add_box(bm, (-HALF_W / 2 - 0.06, -0.07, -0.13), (HALF_W / 2 + 0.06, 0.07, FLOOR_TOP + 0.03),
            "stump_bark")
    finish(root, bm, "Trim")


def build_ramp_tiles(root, stage):
    """The plank ramp's left and right halves (Bevy x < 0 and > 0), each with
    a stringer and a post along its cut side: a half ramp (Rust turns it to
    rise along the edit's path)."""
    for name, sign in (("Left", 1.0), ("Right", -1.0)):
        def make(sign=sign):
            bm = ramp_bmesh(stage)
            x = sign * 0.13
            ramp_stringer(bm, x)
            add_box(bm, (x - 0.1, -HALF_W + 0.04, 0.0), (x + 0.1, -HALF_W + 0.24, LEVEL - 0.35),
                    "stump_bark")
            return bm
        plane = ((0.0, 0.0, 0.0), (-sign, 0.0, 0.0))
        tile_part(root, name, make, [plane])


def _tiles(builder, stage):
    def build(root):
        builder(root, stage)
    return build


# ---------------------------------------------------------------------------
# Cone (roof)
# ---------------------------------------------------------------------------

CONE_H = LEVEL / 2
ROOF_PLANK_W = 0.5  # plank width across the slope
ROOF_GAP = 0.05
ROOF_THICK = 0.09
ROOF_UNDER = 0.1  # the dark underlay sits this far under the plank tops


def cone_corner(tile):
    """A cone tile's ground corner in Blender (x, y), from its Bevy tile index
    (column from Bevy x, row from Bevy z; Bevy (x, z) is Blender (-x, y))."""
    bx = -HALF_W + CELL * (tile % 2)
    bz = -HALF_W + CELL * (tile // 2)
    return Vector((-bx, bz))


def cone_points(raised):
    """The hull of a cone with `raised` corners (Bevy tiles) lifted to the top:
    the base square, the raised corners, and the apex, except for a two-corner
    roof-ramp slope, which is a plain wedge (src/building/edit.rs, cone_points)."""
    pts = [cone_corner(t).to_3d() for t in range(4)]
    for t in raised:
        c = cone_corner(t)
        pts.append(Vector((c.x, c.y, CONE_H)))
    adjacent = len(raised) == 2 and ((raised[0] % 2 == raised[1] % 2) != (raised[0] // 2 == raised[1] // 2))
    if not adjacent:
        pts.append(Vector((0.0, 0.0, CONE_H)))
    return pts


def hull(points):
    """Convex hull faces [(corner points, outward normal)] and edges
    [(a, b, normals of the two faces)] of `points`, coplanar faces merged."""
    bm = bmesh.new()
    verts = [bm.verts.new(p) for p in points]
    res = bmesh.ops.convex_hull(bm, input=verts)
    loose = list({v for v in res.get("geom_interior", []) + res.get("geom_unused", [])
                  if isinstance(v, bmesh.types.BMVert)})
    if loose:
        bmesh.ops.delete(bm, geom=loose, context="VERTS")
    bmesh.ops.dissolve_limit(bm, angle_limit=math.radians(0.5), verts=bm.verts[:],
                             edges=bm.edges[:])
    bm.normal_update()
    centre = sum((v.co for v in bm.verts), Vector()) / len(bm.verts)
    faces = []
    for f in bm.faces:
        n = f.normal.copy()
        if n.dot(f.calc_center_median() - centre) < 0:
            n = -n
        faces.append(([v.co.copy() for v in f.verts], n))
    edges = []
    for e in bm.edges:
        ns = []
        for f in e.link_faces:
            n = f.normal.copy()
            if n.dot(f.calc_center_median() - centre) < 0:
                n = -n
            ns.append(n)
        edges.append((e.verts[0].co.copy(), e.verts[1].co.copy(), ns))
    bm.free()
    return faces, edges


def face_frame(n):
    """(u, v) in a face's plane: u level (along the face), v uphill (or +y on a
    flat face)."""
    u = n.cross(Vector((0.0, 0.0, 1.0)))
    if u.length < 1e-4:
        u = Vector((1.0, 0.0, 0.0))
    u.normalize()
    v = n.cross(u).normalized()
    if v.z < -1e-6 or (abs(v.z) <= 1e-6 and v.y < 0):
        v, u = -v, -u
    return u, v


def span_at(poly2d, vc):
    """The polygon's extent along u on the line v = vc (poly2d: (u, v) corners)."""
    xs = []
    for (u0, v0), (u1, v1) in zip(poly2d, poly2d[1:] + poly2d[:1]):
        if (v0 - vc) * (v1 - vc) <= 0 and abs(v1 - v0) > 1e-9:
            xs.append(u0 + (vc - v0) / (v1 - v0) * (u1 - u0))
    return (min(xs), max(xs)) if len(xs) >= 2 else None


def roof_planks(bm, faces, r):
    """Covers every face but the bottom with level planks (rows up each face,
    each as long as the face is wide there). Returns [(top function, length,
    normal, row)] per plank, in build order (row 0 is a face's lowest)."""
    planks = []
    pitch = ROOF_PLANK_W + ROOF_GAP
    for poly, n in faces:
        if n.z < -0.5:
            continue
        u, v = face_frame(n)
        o = poly[0]
        pts = [((p - o).dot(u), (p - o).dot(v)) for p in poly]
        vmin = min(q[1] for q in pts)
        vmax = max(q[1] for q in pts)
        # Start a little up from the lowest edge so the plank undersides stay
        # above the ground, and stop short of the top edge (a trim covers it).
        lo, hi = vmin + 0.12, vmax - 0.05
        rows = max(1, int(round((hi - lo) / pitch)))
        step = (hi - lo) / rows
        width = min(ROOF_PLANK_W, step - ROOF_GAP)
        for k in range(rows):
            vc = lo + (k + 0.5) * step
            # The plank fits the face across its whole width (its ends tuck
            # under the edge trims instead of poking through them).
            spans = [span_at(pts, min(max(vc + dv, vmin), vmax)) for dv in (-width / 2, width / 2)]
            if None in spans:
                continue
            u0 = max(sp[0] for sp in spans) + 0.02
            u1 = min(sp[1] for sp in spans) - 0.02
            if u1 - u0 < 0.35:
                continue
            # Planks on an upright face (a gable) sit a little in, so their
            # warped ends never poke out of the cell.
            origin = o + u * u0 + v * vc + n * (0.004 if abs(n.z) > 0.2 else -0.045)
            top = plank(bm, (origin, u, v, n), u1 - u0, width, ROOF_THICK, r, bow=0.025)
            planks.append((top, u1 - u0, n, k))
    return planks


def roof_trims(bm, edges):
    """A wooden trim along every edge off the ground (hips, ridges, gable
    edges), sitting on the two faces it joins."""
    for a, b, ns in edges:
        if a.z < 1e-3 and b.z < 1e-3:
            continue
        d = (b - a).normalized()
        up = sum(ns, Vector()).normalized() if ns else Vector((0.0, 0.0, 1.0))
        side = d.cross(up).normalized()
        up = side.cross(d).normalized()
        if up.z < 0:
            up = -up
        w, h = 0.15, 0.08
        # Stop a hair inside the top corners, and short of a ground corner so
        # the trim stays inside the cell; nothing dips under the ground.
        a2 = a + d * (0.2 if a.z < 1e-3 else 0.02)
        b2 = b - d * (0.2 if b.z < 1e-3 else 0.02)
        section = [side * -w + up * -0.03, side * w + up * -0.03, side * w + up * h, side * -w + up * h]
        secs = [[a2 + c for c in section], [b2 + c for c in section]]
        lim = HALF_W - 0.005
        for sec in secs:
            for c in sec:
                c.x = max(-lim, min(lim, c.x))
                c.y = max(-lim, min(lim, c.y))
                c.z = max(c.z, 0.0)
        lofted_bar(bm, secs, "stump_bark")


def roof_caps(bm, points):
    """A chunky wooden cap on every top corner (the apex, raised corners)."""
    tops = sorted({(round(p.x, 3), round(p.y, 3), round(p.z, 3)) for p in points if p.z > 0.5})
    for x, y, z in tops:
        c = Vector((x, y, z))
        # Pull corner caps in so they stay inside the cell.
        c.x = max(-HALF_W + 0.19, min(HALF_W - 0.19, c.x))
        c.y = max(-HALF_W + 0.19, min(HALF_W - 0.19, c.y))
        add_box(bm, (c.x - 0.13, c.y - 0.13, z - 0.1), (c.x + 0.13, c.y + 0.13, z + 0.1),
                "trunk", xf=Matrix.Rotation(math.radians(45), 4, "Z"))


def build_roof(root, raised, stage):
    """A plank roof over the cone hull with `raised` corners: a dark underlay,
    level planks with nails on every face, trims on every edge, caps on the
    top corners. Stage 1 cracks planks; stage 2 splits one and cracks more."""
    bm = palette.new_bmesh()
    r = shapes.rng(59 + 7 * len(raised) + sum(raised))
    points = cone_points(raised)
    faces, edges = hull(points)
    # The underlay: the hull sunk under the plank tops, so gaps read dark and
    # the underside (seen from inside a box) is solid.
    under = []
    for p in points:
        q = Vector((p.x * 0.985, p.y * 0.985, max(p.z - ROOF_UNDER, 0.0)))
        under.append(q)
    u_faces, _ = hull(under)
    for poly, n in u_faces:
        add_poly(bm, poly, "stump_bark", n)
    planks = roof_planks(bm, faces, r)
    # Nails near the ends of each face's lowest plank.
    for i, (top, length, n, row) in enumerate(planks):
        if length < 1.0 or row != 0:
            continue
        for t in (0.22, length - 0.22):
            if stage >= 2 and i == 2 and t < 1.0:
                continue  # popped out
            nail(bm, top(t) - n * 0.004, n, radius=0.075, sides=5, phase=0.4 * i)
    if stage >= 1:
        picks = [(1, 0.35), (4, 0.6)] + ([(0, 0.25), (3, 0.5), (6, 0.45), (7, 0.3)] if stage >= 2 else [])
        for i, at in picks:
            if i >= len(planks):
                continue
            top, length, n, _ = planks[i]
            a = length * at
            pts = [(a, -0.1), (a + 0.28, 0.05), (a + 0.5, -0.07), (a + 0.75, 0.06)]
            pts = [(min(t, length - 0.05), w) for t, w in pts]
            plank_crack(bm, top, n, pts)
    if stage >= 2 and len(planks) > 2:
        # A split plank: a dark gap across it, the broken end sagging.
        top, length, n, _ = planks[2]
        a = length * 0.45
        gap = [top(a, -0.2), top(a + 0.28, -0.2), top(a + 0.22, 0.2), top(a - 0.04, 0.2)]
        add_poly(bm, [p + n * 0.006 for p in gap], CRACK, n)
    roof_trims(bm, edges)
    roof_caps(bm, points)
    finish(root, bm)
    scene.make_attach("Top", root, (0.0, 0.0, CONE_H))


CONE_SHAPES = {
    "": [],
    "_peak": [0],
    "_slope": [0, 1],
    "_ridge": [0, 3],
    "_tent": [0, 1, 2],
}


def _cone_builder(raised, stage):
    def build(root):
        build_roof(root, raised, stage)
    build.__doc__ = f"plank roof, raised corners {raised}, crack stage {stage}"
    return build


# ---------------------------------------------------------------------------
# Debris
# ---------------------------------------------------------------------------

def build_brick_chunk(root):
    """A chunky broken brick with a crumb of mortar on it, about 0.3 m: what a
    wall bursts into. Centred on the origin (it tumbles)."""
    bm = palette.new_bmesh()
    # The brick: a box whose +x end is a jagged break.
    x0, x1 = -0.19, 0.14
    y0, y1 = -0.12, 0.12
    z0, z1 = -0.13, 0.13
    left = [(x0, y0, z0), (x0, y1, z0), (x0, y1, z1), (x0, y0, z1)]
    jag = [(x1 + 0.05, y0, z0), (x1 - 0.02, y1, z0), (x1 + 0.07, y1, z1), (x1 - 0.04, y0, z1)]
    lv = [bm.verts.new(p) for p in left]
    jv = [bm.verts.new(p) for p in jag]
    faces = shapes.bridge(bm, lv, jv)
    faces.append(bm.faces.new(list(reversed(lv))))
    tip = bm.verts.new((x1 + 0.1, 0.0, 0.01))
    for i in range(4):
        faces.append(bm.faces.new((jv[i], jv[(i + 1) % 4], tip)))
    bmesh.ops.recalc_face_normals(bm, faces=faces)
    palette.tag(bm, faces, "brick")
    add_box(bm, (-0.2, -0.1, 0.12), (-0.02, 0.1, 0.17), "mortar", drop=("-z",))
    finish(root, bm)


def build_plank_splinter(root):
    """A snapped plank end with a splintered tip and a nail, about 0.7 m."""
    bm = palette.new_bmesh()
    w, t = 0.13, 0.05
    back = [(-w, -0.32, -t), (w, -0.32, -t), (w, -0.32, t), (-w, -0.32, t)]
    mid = [(-w, 0.18, -t), (w, 0.2, -t), (w, 0.2, t), (-w, 0.18, t)]
    bv = [bm.verts.new(p) for p in back]
    mv = [bm.verts.new(p) for p in mid]
    faces = shapes.bridge(bm, bv, mv)
    faces.append(bm.faces.new(list(reversed(bv))))
    # Splinters: two long teeth and a short one.
    teeth = [bm.verts.new(p) for p in ((-w * 0.5, 0.42, 0.0), (w * 0.55, 0.34, 0.01))]
    notch = bm.verts.new((0.0, 0.24, 0.0))
    faces.append(bm.faces.new((mv[0], mv[3], teeth[0])))
    faces.append(bm.faces.new((mv[3], mv[2], notch, teeth[0])))
    faces.append(bm.faces.new((mv[1], mv[0], teeth[0], notch)))
    faces.append(bm.faces.new((mv[2], mv[1], teeth[1])))
    faces.append(bm.faces.new((mv[1], notch, teeth[1])))
    faces.append(bm.faces.new((notch, mv[2], teeth[1])))
    bmesh.ops.recalc_face_normals(bm, faces=faces)
    palette.tag(bm, faces, "plank")
    nail(bm, (0.0, -0.2, t), (0, 0, 1), radius=0.06, sides=5)
    finish(root, bm)


ASSETS = [
    Asset("wall_brick", "wall", build_wall_brick, "brick wall, 4 x 3 m"),
    Asset("wall_brick_crack1", "wall", build_wall_brick_crack1, "brick wall at 66% HP: cracks"),
    Asset("wall_brick_crack2", "wall", build_wall_brick_crack2,
          "brick wall at 33% HP: big cracks, missing bricks"),
    Asset("floor_plank", "floor", build_floor_plank, "plank floor, 4 x 4 m"),
    Asset("floor_plank_crack1", "floor", build_floor_plank_crack1, "plank floor at 66% HP"),
    Asset("floor_plank_crack2", "floor", build_floor_plank_crack2,
          "plank floor at 33% HP: split plank"),
    Asset("ramp_plank", "ramp", build_ramp_plank, "plank ramp, 4 m run, 3 m rise"),
    Asset("ramp_plank_crack1", "ramp", build_ramp_plank_crack1, "plank ramp at 66% HP"),
    Asset("ramp_plank_crack2", "ramp", build_ramp_plank_crack2,
          "plank ramp at 33% HP: split plank"),
    *[Asset(f"cone_plank{shape}{suffix}", "cone", _cone_builder(raised, stage), about)
      for shape, raised in CONE_SHAPES.items()
      for stage, suffix, about in (
          (0, "", f"plank cone{shape.replace('_', ', ')}: 4 x 4 m, 1.5 m tall"),
          (1, "_crack1", f"plank cone{shape.replace('_', ', ')} at 66% HP"),
          (2, "_crack2", f"plank cone{shape.replace('_', ', ')} at 33% HP"))],
    *[Asset(f"{name}{suffix}", kind, _tiles(builder, stage), f"{about}{note}")
      for name, kind, builder, about in (
          ("wall_brick_tiles", "wall_tiles", build_wall_tiles,
           "brick wall edit tiles: 9 tiles, 12 diagonal halves, 3 frame boards"),
          ("floor_plank_tiles", "floor_tiles", build_floor_tiles,
           "plank floor edit tiles: 4 quarters and a cut-edge beam"),
          ("ramp_plank_tiles", "ramp_tiles", build_ramp_tiles,
           "plank ramp edit tiles: left and right halves"))
      for stage, suffix, note in ((0, "", ""), (1, "_crack1", " at 66% HP"),
                                  (2, "_crack2", " at 33% HP"))],
    Asset("brick_chunk", "wall", build_brick_chunk, "wall debris: a broken brick"),
    Asset("plank_splinter", "floor", build_plank_splinter, "floor/ramp debris: a snapped plank"),
]
