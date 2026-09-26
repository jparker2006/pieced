#!/bin/bash
# Round 4 target board: 12 gallery-view targets, one at a time, R4-M1 as style reference.
cd "$(dirname "$0")/concepts" || exit 1
LOGDIR="${1:-/tmp}"; START="${2:-0}"
REF=R4-M1-brick-walls-wood-floors.png
REFNOTE="An image is attached. Use it only as the reference for the art style and for the designs of the knight, the rifle, the white glove, the sky and the HUD. Create a new, different composition exactly as described; do not copy the attached layout."
STYLE="Art style: flat TV-cartoon look with flat saturated colors, thin clean dark outlines on near objects only, simple two-tone cel shading with one hard shadow band, soft cartoon blob shadows under characters and no other cast shadows, no gradients or texture noise up close. Far things (sky, space station, distant islands) have no outlines and look softer, glowing and magical. World: a grassy cartoon floating island in space with rounded cliffs, puffy cartoon trees, cartoon rocks and tree stumps, and a faint glowing build grid on the grass. Sky: a swirling purple and teal galaxy; a huge gothic cathedral space station with stained-glass solar sails and glowing spires; a ringed planet low on the horizon; small floating islands with waterfalls spilling into space; tiny starships gliding between the towers. Player-built pieces: walls are chunky cartoon brick with bold mortar lines; floors and ramps are warped cartoon wooden planks with big nail heads. No logos, no real franchise characters."
KNIGHT="the enemy knight: a goofy cartoon armored knight-wizard with a big steel bucket helmet whose visor slit shows big white cartoon eyes, a short floppy purple wizard hat with a gold star that hugs the top of the helmet, oversized gauntlets and boots, a small body, purple trim and a small purple cape"
RIFLE="The player's right hand, in a white four-finger cartoon glove, holds the rifle: a chunky toy-like assault rifle with Fortnite-like proportions, brass body, dark wood stock and grip, a glowing blue rune crystal in a glass chamber, and a small energy-cell magazine."
PUMP="The player's right hand, in a white four-finger cartoon glove, holds the pump: a stubby, chunky toy-like pump shotgun with a flared brass bell muzzle, a wooden pump grip, and a glowing violet crystal wrapped in gold rings."
HUD="Clean cartoon HUD: a cyan shield bar above a green health bar bottom-left in rounded cartoon frames with small crystal icons; ammo count bottom-right with a crystal icon; a hotbar bottom-center with exactly five slots (rifle, pump, wall, ramp, floor; no pickaxe); a small crosshair in the center. No other text."
names=(T01-spawn-vista T02-rifle-idle T03-rifle-bolt T04-pump-fan T05-knight-hit T06-headshot T07-shield-break T08-elimination T09-fort T10-station-up T11-island-edge T12-pause-menu)
scenes=(
"The player stands at spawn at one end of the island, holding the rifle at the hip and looking across the whole grassy arena: rocks, tree stumps and puffy trees as cover, the build grid on the grass, and $KNIGHT standing small on the far side. The cathedral station fills the right side of the sky, the galaxy swirl the left side and overhead, the ringed planet low on the horizon, a ring of small waterfall islands around the arena, and ships gliding. A wide, inviting establishing shot. $RIFLE $HUD"
"A close look at the held rifle at rest, tilted slightly toward the viewer so every detail reads: the brass receiver, the dark wood stock, the blue rune crystal glowing brightly in its glass chamber (full magazine), the energy-cell magazine and the chunky sights. Behind it: grass, a puffy tree, one brick wall the player built, and the sky. No enemy. $RIFLE $HUD"
"The rifle has just fired: a small blue star-shaped muzzle burst, the gun kicking up with a squash-and-stretch pose, and a glowing blue starburst spell bolt with a sparkling trail flying toward $KNIGHT, who is running across the grass about 20 meters away. $RIFLE $HUD"
"The pump has just fired at close range: a wide fan of violet and gold spell sparks spreading from the bell muzzle and peppering $KNIGHT, about 5 meters away, with many small violet-and-gold impact bursts. $PUMP $HUD The ammo count shows 5."
"$KNIGHT stands about 10 meters away on the grass beside a rock and takes a body hit from a blue spell bolt: a bright blue-white impact starburst on his chest, his body wobbling in a squash-and-stretch pose, his cartoon eyes wide in surprise, and a floating white damage number 24 popping above him. A white hitmarker on the crosshair. $RIFLE $HUD"
"A headshot: $KNIGHT, about 12 meters away, is hit in the helmet. A bright gold flash burst on the helmet, his floppy hat bouncing up off his head for a moment, a larger gold damage number 48 popping above him, and a gold hitmarker on the crosshair. $RIFLE $HUD"
"The shield of $KNIGHT breaks: a translucent cyan glassy bubble around him shatters into flying cyan glass shards, and little cartoon stars and sparkles circle his helmet as he staggers dizzily, about 8 meters away. $RIFLE $HUD"
"The knight has just been eliminated about 8 meters away: a big puffy white cartoon poof cloud with sparkles where he stood, and his floppy purple wizard hat with the gold star dropping out of the cloud and spinning on the grass. A small gold elimination burst around the crosshair. $RIFLE $HUD"
"The player has just built a 1-by-1 fort on the grid and looks at it from just outside: three chunky cartoon brick walls with the near side left open, a warped wooden-plank ramp inside leading up, and a wooden plank floor on top. Next to the fort, a translucent glowing blue ghost preview of the next brick wall hovers on the grid. $RIFLE $HUD The hotbar's wall slot is highlighted."
"The player looks up at the sky from the island: the huge gothic cathedral space station fills most of the frame, its stained-glass solar sails glowing softly, small starships with light trails gliding between its spires, the swirling purple and teal galaxy behind it, and the ringed planet near the edge. Only the tops of puffy trees at the bottom of the frame and the rifle's barrel tip in the lower right. $HUD"
"The player stands at the edge of the island: the grass ends at rounded cartoon cliffs dropping away into space; a faint, shimmering, translucent magic barrier with glowing rune lines rises at the edge; beyond it a ring of small floating islands bob with waterfalls spilling into the starry void, ships glide past, and the ringed planet sits low on the horizon. $RIFLE $HUD"
"The game's pause menu over a blurred, dimmed view of the island and the galaxy sky: a big cartoon title logo reading PIECED in chunky brass-and-crystal letters with a tiny floppy wizard hat on the letter I, and below it three rounded cartoon buttons reading RESUME, SETTINGS and QUIT, in clean cartoon frames with small crystal icons. No other text, no HUD, no gun."
)
for i in "${!names[@]}"; do
  [ "$i" -lt "$START" ] && continue
  n="${names[$i]}"
  s=$(date +%s)
  if [ "$i" -eq 11 ]; then kind="a video game pause menu screen, 16:9 landscape."; else kind="a first-person video game screenshot, 16:9 landscape."; fi
  codex exec --skip-git-repo-check --ephemeral -m gpt-5.5 -s workspace-write -C "$PWD" -i "$REF" -- \
    "Use your image generation tool to create exactly one image and save it as $n.png in the current directory. $REFNOTE The image: $kind ${scenes[$i]} $STYLE" \
    < /dev/null > "$LOGDIR/$n.log" 2>&1
  # Codex sometimes reports "saved" without copying; recover from its own output folder.
  if [ ! -f "$n.png" ]; then
    sid=$(awk '/^session id:/ {print $3; exit}' "$LOGDIR/$n.log")
    [ -n "$sid" ] && cp "$HOME/.codex/generated_images/$sid/"ig_*.png "$n.png" 2>/dev/null
  fi
  echo "$n exit=$? secs=$(( $(date +%s) - s )) $(ls -la "$n.png" 2>&1 | awk '{print $5}')"
done
