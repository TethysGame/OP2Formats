//! Prints what a saved game holds: `saved_game <SGAMEn.OP2> [OUT.map]`; with OUT.map, also writes its map as it was.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (input, out) = match &args[..] {
        [_, input] => (input, None),
        [_, input, out] => (input, Some(out)),
        _ => {
            eprintln!("usage: saved_game <SGAMEn.OP2> [OUT.map]");
            std::process::exit(2);
        }
    };
    let data = std::fs::read(input).unwrap_or_else(|e| {
        eprintln!("{input}: {e}");
        std::process::exit(1);
    });
    let save = op2_formats::saved_game::SavedGame::parse(&data).unwrap_or_else(|e| {
        eprintln!("{input}: {e}");
        std::process::exit(1);
    });
    println!("{input}: {:?} save, \"{}\", saved at {}", save.kind, save.description, save.save_time);
    println!("mission {}, tick {}, map {}x{}, {} units", save.mission, save.tick, save.map.width, save.map.height, save.units.len());
    for p in &save.players {
        println!("player {}: {}, {}, colour {}", p.id, if p.human { "human" } else { "computer" }, if p.eden { "Eden" } else { "Plymouth" }, p.colour);
    }
    if let Some(out) = out {
        std::fs::write(out, save.map.write()).unwrap_or_else(|e| {
            eprintln!("{out}: {e}");
            std::process::exit(1);
        });
        println!("wrote {out}");
    }
}
