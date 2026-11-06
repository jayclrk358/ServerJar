//! Entities 26.3 ↔ 26.2: spawning, entity data (metadata), movement, login/respawn, animations.

use std::collections::HashMap;

use super::data_26_2 as d;
use super::items::{Dir, Items, Reg};
use super::misc::Misc;
use crate::proto::buf::{DecodeError, Reader, Result, Writer};

fn err(msg: &str) -> DecodeError {
    DecodeError::Other(msg.to_string())
}

/// Entity data value types (26.3 ids; 26.2 has the same list without 43 = dye color).
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
    pub const DYE_COLOR: i32 = 43;
}

#[derive(Default)]
pub struct Entities {
    /// entity id → 26.3 entity type id
    types: HashMap<i32, i32>,
}

impl Entities {
    pub fn clear(&mut self) {
        self.types.clear();
    }

    /// ADD_ENTITY (26.3 → 26.2). May produce a second packet (entity data) for cushions.
    pub fn add_entity(&mut self, items: &Items, payload: &[u8], out: &mut Vec<(i32, Vec<u8>)>) -> Result<()> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let entity_id = r.varint()?;
        w.varint(entity_id);
        w.uuid(r.uuid()?);
        let ty = r.varint()?;
        self.types.insert(entity_id, ty);
        let is_cushion = ty == d::CUSHION_ENTITY_NEW as i32;
        w.varint(if is_cushion { d::FALLING_BLOCK_ENTITY_OLD as i32 } else { items.entity(ty) });
        w.raw(r.bytes(24)?); // x y z
        w.raw(r.bytes(3)?); // pitch yaw head
        let data = r.varint()?;
        let falling_block_new = d::NEW_ENTITY_NAMES.iter().position(|n| *n == "falling_block").map_or(-1, |p| p as i32);
        let data = if is_cushion {
            d::WHITE_CARPET_STATE_OLD as i32
        } else if ty == falling_block_new {
            items.block_state(data)
        } else {
            data
        };
        w.varint(data);
        w.raw(r.remaining()); // velocity
        out.push((super::ids::new::ADD_ENTITY, w.into_inner()));
        if is_cushion {
            // A falling block would fall: tell the client it has no gravity (index 5, boolean).
            let mut m = Writer::new();
            m.varint(entity_id);
            m.u8(5);
            m.varint(ty::BOOL);
            m.bool(true);
            m.u8(0xff);
            out.push((super::ids::new::SET_ENTITY_DATA, m.into_inner()));
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

    /// SET_ENTITY_DATA: remap ids inside values, drop types the client doesn't have.
    pub fn set_entity_data(&self, items: &Items, misc: &Misc, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let entity_id = r.varint()?;
        w.varint(entity_id);
        let is_cushion = self.types.get(&entity_id) == Some(&(d::CUSHION_ENTITY_NEW as i32));
        loop {
            let index = r.u8()?;
            if index == 0xff {
                w.u8(0xff);
                break;
            }
            let t = r.varint()?;
            let mut value = Writer::new();
            Self::entity_data_value(items, misc, t, &mut r, &mut value)?;
            let drop = t == ty::DYE_COLOR || (is_cushion && index > 7);
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
            ty::VARINT | ty::DIRECTION | ty::OPT_VARINT | 20..=32 | 35..=38 | 42 | ty::DYE_COLOR => {
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

    /// MOVE_ENTITY_POS / MOVE_ENTITY_POS_ROT: 26.3 multi-step deltas → 26.2 single delta.
    pub fn move_entity(payload: &[u8], with_rotation: bool) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        w.varint(r.varint()?);
        let props = r.varint()?;
        let on_ground = props & 1 != 0;
        let steps = (props as u32) >> 1;
        let (mut dx, mut dy, mut dz) = (0i32, 0i32, 0i32);
        if steps == 0 {
            dx = i32::from(r.i16()?);
            dy = i32::from(r.i16()?);
            dz = i32::from(r.i16()?);
        } else {
            for _ in 0..steps {
                r.varint()?; // tick delay
                dx += i32::from(r.i16()?);
                dy += i32::from(r.i16()?);
                dz += i32::from(r.i16()?);
            }
        }
        w.i16(dx.clamp(i16::MIN.into(), i16::MAX.into()) as i16);
        w.i16(dy.clamp(i16::MIN.into(), i16::MAX.into()) as i16);
        w.i16(dz.clamp(i16::MIN.into(), i16::MAX.into()) as i16);
        if with_rotation {
            w.raw(r.bytes(2)?);
        }
        w.bool(on_ground);
        Ok(w.into_inner())
    }

    pub fn move_entity_rot(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        w.varint(r.varint()?);
        let on_ground = r.bool()?;
        w.raw(r.bytes(2)?);
        w.bool(on_ground);
        Ok(w.into_inner())
    }

    pub fn entity_position_sync(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        w.varint(r.varint()?);
        let path_type = r.varint()?;
        if path_type == 0 {
            w.raw(r.bytes(24)?); // x y z
            w.raw(&[0u8; 24]); // delta movement
        } else {
            let steps = r.varint()?;
            let (mut x, mut y, mut z) = (0f64, 0f64, 0f64);
            let (mut px, mut py, mut pz) = (0f64, 0f64, 0f64);
            let mut ticks = 0;
            for _ in 0..steps.max(0) {
                px = x;
                py = y;
                pz = z;
                x = r.f64()?;
                y = r.f64()?;
                z = r.f64()?;
                ticks = r.varint()?;
            }
            w.f64(x);
            w.f64(y);
            w.f64(z);
            if steps > 1 && ticks > 0 {
                let t = f64::from(ticks);
                w.f64((x - px) / t);
                w.f64((y - py) / t);
                w.f64((z - pz) / t);
            } else {
                w.raw(&[0u8; 24]);
            }
        }
        w.raw(r.remaining()); // yaw, pitch, on ground
        Ok(w.into_inner())
    }

    /// Gamemode pair: 26.3 varint + optional varint → 26.2 byte + signed byte.
    fn game_modes(r: &mut Reader, w: &mut Writer) -> Result<()> {
        let gm = r.varint()?;
        let prev = r.varint()?;
        w.u8(gm as u8);
        w.i8(if prev == 0 { -1 } else { (prev - 1) as i8 });
        Ok(())
    }

    /// Common player spawn info (shared by LOGIN and RESPAWN).
    fn spawn_info(r: &mut Reader, w: &mut Writer) -> Result<()> {
        w.varint(r.varint()?); // dimension type
        w.string(r.string()?); // dimension name
        w.i64(r.i64()?); // seed
        Self::game_modes(r, w)?;
        w.raw(r.remaining()); // debug, flat, death location, portal cooldown, sea level, ...
        Ok(())
    }

    pub fn login(payload: &[u8]) -> Result<Vec<u8>> {
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
        Self::spawn_info(&mut r, &mut w)?;
        Ok(w.into_inner())
    }

    pub fn respawn(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        Self::spawn_info(&mut r, &mut w)?;
        Ok(w.into_inner())
    }

    /// ANIMATE action ids shifted when swings moved to their own packet.
    pub fn animate(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        w.varint(r.varint()?);
        let action = r.u8()?;
        w.u8(match action {
            0 => 2, // wake up
            1 => 4, // crit
            2 => 5, // magic crit
            other => other,
        });
        Ok(w.into_inner())
    }

    /// SWING_ANIMATION → ANIMATE (arm swing), or nothing for "no animation".
    pub fn swing_animation(payload: &[u8]) -> Result<Option<Vec<u8>>> {
        let mut r = Reader::new(payload);
        let id = r.varint()?;
        let hand = r.varint()?;
        let anim_type = r.varint()?;
        let _duration = r.varint()?;
        if anim_type == 0 {
            return Ok(None);
        }
        let mut w = Writer::new();
        w.varint(id);
        w.u8(if hand == 0 { 0 } else { 3 });
        Ok(Some(w.into_inner()))
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

#[allow(dead_code)]
pub fn dir() -> Dir {
    Dir::ToClient
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn move_entity_sums_steps() {
        let mut p = Writer::new();
        p.varint(7);
        p.varint((2 << 1) | 1); // two steps, on ground
        p.varint(3);
        p.i16(10);
        p.i16(0);
        p.i16(-5);
        p.varint(3);
        p.i16(10);
        p.i16(1);
        p.i16(-5);
        let out = Entities::move_entity(&p.buf, false).unwrap();
        let mut r = Reader::new(&out);
        assert_eq!(r.varint().unwrap(), 7);
        assert_eq!(r.i16().unwrap(), 20);
        assert_eq!(r.i16().unwrap(), 1);
        assert_eq!(r.i16().unwrap(), -10);
        assert!(r.bool().unwrap());
        assert!(r.is_empty());
    }
    #[test]
    fn gamemodes() {
        let mut p = Writer::new();
        p.varint(1);
        p.varint(0);
        let mut w = Writer::new();
        Entities::game_modes(&mut Reader::new(&p.buf), &mut w).unwrap();
        assert_eq!(w.buf, vec![1, 0xff]);
    }
}
