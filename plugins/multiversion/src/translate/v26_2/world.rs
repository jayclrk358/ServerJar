//! World data 26.3 → 26.2: chunks, light, block updates, block entities, tags, registry data.

use super::data_26_2 as d;
use super::items::{Dir, Items};
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

/// Global palette widths: ceil(log2(state count)). 26.3 has 35723 states (16 bits), 26.2 32366 (15).
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
    /// Translates one chunk section's block palette (26.3 → 26.2), copying the biome palette.
    fn section(items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        w.i16(r.i16()?); // non-air block count
        w.i16(r.i16()?); // fluid count
        Self::block_palette(items, r, w)?;
        Self::copy_palette(r, w, BIOMES_PER_SECTION, 3)?;
        Ok(())
    }

    /// Number of longs a 1.21.5+ paletted container occupies (no length prefix on the wire).
    fn packed_len(bits: u32, count: usize) -> usize {
        if bits == 0 {
            return 0;
        }
        let per_long = 64 / bits as usize;
        count.div_ceil(per_long)
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
        let bits = r.u8()? as u32;
        if bits == 0 {
            let state = r.varint()?;
            w.u8(0);
            w.varint(items.block_state(state));
            return Ok(());
        }
        if bits <= 8 {
            // Indirect palette: remap entries, keep the packed indexes as-is.
            let n = r.varint()?;
            if n < 0 {
                return Err(err("negative palette length"));
            }
            let mut entries = Vec::with_capacity(n as usize);
            for _ in 0..n {
                entries.push(items.block_state(r.varint()?));
            }
            let longs = Self::read_longs(r, Self::packed_len(bits.max(4), BLOCKS_PER_SECTION))?;
            w.u8(bits as u8);
            w.varint(n);
            for e in entries {
                w.varint(e);
            }
            Self::write_longs(w, &longs);
            return Ok(());
        }
        // Direct palette: every value is a global state id in 26.3 width; repack in 26.2 width.
        let nbits = new_block_bits().max(bits);
        let longs = Self::read_longs(r, Self::packed_len(nbits, BLOCKS_PER_SECTION))?;
        let values = unpack(&longs, nbits, BLOCKS_PER_SECTION);
        let mapped: Vec<u32> = values.iter().map(|v| items.block_state(*v as i32) as u32).collect();
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

    /// 26.3 bit set (varint byte length + little-endian bytes) → 26.2 long array.
    pub fn bitset_to_longs(r: &mut Reader, w: &mut Writer) -> Result<()> {
        let bytes = r.byte_array()?;
        let mut longs = Vec::with_capacity(bytes.len().div_ceil(8));
        for chunk in bytes.chunks(8) {
            let mut b = [0u8; 8];
            b[..chunk.len()].copy_from_slice(chunk);
            longs.push(i64::from_le_bytes(b));
        }
        w.long_array(&longs);
        Ok(())
    }

    fn light_data(r: &mut Reader, w: &mut Writer) -> Result<()> {
        for _ in 0..4 {
            Self::bitset_to_longs(r, w)?;
        }
        for _ in 0..2 {
            let n = r.varint()?;
            w.varint(n);
            for _ in 0..n.max(0) {
                w.byte_array(r.byte_array()?);
            }
        }
        Ok(())
    }

    pub fn level_chunk_with_light(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.i32(r.i32()?);
        w.i32(r.i32()?);
        // Heightmaps
        let n = r.varint()?;
        w.varint(n);
        for _ in 0..n.max(0) {
            w.varint(r.varint()?);
            w.long_array(&r.long_array()?);
        }
        // Sections
        let data = r.byte_array()?;
        let mut sr = Reader::new(data);
        let mut sections = Writer::with_capacity(data.len());
        while !sr.is_empty() {
            Self::section(items, &mut sr, &mut sections)?;
        }
        w.byte_array(&sections.buf);
        // Block entities
        let n = r.varint()?;
        w.varint(n);
        for _ in 0..n.max(0) {
            w.u8(r.u8()?);
            w.i16(r.i16()?);
            let ty = r.varint()?;
            w.varint(ty);
            Self::block_entity_nbt(ty, &mut r, &mut w)?;
        }
        Self::light_data(&mut r, &mut w)?;
        Ok(w.into_inner())
    }

    pub fn light_update(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.varint(r.varint()?);
        w.varint(r.varint()?);
        Self::light_data(&mut r, &mut w)?;
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
            let pos = v & 0xfff;
            w.varlong((i64::from(items.block_state(state)) << 12) | pos);
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

    pub fn block_entity_data(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        w.i64(r.i64()?);
        let ty = r.varint()?;
        w.varint(ty);
        Self::block_entity_nbt(ty, &mut r, &mut w)?;
        Ok(w.into_inner())
    }

    fn block_entity_nbt(ty: i32, r: &mut Reader, w: &mut Writer) -> Result<()> {
        const DECORATED_POT: i32 = 41;
        if ty != DECORATED_POT {
            return Items::copy_nbt(r, w);
        }
        let Some(mut tag) = nbt::read_optional(r)? else {
            w.u8(0);
            return Ok(());
        };
        // 26.3: sherds = {back:{id}, left:{id}, right:{id}, front:{id}} → 26.2: list of 4 item ids
        if let Some(Tag::Compound(_)) = tag.get("sherds") {
            let sides = tag.remove("sherds").unwrap_or_else(Tag::compound);
            let list = ["back", "left", "right", "front"]
                .iter()
                .map(|side| {
                    let id = sides.get(side).and_then(|s| s.get("id")).and_then(Tag::as_str).unwrap_or("minecraft:brick");
                    Tag::String(id.to_string())
                })
                .collect();
            tag.insert("sherds", Tag::List(list));
        }
        nbt::write(w, &tag);
        Ok(())
    }

    /// UPDATE_TAGS: remap block/item/entity ids, drop tags of removed registries, rename one tag.
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
                if key == "block" && name.trim_start_matches("minecraft:") == "convertible_to_mud" {
                    name = "minecraft:convertable_to_mud".into();
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

    /// REGISTRY_DATA: returns `None` when the whole registry must be dropped for the old client.
    pub fn registry_data(items: &Items, payload: &[u8]) -> Result<Option<Vec<u8>>> {
        let mut r = Reader::new(payload);
        let registry = r.string()?.to_string();
        let key = registry.strip_prefix("minecraft:").unwrap_or(&registry).to_string();
        if REMOVED_REGISTRIES.contains(&key.as_str()) {
            return Ok(None);
        }
        let mut w = Writer::new();
        w.string(&registry);
        let n = r.varint()?;
        w.varint(n);
        for _ in 0..n.max(0) {
            w.string(r.string()?);
            // Optional<Tag>: bool prefix, then the tag.
            let present = r.bool()?;
            w.bool(present);
            if present {
                let mut tag = nbt::read(&mut r)?;
                Self::registry_entry(items, &key, &mut tag);
                nbt::write(&mut w, &tag);
            }
        }
        Ok(Some(w.into_inner()))
    }

    fn registry_entry(_items: &Items, key: &str, tag: &mut Tag) {
        match key {
            "enchantment" => {
                // Effect definitions were reworked (number providers, match_block...). Clients
                // only need them for display; dropping them keeps the registry parseable.
                tag.remove("effects");
            }
            "trim_material" => {
                if let Some(Tag::String(palette)) = tag.remove("palette_id") {
                    let asset = palette.strip_prefix("minecraft:").unwrap_or(&palette);
                    let asset = asset.strip_prefix("trim/").unwrap_or(asset);
                    tag.insert("asset_name", Tag::String(asset.to_string()));
                }
            }
            "dimension_type" | "worldgen/biome" => {
                if let Some(Tag::Compound(attrs)) = tag.get_mut("attributes") {
                    attrs.retain(|(name, _)| !d::REMOVED_ENVIRONMENT_ATTRIBUTES.contains(&name.as_str()));
                    for (_, value) in attrs.iter_mut() {
                        if let Some(Tag::String(m)) = value.get_mut("modifier") {
                            if m == "append" || m == "overlay" {
                                *m = "override".into();
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Registries that exist in 26.3 but not 26.2.
pub const REMOVED_REGISTRIES: &[&str] = &["decorated_pot_pattern", "block_transformer", "worldgen/block_state_provider"];

#[allow(dead_code)]
pub fn dir() -> Dir {
    Dir::ToClient
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pack_unpack_roundtrip() {
        let values: Vec<u32> = (0..4096).map(|i| (i * 7919) % 30000).collect();
        for bits in [15u32, 16] {
            let packed = pack(&values, bits);
            assert_eq!(packed.len(), 1024);
            assert_eq!(unpack(&packed, bits, 4096), values);
        }
    }
    #[test]
    fn global_bits() {
        assert_eq!(new_block_bits(), 16);
        assert_eq!(old_block_bits(), 15);
    }
    #[test]
    fn direct_palette_is_repacked_to_15_bits() {
        let items = Items::new(Dir::ToClient);
        // A section whose blocks are all the last 26.3 state (only exists in 26.3) plus stone.
        let last = d::NEW_BLOCK_STATE_COUNT - 1;
        let mut values = vec![1u32; BLOCKS_PER_SECTION];
        values[0] = last;
        values[4095] = last;
        let mut section = Writer::new();
        section.i16(4096);
        section.i16(0);
        section.u8(16);
        for l in pack(&values, 16) {
            section.i64(l);
        }
        section.u8(0); // biomes: single value
        section.varint(3);
        let mut out = Writer::new();
        World::section(&items, &mut Reader::new(&section.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        assert_eq!(r.i16().unwrap(), 4096);
        assert_eq!(r.i16().unwrap(), 0);
        assert_eq!(r.u8().unwrap(), 15);
        let longs: Vec<i64> = (0..1024).map(|_| r.i64().unwrap()).collect();
        let decoded = unpack(&longs, 15, BLOCKS_PER_SECTION);
        assert_eq!(decoded[1], 1);
        assert_eq!(decoded[0], d::BLOCK_STATES[last as usize]);
        assert!(decoded[0] < d::OLD_BLOCK_STATE_COUNT);
        assert_eq!(r.u8().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), 3);
        assert!(r.is_empty());
    }
    #[test]
    fn bitset_conversion() {
        let mut w = Writer::new();
        w.byte_array(&[0b101, 0, 0, 0, 0, 0, 0, 0, 1]);
        let mut out = Writer::new();
        World::bitset_to_longs(&mut Reader::new(&w.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        assert_eq!(r.long_array().unwrap(), vec![5, 1]);
    }
}
