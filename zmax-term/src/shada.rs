//! nvim ShaDa ("shared data") files — the MessagePack container `:wshada`
//! writes and `:rshada` reads back.
//!
//! starting.txt:1193-1204 (*shada-format*): "ShaDa files are concats of
//! MessagePack entries. Each entry is a concat of exactly four MessagePack
//! objects": the entry type (unsigned integer, never zero), the entry timestamp
//! (unsigned integer), "the length of the fourth entry ... used for fast
//! skipping without parsing", and the entry data itself — "either map or
//! array".
//!
//! This module owns the format and nothing else: the encoder/decoder for the
//! MessagePack subset the format uses ([`Value`]), the entry model ([`Entry`] /
//! [`EntryKind`] — a variant per entry type zmax has state for, and
//! [`EntryKind::Unknown`] for the rest, which includes nvim's type 6 global
//! variables), and the merge `:wshada` performs against the file already on
//! disk. *Which* editor state becomes an entry, and what an entry does when it
//! is read back, is the command's job — see `ex_wshada` / `ex_rshada` in
//! `commands::typed`.
//!
//! The MessagePack subset is hand-rolled: the format only ever uses uint, int,
//! bool, bin, str, array and map, so a msgpack dependency would be all cost.
//! Note the split the spec draws — "All string values in those containers are
//! either binary (applies to filenames) or UTF-8, yet parser needs to expect
//! that invalid bytes may be present in a UTF-8 string" — so file names go out
//! as bin, text goes out as str, and *both* are accepted on the way back in.

use std::path::PathBuf;

// ---------------------------------------------------------------------------
// The MessagePack subset.
// ---------------------------------------------------------------------------

/// One MessagePack object. Only the types ShaDa entries are built from; an ext
/// or float32 object decodes into the nearest of these (a float becomes
/// [`Value::F64`]) so an entry another editor wrote still round-trips.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Nil,
    Bool(bool),
    Uint(u64),
    Int(i64),
    F64(f64),
    Str(String),
    Bin(Vec<u8>),
    Array(Vec<Value>),
    Map(Vec<(Value, Value)>),
}

impl Value {
    /// Append the MessagePack encoding of this object to `out`.
    pub fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Value::Nil => out.push(0xc0),
            Value::Bool(false) => out.push(0xc2),
            Value::Bool(true) => out.push(0xc3),
            Value::Uint(n) => encode_uint(*n, out),
            Value::Int(n) if *n >= 0 => encode_uint(*n as u64, out),
            Value::Int(n) => encode_int(*n, out),
            Value::F64(f) => {
                out.push(0xcb);
                out.extend_from_slice(&f.to_be_bytes());
            }
            Value::Str(s) => {
                let b = s.as_bytes();
                match b.len() {
                    n if n < 32 => out.push(0xa0 | n as u8),
                    n if n <= u8::MAX as usize => out.extend_from_slice(&[0xd9, n as u8]),
                    n if n <= u16::MAX as usize => {
                        out.push(0xda);
                        out.extend_from_slice(&(n as u16).to_be_bytes());
                    }
                    n => {
                        out.push(0xdb);
                        out.extend_from_slice(&(n as u32).to_be_bytes());
                    }
                }
                out.extend_from_slice(b);
            }
            Value::Bin(b) => {
                match b.len() {
                    n if n <= u8::MAX as usize => out.extend_from_slice(&[0xc4, n as u8]),
                    n if n <= u16::MAX as usize => {
                        out.push(0xc5);
                        out.extend_from_slice(&(n as u16).to_be_bytes());
                    }
                    n => {
                        out.push(0xc6);
                        out.extend_from_slice(&(n as u32).to_be_bytes());
                    }
                }
                out.extend_from_slice(b);
            }
            Value::Array(items) => {
                encode_len(items.len(), 0x90, 0xdc, 0xdd, out);
                for item in items {
                    item.encode(out);
                }
            }
            Value::Map(pairs) => {
                encode_len(pairs.len(), 0x80, 0xde, 0xdf, out);
                for (k, v) in pairs {
                    k.encode(out);
                    v.encode(out);
                }
            }
        }
    }

    /// The object's own encoding, on its own — what the entry's third object has
    /// to count bytes of.
    pub fn to_vec(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode(&mut out);
        out
    }

    /// The object as an unsigned integer, for the keys the spec types
    /// `UInteger` (a negative int is not one, and reads as `None`).
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Uint(n) => Some(*n),
            Value::Int(n) if *n >= 0 => Some(*n as u64),
            _ => None,
        }
    }

    /// The object as a signed integer — the `so` (search offset) key, which is
    /// the only signed number in the format.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Uint(n) => i64::try_from(*n).ok(),
            Value::Int(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// The object as text. Both str and bin are accepted: the spec writes file
    /// names as bin and everything else as UTF-8, and invalid bytes have to be
    /// tolerated rather than rejected (starting.txt:1202-1204).
    pub fn as_text(&self) -> Option<String> {
        match self {
            Value::Str(s) => Some(s.clone()),
            Value::Bin(b) => Some(String::from_utf8_lossy(b).into_owned()),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }

    /// The value stored under a map key, comparing the key as text so a writer
    /// that used bin keys is read the same as one that used str.
    pub fn map_get(&self, key: &str) -> Option<&Value> {
        let Value::Map(pairs) = self else { return None };
        pairs
            .iter()
            .find(|(k, _)| k.as_text().as_deref() == Some(key))
            .map(|(_, v)| v)
    }
}

/// A positive integer in the narrowest MessagePack encoding that holds it.
fn encode_uint(n: u64, out: &mut Vec<u8>) {
    match n {
        n if n < 0x80 => out.push(n as u8),
        n if n <= u8::MAX as u64 => out.extend_from_slice(&[0xcc, n as u8]),
        n if n <= u16::MAX as u64 => {
            out.push(0xcd);
            out.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n if n <= u32::MAX as u64 => {
            out.push(0xce);
            out.extend_from_slice(&(n as u32).to_be_bytes());
        }
        n => {
            out.push(0xcf);
            out.extend_from_slice(&n.to_be_bytes());
        }
    }
}

/// A negative integer, likewise narrowest-first.
fn encode_int(n: i64, out: &mut Vec<u8>) {
    match n {
        n if n >= -32 => out.push((n as i8) as u8),
        n if n >= i8::MIN as i64 => out.extend_from_slice(&[0xd0, (n as i8) as u8]),
        n if n >= i16::MIN as i64 => {
            out.push(0xd1);
            out.extend_from_slice(&(n as i16).to_be_bytes());
        }
        n if n >= i32::MIN as i64 => {
            out.push(0xd2);
            out.extend_from_slice(&(n as i32).to_be_bytes());
        }
        n => {
            out.push(0xd3);
            out.extend_from_slice(&n.to_be_bytes());
        }
    }
}

/// The header of an array or a map: the fixed form when the length fits in the
/// low nibble, else the 16- or 32-bit form.
fn encode_len(len: usize, fixed: u8, wide16: u8, wide32: u8, out: &mut Vec<u8>) {
    match len {
        n if n < 16 => out.push(fixed | n as u8),
        n if n <= u16::MAX as usize => {
            out.push(wide16);
            out.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            out.push(wide32);
            out.extend_from_slice(&(n as u32).to_be_bytes());
        }
    }
}

/// Decode one object at `*at`, advancing `*at` past it. `Err` is a parse failure
/// — a critical error for the file as a whole (starting.txt:1359).
fn decode(bytes: &[u8], at: &mut usize) -> anyhow::Result<Value> {
    let head = *take(bytes, at, 1)?.first().expect("one byte was taken");
    let value = match head {
        0x00..=0x7f => Value::Uint(head as u64),
        0x80..=0x8f => decode_map(bytes, at, (head & 0x0f) as usize)?,
        0x90..=0x9f => decode_array(bytes, at, (head & 0x0f) as usize)?,
        0xa0..=0xbf => decode_str(bytes, at, (head & 0x1f) as usize)?,
        0xc0 => Value::Nil,
        0xc2 => Value::Bool(false),
        0xc3 => Value::Bool(true),
        0xc4 => {
            let n = be_uint(bytes, at, 1)? as usize;
            Value::Bin(take(bytes, at, n)?.to_vec())
        }
        0xc5 => {
            let n = be_uint(bytes, at, 2)? as usize;
            Value::Bin(take(bytes, at, n)?.to_vec())
        }
        0xc6 => {
            let n = be_uint(bytes, at, 4)? as usize;
            Value::Bin(take(bytes, at, n)?.to_vec())
        }
        0xca => {
            let bits = be_uint(bytes, at, 4)? as u32;
            Value::F64(f32::from_bits(bits) as f64)
        }
        0xcb => Value::F64(f64::from_bits(be_uint(bytes, at, 8)?)),
        0xcc => Value::Uint(be_uint(bytes, at, 1)?),
        0xcd => Value::Uint(be_uint(bytes, at, 2)?),
        0xce => Value::Uint(be_uint(bytes, at, 4)?),
        0xcf => Value::Uint(be_uint(bytes, at, 8)?),
        0xd0 => Value::Int(be_uint(bytes, at, 1)? as u8 as i8 as i64),
        0xd1 => Value::Int(be_uint(bytes, at, 2)? as u16 as i16 as i64),
        0xd2 => Value::Int(be_uint(bytes, at, 4)? as u32 as i32 as i64),
        0xd3 => Value::Int(be_uint(bytes, at, 8)? as i64),
        0xd9 => {
            let n = be_uint(bytes, at, 1)? as usize;
            decode_str(bytes, at, n)?
        }
        0xda => {
            let n = be_uint(bytes, at, 2)? as usize;
            decode_str(bytes, at, n)?
        }
        0xdb => {
            let n = be_uint(bytes, at, 4)? as usize;
            decode_str(bytes, at, n)?
        }
        0xdc => {
            let n = be_uint(bytes, at, 2)? as usize;
            decode_array(bytes, at, n)?
        }
        0xdd => {
            let n = be_uint(bytes, at, 4)? as usize;
            decode_array(bytes, at, n)?
        }
        0xde => {
            let n = be_uint(bytes, at, 2)? as usize;
            decode_map(bytes, at, n)?
        }
        0xdf => {
            let n = be_uint(bytes, at, 4)? as usize;
            decode_map(bytes, at, n)?
        }
        0xe0..=0xff => Value::Int(head as i8 as i64),
        other => anyhow::bail!("unsupported MessagePack type 0x{other:02x}"),
    };
    Ok(value)
}

fn take<'a>(bytes: &'a [u8], at: &mut usize, n: usize) -> anyhow::Result<&'a [u8]> {
    let end = at.checked_add(n).filter(|end| *end <= bytes.len());
    let Some(end) = end else {
        anyhow::bail!("truncated MessagePack object");
    };
    let slice = &bytes[*at..end];
    *at = end;
    Ok(slice)
}

fn be_uint(bytes: &[u8], at: &mut usize, n: usize) -> anyhow::Result<u64> {
    Ok(take(bytes, at, n)?
        .iter()
        .fold(0u64, |acc, b| (acc << 8) | *b as u64))
}

fn decode_str(bytes: &[u8], at: &mut usize, n: usize) -> anyhow::Result<Value> {
    let raw = take(bytes, at, n)?;
    Ok(Value::Str(String::from_utf8_lossy(raw).into_owned()))
}

fn decode_array(bytes: &[u8], at: &mut usize, n: usize) -> anyhow::Result<Value> {
    let mut items = Vec::with_capacity(n.min(1024));
    for _ in 0..n {
        items.push(decode(bytes, at)?);
    }
    Ok(Value::Array(items))
}

fn decode_map(bytes: &[u8], at: &mut usize, n: usize) -> anyhow::Result<Value> {
    let mut pairs = Vec::with_capacity(n.min(1024));
    for _ in 0..n {
        let k = decode(bytes, at)?;
        let v = decode(bytes, at)?;
        pairs.push((k, v));
    }
    Ok(Value::Map(pairs))
}

// ---------------------------------------------------------------------------
// The entry model.
// ---------------------------------------------------------------------------

/// Entry type numbers, starting.txt:1208-1342. Type 0 is not a valid entry.
pub const TYPE_HEADER: u64 = 1;
pub const TYPE_SEARCH_PATTERN: u64 = 2;
pub const TYPE_SUB_STRING: u64 = 3;
pub const TYPE_HISTORY: u64 = 4;
pub const TYPE_REGISTER: u64 = 5;
pub const TYPE_GLOBAL_MARK: u64 = 7;
pub const TYPE_JUMP: u64 = 8;
pub const TYPE_BUFFER_LIST: u64 = 9;
pub const TYPE_LOCAL_MARK: u64 = 10;
pub const TYPE_CHANGE: u64 = 11;

/// History types, starting.txt:1266-1269: "0 - cmd, 1 - search, 2 - expr,
/// 3 - input, 4 - debug".
pub const HISTORY_CMD: u64 = 0;
pub const HISTORY_SEARCH: u64 = 1;

/// A position in a file — the shape shared by GlobalMark, LocalMark, Jump and
/// Change (starting.txt:1307-1328). `line` is 1-based ("Must be greater then
/// zero"), `col` is the 0-based byte column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Position {
    pub file: PathBuf,
    pub line: usize,
    pub col: usize,
}

/// One register (starting.txt:1270-1298). `contents` is one string per *line*,
/// which is what nvim reads; `value_lines` is zmax's own addition — see
/// [`EntryKind::Register`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Register {
    pub name: char,
    /// `rt`: 0 charwise, 1 linewise, 2 blockwise.
    pub kind: u64,
    /// `rw`: only meaningful for a blockwise register.
    pub width: u64,
    /// `rc`: "Each entry in the array represents its own line."
    pub contents: Vec<String>,
    /// `ru`: whether the unnamed register pointed at this one.
    pub unnamed: bool,
    /// `zv` (zmax): how many `contents` lines each of zmax's own register values
    /// occupies. zmax registers hold a *list* of values (one per selection),
    /// which nvim's line array cannot express; the spec allows extra keys
    /// ("Other keys are allowed for compatibility reasons"), so the boundaries
    /// ride along here and nvim ignores them. Only written when there is more
    /// than one value, so fewer than two entries — an nvim-written register, or
    /// a one-value zmax register — means "the whole line array is one value".
    pub value_lines: Vec<usize>,
}

/// The last search or substitute pattern (starting.txt:1224-1255). Field
/// defaults are the documented ones; a key equal to its default "is normally not
/// present", so [`EntryKind::to_object`] omits it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchPattern {
    /// `sp`: the pattern itself. Required.
    pub pattern: String,
    /// `ss`: this entry describes a `:substitute` pattern.
    pub substitute: bool,
    /// `su`: this was the last used search pattern.
    pub last_used: bool,
    /// `sm`: effective 'magic'.
    pub magic: bool,
    /// `sc`: effective 'smartcase'.
    pub smartcase: bool,
    /// `sl`: the pattern came with a line offset.
    pub line_offset: bool,
    /// `se`: the offset places the cursor at the end of the match.
    pub end_offset: bool,
    /// `so`: the offset value.
    pub offset: i64,
    /// `sh`: `v:hlsearch` is on.
    pub hlsearch: bool,
    /// `sb`: the search direction is backward.
    pub backward: bool,
}

impl Default for SearchPattern {
    fn default() -> Self {
        SearchPattern {
            pattern: String::new(),
            substitute: false,
            last_used: false,
            magic: true,
            smartcase: false,
            line_offset: true,
            end_offset: false,
            offset: 0,
            hlsearch: false,
            backward: false,
        }
    }
}

/// One ShaDa entry's data, by type.
#[derive(Clone, Debug, PartialEq)]
pub enum EntryKind {
    /// Type 1: "data that describes the generator instance that wrote this ShaDa
    /// file. It is ignored when reading ShaDa files."
    Header {
        generator: String,
        version: String,
        encoding: String,
        max_kbyte: u64,
        pid: u64,
    },
    /// Type 2.
    SearchPattern(SearchPattern),
    /// Type 3: the last `:substitute` replacement string.
    SubString(String),
    /// Type 4: one history line. `sep` is "only valid for search history".
    History {
        kind: u64,
        line: String,
        sep: Option<u8>,
    },
    /// Type 5.
    Register(Register),
    /// Type 7 (`global`) / type 10: a named mark. `'A` is global, `'a` local.
    Mark {
        global: bool,
        name: char,
        pos: Position,
    },
    /// Type 8: one jumplist position.
    Jump(Position),
    /// Type 11: one changelist position.
    Change(Position),
    /// Type 9: the buffer list, one position per buffer.
    BufferList(Vec<Position>),
    /// Any entry this build does not model, kept verbatim — starting.txt:1341
    /// allows any other type "for compatibility reasons", and a merge that threw
    /// them away would silently strip whatever nvim (or a newer zmax) wrote.
    Unknown { typ: u64, data: Value },
}

/// One entry: its data plus the timestamp that is the entry's second object.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub timestamp: u64,
    pub kind: EntryKind,
}

/// `(key, value)` for a map, with the key as a str object.
fn pair(key: &str, value: Value) -> (Value, Value) {
    (Value::Str(key.to_string()), value)
}

/// A file name goes out as bin — "binary (applies to filenames)".
fn file_value(path: &std::path::Path) -> Value {
    Value::Bin(path.to_string_lossy().into_owned().into_bytes())
}

impl Position {
    fn to_pairs(&self, name: Option<char>) -> Vec<(Value, Value)> {
        let mut map = vec![pair("f", file_value(&self.file))];
        // `l` defaults to 1 and `c` to 0; the defaults are normally left out.
        if self.line != 1 {
            map.push(pair("l", Value::Uint(self.line as u64)));
        }
        if self.col != 0 {
            map.push(pair("c", Value::Uint(self.col as u64)));
        }
        // `n` defaults to 34 (`"`) and is "Only valid for GlobalMark and
        // LocalMark entries".
        if let Some(name) = name.filter(|n| *n != '"') {
            map.push(pair("n", Value::Uint(name as u64)));
        }
        map
    }

    fn from_value(value: &Value) -> Option<Position> {
        let file = value.map_get("f")?.as_text()?;
        if file.is_empty() {
            return None;
        }
        Some(Position {
            file: PathBuf::from(file),
            line: value
                .map_get("l")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .max(1) as usize,
            col: value.map_get("c").and_then(Value::as_u64).unwrap_or(0) as usize,
        })
    }

    /// The mark name a position map carries, defaulting to `"` (34).
    fn name_from_value(value: &Value) -> char {
        let code = value.map_get("n").and_then(Value::as_u64).unwrap_or(34);
        char::from_u32(code as u32).unwrap_or('"')
    }
}

impl EntryKind {
    /// The entry's type number and its data object — the first and fourth of the
    /// four objects an entry is made of.
    pub fn to_object(&self) -> (u64, Value) {
        match self {
            EntryKind::Header {
                generator,
                version,
                encoding,
                max_kbyte,
                pid,
            } => (
                TYPE_HEADER,
                Value::Map(vec![
                    pair("generator", Value::Bin(generator.clone().into_bytes())),
                    pair("version", Value::Bin(version.clone().into_bytes())),
                    pair("encoding", Value::Bin(encoding.clone().into_bytes())),
                    pair("max_kbyte", Value::Uint(*max_kbyte)),
                    pair("pid", Value::Uint(*pid)),
                ]),
            ),
            EntryKind::SearchPattern(p) => {
                let d = SearchPattern::default();
                let mut map = vec![pair("sp", Value::Bin(p.pattern.clone().into_bytes()))];
                let mut flag = |key: &str, on: bool, default: bool| {
                    if on != default {
                        map.push(pair(key, Value::Bool(on)));
                    }
                };
                flag("sm", p.magic, d.magic);
                flag("sc", p.smartcase, d.smartcase);
                flag("sl", p.line_offset, d.line_offset);
                flag("se", p.end_offset, d.end_offset);
                flag("su", p.last_used, d.last_used);
                flag("ss", p.substitute, d.substitute);
                flag("sh", p.hlsearch, d.hlsearch);
                flag("sb", p.backward, d.backward);
                if p.offset != d.offset {
                    map.push(pair("so", Value::Int(p.offset)));
                }
                (TYPE_SEARCH_PATTERN, Value::Map(map))
            }
            EntryKind::SubString(s) => (
                TYPE_SUB_STRING,
                Value::Array(vec![Value::Bin(s.clone().into_bytes())]),
            ),
            EntryKind::History { kind, line, sep } => {
                let mut items = vec![Value::Uint(*kind), Value::Bin(line.clone().into_bytes())];
                if let Some(sep) = sep {
                    items.push(Value::Uint(*sep as u64));
                }
                (TYPE_HISTORY, Value::Array(items))
            }
            EntryKind::Register(r) => {
                let mut map = vec![
                    pair("n", Value::Uint(r.name as u64)),
                    pair(
                        "rc",
                        Value::Array(
                            r.contents
                                .iter()
                                .map(|line| Value::Bin(line.clone().into_bytes()))
                                .collect(),
                        ),
                    ),
                ];
                if r.kind != 0 {
                    map.push(pair("rt", Value::Uint(r.kind)));
                }
                if r.width != 0 {
                    map.push(pair("rw", Value::Uint(r.width)));
                }
                if r.unnamed {
                    map.push(pair("ru", Value::Bool(true)));
                }
                if r.value_lines.len() > 1 {
                    map.push(pair(
                        "zv",
                        Value::Array(
                            r.value_lines
                                .iter()
                                .map(|n| Value::Uint(*n as u64))
                                .collect(),
                        ),
                    ));
                }
                (TYPE_REGISTER, Value::Map(map))
            }
            EntryKind::Mark { global, name, pos } => (
                if *global {
                    TYPE_GLOBAL_MARK
                } else {
                    TYPE_LOCAL_MARK
                },
                Value::Map(pos.to_pairs(Some(*name))),
            ),
            EntryKind::Jump(pos) => (TYPE_JUMP, Value::Map(pos.to_pairs(None))),
            EntryKind::Change(pos) => (TYPE_CHANGE, Value::Map(pos.to_pairs(None))),
            EntryKind::BufferList(buffers) => (
                TYPE_BUFFER_LIST,
                Value::Array(
                    buffers
                        .iter()
                        .map(|pos| Value::Map(pos.to_pairs(None)))
                        .collect(),
                ),
            ),
            EntryKind::Unknown { typ, data } => (*typ, data.clone()),
        }
    }

    /// The inverse: an entry type and its data object back into the model. A
    /// type this build models but whose data does not fit (a "logical" error,
    /// starting.txt:1346) degrades to [`EntryKind::Unknown`] rather than
    /// aborting the read — nvim skips such an entry and keeps going too.
    pub fn from_object(typ: u64, data: Value) -> EntryKind {
        let unknown = |data: Value| EntryKind::Unknown { typ, data };
        match typ {
            TYPE_HEADER => EntryKind::Header {
                generator: text_key(&data, "generator"),
                version: text_key(&data, "version"),
                encoding: text_key(&data, "encoding"),
                max_kbyte: data
                    .map_get("max_kbyte")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                pid: data.map_get("pid").and_then(Value::as_u64).unwrap_or(0),
            },
            TYPE_SEARCH_PATTERN => {
                let Some(pattern) = data.map_get("sp").and_then(Value::as_text) else {
                    return unknown(data);
                };
                let d = SearchPattern::default();
                let flag = |key: &str, default: bool| {
                    data.map_get(key)
                        .and_then(Value::as_bool)
                        .unwrap_or(default)
                };
                EntryKind::SearchPattern(SearchPattern {
                    pattern,
                    substitute: flag("ss", d.substitute),
                    last_used: flag("su", d.last_used),
                    magic: flag("sm", d.magic),
                    smartcase: flag("sc", d.smartcase),
                    line_offset: flag("sl", d.line_offset),
                    end_offset: flag("se", d.end_offset),
                    offset: data
                        .map_get("so")
                        .and_then(Value::as_i64)
                        .unwrap_or(d.offset),
                    hlsearch: flag("sh", d.hlsearch),
                    backward: flag("sb", d.backward),
                })
            }
            TYPE_SUB_STRING => match data.as_array().and_then(|items| items.first()) {
                // "More entries are allowed for compatibility reasons" — the
                // first one is the replacement string.
                Some(first) => match first.as_text() {
                    Some(s) => EntryKind::SubString(s),
                    None => unknown(data),
                },
                None => unknown(data),
            },
            TYPE_HISTORY => {
                let items = data.as_array().unwrap_or(&[]);
                // "Should have two or three entries."
                let (Some(kind), Some(line)) = (
                    items.first().and_then(Value::as_u64),
                    items.get(1).and_then(Value::as_text),
                ) else {
                    return unknown(data);
                };
                EntryKind::History {
                    kind,
                    line,
                    sep: items
                        .get(2)
                        .and_then(Value::as_u64)
                        .and_then(|n| u8::try_from(n).ok()),
                }
            }
            TYPE_REGISTER => {
                let (Some(name), Some(contents)) = (
                    data.map_get("n")
                        .and_then(Value::as_u64)
                        .and_then(|n| char::from_u32(n as u32)),
                    data.map_get("rc").and_then(Value::as_array),
                ) else {
                    return unknown(data);
                };
                EntryKind::Register(Register {
                    name,
                    kind: data.map_get("rt").and_then(Value::as_u64).unwrap_or(0),
                    width: data.map_get("rw").and_then(Value::as_u64).unwrap_or(0),
                    contents: contents
                        .iter()
                        .map(|v| v.as_text().unwrap_or_default())
                        .collect(),
                    unnamed: data.map_get("ru").and_then(Value::as_bool).unwrap_or(false),
                    value_lines: data
                        .map_get("zv")
                        .and_then(Value::as_array)
                        .map(|lens| {
                            lens.iter()
                                .map(|v| v.as_u64().unwrap_or(0) as usize)
                                .collect()
                        })
                        .unwrap_or_default(),
                })
            }
            TYPE_GLOBAL_MARK | TYPE_LOCAL_MARK => match Position::from_value(&data) {
                Some(pos) => EntryKind::Mark {
                    global: typ == TYPE_GLOBAL_MARK,
                    name: Position::name_from_value(&data),
                    pos,
                },
                None => unknown(data),
            },
            TYPE_JUMP => match Position::from_value(&data) {
                Some(pos) => EntryKind::Jump(pos),
                None => unknown(data),
            },
            TYPE_CHANGE => match Position::from_value(&data) {
                Some(pos) => EntryKind::Change(pos),
                None => unknown(data),
            },
            TYPE_BUFFER_LIST => match data.as_array() {
                Some(items) => {
                    EntryKind::BufferList(items.iter().filter_map(Position::from_value).collect())
                }
                None => unknown(data),
            },
            _ => unknown(data),
        }
    }

    /// What makes two entries "the same thing" when merging an old file with the
    /// state being written: the newer timestamp of a pair with equal keys wins,
    /// and everything else is kept. Keyed by type plus the entry's identity —
    /// a register by its name, a mark by its name and file, a history line by
    /// its text.
    pub fn merge_key(&self) -> (u64, String) {
        let id = match self {
            EntryKind::Header { .. } | EntryKind::SubString(_) | EntryKind::BufferList(_) => {
                String::new()
            }
            EntryKind::SearchPattern(p) => p.substitute.to_string(),
            EntryKind::History { kind, line, .. } => format!("{kind}\u{1}{line}"),
            EntryKind::Register(r) => r.name.to_string(),
            // A global mark's name is unique across all files — `m A` in a second
            // file *moves* `'A` — so the name alone identifies it, and a merge
            // must not end up holding two `'A`s. A local mark belongs to its
            // file, so that one is keyed by both.
            EntryKind::Mark {
                global: true, name, ..
            } => name.to_string(),
            EntryKind::Mark { name, pos, .. } => {
                format!("{name}\u{1}{}", pos.file.display())
            }
            // A jump or a change is identified by where it points: the same
            // position twice is one entry, a different one is another.
            EntryKind::Jump(pos) | EntryKind::Change(pos) => {
                format!("{}\u{1}{}\u{1}{}", pos.file.display(), pos.line, pos.col)
            }
            EntryKind::Unknown { data, .. } => format!("{:?}", data),
        };
        (self.entry_type(), id)
    }

    /// The entry's type number on its own — [`EntryKind::to_object`] without
    /// building (and cloning) the data object, which a merge over a file full of
    /// registers would otherwise do for every entry on both sides.
    pub fn entry_type(&self) -> u64 {
        match self {
            EntryKind::Header { .. } => TYPE_HEADER,
            EntryKind::SearchPattern(_) => TYPE_SEARCH_PATTERN,
            EntryKind::SubString(_) => TYPE_SUB_STRING,
            EntryKind::History { .. } => TYPE_HISTORY,
            EntryKind::Register(_) => TYPE_REGISTER,
            EntryKind::Mark { global: true, .. } => TYPE_GLOBAL_MARK,
            EntryKind::Mark { .. } => TYPE_LOCAL_MARK,
            EntryKind::Jump(_) => TYPE_JUMP,
            EntryKind::Change(_) => TYPE_CHANGE,
            EntryKind::BufferList(_) => TYPE_BUFFER_LIST,
            EntryKind::Unknown { typ, .. } => *typ,
        }
    }
}

fn text_key(data: &Value, key: &str) -> String {
    data.map_get(key)
        .and_then(Value::as_text)
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Files.
// ---------------------------------------------------------------------------

/// Encode entries into the bytes of a ShaDa file. `max_kbyte` is 'shada' `sN`:
/// "Maximum size of an item contents in KiB. If zero then nothing is saved.
/// Unlike Vim this applies to all items, except for the buffer list and header."
/// (options.txt:5604-5612) — measured, as documented, on the entry's data
/// object alone.
pub fn encode_file(entries: &[Entry], max_kbyte: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for entry in entries {
        let (typ, data) = entry.kind.to_object();
        let buf = data.to_vec();
        let exempt = matches!(typ, TYPE_HEADER | TYPE_BUFFER_LIST);
        if !exempt && (max_kbyte == 0 || buf.len() > max_kbyte.saturating_mul(1024)) {
            continue;
        }
        Value::Uint(typ).encode(&mut out);
        Value::Uint(entry.timestamp).encode(&mut out);
        Value::Uint(buf.len() as u64).encode(&mut out);
        out.extend_from_slice(&buf);
    }
    out
}

/// Decode a whole ShaDa file. Returns everything decoded before the first
/// critical error, plus that error — starting.txt:1351: "When reading, critical
/// errors cause the rest of the file to be skipped." The critical errors are the
/// documented ones (starting.txt:1353-1363): a non-unsigned type/timestamp/
/// length, a length past the end of the file, a zero type, a parse failure, and
/// entry data that does not consume exactly the declared number of bytes.
pub fn decode_file(bytes: &[u8]) -> (Vec<Entry>, Option<String>) {
    let mut entries = Vec::new();
    let mut at = 0usize;
    while at < bytes.len() {
        let header = (|| -> anyhow::Result<(u64, u64, usize)> {
            let typ = decode(bytes, &mut at)?
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("entry type is not an unsigned integer"))?;
            let timestamp = decode(bytes, &mut at)?
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("entry timestamp is not an unsigned integer"))?;
            let len = decode(bytes, &mut at)?
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("entry length is not an unsigned integer"))?;
            Ok((typ, timestamp, len as usize))
        })();
        let (typ, timestamp, len) = match header {
            Ok(header) => header,
            Err(e) => return (entries, Some(format!("E576: {e}"))),
        };
        if typ == 0 {
            return (entries, Some("E576: entry with zero type".to_string()));
        }
        let Some(end) = at.checked_add(len).filter(|end| *end <= bytes.len()) else {
            return (
                entries,
                Some("E576: entry length runs past the end of the file".to_string()),
            );
        };
        let mut data_at = at;
        let data = match decode(bytes, &mut data_at) {
            Ok(data) => data,
            Err(e) => return (entries, Some(format!("E576: {e}"))),
        };
        if data_at != end {
            return (
                entries,
                Some("E576: entry data does not match the declared length".to_string()),
            );
        }
        at = end;
        entries.push(Entry {
            timestamp,
            kind: EntryKind::from_object(typ, data),
        });
    }
    (entries, None)
}

/// The merge `:wshada` (without `!`) performs — starting.txt:1161-1163: "The
/// information in the file is first read in to make a merge between old and new
/// info." `old` is what the file holds, `new` is this session's state; where
/// both describe the same thing ([`EntryKind::merge_key`]) the newer timestamp
/// wins, and everything either side has alone is kept. `new` order is preserved,
/// with the old-only entries appended.
pub fn merge(old: Vec<Entry>, new: Vec<Entry>) -> Vec<Entry> {
    let mut merged: Vec<Entry> = Vec::with_capacity(old.len() + new.len());
    let mut keys: std::collections::HashMap<(u64, String), usize> =
        std::collections::HashMap::with_capacity(new.len());
    for entry in new {
        let key = entry.kind.merge_key();
        match keys.get(&key) {
            Some(&i) => merged[i] = entry,
            None => {
                keys.insert(key, merged.len());
                merged.push(entry);
            }
        }
    }
    for entry in old {
        // The header describes the instance that wrote the file, so the one
        // being written now replaces it outright rather than merging.
        if matches!(entry.kind, EntryKind::Header { .. }) {
            continue;
        }
        let key = entry.kind.merge_key();
        match keys.get(&key) {
            Some(&i) => {
                if entry.timestamp > merged[i].timestamp {
                    merged[i] = entry;
                }
            }
            None => {
                keys.insert(key, merged.len());
                merged.push(entry);
            }
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every MessagePack width the format can reach has to round-trip: the
    /// encoder picks the narrowest form, and a reader (nvim's, or ours on the
    /// next `:rshada`) has to get the same object back. The boundaries are where
    /// a hand-rolled codec goes wrong — 127/128 (fixint), 31/32 (fixstr),
    /// 15/16 (fixarray/fixmap), and the 8/16/32-bit length jumps.
    #[test]
    fn msgpack_round_trips_every_width() {
        let long = "x".repeat(40_000);
        let values = vec![
            Value::Nil,
            Value::Bool(true),
            Value::Bool(false),
            Value::Uint(0),
            Value::Uint(127),
            Value::Uint(128),
            Value::Uint(255),
            Value::Uint(256),
            Value::Uint(65_535),
            Value::Uint(65_536),
            Value::Uint(u32::MAX as u64),
            Value::Uint(u32::MAX as u64 + 1),
            Value::Int(-1),
            Value::Int(-32),
            Value::Int(-33),
            Value::Int(-129),
            Value::Int(-32_769),
            Value::Int(i32::MIN as i64 - 1),
            Value::Str(String::new()),
            Value::Str("x".repeat(31)),
            Value::Str("x".repeat(32)),
            Value::Str("x".repeat(256)),
            Value::Str(long.clone()),
            Value::Bin(vec![0u8; 255]),
            Value::Bin(vec![0u8; 256]),
            Value::Bin(vec![7u8; 70_000]),
            Value::Array((0..15).map(Value::Uint).collect()),
            Value::Array((0..16).map(Value::Uint).collect()),
            Value::Array((0..70_000).map(Value::Uint).collect()),
            Value::Map(
                (0..16)
                    .map(|n| (Value::Uint(n), Value::Str(n.to_string())))
                    .collect(),
            ),
        ];
        for value in values {
            let bytes = value.to_vec();
            let mut at = 0;
            let back = decode(&bytes, &mut at).expect("decodes");
            assert_eq!(back, value, "round-trip");
            assert_eq!(at, bytes.len(), "consumed exactly the object");
        }
    }

    /// A negative int and a positive one both read as numbers, but only the
    /// non-negative one is a `UInteger` — the spec types the entry header and
    /// most keys that way, and a decoder that let a negative through would make
    /// a mark land on line `usize::MAX`.
    #[test]
    fn as_u64_rejects_negative() {
        assert_eq!(Value::Int(-1).as_u64(), None);
        assert_eq!(Value::Int(7).as_u64(), Some(7));
        assert_eq!(Value::Uint(7).as_i64(), Some(7));
    }

    fn entry(kind: EntryKind, timestamp: u64) -> Entry {
        Entry { timestamp, kind }
    }

    /// Every entry type zmax writes has to survive the four-object framing and
    /// come back as the same model — this is the whole contract `:wshada` and
    /// `:rshada` rest on.
    #[test]
    fn entries_round_trip_through_a_file() {
        let entries = vec![
            entry(
                EntryKind::Header {
                    generator: "zmax".into(),
                    version: "1.2.3".into(),
                    encoding: "utf-8".into(),
                    max_kbyte: 10,
                    pid: 42,
                },
                100,
            ),
            entry(
                EntryKind::SearchPattern(SearchPattern {
                    pattern: "needle".into(),
                    last_used: true,
                    backward: true,
                    offset: -3,
                    ..SearchPattern::default()
                }),
                101,
            ),
            entry(EntryKind::SubString("replacement".into()), 102),
            entry(
                EntryKind::History {
                    kind: HISTORY_CMD,
                    line: "wq".into(),
                    sep: None,
                },
                103,
            ),
            entry(
                EntryKind::History {
                    kind: HISTORY_SEARCH,
                    line: "needle".into(),
                    sep: Some(b'/'),
                },
                104,
            ),
            entry(
                EntryKind::Register(Register {
                    name: 'a',
                    kind: 1,
                    width: 0,
                    contents: vec!["one".into(), "two".into(), "three".into()],
                    unnamed: false,
                    value_lines: vec![2, 1],
                }),
                105,
            ),
            entry(
                EntryKind::Mark {
                    global: true,
                    name: 'A',
                    pos: Position {
                        file: PathBuf::from("/tmp/a.rs"),
                        line: 12,
                        col: 3,
                    },
                },
                106,
            ),
            entry(
                EntryKind::Mark {
                    global: false,
                    name: 'a',
                    pos: Position {
                        file: PathBuf::from("/tmp/b.rs"),
                        line: 1,
                        col: 0,
                    },
                },
                107,
            ),
            entry(
                EntryKind::Jump(Position {
                    file: PathBuf::from("/tmp/c.rs"),
                    line: 5,
                    col: 6,
                }),
                108,
            ),
            entry(
                EntryKind::BufferList(vec![Position {
                    file: PathBuf::from("/tmp/d.rs"),
                    line: 9,
                    col: 0,
                }]),
                109,
            ),
        ];
        let bytes = encode_file(&entries, 10);
        let (back, error) = decode_file(&bytes);
        assert_eq!(error, None, "a file we wrote must decode cleanly");
        assert_eq!(back, entries);
        // `entry_type` is the cheap path merges take; it has to agree with the
        // type `to_object` frames the entry with, or a merge would compare two
        // different kinds of entry as if they were the same one.
        for entry in &entries {
            assert_eq!(
                entry.kind.entry_type(),
                entry.kind.to_object().0,
                "{:?}",
                entry.kind
            );
        }
    }

    /// A global mark's name is its identity: moving `'A` to another file must
    /// replace the entry the file already holds, not sit beside it — two `'A`
    /// entries and `:rshada!` lands on whichever one is read last. A local mark
    /// is the opposite: `'a` in two files is two marks.
    #[test]
    fn merging_moves_a_global_mark_but_keeps_local_marks_per_file() {
        let mark = |global: bool, name: char, file: &str, ts: u64| {
            entry(
                EntryKind::Mark {
                    global,
                    name,
                    pos: Position {
                        file: PathBuf::from(file),
                        line: 1,
                        col: 0,
                    },
                },
                ts,
            )
        };
        let merged = merge(
            vec![mark(true, 'A', "/old.rs", 10)],
            vec![mark(true, 'A', "/new.rs", 20)],
        );
        assert_eq!(merged, vec![mark(true, 'A', "/new.rs", 20)]);

        let merged = merge(
            vec![mark(false, 'a', "/old.rs", 10)],
            vec![mark(false, 'a', "/new.rs", 20)],
        );
        assert_eq!(merged.len(), 2, "one `'a` per file");
    }

    /// A mark map with no `n` key means `"` (34), and no `l`/`c` means line 1
    /// column 0 — the documented defaults, which is how nvim writes the common
    /// case, so misreading them would put every nvim-written mark on line 0.
    #[test]
    fn position_defaults_match_the_spec() {
        let data = Value::Map(vec![pair("f", file_value(std::path::Path::new("/tmp/x")))]);
        let kind = EntryKind::from_object(TYPE_LOCAL_MARK, data);
        assert_eq!(
            kind,
            EntryKind::Mark {
                global: false,
                name: '"',
                pos: Position {
                    file: PathBuf::from("/tmp/x"),
                    line: 1,
                    col: 0,
                },
            }
        );
    }

    /// 'shada' `sN` skips an over-size item, and `s0` saves nothing at all —
    /// except the header and the buffer list, which options.txt exempts.
    #[test]
    fn shada_s_limit_skips_oversize_items() {
        let big = entry(
            EntryKind::Register(Register {
                name: 'a',
                kind: 0,
                width: 0,
                contents: vec!["x".repeat(3000)],
                unnamed: false,
                value_lines: Vec::new(),
            }),
            1,
        );
        let header = entry(
            EntryKind::Header {
                generator: "zmax".into(),
                version: "0".into(),
                encoding: "utf-8".into(),
                max_kbyte: 1,
                pid: 1,
            },
            1,
        );
        let (kept, _) = decode_file(&encode_file(&[header.clone(), big.clone()], 1));
        assert_eq!(kept, vec![header.clone()], "3000 bytes > 1 KiB");
        let (kept, _) = decode_file(&encode_file(&[header.clone(), big.clone()], 10));
        assert_eq!(kept.len(), 2, "under the cap it is written");
        let (kept, _) = decode_file(&encode_file(&[header.clone(), big], 0));
        assert_eq!(kept, vec![header], "s0 saves nothing but the exempt items");
    }

    /// A truncated file is a critical error (starting.txt:1356): everything
    /// before it is still returned, the rest is skipped. Losing that would turn
    /// a half-written shada into a hard `:rshada` failure.
    #[test]
    fn truncated_file_keeps_what_came_before() {
        let good = entry(
            EntryKind::History {
                kind: HISTORY_CMD,
                line: "w".into(),
                sep: None,
            },
            1,
        );
        let mut bytes = encode_file(&[good.clone()], 10);
        let complete = bytes.len();
        bytes.extend_from_slice(&encode_file(&[good.clone()], 10));
        bytes.truncate(complete + 3);
        let (entries, error) = decode_file(&bytes);
        assert_eq!(entries, vec![good]);
        assert!(error.is_some(), "the truncated tail is reported");
    }

    /// An entry type this build does not model is kept verbatim, so a merge
    /// never strips what nvim wrote (variables, type 6, are the live example).
    #[test]
    fn unknown_entries_survive_a_round_trip_and_a_merge() {
        let variable = entry(
            EntryKind::Unknown {
                typ: 6,
                data: Value::Array(vec![Value::Bin(b"KEEPTHIS".to_vec()), Value::Uint(7)]),
            },
            50,
        );
        let (back, error) = decode_file(&encode_file(&[variable.clone()], 10));
        assert_eq!(error, None);
        assert_eq!(back, vec![variable.clone()]);
        let merged = merge(back, Vec::new());
        assert_eq!(merged, vec![variable]);
    }

    /// The merge rule: same thing on both sides → newer timestamp wins; a thing
    /// only one side has is kept. A merge that let the older register win would
    /// silently undo this session's yank on the next `:wshada`.
    #[test]
    fn merge_prefers_the_newer_entry_and_keeps_the_rest() {
        let reg = |name: char, text: &str, ts: u64| {
            entry(
                EntryKind::Register(Register {
                    name,
                    kind: 0,
                    width: 0,
                    contents: vec![text.to_string()],
                    unnamed: false,
                    value_lines: Vec::new(),
                }),
                ts,
            )
        };
        let old = vec![reg('a', "old", 10), reg('b', "only-old", 10)];
        let new = vec![reg('a', "new", 20), reg('c', "only-new", 20)];
        let merged = merge(old, new);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0], reg('a', "new", 20), "newer wins");
        assert!(merged.contains(&reg('b', "only-old", 10)), "old-only kept");
        assert!(merged.contains(&reg('c', "only-new", 20)), "new-only kept");

        // The other direction: an old entry with the newer timestamp wins, which
        // is what makes a second nvim/zmax instance's later write survive.
        let merged = merge(
            vec![reg('a', "old-but-newer", 30)],
            vec![reg('a', "new", 20)],
        );
        assert_eq!(merged, vec![reg('a', "old-but-newer", 30)]);
    }
}
