//! World data 1.21.9 → 1.21.7: chunks, block updates, block entities, tags, registry data.
//!
//! Chunk, light and heightmap layouts are identical in both versions (1.21.5 format, bit sets as
//! long arrays); only ids change. Block entity types the old client lacks (shelf, copper golem
//! statue) are dropped.

use super::data as d;
use super::items::Items;
use crate::proto::buf::{DecodeError, Reader, Result, Writer};
use crate::proto::nbt::{self, Tag};

const BLOCKS_PER_SECTION: usize = 4096;
const BIOMES_PER_SECTION: usize = 64;

fn err(msg: &str) -> DecodeError {
    DecodeError::Other(msg.to_string())
}

fn bits_for(count: u32) -> u32 {
    32 - (count - 1).leading_zeros()
}

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

/// 1.21.7 block entity type for a 1.21.9 one, or `None` if the old client has no such type.
pub fn block_entity_type(ty: i32) -> Option<i32> {
    let name = d::NEW_BLOCK_ENTITY_NAMES.get(usize::try_from(ty).ok()?)?;
    d::OLD_BLOCK_ENTITY_NAMES.iter().position(|n| n == name).map(|p| p as i32)
}

pub struct World;

impl World {
    fn section(items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        w.i16(r.i16()?); // non-air blocks
        w.i16(r.i16()?); // fluids
        Self::block_palette(items, r, w)?;
        Self::copy_palette(r, w, BIOMES_PER_SECTION, 3)
    }

    /// Longs a 1.21.5+ paletted container occupies (no length prefix on the wire).
    fn packed_len(bits: u32, count: usize) -> usize {
        if bits == 0 {
            return 0;
        }
        count.div_ceil(64 / bits as usize)
    }

    fn read_longs(r: &mut Reader, n: usize) -> Result<Vec<i64>> {
        (0..n).map(|_| r.i64()).collect()
    }

    fn write_longs(w: &mut Writer, longs: &[i64]) {
        for l in longs {
            w.i64(*l);
        }
    }

    fn block_palette(items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let bits = u32::from(r.u8()?);
        if bits == 0 {
            let state = r.varint()?;
            w.u8(0);
            w.varint(items.block_state(state));
            return Ok(());
        }
        if bits <= 8 {
            let n = r.varint()?;
            if n < 0 {
                return Err(err("negative palette length"));
            }
            w.u8(bits as u8);
            w.varint(n);
            for _ in 0..n {
                w.varint(items.block_state(r.varint()?));
            }
            let longs = Self::read_longs(r, Self::packed_len(bits.max(4), BLOCKS_PER_SECTION))?;
            Self::write_longs(w, &longs);
            return Ok(());
        }
        // Direct palette: global ids, repacked in the old width.
        let nbits = new_block_bits().max(bits);
        let longs = Self::read_longs(r, Self::packed_len(nbits, BLOCKS_PER_SECTION))?;
        let mapped: Vec<u32> = unpack(&longs, nbits, BLOCKS_PER_SECTION).iter().map(|v| items.block_state(*v as i32) as u32).collect();
        let obits = old_block_bits();
        w.u8(obits as u8);
        Self::write_longs(w, &pack(&mapped, obits));
        Ok(())
    }

    fn copy_palette(r: &mut Reader, w: &mut Writer, count: usize, max_indirect_bits: u32) -> Result<()> {
        let bits = r.u8()?;
        w.u8(bits);
        if bits == 0 {
            w.varint(r.varint()?);
            return Ok(());
        }
        if u32::from(bits) <= max_indirect_bits {
            let n = r.varint()?;
            w.varint(n);
            for _ in 0..n.max(0) {
                w.varint(r.varint()?);
            }
        }
        let longs = Self::read_longs(r, Self::packed_len(u32::from(bits), count))?;
        Self::write_longs(w, &longs);
        Ok(())
    }

    pub fn level_chunk_with_light(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.i32(r.i32()?);
        w.i32(r.i32()?);
        let n = r.varint()?; // heightmaps
        w.varint(n);
        for _ in 0..n.max(0) {
            w.varint(r.varint()?);
            w.long_array(&r.long_array()?);
        }
        let data = r.byte_array()?;
        let mut sr = Reader::new(data);
        let mut sections = Writer::with_capacity(data.len());
        while !sr.is_empty() {
            Self::section(items, &mut sr, &mut sections)?;
        }
        w.byte_array(&sections.buf);
        let n = r.varint()?; // block entities
        let mut body = Writer::new();
        let mut kept = 0;
        for _ in 0..n.max(0) {
            let xz = r.u8()?;
            let y = r.i16()?;
            let ty = r.varint()?;
            let mut tag = Writer::new();
            Items::copy_nbt(&mut r, &mut tag)?;
            if let Some(t) = block_entity_type(ty) {
                body.u8(xz);
                body.i16(y);
                body.varint(t);
                body.raw(&tag.buf);
                kept += 1;
            }
        }
        w.varint(kept);
        w.raw(&body.buf);
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
        let n = r.varint()?;
        w.varint(n);
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
        w.i32(if event == 2001 { items.block_state(data) } else { data });
        w.raw(r.remaining());
        Ok(w.into_inner())
    }

    /// BLOCK_ENTITY_DATA, or `None` when the old client has no such block entity type.
    pub fn block_entity_data(payload: &[u8]) -> Result<Option<Vec<u8>>> {
        let mut r = Reader::new(payload);
        let pos = r.i64()?;
        let Some(ty) = block_entity_type(r.varint()?) else { return Ok(None) };
        let mut w = Writer::new();
        w.i64(pos);
        w.varint(ty);
        w.raw(r.remaining());
        Ok(Some(w.into_inner()))
    }

    /// UPDATE_TAGS: remap block, item and entity type ids.
    pub fn update_tags(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            let registry = r.string()?;
            w.string(registry);
            let key = registry.strip_prefix("minecraft:").unwrap_or(registry);
            let tags = Items::copy_varint(&mut r, &mut w)?;
            for _ in 0..tags.max(0) {
                w.string(r.string()?);
                let ids = r.list(|r| r.varint())?;
                w.varint(ids.len() as i32);
                for id in ids {
                    w.varint(match key {
                        "block" => items.block(id),
                        "item" => items.item(id),
                        "entity_type" => items.entity(id),
                        _ => id,
                    });
                }
            }
        }
        Ok(w.into_inner())
    }

    /// REGISTRY_DATA. Also returns the dimension coordinate scales when this is the dimension
    /// type registry (needed for world border centers).
    pub fn registry_data(payload: &[u8]) -> Result<(Vec<u8>, Option<Vec<f64>>)> {
        let mut r = Reader::new(payload);
        let registry = r.string()?.to_string();
        let key = registry.strip_prefix("minecraft:").unwrap_or(&registry).to_string();
        let mut w = Writer::with_capacity(payload.len());
        w.string(&registry);
        let n = Items::copy_varint(&mut r, &mut w)?;
        let mut scales = Vec::new();
        for _ in 0..n.max(0) {
            let entry = r.string()?;
            w.string(entry);
            let present = Items::copy_bool(&mut r, &mut w)?;
            let mut scale = if entry.trim_start_matches("minecraft:") == "the_nether" { 8.0 } else { 1.0 };
            if present {
                let mut tag = nbt::read(&mut r)?;
                match key.as_str() {
                    "dimension_type" => {
                        if let Some(s) = tag.get("coordinate_scale").and_then(tag_f64) {
                            scale = s;
                        }
                    }
                    "worldgen/biome" => {
                        if let Some(effects) = tag.get_mut("effects") {
                            for field in ["mood_sound", "additions_sound", "music"] {
                                if let Some(t) = effects.get_mut(field) {
                                    fix_sound(t, "sound");
                                }
                            }
                            fix_sound(effects, "ambient_sound");
                        }
                    }
                    "jukebox_song" => fix_sound(&mut tag, "sound_event"),
                    _ => {}
                }
                nbt::write(&mut w, &tag);
            }
            scales.push(scale);
        }
        Ok((w.into_inner(), (key == "dimension_type").then_some(scales)))
    }
}

fn tag_f64(t: &Tag) -> Option<f64> {
    match t {
        Tag::Double(v) => Some(*v),
        Tag::Float(v) => Some(f64::from(*v)),
        Tag::Int(v) => Some(f64::from(*v)),
        Tag::Long(v) => Some(*v as f64),
        _ => None,
    }
}

/// Sound references the old client lacks would fail registry loading: replace them with an
/// inline, silent sound event (as ViaBackwards does).
fn fix_sound(parent: &mut Tag, field: &str) {
    let known = |id: &str| d::OLD_SOUND_NAMES.contains(&id.strip_prefix("minecraft:").unwrap_or(id));
    let silent = || {
        let mut t = Tag::compound();
        t.insert("sound_id", Tag::String("minecraft:intentionally_empty".into()));
        t
    };
    if matches!(parent.get(field), Some(Tag::String(id)) if !known(id)) {
        parent.insert(field, silent());
        return;
    }
    if let Some(sound @ Tag::Compound(_)) = parent.get_mut(field) {
        if let Some(Tag::String(id)) = sound.get_mut("sound_id") {
            if !known(id) {
                *id = "minecraft:intentionally_empty".into();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::items::Dir;
    use super::*;

    #[test]
    fn direct_palette_is_remapped() {
        let items = Items::new(Dir::ToClient);
        let last = d::NEW_BLOCK_STATE_COUNT - 1;
        let bits = new_block_bits();
        let mut values = vec![1u32; BLOCKS_PER_SECTION];
        values[0] = last;
        let mut s = Writer::new();
        s.i16(4096);
        s.i16(0);
        s.u8(bits as u8);
        for l in pack(&values, bits) {
            s.i64(l);
        }
        s.u8(0);
        s.varint(3);
        let mut out = Writer::new();
        World::section(&items, &mut Reader::new(&s.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        r.i16().unwrap();
        r.i16().unwrap();
        let obits = u32::from(r.u8().unwrap());
        assert_eq!(obits, old_block_bits());
        let longs: Vec<i64> = (0..World::packed_len(obits, BLOCKS_PER_SECTION)).map(|_| r.i64().unwrap()).collect();
        let decoded = unpack(&longs, obits, BLOCKS_PER_SECTION);
        assert_eq!(decoded[0], d::BLOCK_STATES[last as usize]);
        assert!(decoded[0] < d::OLD_BLOCK_STATE_COUNT);
        assert_eq!(decoded[1], 1);
        assert_eq!(r.u8().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), 3);
        assert!(r.is_empty());
    }

    #[test]
    fn new_block_entities_are_dropped() {
        let shelf = d::NEW_BLOCK_ENTITY_NAMES.iter().position(|n| *n == "shelf").unwrap() as i32;
        assert_eq!(block_entity_type(shelf), None);
        let chest = d::NEW_BLOCK_ENTITY_NAMES.iter().position(|n| *n == "chest").unwrap() as i32;
        assert!(block_entity_type(chest).is_some());
        let mut p = Writer::new();
        p.i64(0);
        p.varint(shelf);
        p.u8(0);
        assert!(World::block_entity_data(&p.buf).unwrap().is_none());
    }

    #[test]
    fn dimension_scales_are_collected() {
        let mut p = Writer::new();
        p.string("minecraft:dimension_type");
        p.varint(2);
        p.string("minecraft:overworld");
        p.bool(false);
        p.string("minecraft:the_nether");
        p.bool(true);
        let mut tag = Tag::compound();
        tag.insert("coordinate_scale", Tag::Double(8.0));
        nbt::write(&mut p, &tag);
        let (out, scales) = World::registry_data(&p.buf).unwrap();
        assert_eq!(out, p.buf);
        assert_eq!(scales, Some(vec![1.0, 8.0]));
    }
}
