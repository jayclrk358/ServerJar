//! World data 1.21.6 → 1.21.5: chunks, block updates, block entities, tags, registry data.
//!
//! Chunk and light layouts are identical in both versions (1.21.5 sections: block count, block
//! palette, biome palette; palettes without a length prefix; light masks as long arrays), so only
//! block state ids need remapping.

use super::super::data_1_21_5 as d;
use super::items::{copy_nbt, err, Items};
use crate::proto::buf::{Reader, Result, Writer};

const BLOCKS_PER_SECTION: usize = 4096;
const BIOMES_PER_SECTION: usize = 64;

fn bits_for(count: u32) -> u32 {
    32 - (count - 1).leading_zeros()
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

/// Number of longs a 1.21.5+ paletted container occupies (no length prefix on the wire).
fn packed_len(bits: u32, count: usize) -> usize {
    if bits == 0 {
        return 0;
    }
    count.div_ceil(64 / bits as usize)
}

fn copy_longs(r: &mut Reader, w: &mut Writer, n: usize) -> Result<()> {
    w.raw(r.bytes(n * 8)?);
    Ok(())
}

pub struct World;

impl World {
    fn section(items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        w.i16(r.i16()?); // non-air block count
        Self::block_palette(items, r, w)?;
        Self::copy_biome_palette(r, w)
    }

    fn block_palette(items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let bits = u32::from(r.u8()?);
        if bits == 0 {
            w.u8(0);
            w.varint(items.block_state(r.varint()?));
            return Ok(());
        }
        if bits <= 8 {
            // Indirect palette: remap entries, keep the packed indexes as they are.
            let n = r.varint()?;
            if n < 0 {
                return Err(err("negative palette length"));
            }
            w.u8(bits as u8);
            w.varint(n);
            for _ in 0..n {
                w.varint(items.block_state(r.varint()?));
            }
            return copy_longs(r, w, packed_len(bits.max(4), BLOCKS_PER_SECTION));
        }
        // Direct palette: global ids at the global width of each version.
        let nbits = bits_for(d::NEW_BLOCK_STATE_COUNT).max(bits);
        let longs: Vec<i64> = (0..packed_len(nbits, BLOCKS_PER_SECTION)).map(|_| r.i64()).collect::<Result<_>>()?;
        let mapped: Vec<u32> = unpack(&longs, nbits, BLOCKS_PER_SECTION).iter().map(|v| items.block_state(*v as i32) as u32).collect();
        let obits = bits_for(d::OLD_BLOCK_STATE_COUNT);
        w.u8(obits as u8);
        for l in pack(&mapped, obits) {
            w.i64(l);
        }
        Ok(())
    }

    fn copy_biome_palette(r: &mut Reader, w: &mut Writer) -> Result<()> {
        let bits = r.u8()?;
        w.u8(bits);
        if bits == 0 {
            w.varint(r.varint()?);
            return Ok(());
        }
        if bits <= 3 {
            let n = r.varint()?;
            w.varint(n);
            for _ in 0..n.max(0) {
                w.varint(r.varint()?);
            }
        }
        copy_longs(r, w, packed_len(u32::from(bits), BIOMES_PER_SECTION))
    }

    pub fn level_chunk_with_light(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.raw(r.bytes(8)?); // chunk x, z
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
        w.varint(n);
        for _ in 0..n.max(0) {
            w.u8(r.u8()?);
            w.i16(r.i16()?);
            w.varint(r.varint()?);
            copy_nbt(&mut r, &mut w)?;
        }
        w.raw(r.remaining()); // light data: same layout in both versions
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
        let mut w = Writer::with_capacity(payload.len());
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

    /// UPDATE_TAGS: remap ids, drop ids and registries the old client lacks, rename one tag.
    pub fn update_tags(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let n = r.varint()?;
        let mut kept = 0;
        let mut body = Writer::with_capacity(payload.len());
        for _ in 0..n.max(0) {
            let registry = r.string()?;
            let key = registry.strip_prefix("minecraft:").unwrap_or(registry);
            let tags = r.varint()?;
            if REMOVED_REGISTRIES.contains(&key) {
                for _ in 0..tags.max(0) {
                    r.string()?;
                    r.list(|r| r.varint())?;
                }
                continue;
            }
            body.string(registry);
            body.varint(tags);
            for _ in 0..tags.max(0) {
                let name = r.string()?;
                if key == "block" && name.trim_start_matches("minecraft:") == "triggers_ambient_desert_dry_vegetation_block_sounds" {
                    body.string("minecraft:plays_ambient_desert_block_sounds");
                } else {
                    body.string(name);
                }
                let ids = r.list(|r| r.varint())?;
                let mapped: Vec<i32> = ids
                    .into_iter()
                    .filter_map(|id| match key {
                        "block" => items.block_mapped(id).then(|| items.block(id)),
                        "item" => items.item_mapped(id).then(|| items.item(id)),
                        "entity_type" => Some(items.entity(id)),
                        _ => Some(id),
                    })
                    .collect();
                body.varint(mapped.len() as i32);
                for id in mapped {
                    body.varint(id);
                }
            }
            kept += 1;
        }
        let mut w = Writer::with_capacity(body.buf.len() + 5);
        w.varint(kept);
        w.raw(&body.buf);
        Ok(w.into_inner())
    }

    /// REGISTRY_DATA: `None` when the whole registry must be dropped for the old client.
    pub fn registry_data(payload: &[u8]) -> Result<Option<Vec<u8>>> {
        let mut r = Reader::new(payload);
        let registry = r.string()?;
        let key = registry.strip_prefix("minecraft:").unwrap_or(registry);
        if REMOVED_REGISTRIES.contains(&key) {
            return Ok(None);
        }
        // Entries are identical apart from new optional fields (e.g. cloud_height) that the old
        // client's codecs ignore.
        Ok(Some(payload.to_vec()))
    }
}

/// Registries that exist in 1.21.6 but not 1.21.5.
pub const REMOVED_REGISTRIES: &[&str] = &["dialog"];

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::items::Dir;

    #[test]
    fn pack_unpack_roundtrip() {
        let values: Vec<u32> = (0..4096).map(|i| (i * 7919) % 27000).collect();
        let packed = pack(&values, 15);
        assert_eq!(unpack(&packed, 15, 4096), values);
    }

    #[test]
    fn section_remaps_indirect_palette_and_keeps_layout() {
        let items = Items::new(Dir::ToClient);
        let last = d::NEW_BLOCK_STATE_COUNT as i32 - 1;
        let mut s = Writer::new();
        s.i16(10);
        s.u8(4);
        s.varint(2);
        s.varint(1);
        s.varint(last);
        for _ in 0..256 {
            s.i64(0x1111);
        }
        s.u8(0);
        s.varint(7);
        let mut out = Writer::new();
        World::section(&items, &mut Reader::new(&s.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        assert_eq!(r.i16().unwrap(), 10);
        assert_eq!(r.u8().unwrap(), 4);
        assert_eq!(r.varint().unwrap(), 2);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), d::BLOCK_STATES[last as usize] as i32);
        for _ in 0..256 {
            assert_eq!(r.i64().unwrap(), 0x1111);
        }
        assert_eq!(r.u8().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), 7);
        assert!(r.is_empty());
    }

    #[test]
    fn dialog_registry_and_tags_are_dropped() {
        let mut p = Writer::new();
        p.string("minecraft:dialog");
        p.varint(0);
        assert!(World::registry_data(&p.buf).unwrap().is_none());

        let items = Items::new(Dir::ToClient);
        let mut t = Writer::new();
        t.varint(2);
        t.string("minecraft:dialog");
        t.varint(1);
        t.string("minecraft:pause_screen_additions");
        t.varint(1);
        t.varint(0);
        t.string("minecraft:block");
        t.varint(1);
        t.string("minecraft:triggers_ambient_desert_dry_vegetation_block_sounds");
        t.varint(1);
        t.varint(1);
        let out = World::update_tags(&items, &t.buf).unwrap();
        let mut r = Reader::new(&out);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.string().unwrap(), "minecraft:block");
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.string().unwrap(), "minecraft:plays_ambient_desert_block_sounds");
        assert_eq!(r.list(|r| r.varint()).unwrap(), vec![1]);
    }
}
