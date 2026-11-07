//! World data 1.21.5 → 1.21.4: chunks (heightmaps went from NBT to an id list, paletted
//! containers lost their long-array length prefix), block entities (names and sign text were
//! JSON strings before 1.21.5), tags and registry data.

use super::data_1_21_4 as d;
use super::items::{Items, Registries};
use super::text;
use crate::proto::buf::{DecodeError, Reader, Result, Writer};
use crate::proto::nbt::{self, Tag};

const BLOCKS_PER_SECTION: usize = 4096;
const BIOMES_PER_SECTION: usize = 64;

fn err(msg: &str) -> DecodeError {
    DecodeError::Other(msg.to_string())
}

fn bits_for(count: u32) -> u32 {
    32 - (count.max(2) - 1).leading_zeros()
}

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

fn packed_len(bits: u32, count: usize) -> usize {
    if bits == 0 {
        return 0;
    }
    count.div_ceil(64 / bits as usize)
}

fn read_longs(r: &mut Reader, n: usize) -> Result<Vec<i64>> {
    (0..n).map(|_| r.i64()).collect()
}

const HEIGHTMAPS: &[&str] = &["WORLD_SURFACE_WG", "WORLD_SURFACE", "OCEAN_FLOOR_WG", "OCEAN_FLOOR", "MOTION_BLOCKING", "MOTION_BLOCKING_NO_LEAVES"];

pub struct World;

impl World {
    /// 1.21.5 block palette → 1.21.4 (length-prefixed data, remapped states).
    fn block_palette(items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let bits = u32::from(r.u8()?);
        if bits == 0 {
            let state = r.varint()?;
            w.u8(0);
            w.varint(items.block_state(state));
            w.varint(0);
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
            let longs = read_longs(r, packed_len(bits.max(4), BLOCKS_PER_SECTION))?;
            w.long_array(&longs);
            return Ok(());
        }
        // Direct palette: global state ids, remapped and repacked at the old width.
        let longs = read_longs(r, packed_len(bits, BLOCKS_PER_SECTION))?;
        let values = unpack(&longs, bits, BLOCKS_PER_SECTION);
        let mapped: Vec<u32> = values.iter().map(|v| items.block_state(*v as i32) as u32).collect();
        let obits = bits_for(d::OLD_BLOCK_STATE_COUNT);
        w.u8(obits as u8);
        w.long_array(&pack(&mapped, obits));
        Ok(())
    }

    /// Biome palette: ids are synced registry ids (unchanged); only the framing differs.
    fn biome_palette(r: &mut Reader, w: &mut Writer) -> Result<()> {
        let bits = r.u8()?;
        w.u8(bits);
        if bits == 0 {
            w.varint(r.varint()?);
            w.varint(0);
            return Ok(());
        }
        if bits <= 3 {
            let n = Items::copy_varint(r, w)?;
            for _ in 0..n.max(0) {
                w.varint(r.varint()?);
            }
        }
        let longs = read_longs(r, packed_len(u32::from(bits), BIOMES_PER_SECTION))?;
        w.long_array(&longs);
        Ok(())
    }

    fn section(items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        w.i16(r.i16()?);
        Self::block_palette(items, r, w)?;
        Self::biome_palette(r, w)
    }

    pub fn level_chunk_with_light(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len() + 512);
        w.i32(r.i32()?);
        w.i32(r.i32()?);
        // Heightmaps: (type id, longs)[] → NBT compound keyed by name.
        let n = r.varint()?;
        let mut entries = Vec::new();
        for _ in 0..n.max(0) {
            let ty = r.varint()?;
            let data = r.long_array()?;
            if let Some(name) = HEIGHTMAPS.get(ty.max(0) as usize) {
                entries.push((name.to_string(), Tag::LongArray(data)));
            }
        }
        nbt::write(&mut w, &Tag::Compound(entries));
        let data = r.byte_array()?;
        let mut sr = Reader::new(data);
        let mut sections = Writer::with_capacity(data.len() + 64);
        while !sr.is_empty() {
            Self::section(items, &mut sr, &mut sections)?;
        }
        w.byte_array(&sections.buf);
        let n = r.varint()?;
        let mut body = Writer::new();
        let mut kept = 0;
        for _ in 0..n.max(0) {
            let xz = r.u8()?;
            let y = r.i16()?;
            let ty = r.varint()?;
            let mut tag_w = Writer::new();
            Self::block_entity_nbt(ty, &mut r, &mut tag_w)?;
            if let Some(old) = Self::block_entity_type(ty) {
                body.u8(xz);
                body.i16(y);
                body.varint(old);
                body.raw(&tag_w.buf);
                kept += 1;
            }
        }
        w.varint(kept);
        w.raw(&body.buf);
        w.raw(r.remaining()); // light data: same format
        Ok(w.into_inner())
    }

    /// CHUNKS_BIOMES: per chunk, a byte array of biome palettes.
    pub fn chunks_biomes(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len() + 64);
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            w.i64(r.i64()?);
            let data = r.byte_array()?;
            let mut br = Reader::new(data);
            let mut out = Writer::new();
            while !br.is_empty() {
                Self::biome_palette(&mut br, &mut out)?;
            }
            w.byte_array(&out.buf);
        }
        Ok(w.into_inner())
    }

    fn block_entity_type(ty: i32) -> Option<i32> {
        let name = d::NEW_BLOCK_ENTITY_NAMES.get(ty.max(0) as usize)?;
        d::OLD_BLOCK_ENTITY_NAMES.iter().position(|n| n == name).map(|p| p as i32)
    }

    /// Block entity NBT: CustomName and sign lines back to JSON strings.
    fn block_entity_nbt(ty: i32, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let Some(mut tag) = nbt::read_optional(r)? else {
            w.u8(0);
            return Ok(());
        };
        let name = d::NEW_BLOCK_ENTITY_NAMES.get(ty.max(0) as usize).copied().unwrap_or("");
        if name == "sign" || name == "hanging_sign" {
            for side in ["front_text", "back_text"] {
                if let Some(t) = tag.get_mut(side) {
                    for key in ["messages", "filtered_messages"] {
                        if let Some(Tag::List(lines)) = t.get(key).cloned() {
                            t.insert(key, Tag::List(lines.iter().map(|l| Tag::String(text::to_json(l))).collect()));
                        }
                    }
                }
            }
        }
        if let Some(n) = tag.get("CustomName").cloned() {
            tag.insert("CustomName", Tag::String(text::to_json(&n)));
        }
        nbt::write(w, &tag);
        Ok(())
    }

    /// BLOCK_ENTITY_DATA. `None` = the old client has no such block entity.
    pub fn block_entity_data(payload: &[u8]) -> Result<Option<Vec<u8>>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        w.i64(r.i64()?);
        let ty = r.varint()?;
        let Some(old) = Self::block_entity_type(ty) else { return Ok(None) };
        w.varint(old);
        Self::block_entity_nbt(ty, &mut r, &mut w)?;
        Ok(Some(w.into_inner()))
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
        w.u8(r.u8()?);
        w.u8(r.u8()?);
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

    /// UPDATE_TAGS: remap ids, drop tags of registries 1.21.4 lacks, rename one block tag.
    pub fn update_tags(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let n = r.varint()?;
        let mut kept = 0;
        let mut body = Writer::new();
        for _ in 0..n.max(0) {
            let registry = r.string()?.to_string();
            let key = registry.strip_prefix("minecraft:").unwrap_or(&registry).to_string();
            let tags = r.varint()?;
            if REMOVED_REGISTRIES.contains(&key.as_str()) {
                for _ in 0..tags.max(0) {
                    r.string()?;
                    r.list(|r| r.varint())?;
                }
                continue;
            }
            body.string(&registry);
            body.varint(tags);
            for _ in 0..tags.max(0) {
                let mut name = r.string()?.to_string();
                if key == "block" && name.trim_start_matches("minecraft:") == "dry_vegetation_may_place_on" {
                    name = "minecraft:dead_bush_may_place_on".into();
                }
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
            }
            kept += 1;
        }
        let mut w = Writer::new();
        w.varint(kept);
        w.raw(&body.buf);
        Ok(w.into_inner())
    }

    /// REGISTRY_DATA. Records the entry keys (for hashing and entity variants); returns `None`
    /// when the whole registry is new in 1.21.5.
    pub fn registry_data(regs: &mut Registries, payload: &[u8]) -> Result<Option<Vec<u8>>> {
        let mut r = Reader::new(payload);
        let registry = r.string()?.to_string();
        let key = registry.strip_prefix("minecraft:").unwrap_or(&registry).to_string();
        let n = r.varint()?;
        let mut keys = Vec::new();
        let mut w = Writer::new();
        w.string(&registry);
        w.varint(n);
        for _ in 0..n.max(0) {
            let entry = r.string()?.to_string();
            w.string(&entry);
            keys.push(entry);
            let present = r.bool()?;
            w.bool(present);
            if present {
                let mut tag = nbt::read(&mut r)?;
                Self::registry_entry(&key, &mut tag);
                nbt::write(&mut w, &tag);
            }
        }
        regs.set(&key, keys);
        if REMOVED_REGISTRIES.contains(&key.as_str()) {
            return Ok(None);
        }
        Ok(Some(w.into_inner()))
    }

    fn registry_entry(key: &str, tag: &mut Tag) {
        match key {
            "trim_pattern" => tag.insert("template_item", Tag::String("stone".into())),
            "trim_material" => tag.insert("ingredient", Tag::String("stone".into())),
            "enchantment" => {
                if let Some(Tag::List(slots)) = tag.get_mut("slots") {
                    slots.retain(|s| s.as_str().map(|s| s.trim_start_matches("minecraft:")) != Some("saddle"));
                }
            }
            "wolf_variant" => {
                let assets = tag.remove("assets").unwrap_or_else(Tag::compound);
                for (from, to) in [("wild", "wild_texture"), ("tame", "tame_texture"), ("angry", "angry_texture")] {
                    if let Some(v) = assets.get(from).cloned() {
                        tag.insert(to, v);
                    }
                }
                tag.insert("biomes", Tag::List(Vec::new()));
            }
            _ => {}
        }
        for field in ["description", "title", "author"] {
            if let Some(t) = tag.get_mut(field) {
                text::downgrade(t);
            }
        }
    }
}

/// Registries that exist in 1.21.5 but not 1.21.4.
pub const REMOVED_REGISTRIES: &[&str] = &[
    "pig_variant", "cow_variant", "frog_variant", "cat_variant", "chicken_variant", "test_environment", "test_instance",
    "wolf_sound_variant",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translate::v1_21_4::items::Dir;

    #[test]
    fn palette_gets_length_prefix() {
        let items = Items::new(Dir::ToClient);
        let mut s = Writer::new();
        s.i16(4096);
        s.u8(4); // indirect, 4 bits
        s.varint(2);
        s.varint(0);
        s.varint(1);
        for _ in 0..256 {
            s.i64(0x1111_1111_1111_1111);
        }
        s.u8(0); // biomes: single
        s.varint(3);
        let mut out = Writer::new();
        World::section(&items, &mut Reader::new(&s.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        assert_eq!(r.i16().unwrap(), 4096);
        assert_eq!(r.u8().unwrap(), 4);
        assert_eq!(r.varint().unwrap(), 2);
        r.varint().unwrap();
        r.varint().unwrap();
        assert_eq!(r.long_array().unwrap().len(), 256);
        assert_eq!(r.u8().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), 3);
        assert_eq!(r.varint().unwrap(), 0); // empty data array
        assert!(r.is_empty());
    }

    #[test]
    fn direct_palette_is_remapped() {
        let items = Items::new(Dir::ToClient);
        let bits = bits_for(d::NEW_BLOCK_STATE_COUNT);
        let last = d::NEW_BLOCK_STATE_COUNT - 1;
        let mut values = vec![1u32; BLOCKS_PER_SECTION];
        values[0] = last;
        let mut s = Writer::new();
        s.i16(4096);
        s.u8(bits as u8);
        for l in pack(&values, bits) {
            s.i64(l);
        }
        s.u8(0);
        s.varint(0);
        let mut out = Writer::new();
        World::section(&items, &mut Reader::new(&s.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        r.i16().unwrap();
        let obits = u32::from(r.u8().unwrap());
        let decoded = unpack(&r.long_array().unwrap(), obits, BLOCKS_PER_SECTION);
        assert_eq!(decoded[0], d::BLOCK_STATES[last as usize]);
        assert_eq!(decoded[1], d::BLOCK_STATES[1]);
    }

    #[test]
    fn heightmaps_become_nbt() {
        let items = Items::new(Dir::ToClient);
        let mut p = Writer::new();
        p.i32(1);
        p.i32(2);
        p.varint(1);
        p.varint(4); // MOTION_BLOCKING
        p.long_array(&[7, 8]);
        p.byte_array(&[]);
        p.varint(0);
        p.raw(&[9, 9]); // light (opaque)
        let out = World::level_chunk_with_light(&items, &p.buf).unwrap();
        let mut r = Reader::new(&out);
        r.i32().unwrap();
        r.i32().unwrap();
        let tag = nbt::read(&mut r).unwrap();
        assert_eq!(tag.get("MOTION_BLOCKING"), Some(&Tag::LongArray(vec![7, 8])));
        assert_eq!(r.byte_array().unwrap().len(), 0);
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.remaining(), &[9, 9]);
    }

    #[test]
    fn sign_text_becomes_json() {
        let sign = d::NEW_BLOCK_ENTITY_NAMES.iter().position(|n| *n == "sign").unwrap() as i32;
        let tag = Tag::Compound(vec![("front_text".into(), Tag::Compound(vec![("messages".into(), Tag::List(vec![Tag::String("hi".into())]))]))]);
        let mut p = Writer::new();
        p.i64(0);
        p.varint(sign);
        nbt::write(&mut p, &tag);
        let out = World::block_entity_data(&p.buf).unwrap().unwrap();
        let mut r = Reader::new(&out);
        r.i64().unwrap();
        r.varint().unwrap();
        let t = nbt::read(&mut r).unwrap();
        let line = t.get("front_text").and_then(|f| f.get("messages")).and_then(Tag::as_list).unwrap()[0].clone();
        assert_eq!(line, Tag::String("\"hi\"".into()));
    }
}
