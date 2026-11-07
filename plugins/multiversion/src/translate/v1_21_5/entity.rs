//! Entities 1.21.6 → 1.21.5: spawning, entity data (metadata), equipment.
//!
//! Follows the vanilla changes between these versions: happy ghasts become ghasts (babies are
//! shrunk with the scale attribute), hanging entities lose their direction field (index 8), and
//! non-living entities spawned with a velocity get an extra motion packet because the 1.21.5
//! client ignores the spawn velocity for them.

use std::collections::HashMap;

use super::super::data_1_21_5 as d;
use super::ids;
use super::items::{copy_bool, copy_nbt, copy_opt_nbt, copy_string, copy_varint, err, Items, Reg};
use super::misc::Misc;
use crate::proto::buf::{Reader, Result, Writer};

/// Entity data value types (1.21.5 and 1.21.6 share the list).
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
    // 21 pose, 22..=28 variant ids: plain varints
    pub const OPT_GLOBAL_POSITION: i32 = 29;
    pub const PAINTING_VARIANT: i32 = 30;
    // 31 sniffer state, 32 armadillo state: plain varints
    pub const VECTOR3F: i32 = 33;
    pub const QUATERNION: i32 = 34;
}

/// Concrete 1.21.6 entity types that are living entities (vanilla 1.21.6 entity types).
const LIVING: &[&str] = &[
    "armor_stand", "player", "ender_dragon", "slime", "magma_cube", "bat", "ghast", "phantom", "allay", "snow_golem", "iron_golem",
    "shulker", "villager", "wandering_trader", "axolotl", "chicken", "panda", "pig", "polar_bear", "rabbit", "sheep", "bee", "turtle",
    "fox", "frog", "goat", "hoglin", "strider", "sniffer", "armadillo", "happy_ghast", "ocelot", "cow", "mooshroom", "cat", "wolf",
    "parrot", "horse", "skeleton_horse", "zombie_horse", "camel", "donkey", "mule", "llama", "trader_llama", "blaze", "creeper",
    "endermite", "enderman", "giant", "silverfish", "vex", "wither", "breeze", "zoglin", "warden", "creaking", "skeleton", "stray",
    "wither_skeleton", "bogged", "zombie", "drowned", "husk", "zombified_piglin", "zombie_villager", "guardian", "elder_guardian",
    "spider", "cave_spider", "piglin", "piglin_brute", "dolphin", "squid", "glow_squid", "pufferfish", "tadpole", "cod", "salmon",
    "tropical_fish", "witch", "ravager", "vindicator", "pillager", "evoker", "illusioner",
];

const HANGING: &[&str] = &["leash_knot", "painting", "item_frame", "glow_item_frame"];

/// Scale that turns a 4x4 ghast into a roughly 1x1 ghastling.
const GHASTLING_SCALE: f64 = 0.2375;

fn type_name(ty: i32) -> &'static str {
    usize::try_from(ty).ok().and_then(|i| d::NEW_ENTITY_NAMES.get(i)).copied().unwrap_or("")
}

#[derive(Default)]
pub struct Entities {
    /// entity id → 1.21.6 entity type id
    types: HashMap<i32, i32>,
    /// happy ghasts currently shown as babies
    baby_ghasts: HashMap<i32, bool>,
}

impl Entities {
    pub fn clear(&mut self) {
        self.types.clear();
        self.baby_ghasts.clear();
    }

    /// ADD_ENTITY. Produces (1.21.5 packet id, payload) pairs.
    pub fn add_entity(&mut self, items: &Items, payload: &[u8], out: &mut Vec<(i32, Vec<u8>)>) -> Result<()> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let entity_id = copy_varint(&mut r, &mut w)?;
        w.uuid(r.uuid()?);
        let ty = r.varint()?;
        self.types.insert(entity_id, ty);
        self.baby_ghasts.remove(&entity_id);
        w.varint(items.entity(ty));
        w.raw(r.bytes(24 + 3)?); // x y z, pitch yaw head yaw
        let data = r.varint()?;
        let name = type_name(ty);
        w.varint(if name == "falling_block" { items.block_state(data) } else { data });
        let velocity = r.bytes(6)?;
        w.raw(velocity);
        w.raw(r.remaining());
        out.push((ids::new::ADD_ENTITY, w.into_inner()));
        if velocity.iter().any(|b| *b != 0) && !LIVING.contains(&name) {
            let mut m = Writer::new();
            m.varint(entity_id);
            m.raw(velocity);
            out.push((ids::old::SET_ENTITY_MOTION, m.into_inner()));
        }
        Ok(())
    }

    pub fn remove_entities(&mut self, payload: &[u8]) -> Result<()> {
        let mut r = Reader::new(payload);
        for id in r.list(|r| r.varint())? {
            self.types.remove(&id);
            self.baby_ghasts.remove(&id);
        }
        Ok(())
    }

    /// SET_ENTITY_DATA: remap ids inside values and apply the per-type index changes. May add an
    /// UPDATE_ATTRIBUTES packet (ghastling scale). Returns (1.21.5 packet id, payload) pairs.
    pub fn set_entity_data(&mut self, items: &Items, misc: &Misc, payload: &[u8], out: &mut Vec<(i32, Vec<u8>)>) -> Result<()> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let entity_id = copy_varint(&mut r, &mut w)?;
        let name = self.types.get(&entity_id).map_or("", |t| type_name(*t));
        let hanging = HANGING.contains(&name);
        let happy_ghast = name == "happy_ghast";
        let mut baby = None;
        loop {
            let index = r.u8()?;
            if index == 0xff {
                w.u8(0xff);
                break;
            }
            let t = r.varint()?;
            let mut value = Writer::new();
            Self::value(items, misc, t, &mut r, &mut value)?;
            let index = if hanging {
                match index {
                    8 => continue,
                    i if i > 8 => i - 1,
                    i => i,
                }
            } else if happy_ghast && index >= 16 {
                // 16 baby (shown via scale), 17 leash holder, 18 stays still: no ghast fields.
                if index == 16 && t == ty::BOOL {
                    baby = Some(value.buf.first() == Some(&1));
                }
                continue;
            } else {
                index
            };
            w.u8(index);
            w.varint(t);
            w.raw(&value.buf);
        }
        out.push((ids::old::SET_ENTITY_DATA, w.into_inner()));
        if let Some(is_baby) = baby {
            let was = self.baby_ghasts.insert(entity_id, is_baby).unwrap_or(false);
            if was != is_baby {
                out.push((ids::old::UPDATE_ATTRIBUTES, scale_packet(entity_id, if is_baby { GHASTLING_SCALE } else { 1.0 })));
            }
        }
        Ok(())
    }

    fn value(items: &Items, misc: &Misc, t: i32, r: &mut Reader, w: &mut Writer) -> Result<()> {
        match t {
            ty::BYTE => {
                w.u8(r.u8()?);
            }
            ty::VARINT | ty::DIRECTION | ty::OPT_VARINT | 21..=28 | 31 | 32 => {
                copy_varint(r, w)?;
            }
            ty::VARLONG => {
                w.varlong(r.varlong()?);
            }
            ty::FLOAT => {
                w.raw(r.bytes(4)?);
            }
            ty::STRING => copy_string(r, w)?,
            ty::COMPONENT | ty::COMPOUND_TAG => copy_nbt(r, w)?,
            ty::OPT_COMPONENT => copy_opt_nbt(r, w)?,
            ty::ITEM => items.item_stack(r, w)?,
            ty::BOOL => {
                copy_bool(r, w)?;
            }
            ty::ROTATIONS | ty::VECTOR3F => {
                w.raw(r.bytes(12)?);
            }
            ty::QUATERNION => {
                w.raw(r.bytes(16)?);
            }
            ty::POSITION => {
                w.raw(r.bytes(8)?);
            }
            ty::OPT_POSITION => {
                if copy_bool(r, w)? {
                    w.raw(r.bytes(8)?);
                }
            }
            ty::OPT_UUID => {
                if copy_bool(r, w)? {
                    w.raw(r.bytes(16)?);
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
                let n = copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    misc.particle(items, r, w)?;
                }
            }
            ty::VILLAGER_DATA => {
                for _ in 0..3 {
                    copy_varint(r, w)?;
                }
            }
            ty::OPT_GLOBAL_POSITION => {
                if copy_bool(r, w)? {
                    copy_string(r, w)?;
                    w.raw(r.bytes(8)?);
                }
            }
            ty::PAINTING_VARIANT => items.holder(r, w, Reg::Same, |_, r, w| {
                copy_varint(r, w)?;
                copy_varint(r, w)?;
                copy_string(r, w)?;
                copy_opt_nbt(r, w)?;
                copy_opt_nbt(r, w)
            })?,
            other => return Err(err(&format!("unknown entity data type {other}"))),
        }
        Ok(())
    }

    pub fn set_equipment(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        copy_varint(&mut r, &mut w)?;
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

/// UPDATE_ATTRIBUTES (1.21.5) setting the scale attribute's base value.
fn scale_packet(entity_id: i32, scale: f64) -> Vec<u8> {
    let scale_id = d::OLD_ATTRIBUTE_NAMES.iter().position(|n| *n == "scale").unwrap_or(0) as i32;
    let mut w = Writer::new();
    w.varint(entity_id);
    w.varint(1);
    w.varint(scale_id);
    w.f64(scale);
    w.varint(0);
    w.into_inner()
}

#[cfg(test)]
mod tests {
    use super::super::items::Dir;
    use super::*;

    fn type_id(name: &str) -> i32 {
        d::NEW_ENTITY_NAMES.iter().position(|n| *n == name).unwrap() as i32
    }

    fn spawn(e: &mut Entities, items: &Items, id: i32, ty: i32, velocity: [u8; 6]) -> Vec<(i32, Vec<u8>)> {
        let mut p = Writer::new();
        p.varint(id);
        p.uuid(1);
        p.varint(ty);
        p.raw(&[0u8; 27]);
        p.varint(0);
        p.raw(&velocity);
        let mut out = Vec::new();
        e.add_entity(items, &p.buf, &mut out).unwrap();
        out
    }

    #[test]
    fn happy_ghast_becomes_ghast_and_babies_shrink() {
        let items = Items::new(Dir::ToClient);
        let mut e = Entities::default();
        let out = spawn(&mut e, &items, 5, type_id("happy_ghast"), [0; 6]);
        assert_eq!(out.len(), 1);
        let mut r = Reader::new(&out[0].1);
        r.varint().unwrap();
        r.uuid().unwrap();
        let ghast_old = d::OLD_ENTITY_NAMES.iter().position(|n| *n == "ghast").unwrap() as i32;
        assert_eq!(r.varint().unwrap(), ghast_old);

        let mut m = Writer::new();
        m.varint(5);
        m.u8(0);
        m.varint(ty::BYTE);
        m.u8(0);
        m.u8(16);
        m.varint(ty::BOOL);
        m.bool(true);
        m.u8(18);
        m.varint(ty::BOOL);
        m.bool(false);
        m.u8(0xff);
        let mut out = Vec::new();
        e.set_entity_data(&items, &Misc::new(), &m.buf, &mut out).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].1, vec![5, 0, 0, 0, 0xff]);
        assert_eq!(out[1].0, ids::old::UPDATE_ATTRIBUTES);
    }

    #[test]
    fn hanging_entities_lose_direction_index() {
        let items = Items::new(Dir::ToClient);
        let mut e = Entities::default();
        spawn(&mut e, &items, 9, type_id("item_frame"), [0; 6]);
        let mut m = Writer::new();
        m.varint(9);
        m.u8(8);
        m.varint(ty::DIRECTION);
        m.varint(3);
        m.u8(10);
        m.varint(ty::VARINT);
        m.varint(2);
        m.u8(0xff);
        let mut out = Vec::new();
        e.set_entity_data(&items, &Misc::new(), &m.buf, &mut out).unwrap();
        assert_eq!(out[0].1, vec![9, 9, ty::VARINT as u8, 2, 0xff]);
    }

    #[test]
    fn moving_non_living_spawn_gets_motion_packet() {
        let items = Items::new(Dir::ToClient);
        let mut e = Entities::default();
        let out = spawn(&mut e, &items, 3, type_id("arrow"), [0, 1, 0, 2, 0, 3]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].0, ids::old::SET_ENTITY_MOTION);
        assert_eq!(out[1].1, vec![3, 0, 1, 0, 2, 0, 3]);
        let out = spawn(&mut e, &items, 4, type_id("zombie"), [0, 1, 0, 2, 0, 3]);
        assert_eq!(out.len(), 1);
    }
}
