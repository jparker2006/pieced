# Research: first-person game feel, building and art readability

Collected 2026-09-24 for Milestone 1. Markers: **[n]** is a source that was opened, **[n\*]** was seen only via a search summary or secondary write-up (unverified), and *Synthesis* is inference rather than a sourced fact.

## Gunfeel

- **Hitscan** for both guns. At arena ranges, projectile travel time only adds latency. Swink puts "instant" at under 100 ms and a human perceive–decide–act cycle at about 240 ms [1\*]. The pump fires several hitscan pellets in a **fixed** pattern, not a random one.
- **Same-frame hit confirmation:**
  - a hitmarker, plus a hit sound distinct from the gunshot;
  - a headshot variant of both, and a floating damage number;
  - a larger version on kills and piece breaks, with 1–2 frames of hitstop on kills only [3].
- **Vlambeer techniques adapted to first person** [3][2\*]: bigger bullets and tracers, muzzle flash, gun kickback, knockback on hit, and more bass in the shot.
  - Put most of the kick in the viewmodel, using a spring that recovers in about 100–150 ms (*Synthesis*).
  - Keep camera recoil small and self-recovering, because pulling down on a trackpad is hard.
- **Keep screenshake minimal.** In first person, shake is aim noise. The accessibility guidelines say weapon bob and anything else between input and camera must be avoidable [4]. Shake only on nearby piece breaks, at no more than about 0.5°, with a slider down to 0.
- **From Fortnite Ballistic** (Epic's first-person mode) [5\*]: offer toggles for reticle bloom and "reticle follows recoil".
- **Fire on press, not release.** Buffer a pump press made up to about 150 ms early.
- **TTK versus reactive building.** COD's average TTK is about 0.3 s, roughly one reaction cycle [6\*], so nobody can build in reaction. Fortnite's turbo build places a piece every 0.05 s [7]. Reactive building only works when TTK is well above reaction time. A wall should soak at least 1 s of rifle fire, and the pump should be the box-cracker.

## Movement

- Community measurements put Valorant's run at about 6.75 m/s [8\*].
- For responsiveness (*Synthesis*):
  - Reach full speed in about 0.08–0.1 s and stop in about 0.06 s, inside Swink's 100 ms window.
  - Air control 30–50%.
  - About 100 ms of jump buffer and coyote time.
  - Jump height about 1.2 m, so a jump never clears a wall. Fortnite walls are 3.84 m tall [9\*].
- **Look input:** winit says cursor-position events include OS acceleration and must not drive a 3D camera; use raw mouse motion [10]. No look smoothing [4].
- **FOV:** Bevy's FOV is vertical. 70° vertical is about 103° horizontal at 16:9 and about 95° at the Air's ~1.54 aspect. Offer a 60–90° slider.
- **Bob:** no camera bob; viewmodel sway only, with an off switch [4].
- **Frame rate:** a steady frame rate matters more than a high one [4]. Use vsync at 60 and a fixed 60 Hz gameplay step.

## Building in first person

- **Grid size:** a Fortnite tile is 512 × 512 cm and 384 cm tall [9\*]. A compact solo arena can use 4 × 4 × 3 m (*Synthesis*).
- **Targeting** (common convention, *Synthesis*): raycast from the camera and pick the cell next to the player in the look direction.
  - Walls go on the edge you face, snapped to four yaws.
  - Ramps rise away from you.
  - Floors go at foot level, or one level up when you look up.
  - Always show a ghost preview, tinted when the spot is invalid.
- **Reach:** the player's own cell plus one outward. Longer reach makes first-person placement fiddly.
- **Speed** [7]:
  - Hold to keep placing every 0.05 s while the ghost moves.
  - After a piece is destroyed, lock its spot for 0.15 s before rebuilding.
  - "Builder Pro" style, where the piece key both selects and places, is an option.
- **Readability when boxed in:**
  - bright, flat interior faces, with no dark ambient;
  - HP bar and crack states (66% / 33%) on the piece under the crosshair;
  - strong damage-direction indicators and positional audio;
  - enemy rim light that reads through gaps [11];
  - camera near plane about 0.05 m and player radius at least 0.3 m, so walls never clip.
- **Caution:** Epic's own first-person mode, Ballistic, had **no building** and was reportedly shut down in April 2026 [5\*]. First-person building is unproven. Keep it to three pieces and prove the feel first.

## Trackpad

- No high-quality study of FPS aiming on a trackpad was found. Treat it as open design space.
- **macOS:** `Confined` cursor grab does not work; `Locked` does [10]. Re-grab after regaining focus.
- **Unknown:** whether macOS trackpad deltas carry acceleration. Measure logged deltas against finger travel.
- **Suggested defaults** (*Synthesis*):
  - high default sensitivity, about 180° per full-width swipe;
  - an optional acceleration curve;
  - a separate sensitivity for build mode;
  - mild aim friction on the rifle, behind a toggle [12\*].
- **Keys:** Apple recommends single-key commands and rebindable keys when a mouse or trackpad is in use [13]. Fire with a **physical press**; palm rejection may suppress taps while keys are held [14\*]. Beware accidental Cmd+Q, Cmd+W and Cmd+H.

## Low-poly readability

- **TF2 [11]:**
  - silhouette-first character design;
  - muted world with small saturated accents;
  - fine detail omitted;
  - rim highlights instead of dark outlines;
  - half-Lambert shading (dot × 0.5 + 0.5, squared) keeps form on the dark side.
- **Valorant [15]:**
  - minimum spec is a 2012 integrated GPU;
  - forward renderer with one directional light plus baked lighting;
  - enemy edge glow, and distant characters are brightened;
  - fixed shader budgets per quality level.
- **For Pieced** (*Synthesis*):
  - the target gets one saturated color used nowhere else, plus a rim light, against a muted mid-tone arena;
  - building materials differ in brightness, not only hue;
  - prefer MSAA over post-processing;
  - check readability with a greyscale screenshot.

## Sources

1. Swink, *Game Feel* (search summary)\* https://books.google.lk/books/about/Game_Feel.html?id=3oFjDwAAQBAJ
2. Nijman, "The art of screenshake"\* https://www.youtube.com/watch?v=AJdEqssNZ-U
3. https://www.bluetengu.com/2014/12/12/art-of-screenshake-experiments/
4. https://gameaccessibilityguidelines.com/avoid-vr-simulation-sickness-triggers/
5. Fortnite Ballistic\* https://en.wikipedia.org/wiki/Fortnite_Ballistic
6. COD TTK\* https://www.dexerto.com/call-of-duty/black-ops-cold-war-breakdown-reveals-longer-ttk-modern-warfare-1418138/
7. https://www.pcgamesn.com/fortnite/turbo-build
8. Valorant speeds (community)\* https://www.switchbladegaming.com/valorant/neon-guide/
9. UEFN tile size\* https://dev.epicgames.com/documentation/fortnite/fall-guys-obstacle-course-assets-in-fortnite-creative
10. https://docs.rs/winit/latest/winit/window/enum.CursorGrabMode.html
11. https://steamcdn-a.akamaihd.net/apps/valve/2007/NPAR07_IllustrativeRenderingInTeamFortress2.pdf
12. Aim assist\* https://www.halopedia.org/Aim_assist
13. https://developer.apple.com/design/human-interface-guidelines/game-controls
14. Palm rejection\* https://forums.macrumors.com/threads/getting-trackpad-to-ignore-touches-while-typing.2103667/
15. https://www.riotgames.com/en/news/valorant-shaders-and-gameplay-clarity
