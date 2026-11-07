//! Entities 26.2 → 26.1: spawning, entity data (metadata), login, attributes.
//!
//! 26.2 added the sulfur cube (shown to 26.1 clients as a named slime) and gave every cube mob
//! two extra data fields (baby, age locked) before its size.

use std::collections::HashMap;

use super::data_26_1 as d;
use super::items::{Items, Reg};
use super::misc::Misc;
use crate::proto::buf::{DecodeError, Reader, Result, Writer};
use crate::proto::nbt::{self, Tag};

fn err(msg: &str) -> DecodeError {
    DecodeError::Other(msg.to_string())
}

/// Entity data value types (identical lists in 26.2 and 26.1).
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
    pub const OPT_GLOBAL_POSITION: i32 = 33;
    pub const PAINTING_VARIANT: i32 = 34;
    pub const VECTOR3F: i32 = 39;
    pub const QUATERNION: i32 = 40;
    pub const RESOLVABLE_PROFILE: i32 = 41;
    pub const HUMANOID_ARM: i32 = 42;
}

fn entity_type(name: &str) -> i32 {
    d::NEW_ENTITY_NAMES.iter().position(|n| *n == name).map_or(-1, |p| p as i32)
}

#[derive(Default)]
pub struct Entities {
    /// entity id → 26.2 entity type id
    types: HashMap<i32, i32>,
}

impl Entities {
    pub fn clear(&mut self) {
        self.types.clear();
    }

    fn is_cube_mob(ty: i32) -> bool {
        ty == entity_type("slime") || ty == entity_type("magma_cube") || ty == entity_type("sulfur_cube")
    }

    /// ADD_ENTITY (26.2 → 26.1). Sulfur cubes become slimes carrying their original name.
    pub fn add_entity(&mut self, items: &Items, payload: &[u8], out: &mut Vec<(i32, Vec<u8>)>) -> Result<()> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let entity_id = r.varint()?;
        w.varint(entity_id);
        w.uuid(r.uuid()?);
        let ty = r.varint()?;
        self.types.insert(entity_id, ty);
        w.varint(items.entity(ty));
        w.raw(r.bytes(24)?); // x y z
        Self::copy_movement(&mut r, &mut w)?; // low-precision velocity (1.21.9+)
        w.raw(r.bytes(3)?); // pitch yaw head
        let data = r.varint()?;
        w.varint(if ty == entity_type("falling_block") { items.block_state(data) } else { data });
        w.raw(r.remaining());
        out.push((super::ids::new::ADD_ENTITY, w.into_inner()));
        if ty == entity_type("sulfur_cube") {
            // Tag the stand-in slime with the real mob's name (index 2 custom name, 3 visible).
            let mut m = Writer::new();
            m.varint(entity_id);
            m.u8(2);
            m.varint(ty::OPT_COMPONENT);
            m.bool(true);
            nbt::write(&mut m, &Tag::String("Sulfur Cube".into()));
            m.u8(3);
            m.varint(ty::BOOL);
            m.bool(true);
            m.u8(0xff);
            out.push((super::ids::old::SET_ENTITY_DATA, m.into_inner()));
        }
        Ok(())
    }

    /// Low-precision vector (1.21.9+): one 0 byte for zero, else 2 + 4 bytes plus an optional
    /// varint scale tail when bit 2 of the first byte is set.
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

    /// SET_ENTITY_DATA: remap ids inside values; squeeze out the cube mob fields 26.1 lacks.
    pub fn set_entity_data(&self, items: &Items, misc: &Misc, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let entity_id = r.varint()?;
        w.varint(entity_id);
        let ty_id = self.types.get(&entity_id).copied().unwrap_or(-1);
        let cube_mob = Self::is_cube_mob(ty_id);
        let sulfur_cube = ty_id == entity_type("sulfur_cube");
        loop {
            let index = r.u8()?;
            if index == 0xff {
                w.u8(0xff);
                break;
            }
            let t = r.varint()?;
            let mut value = Writer::new();
            Self::entity_data_value(items, misc, t, &mut r, &mut value)?;
            let (drop, index) = if cube_mob {
                match index {
                    16 | 17 => (true, index), // baby, age locked
                    19 | 20 if sulfur_cube => (true, index), // max fuse, from bucket
                    i if i >= 18 => (false, i - 2),
                    i => (false, i),
                }
            } else {
                (false, index)
            };
            if !drop {
                w.u8(index);
                w.varint(t);
                w.raw(&value.buf);
            }
        }
        Ok(w.into_inner())
    }

    fn entity_data_value(items: &Items, misc: &Misc, t: i32, r: &mut Reader, w: &mut Writer) -> Result<()> {
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

    /// Common player spawn info (LOGIN tail and RESPAWN). Returns the dimension type id and
    /// strips the trailing "online mode" flag 26.2 added to LOGIN.
    fn spawn_info(r: &mut Reader, w: &mut Writer, login: bool) -> Result<i32> {
        let dimension_type = r.varint()?;
        w.varint(dimension_type);
        w.string(r.string()?); // dimension name
        w.i64(r.i64()?); // seed
        w.raw(r.bytes(2)?); // gamemode, previous gamemode
        w.raw(r.bytes(2)?); // debug, flat
        if Items::copy_bool(r, w)? {
            w.string(r.string()?); // death dimension
            w.i64(r.i64()?); // death position
        }
        w.varint(r.varint()?); // portal cooldown
        w.varint(r.varint()?); // sea level
        if login {
            r.bool()?; // online mode (26.2+)
        }
        w.raw(r.remaining()); // respawn: data kept byte
        Ok(dimension_type)
    }

    pub fn login(payload: &[u8]) -> Result<(Vec<u8>, i32)> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.i32(r.i32()?);
        w.bool(r.bool()?);
        let n = r.varint()?;
        w.varint(n);
        for _ in 0..n.max(0) {
            w.string(r.string()?);
        }
        for _ in 0..3 {
            w.varint(r.varint()?); // max players, view distance, simulation distance
        }
        w.raw(r.bytes(3)?); // reduced debug, respawn screen, limited crafting
        let dim = Self::spawn_info(&mut r, &mut w, true)?;
        Ok((w.into_inner(), dim))
    }

    pub fn respawn(payload: &[u8]) -> Result<(Vec<u8>, i32)> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let dim = Self::spawn_info(&mut r, &mut w, false)?;
        Ok((w.into_inner(), dim))
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

    /// UPDATE_ATTRIBUTES: drop attributes the old client doesn't know.
    pub fn update_attributes(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.varint(r.varint()?);
        let n = r.varint()?;
        let mut body = Writer::new();
        let mut kept = 0;
        for _ in 0..n.max(0) {
            let attr = r.varint()?;
            let mut entry = Writer::new();
            entry.f64(r.f64()?);
            let modifiers = r.varint()?;
            entry.varint(modifiers);
            for _ in 0..modifiers.max(0) {
                entry.string(r.string()?);
                entry.f64(r.f64()?);
                entry.u8(r.u8()?);
            }
            if let Some(mapped) = items.attribute(attr) {
                body.varint(mapped);
                body.raw(&entry.buf);
                kept += 1;
            }
        }
        w.varint(kept);
        w.raw(&body.buf);
        Ok(w.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::super::items::Dir;
    use super::*;

    #[test]
    fn sulfur_cube_spawns_as_named_slime() {
        let items = Items::new(Dir::ToClient);
        let mut e = Entities::default();
        let mut p = Writer::new();
        p.varint(42);
        p.uuid(7);
        p.varint(entity_type("sulfur_cube"));
        p.raw(&[0u8; 24]);
        p.u8(0);
        p.raw(&[1, 2, 3]);
        p.varint(0);
        let mut out = Vec::new();
        e.add_entity(&items, &p.buf, &mut out).unwrap();
        assert_eq!(out.len(), 2);
        let mut r = Reader::new(&out[0].1);
        assert_eq!(r.varint().unwrap(), 42);
        r.uuid().unwrap();
        let slime = d::OLD_ENTITY_NAMES.iter().position(|n| *n == "slime").unwrap() as i32;
        assert_eq!(r.varint().unwrap(), slime);
        assert_eq!(out[1].0, super::super::ids::old::SET_ENTITY_DATA);
    }

    #[test]
    fn cube_mob_metadata_is_squeezed() {
        let items = Items::new(Dir::ToClient);
        let misc = Misc::new();
        let mut e = Entities::default();
        e.types.insert(5, entity_type("sulfur_cube"));
        let mut p = Writer::new();
        p.varint(5);
        p.u8(15); // some mob field
        p.varint(ty::BYTE);
        p.u8(1);
        p.u8(16); // baby
        p.varint(ty::BOOL);
        p.bool(false);
        p.u8(17); // age locked
        p.varint(ty::BOOL);
        p.bool(false);
        p.u8(18); // size
        p.varint(ty::VARINT);
        p.varint(3);
        p.u8(19); // max fuse
        p.varint(ty::VARINT);
        p.varint(30);
        p.u8(0xff);
        let out = e.set_entity_data(&items, &misc, &p.buf).unwrap();
        let mut r = Reader::new(&out);
        assert_eq!(r.varint().unwrap(), 5);
        assert_eq!(r.u8().unwrap(), 15);
        assert_eq!(r.varint().unwrap(), ty::BYTE);
        assert_eq!(r.u8().unwrap(), 1);
        assert_eq!(r.u8().unwrap(), 16); // size moved down
        assert_eq!(r.varint().unwrap(), ty::VARINT);
        assert_eq!(r.varint().unwrap(), 3);
        assert_eq!(r.u8().unwrap(), 0xff);
        assert!(r.is_empty());
    }

    #[test]
    fn login_drops_online_mode_flag() {
        let mut p = Writer::new();
        p.i32(1); // entity id
        p.bool(false); // hardcore
        p.varint(1);
        p.string("minecraft:overworld");
        p.varint(20);
        p.varint(10);
        p.varint(10);
        p.raw(&[0, 1, 0]);
        p.varint(2); // dimension type
        p.string("minecraft:overworld");
        p.i64(99);
        p.raw(&[0, 0xff]); // gamemodes
        p.raw(&[0, 0]); // debug, flat
        p.bool(false); // no death location
        p.varint(0);
        p.varint(63);
        p.bool(true); // online mode
        let (out, dim) = Entities::login(&p.buf).unwrap();
        assert_eq!(dim, 2);
        assert_eq!(out.len(), p.buf.len() - 1);
        assert_eq!(&out[..], &p.buf[..p.buf.len() - 1]);
    }

    #[test]
    fn unknown_attributes_are_removed() {
        let items = Items::new(Dir::ToClient);
        let bounciness = d::NEW_ATTRIBUTE_NAMES.iter().position(|n| *n == "bounciness").unwrap() as i32;
        let armor = d::NEW_ATTRIBUTE_NAMES.iter().position(|n| *n == "armor").unwrap() as i32;
        let armor_old = d::OLD_ATTRIBUTE_NAMES.iter().position(|n| *n == "armor").unwrap() as i32;
        let mut p = Writer::new();
        p.varint(9);
        p.varint(2);
        p.varint(bounciness);
        p.f64(1.0);
        p.varint(0);
        p.varint(armor);
        p.f64(20.0);
        p.varint(1);
        p.string("minecraft:m");
        p.f64(2.0);
        p.u8(0);
        let out = Entities::update_attributes(&items, &p.buf).unwrap();
        let mut r = Reader::new(&out);
        assert_eq!(r.varint().unwrap(), 9);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), armor_old);
        assert_eq!(r.f64().unwrap(), 20.0);
    }
}
