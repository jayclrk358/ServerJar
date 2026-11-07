//! World data 26.1 → 1.21.11: chunks, block updates, tags, registry data.

use super::super::data_1_21_11 as d;
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

/// Registries that exist in 26.1 but not in 1.21.11.
pub const REMOVED_REGISTRIES: &[&str] = &["world_clock", "cat_sound_variant", "cow_sound_variant", "pig_sound_variant", "chicken_sound_variant"];

/// Block tags renamed in 26.1 (26.1 name → 1.21.11 name).
const RENAMED_BLOCK_TAGS: &[(&str, &str)] = &[
    ("supports_dry_vegetation", "dry_vegetation_may_place_on"),
    ("supports_bamboo", "bamboo_plantable_on"),
    ("supports_small_dripleaf", "small_dripleaf_placeable"),
    ("supports_big_dripleaf", "big_dripleaf_placeable"),
    ("overrides_mushroom_light_requirement", "mushroom_grow_block"),
    ("support_override_snow_layer", "snow_layer_can_survive_on"),
    ("cannot_support_snow_layer", "snow_layer_cannot_survive_on"),
];

/// Item tag the 1.21.11 client needs but 26.1 no longer sends.
const DYEABLE_ITEMS: &[&str] = &["leather_helmet", "leather_chestplate", "leather_leggings", "leather_boots", "leather_horse_armor", "wolf_armor"];

pub struct World;

impl World {
    /// 26.1 section (non-air count, fluid count, palettes) → 1.21.11 (no fluid count).
    fn section(items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        w.i16(r.i16()?); // non-air block count
        r.i16()?; // fluid count: 26.1 only
        Self::block_palette(items, r, w)?;
        Self::copy_palette(r, w, BIOMES_PER_SECTION, 3)?;
        Ok(())
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
        // Direct palette: global ids, repacked at the old width.
        let nbits = bits_for(d::NEW_BLOCK_STATE_COUNT).max(bits);
        let longs = Self::read_longs(r, Self::packed_len(nbits, BLOCKS_PER_SECTION))?;
        let values = unpack(&longs, nbits, BLOCKS_PER_SECTION);
        let mapped: Vec<u32> = values.iter().map(|v| items.block_state(*v as i32) as u32).collect();
        let obits = bits_for(d::OLD_BLOCK_STATE_COUNT);
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
        w.raw(r.remaining()); // block entities (same registry) + light data (same layout)
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

    /// UPDATE_TAGS: remap ids, drop removed registries, rename block tags, add `dyeable`.
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
            let mut entries: Vec<(String, Vec<i32>)> = Vec::with_capacity(tags.max(0) as usize);
            for _ in 0..tags.max(0) {
                let mut name = r.string()?.to_string();
                if key == "block" {
                    let bare = name.strip_prefix("minecraft:").unwrap_or(&name);
                    if let Some((_, old)) = RENAMED_BLOCK_TAGS.iter().find(|(new, _)| *new == bare) {
                        name = format!("minecraft:{old}");
                    }
                }
                let ids = r.list(|r| r.varint())?;
                let ids = ids
                    .into_iter()
                    .map(|id| match key.as_str() {
                        "block" => items.block(id),
                        "item" => items.item(id),
                        "entity_type" => items.entity(id),
                        _ => id,
                    })
                    .collect();
                entries.push((name, ids));
            }
            if key == "item" && !entries.iter().any(|(n, _)| n.strip_prefix("minecraft:").unwrap_or(n) == "dyeable") {
                let ids = DYEABLE_ITEMS
                    .iter()
                    .filter_map(|n| d::OLD_ITEM_NAMES.iter().position(|o| o == n).map(|i| i as i32))
                    .collect();
                entries.push(("minecraft:dyeable".into(), ids));
            }
            body.string(&registry);
            body.varint(entries.len() as i32);
            for (name, ids) in entries {
                body.string(&name);
                body.varint(ids.len() as i32);
                for id in ids {
                    body.varint(id);
                }
            }
            kept += 1;
        }
        let mut w = Writer::new();
        w.varint(kept);
        w.raw(&body.buf);
        Ok(w.into_inner())
    }

    /// The registry name and entry keys of a REGISTRY_DATA packet (used to learn clock ids).
    pub fn registry_keys(payload: &[u8]) -> Result<(String, Vec<String>)> {
        let mut r = Reader::new(payload);
        let registry = r.string()?.to_string();
        let n = r.varint()?;
        let mut keys = Vec::with_capacity(n.max(0) as usize);
        for _ in 0..n.max(0) {
            keys.push(r.string()?.to_string());
            if r.bool()? {
                nbt::skip(&mut r)?;
            }
        }
        Ok((registry, keys))
    }

    /// REGISTRY_DATA: `None` when the whole registry must be dropped for the old client.
    pub fn registry_data(payload: &[u8]) -> Result<Option<Vec<u8>>> {
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
            let present = r.bool()?;
            w.bool(present);
            if present {
                let mut tag = nbt::read(&mut r)?;
                Self::registry_entry(&key, &mut tag);
                nbt::write(&mut w, &tag);
            }
        }
        Ok(Some(w.into_inner()))
    }

    fn remove_env_attributes(tag: Option<&mut Tag>) {
        if let Some(Tag::Compound(attrs)) = tag {
            attrs.retain(|(name, _)| {
                let bare = name.strip_prefix("minecraft:").unwrap_or(name);
                !d::REMOVED_ENVIRONMENT_ATTRIBUTES.iter().any(|r| r.strip_prefix("minecraft:").unwrap_or(r) == bare)
            });
        }
    }

    /// "entity/frog/frog_warm" → "entity/frog/warm_frog" (1.21.11 naming).
    fn swap_affix(entity: &str, tag: &mut Tag) {
        if let Some(Tag::String(asset)) = tag.get_mut("asset_id") {
            let prefix = format!("{entity}_");
            if asset.contains(&prefix) {
                *asset = format!("{}_{entity}", asset.replace(&prefix, ""));
            }
        }
    }

    fn registry_entry(key: &str, tag: &mut Tag) {
        match key {
            "dimension_type" => {
                Self::remove_env_attributes(tag.get_mut("attributes"));
                tag.remove("default_clock");
                tag.remove("has_ender_dragon_fight");
            }
            "timeline" => {
                Self::remove_env_attributes(tag.get_mut("tracks"));
                tag.remove("clock");
            }
            "worldgen/biome" => Self::remove_env_attributes(tag.get_mut("attributes")),
            "wolf_sound_variant" => {
                tag.remove("baby_sounds");
                if let Some(Tag::Compound(adult)) = tag.remove("adult_sounds") {
                    for (k, v) in adult {
                        if k != "step_sound" {
                            tag.insert(&k, v);
                        }
                    }
                }
            }
            "wolf_variant" => {
                tag.remove("baby_assets");
            }
            "frog_variant" | "chicken_variant" | "cow_variant" | "pig_variant" => {
                let entity = key.trim_end_matches("_variant");
                Self::swap_affix(entity, tag);
                tag.remove("baby_asset_id");
            }
            "cat_variant" => {
                if let Some(Tag::String(asset)) = tag.get_mut("asset_id") {
                    *asset = asset.replace("cat_", "");
                }
                tag.remove("baby_asset_id");
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::items::Dir;

    #[test]
    fn section_drops_fluid_count_and_remaps_single_value() {
        let items = Items::new(Dir::ToClient);
        let mut s = Writer::new();
        s.i16(4096).i16(7).u8(0).varint(1).u8(0).varint(3);
        let mut out = Writer::new();
        World::section(&items, &mut Reader::new(&s.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        assert_eq!(r.i16().unwrap(), 4096);
        assert_eq!(r.u8().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), d::BLOCK_STATES[1] as i32);
        assert_eq!(r.u8().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), 3);
        assert!(r.is_empty());
    }

    #[test]
    fn direct_palette_roundtrip_width() {
        let items = Items::new(Dir::ToClient);
        let nbits = bits_for(d::NEW_BLOCK_STATE_COUNT);
        let last = d::NEW_BLOCK_STATE_COUNT - 1;
        let mut values = vec![1u32; BLOCKS_PER_SECTION];
        values[10] = last;
        let mut s = Writer::new();
        s.i16(4096).i16(0).u8(nbits as u8);
        for l in pack(&values, nbits) {
            s.i64(l);
        }
        s.u8(0).varint(0);
        let mut out = Writer::new();
        World::section(&items, &mut Reader::new(&s.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        r.i16().unwrap();
        let obits = u32::from(r.u8().unwrap());
        assert_eq!(obits, bits_for(d::OLD_BLOCK_STATE_COUNT));
        let longs: Vec<i64> = (0..World::packed_len(obits, BLOCKS_PER_SECTION)).map(|_| r.i64().unwrap()).collect();
        let decoded = unpack(&longs, obits, BLOCKS_PER_SECTION);
        assert_eq!(decoded[10], d::BLOCK_STATES[last as usize]);
        assert_eq!(decoded[0], d::BLOCK_STATES[1]);
    }

    #[test]
    fn tags_are_renamed_and_dyeable_added() {
        let items = Items::new(Dir::ToClient);
        let mut p = Writer::new();
        p.varint(3);
        p.string("minecraft:block").varint(1).string("minecraft:supports_bamboo").varint(0);
        p.string("minecraft:item").varint(0);
        p.string("minecraft:cat_sound_variant").varint(1).string("x").varint(0);
        let out = World::update_tags(&items, &p.buf).unwrap();
        let mut r = Reader::new(&out);
        assert_eq!(r.varint().unwrap(), 2);
        assert_eq!(r.string().unwrap(), "minecraft:block");
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.string().unwrap(), "minecraft:bamboo_plantable_on");
        r.list(|r| r.varint()).unwrap();
        assert_eq!(r.string().unwrap(), "minecraft:item");
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.string().unwrap(), "minecraft:dyeable");
        assert_eq!(r.list(|r| r.varint()).unwrap().len(), DYEABLE_ITEMS.len());
        assert!(r.is_empty());
    }

    #[test]
    fn registry_rewrites() {
        assert!(World::registry_data(&{
            let mut p = Writer::new();
            p.string("minecraft:world_clock").varint(0);
            p.buf
        })
        .unwrap()
        .is_none());
        let mut frog = Tag::compound();
        frog.insert("asset_id", Tag::String("minecraft:entity/frog/frog_warm".into()));
        World::registry_entry("frog_variant", &mut frog);
        assert_eq!(frog.get("asset_id").and_then(Tag::as_str), Some("minecraft:entity/frog/warm_frog"));
        let mut cat = Tag::compound();
        cat.insert("asset_id", Tag::String("minecraft:entity/cat/cat_jellie".into()));
        World::registry_entry("cat_variant", &mut cat);
        assert_eq!(cat.get("asset_id").and_then(Tag::as_str), Some("minecraft:entity/cat/jellie"));
    }
}
