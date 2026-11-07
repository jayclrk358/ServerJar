//! Data component hashes, as sent by 1.21.5+ clients in container clicks ("hashed stacks").
//!
//! Vanilla hashes a component by encoding it with its codec into a small tagged byte format and
//! taking CRC32C of the result; maps and lists hash their children separately and embed only the
//! child hashes (maps sorted by unsigned key hash, then value hash). This mirrors the vanilla
//! item hashing exactly so the hashes match what the server computes.

/// A value in the codec's data model, ready to be hashed.
#[derive(Debug, Clone, PartialEq)]
pub enum Hv {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    Str(String),
    Bool(bool),
    Bytes(Vec<i8>),
    Ints(Vec<i32>),
    Longs(Vec<i64>),
    List(Vec<Hv>),
    Map(Vec<(Hv, Hv)>),
    /// A value whose encoder writes nothing (an absent optional); hashes to CRC32C of nothing.
    Nothing,
}

const TAG_MAP_START: u8 = 2;
const TAG_MAP_END: u8 = 3;
const TAG_LIST_START: u8 = 4;
const TAG_LIST_END: u8 = 5;
const TAG_BYTE: u8 = 6;
const TAG_SHORT: u8 = 7;
const TAG_INT: u8 = 8;
const TAG_LONG: u8 = 9;
const TAG_FLOAT: u8 = 10;
const TAG_DOUBLE: u8 = 11;
const TAG_STRING: u8 = 12;
const TAG_BOOLEAN: u8 = 13;
const TAG_BYTE_ARRAY_START: u8 = 14;
const TAG_BYTE_ARRAY_END: u8 = 15;
const TAG_INT_ARRAY_START: u8 = 16;
const TAG_INT_ARRAY_END: u8 = 17;
const TAG_LONG_ARRAY_START: u8 = 18;
const TAG_LONG_ARRAY_END: u8 = 19;

/// Hash used for components this plugin can't encode (a fixed sentinel value).
pub const UNKNOWN_HASH: i32 = 399_825_415;

fn crc32c_table() -> &'static [u32; 256] {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, slot) in t.iter_mut().enumerate() {
            let mut crc = i as u32;
            for _ in 0..8 {
                crc = if crc & 1 == 1 { (crc >> 1) ^ 0x82F6_3B78 } else { crc >> 1 };
            }
            *slot = crc;
        }
        t
    })
}

pub fn crc32c(data: &[u8]) -> i32 {
    let t = crc32c_table();
    let mut crc: u32 = !0;
    for b in data {
        crc = (crc >> 8) ^ t[((crc ^ u32::from(*b)) & 0xff) as usize];
    }
    !crc as i32
}

fn encode(v: &Hv, out: &mut Vec<u8>) {
    match v {
        Hv::Byte(b) => {
            out.push(TAG_BYTE);
            out.push(*b as u8);
        }
        Hv::Short(s) => {
            out.push(TAG_SHORT);
            out.extend_from_slice(&s.to_le_bytes());
        }
        Hv::Int(i) => {
            out.push(TAG_INT);
            out.extend_from_slice(&i.to_le_bytes());
        }
        Hv::Long(l) => {
            out.push(TAG_LONG);
            out.extend_from_slice(&l.to_le_bytes());
        }
        Hv::Float(f) => {
            out.push(TAG_FLOAT);
            out.extend_from_slice(&f.to_bits().to_le_bytes());
        }
        Hv::Double(d) => {
            out.push(TAG_DOUBLE);
            out.extend_from_slice(&d.to_bits().to_le_bytes());
        }
        Hv::Str(s) => {
            // Java strings: length in UTF-16 units, then each unit little-endian.
            let units: Vec<u16> = s.encode_utf16().collect();
            out.push(TAG_STRING);
            out.extend_from_slice(&(units.len() as i32).to_le_bytes());
            for u in units {
                out.extend_from_slice(&u.to_le_bytes());
            }
        }
        Hv::Bool(b) => {
            out.push(TAG_BOOLEAN);
            out.push(u8::from(*b));
        }
        Hv::Bytes(a) => {
            out.push(TAG_BYTE_ARRAY_START);
            out.extend(a.iter().map(|b| *b as u8));
            out.push(TAG_BYTE_ARRAY_END);
        }
        Hv::Ints(a) => {
            out.push(TAG_INT_ARRAY_START);
            for i in a {
                out.extend_from_slice(&i.to_le_bytes());
            }
            out.push(TAG_INT_ARRAY_END);
        }
        Hv::Longs(a) => {
            out.push(TAG_LONG_ARRAY_START);
            for l in a {
                out.extend_from_slice(&l.to_le_bytes());
            }
            out.push(TAG_LONG_ARRAY_END);
        }
        Hv::List(items) => {
            out.push(TAG_LIST_START);
            for item in items {
                out.extend_from_slice(&hash(item).to_le_bytes());
            }
            out.push(TAG_LIST_END);
        }
        Hv::Map(entries) => {
            let mut hashed: Vec<(u32, u32)> = entries.iter().map(|(k, v)| (hash(k) as u32, hash(v) as u32)).collect();
            hashed.sort_unstable();
            out.push(TAG_MAP_START);
            for (k, v) in hashed {
                out.extend_from_slice(&k.to_le_bytes());
                out.extend_from_slice(&v.to_le_bytes());
            }
            out.push(TAG_MAP_END);
        }
        Hv::Nothing => {}
    }
}

/// The hash vanilla computes for a value.
pub fn hash(v: &Hv) -> i32 {
    let mut out = Vec::new();
    encode(v, &mut out);
    crc32c(&out)
}

/// Builder for map values with vanilla's optional-field conventions.
#[derive(Default)]
pub struct MapB(pub Vec<(Hv, Hv)>);

impl MapB {
    pub fn new() -> Self {
        Self(Vec::new())
    }
    pub fn put(mut self, key: &str, value: Hv) -> Self {
        self.0.push((Hv::Str(key.to_string()), value));
        self
    }
    /// Writes the value unless it is `None`.
    pub fn opt(self, key: &str, value: Option<Hv>) -> Self {
        match value {
            Some(v) => self.put(key, v),
            None => self,
        }
    }
    /// Writes the value unless it equals the codec default.
    pub fn def(self, key: &str, value: Hv, default: &Hv) -> Self {
        if &value == default { self } else { self.put(key, value) }
    }
    pub fn build(self) -> Hv {
        Hv::Map(self.0)
    }
}

/// Identifiers are hashed in their full `namespace:path` form.
pub fn ident(s: &str) -> Hv {
    Hv::Str(full_key(s))
}

pub fn full_key(s: &str) -> String {
    match s.find(':') {
        None => format!("minecraft:{s}"),
        Some(0) => format!("minecraft{s}"),
        Some(_) => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Expected values computed with the vanilla algorithm.
    #[test]
    fn use_cooldown() {
        let v = MapB::new().put("seconds", Hv::Float(0.5)).put("cooldown_group", ident(":stick")).build();
        assert_eq!(hash(&v), -524_579_578);
    }

    #[test]
    fn compound_tag() {
        let v = MapB::new()
            .put("id", Hv::Int(1))
            .put("type", Hv::Str("minecraft:stick".into()))
            .put("wow", Hv::Map(vec![]))
            .put("list", Hv::List(vec![Hv::Float(0.3)]))
            .put("emptylist", Hv::List(vec![]))
            .build();
        assert_eq!(hash(&v), -859_008_863);
    }

    #[test]
    fn arrays() {
        let v = MapB::new().put("floats", Hv::List(vec![Hv::Float(1.0)])).build();
        assert_eq!(hash(&v), 2_007_278_159);
    }

    #[test]
    fn nested_map() {
        let target = MapB::new().put("dimension", ident("wow")).put("pos", Hv::Ints(vec![5, 6, 7])).build();
        let v = MapB::new().put("target", target).build();
        assert_eq!(hash(&v), 1_876_575_779);
    }

    #[test]
    fn banner_pattern_holder() {
        let pattern = MapB::new().put("asset_id", ident("bricks")).put("translation_key", Hv::Str("some_key".into())).build();
        let v = Hv::List(vec![MapB::new().put("pattern", pattern).put("color", Hv::Str("cyan".into())).build()]);
        assert_eq!(hash(&v), -2_006_113_844);
    }

    #[test]
    fn container() {
        let stone = || MapB::new().put("id", ident("stone")).put("count", Hv::Int(1)).build();
        let one = Hv::List(vec![MapB::new().put("slot", Hv::Int(0)).put("item", stone()).build()]);
        assert_eq!(hash(&one), 231_516_551);
        let skipped_empty = Hv::List(vec![MapB::new().put("slot", Hv::Int(1)).put("item", stone()).build()]);
        assert_eq!(hash(&skipped_empty), 1_506_540_737);
    }

    #[test]
    fn item_with_components() {
        let comps = MapB::new().put("minecraft:enchantment_glint_override", Hv::Bool(true)).build();
        let v = MapB::new().put("id", ident("stone")).put("count", Hv::Int(1)).put("components", comps).build();
        assert_eq!(hash(&v), -95_388_252);
    }
}
