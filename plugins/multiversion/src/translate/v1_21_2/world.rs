//! World data 1.21.4 → 1.21.2: chunks, block updates, block events, tags, registry data.
//!
//! Chunk layout of both versions (before 1.21.5): heightmaps are an NBT compound, sections have
//! no fluid count, and every paletted container carries a varint long-array length.

use super::data_1_21_2 as d;
use super::items::Items;
use crate::proto::buf::{DecodeError, Reader, Result, Writer};
use crate::proto::nbt::{self, Tag};

const BLOCKS_PER_SECTION: usize = 4096;

fn err(msg: &str) -> DecodeError {
    DecodeError::Other(msg.to_string())
}

fn bits_for(count: u32) -> u32 {
    32 - (count - 1).leading_zeros()
}

/// Global palette widths: 1.21.4 has 27866 states, 1.21.2 27318 (both 15 bits).
fn new_block_bits() -> u32 {
    bits_for(d::NEW_BLOCK_STATE_COUNT)
}
fn old_block_bits() -> u32 {
    bits_for(d::OLD_BLOCK_STATE_COUNT)
}

/// Unpacks a vanilla "no values span longs" compact array.
fn unpack(longs: &[i64], bits: u32, count: usize) -> Vec<u32> {
    let per_long = 64 / bits as usize;
    let mask = (1u64 << bits) - 1;
    let mut out = Vec::with_capacity(count);
    'outer: for l in longs {
        let v = *l as u64;
        for i in 0..per_long {
            if out.len() == count {
                break 'outer;
            }
            out.push(((v >> (i as u32 * bits)) & mask) as u32);
        }
    }
    out.resize(count, 0);
    out
}

fn pack(values: &[u32], bits: u32) -> Vec<i64> {
    let per_long = 64 / bits as usize;
    let mut out = Vec::with_capacity(values.len().div_ceil(per_long));
    for chunk in values.chunks(per_long) {
        let mut v: u64 = 0;
        for (i, x) in chunk.iter().enumerate() {
            v |= (u64::from(*x) & ((1u64 << bits) - 1)) << (i as u32 * bits);
        }
        out.push(v as i64);
    }
    out
}

pub struct World;

impl World {
    fn section(items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        w.i16(r.i16()?); // non-air block count
        Self::block_palette(items, r, w)?;
        Self::copy_palette(r, w, 3)?; // biomes
        Ok(())
    }

    fn block_palette(items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let bits = u32::from(r.u8()?);
        if bits == 0 {
            let state = r.varint()?;
            w.u8(0);
            w.varint(items.block_state(state));
            w.long_array(&r.long_array()?); // always empty, kept as sent
            return Ok(());
        }
        if bits <= 8 {
            // Indirect palette: remap entries, keep the packed indexes.
            let n = r.varint()?;
            if n < 0 {
                return Err(err("negative palette length"));
            }
            w.u8(bits as u8);
            w.varint(n);
            for _ in 0..n {
                w.varint(items.block_state(r.varint()?));
            }
            w.long_array(&r.long_array()?);
            return Ok(());
        }
        // Direct palette: global ids; remap and repack in the old width.
        let longs = r.long_array()?;
        let values = unpack(&longs, bits, BLOCKS_PER_SECTION);
        let mapped: Vec<u32> = values.iter().map(|v| items.block_state(*v as i32) as u32).collect();
        let obits = old_block_bits().max(if bits == new_block_bits() { 0 } else { bits });
        w.u8(obits as u8);
        w.long_array(&pack(&mapped, obits));
        Ok(())
    }

    fn copy_palette(r: &mut Reader, w: &mut Writer, max_indirect_bits: u32) -> Result<()> {
        let bits = r.u8()?;
        w.u8(bits);
        if bits == 0 {
            w.varint(r.varint()?);
        } else if u32::from(bits) <= max_indirect_bits {
            let n = r.varint()?;
            w.varint(n);
            for _ in 0..n.max(0) {
                w.varint(r.varint()?);
            }
        }
        w.long_array(&r.long_array()?);
        Ok(())
    }

    pub fn level_chunk_with_light(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.i32(r.i32()?);
        w.i32(r.i32()?);
        Items::copy_nbt(&mut r, &mut w)?; // heightmaps
        let data = r.byte_array()?;
        let mut sr = Reader::new(data);
        let mut sections = Writer::with_capacity(data.len());
        while !sr.is_empty() {
            Self::section(items, &mut sr, &mut sections)?;
        }
        w.byte_array(&sections.buf);
        // Block entities: same type registry in both versions.
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            w.u8(r.u8()?);
            w.i16(r.i16()?);
            Items::copy_varint(&mut r, &mut w)?;
            Items::copy_nbt(&mut r, &mut w)?;
        }
        w.raw(r.remaining()); // light data: unchanged
        Ok(w.into_inner())
    }

    pub fn block_update(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        w.i64(r.i64()?);
        w.varint(items.block_state(r.varint()?));
        Ok(w.into_inner())
    }

    pub fn section_blocks_update(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        w.i64(r.i64()?);
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            let v = r.varlong()?;
            let state = (v >> 12) as i32;
            w.varlong((i64::from(items.block_state(state)) << 12) | (v & 0xfff));
        }
        Ok(w.into_inner())
    }

    pub fn block_event(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        w.i64(r.i64()?);
        w.raw(r.bytes(2)?);
        w.varint(items.block(r.varint()?));
        Ok(w.into_inner())
    }

    pub fn level_event(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        let event = r.i32()?;
        w.i32(event);
        w.i64(r.i64()?);
        let data = r.i32()?;
        // 2001 = block break particles + sound: the data is a block state id.
        w.i32(if event == 2001 { items.block_state(data) } else { data });
        w.raw(r.remaining());
        Ok(w.into_inner())
    }

    /// UPDATE_TAGS: remap block/item/entity ids and add the tags 1.21.4 removed but the 1.21.2
    /// client still requires (`tall_flowers`, `flowers`), as empty tags.
    pub fn update_tags(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let n = r.varint()?;
        let mut w = Writer::new();
        w.varint(n);
        for _ in 0..n.max(0) {
            let registry = r.string()?.to_string();
            let key = registry.strip_prefix("minecraft:").unwrap_or(&registry).to_string();
            let count = r.varint()?;
            let mut body = Writer::new();
            let mut names = Vec::new();
            for _ in 0..count.max(0) {
                let name = r.string()?.to_string();
                body.string(&name);
                let ids = r.list(|r| r.varint())?;
                body.varint(ids.len() as i32);
                for id in ids {
                    body.varint(match key.as_str() {
                        "block" => items.block(id),
                        "item" => items.item(id),
                        "entity_type" => items.entity(id),
                        _ => id,
                    });
                }
                names.push(name);
            }
            let wanted: &[&str] = match key.as_str() {
                "item" => &["minecraft:tall_flowers", "minecraft:flowers"],
                "block" => &["minecraft:tall_flowers"],
                _ => &[],
            };
            let missing: Vec<&str> = wanted.iter().copied().filter(|t| !names.iter().any(|n| n == t)).collect();
            w.string(&registry);
            w.varint(count + missing.len() as i32);
            w.raw(&body.buf);
            for tag in missing {
                w.string(tag);
                w.varint(0);
            }
        }
        Ok(w.into_inner())
    }

    /// REGISTRY_DATA (configuration): rewrites biome music and trim materials.
    pub fn registry_data(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let registry = r.string()?.to_string();
        let key = registry.strip_prefix("minecraft:").unwrap_or(&registry).to_string();
        let mut w = Writer::new();
        w.string(&registry);
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            let entry = r.string()?.to_string();
            w.string(&entry);
            let present = r.bool()?;
            w.bool(present);
            if present {
                let mut tag = nbt::read(&mut r)?;
                Self::registry_entry(&key, &entry, &mut tag);
                nbt::write(&mut w, &tag);
            }
        }
        Ok(w.into_inner())
    }

    fn registry_entry(key: &str, entry: &str, tag: &mut Tag) {
        match key {
            "worldgen/biome" => {
                let Some(effects) = tag.get_mut("effects") else { return };
                // 1.21.4: weighted list of {data, weight}; 1.21.2: one music entry.
                if let Some(Tag::List(list)) = effects.get("music") {
                    match list.first().and_then(|m| m.get("data")).cloned() {
                        Some(data) => effects.insert("music", data),
                        None => {
                            effects.remove("music");
                        }
                    }
                }
                for field in ["mood_sound", "additions_sound", "music"] {
                    if let Some(sub) = effects.get_mut(field) {
                        fix_sound(sub, "sound");
                    }
                }
                fix_sound(effects, "ambient_sound");
            }
            "trim_material" => {
                tag.insert("item_model_index", Tag::Float(super::items::item_model_index(entry)));
                if tag.get("asset_name").and_then(Tag::as_str) == Some("resin") {
                    tag.insert("asset_name", Tag::String("gold".into()));
                }
            }
            "jukebox_song" => fix_sound(tag, "sound_event"),
            _ => {}
        }
    }
}

/// Replaces a sound the old client lacks (by name) with an empty one.
fn fix_sound(tag: &mut Tag, field: &str) {
    let missing = |s: &str| {
        let vanilla = !s.contains(':') || s.starts_with("minecraft:");
        vanilla && !d::OLD_SOUND_NAMES.contains(&s.strip_prefix("minecraft:").unwrap_or(s))
    };
    match tag.get_mut(field) {
        Some(Tag::String(s)) if missing(s) => *s = "minecraft:intentionally_empty".into(),
        Some(t @ Tag::Compound(_)) => {
            if let Some(Tag::String(s)) = t.get_mut("sound_id") {
                if missing(s) {
                    *s = "minecraft:intentionally_empty".into();
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::items::Dir;

    #[test]
    fn global_bits_are_15() {
        assert_eq!(new_block_bits(), 15);
        assert_eq!(old_block_bits(), 15);
    }

    #[test]
    fn direct_palette_is_remapped() {
        let items = Items::new(Dir::ToClient);
        let last = d::NEW_BLOCK_STATE_COUNT - 1;
        let mut values = vec![1u32; BLOCKS_PER_SECTION];
        values[0] = last;
        let mut s = Writer::new();
        s.i16(4096);
        s.u8(15);
        s.long_array(&pack(&values, 15));
        s.u8(0); // biomes single value
        s.varint(3);
        s.varint(0);
        let mut out = Writer::new();
        World::section(&items, &mut Reader::new(&s.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        assert_eq!(r.i16().unwrap(), 4096);
        assert_eq!(r.u8().unwrap(), 15);
        let decoded = unpack(&r.long_array().unwrap(), 15, BLOCKS_PER_SECTION);
        assert_eq!(decoded[0], d::BLOCK_STATES[last as usize]);
        assert_eq!(decoded[1], 1);
        assert_eq!(r.u8().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), 3);
        assert_eq!(r.varint().unwrap(), 0);
        assert!(r.is_empty());
    }

    #[test]
    fn indirect_palette_keeps_data() {
        let items = Items::new(Dir::ToClient);
        let mut s = Writer::new();
        s.i16(10);
        s.u8(4);
        s.varint(2);
        s.varint(0);
        s.varint((d::NEW_BLOCK_STATE_COUNT - 1) as i32);
        s.long_array(&[0x1111; 256]);
        s.u8(1); // biomes: indirect, 1 bit
        s.varint(2);
        s.varint(4);
        s.varint(5);
        s.long_array(&[7]);
        let mut out = Writer::new();
        World::section(&items, &mut Reader::new(&s.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        r.i16().unwrap();
        assert_eq!(r.u8().unwrap(), 4);
        assert_eq!(r.varint().unwrap(), 2);
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), d::BLOCK_STATES[(d::NEW_BLOCK_STATE_COUNT - 1) as usize] as i32);
        assert_eq!(r.long_array().unwrap().len(), 256);
        assert_eq!(r.u8().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), 2);
        r.varint().unwrap();
        r.varint().unwrap();
        assert_eq!(r.long_array().unwrap(), vec![7]);
        assert!(r.is_empty());
    }

    #[test]
    fn biome_music_is_unwrapped() {
        let mut tag = Tag::Compound(vec![(
            "effects".into(),
            Tag::Compound(vec![(
                "music".into(),
                Tag::List(vec![Tag::Compound(vec![
                    ("data".into(), Tag::Compound(vec![("sound".into(), Tag::String("minecraft:music.game".into()))])),
                    ("weight".into(), Tag::Int(1)),
                ])]),
            )]),
        )]);
        World::registry_entry("worldgen/biome", "minecraft:plains", &mut tag);
        let music = tag.get("effects").unwrap().get("music").unwrap();
        assert_eq!(music.get("sound").and_then(Tag::as_str), Some("minecraft:music.game"));
    }

    #[test]
    fn missing_flower_tags_are_added() {
        let items = Items::new(Dir::ToClient);
        let mut p = Writer::new();
        p.varint(1);
        p.string("minecraft:item");
        p.varint(1);
        p.string("minecraft:flowers");
        p.varint(0);
        let out = World::update_tags(&items, &p.buf).unwrap();
        let mut r = Reader::new(&out);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.string().unwrap(), "minecraft:item");
        assert_eq!(r.varint().unwrap(), 2);
        assert_eq!(r.string().unwrap(), "minecraft:flowers");
        r.varint().unwrap();
        assert_eq!(r.string().unwrap(), "minecraft:tall_flowers");
        assert_eq!(r.varint().unwrap(), 0);
        assert!(r.is_empty());
    }
}
