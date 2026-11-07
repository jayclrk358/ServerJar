//! Entities 26.1 → 1.21.11: spawning, entity data (metadata), game mode tracking, equipment.

use std::collections::HashMap;

use super::super::data_1_21_11 as d;
use super::items::{Items, Reg};
use super::misc::Misc;
use crate::proto::buf::{DecodeError, Reader, Result, Writer};

fn err(msg: &str) -> DecodeError {
    DecodeError::Other(msg.to_string())
}

/// Entity data value types in 26.1 (vanilla).
mod ty {
    pub const BYTE: i32 = 0;
    pub const VARINT: i32 = 1;
    pub const VARLONG: i32 = 2;
    pub const FLOAT: i32 = 3;
    pub const STRING: i32 = 4;
    pub const COMPONENT: i32 = 5;
    pub const OPT_COMPONENT: i32 = 6;
    pub const ITEM: i32 = 7;
    pub const BOOL: i32 = 8;
    pub const ROTATIONS: i32 = 9;
    pub const POSITION: i32 = 10;
    pub const OPT_POSITION: i32 = 11;
    pub const DIRECTION: i32 = 12;
    pub const OPT_UUID: i32 = 13;
    pub const BLOCK_STATE: i32 = 14;
    pub const OPT_BLOCK_STATE: i32 = 15;
    pub const PARTICLE: i32 = 16;
    pub const PARTICLES: i32 = 17;
    pub const VILLAGER_DATA: i32 = 18;
    pub const OPT_VARINT: i32 = 19;
    pub const CAT_SOUND_VARIANT: i32 = 22;
    pub const COW_SOUND_VARIANT: i32 = 24;
    pub const PIG_SOUND_VARIANT: i32 = 29;
    pub const CHICKEN_SOUND_VARIANT: i32 = 31;
    pub const OPT_GLOBAL_POSITION: i32 = 33;
    pub const PAINTING_VARIANT: i32 = 34;
    pub const VECTOR3F: i32 = 39;
    pub const QUATERNION: i32 = 40;
    pub const RESOLVABLE_PROFILE: i32 = 41;
    pub const HUMANOID_ARM: i32 = 42;
}

/// 26.1 entity data type → 1.21.11 type; `None` for the four sound variant types 26.1 added.
fn map_type(t: i32) -> Option<i32> {
    match t {
        0..=21 => Some(t),
        ty::CAT_SOUND_VARIANT | ty::COW_SOUND_VARIANT | ty::PIG_SOUND_VARIANT | ty::CHICKEN_SOUND_VARIANT => None,
        23 => Some(22),
        25..=28 => Some(t - 2),
        30 => Some(27),
        32 => Some(28),
        33..=ty::HUMANOID_ARM => Some(t - 4),
        _ => None,
    }
}

/// Entity types with an "age locked" flag at index 17 in 26.1 (every ageable mob).
const AGEABLE: &[&str] = &[
    "villager", "wandering_trader", "axolotl", "chicken", "panda", "pig", "polar_bear", "rabbit", "sheep", "bee", "turtle",
    "fox", "frog", "goat", "hoglin", "strider", "sniffer", "armadillo", "happy_ghast", "ocelot", "cow", "mooshroom", "cat",
    "wolf", "parrot", "nautilus", "zombie_nautilus", "horse", "skeleton_horse", "zombie_horse", "camel", "camel_husk",
    "donkey", "mule", "llama", "trader_llama", "dolphin", "squid", "glow_squid",
];

/// Entity data indexes (26.1 numbering) the 1.21.11 client doesn't have, per entity type.
fn removed_indexes(entity: &str) -> &'static [u8] {
    match entity {
        "villager" => &[17, 20],  // age locked, villager data finalized
        "cat" => &[17, 24],       // age locked, sound variant
        "chicken" | "cow" => &[17, 19],
        "pig" => &[17, 20],
        "tadpole" => &[17],       // age locked
        "zombie_villager" => &[21], // villager data finalized
        e if AGEABLE.contains(&e) => &[17],
        _ => &[],
    }
}

#[derive(Default)]
pub struct Entities {
    /// entity id → 26.1 entity type id
    types: HashMap<i32, i32>,
    /// Current game mode (for turning attacks into spectate requests), -1 = unknown.
    pub game_mode: i32,
}

impl Entities {
    pub fn new() -> Self {
        Self { types: HashMap::new(), game_mode: -1 }
    }

    pub fn clear(&mut self) {
        self.types.clear();
    }

    pub fn add_entity(&mut self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let entity_id = r.varint()?;
        w.varint(entity_id);
        w.uuid(r.uuid()?);
        let t = r.varint()?;
        self.types.insert(entity_id, t);
        w.varint(items.entity(t));
        w.raw(r.bytes(24)?); // x y z
        Self::copy_movement(&mut r, &mut w)?;
        w.raw(r.bytes(3)?); // pitch yaw head
        let data = r.varint()?;
        let is_falling_block = d::NEW_ENTITY_NAMES.get(t as usize) == Some(&"falling_block");
        w.varint(if is_falling_block { items.block_state(data) } else { data });
        w.raw(r.remaining());
        Ok(w.into_inner())
    }

    /// Low-precision vector (1.21.9+): one 0 byte for zero, else 6 bytes plus an optional varint.
    pub fn copy_movement(r: &mut Reader, w: &mut Writer) -> Result<()> {
        let first = r.u8()?;
        w.u8(first);
        if first == 0 {
            return Ok(());
        }
        w.raw(r.bytes(5)?);
        if first & 0b100 != 0 {
            w.varint(r.varint()?);
        }
        Ok(())
    }

    pub fn remove_entities(&mut self, payload: &[u8]) -> Result<()> {
        let mut r = Reader::new(payload);
        for id in r.list(|r| r.varint())? {
            self.types.remove(&id);
        }
        Ok(())
    }

    pub fn set_entity_data(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let entity_id = r.varint()?;
        w.varint(entity_id);
        let name = self.types.get(&entity_id).and_then(|t| d::NEW_ENTITY_NAMES.get(*t as usize)).copied().unwrap_or("");
        let removed = removed_indexes(name);
        loop {
            let index = r.u8()?;
            if index == 0xff {
                w.u8(0xff);
                break;
            }
            let t = r.varint()?;
            let mut value = Writer::new();
            Self::entity_data_value(items, t, &mut r, &mut value)?;
            let Some(mapped) = map_type(t) else { continue };
            if removed.contains(&index) {
                continue;
            }
            let shift = removed.iter().filter(|i| **i < index).count() as u8;
            w.u8(index - shift);
            w.varint(mapped);
            w.raw(&value.buf);
        }
        Ok(w.into_inner())
    }

    fn entity_data_value(items: &Items, t: i32, r: &mut Reader, w: &mut Writer) -> Result<()> {
        match t {
            ty::BYTE => {
                w.u8(r.u8()?);
            }
            ty::VARINT | ty::DIRECTION | ty::OPT_VARINT | 20..=32 | 35..=38 | ty::HUMANOID_ARM => {
                w.varint(r.varint()?);
            }
            ty::VARLONG => {
                w.varlong(r.varlong()?);
            }
            ty::FLOAT => {
                w.raw(r.bytes(4)?);
            }
            ty::STRING => {
                w.string(r.string()?);
            }
            ty::COMPONENT => Items::copy_nbt(r, w)?,
            ty::OPT_COMPONENT => Items::copy_opt_nbt(r, w)?,
            ty::ITEM => items.item_stack(r, w)?,
            ty::BOOL => {
                w.bool(r.bool()?);
            }
            ty::ROTATIONS | ty::VECTOR3F => {
                w.raw(r.bytes(12)?);
            }
            ty::QUATERNION => {
                w.raw(r.bytes(16)?);
            }
            ty::POSITION => {
                w.i64(r.i64()?);
            }
            ty::OPT_POSITION => {
                if Items::copy_bool(r, w)? {
                    w.i64(r.i64()?);
                }
            }
            ty::OPT_UUID => {
                if Items::copy_bool(r, w)? {
                    w.uuid(r.uuid()?);
                }
            }
            ty::BLOCK_STATE => {
                w.varint(items.block_state(r.varint()?));
            }
            ty::OPT_BLOCK_STATE => {
                let v = r.varint()?;
                w.varint(if v == 0 { 0 } else { items.block_state(v) });
            }
            ty::PARTICLE => Misc::particle(items, r, w)?,
            ty::PARTICLES => {
                let n = Items::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    Misc::particle(items, r, w)?;
                }
            }
            ty::VILLAGER_DATA => {
                for _ in 0..3 {
                    w.varint(r.varint()?);
                }
            }
            ty::OPT_GLOBAL_POSITION => {
                if Items::copy_bool(r, w)? {
                    w.string(r.string()?);
                    w.i64(r.i64()?);
                }
            }
            ty::PAINTING_VARIANT => {
                items.holder(r, w, Reg::Same, |_, r, w| {
                    Items::copy_varint(r, w)?;
                    Items::copy_varint(r, w)?;
                    Items::copy_string(r, w)?;
                    Items::copy_opt_nbt(r, w)?;
                    Items::copy_opt_nbt(r, w)
                })?;
            }
            ty::RESOLVABLE_PROFILE => {
                if Items::copy_bool(r, w)? {
                    w.uuid(r.uuid()?);
                    w.string(r.string()?);
                } else {
                    Items::copy_opt_string(r, w)?;
                    if Items::copy_bool(r, w)? {
                        w.uuid(r.uuid()?);
                    }
                }
                let n = Items::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    Items::copy_string(r, w)?;
                    Items::copy_string(r, w)?;
                    Items::copy_opt_string(r, w)?;
                }
                for _ in 0..3 {
                    Items::copy_opt_string(r, w)?;
                }
                if Items::copy_bool(r, w)? {
                    w.bool(r.bool()?);
                }
            }
            other => return Err(err(&format!("unknown entity data type {other}"))),
        }
        Ok(())
    }

    /// Reads the game mode out of the common spawn info (dimension type, name, seed, mode).
    fn spawn_info_game_mode(r: &mut Reader) -> Result<i32> {
        r.varint()?;
        r.string()?;
        r.i64()?;
        Ok(i32::from(r.u8()?))
    }

    /// LOGIN: unchanged layout; only the game mode is recorded.
    pub fn login(&mut self, payload: &[u8]) -> Result<()> {
        let mut r = Reader::new(payload);
        r.i32()?;
        r.bool()?;
        let n = r.varint()?;
        for _ in 0..n.max(0) {
            r.string()?;
        }
        for _ in 0..3 {
            r.varint()?;
        }
        r.bytes(3)?;
        self.game_mode = Self::spawn_info_game_mode(&mut r)?;
        Ok(())
    }

    pub fn respawn(&mut self, payload: &[u8]) -> Result<()> {
        self.game_mode = Self::spawn_info_game_mode(&mut Reader::new(payload))?;
        Ok(())
    }

    /// GAME_EVENT 3 = change game mode.
    pub fn game_event(&mut self, payload: &[u8]) -> Result<()> {
        let mut r = Reader::new(payload);
        if r.u8()? == 3 {
            self.game_mode = (r.f32()? + 0.5).floor() as i32;
        }
        Ok(())
    }

    pub fn is_spectator(&self) -> bool {
        self.game_mode == 3
    }

    pub fn set_equipment(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.varint(r.varint()?);
        loop {
            let slot = r.i8()?;
            w.i8(slot);
            items.item_stack(&mut r, &mut w)?;
            if slot >= 0 {
                break;
            }
        }
        Ok(w.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::items::Dir;

    fn entity_type(name: &str) -> i32 {
        d::NEW_ENTITY_NAMES.iter().position(|n| *n == name).unwrap() as i32
    }

    fn spawn(e: &mut Entities, items: &Items, id: i32, t: i32) {
        let mut p = Writer::new();
        p.varint(id).uuid(1).varint(t).raw(&[0u8; 24]).u8(0).raw(&[0, 0, 0]).varint(0);
        e.add_entity(items, &p.buf).unwrap();
    }

    #[test]
    fn data_types_shift_and_sound_variants_drop() {
        assert_eq!(map_type(21), Some(21));
        assert_eq!(map_type(22), None);
        assert_eq!(map_type(23), Some(22));
        assert_eq!(map_type(25), Some(23));
        assert_eq!(map_type(30), Some(27));
        assert_eq!(map_type(32), Some(28));
        assert_eq!(map_type(33), Some(29));
        assert_eq!(map_type(42), Some(38));
    }

    #[test]
    fn cat_loses_age_lock_and_sound_variant() {
        let items = Items::new(Dir::ToClient);
        let mut e = Entities::new();
        spawn(&mut e, &items, 5, entity_type("cat"));
        let mut p = Writer::new();
        p.varint(5);
        p.u8(16).varint(ty::BOOL).bool(false); // baby
        p.u8(17).varint(ty::BOOL).bool(true); // age locked (26.1 only)
        p.u8(23).varint(21).varint(2); // cat variant
        p.u8(24).varint(ty::CAT_SOUND_VARIANT).varint(1); // sound variant (26.1 only)
        p.u8(25).varint(ty::BOOL).bool(true);
        p.u8(0xff);
        let out = e.set_entity_data(&items, &p.buf).unwrap();
        let mut r = Reader::new(&out);
        assert_eq!(r.varint().unwrap(), 5);
        assert_eq!(r.u8().unwrap(), 16);
        r.varint().unwrap();
        r.bool().unwrap();
        assert_eq!(r.u8().unwrap(), 22); // 23 minus age lock
        assert_eq!(r.varint().unwrap(), 21);
        r.varint().unwrap();
        assert_eq!(r.u8().unwrap(), 23); // 25 minus both
        r.varint().unwrap();
        r.bool().unwrap();
        assert_eq!(r.u8().unwrap(), 0xff);
        assert!(r.is_empty());
    }

    #[test]
    fn game_mode_tracking() {
        let mut e = Entities::new();
        let mut p = Writer::new();
        p.u8(3).f32(3.0);
        e.game_event(&p.buf).unwrap();
        assert!(e.is_spectator());
        let mut p = Writer::new();
        p.varint(0).string("minecraft:overworld").i64(0).u8(1).i8(-1);
        e.respawn(&p.buf).unwrap();
        assert!(!e.is_spectator());
    }
}
