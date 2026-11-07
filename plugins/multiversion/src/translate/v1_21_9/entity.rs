//! Entities 1.21.11 → 1.21.9: spawn type remaps and entity data (metadata).
//!
//! Follows the vanilla changes between these versions: nautilus → squid, zombie nautilus → glow
//! squid, camel husk → camel, parched → skeleton (via the generated entity table); the zombie
//! nautilus variant data type is removed; the humanoid arm type becomes a byte; wolf and bee anger
//! end times go back from absolute game time to remaining ticks.

use std::collections::HashMap;

use super::super::data_1_21_9 as d;
use super::items::{Items, Reg};
use super::misc::Misc;
use crate::proto::buf::{DecodeError, Reader, Result, Writer};

fn err(msg: &str) -> DecodeError {
    DecodeError::Other(msg.to_string())
}

/// Entity data value types in 1.21.11.
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
    /// 20..=27 are varint enums/registry ids (pose, cat/cow/wolf/frog/pig/chicken variants).
    pub const ZOMBIE_NAUTILUS_VARIANT: i32 = 28;
    pub const OPT_GLOBAL_POSITION: i32 = 29;
    pub const PAINTING_VARIANT: i32 = 30;
    /// 31..=34 are varint states (sniffer, armadillo, copper golem, weathering copper).
    pub const VECTOR3F: i32 = 35;
    pub const QUATERNION: i32 = 36;
    pub const RESOLVABLE_PROFILE: i32 = 37;
    pub const HUMANOID_ARM: i32 = 38;
}

fn new_entity(name: &str) -> i32 {
    d::NEW_ENTITY_NAMES.iter().position(|n| *n == name).map_or(-1, |p| p as i32)
}

#[derive(Default)]
pub struct Entities {
    /// entity id → 1.21.11 entity type id
    types: HashMap<i32, i32>,
}

impl Entities {
    pub fn clear(&mut self) {
        self.types.clear();
    }

    /// ADD_ENTITY: only the type id changes (block states are identical in both versions).
    pub fn add_entity(&mut self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let entity_id = r.varint()?;
        w.varint(entity_id);
        w.uuid(r.uuid()?);
        let t = r.varint()?;
        self.types.insert(entity_id, t);
        w.varint(items.entity(t));
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

    /// SET_ENTITY_DATA. `game_time` is the last time the server sent, used for anger timers.
    pub fn set_entity_data(&self, items: &Items, misc: &Misc, game_time: i64, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let entity_id = r.varint()?;
        w.varint(entity_id);
        let etype = self.types.get(&entity_id).copied().unwrap_or(-1);
        let is_wolf = etype == new_entity("wolf");
        let is_bee = etype == new_entity("bee");
        let is_zombie_nautilus = etype == new_entity("zombie_nautilus");
        let is_nautilus = is_zombie_nautilus || etype == new_entity("nautilus");
        loop {
            let index = r.u8()?;
            if index == 0xff {
                w.u8(0xff);
                break;
            }
            let t = r.varint()?;
            let mut value = Writer::new();
            Self::entity_data_value(items, misc, t, &mut r, &mut value)?;
            // Nautilus tame flags, owner, dashing and variant have no place on a squid.
            if is_nautilus && (17..=19).contains(&index) || is_zombie_nautilus && index == 20 {
                continue;
            }
            if t == ty::ZOMBIE_NAUTILUS_VARIANT {
                continue;
            }
            if t == ty::VARLONG && ((is_wolf && index == 21) || (is_bee && index == 18)) {
                let end = Reader::new(&value.buf).varlong()?;
                let remaining = (end - game_time).clamp(0, i64::from(i32::MAX)) as i32;
                w.u8(index);
                w.varint(ty::VARINT);
                w.varint(remaining);
                continue;
            }
            if t == ty::HUMANOID_ARM {
                let arm = Reader::new(&value.buf).varint()?;
                w.u8(index);
                w.varint(ty::BYTE);
                w.u8(arm as u8);
                continue;
            }
            w.u8(index);
            w.varint(if t > ty::ZOMBIE_NAUTILUS_VARIANT { t - 1 } else { t });
            w.raw(&value.buf);
        }
        Ok(w.into_inner())
    }

    fn entity_data_value(items: &Items, misc: &Misc, t: i32, r: &mut Reader, w: &mut Writer) -> Result<()> {
        match t {
            ty::BYTE => {
                w.u8(r.u8()?);
            }
            ty::VARINT | ty::DIRECTION | ty::OPT_VARINT | 20..=28 | 31..=34 | ty::HUMANOID_ARM => {
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
            ty::BLOCK_STATE | ty::OPT_BLOCK_STATE => {
                w.varint(r.varint()?); // block states are identical
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
            ty::RESOLVABLE_PROFILE => items.resolvable_profile(r, w)?,
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

    /// UPDATE_MOB_EFFECT / REMOVE_MOB_EFFECT: `None` for breath of the nautilus (id 39).
    pub fn mob_effect(payload: &[u8]) -> Result<Option<Vec<u8>>> {
        let mut r = Reader::new(payload);
        r.varint()?;
        if r.varint()? == 39 {
            return Ok(None);
        }
        Ok(Some(payload.to_vec()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translate::v1_21_9::items::Dir;

    fn spawn(e: &mut Entities, id: i32, name: &str) -> Vec<u8> {
        let mut p = Writer::new();
        p.varint(id);
        p.uuid(1);
        p.varint(new_entity(name));
        p.raw(&[0u8; 24]);
        p.u8(0); // no movement
        p.raw(&[0, 0, 0]);
        p.varint(0);
        e.add_entity(&Items::new(Dir::ToClient), &p.buf).unwrap()
    }

    #[test]
    fn nautilus_becomes_squid() {
        let mut e = Entities::default();
        let out = spawn(&mut e, 5, "nautilus");
        let mut r = Reader::new(&out);
        r.varint().unwrap();
        r.uuid().unwrap();
        assert_eq!(d::OLD_ENTITY_NAMES[r.varint().unwrap() as usize], "squid");
    }

    #[test]
    fn entity_data_types_shift_and_filters_apply() {
        let mut e = Entities::default();
        spawn(&mut e, 7, "wolf");
        let mut p = Writer::new();
        p.varint(7);
        p.u8(21); // anger end time
        p.varint(ty::VARLONG);
        p.varlong(1_150);
        p.u8(9); // vector3f
        p.varint(ty::VECTOR3F);
        p.raw(&[0u8; 12]);
        p.u8(15); // main arm
        p.varint(ty::HUMANOID_ARM);
        p.varint(1);
        p.u8(0xff);
        let out = e.set_entity_data(&Items::new(Dir::ToClient), &Misc::new(), 1_000, &p.buf).unwrap();
        let mut r = Reader::new(&out);
        assert_eq!(r.varint().unwrap(), 7);
        assert_eq!(r.u8().unwrap(), 21);
        assert_eq!(r.varint().unwrap(), ty::VARINT);
        assert_eq!(r.varint().unwrap(), 150);
        assert_eq!(r.u8().unwrap(), 9);
        assert_eq!(r.varint().unwrap(), 34); // 1.21.9 vector3f
        r.bytes(12).unwrap();
        assert_eq!(r.u8().unwrap(), 15);
        assert_eq!(r.varint().unwrap(), ty::BYTE);
        assert_eq!(r.u8().unwrap(), 1);
        assert_eq!(r.u8().unwrap(), 0xff);
        assert!(r.is_empty());
    }

    #[test]
    fn nautilus_variant_is_dropped() {
        let mut e = Entities::default();
        spawn(&mut e, 3, "zombie_nautilus");
        let mut p = Writer::new();
        p.varint(3);
        p.u8(20);
        p.varint(ty::ZOMBIE_NAUTILUS_VARIANT);
        p.varint(0);
        p.u8(0xff);
        let out = e.set_entity_data(&Items::new(Dir::ToClient), &Misc::new(), 0, &p.buf).unwrap();
        assert_eq!(out, vec![3, 0xff]);
    }
}
