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
