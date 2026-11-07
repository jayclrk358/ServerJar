//! Entities 1.21.9 → 1.21.7: spawning (low-precision movement vectors), motion, entity data
//! (metadata) types and player/avatar indexes, player rotation, spawn position, world border.
//!
//! Follows ViaBackwards' `EntityPacketRewriter1_21_9`. Copper golems are shown as frogs and
//! mannequins as fake tab-hidden players, like ViaBackwards does.

use std::collections::HashMap;

use super::data as d;
use super::ids;
use super::items::{Items, Reg};
use super::misc::Misc;
use crate::proto::buf::{DecodeError, Reader, Result, Writer};
use crate::proto::nbt::{self, Tag};

fn err(msg: &str) -> DecodeError {
    DecodeError::Other(msg.to_string())
}

/// Entity data value type ids in 1.21.9.
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
    pub const OPT_GLOBAL_POSITION: i32 = 28;
    pub const PAINTING_VARIANT: i32 = 29;
    pub const COPPER_GOLEM_STATE: i32 = 32;
    pub const WEATHERING_COPPER_STATE: i32 = 33;
    pub const VECTOR3F: i32 = 34;
    pub const QUATERNION: i32 = 35;
    pub const RESOLVABLE_PROFILE: i32 = 36;
    /// 1.21.7 only: used for shoulder parrots.
    pub const OLD_COMPOUND_TAG: i32 = 16;
}

/// 1.21.9 entity data type id → 1.21.7 id (`None`: the old client has no such type).
fn map_data_type(t: i32) -> Option<i32> {
    match t {
        0..=15 => Some(t),
        16..=31 => Some(t + 1), // 1.21.7 has the compound tag type at 16
        ty::COPPER_GOLEM_STATE | ty::WEATHERING_COPPER_STATE | ty::RESOLVABLE_PROFILE => None,
        ty::VECTOR3F | ty::QUATERNION => Some(t - 1),
        _ => None,
    }
}

fn entity_id(name: &str) -> i32 {
    d::NEW_ENTITY_NAMES.iter().position(|n| *n == name).map_or(-1, |p| p as i32)
}

/// Decodes a 1.21.9 low-precision vector (ViaVersion's `LowPrecisionVectorType`).
pub fn read_low_precision_vector(r: &mut Reader) -> Result<(f64, f64, f64)> {
    let first = u64::from(r.u8()?);
    if first == 0 {
        return Ok((0.0, 0.0, 0.0));
    }
    let second = u64::from(r.u8()?);
    let remaining = u64::from(r.i32()? as u32);
    let packed = (remaining << 16) | (second << 8) | first;
    let mut scale = first & 3;
    if first & 4 != 0 {
        scale |= (u64::from(r.varint()? as u32)) << 2;
    }
    let unpack = |v: u64| -> f64 {
        let clamped = ((v & 0x7fff) as f64).min(32766.0);
        clamped * 2.0 / 32766.0 - 1.0
    };
    let s = scale as f64;
    Ok((unpack(packed >> 3) * s, unpack(packed >> 18) * s, unpack(packed >> 33) * s))
}

/// Legacy velocity short (1/8000 block per tick), rounded like ViaBackwards does.
fn legacy_velocity(v: f64) -> i16 {
    (v * 8000.0).round().clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}

fn write_velocity(w: &mut Writer, (x, y, z): (f64, f64, f64)) {
    w.i16(legacy_velocity(x));
    w.i16(legacy_velocity(y));
    w.i16(legacy_velocity(z));
}

/// A mannequin shown as a player: it needs a (hidden) tab list entry to render.
struct Mannequin {
    uuid: u128,
}

pub struct Entities {
    /// entity id → 1.21.9 entity type id
    types: HashMap<i32, i32>,
    mannequins: HashMap<i32, Mannequin>,
    /// Last absolute rotation the client reported (for relative PLAYER_ROTATION).
    yaw: f32,
    pitch: f32,
    /// `coordinate_scale` per dimension type registry index.
    dimension_scales: Vec<f64>,
    current_dimension: i32,
}

impl Default for Entities {
    fn default() -> Self {
        Self { types: HashMap::new(), mannequins: HashMap::new(), yaw: 0.0, pitch: 0.0, dimension_scales: Vec::new(), current_dimension: -1 }
    }
}

impl Entities {
    pub fn clear(&mut self) {
        self.types.clear();
        self.mannequins.clear();
    }

    pub fn set_dimension_scales(&mut self, scales: Vec<f64>) {
        self.dimension_scales = scales;
    }

    fn scale(&self) -> f64 {
        usize::try_from(self.current_dimension).ok().and_then(|i| self.dimension_scales.get(i)).copied().unwrap_or(1.0)
    }

    /// ADD_ENTITY. Output packets carry 1.21.7 ids.
    pub fn add_entity(&mut self, items: &Items, payload: &[u8], out: &mut Vec<(i32, Vec<u8>)>) -> Result<()> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len() + 6);
        let id = r.varint()?;
        let uuid = r.uuid()?;
        let ty = r.varint()?;
        self.types.insert(id, ty);
        w.varint(id);
        w.uuid(uuid);
        w.varint(items.entity(ty));
        w.raw(r.bytes(24)?); // x y z
        let movement = read_low_precision_vector(&mut r)?;
        w.raw(r.bytes(3)?); // pitch yaw head yaw
        let data = r.varint()?;
        w.varint(if ty == entity_id("falling_block") { items.block_state(data) } else { data });
        write_velocity(&mut w, movement);
        if ty == entity_id("mannequin") {
            self.mannequins.insert(id, Mannequin { uuid });
            out.push((ids::old::PLAYER_INFO_UPDATE, mannequin_player_info(uuid, id)));
        }
        out.push((ids::old::ADD_ENTITY, w.into_inner()));
        Ok(())
    }

    /// REMOVE_ENTITIES: also drops the tab list entries of removed mannequins.
    pub fn remove_entities(&mut self, payload: &[u8], out: &mut Vec<(i32, Vec<u8>)>) -> Result<()> {
        let mut r = Reader::new(payload);
        let mut uuids = Vec::new();
        for id in r.list(|r| r.varint())? {
            self.types.remove(&id);
            if let Some(m) = self.mannequins.remove(&id) {
                uuids.push(m.uuid);
            }
        }
        out.push((ids::old::REMOVE_ENTITIES, payload.to_vec()));
        if !uuids.is_empty() {
            let mut w = Writer::new();
            w.varint(uuids.len() as i32);
            for u in uuids {
                w.uuid(u);
            }
            out.push((ids::old::PLAYER_INFO_REMOVE, w.into_inner()));
        }
        Ok(())
    }

    pub fn set_entity_motion(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        w.varint(r.varint()?);
        write_velocity(&mut w, read_low_precision_vector(&mut r)?);
        Ok(w.into_inner())
    }

    /// PLAYER_ROTATION: 1.21.9 can rotate relatively; 1.21.7 only knows absolute angles.
    pub fn player_rotation(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut yaw = r.f32()?;
        if r.bool()? {
            yaw += self.yaw;
        }
        let mut pitch = r.f32()?;
        if r.bool()? {
            pitch += self.pitch;
        }
        self.yaw = yaw;
        self.pitch = pitch;
        let mut w = Writer::new();
        w.f32(yaw);
        w.f32(pitch);
        Ok(w.into_inner())
    }

    /// Serverbound MOVE_PLAYER_POS_ROT / MOVE_PLAYER_ROT: remember the rotation.
    pub fn store_rotation(&mut self, payload: &[u8], with_position: bool) {
        let mut r = Reader::new(payload);
        if with_position && r.bytes(24).is_err() {
            return;
        }
        if let (Ok(yaw), Ok(pitch)) = (r.f32(), r.f32()) {
            self.yaw = yaw;
            self.pitch = pitch;
        }
    }

    /// SET_DEFAULT_SPAWN_POSITION: global position + yaw + pitch → block position + yaw.
    pub fn default_spawn_position(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        r.string()?; // dimension
        let mut w = Writer::new();
        w.i64(r.i64()?);
        w.raw(r.bytes(4)?); // yaw
        Ok(w.into_inner())
    }

    /// INITIALIZE_BORDER / SET_BORDER_CENTER: 1.21.9 sends the center in the dimension's own
    /// coordinates; older clients expect it multiplied by the dimension's coordinate scale.
    pub fn border_center(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let scale = self.scale();
        let mut w = Writer::with_capacity(payload.len());
        w.f64(r.f64()? * scale);
        w.f64(r.f64()? * scale);
        w.raw(r.remaining());
        Ok(w.into_inner())
    }

    pub fn login(&mut self, payload: &[u8]) -> Result<()> {
        let mut r = Reader::new(payload);
        r.i32()?;
        r.bool()?;
        r.list(|r| r.string().map(|_| ()))?;
        for _ in 0..3 {
            r.varint()?;
        }
        r.bytes(3)?;
        self.current_dimension = r.varint()?;
        Ok(())
    }

    pub fn respawn(&mut self, payload: &[u8]) -> Result<()> {
        self.current_dimension = Reader::new(payload).varint()?;
        Ok(())
    }

    /// SET_ENTITY_DATA: remap value types, ids inside values and moved player indexes.
    pub fn set_entity_data(&self, items: &Items, misc: &Misc, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let id = r.varint()?;
        w.varint(id);
        let ty = self.types.get(&id).copied().unwrap_or(-1);
        let is_player = ty == entity_id("player");
        let is_mannequin = ty == entity_id("mannequin");
        loop {
            let index = r.u8()?;
            if index == 0xff {
                w.u8(0xff);
                break;
            }
            let t = r.varint()?;
            let mut value = Writer::new();
            Self::entity_data_value(items, misc, t, &mut r, &mut value)?;
            let mut index = index;
            let mut out_type = map_data_type(t);
            if is_player || is_mannequin {
                match index {
                    15 => index = 18, // main hand
                    16 => index = 17, // model customisation
                    17 if is_player => index = 15, // extra hearts
                    18 if is_player => index = 16, // score
                    17..=19 if is_mannequin => out_type = None, // profile, immovable, description
                    19 | 20 if is_player && t == ty::OPT_VARINT => {
                        // shoulder parrot: optional variant (varint, 0 = none, else id + 1)
                        // → entity compound
                        let variant = Reader::new(&value.buf).varint()?;
                        let mut tag = Tag::compound();
                        if variant != 0 {
                            tag.insert("id", Tag::String("parrot".into()));
                            tag.insert("Variant", Tag::Int(variant - 1));
                        }
                        value = Writer::new();
                        nbt::write(&mut value, &tag);
                        out_type = Some(ty::OLD_COMPOUND_TAG);
                    }
                    _ => {}
                }
            }
            if let Some(t) = out_type {
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
            ty::VARINT | ty::DIRECTION | ty::OPT_VARINT | 20..=27 | 30..=33 => {
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
            ty::PAINTING_VARIANT => items.holder(r, w, Reg::Same, |_, r, w| {
                Items::copy_varint(r, w)?;
                Items::copy_varint(r, w)?;
                Items::copy_string(r, w)?;
                Items::copy_opt_nbt(r, w)?;
                Items::copy_opt_nbt(r, w)
            })?,
            ty::RESOLVABLE_PROFILE => super::items::resolvable_profile_to_game_profile(r, w)?,
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
}

/// PLAYER_INFO_UPDATE adding an unlisted entry so the client can render a mannequin as a
/// player. The name is a run of colour codes, which renders as an empty name tag.
fn mannequin_player_info(uuid: u128, entity_id: i32) -> Vec<u8> {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut name = String::new();
    for i in 0..7 {
        name.push('§');
        name.push(HEX[((entity_id as u32 >> (i * 4)) & 0xf) as usize] as char);
    }
    name.push_str("§r");
    let mut w = Writer::new();
    w.u8(0xff); // all eight actions
    w.varint(1);
    w.uuid(uuid);
    w.string(&name); // add player: name
    w.varint(0); // properties
    w.bool(false); // chat session
    w.varint(0); // game mode
    w.bool(false); // listed
    w.varint(0); // latency
    w.bool(false); // display name
    w.varint(1000); // list order
    w.bool(true); // show hat
    w.into_inner()
}

#[cfg(test)]
mod tests {
    use super::super::items::Dir;
    use super::*;

    #[test]
    fn low_precision_vector_decodes() {
        assert_eq!(read_low_precision_vector(&mut Reader::new(&[0])).unwrap(), (0.0, 0.0, 0.0));
        // scale 1, all three components at the midpoint (16383) = 0, then x at max.
        let mid: u64 = 16383;
        let max: u64 = 32766;
        let packed: u64 = 1 | (max << 3) | (mid << 18) | (mid << 33);
        let bytes = [(packed & 0xff) as u8, ((packed >> 8) & 0xff) as u8];
        let rest = ((packed >> 16) as u32).to_be_bytes();
        let mut buf = bytes.to_vec();
        buf.extend_from_slice(&rest);
        let (x, y, z) = read_low_precision_vector(&mut Reader::new(&buf)).unwrap();
        assert!((x - 1.0).abs() < 1e-9);
        assert!(y.abs() < 1e-4 && z.abs() < 1e-4);
        assert_eq!(legacy_velocity(x), 8000);
    }

    #[test]
    fn add_entity_moves_velocity_to_the_end() {
        let items = Items::new(Dir::ToClient);
        let mut e = Entities::default();
        let mut p = Writer::new();
        p.varint(42);
        p.uuid(7);
        p.varint(0); // acacia_boat
        p.raw(&[0u8; 24]);
        p.u8(0); // zero movement
        p.raw(&[1, 2, 3]);
        p.varint(5);
        let mut out = Vec::new();
        e.add_entity(&items, &p.buf, &mut out).unwrap();
        assert_eq!(out.len(), 1);
        let mut r = Reader::new(&out[0].1);
        assert_eq!(r.varint().unwrap(), 42);
        assert_eq!(r.uuid().unwrap(), 7);
        assert_eq!(r.varint().unwrap(), 0);
        r.bytes(24).unwrap();
        assert_eq!(r.bytes(3).unwrap(), &[1, 2, 3]);
        assert_eq!(r.varint().unwrap(), 5);
        assert_eq!(r.remaining(), &[0u8; 6]);
    }

    #[test]
    fn mannequin_spawns_with_player_info() {
        let items = Items::new(Dir::ToClient);
        let mut e = Entities::default();
        let mut p = Writer::new();
        p.varint(9);
        p.uuid(77);
        p.varint(entity_id("mannequin"));
        p.raw(&[0u8; 24]);
        p.u8(0);
        p.raw(&[0, 0, 0]);
        p.varint(0);
        let mut out = Vec::new();
        e.add_entity(&items, &p.buf, &mut out).unwrap();
        assert_eq!(out[0].0, ids::old::PLAYER_INFO_UPDATE);
        assert_eq!(out[1].0, ids::old::ADD_ENTITY);
        let mut r = Reader::new(&out[1].1);
        r.varint().unwrap();
        r.uuid().unwrap();
        assert_eq!(r.varint().unwrap(), d::OLD_ENTITY_NAMES.iter().position(|n| *n == "player").unwrap() as i32);
        let mut rm = Writer::new();
        rm.varint(1);
        rm.varint(9);
        let mut out = Vec::new();
        e.remove_entities(&rm.buf, &mut out).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].0, ids::old::PLAYER_INFO_REMOVE);
    }

    #[test]
    fn player_indexes_and_types_move() {
        let items = Items::new(Dir::ToClient);
        let misc = Misc::new();
        let mut e = Entities::default();
        e.types.insert(1, entity_id("player"));
        let mut p = Writer::new();
        p.varint(1);
        p.u8(15); // main hand
        p.varint(ty::BYTE);
        p.u8(1);
        p.u8(19); // left shoulder parrot variant 2
        p.varint(ty::OPT_VARINT);
        p.varint(3); // variant 2
        p.u8(6); // pose (type 20 → 21)
        p.varint(20);
        p.varint(0);
        p.u8(0xff);
        let out = e.set_entity_data(&items, &misc, &p.buf).unwrap();
        let mut r = Reader::new(&out);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!((r.u8().unwrap(), r.varint().unwrap(), r.u8().unwrap()), (18, 0, 1));
        assert_eq!((r.u8().unwrap(), r.varint().unwrap()), (19, ty::OLD_COMPOUND_TAG));
        let tag = nbt::read(&mut r).unwrap();
        assert_eq!(tag.get("Variant").and_then(Tag::as_i32), Some(2));
        assert_eq!((r.u8().unwrap(), r.varint().unwrap(), r.varint().unwrap()), (6, 21, 0));
        assert_eq!(r.u8().unwrap(), 0xff);
    }

    #[test]
    fn relative_rotation_is_resolved() {
        let mut e = Entities::default();
        let mut s = Writer::new();
        s.f32(10.0);
        s.f32(5.0);
        s.u8(0);
        e.store_rotation(&s.buf, false);
        let mut p = Writer::new();
        p.f32(15.0);
        p.bool(true);
        p.f32(-20.0);
        p.bool(false);
        let out = e.player_rotation(&p.buf).unwrap();
        let mut r = Reader::new(&out);
        assert_eq!(r.f32().unwrap(), 25.0);
        assert_eq!(r.f32().unwrap(), -20.0);
    }
}
