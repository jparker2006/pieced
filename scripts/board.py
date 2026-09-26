#!/usr/bin/env python3
"""Make the Milestone 2 target board: one local HTML page that shows every
gallery view beside its target image, for Jake to score (gate S1).

Usage:
  python3 scripts/board.py GALLERY_DIR [OUT_HTML] [--targets DIR] [--inline]

GALLERY_DIR is a gallery run: a native `gallery` evidence folder
(`summary.json`) or the offscreen render's folder (`gallery.json`), holding
`T01-spawn-vista.png` ... `T12-pause-menu.png` and their `-grey.png` copies.

OUT_HTML defaults to `evidence/m2-board/index.html` (git-ignored). The images
are copied next to it (`img/`) and referenced relatively, so the folder is
self-contained; `--inline` embeds them in the page instead. The page uses no
network at all.

Targets come from `docs/design/concepts/` (local only, never committed); in a
worktree the main checkout's copy is used. `--targets` points elsewhere.

Each row shows the target and the game shot side by side with a greyscale
toggle, a 1-5 score and a notes box. Scores and notes persist in the browser,
and "Copy scores" puts them all on the clipboard as plain text to paste into
chat.

Standard library only.
"""

import base64
import datetime
import html
import json
import os
import shutil
import subprocess
import sys

# The twelve views, in board order: (id, file stem, what the target shows).
# The gallery's own manifest wins when it has one.
VIEWS = [
    ("T01", "T01-spawn-vista", "Spawn vista"),
    ("T02", "T02-rifle-idle", "Rifle idle, gun tilted toward the viewer"),
    ("T03", "T03-rifle-bolt", "Rifle bolt in flight; the knight running at about 20 m"),
    ("T04", "T04-pump-fan", "Pump spark fan; the knight at about 5 m"),
    ("T05", "T05-knight-hit", "Knight body hit at about 10 m: wide eyes, damage number"),
    ("T06", "T06-headshot", "Headshot at about 12 m: hat bouncing, gold number"),
    ("T07", "T07-shield-break", "Shield break at about 8 m, stars"),
    ("T08", "T08-elimination", "Elimination poof at about 8 m, hat spinning on the grass"),
    ("T09", "T09-fort", "A 1×1 fort with a blue ghost wall beside it; wall slot selected"),
    ("T10", "T10-station-up", "Looking up at the station"),
    ("T11", "T11-island-edge", "The island edge and barrier"),
    ("T12", "T12-pause-menu", "The pause menu"),
]
PASS_SCORE = 4

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def main_checkout(repo):
    """The main checkout's root when `repo` is a git worktree, else `repo`."""
    try:
        common = subprocess.run(
            ["git", "-C", repo, "rev-parse", "--path-format=absolute", "--git-common-dir"],
            capture_output=True,
            text=True,
            check=True,
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        return repo
    return os.path.dirname(common) if common else repo


def default_targets(repo=REPO):
    for root in (repo, main_checkout(repo)):
        path = os.path.join(root, "docs", "design", "concepts")
        if os.path.isdir(path):
            return path
    return os.path.join(repo, "docs", "design", "concepts")


def load_run(gallery_dir):
    """The run's metadata: its views (id, name, title, record) and a label."""
    info = {"label": os.path.basename(os.path.normpath(gallery_dir)), "commit": None,
            "kind": "unknown", "records": {}}
    listed = None
    for name, kind in (("gallery.json", "offscreen render"), ("summary.json", "native run")):
        path = os.path.join(gallery_dir, name)
        if not os.path.isfile(path):
            continue
        with open(path, encoding="utf-8") as f:
            doc = json.load(f)
        info["kind"] = kind
        info["commit"] = doc.get("commit")
        scenario = doc.get("scenario", doc) if isinstance(doc, dict) else {}
        views = scenario.get("views") if isinstance(scenario, dict) else None
        if isinstance(views, list) and views and isinstance(views[0], dict):
            listed = views
        break
    views = []
    if listed:
        for v in listed:
            vid, stem = v.get("id"), v.get("name")
            if not vid or not stem:
                continue
            views.append((vid, stem, v.get("title") or stem))
            info["records"][vid] = v
    known = {v[0] for v in views}
    for vid, stem, title in VIEWS:
        if vid not in known:
            views.append((vid, stem, title))
    views.sort(key=lambda v: v[0])
    info["views"] = views
    return info


def find_target(targets_dir, vid, stem):
    exact = os.path.join(targets_dir, stem + ".png")
    if os.path.isfile(exact):
        return exact
    if os.path.isdir(targets_dir):
        for name in sorted(os.listdir(targets_dir)):
            if name.startswith(vid + "-") and name.endswith(".png"):
                return os.path.join(targets_dir, name)
    return None


def data_uri(path):
    with open(path, "rb") as f:
        return "data:image/png;base64," + base64.b64encode(f.read()).decode("ascii")


class Images:
    """Places images for the page: copied into `img/` beside it, or inlined."""

    def __init__(self, out_dir, inline):
        self.out_dir = out_dir
        self.inline = inline
        self.img_dir = os.path.join(out_dir, "img")
        if not inline:
            os.makedirs(self.img_dir, exist_ok=True)

    def src(self, path, name):
        if not path or not os.path.isfile(path):
            return None
        if self.inline:
            return data_uri(path)
        dest = os.path.join(self.img_dir, name)
        if os.path.abspath(path) != os.path.abspath(dest):
            shutil.copyfile(path, dest)
        return "img/" + name


def record_line(record):
    """A one-line account of what happened in the view, from the run's record."""
    if not record:
        return ""
    parts = []
    for key, label in (("shots", "shots"), ("hits", "hits"), ("headshots", "headshots"),
                       ("shield_breaks", "shield breaks"), ("kills", "eliminations")):
        n = record.get(key)
        if isinstance(n, int) and n:
            parts.append(f"{n} {label}")
    if record.get("missed"):
        parts.append("moment missed (captured at timeout)")
    if record.get("rejected"):
        parts.append(f"{len(record['rejected'])} pieces rejected")
    if record.get("paused"):
        parts.append("paused")
    return ", ".join(parts)


CSS = """
:root { color-scheme: light dark; --bg: #f4f1ea; --card: #fffdf8; --ink: #22201c;
  --dim: #6d675c; --line: #ddd5c6; --accent: #7a4fd6; --good: #1f8a4c; --warn: #b3541e; }
@media (prefers-color-scheme: dark) { :root { --bg: #16151a; --card: #211f27;
  --ink: #ece8f4; --dim: #a39db0; --line: #3a3644; --accent: #b18cff; --good: #5fd08f;
  --warn: #ff9b62; } }
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--ink);
  font: 15px/1.45 -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif; }
header { position: sticky; top: 0; z-index: 2; background: var(--bg);
  border-bottom: 1px solid var(--line); padding: 12px 16px; display: flex;
  flex-wrap: wrap; gap: 8px 16px; align-items: center; }
header h1 { font-size: 18px; margin: 0; flex: 1 1 auto; }
header .meta { color: var(--dim); font-size: 13px; width: 100%; }
button, label.toggle { font: inherit; border: 1px solid var(--line); background: var(--card);
  color: var(--ink); border-radius: 8px; padding: 6px 12px; cursor: pointer; }
button.primary { background: var(--accent); border-color: var(--accent); color: #fff; }
#progress { color: var(--dim); font-size: 13px; }
main { max-width: 1500px; margin: 0 auto; padding: 16px; display: grid; gap: 16px; }
section.view { background: var(--card); border: 1px solid var(--line); border-radius: 12px;
  padding: 12px 14px 14px; }
section.view h2 { font-size: 16px; margin: 0 0 2px; display: flex; gap: 8px;
  align-items: baseline; flex-wrap: wrap; }
section.view h2 .id { color: var(--accent); }
section.view h2 .badge { font-size: 12px; font-weight: 600; border-radius: 99px;
  padding: 1px 8px; border: 1px solid var(--line); color: var(--dim); }
section.view.pass h2 .badge { color: var(--good); border-color: var(--good); }
section.view.low h2 .badge { color: var(--warn); border-color: var(--warn); }
.record { color: var(--dim); font-size: 13px; margin: 0 0 8px; min-height: 1em; }
.pair { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }
figure { margin: 0; min-width: 0; }
figcaption { font-size: 12px; color: var(--dim); margin-bottom: 4px; text-transform: uppercase;
  letter-spacing: .06em; }
figure img { width: 100%; height: auto; display: block; border-radius: 6px;
  background: #0003; }
figure .missing { aspect-ratio: 16 / 9; display: grid; place-items: center; border-radius: 6px;
  border: 1px dashed var(--line); color: var(--dim); font-size: 13px; }
.grey figure.target img { filter: grayscale(1); }
.grey figure.game img.css-grey { filter: grayscale(1); }
.controls { display: flex; flex-wrap: wrap; gap: 10px 18px; align-items: center; margin-top: 10px; }
.score { display: flex; gap: 4px; align-items: center; }
.score span { color: var(--dim); font-size: 13px; margin-right: 4px; }
.score label { position: relative; }
.score input { position: absolute; opacity: 0; }
.score label b { display: inline-grid; place-items: center; width: 34px; height: 34px;
  border-radius: 8px; border: 1px solid var(--line); cursor: pointer; font-weight: 600; }
.score input:checked + b { background: var(--accent); border-color: var(--accent); color: #fff; }
.score input:focus-visible + b { outline: 2px solid var(--accent); outline-offset: 2px; }
textarea { width: 100%; min-height: 54px; font: inherit; color: var(--ink); background: var(--bg);
  border: 1px solid var(--line); border-radius: 8px; padding: 6px 8px; resize: vertical; }
.notes { flex: 1 1 320px; }
#copied { position: fixed; bottom: 16px; left: 50%; transform: translateX(-50%);
  background: var(--ink); color: var(--bg); padding: 8px 14px; border-radius: 8px;
  opacity: 0; transition: opacity .2s; pointer-events: none; }
#copied.show { opacity: 1; }
#fallback { display: none; }
#fallback.show { display: block; }
@media (max-width: 720px) { .pair { grid-template-columns: 1fr; } main { padding: 12px; }
  header { position: static; } }
"""

SCRIPT = """
(function () {
  const board = document.body.dataset.board;
  const key = 'pieced-board:' + board;
  const views = Array.from(document.querySelectorAll('section.view'));
  let saved = {};
  try { saved = JSON.parse(localStorage.getItem(key) || '{}') || {}; } catch (e) { saved = {}; }
  function save() {
    const state = {};
    views.forEach(v => {
      const picked = v.querySelector('input[type=radio]:checked');
      state[v.dataset.id] = { score: picked ? Number(picked.value) : null,
        notes: v.querySelector('textarea').value };
    });
    try { localStorage.setItem(key, JSON.stringify(state)); } catch (e) {}
  }
  function refresh() {
    let scored = 0, sum = 0; const low = [];
    views.forEach(v => {
      const picked = v.querySelector('input[type=radio]:checked');
      const badge = v.querySelector('.badge');
      v.classList.remove('pass', 'low');
      if (picked) {
        const s = Number(picked.value); scored += 1; sum += s;
        v.classList.add(s >= PASS ? 'pass' : 'low');
        badge.textContent = s + ' / 5';
        if (s < PASS) low.push(v.dataset.id);
      } else { badge.textContent = 'not scored'; }
    });
    const mean = scored ? (sum / scored).toFixed(1) : '–';
    document.getElementById('progress').textContent =
      scored + ' / ' + views.length + ' scored · mean ' + mean +
      (low.length ? ' · below ' + PASS + ': ' + low.join(', ') : '');
  }
  views.forEach(v => {
    const s = saved[v.dataset.id];
    if (s) {
      if (s.score) { const r = v.querySelector('input[value="' + s.score + '"]'); if (r) r.checked = true; }
      if (s.notes) v.querySelector('textarea').value = s.notes;
    }
    v.querySelectorAll('input[type=radio]').forEach(r => r.addEventListener('change', () => { save(); refresh(); }));
    v.querySelector('textarea').addEventListener('input', save);
    v.querySelector('.grey-one').addEventListener('change', e => setGrey(v, e.target.checked));
  });
  function setGrey(view, on) {
    view.classList.toggle('grey', on);
    view.querySelector('.grey-one').checked = on;
    const img = view.querySelector('figure.game img');
    if (img && img.dataset.grey) img.src = on ? img.dataset.grey : img.dataset.color;
  }
  document.getElementById('grey-all').addEventListener('change', e => {
    views.forEach(v => setGrey(v, e.target.checked));
  });
  function text() {
    const lines = ['Pieced M2 target board: ' + board];
    let scored = 0, sum = 0; const low = [];
    views.forEach(v => {
      const picked = v.querySelector('input[type=radio]:checked');
      const notes = v.querySelector('textarea').value.trim().replace(/\\s*\\n\\s*/g, ' / ');
      const score = picked ? Number(picked.value) : null;
      if (score) { scored += 1; sum += score; if (score < PASS) low.push(v.dataset.id); }
      lines.push(v.dataset.id + ' ' + v.dataset.title + ': ' + (score ? score + '/5' : 'not scored') +
        (notes ? ' - ' + notes : ''));
    });
    lines.push('Scored ' + scored + '/' + views.length + (scored ? ', mean ' + (sum / scored).toFixed(1) : '') +
      (low.length ? ', below ' + PASS + ': ' + low.join(', ') : ''));
    return lines.join('\\n');
  }
  function flash(msg) {
    const el = document.getElementById('copied'); el.textContent = msg;
    el.classList.add('show'); setTimeout(() => el.classList.remove('show'), 1600);
  }
  document.getElementById('copy').addEventListener('click', () => {
    const t = text();
    const fallback = () => {
      const box = document.getElementById('fallback');
      const area = box.querySelector('textarea');
      area.value = t; box.classList.add('show'); area.focus(); area.select();
      let ok = false; try { ok = document.execCommand('copy'); } catch (e) {}
      flash(ok ? 'Copied' : 'Select the text below and copy it');
    };
    if (navigator.clipboard && window.isSecureContext) {
      navigator.clipboard.writeText(t).then(() => flash('Copied'), fallback);
    } else { fallback(); }
  });
  refresh();
})();
"""


def render(info, rows, targets_dir):
    esc = html.escape
    now = datetime.datetime.now().strftime("%Y-%m-%d %H:%M")
    meta = [f"Run <b>{esc(info['label'])}</b> ({esc(info['kind'])})"]
    if info.get("commit"):
        meta.append(f"commit {esc(str(info['commit']))}")
    shown = os.path.relpath(targets_dir, REPO) if targets_dir.startswith(REPO) else targets_dir
    if os.path.basename(os.path.dirname(os.path.dirname(targets_dir))) == "docs":
        shown = os.path.join("docs", "design", "concepts")
    meta.append(f"targets from {esc(shown)}")
    meta.append(f"made {now}")
    parts = [
        "<!doctype html>",
        '<html lang="en"><head><meta charset="utf-8">',
        '<meta name="viewport" content="width=device-width, initial-scale=1">',
        "<title>Pieced Target Board</title>",
        f"<style>{CSS}</style></head>",
        f'<body data-board="{esc(info["label"])}">',
        "<header><h1>Pieced target board</h1>",
        '<label class="toggle"><input type="checkbox" id="grey-all"> Greyscale</label>',
        '<button id="copy" class="primary">Copy scores</button>',
        '<span id="progress"></span>',
        f'<div class="meta">{" · ".join(meta)}. Score each view 1-5 against its target '
        f"(same style and composition; the gate is {PASS_SCORE}+ on all twelve).</div>",
        "</header><main>",
        '<div id="fallback"><textarea rows="14" aria-label="Scores as text"></textarea></div>',
    ]
    for row in rows:
        vid, title = row["id"], row["title"]
        name = f"score-{vid}"
        scores = "".join(
            f'<label><input type="radio" name="{name}" value="{n}" aria-label="{n} of 5">'
            f"<b>{n}</b></label>"
            for n in range(1, 6)
        )

        def figure(kind, caption, src, grey=None):
            if not src:
                return (f'<figure class="{kind}"><figcaption>{caption}</figcaption>'
                        f'<div class="missing">missing</div></figure>')
            grey_attrs = ""
            cls = ""
            if kind == "game":
                if grey:
                    grey_attrs = f' data-color="{esc(src)}" data-grey="{esc(grey)}"'
                else:
                    cls = ' class="css-grey"'
            return (f'<figure class="{kind}"><figcaption>{caption}</figcaption>'
                    f'<a href="{esc(src)}" target="_blank" rel="noopener">'
                    f'<img{cls} src="{esc(src)}"{grey_attrs} alt="{caption}: {esc(title)}" '
                    f'loading="lazy"></a></figure>')

        parts.append(
            f'<section class="view" data-id="{esc(vid)}" data-title="{esc(title)}">'
            f'<h2><span class="id">{esc(vid)}</span> {esc(title)}'
            f'<span class="badge">not scored</span></h2>'
            f'<p class="record">{esc(row["record"])}</p>'
            f'<div class="pair">{figure("target", "Target", row["target"])}'
            f'{figure("game", "Game", row["game"], row["grey"])}</div>'
            f'<div class="controls"><div class="score"><span>Score</span>{scores}</div>'
            f'<label class="toggle"><input type="checkbox" class="grey-one"> Greyscale</label>'
            f'<div class="notes"><textarea placeholder="Notes for {esc(vid)}" '
            f'aria-label="Notes for {esc(vid)}"></textarea></div></div></section>'
        )
    parts.append('</main><div id="copied" role="status"></div>')
    parts.append(f"<script>const PASS = {PASS_SCORE};{SCRIPT}</script></body></html>")
    return "\n".join(parts)


def build(gallery_dir, out_html=None, targets_dir=None, inline=False):
    """Writes the board page and returns its path."""
    if not os.path.isdir(gallery_dir):
        raise SystemExit(f"no gallery folder at {gallery_dir}")
    out_html = out_html or os.path.join(REPO, "evidence", "m2-board", "index.html")
    targets_dir = targets_dir or default_targets()
    out_dir = os.path.dirname(os.path.abspath(out_html))
    os.makedirs(out_dir, exist_ok=True)
    info = load_run(gallery_dir)
    images = Images(out_dir, inline)
    rows = []
    for vid, stem, title in info["views"]:
        target = find_target(targets_dir, vid, stem)
        game = os.path.join(gallery_dir, stem + ".png")
        grey = os.path.join(gallery_dir, stem + "-grey.png")
        rows.append({
            "id": vid,
            "title": title,
            "record": record_line(info["records"].get(vid)),
            "target": images.src(target, f"target-{stem}.png"),
            "game": images.src(game, f"game-{stem}.png"),
            "grey": images.src(grey, f"game-{stem}-grey.png"),
        })
    page = render(info, rows, targets_dir)
    with open(out_html, "w", encoding="utf-8") as f:
        f.write(page)
    missing = [r["id"] for r in rows if not r["game"]]
    no_target = [r["id"] for r in rows if not r["target"]]
    if missing:
        print(f"warning: no game shot for {', '.join(missing)}", file=sys.stderr)
    if no_target:
        print(f"warning: no target image for {', '.join(no_target)}", file=sys.stderr)
    return out_html


def main(argv):
    args = [a for a in argv if not a.startswith("--")]
    flags = [a for a in argv if a.startswith("--")]
    targets = None
    if "--targets" in argv:
        i = argv.index("--targets")
        if i + 1 >= len(argv):
            raise SystemExit("--targets needs a folder")
        targets = argv[i + 1]
        args = [a for a in args if a != targets]
    unknown = [f for f in flags if f not in ("--targets", "--inline")]
    if unknown or not 1 <= len(args) <= 2:
        print(__doc__.strip().split("\n\n")[1], file=sys.stderr)
        return 2
    out = build(args[0], args[1] if len(args) > 1 else None, targets, "--inline" in flags)
    print(out)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
