//! Asset audit (docs/M2-SPEC.md → Asset pipeline, gate S9): every file under
//! `assets/` is listed in `assets/ASSETS.md` with an existing source script or
//! license file, and every model the manifest names exists.

use pieced::models::parse_manifest;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn files_under(dir: &Path, root: &Path, out: &mut BTreeSet<String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if name == ".DS_Store" {
            continue;
        }
        if path.is_dir() {
            files_under(&path, root, out);
        } else {
            let rel = path.strip_prefix(root).unwrap();
            out.insert(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// Backticked spans in a Markdown table cell.
fn code_spans(cell: &str) -> Vec<&str> {
    cell.split('`').skip(1).step_by(2).collect()
}

/// `(file, made-by cell)` for every table row whose first cell is a backticked path.
fn listed_rows(md: &str) -> Vec<(String, String)> {
    md.lines()
        .filter_map(|line| {
            let cells: Vec<&str> = line.trim().trim_matches('|').split('|').collect();
            if cells.len() < 2 {
                return None;
            }
            let file = code_spans(cells[0].trim()).first()?.to_string();
            Some((file, cells[1].trim().to_string()))
        })
        .collect()
}

#[test]
fn every_asset_file_is_listed_with_its_source() {
    let assets = repo().join("assets");
    let md = fs::read_to_string(assets.join("ASSETS.md")).expect("assets/ASSETS.md exists");
    let rows = listed_rows(&md);
    let listed: BTreeSet<String> = rows.iter().map(|(f, _)| f.clone()).collect();
    assert_eq!(listed.len(), rows.len(), "a file is listed twice");

    let mut on_disk = BTreeSet::new();
    files_under(&assets, &assets, &mut on_disk);

    let unlisted: Vec<_> = on_disk.difference(&listed).collect();
    assert!(
        unlisted.is_empty(),
        "files under assets/ missing from assets/ASSETS.md: {unlisted:?}"
    );
    let stale: Vec<_> = listed.difference(&on_disk).collect();
    assert!(
        stale.is_empty(),
        "assets/ASSETS.md lists files that don't exist: {stale:?}"
    );
    for (file, made_by) in &rows {
        let sources: Vec<&str> = code_spans(made_by)
            .into_iter()
            .filter(|p| repo().join(p).is_file())
            .collect();
        assert!(
            !sources.is_empty(),
            "assets/{file}: the second column must name an existing source script or \
             license file in backticks (got {made_by:?})"
        );
    }
}

#[test]
fn every_manifest_entry_has_its_files() {
    let models = repo().join("assets/models");
    let manifest = parse_manifest(&fs::read_to_string(models.join("manifest.json")).unwrap())
        .expect("manifest.json parses");
    assert!(!manifest.is_empty());
    for entry in &manifest {
        assert!(
            models.join(&entry.file).is_file(),
            "manifest names {} but assets/models/{} is missing",
            entry.name,
            entry.file
        );
        assert!(
            models.join(format!("{}.json", entry.name)).is_file(),
            "{} has no sidecar",
            entry.name
        );
    }
    // And nothing in assets/models is orphaned.
    let expected: BTreeSet<String> = manifest
        .iter()
        .flat_map(|e| [e.file.clone(), format!("{}.json", e.name)])
        .chain(["manifest.json".to_string()])
        .collect();
    let mut on_disk = BTreeSet::new();
    files_under(&models, &models, &mut on_disk);
    assert_eq!(
        on_disk, expected,
        "assets/models holds exactly the manifest's models"
    );
}

/// The HUD's icons and the logo (slice F): committed, listed with the Blender
/// script that renders them, embedded in the game, and real transparent PNGs.
#[test]
fn ui_icons_and_logo_are_committed_listed_and_embedded() {
    use pieced::hud::art::{UI_IMAGES, decode_png};
    let ui = repo().join("assets/ui");
    let md = fs::read_to_string(repo().join("assets/ASSETS.md")).unwrap();
    let rows = listed_rows(&md);
    let expected = [
        ("icons/rifle.png", "icons.py", 128),
        ("icons/pump.png", "icons.py", 128),
        ("icons/wall_brick.png", "icons.py", 128),
        ("icons/ramp_plank.png", "icons.py", 128),
        ("icons/floor_plank.png", "icons.py", 128),
        ("icons/cone_plank.png", "icons.py", 128),
        ("icons/crystal_blue.png", "icons.py", 64),
        ("icons/crystal_violet.png", "icons.py", 64),
        ("icons/heart.png", "icons.py", 64),
        ("logo.png", "logo.py", 0),
    ];
    for (file, script, size) in expected {
        assert!(ui.join(file).is_file(), "assets/ui/{file} is missing");
        let (_, made_by) = rows
            .iter()
            .find(|(f, _)| *f == format!("ui/{file}"))
            .unwrap_or_else(|| panic!("ui/{file} is not in assets/ASSETS.md"));
        assert!(
            made_by.contains(&format!("art/blender/assets/{script}")),
            "ui/{file} names its script: {made_by}"
        );
        let bytes = UI_IMAGES
            .iter()
            .find(|(p, _)| *p == file)
            .unwrap_or_else(|| panic!("{file} is not embedded in src/hud/art.rs"))
            .1;
        assert_eq!(bytes, fs::read(ui.join(file)).unwrap().as_slice());
        let image = decode_png(bytes).unwrap();
        let (w, h) = (image.width(), image.height());
        if size > 0 {
            assert_eq!((w, h), (size, size), "{file}");
        } else {
            assert!(w > 2 * h && w >= 1000, "the logo is a wide banner: {w}x{h}");
        }
        // A transparent background, with something drawn on it.
        let data = image.data.as_ref().unwrap();
        assert_eq!(data.len(), (w * h * 4) as usize, "{file} is RGBA8");
        assert_eq!(data[3], 0, "{file}: the corner is transparent");
        let opaque = data.chunks(4).filter(|p| p[3] == 255).count();
        assert!(opaque * 10 > (w * h) as usize, "{file} is mostly empty");
    }
    // Nothing else in assets/ui, and nothing embedded that isn't there.
    let mut on_disk = BTreeSet::new();
    files_under(&ui, &ui, &mut on_disk);
    let embedded: BTreeSet<String> = UI_IMAGES.iter().map(|(p, _)| p.to_string()).collect();
    assert_eq!(on_disk, embedded);
    assert_eq!(embedded.len(), expected.len());
}

/// The spec allows one third-party font (with its license), picked from a HUD
/// mock-up; the other candidate is gone.
#[test]
fn exactly_one_hud_font_ships() {
    let mut fonts = BTreeSet::new();
    let dir = repo().join("assets/fonts");
    files_under(&dir, &dir, &mut fonts);
    let ttf: Vec<_> = fonts.iter().filter(|f| f.ends_with(".ttf")).collect();
    assert_eq!(ttf, [pieced::hud::art::FONT_FILE]);
    assert_eq!(fonts.len(), 2, "the font and its license: {fonts:?}");
}
