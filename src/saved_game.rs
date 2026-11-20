//! Saved games (`.op2`): the map as it was when the game was saved, every live unit on it, each player's colony and
//! colour, the tick and the mission's name.
//!
//! The file is one flat stream of sections with no length headers (docs/saved-games.md), so this walks them in order
//! as far as the player blocks and stops there; the research, minimap, view and random number sections after them
//! aren't read. Saves of the stock game and of OPU 1.4.x are both read: OPU's start with their own signature, hold
//! 2047 unit slots instead of 1023 and keep 0x7C bytes per logged message instead of 0x4C (measured on saves OPU
//! 1.4.1 wrote, against the running game's own state at the same tick).

use crate::map::Map;
use crate::reader::Reader;
use crate::Error;

/// The stock game's signature (`Outpost2.exe` 1.2.0.7, at 0x4D6380): 23 characters, 0x1A, 0.
const STOCK_SIGNATURE: &[u8; 25] = b"OUTPOST 2.00 SAVED GAME\x1a\0";
/// The start of OPU 1.4.x's signature; its last two bytes aren't checked (OPU 1.4.1 writes 0 and `1`).
const OPU_SIGNATURE: &[u8; 23] = b"OUTPOST2 1.4.0-OPU SAVE";
/// The map block's offset: after the signature, the preview block and the 115 unit type records.
const MAP_BLOCK: usize = 25 + 0x1086C + 115 * 0x1E0;
/// The preview block: checksum, save time, 0, then the description (80 bytes).
const PREVIEW: usize = 25;
const UNIT_RECORD: usize = 0x78;
const GAME_BLOCK: usize = 0x484;
const MISSION_MAGIC: u32 = 0xAEDE_0034;
const PLAYER_MAGIC: u32 = 0xE3D4_C5B4;
const PLAYER_END_MAGIC: u32 = 0xEDA3_4CEF;
const PLAYER_STATE: usize = 0x4C8;
/// The game keeps seven player blocks, players 0 to 6 (6 is the mission's own, Gaia's).
const PLAYER_BLOCKS: usize = 7;

/// Which game wrote the save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveKind {
    /// `Outpost2.exe` as shipped (1.2.0.7): 1023 unit slots.
    Stock,
    /// Outpost 2 with OPU 1.4.x: 2047 unit slots.
    Opu,
}

impl SaveKind {
    /// Unit slots 1 to this hold units (slot 0 heads the unit list, the one after the last ends it).
    pub fn unit_slots(self) -> usize {
        match self {
            SaveKind::Stock => 1023,
            SaveKind::Opu => 2047,
        }
    }

    /// Bytes per message in the message log.
    fn message_size(self) -> usize {
        match self {
            SaveKind::Stock => 0x4C,
            SaveKind::Opu => 0x7C,
        }
    }
}

/// A saved game, read as far as its player blocks.
#[derive(Debug, Clone)]
pub struct SavedGame {
    pub kind: SaveKind,
    /// The description typed in the save dialog; empty for a save made without the dialog.
    pub description: String,
    /// When it was saved, seconds since 1970 (0 for a save made without the dialog).
    pub save_time: u32,
    /// The map as it was: its tiles carry the game's running state (bits 16 and up: unit index, lava, the Blight,
    /// walls and buildings). Read as a `.map`, so `Map::write` gives a map file of that moment, with no tile groups.
    pub map: Map,
    /// The live units, in the game's unit list order (by y).
    pub units: Vec<SavedUnit>,
    /// The game tick it was saved at.
    pub tick: u32,
    /// The players in the game (the game block's player count of the seven blocks), player 0 first.
    pub players: Vec<SavedPlayer>,
    /// The mission DLL's file name (`e05.dll`).
    pub mission: String,
    /// Daylight everywhere (game option 0x13): the game draws the whole map lit.
    pub daylight_everywhere: bool,
}

/// One live unit: a map object of any kind (vehicle, structure, beacon, wreck, disaster, a weapon's fire...), as the
/// first 0x78 bytes of the game's object hold it.
#[derive(Debug, Clone)]
pub struct SavedUnit {
    /// Its slot, the game's unit index.
    pub slot: u32,
    /// Its type (map id).
    pub type_id: u32,
    /// Its position in pixels, in the game's coordinates (`Map::x_padding` columns more than the file's on a map that
    /// doesn't wrap).
    pub pixel_x: i32,
    pub pixel_y: i32,
    /// Facing, 0-255 (32 per eighth of a turn).
    pub rotation: u8,
    /// The controlling player, and the player whose statistics it uses.
    pub owner: u8,
    pub creator: u8,
    /// Damage taken; its hit points are its type's less this.
    pub damage: i16,
    /// A tank's or guard post's weapon (map id), a ConVec's kit (map id), a cargo truck's load (type in the low 4
    /// bits, amount above), a wreck's tech number less 8000 (above the low 4 bits).
    pub cargo: u16,
    /// The animation and frame it shows.
    pub animation: u16,
    pub frame: i16,
    /// The object's flags (`MapObject` +0x44).
    pub flags: u32,
    /// The record as saved, pointers turned into slot numbers (`type_id` stands in the class pointer's place).
    pub record: [u8; UNIT_RECORD],
}

impl SavedUnit {
    /// A mining beacon's ore (0 common, 1 rare), bar yield (0 three bars, 1 two, 2 one) and variant. Meaningful for a
    /// mining beacon only; for anything else these bytes are other fields.
    pub fn mining_beacon(&self) -> (u8, i32, i32) {
        let i32_at = |o: usize| i32::from_le_bytes(self.record[o..o + 4].try_into().expect("4 bytes"));
        (self.record[0x64], i32_at(0x5C), i32_at(0x60))
    }

    /// A cargo truck's load: its cargo type and amount.
    pub fn truck_cargo(&self) -> (u16, u16) {
        (self.cargo & 0xF, self.cargo >> 4)
    }
}

/// One player in the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SavedPlayer {
    pub id: u32,
    pub human: bool,
    pub eden: bool,
    /// Its colour: 0 blue, 1 red, 2 green, 3 yellow, 4 cyan, 5 magenta.
    pub colour: u32,
}

impl SavedGame {
    /// Whether `data` starts like a saved game (either signature).
    pub fn is_saved_game(data: &[u8]) -> bool {
        Self::kind_of(data).is_some()
    }

    fn kind_of(data: &[u8]) -> Option<SaveKind> {
        if data.starts_with(STOCK_SIGNATURE) {
            Some(SaveKind::Stock)
        } else if data.starts_with(OPU_SIGNATURE) {
            Some(SaveKind::Opu)
        } else {
            None
        }
    }

    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let kind = Self::kind_of(data).ok_or_else(|| Error::Invalid("not a saved game (no saved-game signature)".into()))?;
        let mut r = Reader::at(data, PREVIEW);
        r.skip(4)?;
        let save_time = r.u32()?;
        r.skip(4)?;
        let text = r.bytes(80)?;
        let description = String::from_utf8_lossy(&text[..text.iter().position(|&b| b == 0).unwrap_or(text.len())]).into_owned();

        let mut r = Reader::at(data, MAP_BLOCK);
        let map = Map::parse_head(&mut r, true)?;
        let units = Self::read_units(&mut r, kind, map.version)?;

        // The message log: the last message's time, the oldest's ring position, the count, the messages.
        r.skip(8)?;
        let messages = r.u32()?;
        let messages = r.count(messages, kind.message_size())?;
        r.skip(messages * kind.message_size())?;

        let game = r.bytes(GAME_BLOCK)?;
        let word = |o: usize| u32::from_le_bytes(game[o..o + 4].try_into().expect("4 bytes"));
        let (player_count, tick, daylight_everywhere) = (word(0x68) as usize, word(0x84), word(0x4C) != 0);
        let name = &game[0xB4..];
        let mission = String::from_utf8_lossy(&name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())]).into_owned();
        if player_count > PLAYER_BLOCKS {
            return Err(Error::Invalid(format!("the game block counts {player_count} players")));
        }

        // The mission block's script objects are each class's own data, not walked here: the block ends at its second
        // magic, the one the first player block's magic follows.
        let start = r.pos();
        if r.u32()? != MISSION_MAGIC {
            return Err(Error::Invalid(format!("no mission block at {start:#x}")));
        }
        let mut end = [0u8; 8];
        end[..4].copy_from_slice(&MISSION_MAGIC.to_le_bytes());
        end[4..].copy_from_slice(&PLAYER_MAGIC.to_le_bytes());
        let close = data[r.pos()..]
            .windows(8)
            .position(|w| w == end)
            .ok_or_else(|| Error::Invalid("the mission block has no end".into()))?;
        let mut r = Reader::at(data, r.pos() + close + 4);

        let mut players = Vec::with_capacity(player_count);
        for n in 0..PLAYER_BLOCKS {
            if r.u32()? != PLAYER_MAGIC {
                return Err(Error::Invalid(format!("player block {n}: no magic at {:#x}", r.pos() - 4)));
            }
            let state = r.bytes(PLAYER_STATE)?;
            let word = |o: usize| u32::from_le_bytes(state[o..o + 4].try_into().expect("4 bytes"));
            if n < player_count {
                players.push(SavedPlayer { id: word(0), human: word(0x28) != 0, eden: word(0x2C) != 0, colour: word(0x30) });
            }
            // 16 queued commands: a 14-byte header with the data's length at offset 4, then the data.
            for _ in 0..16 {
                let header = r.bytes(14)?;
                let len = i16::from_le_bytes([header[4], header[5]]);
                r.skip(usize::try_from(len).map_err(|_| Error::Invalid(format!("player block {n}: command length {len}")))?)?;
            }
            r.skip(12)?;
            if r.u32()? != PLAYER_END_MAGIC {
                return Err(Error::Invalid(format!("player block {n}: no end magic at {:#x}", r.pos() - 4)));
            }
        }

        Ok(SavedGame { kind, description, save_time, map, units, tick, players, mission, daylight_everywhere })
    }

    /// The unit section after the map block's first version tag, to the tag after it; the live units in list order.
    fn read_units(r: &mut Reader<'_>, kind: SaveKind, version: u32) -> Result<Vec<SavedUnit>, Error> {
        let live = r.u32()?;
        let _highest = r.u32()?;
        let ring_write = r.u32()?;
        let ring_read = r.u32()?;
        let record_size = r.u32()?;
        if record_size as usize != UNIT_RECORD {
            return Err(Error::Invalid(format!("unit record size {record_size:#x}, not 0x78")));
        }
        let blocks = r.u32()?;
        let free_blocks = r.u32()?;
        let blocks = r.count(blocks, 512)?;
        r.skip(blocks * 512)?;
        let free_blocks = r.count(free_blocks, 4)?;
        r.skip(free_blocks * 4)?;
        let first = r.i32()?;
        let _last = r.i32()?;
        let slots = kind.unit_slots();
        let records = r.bytes(slots * UNIT_RECORD)?;
        // The free-slot ring, one entry per slot and one more, is there only when its positions differ.
        if ring_write != ring_read {
            r.skip((slots + 1) * 4)?;
        }
        let tag = r.u32()?;
        if tag != version {
            return Err(Error::Invalid(format!("version tag {tag:#x} after the units, not {version:#x}")));
        }

        // Follow the list from the first unit; it ends at the slot after the last one.
        let mut units = Vec::new();
        let mut slot = first;
        while slot >= 1 && slot as usize <= slots {
            if units.len() == slots {
                return Err(Error::Invalid("the unit list loops".into()));
            }
            let at = (slot as usize - 1) * UNIT_RECORD;
            let record: [u8; UNIT_RECORD] = records[at..at + UNIT_RECORD].try_into().expect("a whole record");
            let u32_at = |o: usize| u32::from_le_bytes(record[o..o + 4].try_into().expect("4 bytes"));
            units.push(SavedUnit {
                slot: slot as u32,
                type_id: u32_at(0),
                pixel_x: u32_at(0x14) as i32,
                pixel_y: u32_at(0x18) as i32,
                rotation: record[0x1C],
                owner: record[0x1D] & 0xF,
                creator: record[0x1D] >> 4,
                damage: i16::from_le_bytes([record[0x1E], record[0x1F]]),
                cargo: u16::from_le_bytes([record[0x24], record[0x25]]),
                animation: u16::from_le_bytes([record[0x40], record[0x41]]),
                frame: i16::from_le_bytes([record[0x42], record[0x43]]),
                flags: u32_at(0x44),
                record,
            });
            slot = u32_at(4) as i32;
        }
        if units.len() != live as usize {
            return Err(Error::Invalid(format!("{} units in the unit list, the header counts {live}", units.len())));
        }
        Ok(units)
    }
}
