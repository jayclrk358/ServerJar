//! Entities 1.21.4 → 1.21.2: spawning, entity data, equipment and the player list.
//!
//! The entity data serializer list is the same in both versions. What changed: salmon size went
//! from a string to a varint enum (index 17), and creakings gained two fields (18 tearing down,
//! 19 home position) that the old client does not have.

use std::collections::HashMap;

use super::data_1_21_2 as d;
use super::items::{Items, Reg};
use super::misc::Misc;
use crate::proto::buf::{DecodeError, Reader, Result, Writer};

fn err(msg: &str) -> DecodeError {
    DecodeError::Other(msg.to_string())
}

/// Entity data serializer ids (1.21.2 and 1.21.4).
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
    pub const COMPOUND_TAG: i32 = 16;
    pub const PARTICLE: i32 = 17;
    pub const PARTICLES: i32 = 18;
    pub const VILLAGER_DATA: i32 = 19;
    pub const OPT_VARINT: i32 = 20;
    pub const POSE: i32 = 21;
    pub const CAT_VARIANT: i32 = 22;
    pub const WOLF_VARIANT: i32 = 23;
    pub const FROG_VARIANT: i32 = 24;
    pub const OPT_GLOBAL_POSITION: i32 = 25;
    pub const PAINTING_VARIANT: i32 = 26;
    pub const SNIFFER_STATE: i32 = 27;
    pub const ARMADILLO_STATE: i32 = 28;
    pub const VECTOR3F: i32 = 29;
    pub const QUATERNION: i32 = 30;
}

fn new_entity_id(name: &str) -> i32 {
    d::NEW_ENTITY_NAMES.iter().position(|n| *n == name).map_or(-1, |p| p as i32)
}

pub struct Entities {
    /// entity id → 1.21.4 entity type id
    types: HashMap<i32, i32>,
    salmon: i32,
    creaking: i32,
    falling_block: i32,
}

impl Default for Entities {
    fn default() -> Self {
        Self {
            types: HashMap::new(),
            salmon: new_entity_id("salmon"),
            creaking: new_entity_id("creaking"),
            falling_block: new_entity_id("falling_block"),
        }
    }
}

impl Entities {
    pub fn clear(&mut self) {
        self.types.clear();
    }

    /// ADD_ENTITY: id, uuid, type, xyz, pitch, yaw, head yaw, data, velocity (same layout).
    pub fn add_entity(&mut self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let entity_id = Items::copy_varint(&mut r, &mut w)?;
        w.uuid(r.uuid()?);
        let ty = r.varint()?;
        self.types.insert(entity_id, ty);
        w.varint(items.entity(ty));
        w.raw(r.bytes(24 + 3)?);
        let data = r.varint()?;
        w.varint(if ty == self.falling_block { items.block_state(data) } else { data });
        w.raw(r.remaining());
        Ok(w.into_inner())
    }

    pub fn remove_entities(&mut self, payload: &[u8]) -> Result<()> {
        let mut r = Reader::new(payload);
        for id in r.list(|r| r.varint())? {
            self.types.remove(&id);
        }
        Ok(())
    }

    /// SET_ENTITY_DATA: remap ids inside values and apply the per-type changes.
    pub fn set_entity_data(&self, items: &Items, misc: &Misc, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let entity_id = Items::copy_varint(&mut r, &mut w)?;
        let entity_type = self.types.get(&entity_id).copied().unwrap_or(-1);
        loop {
            let index = r.u8()?;
            if index == 0xff {
                w.u8(0xff);
                break;
            }
            let t = r.varint()?;
            if entity_type == self.salmon && index == 17 && t == ty::VARINT {
                let size = match r.varint()? {
                    0 => "small",
                    2 => "large",
                    _ => "medium",
                };
                w.u8(index);
                w.varint(ty::STRING);
                w.string(size);
                continue;
            }
            let mut value = Writer::new();
            Self::entity_data_value(items, misc, t, &mut r, &mut value)?;
            if entity_type == self.creaking && index >= 18 {
                continue; // tearing down, home position
            }
            w.u8(index);
            w.varint(t);
            w.raw(&value.buf);
        }
        Ok(w.into_inner())
    }

    fn entity_data_value(items: &Items, misc: &Misc, t: i32, r: &mut Reader, w: &mut Writer) -> Result<()> {
        match t {
            ty::BYTE => {
                w.u8(r.u8()?);
            }
            ty::VARINT | ty::DIRECTION | ty::OPT_VARINT | ty::POSE | ty::CAT_VARIANT | ty::FROG_VARIANT | ty::SNIFFER_STATE
            | ty::ARMADILLO_STATE => {
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
            ty::COMPONENT | ty::COMPOUND_TAG => Items::copy_nbt(r, w)?,
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
            ty::PARTICLE => misc.particle(items, r, w)?,
            ty::PARTICLES => {
                let n = Items::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    misc.particle(items, r, w)?;
                }
            }
            ty::VILLAGER_DATA => {
                for _ in 0..3 {
                    w.varint(r.varint()?);
                }
            }
            ty::WOLF_VARIANT => {
                items.holder(r, w, Reg::Same, |s, r, w| {
                    for _ in 0..3 {
                        Items::copy_string(r, w)?; // wild, tame, angry textures
                    }
                    s.holder_set(r, w, Reg::Same) // biomes
                })?;
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
            other => return Err(err(&format!("unknown entity data type {other}"))),
        }
        Ok(())
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

    /// PLAYER_INFO_UPDATE: 1.21.4 added action 7 (show hat); drop the bit and its value.
    pub fn player_info_update(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let actions = r.u8()?;
        w.u8(actions & 0x7f);
        let n = Items::copy_varint(&mut r, &mut w)?;
        let items = Items::new(super::items::Dir::ToClient);
        for _ in 0..n.max(0) {
            w.uuid(r.uuid()?);
            if actions & 0x01 != 0 {
                Items::copy_string(&mut r, &mut w)?; // name
                items.profile_properties(&mut r, &mut w)?;
            }
            if actions & 0x02 != 0 && Items::copy_bool(&mut r, &mut w)? {
                w.uuid(r.uuid()?); // chat session id
                w.i64(r.i64()?); // key expiry
                w.byte_array(r.byte_array()?); // public key
                w.byte_array(r.byte_array()?); // key signature
            }
            if actions & 0x04 != 0 {
                Items::copy_varint(&mut r, &mut w)?; // game mode
            }
            if actions & 0x08 != 0 {
                Items::copy_bool(&mut r, &mut w)?; // listed
            }
            if actions & 0x10 != 0 {
                Items::copy_varint(&mut r, &mut w)?; // latency
            }
            if actions & 0x20 != 0 {
                Items::copy_opt_nbt(&mut r, &mut w)?; // display name
            }
            if actions & 0x40 != 0 {
                Items::copy_varint(&mut r, &mut w)?; // list order
            }
            if actions & 0x80 != 0 {
                r.bool()?; // show hat
            }
        }
        Ok(w.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::super::items::Dir;
    use super::*;

    #[test]
    fn salmon_size_becomes_string() {
        let items = Items::new(Dir::ToClient);
        let misc = Misc::new();
        let mut e = Entities::default();
        e.types.insert(4, e.salmon);
        let mut p = Writer::new();
        p.varint(4);
        p.u8(17);
        p.varint(ty::VARINT);
        p.varint(2);
        p.u8(0xff);
        let out = e.set_entity_data(&items, &misc, &p.buf).unwrap();
        let mut r = Reader::new(&out);
        assert_eq!(r.varint().unwrap(), 4);
        assert_eq!(r.u8().unwrap(), 17);
        assert_eq!(r.varint().unwrap(), ty::STRING);
        assert_eq!(r.string().unwrap(), "large");
        assert_eq!(r.u8().unwrap(), 0xff);
        assert!(r.is_empty());
    }

    #[test]
    fn creaking_new_fields_are_dropped() {
        let items = Items::new(Dir::ToClient);
        let misc = Misc::new();
        let mut e = Entities::default();
        e.types.insert(9, e.creaking);
        let mut p = Writer::new();
        p.varint(9);
        p.u8(17);
        p.varint(ty::BOOL);
        p.bool(true);
        p.u8(18);
        p.varint(ty::BOOL);
        p.bool(true);
        p.u8(19);
        p.varint(ty::OPT_POSITION);
        p.bool(true);
        p.i64(123);
        p.u8(0xff);
        let out = e.set_entity_data(&items, &misc, &p.buf).unwrap();
        let mut r = Reader::new(&out);
        assert_eq!(r.varint().unwrap(), 9);
        assert_eq!(r.u8().unwrap(), 17);
        r.varint().unwrap();
        r.bool().unwrap();
        assert_eq!(r.u8().unwrap(), 0xff);
        assert!(r.is_empty());
    }

    #[test]
    fn player_info_drops_show_hat() {
        let mut p = Writer::new();
        p.u8(0x80 | 0x04 | 0x08);
        p.varint(1);
        p.uuid(5);
        p.varint(1); // game mode
        p.bool(true); // listed
        p.bool(false); // show hat
        let out = Entities::player_info_update(&p.buf).unwrap();
        let mut r = Reader::new(&out);
        assert_eq!(r.u8().unwrap(), 0x0c);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.uuid().unwrap(), 5);
        assert_eq!(r.varint().unwrap(), 1);
        assert!(r.bool().unwrap());
        assert!(r.is_empty());
    }

    #[test]
    fn falling_block_data_is_a_block_state() {
        let items = Items::new(Dir::ToClient);
        let mut e = Entities::default();
        let last = (d::NEW_BLOCK_STATE_COUNT - 1) as i32;
        let mut p = Writer::new();
        p.varint(1);
        p.uuid(2);
        p.varint(e.falling_block);
        p.raw(&[0u8; 27]);
        p.varint(last);
        p.raw(&[0u8; 6]);
        let out = e.add_entity(&items, &p.buf).unwrap();
        let mut r = Reader::new(&out);
        r.varint().unwrap();
        r.uuid().unwrap();
        assert_eq!(r.varint().unwrap(), items.entity(e.falling_block));
        r.bytes(27).unwrap();
        assert_eq!(r.varint().unwrap(), d::BLOCK_STATES[last as usize] as i32);
        assert_eq!(r.remaining().len(), 6);
    }
}
