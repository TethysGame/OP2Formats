//! OP2Lua's `<Name>.placement.lua`: a mission's starting layout, a Lua chunk that returns one table (players, units,
//! beacons, walls, regions, markers; see `docs-community/lua-missions.md`).
//!
//! The file is read and written as **text**, never run: an editor changes a file a person (or a converter) wrote, so
//! only the parts it changes are rewritten and everything else stays as it was, comments, formatting and fields this
//! module doesn't know included. Reading a file and writing it back unchanged gives the same bytes.
//!
//! Comments and strings are masked out ([`mask`]) before any key or brace is matched, so a brace in a comment or a
//! name doesn't throw the table bounds off. Every index is a byte index into the text, and the mask has the same
//! length, so an index into one is an index into the other.
//!
//! What is read is what a placement writes as plain data: `at = { x, y }`, `type = "Lynx"`, `player = 2`. A value the
//! file computes (a variable, an expression, a loop) isn't seen; such an entry is kept as it is.
//!
//! ```
//! use op2_formats::placement::{Placement, Unit};
//!
//! let text = "return {\n  map = \"eden01.map\",\n  units = {\n    { type = \"Lynx\", player = 1, at = { 30, 54 }, weapon = \"Laser\" },\n  },\n}\n";
//! let mut p = Placement::parse(text);
//! assert_eq!(p.header.map.as_deref(), Some("eden01.map"));
//! assert_eq!(p.to_text(), text); // unchanged: the same bytes
//! p.units.units.push(Unit::new("ConVec", 1, 31, 54, ""));
//! assert!(p.to_text().contains("type = \"ConVec\", player = 1, at = { 31, 54 }"));
//! ```

use std::path::Path;

use crate::Error;

// ===== masking =====

/// The text with every comment byte blanked to a space and every byte inside a string to `_` (quotes and line
/// breaks stay), the same length as the text. Other non-ASCII bytes become `_` too, so the mask is ASCII.
pub fn mask(text: &str) -> String {
    let t = text.as_bytes();
    let n = t.len();
    let mut m: Vec<u8> = t.iter().map(|&b| if b.is_ascii() { b } else { b'_' }).collect();
    let mut i = 0;
    while i < n {
        let c = t[i];
        if c == b'-' && i + 1 < n && t[i + 1] == b'-' {
            if let Some(level) = long_bracket_level(t, i + 2) {
                // --[[ ... ]] (or --[==[ ... ]==])
                let close = close_bracket(level);
                let stop = find_bytes(t, &close, i + 2 + level + 2).map_or(n, |e| e + close.len());
                blank(&mut m, i, stop, b' ');
                i = stop;
            } else {
                let mut e = i;
                while e < n && t[e] != b'\n' && t[e] != b'\r' {
                    e += 1;
                }
                blank(&mut m, i, e, b' ');
                i = e;
            }
        } else if c == b'"' || c == b'\'' {
            let mut e = i + 1;
            while e < n && t[e] != c && t[e] != b'\n' {
                if t[e] == b'\\' {
                    e += 1;
                }
                e += 1;
            }
            blank(&mut m, i + 1, e.min(n), b'_');
            i = e + 1;
        } else if let Some(level) = long_bracket_level(t, i) {
            let close = close_bracket(level);
            match find_bytes(t, &close, i + level + 2) {
                Some(e) => {
                    blank(&mut m, i + level + 2, e, b'_');
                    i = e + close.len();
                }
                None => {
                    blank(&mut m, i + level + 2, n, b'_');
                    i = n;
                }
            }
        } else {
            i += 1;
        }
    }
    String::from_utf8(m).expect("the mask is ASCII")
}

/// The level (count of `=`) of a Lua long bracket `[`, `=`*, `[` starting at `i`.
fn long_bracket_level(t: &[u8], i: usize) -> Option<usize> {
    if t.get(i) != Some(&b'[') {
        return None;
    }
    let mut j = i + 1;
    while t.get(j) == Some(&b'=') {
        j += 1;
    }
    (t.get(j) == Some(&b'[')).then_some(j - i - 1)
}

fn close_bracket(level: usize) -> Vec<u8> {
    let mut v = vec![b']'];
    v.extend(std::iter::repeat_n(b'=', level));
    v.push(b']');
    v
}

fn find_bytes(t: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from > t.len() {
        return None;
    }
    t[from..].windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

/// Overwrites `m[from..to]` with `fill`, leaving line breaks so line numbers still match.
fn blank(m: &mut [u8], from: usize, to: usize, fill: u8) {
    for b in m.iter_mut().take(to).skip(from) {
        if *b != b'\n' && *b != b'\r' {
            *b = fill;
        }
    }
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn skip_space(m: &[u8], mut i: usize) -> usize {
    while i < m.len() && m[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

/// Each `key = ` in masked text from `from` on, as (where the key starts, where its value starts). The key must start
/// a word, and `==` isn't an assignment.
fn assignments<'a>(masked: &'a str, key: &'a str, from: usize) -> impl Iterator<Item = (usize, usize)> + 'a {
    let m = masked.as_bytes();
    let k = key.as_bytes();
    let mut at = from;
    std::iter::from_fn(move || {
        while let Some(p) = find_bytes(m, k, at) {
            at = p + 1;
            if p > 0 && is_ident(m[p - 1]) {
                continue;
            }
            let eq = skip_space(m, p + k.len());
            if m.get(eq) != Some(&b'=') || m.get(eq + 1) == Some(&b'=') {
                continue;
            }
            return Some((p, skip_space(m, eq + 1)));
        }
        None
    })
}

/// The index of the `}` matching the `{` at `open` in masked text.
pub fn match_brace(masked: &str, open: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (i, &c) in masked.as_bytes().iter().enumerate().skip(open) {
        match c {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Where a `key = { ... }` table is in the text: where its key starts, its `{` and its `}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableSpan {
    /// Where the key starts (`units` of `units = {`): a replacement of the table is written from here.
    pub key_start: usize,
    /// The table's `{` and its matching `}`.
    pub open: usize,
    pub close: usize,
}

/// The first `key = { ... }` table outside comments and strings.
pub fn find_table(masked: &str, key: &str) -> Option<TableSpan> {
    let m = masked.as_bytes();
    let (key_start, open) = assignments(masked, key, 0).find(|&(_, v)| m.get(v) == Some(&b'{'))?;
    let close = match_brace(masked, open)?;
    Some(TableSpan { key_start, open, close })
}

/// The text's line ending: LF when it has line breaks and none is CRLF, else CRLF.
pub fn new_line_of(text: &str) -> &'static str {
    if text.contains("\r\n") || !text.contains('\n') { "\r\n" } else { "\n" }
}

/// Replaces the `key = { ... }` table with `block` (written from its key), or adds `block` before the file's last `}`
/// when there is none.
pub fn replace_table(text: &str, key: &str, block: &str) -> String {
    let masked = mask(text);
    if let Some(span) = find_table(&masked, key) {
        return format!("{}{}{}", &text[..span.key_start], block, &text[span.close + 1..]);
    }
    let nl = new_line_of(text);
    match masked.rfind('}') {
        None => format!("{text}{nl}{block}{nl}"),
        Some(last) => format!("{}  {block},{nl}{}", &text[..last], &text[last..]),
    }
}

/// One entry of a table: its `{` and `}`, the text before it since the previous entry (separators, comments,
/// indentation), and its key when it is written `name = { ... }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableEntry {
    /// The entry's `{` and its matching `}`.
    pub open: usize,
    pub close: usize,
    /// The text between the previous entry's `}` (or the table's `{`) and this entry's `{`.
    pub lead: String,
    /// `name` when the entry is written `name = { ... }` (regions, markers); None for a list entry.
    pub key: Option<String>,
}

/// The `name` of a lead ending `name =` (masked text).
fn trailing_key(masked_lead: &str) -> Option<String> {
    let m = masked_lead.trim_end().as_bytes();
    let rest = m.strip_suffix(b"=")?;
    if rest.ends_with(b"=") {
        return None;
    }
    let rest = rest.trim_ascii_end();
    let start = rest.iter().rposition(|&b| !is_ident(b)).map_or(0, |p| p + 1);
    let name = &rest[start..];
    (!name.is_empty() && !name[0].is_ascii_digit()).then(|| String::from_utf8_lossy(name).into_owned())
}

/// The entries at the top level of the table whose braces are `open..close`, and the text after the last one up to
/// the closing brace.
pub fn entries(text: &str, masked: &str, open: usize, close: usize) -> (Vec<TableEntry>, String) {
    let mb = masked.as_bytes();
    let mut result = Vec::new();
    let mut prev_end = open;
    let mut i = open + 1;
    while i < close {
        if mb[i] == b'{' {
            let Some(e) = match_brace(masked, i).filter(|&e| e <= close) else { break };
            let lead = text[prev_end + 1..i].to_string();
            let key = trailing_key(&masked[prev_end + 1..i]);
            result.push(TableEntry { open: i, close: e, lead, key });
            prev_end = e;
            i = e + 1;
        } else {
            i += 1;
        }
    }
    (result, text[prev_end + 1..close].to_string())
}

// ===== fields inside one entry =====

/// Where a field's value is in an entry's text: key start, value start, value end (exclusive). The field must sit
/// at the entry's top level, not inside a nested table such as `at = { x = .. }`.
fn find_field(raw: &str, masked: &str, key: &str) -> Option<(usize, usize, usize)> {
    let mb = masked.as_bytes();
    for (ks, vs) in assignments(masked, key, 0) {
        if depth(masked, ks) != 0 {
            continue;
        }
        if vs >= raw.len() {
            return None;
        }
        let c = mb[vs];
        let ve = if c == b'"' || c == b'\'' {
            masked[vs + 1..].find(c as char)? + vs + 2
        } else if c == b'{' {
            match_brace(masked, vs)? + 1
        } else {
            let mut e = vs;
            while e < mb.len() && !matches!(mb[e], b',' | b'}' | b';') && !mb[e].is_ascii_whitespace() {
                e += 1;
            }
            e
        };
        return Some((ks, vs, ve));
    }
    None
}

fn depth(masked: &str, i: usize) -> i32 {
    masked.as_bytes()[..i].iter().fold(0, |d, &c| match c {
        b'{' => d + 1,
        b'}' => d - 1,
        _ => d,
    })
}

/// A string field's value (None when absent or not a string). Escapes are kept as written.
pub fn string_field(raw: &str, key: &str) -> Option<String> {
    let (_, vs, ve) = find_field(raw, &mask(raw), key)?;
    let b = raw.as_bytes()[vs];
    (b == b'"' || b == b'\'').then(|| raw[vs + 1..ve - 1].to_string())
}

/// An integer field's value (None when absent or not a whole number).
pub fn int_field(raw: &str, key: &str) -> Option<i32> {
    let (_, vs, ve) = find_field(raw, &mask(raw), key)?;
    raw[vs..ve].parse().ok()
}

/// `{ a, b }` with whole numbers, or `{ a, b, c, d }`.
fn int_list(value: &str) -> Option<Vec<i32>> {
    let inner = value.trim().strip_prefix('{')?.strip_suffix('}')?;
    let items: Vec<&str> = inner.split(',').map(str::trim).collect();
    let items = if items.last() == Some(&"") { &items[..items.len() - 1] } else { &items[..] };
    items.iter().map(|s| s.parse().ok()).collect()
}

/// An `at = { x, y }` location; None for any other form (a raw location `{ x =, y =, raw = true }`, or none).
pub fn at_field(raw: &str) -> Option<(i32, i32)> {
    let (_, vs, ve) = find_field(raw, &mask(raw), "at")?;
    match int_list(&raw[vs..ve])?.as_slice() {
        [x, y] => Some((*x, *y)),
        _ => None,
    }
}

/// Sets a field to a Lua value (written as given), adding it at the end of the entry when absent.
pub fn set_field(raw: &str, key: &str, value: &str) -> String {
    if let Some((_, vs, ve)) = find_field(raw, &mask(raw), key) {
        return format!("{}{}{}", &raw[..vs], value, &raw[ve..]);
    }
    let body = raw.trim_end();
    let trailing = &raw[body.len()..];
    let sep = if body.is_empty() || body.ends_with(',') || body.ends_with(';') { " " } else { ", " };
    format!("{body}{sep}{key} = {value}{}", if trailing.is_empty() { " " } else { trailing })
}

/// Removes a field and the comma that went with it.
pub fn remove_field(raw: &str, key: &str) -> String {
    let masked = mask(raw);
    let Some((ks, _, ve)) = find_field(raw, &masked, key) else { return raw.to_string() };
    let mb = masked.as_bytes();
    // Prefer the comma before the field; else take the one after it.
    let mut b = ks;
    while b > 0 && mb[b - 1].is_ascii_whitespace() {
        b -= 1;
    }
    if b > 0 && mb[b - 1] == b',' {
        return format!("{}{}", &raw[..b - 1], &raw[ve..]);
    }
    let mut a = ve;
    while a < mb.len() && mb[a].is_ascii_whitespace() {
        a += 1;
    }
    if a < mb.len() && mb[a] == b',' {
        a += 1;
        while a < mb.len() && mb[a] == b' ' {
            a += 1;
        }
    } else {
        a = ve;
    }
    format!("{}{}", &raw[..ks], &raw[a..])
}

/// `s` as a Lua string literal.
pub fn lua_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

// ===== the header =====

/// The first `key = "..."` at the top level of the returned table (not inside a unit or other entry): the
/// placement's `name`, `map`, `tech`, `type`.
pub fn scalar_string(text: &str, key: &str) -> Option<String> {
    let masked = mask(text);
    let (_, vs, ve) = find_field_anywhere(text, &masked, key, b'"')?;
    Some(text[vs + 1..ve - 1].to_string())
}

/// The first `key = n` at the top level of the returned table: `max_tech`, `players_count`.
pub fn scalar_int(text: &str, key: &str) -> Option<i32> {
    let masked = mask(text);
    let m = masked.as_bytes();
    assignments(&masked, key, 0).filter(|&(ks, _)| depth(&masked, ks) == 1).find_map(|(_, vs)| {
        let mut e = vs + usize::from(m.get(vs) == Some(&b'-'));
        while e < m.len() && m[e].is_ascii_digit() {
            e += 1;
        }
        text[vs..e].parse().ok()
    })
}

/// The first `key = "..."` (a double-quoted value) at the top level of the returned table: key start, value start,
/// value end.
fn find_field_anywhere(text: &str, masked: &str, key: &str, quote: u8) -> Option<(usize, usize, usize)> {
    let m = masked.as_bytes();
    assignments(masked, key, 0).filter(|&(ks, _)| depth(masked, ks) == 1).find_map(|(ks, vs)| {
        if m.get(vs) != Some(&quote) {
            return None;
        }
        let end = masked[vs + 1..].find(quote as char)? + vs + 2;
        (end <= text.len()).then_some((ks, vs, end))
    })
}

/// The text with the first top-level `key = "..."` given `value` (unchanged when there is none).
pub fn replace_scalar_string(text: &str, key: &str, value: &str) -> String {
    match find_field_anywhere(text, &mask(text), key, b'"') {
        Some((_, vs, ve)) => format!("{}\"{}\"{}", &text[..vs], value, &text[ve..]),
        None => text.to_string(),
    }
}

/// The text with the first top-level `key = n` given `value` (unchanged when there is none).
pub fn replace_scalar_int(text: &str, key: &str, value: i32) -> String {
    let masked = mask(text);
    let m = masked.as_bytes();
    for (_, vs) in assignments(&masked, key, 0).filter(|&(ks, _)| depth(&masked, ks) == 1) {
        let mut e = vs + usize::from(m.get(vs) == Some(&b'-'));
        let digits = e;
        while e < m.len() && m[e].is_ascii_digit() {
            e += 1;
        }
        if e > digits {
            return format!("{}{}{}", &text[..vs], value, &text[e..]);
        }
    }
    text.to_string()
}

/// The fields at the top of a placement that describe the mission. OP2Lua's runtime reads only `map` (for a warning:
/// the game loads the DLL's map); the others are for the tools, and the mission DLL holds the values the game uses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Header {
    /// The mission's name, as the tools show it (the game lists the DLL's `LevelDesc`).
    pub name: Option<String>,
    /// The map file, e.g. `eden01.map` (the game loads the DLL's `MapName`).
    pub map: Option<String>,
    /// The tech tree file, e.g. `MULTITEK.TXT`.
    pub tech: Option<String>,
    /// `Colony`, `Tutorial`, `AutoDemo`, a multiplayer type (`MultiLastOneStanding`, ...) or a campaign number.
    pub mission_type: Option<String>,
    /// The highest tech level (`max_tech`).
    pub max_tech: Option<i32>,
    /// The player count (`players_count`).
    pub players: Option<i32>,
}

impl Header {
    /// The header fields the text has at the top level of its returned table; None for the ones it doesn't.
    pub fn read(text: &str) -> Self {
        Self {
            name: scalar_string(text, "name"),
            map: scalar_string(text, "map"),
            tech: scalar_string(text, "tech"),
            mission_type: scalar_string(text, "type"),
            max_tech: scalar_int(text, "max_tech"),
            players: scalar_int(text, "players_count"),
        }
    }

    /// `text` with the fields that are set written over the ones it has. A field the text doesn't have isn't added.
    pub fn write_into(&self, text: &str) -> String {
        let mut t = text.to_string();
        for (key, value) in [
            ("name", &self.name),
            ("map", &self.map),
            ("tech", &self.tech),
            ("type", &self.mission_type),
        ] {
            if let Some(v) = value {
                t = replace_scalar_string(&t, key, v);
            }
        }
        for (key, value) in [("max_tech", self.max_tech), ("players_count", self.players)] {
            if let Some(v) = value {
                t = replace_scalar_int(&t, key, v);
            }
        }
        t
    }
}

// ===== units =====

/// A unit of the units table. A loaded one keeps its entry's text (`raw`, `lead`), so writing it back keeps every
/// field this module doesn't edit (direction, health, lights, kit, cargo, ...) and the comments around it; only the
/// fields that changed (at, type, player, weapon) are rewritten.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Unit {
    /// A unit type name: `Lynx`, `CommandCenter`, ...
    pub unit_type: String,
    /// The owner, 1-based.
    pub player: i32,
    /// Status-bar coordinates (1-based).
    pub x: i32,
    pub y: i32,
    /// False: the entry has no plain `{ x, y }` (e.g. a raw location); kept as written.
    pub has_at: bool,
    /// The entry's `name = "..."`, which publishes the unit as `units[name]`.
    pub name: Option<String>,
    /// Empty for none.
    pub weapon: String,
    /// The text between the entry's braces; None for a unit made by [`Unit::new`].
    pub raw: Option<String>,
    /// The text before the entry since the previous one.
    pub lead: Option<String>,
    /// Its index in the file's table when read.
    pub orig_index: Option<usize>,
    orig: Option<(String, i32, i32, i32, String)>,
}

impl Unit {
    /// A new unit, written out in full: `{ type = "...", player = n, at = { x, y }, weapon = "..." }`.
    pub fn new(unit_type: &str, player: i32, x: i32, y: i32, weapon: &str) -> Self {
        Self {
            unit_type: unit_type.into(),
            player,
            x,
            y,
            has_at: true,
            weapon: weapon.into(),
            ..Default::default()
        }
    }

    /// The text between the entry's braces.
    fn entry_text(&self) -> String {
        let (Some(raw), Some((ot, op, ox, oy, ow))) = (&self.raw, &self.orig) else {
            let mut s =
                format!(" type = {}, player = {}, at = {{ {}, {} }}", lua_string(&self.unit_type), self.player, self.x, self.y);
            if !self.weapon.is_empty() {
                s += &format!(", weapon = {}", lua_string(&self.weapon));
            }
            return s + " ";
        };
        let mut raw = raw.clone();
        if &self.unit_type != ot {
            raw = set_field(&raw, "type", &lua_string(&self.unit_type));
        }
        if self.player != *op {
            raw = set_field(&raw, "player", &self.player.to_string());
        }
        if self.has_at && (self.x != *ox || self.y != *oy) {
            raw = set_field(&raw, "at", &format!("{{ {}, {} }}", self.x, self.y));
        }
        if &self.weapon != ow {
            raw = if self.weapon.is_empty() {
                remove_field(&raw, "weapon")
            } else {
                set_field(&raw, "weapon", &lua_string(&self.weapon))
            };
        }
        raw
    }
}

/// The units table: its entries, the text from its key to its `{` (`header`), and the text after the last entry
/// (`tail`; None when the file has no units table).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitsTable {
    /// The units in the file's order; new ones are added at the end.
    pub units: Vec<Unit>,
    /// The text from the key to the table's `{`, e.g. `units = {`.
    pub header: String,
    /// The text after the last entry up to the table's `}`; None when the file has no units table.
    pub tail: Option<String>,
    /// The file's line ending, for the lines a new entry adds.
    pub new_line: &'static str,
    /// Whether the file has a units table.
    pub found: bool,
    /// Entries in the file's table when it was read.
    pub orig_count: usize,
}

impl Default for UnitsTable {
    fn default() -> Self {
        Self { units: Vec::new(), header: "units = {".into(), tail: None, new_line: "\r\n", found: false, orig_count: 0 }
    }
}

/// The file's units table, each entry's text kept, the fields read out (player 1 and type `Unit` when absent).
pub fn parse_units(text: &str) -> UnitsTable {
    let mut table = UnitsTable { new_line: new_line_of(text), ..Default::default() };
    let masked = mask(text);
    let Some(span) = find_table(&masked, "units") else { return table };
    table.found = true;
    table.header = text[span.key_start..=span.open].to_string();
    let (list, tail) = entries(text, &masked, span.open, span.close);
    for (index, en) in list.iter().enumerate() {
        let raw = text[en.open + 1..en.close].to_string();
        let at = at_field(&raw);
        let (x, y) = at.unwrap_or((0, 0));
        let unit_type = string_field(&raw, "type").unwrap_or_else(|| "Unit".into());
        let player = int_field(&raw, "player").unwrap_or(1);
        let weapon = string_field(&raw, "weapon").unwrap_or_default();
        table.units.push(Unit {
            orig: Some((unit_type.clone(), player, x, y, weapon.clone())),
            unit_type,
            player,
            x,
            y,
            has_at: at.is_some(),
            name: string_field(&raw, "name"),
            weapon,
            raw: Some(raw),
            lead: Some(en.lead.clone()),
            orig_index: Some(index),
        });
    }
    table.orig_count = list.len();
    table.tail = Some(tail);
    table
}

/// `s` without its first comma outside comments and strings (the separator before an entry).
fn remove_first_comma(s: &str) -> String {
    match mask(s).find(',') {
        Some(k) => format!("{}{}", &s[..k], &s[k + 1..]),
        None => s.to_string(),
    }
}

/// The units table's text, from its key to its `}`. A table read and written back unchanged is the same text.
pub fn emit_units(table: &UnitsTable) -> String {
    let nl = table.new_line;
    let mut out = table.header.clone();
    let n = table.units.len();
    for (i, u) in table.units.iter().enumerate() {
        let mut lead = u.lead.clone().unwrap_or_else(|| format!("{nl}    "));
        let has_comma = mask(&lead).contains(',');
        if i == 0 && has_comma {
            lead = remove_first_comma(&lead); // the first entry now (the ones before it were deleted)
        } else if i > 0 && !has_comma {
            lead = format!(",{lead}"); // a new entry, or the old last one with an entry after it
        }
        out += &lead;
        out.push('{');
        out += &u.entry_text();
        out.push('}');
    }
    let mut tail = table.tail.clone().unwrap_or_else(|| format!("{nl}  "));
    if n == 0 {
        tail = remove_first_comma(&tail); // no entry left for a trailing comma to follow
    } else if table.units[n - 1].raw.is_none() {
        // The last entry is new: end it with a comma and put the closing brace on its own line.
        if !tail.contains('\n') {
            tail = format!("{nl}  ");
        }
        if !mask(&tail).contains(',') {
            tail = format!(",{tail}");
        }
    }
    out += &tail;
    out.push('}');
    out
}

/// A lead up to and including its first line break (all of it when it has none), and what comes after.
fn split_first_line(s: &str) -> (&str, &str) {
    match s.find('\n') {
        Some(k) => (&s[..=k], &s[k + 1..]),
        None => (s, ""),
    }
}

/// Removes unit `i`. The text between two entries is the rest of the earlier entry's line (its separator and a
/// comment on that line), then the lines before the next entry (a heading such as `-- Player 2's base`). The deleted
/// entry's own line comment goes with it; the line comment of the entry before it, and a heading above it, stay.
pub fn delete_unit(table: &mut UnitsTable, i: usize) {
    let gone = table.units.remove(i);
    let Some(gone_lead) = gone.lead else { return }; // a new unit: nothing around it to keep
    let following =
        if i < table.units.len() { table.units[i].lead.clone() } else { Some(table.tail.clone().unwrap_or_default()) };
    let Some(following) = following else { return }; // the next one is new too
    let (gone_first, gone_rest) = split_first_line(&gone_lead);
    let (_, rest) = split_first_line(&following);
    let mut rest = rest.to_string();
    if gone_rest.contains("--") {
        rest = format!("{}{rest}", gone_rest.trim_end_matches([' ', '\t']));
    }
    let merged = format!("{gone_first}{rest}");
    if i < table.units.len() {
        table.units[i].lead = Some(merged);
    } else {
        table.tail = Some(merged);
    }
}

/// A beacon's `after = n` counts the units table's entries placed before it. With some of the file's units deleted,
/// n becomes the number of the remaining ones that were among those n, so each beacon still follows the same unit.
pub fn fix_beacon_after(text: &str, table: &UnitsTable) -> String {
    let masked = mask(text);
    let Some(span) = find_table(&masked, "beacons") else { return text.to_string() };
    let kept: std::collections::HashSet<usize> = table.units.iter().filter_map(|u| u.orig_index).collect();
    let deleted: Vec<usize> = (0..table.orig_count).filter(|k| !kept.contains(k)).collect();
    if deleted.is_empty() {
        return text.to_string();
    }
    let m = masked.as_bytes();
    let mut out = String::new();
    let mut last = 0;
    for (_, vs) in assignments(&masked, "after", span.open).take_while(|&(ks, _)| ks < span.close) {
        let mut e = vs;
        while e < m.len() && m[e].is_ascii_digit() {
            e += 1;
        }
        let Ok(n) = text[vs..e].parse::<usize>() else { continue };
        let n2 = n - deleted.iter().filter(|&&k| k < n).count();
        out += &text[last..vs];
        out += &n2.to_string();
        last = e;
    }
    out += &text[last..];
    out
}

// ===== beacons, walls and markers =====

/// Which table a map object comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectKind {
    Beacon,
    /// Walls and tubes.
    Wall,
    Marker,
}

/// A beacon, wall or tube, or marker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapObject {
    /// The table it is in.
    pub kind: ObjectKind,
    /// Beacon: `MiningBeacon`, `MagmaVent`, `Fumarole`; wall: `Tube`, `Wall`, `LavaWall`, `MicrobeWall`; marker:
    /// `Circle`, `DNA`, `Beaker`. The runtime's default when the entry has none.
    pub obj_type: String,
    /// A mining beacon's ore: `Common`, `Rare`, or `Random` when the entry doesn't say.
    pub ore: String,
    /// Status-bar coordinates.
    pub x: i32,
    pub y: i32,
    /// A marker's key; a beacon's `name` field.
    pub name: Option<String>,
}

/// The beacons, walls and markers that have a plain `at = { x, y }`.
pub fn parse_map_objects(text: &str) -> Vec<MapObject> {
    let masked = mask(text);
    let mut out = Vec::new();
    // Each table, the kind it holds, and the type an entry without one gets (as OP2Lua's Placement.cpp defaults it).
    for (table, kind, default) in [
        ("beacons", ObjectKind::Beacon, "MiningBeacon"),
        ("walls", ObjectKind::Wall, "Wall"),
        ("markers", ObjectKind::Marker, "Circle"),
    ] {
        let Some(span) = find_table(&masked, table) else { continue };
        let (list, _) = entries(text, &masked, span.open, span.close);
        for en in list {
            let raw = &text[en.open + 1..en.close];
            let Some((x, y)) = at_field(raw) else { continue };
            out.push(MapObject {
                kind,
                obj_type: string_field(raw, "type").unwrap_or_else(|| default.into()),
                ore: string_field(raw, "ore").unwrap_or_else(|| "Random".into()),
                x,
                y,
                name: if kind == ObjectKind::Marker { en.key.clone() } else { string_field(raw, "name") },
            });
        }
    }
    out
}

// ===== regions =====

/// A named region: a rectangle (x1, y1)-(x2, y2) or a point, in status-bar coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    /// The key it is published under (`regions[name]`): a Lua name.
    pub name: String,
    /// A point is kept with x2, y2 equal to x1, y1.
    pub point: bool,
    /// One corner, and the opposite one (either order).
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32,
}

impl Region {
    /// Puts the corners in order (x1 <= x2, y1 <= y2).
    pub fn normalize(&mut self) {
        if self.x1 > self.x2 {
            std::mem::swap(&mut self.x1, &mut self.x2);
        }
        if self.y1 > self.y2 {
            std::mem::swap(&mut self.y1, &mut self.y2);
        }
    }

    /// Whether the region covers status-bar tile (x, y).
    pub fn contains(&self, x: i32, y: i32) -> bool {
        (self.x1.min(self.x2)..=self.x1.max(self.x2)).contains(&x)
            && (self.y1.min(self.y2)..=self.y1.max(self.y2)).contains(&y)
    }
}

/// The regions table: `name = { x, y }` (a point) and `name = { x1, y1, x2, y2 }` (a rectangle).
pub fn parse_regions(text: &str) -> Vec<Region> {
    let masked = mask(text);
    let Some(span) = find_table(&masked, "regions") else { return Vec::new() };
    let (list, _) = entries(text, &masked, span.open, span.close);
    list.into_iter()
        .filter_map(|en| {
            let name = en.key?;
            match int_list(&text[en.open..=en.close])?.as_slice() {
                [x, y] => Some(Region { name, point: true, x1: *x, y1: *y, x2: *x, y2: *y }),
                [x1, y1, x2, y2] => Some(Region { name, point: false, x1: *x1, y1: *y1, x2: *x2, y2: *y2 }),
                _ => None,
            }
        })
        .collect()
}

/// The `regions = { ... }` table's text, one region a line, with the file's line ending.
pub fn emit_regions(regions: &[Region], nl: &str) -> String {
    let mut s = format!("regions = {{{nl}");
    for r in regions {
        if r.point {
            s += &format!("    {} = {{ {}, {} }},{nl}", r.name, r.x1, r.y1);
        } else {
            s += &format!("    {} = {{ {}, {}, {}, {} }},{nl}", r.name, r.x1, r.y1, r.x2, r.y2);
        }
    }
    s + "  }"
}

// ===== the whole file =====

/// A placement file: its text and what it holds. Change `header`, `units` or `regions`, then [`Placement::to_text`]
/// writes them into the text and leaves the rest as it was. `objects` (beacons, walls, markers) is read only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    /// The file's text as read.
    pub text: String,
    /// What the file holds, as read; change these and call [`Placement::to_text`].
    pub header: Header,
    pub units: UnitsTable,
    pub regions: Vec<Region>,
    /// Beacons, walls and markers, read only.
    pub objects: Vec<MapObject>,
}

impl Placement {
    /// Reads placement text.
    pub fn parse(text: &str) -> Self {
        Self {
            text: text.to_string(),
            header: Header::read(text),
            units: parse_units(text),
            regions: parse_regions(text),
            objects: parse_map_objects(text),
        }
    }

    /// Reads a placement file (UTF-8 text).
    pub fn read(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|source| Error::Io { path: path.display().to_string(), source })?;
        let text = String::from_utf8(bytes)
            .map_err(|_| Error::Invalid(format!("{} isn't UTF-8 text", path.display())))?;
        Ok(Self::parse(&text))
    }

    /// The file's text with the header, units and regions written in. Unchanged, it is the text read. The regions
    /// table is rewritten only when the regions changed (it is written in a layout of its own); deleted units renumber
    /// the beacons' `after = n`.
    pub fn to_text(&self) -> String {
        let mut out = self.header.write_into(&self.text);
        if parse_regions(&self.text) != self.regions {
            out = replace_table(&out, "regions", &emit_regions(&self.regions, new_line_of(&self.text)));
        }
        if self.units.found || !self.units.units.is_empty() {
            out = replace_table(&out, "units", &emit_units(&self.units));
            out = fix_beacon_after(&out, &self.units);
        }
        out
    }

    /// Whether [`Placement::to_text`] differs from the text read.
    pub fn changed(&self) -> bool {
        self.to_text() != self.text
    }

    /// Writes [`Placement::to_text`] to `path`.
    pub fn write(&self, path: impl AsRef<Path>) -> Result<(), Error> {
        let path = path.as_ref();
        std::fs::write(path, self.to_text()).map_err(|source| Error::Io { path: path.display().to_string(), source })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "-- a test { layout }\nreturn {\n  name = \"Test\", map = \"eden01.map\", tech = \"MULTITEK.TXT\", type = \"Colony\",\n  max_tech = 12, players_count = 2,\n  units = {\n    -- Player 1's base\n    { type = \"CommandCenter\", player = 1, at = { 28, 56 } },  -- the CC\n    { type = \"Lynx\", player = 1, at = { 30, 54 }, weapon = \"Laser\", direction = \"East\" },\n\n    -- Player 2 (\"the enemy\" {braces})\n    { type = \"Tokamak\", player = 2, at = { 60, 44 } },\n    { type = \"Scout\", at = { x = 5, y = 6, raw = true } },\n  },\n  beacons = { { at = { 5, 6 }, ore = \"Rare\", after = 3 }, { at = { 7, 8 }, after = 1 } },\n  walls = { { type = \"Tube\", at = { 40, 44 } } },\n  markers = { home = { at = { 9, 9 } } },\n  regions = { base = { 1, 2, 3, 4 }, spot = { 5, 6 } },\n}\n";

    #[test]
    fn mask_blanks_comments_and_strings() {
        let t = "a = \"{x}\" -- {\n--[[ { ]] b = [==[ } ]==] }";
        let m = mask(t);
        assert_eq!(m.len(), t.len());
        assert_eq!(m.matches('{').count(), 0);
        assert_eq!(m.matches('}').count(), 1);
        assert!(m.contains('\n'));
    }

    #[test]
    fn unchanged_round_trip_is_identical() {
        let p = Placement::parse(SAMPLE);
        assert_eq!(p.units.units.len(), 4);
        assert_eq!(p.to_text(), SAMPLE);
        assert!(!p.changed());
        let crlf = SAMPLE.replace('\n', "\r\n");
        assert_eq!(Placement::parse(&crlf).to_text(), crlf);
    }

    #[test]
    fn reads_everything() {
        let p = Placement::parse(SAMPLE);
        assert_eq!(p.header.name.as_deref(), Some("Test"));
        assert_eq!(p.header.mission_type.as_deref(), Some("Colony"));
        assert_eq!((p.header.max_tech, p.header.players), (Some(12), Some(2)));
        let u = &p.units.units[1];
        assert_eq!((u.unit_type.as_str(), u.player, u.x, u.y, u.weapon.as_str()), ("Lynx", 1, 30, 54, "Laser"));
        assert!(!p.units.units[3].has_at && p.units.units[3].player == 1);
        assert_eq!(p.objects.len(), 4);
        assert_eq!(p.objects[0].ore, "Rare");
        assert_eq!(p.objects[2].obj_type, "Tube");
        assert_eq!(p.objects[3].name.as_deref(), Some("home"));
        assert_eq!(p.regions[0], Region { name: "base".into(), point: false, x1: 1, y1: 2, x2: 3, y2: 4 });
        assert!(p.regions[1].point && p.regions[1].contains(5, 6));
    }

    #[test]
    fn edits_only_what_changed() {
        let mut p = Placement::parse(SAMPLE);
        p.units.units[1].x = 31;
        p.units.units[1].weapon = String::new();
        p.header.name = Some("Iron Front".into());
        p.header.max_tech = Some(8);
        let out = p.to_text();
        assert!(out.contains("{ type = \"Lynx\", player = 1, at = { 31, 54 }, direction = \"East\" }"), "{out}");
        assert!(out.contains("name = \"Iron Front\"") && out.contains("max_tech = 8,"), "{out}");
        assert!(out.contains("-- the CC") && out.contains("regions = { base = { 1, 2, 3, 4 }"), "{out}");
    }

    #[test]
    fn delete_keeps_headings_and_renumbers_beacons() {
        let mut p = Placement::parse(SAMPLE);
        delete_unit(&mut p.units, 0);
        let out = p.to_text();
        assert!(out.contains("-- Player 1's base"), "{out}");
        assert!(!out.contains("-- the CC"), "{out}");
        assert!(out.contains("after = 2") && out.contains("after = 0"), "{out}");
        assert_eq!(parse_units(&out).units.len(), 3);
    }

    #[test]
    fn adds_units_and_regions() {
        let mut p = Placement::parse(SAMPLE);
        p.units.units.push(Unit::new("ConVec", 2, 10, 11, ""));
        p.regions = vec![Region { name: "zone".into(), point: true, x1: 4, y1: 5, x2: 4, y2: 5 }];
        let out = p.to_text();
        let again = Placement::parse(&out);
        assert_eq!(again.units.units.len(), 5);
        assert_eq!(again.units.units[4].unit_type, "ConVec");
        assert_eq!(again.regions, p.regions);
        assert!(out.contains("markers = { home"));
    }

    #[test]
    fn adds_tables_to_a_file_without_them() {
        let text = "return {\r\n  name = \"X\",\r\n}\r\n";
        let mut p = Placement::parse(text);
        p.units.units.push(Unit::new("Scout", 1, 3, 4, ""));
        p.regions.push(Region { name: "r".into(), point: true, x1: 1, y1: 1, x2: 1, y2: 1 });
        let out = p.to_text();
        assert_eq!(Placement::parse(&out).units.units.len(), 1);
        assert_eq!(Placement::parse(&out).regions.len(), 1);
        assert!(out.ends_with("}\r\n"));
    }

    #[test]
    fn header_fields_are_top_level_only() {
        let text = "return {
  units = { { type = \"Lynx\", name = \"a\" } },
  map = \"m.map\",
}
";
        let h = Header::read(text);
        assert_eq!((h.mission_type, h.name, h.map.as_deref()), (None, None, Some("m.map")));
        let mut p = Placement::parse(text);
        p.header.mission_type = Some("Colony".into());
        assert_eq!(p.to_text(), text); // not in the file: not added, and the unit's type isn't touched
    }

    #[test]
    fn keys_need_a_word_boundary() {
        let text = "return { my_units = { { type = \"Lynx\" } }, map == x, map = \"a.map\" }";
        assert!(!parse_units(text).found);
        assert_eq!(scalar_string(text, "map").as_deref(), Some("a.map"));
    }
}
