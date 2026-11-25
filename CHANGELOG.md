# Changelog

Changes to the `op2-formats` crate and its C interface, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [semver](https://semver.org/): while
the version is 0.x, a breaking change raises the minor version.

## [Unreleased]

## [0.6.0] - 2026-11-20

Saved games (`.op2`), with OPU 1.4.x's save layout. No breaking changes.

### Added

- `saved_game`: saved games (`.op2`) of the stock game and of OPU 1.4.x. `SavedGame::parse` walks the file as far as
  the player blocks and gives the description and save time, the map as it was (a `Map` whose tiles carry the
  running state; `Map::write` makes a `.map` of that moment), every live unit in the game's list order (type,
  position, facing, owner, damage, cargo or weapon, animation, flags, and the raw record; a mining beacon's ore, yield
  and variant), the players (human, colony, colour), the tick and the mission DLL. OPU's saves hold 2047 unit slots
  and 0x7C bytes per logged message, where the stock game's hold 1023 and 0x4C. Checked against five saves OPU 1.4.1
  wrote, four of them against the running game's own state at the same tick: every unit's fields and every tile
  agree (`tests/saved_game.rs` with `$OP2_SAVES_DIR`). Rust only for now: the C interface doesn't cover it.

### Changed

- `Map::parse` on a saved game names `saved_game::SavedGame::parse` in its `Unsupported` error.

## [0.5.0] - 2026-11-20

OP2Lua's placement files, and maps with data after their tile groups. No breaking changes.

### Added

- `placement`: OP2Lua's `<Name>.placement.lua` mission layouts, read and written as text without running Lua.
  `Placement` holds the header (`name`, `map`, `tech`, `type`, `max_tech`, `players_count`), the units, the regions
  and the beacons, walls and markers; `to_text` writes changed units, regions and header fields back and leaves the
  rest of the file as it was (comments, formatting, unknown fields), so a file read and written unchanged keeps its
  bytes. Deleting units renumbers the beacons' `after = n`. The lower-level functions (comment and string masking,
  finding a table and its entries, reading and setting a field) are public too. Rust only for now: the C interface
  doesn't cover it. Checked against 271 placement files (`tests/placement.rs` with `$OP2_PLACEMENT_DIR`).
- `Map::trailing`: bytes after a map's tile groups, which the game doesn't read, kept and written back.

### Fixed

- A map with data after its tile groups now writes back byte for byte; `Map::write` dropped it. Seen in a community
  map (a campaign's 64 × 128 map, which carries two leftover copies of its sections after the tiles). `MapJson::to_map`
  with a base map keeps them too.

## [0.4.0] - 2026-11-11

Finding op2-rs's source tree at run time, from bufke (pull request 2), so op2-rs's tools and tests work with
the crate from crates.io. No breaking changes.

### Added

- `source_tree`: the checkout a development build runs from (the folder above the Cargo workspace whose `target`
  folder holds the running program), found at run time.
- `testdata::oracle_runs_dir` honours `$OP2_RUNS_DIR`.

### Fixed

- `source_tree_game_dir` and `testdata::oracle_runs_dir` work when the crate comes from crates.io. They located the
  checkout from the crate's own build path (`CARGO_MANIFEST_DIR`), which for a published crate is in Cargo's registry,
  so a dependent project's tests and tools silently lost the source tree's game folder and recorded runs.

## [0.3.0] - 2026-11-10

op2-rs's strict string-table check, from bufke (pull request 1). One breaking change, in the install check.

### Added

- `strings::GameStrings::load_checked` and `parse_checked`: a table that lacks any id in `0..=LAST_ID` (the ids
  op2-rs reads) is refused with `strings::Error` (`Missing`, or `Incomplete` with how many are missing and the
  first), so a caller using the table as its only source of UI text never meets a hole. `GameStrings::text` looks up
  such a fixed id. `GameStrings::load` is unchanged: it still gives an empty table on a stock install.

### Changed

- Breaking: `install::Check::exe_tables_problem` is now `data_problem`, and with OPU present it also reports a
  `language.ini` that `load_checked` refuses (or the game's files failing to open), so an install whose strings
  op2-rs can't use doesn't pass the check.

## [0.2.0] - 2026-11-10

The community's `.opm` missions, JSON maps and tile group files, and the game's mission DLLs, in the one crate.
Nothing from 0.1.0 changed: the new parts are new modules, and the two that need extra dependencies are behind
features that are off by default.

### Added

- `opm` module (feature `opm`): reads and writes OP2MissionEditor's `.opm` missions. The reader accepts every known
  writer's layout (OP2MissionEditor's, the SDK tools', the older SDK layout, comments, trailing commas, `null`s,
  members in any order, unknown members kept); the writer produces the editor's layout. `Unit::cargo` decodes a
  unit's cargo: truck cargo, starship modules, wreckage, gene banks, a ConVec's structure kit, weapons. Example:
  `opm_rewrite`.
- `mapjson` module (feature `mapjson`): converts `.map` files to and from OP2MapJsonTools' JSON maps (PerRowPadded),
  byte for byte as that tool writes them.
- `mapjson::TileGroupJson`: OP2Mapper3's and OP2TileSetTools' tile group files, read and written in their layout,
  and converted to and from a `.map`'s tile groups (`map_tile_groups`, `tileset_prefix`).
- `mission` module: reads a mission DLL's description without loading it: `DescBlock` (type, players, maximum tech
  level, unit-only flag), `LevelDesc`, `MapName`, `TechtreeName`, `DescBlockEx`'s extra AI players, every export,
  and the game's file checksum. It warns where a DLL would confuse the game's own PE reader. `mission::missions`
  lists an install's missions; `multiplayer_name` reads a lobby file name's type and player count.
- `GameFiles::loose_files`: every loose file the lookups can resolve to.
- Tests: `tests/missions.rs` checks every count in `docs/mission-dlls.md` against an OPU 1.4.1 install (159
  missions); `tests/opm.rs` and `tests/mapjson.rs` with their sample files; two `.opm` missions in `tests/data/opm`.

### Compared with the earlier `op2-opm` crate

The `opm` module began as a separate crate in this repository, never published. Its model now follows the SDK and
the game:

- `MissionType` values are `DescBlock`'s: `Colony` −1, `AutoDemo` −2, `Tutorial` −3, `MultiLandRush` −4 to
  `MultiLastOneStanding` −8 (they were in the wrong order). `MissionType::number` also reads a campaign mission's
  number.
- `MoraleLevel` is numbered from `Excellent` 0, and an absent one is `Excellent`, as the SDK reads it.
- `BotType` has the SDK's ten presets (it had three).
- No colour 7 (`White`) and no `Common` or `Rare` marker: the SDK defines none, so they read as unknown names and are
  written back as they were.
- An empty truck is `Empty` (`TruckCargo` 0), not `None`.
- A numeric `CargoType` is decoded by its low 4 bits, as the game stores truck cargo, so `90` and `10` are both a gene
  bank.
- A ConVec's cargo is a structure kit, with `CargoAmount` its weapon.
- `Region` has its `ID`.

### C interface (`op2formats-c`, not on crates.io)

- Mission DLLs: `op2f_mission_*` and `op2f_game_missions`.
- Tile group files: `op2f_tilegroup_*`, `op2f_map_add_tile_group` and `op2f_map_remove_tile_group`.
- `Op2fOpmLevel.mission_type` gives the corrected values, and `Op2fOpmCargo` gains `Kit` (added last, so the other
  values keep their numbers).
- 345 functions in all; the CMake package version is 0.2.0.

## [0.1.0] - 2026-11-09

The first release: the `op2-formats` crate of [op2-rs](https://gitlab.com/bufke/op2-rs), bufke's reimplementation
of Outpost 2, as op2-rs had it at commit `69fb43ff`, with its full history. The public API is op2-rs's, unchanged.

- Readers for `.vol` archives (and LZH), `.map` files, tilesets, `op2_art.prt` sprites, the rule sheets and tech
  trees, UI strings, `op2.clm` music and `.wav` sounds, the AVI cutscenes (Cinepak, MS ADPCM), the Blight's masks,
  Windows resources in the game's DLLs, and the tables in `Outpost2.exe` (`exe::GameExe`).
- Finding and checking an install; GOG's offline installer (feature `inno`), OPU's installer (feature `opu`) and
  downloading it (feature `fetch-opu`).
- Package metadata for crates.io (repository, description, `rust-version` 1.99).

[Unreleased]: https://github.com/TethysGame/OP2Formats/compare/v0.6.0...HEAD
[0.6.0]: https://github.com/TethysGame/OP2Formats/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/TethysGame/OP2Formats/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/TethysGame/OP2Formats/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/TethysGame/OP2Formats/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/TethysGame/OP2Formats/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/TethysGame/OP2Formats/releases/tag/v0.1.0
