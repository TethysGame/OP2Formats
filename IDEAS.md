# Ideas

Possible additions, not planned yet. What is planned is in [NOTES.md](NOTES.md).

- Tile groups in JSON maps. The map JSON has no tile groups yet (OP2MapJsonTools' "TO ADD"); a `tileGroups` member
  would be kept by the `mapjson` module as an unknown member but not turned into the `.map`'s tile groups. A layout
  could reuse tile group JSON's (`mapjson::TileGroupJson`): one object per group with its header and rows.
- More of `.opm`: merging a variant and a difficulty onto the master variant, `.opm` to map and in-game coordinates,
  unit names to unit art, and checking a mission against its map and tech tree.
- Formats documented but not read: the story texts (`docs/story.md`), the
  settings files (`docs/settings.md`) and OP2Mapper2's files. Of OP2Lua's missions only the placement file is read
  (`placement`); the mission script and its stub DLL's specifics (`docs-community/lua-missions.md`) aren't. Of a
  saved game, the sections after the player blocks (research, minimap, views, random number state) and the
  mission block's script objects aren't decoded (`docs/saved-games.md`).
- OP2Lua placements in the C interface, and a writer for the parts `placement` reads but doesn't write (players,
  beacons, walls, markers).
