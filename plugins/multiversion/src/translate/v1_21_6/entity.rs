//! Entities 1.21.7 → 1.21.6: spawning, entity data (metadata) and equipment.
//!
//! Entity types, data serializers and all packet layouts are identical in both versions; only item,
//! block state and particle ids carried in values are remapped.

use super::super::data_1_21_6 as d;
use super::items::Items;
use super::misc::Misc;
use crate::proto::buf::{DecodeError, Reader, Result, Writer};

fn err(msg: &str) -> DecodeError {
    DecodeError::Other(msg.to_string())
}

/// Entity data serializer ids (vanilla entity data types, used by 1.21.5 to 1.21.7).
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
    /// 22..=28: cat, cow, wolf, wolf sound, frog, pig, chicken variants (all varints).
    pub const FIRST_VARIANT: i32 = 22;
    pub const LAST_VARIANT: i32 = 28;
    pub const OPT_GLOBAL_POSITION: i32 = 29;
    pub const PAINTING_VARIANT: i32 = 30;
    pub const SNIFFER_STATE: i32 = 31;
    pub const ARMADILLO_STATE: i32 = 32;
    pub const VECTOR3F: i32 = 33;
    pub const QUATERNION: i32 = 34;
}

pub struct Entities;

impl Entities {
    /// ADD_ENTITY: only falling blocks carry an id (their block state) in the data field.
    pub fn add_entity(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.varint(r.varint()?); // entity id
        w.uuid(r.uuid()?);
        let ty = r.varint()?;
        w.varint(items.entity(ty));
        w.raw(r.bytes(24 + 3)?); // x y z, pitch yaw head yaw
        let data = r.varint()?;
        let falling_block = d::NEW_ENTITY_NAMES.iter().position(|n| *n == "falling_block").map_or(-1, |p| p as i32);
        w.varint(if ty == falling_block { items.block_state(data) } else { data });
        w.raw(r.remaining()); // velocity
        Ok(w.into_inner())
    }

    /// SET_ENTITY_DATA: remap ids inside values.
    pub fn set_entity_data(items: &Items, misc: &Misc, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.varint(r.varint()?);
        loop {
            let index = r.u8()?;
            w.u8(index);
            if index == 0xff {
                break;
            }
            let t = Items::copy_varint(&mut r, &mut w)?;
            Self::entity_data_value(items, misc, t, &mut r, &mut w)?;
        }
        Ok(w.into_inner())
    }

    fn entity_data_value(items: &Items, misc: &Misc, t: i32, r: &mut Reader, w: &mut Writer) -> Result<()> {
        match t {
            ty::BYTE => {
                w.u8(r.u8()?);
            }
            ty::VARINT | ty::DIRECTION | ty::OPT_VARINT | ty::POSE | ty::SNIFFER_STATE | ty::ARMADILLO_STATE => {
                w.varint(r.varint()?);
            }
            ty::FIRST_VARIANT..=ty::LAST_VARIANT => {
                w.varint(r.varint()?);
            }
            ty::VARLONG => {
                w.varlong(r.varlong()?);
            }
            ty::FLOAT => Items::copy_bytes(r, w, 4)?,
            ty::STRING => Items::copy_string(r, w)?,
            ty::COMPONENT | ty::COMPOUND_TAG => Items::copy_nbt(r, w)?,
            ty::OPT_COMPONENT => Items::copy_opt_nbt(r, w)?,
            ty::ITEM => items.item_stack(r, w)?,
            ty::BOOL => {
                w.bool(r.bool()?);
            }
            ty::ROTATIONS | ty::VECTOR3F => Items::copy_bytes(r, w, 12)?,
            ty::QUATERNION => Items::copy_bytes(r, w, 16)?,
            ty::POSITION => Items::copy_bytes(r, w, 8)?,
            ty::OPT_POSITION => {
                if Items::copy_bool(r, w)? {
                    Items::copy_bytes(r, w, 8)?;
                }
            }
            ty::OPT_UUID => {
                if Items::copy_bool(r, w)? {
                    Items::copy_bytes(r, w, 16)?;
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
            ty::OPT_GLOBAL_POSITION => {
                if Items::copy_bool(r, w)? {
                    Items::copy_string(r, w)?;
                    Items::copy_bytes(r, w, 8)?;
                }
            }
            ty::PAINTING_VARIANT => items.holder(r, w, super::items::Reg::Same, |_, r, w| {
                Items::copy_varint(r, w)?;
                Items::copy_varint(r, w)?;
                Items::copy_string(r, w)?;
                Items::copy_opt_nbt(r, w)?;
                Items::copy_opt_nbt(r, w)
            })?,
            other => return Err(err(&format!("unknown entity data type {other}"))),
        }
        Ok(())
    }

    /// SET_EQUIPMENT: entity id, then (slot byte with continuation bit, item stack) pairs.
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
    use super::super::items::Dir;
    use super::*;

    #[test]
    fn item_entity_data_is_remapped() {
        let items = Items::new(Dir::ToClient);
        let misc = Misc::new();
        let disc = d::NEW_ITEM_NAMES.iter().position(|n| *n == "music_disc_lava_chicken").unwrap() as i32;
        let mut p = Writer::new();
        p.varint(12);
        p.u8(8); // item entity stack index
        p.varint(ty::ITEM);
        p.varint(1);
        p.varint(disc);
        p.varint(0);
        p.varint(0);
        p.u8(0);
        p.varint(ty::BYTE);
        p.u8(3);
        p.u8(0xff);
        let out = Entities::set_entity_data(&items, &misc, &p.buf).unwrap();
        let mut r = Reader::new(&out);
        assert_eq!(r.varint().unwrap(), 12);
        assert_eq!(r.u8().unwrap(), 8);
        assert_eq!(r.varint().unwrap(), ty::ITEM);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), d::OLD_ITEM_NAMES.iter().position(|n| *n == "music_disc_pigstep").unwrap() as i32);
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.u8().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), ty::BYTE);
        assert_eq!(r.u8().unwrap(), 3);
        assert_eq!(r.u8().unwrap(), 0xff);
        assert!(r.is_empty());
    }

    #[test]
    fn add_entity_layout_is_kept() {
        let items = Items::new(Dir::ToClient);
        let mut p = Writer::new();
        p.varint(42);
        p.uuid(7);
        p.varint(3);
        p.raw(&[0u8; 27]);
        p.varint(5);
        p.raw(&[0, 1, 0, 2, 0, 3]);
        assert_eq!(Entities::add_entity(&items, &p.buf).unwrap(), p.buf);
    }
}
