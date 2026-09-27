"""Gloves: white four-finger cartoon gloves for the viewmodel (targets T02, T04).

Looney-style: big, fat sausage fingers (three plus a thumb), a puffy palm and a
rolled white cuff, over a dark cloth sleeve that bends at the elbow back toward
the player and runs off screen however far out the gun is held. Two parts:

    GloveR  the right hand, wrapped round a pistol grip
    GloveL  the left hand, cupping the gun from below (forend / pump grip)

Each glove is authored in its grip frame, and that frame is the part's pivot:
place a glove by giving its node the gun's `GripR` / `GripL` attach transform
(`art/blender/assets/guns.py` explains both frames; in Bevy, map the sidecar
frame onto the glTF node with `viewmodel::model_to_node`). Both guns share the pistol
grip and size their left-hand grip to `guns.GRIP_L_HALF`, so one pair of gloves
fits both. Here the gloves sit where they hold the rifle, then the pair is lifted
so its lowest point is on the ground (the pipeline's pivot rule).

Grip frames (Blender axes; the gun faces -Y, +X is the gun's left):
    GripR  +Z runs up the raked pistol grip; the grip is an ellipse of half
           extents (GRIP_R_HALF) round the Z axis. The palm is on the gun's right
           (-X), the fingers wrap round the front and the thumb lies along the
           left side above them.
    GripL  +Z up, the gun along Y; a rounded box of half extents
           guns.GRIP_L_HALF (x, z) round the Y axis. The palm cups the bottom from
           the far (right) side and the fingers curl round the bottom and up the
           near (left) side, where the player sees them, as in T04; the thumb runs
           forward along the far side and the forearm drops away below.
"""

import math

import bmesh
from mathutils import Euler, Vector

from assets import guns
from lib import scene, shapes
from lib.registry import Asset

GRIP_R_HALF = (0.020, 0.029)   # pistol grip half width (x) and depth (y)
FINGER_SEG = 12
# Forearm directions (unit, in each grip frame), from the wrist toward the elbow.
# They point back toward the player so the sleeves leave the screen.
FOREARM_R = Vector((-0.10, 0.695, -0.709)).normalized()
FOREARM_L = Vector((0.42, 0.06, -0.90)).normalized()
# Past the elbow the sleeves turn back toward the player (and the left one out
# to the player's side), `SLEEVE_REACH` long: the arms come from below the
# camera, however far out the gun is held.
ELBOW_R = Vector((-0.05, 0.80, -0.60)).normalized()
ELBOW_L = Vector((0.40, 0.58, -0.71)).normalized()
SLEEVE_REACH = 0.62
# The sleeves are dark, matte cloth (the targets' near-black violet sleeves).
SLEEVE = "hud_panel"


def sausage(points, radius, color="glove_white", seg=FINGER_SEG, round_end=True,
            round_start=False):
    """A finger: a tube through `points` with a hemispherical tip."""
    pts = [Vector(p) for p in points]
    radii = [radius] * len(pts)
    if round_end:
        t = (pts[-1] - pts[-2]).normalized()
        end = pts[-1]
        for ang in (48.0, 84.0):
            a = math.radians(ang)
            pts.append(end + t * radius * math.sin(a))
            radii.append(radius * math.cos(a))
    if round_start:
        t = (pts[0] - pts[1]).normalized()
        start = pts[0]
        for ang in (48.0, 84.0):
            a = math.radians(ang)
            pts.insert(0, start + t * radius * math.sin(a))
            radii.insert(0, radius * math.cos(a))
    bm = bmesh.new()
    # The root end is buried in the palm, so it needs no cap.
    shapes.loft(bm, [(c, (lambda r: lambda a: r)(r)) for c, r in zip(pts, radii)], seg,
                cap_start=round_start)
    return guns.finish(bm, color)


def blob(center, half, color="glove_white", exponent=2.5, rotation=(0.0, 0.0, 0.0),
         u=10, v=6):
    """A puffy rounded box (superellipsoid) of half extents `half`."""
    bm = bmesh.new()
    bmesh.ops.create_uvsphere(bm, u_segments=u, v_segments=v, radius=1.0)
    for vert in bm.verts:
        p = shapes.superellipsoid_point(vert.co.normalized(), exponent)
        vert.co = Vector((p.x * half[0], p.y * half[1], p.z * half[2]))
    return guns.finish(bm, color, matrix=guns.place(center, rotation))


def cuff_and_sleeve(wrist, forearm, elbow, objs, seg=12, scale=1.0):
    """The rolled glove cuff at the wrist and a dark sleeve that leaves along
    the forearm, then bends at the elbow back toward the player, so it runs off
    screen from however far out the gun is held (T01-T05)."""
    profile = [(t * scale, r * scale) for t, r in
               ((0.000, 0.026), (0.018, 0.034), (0.040, 0.046), (0.053, 0.045),
                (0.058, 0.036), (0.030, 0.028))]
    objs.append(guns.lathe(wrist, forearm, profile, "glove_white", seg))
    wrist = Vector(wrist)
    start = wrist + forearm * 0.040
    bend = wrist + forearm * 0.13
    end = bend + elbow * SLEEVE_REACH
    # Open-ended: one end is inside the cuff, the other far off screen.
    frames = []
    for c, d, r in ((start, forearm, 0.032 * scale),
                    (bend, (forearm + elbow).normalized(), 0.036 * scale),
                    (end, elbow, 0.052 * scale)):
        u, v = guns.axis_frame(d)
        frames.append((c, u, v, r, r, 2.0))
    objs.append(guns.loft(frames, SLEEVE, seg=10, cap=False))


def right_glove():
    """GloveR in the GripR frame (see the module doc)."""
    a, b = GRIP_R_HALF
    objs = []
    # Three fingers wrapped round the front of the grip, knuckles on the right.
    for z, r in ((0.033, 0.0212), (-0.006, 0.0210), (-0.044, 0.0196)):
        ra, rb = a + r + 0.002, b + r + 0.002
        path = [(-ra * math.cos(math.radians(p)), -rb * math.sin(math.radians(p)), z)
                for p in (-30.0, 25.0, 80.0, 135.0, 185.0)]
        objs.append(sausage(path, r))
    # Thumb: over the web behind the grip, then forward along the gun's left side.
    objs.append(sausage([(-0.028, 0.028, 0.046), (-0.006, 0.048, 0.058), (0.024, 0.040, 0.062),
                         (0.038, 0.014, 0.060), (0.040, -0.012, 0.057)], 0.0220))
    # Palm on the right side and the heel behind it, running into the wrist.
    objs.append(blob((-0.040, 0.013, 0.000), (0.027, 0.046, 0.064)))
    wrist = Vector((-0.026, 0.054, -0.046))
    objs.append(blob((-0.029, 0.039, -0.031), (0.026, 0.029, 0.036), u=8, v=5))
    cuff_and_sleeve(wrist, FOREARM_R, ELBOW_R, objs)
    return objs


def left_glove():
    """GloveL in the GripL frame (see the module doc)."""
    a, b = guns.GRIP_L_HALF
    objs = []
    # Three fingers from knuckles at the bottom (far side), curling round the
    # bottom and up the near side (+X, toward the player's camera).
    for y, r in ((-0.045, 0.0238), (0.000, 0.0236), (0.044, 0.0220)):
        ra, rb = a + r + 0.002, b + r + 0.002
        path = [(ra * math.sin(math.radians(p)), y, -rb * math.cos(math.radians(p)))
                for p in (-42.0, 5.0, 50.0, 95.0, 128.0)]
        objs.append(sausage(path, r))
    # Thumb forward along the far side.
    objs.append(sausage([(-0.052, 0.026, -0.030), (-0.060, 0.002, -0.010), (-0.061, -0.028, -0.002),
                         (-0.059, -0.054, 0.000)], 0.0236))
    # Palm cupping the bottom (far side), heel down toward the wrist.
    objs.append(blob((-0.047, 0.000, -0.061), (0.030, 0.068, 0.042),
                     rotation=(0.0, math.radians(-35.0), 0.0)))
    wrist = Vector((-0.028, 0.014, -0.104))
    objs.append(blob((-0.034, 0.012, -0.084), (0.032, 0.036, 0.036), u=8, v=5))
    cuff_and_sleeve(wrist, FOREARM_L, ELBOW_L, objs, scale=1.12)
    return objs


def glove_part(name, objs, parent, location, rotation):
    """Joins a glove (built in its grip frame) into a part placed at the frame."""
    obj = shapes.join(objs, name + "Tmp")
    mesh = guns.triangulate(shapes.detach_mesh(obj))
    p = scene.make_part(name, mesh, parent, Vector(location))
    p.rotation_euler = Euler(rotation)
    shapes.smooth_shading(p, sharp_angle_deg=80.0)
    return p


def place_gloves(root, grip_r, grip_l):
    """Adds GloveR and GloveL at the given grip frames [(location, euler)]."""
    glove_part("GloveR", right_glove(), root, *grip_r)
    glove_part("GloveL", left_glove(), root, *grip_l)


def build_gloves(root):
    # Where they hold the rifle (its design space; the lift below is uniform).
    place_gloves(root, (guns.grip_r_location(), guns.GRIP_R_ROTATION),
                 (guns.R_GRIP_L, (0.0, 0.0, 0.0)))
    guns.settle_on_ground(root)


ASSETS = [
    Asset("gloves", "gloves", build_gloves, "white four-finger cartoon gloves and dark sleeves"),
]
