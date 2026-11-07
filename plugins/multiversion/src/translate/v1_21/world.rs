//! World data 1.21.2 → 1.21: chunks, block updates, block entities, tags, registry data.
//! Both versions use the 1.20.2 chunk format (length-prefixed long arrays, NBT heightmaps).

use std::collections::HashMap;

use super::data_1_21 as d;
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

fn strip_ns(s: &str) -> &str {
    s.strip_prefix("minecraft:").unwrap_or(s)
}

pub struct World;

impl World {
    fn section(items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        w.i16(r.i16()?); // non-air block count
        Self::block_palette(items, r, w)?;
        Self::biome_palette(r, w)
    }

    fn block_palette(items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let bits = u32::from(r.u8()?);
        if bits == 0 {
            w.u8(0);
            w.varint(items.block_state(r.varint()?));
            w.long_array(&r.long_array()?);
            return Ok(());
        }
        if bits <= 8 {
            w.u8(bits as u8);
            let n = r.varint()?;
            if n < 0 {
                return Err(err("negative palette length"));
            }
            w.varint(n);
            for _ in 0..n {
                w.varint(items.block_state(r.varint()?));
            }
            w.long_array(&r.long_array()?);
            return Ok(());
        }
        // direct palette: global ids, repacked in the client's width
        let longs = r.long_array()?;
        let values = unpack(&longs, bits, BLOCKS_PER_SECTION);
        let mapped: Vec<u32> = values.iter().map(|v| items.block_state(*v as i32).max(0) as u32).collect();
        let obits = bits_for(d::OLD_BLOCK_STATE_COUNT);
        w.u8(obits as u8);
        w.long_array(&pack(&mapped, obits));
        Ok(())
    }

    fn biome_palette(r: &mut Reader, w: &mut Writer) -> Result<()> {
        let bits = r.u8()?;
        w.u8(bits);
        if bits == 0 {
            w.varint(r.varint()?);
        } else if bits <= 3 {
            let n = Items::copy_varint(r, w)?;
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
        w.raw(r.bytes(8)?); // chunk x, z
        Items::copy_nbt(&mut r, &mut w)?; // heightmaps
        let data = r.byte_array()?;
        let mut sr = Reader::new(data);
        let mut sections = Writer::with_capacity(data.len());
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
            let mut tag = Writer::new();
            Items::copy_nbt(&mut r, &mut tag)?;
            if Items::block_entity_unknown(ty) {
                continue;
            }
            body.u8(xz);
            body.i16(y);
            body.varint(Items::block_entity(ty));
            body.raw(&tag.buf);
            kept += 1;
        }
        w.varint(kept);
        w.raw(&body.buf);
        w.raw(r.remaining()); // light data: same layout
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
        w.raw(r.bytes(10)?); // position, action, param
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

    /// BLOCK_ENTITY_DATA; `None` for block entity types the client does not know.
    pub fn block_entity_data(payload: &[u8]) -> Result<Option<Vec<u8>>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        w.i64(r.i64()?);
        let ty = r.varint()?;
        if Items::block_entity_unknown(ty) {
            return Ok(None);
        }
        w.varint(Items::block_entity(ty));
        w.raw(r.remaining());
        Ok(Some(w.into_inner()))
    }

    /// UPDATE_TAGS: remaps block/item/entity ids and records the server's item tags (used to resolve
    /// tag ingredients in recipes).
    pub fn update_tags(items: &Items, payload: &[u8], item_tags: &mut HashMap<String, Vec<i32>>) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            let registry = r.string()?.to_string();
            w.string(&registry);
            let key = strip_ns(&registry).to_string();
            if key == "item" {
                item_tags.clear();
            }
            let tags = Items::copy_varint(&mut r, &mut w)?;
            for _ in 0..tags.max(0) {
                let name = r.string()?.to_string();
                w.string(&name);
                let ids = r.list(|r| r.varint())?;
                if key == "item" {
                    item_tags.insert(strip_ns(&name).to_string(), ids.clone());
                }
                let mapped: Vec<i32> = ids
                    .iter()
                    .filter(|id| key != "block" || !Items::block_unknown(**id))
                    .map(|id| match key.as_str() {
                        "block" => items.block(*id),
                        "item" => items.item(*id),
                        "entity_type" => items.entity(*id),
                        _ => *id,
                    })
                    .collect();
                w.varint(mapped.len() as i32);
                for id in mapped {
                    w.varint(id);
                }
            }
        }
        Ok(w.into_inner())
    }

    /// UPDATE_ENABLED_FEATURES: bundles are a feature flag in 1.21 and always on in 1.21.2.
    pub fn enabled_features(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut flags: Vec<String> = r.list(|r| r.string().map(str::to_string))?;
        if !flags.iter().any(|f| strip_ns(f) == "bundle") {
            flags.push("minecraft:bundle".into());
        }
        let mut w = Writer::new();
        w.varint(flags.len() as i32);
        for f in &flags {
            w.string(f);
        }
        Ok(w.into_inner())
    }

    /// REGISTRY_DATA: returns `None` when the whole registry must be dropped for the old client.
    pub fn registry_data(payload: &[u8]) -> Result<Option<Vec<u8>>> {
        let mut r = Reader::new(payload);
        let registry = r.string()?.to_string();
        let key = strip_ns(&registry).to_string();
        if key == "instrument" {
            // 1.21 has a built-in instrument registry
            return Ok(None);
        }
        let mut w = Writer::with_capacity(payload.len());
        w.string(&registry);
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            w.string(r.string()?);
            let present = Items::copy_bool(&mut r, &mut w)?;
            if present {
                let mut tag = nbt::read(&mut r)?;
                Self::registry_entry(&key, &mut tag);
                nbt::write(&mut w, &tag);
            }
        }
        Ok(Some(w.into_inner()))
    }

    fn registry_entry(key: &str, tag: &mut Tag) {
        match key {
            "worldgen/biome" => {
                if let Some(options) = tag.get_mut("effects").and_then(|e| e.get_mut("particle")).and_then(|p| p.get_mut("options")) {
                    let ty = options.get("type").and_then(Tag::as_str).map(|s| strip_ns(s).to_string());
                    match ty.as_deref() {
                        Some("dust") => Self::color_to_floats(options, "color"),
                        Some("dust_color_transition") => {
                            Self::color_to_floats(options, "from_color");
                            Self::color_to_floats(options, "to_color");
                        }
                        _ => {}
                    }
                }
            }
            "enchantment" => Self::enchantment(tag),
            _ => {}
        }
    }

    /// 1.21.2 int RGB → 1.21 float list.
    fn color_to_floats(options: &mut Tag, key: &str) {
        let Some(rgb) = options.get(key).and_then(Tag::as_i32) else { return };
        let c = |shift: i32| Tag::Float(((rgb >> shift) & 0xFF) as f32 / 255.0);
        options.insert(key, Tag::List(vec![c(16), c(8), c(0)]));
    }

    /// Enchantment definitions: renamed effect type and attribute ids.
    fn enchantment(tag: &mut Tag) {
        match tag {
            Tag::Compound(entries) => {
                for (k, v) in entries.iter_mut() {
                    if let Tag::String(s) = v {
                        if k == "type" && strip_ns(s) == "change_item_damage" {
                            *s = "minecraft:damage_item".into();
                        } else if k == "attribute" {
                            if let Some(idx) = d::NEW_ATTRIBUTE_NAMES.iter().position(|n| *n == strip_ns(s)) {
                                if let Some(old) = d::ATTRIBUTES.get(idx).and_then(|m| d::OLD_ATTRIBUTE_NAMES.get(*m as usize)) {
                                    *s = format!("minecraft:{old}");
                                }
                            }
                        }
                    } else {
                        Self::enchantment(v);
                    }
                }
            }
            Tag::List(list) => list.iter_mut().for_each(Self::enchantment),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::items::Dir;
    use super::*;

    #[test]
    fn pack_unpack_roundtrip() {
        let values: Vec<u32> = (0..4096).map(|i| (i * 7919) % 27000).collect();
        let packed = pack(&values, 15);
        assert_eq!(unpack(&packed, 15, 4096), values);
    }

    #[test]
    fn section_remaps_palette_and_keeps_layout() {
        let items = Items::new(Dir::ToClient);
        let last = d::NEW_BLOCK_STATE_COUNT as i32 - 1;
        let mut s = Writer::new();
        s.i16(4096);
        s.u8(4);
        s.varint(2);
        s.varint(0);
        s.varint(last);
        s.long_array(&vec![0x1111_1111_1111_1111u64 as i64; 256]);
        s.u8(0);
        s.varint(3);
        s.varint(0);
        let mut out = Writer::new();
        World::section(&items, &mut Reader::new(&s.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        assert_eq!(r.i16().unwrap(), 4096);
        assert_eq!(r.u8().unwrap(), 4);
        assert_eq!(r.varint().unwrap(), 2);
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), d::BLOCK_STATES[last as usize] as i32);
        assert_eq!(r.long_array().unwrap().len(), 256);
        assert_eq!(r.u8().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), 3);
        assert_eq!(r.long_array().unwrap().len(), 0);
        assert!(r.is_empty());
    }

    #[test]
    fn instrument_registry_is_dropped_and_features_get_bundles() {
        let mut p = Writer::new();
        p.string("minecraft:instrument");
        p.varint(0);
        assert!(World::registry_data(&p.buf).unwrap().is_none());
        let mut f = Writer::new();
        f.varint(1);
        f.string("minecraft:vanilla");
        let out = World::enabled_features(&f.buf).unwrap();
        let mut r = Reader::new(&out);
        assert_eq!(r.varint().unwrap(), 2);
    }

    #[test]
    fn dust_color_becomes_floats() {
        let mut options = Tag::compound();
        options.insert("type", Tag::String("minecraft:dust".into()));
        options.insert("color", Tag::Int(0xFF0000));
        World::color_to_floats(&mut options, "color");
        assert_eq!(options.get("color"), Some(&Tag::List(vec![Tag::Float(1.0), Tag::Float(0.0), Tag::Float(0.0)])));
    }
}
