//! Entities 1.21.5 → 1.21.4 (ViaBackwards' `EntityPacketRewriter1_21_5`): spawning, entity data,
//! saddles (an equipment slot in 1.21.5, entity data before), teams.

use std::collections::HashMap;

use super::data_1_21_4 as d;
use super::ids;
use super::items::{Items, Reg, Registries};
use super::misc::Misc;
use super::text;
use crate::proto::buf::{DecodeError, Reader, Result, Writer};

fn err(msg: &str) -> DecodeError {
    DecodeError::Other(msg.to_string())
}

/// Entity data value type ids, 1.21.5.
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
    pub const COMPOUND: i32 = 16;
    pub const PARTICLE: i32 = 17;
    pub const PARTICLES: i32 = 18;
    pub const VILLAGER_DATA: i32 = 19;
    pub const OPT_VARINT: i32 = 20;
    pub const POSE: i32 = 21;
    pub const CAT_VARIANT: i32 = 22;
    pub const COW_VARIANT: i32 = 23;
    pub const WOLF_VARIANT: i32 = 24;
    pub const WOLF_SOUND_VARIANT: i32 = 25;
    pub const FROG_VARIANT: i32 = 26;
    pub const PIG_VARIANT: i32 = 27;
    pub const CHICKEN_VARIANT: i32 = 28;
    pub const OPT_GLOBAL_POSITION: i32 = 29;
    pub const PAINTING_VARIANT: i32 = 30;
    pub const SNIFFER_STATE: i32 = 31;
    pub const ARMADILLO_STATE: i32 = 32;
    pub const VECTOR3F: i32 = 33;
    pub const QUATERNION: i32 = 34;
}

/// 1.21.5 data type → 1.21.4 data type (`None` = the old client has no such type).
fn old_type(t: i32) -> Option<i32> {
    match t {
        0..=22 => Some(t),
        ty::WOLF_VARIANT => Some(23),
        ty::FROG_VARIANT => Some(24),
        ty::COW_VARIANT | ty::WOLF_SOUND_VARIANT | ty::PIG_VARIANT | ty::CHICKEN_VARIANT => None,
        29..=34 => Some(t - 4),
        _ => None,
    }
}

const SADDLE_SLOT: i8 = 7;
const SADDLED_FLAG: u8 = 4;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Minecart,
    Horse,
    Mooshroom,
    Pig,
    Strider,
    Chicken,
    Cow,
    Wolf,
    ExperienceOrb,
    Dolphin,
    Turtle,
    FallingBlock,
    Other,
}

fn kind_of(type_id: i32) -> Kind {
    match d::NEW_ENTITY_NAMES.get(type_id.max(0) as usize).copied().unwrap_or("") {
        "minecart" | "chest_minecart" | "command_block_minecart" | "furnace_minecart" | "hopper_minecart" | "spawner_minecart"
        | "tnt_minecart" => Kind::Minecart,
        "horse" | "skeleton_horse" | "zombie_horse" | "camel" | "donkey" | "mule" | "llama" | "trader_llama" => Kind::Horse,
        "mooshroom" => Kind::Mooshroom,
        "pig" => Kind::Pig,
        "strider" => Kind::Strider,
        "chicken" => Kind::Chicken,
        "cow" => Kind::Cow,
        "wolf" => Kind::Wolf,
        "experience_orb" => Kind::ExperienceOrb,
        "dolphin" => Kind::Dolphin,
        "turtle" => Kind::Turtle,
        "falling_block" => Kind::FallingBlock,
        _ => Kind::Other,
    }
}

#[derive(Default)]
pub struct Entities {
    /// entity id → kind
    kinds: HashMap<i32, Kind>,
    /// horses: last flags byte from the server and whether a saddle is equipped
    horses: HashMap<i32, (u8, bool)>,
}

/// One entity data entry, already converted.
struct Entry {
    index: u8,
    t: i32,
    value: Vec<u8>,
}

impl Entities {
    pub fn clear(&mut self) {
        self.kinds.clear();
        self.horses.clear();
    }

    /// ADD_ENTITY. Experience orbs go back to their own packet (+ motion if moving).
    pub fn add_entity(&mut self, items: &Items, payload: &[u8], out: &mut Vec<(i32, Vec<u8>)>) -> Result<()> {
        let mut r = Reader::new(payload);
        let entity_id = r.varint()?;
        let uuid = r.uuid()?;
        let type_id = r.varint()?;
        let kind = kind_of(type_id);
        self.kinds.insert(entity_id, kind);
        let pos = r.bytes(24)?;
        if kind == Kind::ExperienceOrb {
            r.bytes(3)?; // pitch yaw head
            let value = r.varint()?;
            let vx = r.i16()?;
            let vy = r.i16()?;
            let vz = r.i16()?;
            let mut w = Writer::new();
            w.varint(entity_id);
            w.raw(pos);
            w.i16(value.clamp(0, i32::from(i16::MAX)) as i16);
            out.push((ids::old::ADD_EXPERIENCE_ORB, w.into_inner()));
            if vx != 0 || vy != 0 || vz != 0 {
                let mut m = Writer::new();
                m.varint(entity_id);
                m.i16(vx);
                m.i16(vy);
                m.i16(vz);
                out.push((ids::old::SET_ENTITY_MOTION, m.into_inner()));
            }
            return Ok(());
        }
        let mut w = Writer::with_capacity(payload.len());
        w.varint(entity_id);
        w.uuid(uuid);
        w.varint(items.entity(type_id));
        w.raw(pos);
        w.raw(r.bytes(3)?);
        let data = r.varint()?;
        w.varint(if kind == Kind::FallingBlock { items.block_state(data) } else { data });
        w.raw(r.remaining());
        let mapped = ids::play_clientbound(ids::new::ADD_ENTITY).ok_or_else(|| err("no add_entity id"))?;
        out.push((mapped, w.into_inner()));
        Ok(())
    }

    pub fn remove_entities(&mut self, payload: &[u8]) -> Result<()> {
        let mut r = Reader::new(payload);
        for id in r.list(|r| r.varint())? {
            self.kinds.remove(&id);
            self.horses.remove(&id);
        }
        Ok(())
    }

    /// SET_ENTITY_DATA. Returns `None` if nothing is left to send.
    pub fn set_entity_data(&mut self, items: &Items, misc: &Misc, regs: &Registries, payload: &[u8]) -> Result<Option<Vec<u8>>> {
        let mut r = Reader::new(payload);
        let entity_id = r.varint()?;
        let kind = self.kinds.get(&entity_id).copied().unwrap_or(Kind::Other);
        let mut entries: Vec<Entry> = Vec::new();
        loop {
            let index = r.u8()?;
            if index == 0xff {
                break;
            }
            let t = r.varint()?;
            let mut value = Writer::new();
            Self::value(items, misc, t, &mut r, &mut value)?;
            let Some(old) = old_type(t) else { continue };
            let mut e = Entry { index, t: old, value: value.buf };
            if !self.entry_for_kind(kind, entity_id, regs, t, &mut e, &mut entries) {
                continue;
            }
            entries.push(e);
        }
        if entries.is_empty() {
            return Ok(None);
        }
        let mut w = Writer::with_capacity(payload.len());
        w.varint(entity_id);
        for e in entries {
            w.u8(e.index);
            w.varint(e.t);
            w.raw(&e.value);
        }
        w.u8(0xff);
        Ok(Some(w.into_inner()))
    }

    /// Per-entity rewrites; returns false to drop the entry. May push extra entries.
    fn entry_for_kind(&mut self, kind: Kind, entity_id: i32, regs: &Registries, new_type: i32, e: &mut Entry, extra: &mut Vec<Entry>) -> bool {
        // Type-wide value changes first.
        match new_type {
            ty::WOLF_VARIANT => {
                // 1.21.4 wolf variant is a holder (id + 1).
                let id = Reader::new(&e.value).varint().unwrap_or(0);
                let mut w = Writer::new();
                w.varint(id + 1);
                e.value = w.buf;
            }
            ty::FROG_VARIANT => {
                let id = Reader::new(&e.value).varint().unwrap_or(0);
                let v = match regs.key("frog_variant", id).map(strip_ns) {
                    Some("cold") => 2,
                    Some("warm") => 1,
                    _ => 0,
                };
                e.value = varint_bytes(v);
            }
            ty::CAT_VARIANT => {
                let id = Reader::new(&e.value).varint().unwrap_or(0);
                let v = match regs.key("cat_variant", id).map(strip_ns) {
                    Some("tabby") => 0,
                    Some("black") => 1,
                    Some("red") => 2,
                    Some("siamese") => 3,
                    Some("british_shorthair") => 4,
                    Some("calico") => 5,
                    Some("persian") => 6,
                    Some("ragdoll") => 7,
                    Some("white") => 8,
                    Some("jellie") => 9,
                    Some("all_black") => 10,
                    _ => 1,
                };
                e.value = varint_bytes(v);
            }
            _ => {}
        }
        match kind {
            Kind::Minecart => {
                if e.index == 11 {
                    // 1.21.5: optional block state; 1.21.4: varint state + "custom display" flag.
                    let state = Reader::new(&e.value).varint().unwrap_or(0);
                    if state == 0 {
                        return false;
                    }
                    e.t = ty::VARINT;
                    extra.push(Entry { index: 13, t: ty::BOOL, value: vec![1] });
                } else if e.index >= 13 {
                    e.index += 1;
                }
            }
            Kind::Mooshroom if e.index == 17 => {
                let v = Reader::new(&e.value).varint().unwrap_or(0);
                let mut w = Writer::new();
                w.string(if v == 0 { "red" } else { "brown" });
                e.t = ty::STRING;
                e.value = w.buf;
            }
            Kind::Horse if e.index == 17 => {
                let flags = e.value.first().copied().unwrap_or(0);
                let saddled = self.horses.get(&entity_id).is_some_and(|(_, s)| *s);
                self.horses.insert(entity_id, (flags, saddled));
                if saddled {
                    e.value = vec![flags | SADDLED_FLAG];
                }
            }
            Kind::Chicken | Kind::Cow if e.index == 17 => return false,
            Kind::Pig => {
                if e.index == 18 {
                    return false; // variant
                }
                if e.index >= 17 {
                    e.index += 1; // saddle used to be 17
                }
            }
            Kind::Wolf if e.index == 23 => return false,
            Kind::ExperienceOrb if e.index == 8 => return false,
            Kind::Dolphin | Kind::Turtle if e.index >= 17 => e.index += 1,
            _ => {}
        }
        true
    }

    fn value(items: &Items, misc: &Misc, t: i32, r: &mut Reader, w: &mut Writer) -> Result<()> {
        match t {
            ty::BYTE => {
                w.u8(r.u8()?);
            }
            ty::VARINT | ty::DIRECTION | ty::OPT_VARINT | ty::POSE | ty::CAT_VARIANT | ty::COW_VARIANT | ty::WOLF_VARIANT
            | ty::WOLF_SOUND_VARIANT | ty::FROG_VARIANT | ty::PIG_VARIANT | ty::CHICKEN_VARIANT | ty::SNIFFER_STATE
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
            ty::COMPONENT => text::copy_component(r, w)?,
            ty::OPT_COMPONENT => text::copy_opt_component(r, w)?,
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
            ty::COMPOUND => Items::copy_nbt(r, w)?,
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
                    text::copy_opt_component(r, w)?;
                    text::copy_opt_component(r, w)
                })?;
            }
            other => return Err(err(&format!("unknown entity data type {other}"))),
        }
        Ok(())
    }

    /// SET_EQUIPMENT: the saddle slot becomes entity data; returns the packets to send.
    pub fn set_equipment(&mut self, items: &Items, payload: &[u8], out: &mut Vec<(i32, Vec<u8>)>) -> Result<()> {
        let mut r = Reader::new(payload);
        let entity_id = r.varint()?;
        let mut kept: Vec<(i8, Vec<u8>)> = Vec::new();
        loop {
            let slot_byte = r.i8()?;
            let slot = slot_byte & 0x7f;
            let start = r.position();
            let item_id = {
                let mut peek = Reader::new(&payload[start..]);
                if peek.varint()? > 0 { peek.varint()? } else { -1 }
            };
            let mut item = Writer::new();
            items.item_stack(&mut r, &mut item)?;
            if slot == SADDLE_SLOT {
                let saddle = d::NEW_ITEM_NAMES.iter().position(|n| *n == "saddle").map_or(-2, |p| p as i32);
                self.saddle(entity_id, item_id == saddle, out);
            } else {
                kept.push((slot, item.buf));
            }
            if slot_byte >= 0 {
                break;
            }
        }
        if kept.is_empty() {
            return Ok(());
        }
        let mut w = Writer::with_capacity(payload.len());
        w.varint(entity_id);
        let n = kept.len();
        for (i, (slot, item)) in kept.into_iter().enumerate() {
            w.i8(if i + 1 < n { slot | -128 } else { slot });
            w.raw(&item);
        }
        let mapped = ids::play_clientbound(ids::new::SET_EQUIPMENT).ok_or_else(|| err("no set_equipment id"))?;
        out.push((mapped, w.into_inner()));
        Ok(())
    }

    fn saddle(&mut self, entity_id: i32, saddled: bool, out: &mut Vec<(i32, Vec<u8>)>) {
        let kind = self.kinds.get(&entity_id).copied().unwrap_or(Kind::Other);
        let mut m = Writer::new();
        m.varint(entity_id);
        match kind {
            Kind::Horse => {
                let (flags, was) = self.horses.get(&entity_id).copied().unwrap_or((0, false));
                if was == saddled && self.horses.contains_key(&entity_id) {
                    return;
                }
                self.horses.insert(entity_id, (flags, saddled));
                m.u8(17);
                m.varint(ty::BYTE);
                m.u8(if saddled { flags | SADDLED_FLAG } else { flags });
            }
            Kind::Pig => {
                m.u8(17);
                m.varint(ty::BOOL);
                m.bool(saddled);
            }
            Kind::Strider => {
                m.u8(19);
                m.varint(ty::BOOL);
                m.bool(saddled);
            }
            _ => return,
        }
        m.u8(0xff);
        out.push((ids::old::SET_ENTITY_DATA, m.into_inner()));
    }

    /// SET_PLAYER_TEAM: name tag visibility and collision rule went from strings to ids.
    pub fn set_player_team(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len() + 16);
        w.string(r.string()?);
        let mode = r.i8()?;
        w.i8(mode);
        if mode == 0 || mode == 2 {
            text::copy_component(&mut r, &mut w)?;
            w.u8(r.u8()?);
            let visibility = ["always", "never", "hideForOtherTeams", "hideForOwnTeam"];
            let collision = ["always", "never", "pushOtherTeams", "pushOwnTeam"];
            w.string(visibility.get(r.varint()?.max(0) as usize).copied().unwrap_or("always"));
            w.string(collision.get(r.varint()?.max(0) as usize).copied().unwrap_or("always"));
            w.varint(r.varint()?);
            text::copy_component(&mut r, &mut w)?;
            text::copy_component(&mut r, &mut w)?;
        }
        w.raw(r.remaining());
        Ok(w.into_inner())
    }
}

fn varint_bytes(v: i32) -> Vec<u8> {
    let mut w = Writer::new();
    w.varint(v);
    w.buf
}

fn strip_ns(s: &str) -> &str {
    s.strip_prefix("minecraft:").unwrap_or(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translate::v1_21_4::items::Dir;

    fn entity_type(name: &str) -> i32 {
        d::NEW_ENTITY_NAMES.iter().position(|n| *n == name).unwrap() as i32
    }

    fn spawn(e: &mut Entities, id: i32, ty_id: i32) -> Vec<(i32, Vec<u8>)> {
        let mut p = Writer::new();
        p.varint(id);
        p.uuid(1);
        p.varint(ty_id);
        p.raw(&[0u8; 24]);
        p.raw(&[0, 0, 0]);
        p.varint(7);
        p.i16(0);
        p.i16(5);
        p.i16(0);
        let mut out = Vec::new();
        e.add_entity(&Items::new(Dir::ToClient), &p.buf, &mut out).unwrap();
        out
    }

    #[test]
    fn experience_orb_gets_its_own_packet_and_motion() {
        let mut e = Entities::default();
        let out = spawn(&mut e, 3, entity_type("experience_orb"));
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0, ids::old::ADD_EXPERIENCE_ORB);
        let mut r = Reader::new(&out[0].1);
        assert_eq!(r.varint().unwrap(), 3);
        r.bytes(24).unwrap();
        assert_eq!(r.i16().unwrap(), 7);
        assert!(r.is_empty());
        assert_eq!(out[1].0, ids::old::SET_ENTITY_MOTION);
    }

    #[test]
    fn pig_indexes_shift_and_saddle_becomes_data() {
        let mut e = Entities::default();
        spawn(&mut e, 9, entity_type("pig"));
        let mut p = Writer::new();
        p.varint(9);
        p.u8(17); // boost time
        p.varint(ty::VARINT);
        p.varint(40);
        p.u8(18); // variant
        p.varint(ty::PIG_VARIANT);
        p.varint(1);
        p.u8(0xff);
        let out = e
            .set_entity_data(&Items::new(Dir::ToClient), &Misc::new(), &Registries::default(), &p.buf)
            .unwrap()
            .unwrap();
        let mut r = Reader::new(&out);
        assert_eq!(r.varint().unwrap(), 9);
        assert_eq!(r.u8().unwrap(), 18);
        assert_eq!(r.varint().unwrap(), ty::VARINT);
        assert_eq!(r.varint().unwrap(), 40);
        assert_eq!(r.u8().unwrap(), 0xff);
        assert!(r.is_empty());

        // saddle equipment → pig saddle data at index 17
        let saddle = d::NEW_ITEM_NAMES.iter().position(|n| *n == "saddle").unwrap() as i32;
        let mut q = Writer::new();
        q.varint(9);
        q.i8(SADDLE_SLOT);
        q.varint(1);
        q.varint(saddle);
        q.varint(0);
        q.varint(0);
        let mut packets = Vec::new();
        e.set_equipment(&Items::new(Dir::ToClient), &q.buf, &mut packets).unwrap();
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].0, ids::old::SET_ENTITY_DATA);
        assert_eq!(&packets[0].1[1..], &[17, ty::BOOL as u8, 1, 0xff]);
    }

    #[test]
    fn team_rules_become_strings() {
        let mut p = Writer::new();
        p.string("t");
        p.i8(2);
        p.u8(0); // display name: End tag stands in for an empty component
        p.u8(1);
        p.varint(1);
        p.varint(3);
        p.varint(12);
        p.u8(0);
        p.u8(0);
        let out = Entities::set_player_team(&p.buf).unwrap();
        let mut r = Reader::new(&out);
        assert_eq!(r.string().unwrap(), "t");
        assert_eq!(r.i8().unwrap(), 2);
        assert_eq!(r.u8().unwrap(), 0);
        assert_eq!(r.u8().unwrap(), 1);
        assert_eq!(r.string().unwrap(), "never");
        assert_eq!(r.string().unwrap(), "pushOwnTeam");
    }
}
