//! Saved games (`.op2`). `$OP2_SAVES_DIR`, when set, names folders (separated by `;`) whose saved games, at any depth,
//! must each read whole: every live unit the header counts, players, and a map that writes and reads back. Without it
//! only the checks that need no saved game run.

use std::path::PathBuf;

use op2_formats::map::Map;
use op2_formats::saved_game::SavedGame;

fn saved_games() -> Option<Vec<PathBuf>> {
    let dirs = std::env::var_os("OP2_SAVES_DIR")?;
    let mut stack: Vec<PathBuf> = std::env::split_paths(&dirs).collect();
    let mut found = Vec::new();
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("op2")) {
                found.push(p);
            }
        }
    }
    found.sort();
    Some(found)
}

#[test]
fn real_saves_read_whole() {
    let Some(files) = saved_games() else {
        eprintln!("skipping: set OP2_SAVES_DIR to folders of saved games (.op2)");
        return;
    };
    let mut read = 0;
    for path in &files {
        let data = std::fs::read(path).unwrap();
        // SGAME10.OP2 is the save dialog's slot index, not a game.
        if !SavedGame::is_saved_game(&data) {
            continue;
        }
        let save = SavedGame::parse(&data).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(!save.units.is_empty(), "{}: no units", path.display());
        assert!(!save.players.is_empty(), "{}: no players", path.display());
        assert!(save.mission.to_ascii_lowercase().ends_with(".dll"), "{}: mission {:?}", path.display(), save.mission);
        // The live list is sorted by y and holds each slot once.
        assert!(save.units.windows(2).all(|w| w[0].pixel_y <= w[1].pixel_y), "{}: unit list out of order", path.display());
        let mut slots: Vec<u32> = save.units.iter().map(|u| u.slot).collect();
        slots.sort_unstable();
        slots.dedup();
        assert_eq!(slots.len(), save.units.len(), "{}: a slot twice", path.display());
        // Its map is a `.map` of that moment: it writes and reads back with the same tiles.
        let map = Map::parse(&save.map.write()).unwrap_or_else(|e| panic!("{}: its map doesn't read back: {e}", path.display()));
        assert_eq!((map.width, map.height), (save.map.width, save.map.height));
        for y in 0..map.height {
            for x in 0..map.width {
                assert_eq!(map.tile_bits(x, y), save.map.tile_bits(x, y), "{}: tile {x},{y}", path.display());
            }
        }
        read += 1;
    }
    assert!(read > 0, "no saved games under OP2_SAVES_DIR");
}

#[test]
fn signatures() {
    let mut stock = b"OUTPOST 2.00 SAVED GAME\x1a\0".to_vec();
    let mut opu = b"OUTPOST2 1.4.0-OPU SAVE\x001".to_vec();
    assert!(SavedGame::is_saved_game(&stock) && SavedGame::is_saved_game(&opu));
    assert!(!SavedGame::is_saved_game(b"OUTPOST 2.00 SAVED GAMX"));
    // A signature with nothing after it is cut short, not taken for a game.
    stock.resize(64, 0);
    opu.resize(64, 0);
    assert!(SavedGame::parse(&stock).is_err() && SavedGame::parse(&opu).is_err());
    // Something else isn't a saved game, and a `.map` reader sends a saved game to this one.
    assert!(SavedGame::parse(b"not a saved game at all, not at all").is_err());
    let err = Map::parse(&opu).unwrap_err().to_string();
    assert!(err.contains("saved game"), "{err}");
}
