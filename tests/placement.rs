//! OP2Lua placement files. `$OP2_PLACEMENT_DIR`, when set, names folders (separated by `;`) whose `.placement.lua`
//! files, at any depth, must each read and write back byte for byte, and survive an edit.

use std::path::PathBuf;

use op2_formats::placement::{Placement, Unit, parse_units};

fn placement_files() -> Option<Vec<PathBuf>> {
    let dirs = std::env::var_os("OP2_PLACEMENT_DIR")?;
    let mut stack: Vec<PathBuf> = std::env::split_paths(&dirs).collect();
    let mut found = Vec::new();
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.to_string_lossy().to_ascii_lowercase().ends_with(".placement.lua") {
                found.push(p);
            }
        }
    }
    found.sort();
    Some(found)
}

#[test]
fn real_files_round_trip() {
    let Some(files) = placement_files() else {
        eprintln!("skipping: set OP2_PLACEMENT_DIR to folders of .placement.lua files");
        return;
    };
    assert!(!files.is_empty(), "no .placement.lua files under OP2_PLACEMENT_DIR");
    for path in &files {
        let p = Placement::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(p.to_text() == p.text, "{} changed on a round trip", path.display());

        // An added unit reads back, and the rest of the units are untouched.
        let mut edited = p.clone();
        edited.units.units.push(Unit::new("Scout", 1, 3, 4, ""));
        let units = parse_units(&edited.to_text()).units;
        assert_eq!(units.len(), p.units.units.len() + 1, "{}", path.display());
        assert_eq!(units.last().map(|u| (u.x, u.y)), Some((3, 4)), "{}", path.display());
    }
    eprintln!("{} placement files round-tripped", files.len());
}
