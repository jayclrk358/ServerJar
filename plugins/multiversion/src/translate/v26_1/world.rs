//! World data 26.2 → 26.1: chunks, block updates, block entities, tags, registry data.
//!
//! 26.2 dropped the bed block entity; a 26.1 client needs one per bed block to render it, so
//! chunks and block updates grow synthesized bed block entities.

use super::data_26_1 as d;
use super::items::Items;
use crate::proto::buf::{DecodeError, Reader, Result, Writer};
use crate::proto::nbt::{self, Tag};

const BLOCKS_PER_SECTION: usize = 4096;
const BIOMES_PER_SECTION: usize = 64;
/// 26.2 block state ids of every bed state (white_bed[...] .. black_bed[...]).
const BED_STATES: std::ops::RangeInclusive<i32> = 1931..=2186;
/// `minecraft:bed` block entity type in 26.1.
const BED_BLOCK_ENTITY_OLD: i32 = 25;

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

pub fn is_bed(state: i32) -> bool {
    BED_STATES.contains(&state)
}

/// A block entity to synthesize: packed xz, absolute y.
struct BedPos {
    packed_xz: u8,
    y: i16,
}

#[derive(Default)]
pub struct World {
    /// min_y of each dimension type, in registry order.
    dimension_min_y: Vec<i32>,
    /// min_y of the dimension the player is in.
    min_y: i32,
}

impl World {
    pub fn new() -> Self {
        Self { dimension_min_y: Vec::new(), min_y: -64 }
    }

    /// Called with the dimension type id from LOGIN / RESPAWN.
    pub fn set_dimension(&mut self, dimension_type: i32) {
        if let Some(y) = usize::try_from(dimension_type).ok().and_then(|i| self.dimension_min_y.get(i)) {
            self.min_y = *y;
        }
    }

    /// Translates one chunk section; collects bed positions (section-local index) for beds.
    fn section(items: &Items, r: &mut Reader, w: &mut Writer, beds: &mut Vec<usize>) -> Result<()> {
        w.i16(r.i16()?); // non-air block count
        w.i16(r.i16()?); // fluid count
        Self::block_palette(items, r, w, beds)?;
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

    fn block_palette(items: &Items, r: &mut Reader, w: &mut Writer, beds: &mut Vec<usize>) -> Result<()> {
        let bits = r.u8()? as u32;
        if bits == 0 {
            let state = r.varint()?;
            w.u8(0);
            w.varint(items.block_state(state));
            if is_bed(state) {
                beds.extend(0..BLOCKS_PER_SECTION);
            }
            return Ok(());
        }
        if bits <= 8 {
            // Indirect palette: remap entries, keep the packed indexes as-is.
            let n = r.varint()?;
            if n < 0 {
                return Err(err("negative palette length"));
            }
            let mut entries = Vec::with_capacity(n as usize);
            let mut bed_entries = Vec::new();
            for i in 0..n {
                let state = r.varint()?;
                if is_bed(state) {
                    bed_entries.push(i as u32);
                }
                entries.push(items.block_state(state));
            }
            let longs = Self::read_longs(r, Self::packed_len(bits.max(4), BLOCKS_PER_SECTION))?;
            if !bed_entries.is_empty() {
                for (idx, v) in unpack(&longs, bits.max(4), BLOCKS_PER_SECTION).iter().enumerate() {
                    if bed_entries.contains(v) {
                        beds.push(idx);
                    }
                }
            }
            w.u8(bits as u8);
            w.varint(n);
            for e in entries {
                w.varint(e);
            }
            Self::write_longs(w, &longs);
            return Ok(());
        }
        // Direct palette: every value is a global state id; repack in the old width.
        let nbits = new_block_bits().max(bits);
        let longs = Self::read_longs(r, Self::packed_len(nbits, BLOCKS_PER_SECTION))?;
        let values = unpack(&longs, nbits, BLOCKS_PER_SECTION);
        for (idx, v) in values.iter().enumerate() {
            if is_bed(*v as i32) {
                beds.push(idx);
            }
        }
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

    /// Light data (long-array bit sets + byte arrays): identical in both versions.
    fn light_data(r: &mut Reader, w: &mut Writer) -> Result<()> {
        for _ in 0..4 {
            w.long_array(&r.long_array()?);
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

    pub fn level_chunk_with_light(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
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
        let mut beds: Vec<BedPos> = Vec::new();
        let mut section_index = 0i32;
        while !sr.is_empty() {
            let mut local = Vec::new();
            Self::section(items, &mut sr, &mut sections, &mut local)?;
            for idx in local {
                beds.push(BedPos {
                    packed_xz: (((idx & 15) << 4) | ((idx >> 4) & 15)) as u8,
                    y: ((idx >> 8) as i32 + self.min_y + (section_index << 4)) as i16,
                });
            }
            section_index += 1;
        }
        w.byte_array(&sections.buf);
        // Block entities
        let n = r.varint()?;
        let mut body = Writer::new();
        let mut kept = 0;
        for _ in 0..n.max(0) {
            let packed_xz = r.u8()?;
            let y = r.i16()?;
            let ty = r.varint()?;
            let mut entry = Writer::new();
            Items::copy_nbt(&mut r, &mut entry)?;
            if let Some(mapped) = items.block_entity(ty) {
                body.u8(packed_xz);
                body.i16(y);
                body.varint(mapped);
                body.raw(&entry.buf);
                kept += 1;
            }
        }
        for bed in &beds {
            body.u8(bed.packed_xz);
            body.i16(bed.y);
            body.varint(BED_BLOCK_ENTITY_OLD);
            nbt::write(&mut body, &Tag::compound());
            kept += 1;
        }
        w.varint(kept);
        w.raw(&body.buf);
        Self::light_data(&mut r, &mut w)?;
        Ok(w.into_inner())
    }

    /// BLOCK_ENTITY_DATA packet payload for a synthesized bed.
    fn bed_entity_packet(pos: i64) -> Vec<u8> {
        let mut w = Writer::new();
        w.i64(pos);
        w.varint(BED_BLOCK_ENTITY_OLD);
        nbt::write(&mut w, &Tag::compound());
        w.into_inner()
    }

    fn pack_position(x: i32, y: i32, z: i32) -> i64 {
        ((i64::from(x) & 0x3FF_FFFF) << 38) | ((i64::from(z) & 0x3FF_FFFF) << 12) | (i64::from(y) & 0xFFF)
    }

    /// BLOCK_UPDATE: returns the payload plus extra BLOCK_ENTITY_DATA payloads for beds.
    pub fn block_update(items: &Items, payload: &[u8]) -> Result<(Vec<u8>, Vec<Vec<u8>>)> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        let pos = r.i64()?;
        w.i64(pos);
        let state = r.varint()?;
        w.varint(items.block_state(state));
        let extra = if is_bed(state) { vec![Self::bed_entity_packet(pos)] } else { Vec::new() };
        Ok((w.into_inner(), extra))
    }

    pub fn section_blocks_update(items: &Items, payload: &[u8]) -> Result<(Vec<u8>, Vec<Vec<u8>>)> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        let section = r.i64()?;
        w.i64(section);
        let chunk_x = (section >> 42) as i32;
        let chunk_y = ((section << 44) >> 44) as i32;
        let chunk_z = ((section << 22) >> 42) as i32;
        let n = r.varint()?;
        w.varint(n);
        let mut extra = Vec::new();
        for _ in 0..n.max(0) {
            let v = r.varlong()?;
            let state = (v >> 12) as i32;
            let pos = v & 0xfff;
            w.varlong((i64::from(items.block_state(state)) << 12) | pos);
            if is_bed(state) {
                let x = ((pos >> 8) & 15) as i32 + (chunk_x << 4);
                let z = ((pos >> 4) & 15) as i32 + (chunk_z << 4);
                let y = (pos & 15) as i32 + (chunk_y << 4);
                extra.push(Self::bed_entity_packet(Self::pack_position(x, y, z)));
            }
        }
        Ok((w.into_inner(), extra))
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

    /// BLOCK_ENTITY_DATA: remap the type; `None` when 26.1 has no such block entity.
    pub fn block_entity_data(items: &Items, payload: &[u8]) -> Result<Option<Vec<u8>>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        w.i64(r.i64()?);
        let ty = r.varint()?;
        let Some(mapped) = items.block_entity(ty) else { return Ok(None) };
        w.varint(mapped);
        Items::copy_nbt(&mut r, &mut w)?;
        Ok(Some(w.into_inner()))
    }

    /// UPDATE_TAGS: remap ids, drop tags of registries the old client lacks.
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
                body.string(r.string()?);
                let ids = r.list(|r| r.varint())?;
                let mapped: Vec<i32> = ids
                    .into_iter()
                    .filter_map(|id| match key.as_str() {
                        "block" => Some(items.block(id)),
                        "item" => Some(items.item(id)),
                        "entity_type" => Some(items.entity(id)),
                        "block_entity_type" => items.block_entity(id),
                        "attribute" => items.attribute(id),
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
        let mut w = Writer::new();
        w.varint(kept);
        w.raw(&body.buf);
        Ok(w.into_inner())
    }

    /// REGISTRY_DATA: returns `None` when the whole registry must be dropped for the old client.
    pub fn registry_data(&mut self, items: &Items, payload: &[u8]) -> Result<Option<Vec<u8>>> {
        let mut r = Reader::new(payload);
        let registry = r.string()?.to_string();
        let key = registry.strip_prefix("minecraft:").unwrap_or(&registry).to_string();
        if REMOVED_REGISTRIES.contains(&key.as_str()) {
            return Ok(None);
        }
        if key == "dimension_type" {
            self.dimension_min_y.clear();
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
                if key == "dimension_type" {
                    self.dimension_min_y.push(tag.get("min_y").and_then(Tag::as_i32).unwrap_or(-64));
                }
                Self::registry_entry(items, &key, &mut tag);
                nbt::write(&mut w, &tag);
            } else if key == "dimension_type" {
                self.dimension_min_y.push(-64);
            }
        }
        Ok(Some(w.into_inner()))
    }

    fn registry_entry(items: &Items, key: &str, tag: &mut Tag) {
        match key {
            "enchantment" => {
                // Effect definitions only matter for display; the enchantment predicate format
                // changed again in 26.2, so drop them rather than translate.
                tag.remove("effects");
            }
            "jukebox_song" => Self::map_sound(items, tag, "sound_event"),
            "worldgen/biome" => {
                if let Some(effects) = tag.get_mut("effects") {
                    for sub in ["mood_sound", "additions_sound", "music"] {
                        if let Some(t) = effects.get_mut(sub) {
                            Self::map_sound(items, t, "sound");
                        }
                    }
                    Self::map_sound(items, effects, "ambient_sound");
                }
                if let Some(attrs) = tag.get_mut("attributes") {
                    for name in ["minecraft:audio/background_music", "audio/background_music"] {
                        if let Some(music) = attrs.get_mut(name) {
                            if let Some(default) = music.get_mut("default") {
                                Self::map_sound(items, default, "sound");
                            }
                        }
                    }
                    for name in ["minecraft:audio/ambient_sounds", "audio/ambient_sounds"] {
                        if let Some(ambient) = attrs.get_mut(name) {
                            for sub in ["loop", "mood", "additions"] {
                                Self::map_sound(items, ambient, sub);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Replaces a named sound the old client lacks with its mapped name, or an empty sound.
    fn map_sound(items: &Items, tag: &mut Tag, name: &str) {
        let Some(Tag::String(sound)) = tag.get(name) else { return };
        match items.sound_name(sound) {
            Some(mapped) => tag.insert(name, Tag::String(mapped)),
            None => {
                if sound.contains(':') && !sound.starts_with("minecraft:") {
                    return; // custom namespace: resource pack sound, leave it
                }
                let mut event = Tag::compound();
                event.insert("sound_id", Tag::String("minecraft:intentionally_empty".into()));
                tag.insert(name, event);
            }
        }
    }
}

/// Registries that exist in 26.2 but not 26.1.
pub const REMOVED_REGISTRIES: &[&str] = &["sulfur_cube_archetype"];

#[cfg(test)]
mod tests {
    use super::super::items::Dir;
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
    fn bed_range_matches_block_names() {
        assert_eq!(d::OLD_BLOCK_ENTITY_NAMES[BED_BLOCK_ENTITY_OLD as usize], "bed");
        // white_bed and black_bed each have 16 states; the range covers exactly the 16 bed colors.
        assert_eq!(BED_STATES.end() - BED_STATES.start() + 1, 16 * 16);
    }

    #[test]
    fn chunk_grows_bed_block_entities() {
        let items = Items::new(Dir::ToClient);
        let mut world = World::new();
        world.min_y = -64;
        let mut p = Writer::new();
        p.i32(0);
        p.i32(0);
        p.varint(0); // no heightmaps
        // one section: indirect palette {air, white_bed[first]}, block 0 and block 4095 are beds
        let mut section = Writer::new();
        section.i16(2);
        section.i16(0);
        section.u8(4);
        section.varint(2);
        section.varint(0);
        section.varint(*BED_STATES.start());
        let mut values = vec![0u32; BLOCKS_PER_SECTION];
        values[0] = 1;
        values[4095] = 1;
        for l in pack(&values, 4) {
            section.i64(l);
        }
        section.u8(0);
        section.varint(0);
        p.byte_array(&section.buf);
        p.varint(0); // no block entities
        for _ in 0..4 {
            p.long_array(&[]);
        }
        p.varint(0);
        p.varint(0);
        let out = world.level_chunk_with_light(&items, &p.buf).unwrap();
        let mut r = Reader::new(&out);
        r.i32().unwrap();
        r.i32().unwrap();
        r.varint().unwrap();
        r.byte_array().unwrap();
        assert_eq!(r.varint().unwrap(), 2);
        assert_eq!(r.u8().unwrap(), 0);
        assert_eq!(r.i16().unwrap(), -64);
        assert_eq!(r.varint().unwrap(), BED_BLOCK_ENTITY_OLD);
        nbt::skip(&mut r).unwrap();
        assert_eq!(r.u8().unwrap(), 0xff);
        assert_eq!(r.i16().unwrap(), -64 + 15);
    }

    #[test]
    fn block_update_emits_bed_entity() {
        let items = Items::new(Dir::ToClient);
        let mut p = Writer::new();
        p.i64(World::pack_position(1, 70, -3));
        p.varint(*BED_STATES.end());
        let (out, extra) = World::block_update(&items, &p.buf).unwrap();
        assert_eq!(extra.len(), 1);
        let mut r = Reader::new(&out);
        assert_eq!(r.i64().unwrap(), World::pack_position(1, 70, -3));
        assert_eq!(r.varint().unwrap(), d::BLOCK_STATES[*BED_STATES.end() as usize] as i32);
        let mut r = Reader::new(&extra[0]);
        assert_eq!(r.i64().unwrap(), World::pack_position(1, 70, -3));
        assert_eq!(r.varint().unwrap(), BED_BLOCK_ENTITY_OLD);
    }

    #[test]
    fn registry_tracks_dimension_min_y_and_drops_removed_registry() {
        let items = Items::new(Dir::ToClient);
        let mut world = World::new();
        let mut p = Writer::new();
        p.string("minecraft:dimension_type");
        p.varint(2);
        p.string("minecraft:overworld");
        p.bool(true);
        let mut t = Tag::compound();
        t.insert("min_y", Tag::Int(-64));
        nbt::write(&mut p, &t);
        p.string("minecraft:the_nether");
        p.bool(true);
        let mut t = Tag::compound();
        t.insert("min_y", Tag::Int(0));
        nbt::write(&mut p, &t);
        assert!(world.registry_data(&items, &p.buf).unwrap().is_some());
        world.set_dimension(1);
        assert_eq!(world.min_y, 0);
        let mut p = Writer::new();
        p.string("minecraft:sulfur_cube_archetype");
        p.varint(0);
        assert!(world.registry_data(&items, &p.buf).unwrap().is_none());
    }

    #[test]
    fn jukebox_sound_is_mapped() {
        let items = Items::new(Dir::ToClient);
        let mut t = Tag::compound();
        t.insert("sound_event", Tag::String("minecraft:music_disc.bounce".into()));
        World::registry_entry(&items, "jukebox_song", &mut t);
        assert_eq!(t.get("sound_event").and_then(Tag::as_str), Some("minecraft:music_disc.wait"));
    }
}
