#!/bin/sh
# Builds the game's music (assets/music/) from the CC BY 4.0 sources Jake picked
# (art/music/src/, credited in art/music/src/CREDITS.md and assets/ASSETS.md):
# resamples to 44.1 kHz, trims, loops, loudness-normalizes and encodes them as Ogg Vorbis, and writes
# assets/music/music.json (every file's length and loop start, read by
# src/audio/music.rs).
#
#   scripts/build-music.sh           # rebuild assets/music/
#   scripts/build-music.sh --check   # rebuild into a temp dir and fail unless it
#                                    # matches the committed files byte for byte
#
# Needs ffmpeg (`ffmpeg` on PATH, /opt/homebrew/bin/ffmpeg, or $FFMPEG). The encode
# is deterministic for a given ffmpeg build (bit-exact flags, fixed Ogg serials);
# checked with ffmpeg 8.1.1.
#
# Loops: a loop file is  src[S, B + X)  where the last X samples are an
# equal-power crossfade of src[B, B + X) (fading out) into src[A, A + X) (fading
# in), and playback loops from file frame A + X - S back after the end. So the
# first pass plays the source untouched from S (the intro), and every later pass
# comes round through the crossfade, which ends exactly where the loop restarts.
# A and B sit 30 ms before matching attacks a whole number of bars apart; they
# were found by analysing the sources (chroma and band-energy self-similarity to
# find the restatements, then onset and waveform alignment to the sample):
#
#   combat_low   108 bpm, 22 bars: the opening theme (2.41 s) and its
#                restatement (51.32 s); waveform correlation 0.54 over 0.4 s,
#                chroma 0.94 over 0.5 s.
#   combat_high  123.1 bpm, 28 bars: the theme at 9.38 s and its restatement at
#                63.98 s; waveform 0.53 over 1 s, chroma 0.96.
#   menu         plays whole: its natural ending fades into its quiet intro.
#
# The death sting is derived at load (src/audio/music.rs bends this chord down),
# from a sustained brass C7 chord in the battle cue (70.46 s).
set -eu
cd "$(dirname "$0")/.."

FFMPEG="${FFMPEG:-}"
if [ -z "$FFMPEG" ]; then
  if command -v ffmpeg >/dev/null 2>&1; then FFMPEG=ffmpeg
  elif [ -x /opt/homebrew/bin/ffmpeg ]; then FFMPEG=/opt/homebrew/bin/ffmpeg
  else echo "build-music: ffmpeg not found (set FFMPEG)" >&2; exit 1
  fi
fi

SRC=art/music/src
OUT=assets/music
CHECK=0
if [ "${1:-}" = "--check" ]; then
  CHECK=1
  OUT=$(mktemp -d -t pieced-music-check)
fi
mkdir -p "$OUT"

RATE=44100
# Loops sit under the effects; stings are events and a little louder.
LOOP_LUFS=-18
STING_LUFS=-16
# Native Vorbis encoder (libvorbis isn't in Homebrew's ffmpeg): quality 6 is
# about 170 kbps stereo.
QUALITY=6

ENTRIES=""

# Wait while Jake plays a logged session (scripts/quiet.sh), like the builds.
while [ -f "${PIECED_TARGET_DIR:-$HOME/Library/Caches/pieced-target}/.quiet" ]; do sleep 10; done

# measure <input> <filter graph ending in [out]>: prints integrated loudness (LUFS).
measure() {
  taskpolicy -c utility nice -n 10 "$FFMPEG" -hide_banner -nostats -v info -i "$1" \
    -filter_complex "$2;[out]ebur128=framelog=quiet[m]" -map "[m]" -f null - 2>&1 |
    awk '/Integrated loudness:/ { want = 1 } want && $1 == "I:" { print $2; exit }'
}

# encode <input> <filter graph ending in [out]> <target LUFS> <output name> <title> <comment>
encode() {
  input=$1 graph=$2 target=$3 name=$4 title=$5 comment=$6
  lufs=$(measure "$input" "$graph")
  [ -n "$lufs" ] || { echo "build-music: couldn't measure $name" >&2; exit 1; }
  gain=$(awk -v t="$target" -v m="$lufs" 'BEGIN { printf "%.2f", t - m }')
  taskpolicy -c utility nice -n 10 "$FFMPEG" -hide_banner -nostats -v error -y -i "$input" \
    -filter_complex "$graph;[out]volume=${gain}dB:precision=double[v]" -map "[v]" \
    -ar "$RATE" -ac 2 -c:a vorbis -strict experimental -q:a "$QUALITY" \
    -map_metadata -1 -metadata title="$title" -metadata license="CC BY 4.0" \
    -metadata comment="$comment" \
    -fflags +bitexact -flags:a +bitexact "$OUT/$name.ogg"
  echo "build-music: $name.ogg  $lufs LUFS -> $target ($gain dB)"
}

# note <name> <frames> <loop start frame or -1>
note() {
  ENTRIES="$ENTRIES${ENTRIES:+,
}  {\"name\": \"$1\", \"frames\": $2, \"loop_start\": $3}"
}

# loop <name> <source> <S> <A> <bars length L> <X> <title> <comment> (sample frames)
loop() {
  name=$1 src=$2 s=$3 a=$4 len=$5 x=$6
  b=$((a + len))
  graph="[0:a]aresample=$RATE,asplit=3[p][q][r];\
[p]atrim=start_sample=$s:end_sample=$b,asetpts=PTS-STARTPTS[body];\
[q]atrim=start_sample=$b:end_sample=$((b + x)),asetpts=PTS-STARTPTS,afade=t=out:start_sample=0:nb_samples=$x:curve=qsin[tail];\
[r]atrim=start_sample=$a:end_sample=$((a + x)),asetpts=PTS-STARTPTS,afade=t=in:start_sample=0:nb_samples=$x:curve=qsin[head];\
[tail][head]amix=inputs=2:normalize=0:duration=longest[xf];\
[body][xf]concat=n=2:v=0:a=1[out]"
  encode "$SRC/$src" "$graph" "$LOOP_LUFS" "$name" "$7" "$8"
  note "$name" $((b + x - s)) $((a + x - s))
}

# cut <name> <source> <S> <E> <fade-in frames> <fade-out frames> <target> <title> <comment>
cut() {
  name=$1 src=$2 s=$3 e=$4 fin=$5 fout=$6
  graph="[0:a]aresample=$RATE,atrim=start_sample=$s:end_sample=$e,asetpts=PTS-STARTPTS,\
afade=t=in:start_sample=0:nb_samples=$fin:curve=qsin,\
afade=t=out:start_sample=$((e - s - fout)):nb_samples=$fout:curve=qsin[out]"
  encode "$SRC/$src" "$graph" "$7" "$name" "$8" "$9"
  # A 10th argument of "whole" makes the cut loop whole (loop start 0).
  if [ "${10:-}" = whole ]; then note "$name" $((e - s)) 0; else note "$name" $((e - s)) -1; fi
}

# The menu: the whole Space Fanfare, 0.28 s to 48.0 s, its ending eased out
# over the last 2 s so it comes round into its quiet intro.
cut menu menu-space-fanfare.ogg 12348 2116800 64 88200 "$LOOP_LUFS" \
  "Space Fanfare (Pieced menu loop)" \
  "From \"Space Fanfare - Cinematic Orchestral Music (Star Trek Inspired)\" by humanoide9000 (freesound.org/s/744049/), CC BY 4.0" \
  whole

# Combat low: the opening from 0.40 s; loops 2.382 s <-> 51.287 s.
loop combat_low combat-low-orchestral-adventure.ogg 17640 105046 2156712 44100 \
  "Orchestral Adventure (Pieced combat loop)" \
  "From \"Cinematic orchestral adventure music\" by humanoide9000 (freesound.org/s/689177/), CC BY 4.0"

# Combat high: starts on the theme at 9.351 s; loops 9.351 s <-> 63.955 s.
loop combat_high combat-high-battle-star-wars-style.ogg 412375 412375 2408042 44100 \
  "Battle (Pieced intense combat loop)" \
  "From \"Cinematic Battle Music (Star Wars Style)\" by humanoide9000 (freesound.org/s/685841/), CC BY 4.0"

# Stings.
cut sting_round sting-round-orchestral-victory-fanfare.ogg 0 198450 32 11025 "$STING_LUFS" \
  "Orchestral Victory Fanfare (Pieced round-start sting)" \
  "From \"Music: Orchestral Victory Fanfare\" by Sheyvan (freesound.org/s/470083/), CC BY 4.0"
cut sting_best sting-best-victory-fanfare.ogg 7938 454230 32 17640 "$STING_LUFS" \
  "Victory Fanfare (Pieced new-best sting)" \
  "From \"Victory Fanfare\" by humanoide9000 (freesound.org/s/466133/), CC BY 4.0"
cut death_chord combat-high-battle-star-wars-style.ogg 3107286 3184020 220 1323 "$STING_LUFS" \
  "Brass chord (source of Pieced's death sting)" \
  "From \"Cinematic Battle Music (Star Wars Style)\" by humanoide9000 (freesound.org/s/685841/), CC BY 4.0"

printf '[\n%s\n]\n' "$ENTRIES" > "$OUT/music.json"

if [ "$CHECK" -eq 1 ]; then
  bad=0
  for f in $(cd "$OUT" && ls); do
    if ! cmp -s "$OUT/$f" "assets/music/$f"; then
      echo "build-music --check: assets/music/$f differs from a fresh build" >&2
      bad=1
    fi
  done
  for f in $(cd assets/music && ls); do
    [ -e "$OUT/$f" ] || { echo "build-music --check: assets/music/$f is not produced by the build" >&2; bad=1; }
  done
  rm -rf "$OUT"
  [ "$bad" -eq 0 ] || exit 1
  echo "build-music --check: assets/music matches a fresh build"
fi
