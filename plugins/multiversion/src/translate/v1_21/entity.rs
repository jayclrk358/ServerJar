//! Entities 1.21.2 → 1.21: spawning, entity data, teleports and player movement, following
//! the vanilla changes.

use std::collections::HashMap;

use super::data_1_21 as d;
use super::ids::old;
use super::items::{Items, Reg};
use super::misc::Misc;
use crate::proto::buf::{DecodeError, Reader, Result, Writer};
use crate::proto::nbt::{self, Tag};

fn err(msg: &str) -> DecodeError {
    DecodeError::Other(msg.to_string())
}

/// Entity data value types (identical ids in 1.21 and 1.21.2).
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
    pub const WOLF_VARIANT: i32 = 23;
    pub const FROG_VARIANT: i32 = 24;
    pub const OPT_GLOBAL_POSITION: i32 = 25;
    pub const PAINTING_VARIANT: i32 = 26;
    pub const SNIFFER_STATE: i32 = 27;
    pub const ARMADILLO_STATE: i32 = 28;
    pub const VECTOR3F: i32 = 29;
    pub const QUATERNION: i32 = 30;
}

// relative teleport flags (1.21.2 int bit set)
const REL_X: i32 = 1 << 0;
const REL_Y: i32 = 1 << 1;
const REL_Z: i32 = 1 << 2;
const REL_Y_ROT: i32 = 1 << 3;
const REL_X_ROT: i32 = 1 << 4;
const REL_DELTA_X: i32 = 1 << 5;
const REL_DELTA_Y: i32 = 1 << 6;
const REL_DELTA_Z: i32 = 1 << 7;
const REL_ROTATE_DELTA: i32 = 1 << 8;

fn new_entity(name: &str) -> i32 {
    d::NEW_ENTITY_NAMES.iter().position(|n| *n == name).map_or(-1, |p| p as i32)
}

/// Entity families whose data layout changed (1.21.2 entity type names).
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Other,
    Boat(i32),
    Minecart,
    Salmon,
    Arrow,
    /// ageable water creature with its baby scale
    WaterBaby(f32),
    Creaking,
    FallingBlock,
}

fn kind_of(ty: i32) -> Kind {
    let name = d::NEW_ENTITY_NAMES.get(ty.max(0) as usize).copied().unwrap_or("");
    if name.ends_with("_boat") || name.ends_with("_raft") {
        let wood = name.trim_end_matches("_boat").trim_end_matches("_raft").trim_end_matches("_chest");
        let boat_type = match wood {
            "oak" => 0,
            "spruce" => 1,
            "birch" => 2,
            "jungle" => 3,
            "acacia" => 4,
            "cherry" => 5,
            "dark_oak" => 6,
            "mangrove" => 7,
            "bamboo" => 8,
            _ => 0,
        };
        return Kind::Boat(boat_type);
    }
    match name {
        "minecart" | "chest_minecart" | "command_block_minecart" | "furnace_minecart" | "hopper_minecart" | "spawner_minecart" | "tnt_minecart" => Kind::Minecart,
        "salmon" => Kind::Salmon,
        "arrow" | "spectral_arrow" | "trident" => Kind::Arrow,
        "squid" | "glow_squid" => Kind::WaterBaby(0.5),
        "dolphin" => Kind::WaterBaby(0.65),
        "creaking" | "creaking_transient" => Kind::Creaking,
        "falling_block" => Kind::FallingBlock,
        _ => Kind::Other,
    }
}

/// Last known state of the client's own player (fed by its movement packets).
#[derive(Default, Clone, Copy)]
pub struct PlayerState {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
}

#[derive(Default)]
pub struct Entities {
    /// entity id → 1.21.2 entity type id
    types: HashMap<i32, i32>,
    babies: HashMap<i32, bool>,
    pub client_id: i32,
    pub world: String,
    pub player: PlayerState,
    pub vehicle: Option<i32>,
    pub sneaking: bool,
    pub sprinting: bool,
    /// last PLAYER_INPUT flags sent to the server
    pub last_input: u8,
    /// movement-derived part of the input (forward/back/left/right/jump)
    pub move_input: u8,
}

impl Entities {
    pub fn clear(&mut self) {
        self.types.clear();
        self.babies.clear();
    }

    /// ADD_ENTITY: remaps the type and adds the data the 1.21 client needs (boat wood type,
    /// a name for creakings shown as wardens).
    pub fn add_entity(&mut self, items: &Items, payload: &[u8], out: &mut Vec<(i32, Vec<u8>)>) -> Result<()> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let entity_id = r.varint()?;
        w.varint(entity_id);
        w.uuid(r.uuid()?);
        let ty = r.varint()?;
        self.types.insert(entity_id, ty);
        w.varint(items.entity(ty));
        w.raw(r.bytes(24 + 3)?); // x y z, pitch yaw head yaw
        let data = r.varint()?;
        let kind = kind_of(ty);
        w.varint(if kind == Kind::FallingBlock { items.block_state(data) } else { data });
        w.raw(r.remaining()); // velocity
        out.push((old::ADD_ENTITY, w.into_inner()));
        match kind {
            Kind::Boat(boat_type) => {
                let mut m = Writer::new();
                m.varint(entity_id);
                m.u8(11);
                m.varint(ty::VARINT);
                m.varint(boat_type);
                m.u8(0xff);
                out.push((old::SET_ENTITY_DATA, m.into_inner()));
            }
            Kind::Creaking => {
                let mut m = Writer::new();
                m.varint(entity_id);
                m.u8(2); // custom name
                m.varint(ty::OPT_COMPONENT);
                m.bool(true);
                nbt::write(&mut m, &Tag::String("Creaking".into()));
                m.u8(0xff);
                out.push((old::SET_ENTITY_DATA, m.into_inner()));
            }
            _ => {}
        }
        Ok(())
    }

    /// REMOVE_ENTITIES. Returns true when the client's vehicle went away.
    pub fn remove_entities(&mut self, payload: &[u8]) -> Result<bool> {
        let mut r = Reader::new(payload);
        let mut vehicle_gone = false;
        for id in r.list(|r| r.varint())? {
            self.types.remove(&id);
            self.babies.remove(&id);
            if self.vehicle == Some(id) {
                self.vehicle = None;
                vehicle_gone = true;
            }
        }
        Ok(vehicle_gone)
    }

    /// SET_PASSENGERS. Returns true when the client left its vehicle.
    pub fn set_passengers(&mut self, payload: &[u8]) -> Result<bool> {
        let mut r = Reader::new(payload);
        let vehicle = r.varint()?;
        let passengers = r.list(|r| r.varint())?;
        let mut left = false;
        if self.vehicle == Some(vehicle) {
            self.vehicle = None;
            left = true;
        }
        if passengers.contains(&self.client_id) {
            self.vehicle = Some(vehicle);
            left = false;
        }
        Ok(left)
    }

    /// SET_ENTITY_DATA: converts values and applies the per-type index changes. May also produce an
    /// attribute packet (baby water creatures are shown with a smaller scale).
    pub fn set_entity_data(&mut self, items: &Items, misc: &Misc, payload: &[u8], out: &mut Vec<(i32, Vec<u8>)>) -> Result<()> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let entity_id = r.varint()?;
        w.varint(entity_id);
        let kind = self.types.get(&entity_id).map_or(Kind::Other, |t| kind_of(*t));
        loop {
            let index = r.u8()?;
            if index == 0xff {
                w.u8(0xff);
                break;
            }
            let t = r.varint()?;
            let start = r.position();
            let mut value = Writer::new();
            Self::entity_data_value(items, misc, t, &mut r, &mut value)?;
            let mut index = i32::from(index);
            let mut keep = true;
            match kind {
                Kind::Boat(_) if index >= 11 => index += 1,
                Kind::Minecart if index == 11 && t == ty::VARINT => {
                    let state = Reader::new(&value.buf).varint()?;
                    value = Writer::new();
                    value.varint(items.block_state(state));
                }
                Kind::Salmon if index == 17 => keep = false,
                Kind::Salmon if index > 17 => index -= 1,
                Kind::Arrow if index == 10 => keep = false,
                Kind::Arrow if index > 10 => index -= 1,
                Kind::WaterBaby(scale) if index == 16 => {
                    keep = false;
                    if t == ty::BOOL {
                        r.set_position(start);
                        let baby = r.bool()?;
                        let previous = self.babies.insert(entity_id, baby);
                        if previous != Some(baby) && (baby || previous.is_some()) {
                            out.push((old::UPDATE_ATTRIBUTES, Self::scale_packet(entity_id, if baby { scale } else { 1.0 })));
                        }
                    }
                }
                Kind::WaterBaby(_) if index > 16 => index -= 1,
                Kind::Creaking if index == 16 || index == 17 => keep = false,
                _ => {}
            }
            if keep {
                w.u8(index as u8);
                w.varint(t);
                w.raw(&value.buf);
            }
        }
        out.push((old::SET_ENTITY_DATA, w.into_inner()));
        Ok(())
    }

    fn scale_packet(entity_id: i32, scale: f32) -> Vec<u8> {
        let attribute = d::OLD_ATTRIBUTE_NAMES.iter().position(|n| *n == "generic.scale").unwrap_or(24) as i32;
        let mut w = Writer::new();
        w.varint(entity_id);
        w.varint(1);
        w.varint(attribute);
        w.f64(f64::from(scale));
        w.varint(0);
        w.into_inner()
    }

    fn entity_data_value(items: &Items, misc: &Misc, t: i32, r: &mut Reader, w: &mut Writer) -> Result<()> {
        match t {
            ty::BYTE => {
                w.u8(r.u8()?);
            }
            ty::VARINT | ty::DIRECTION | ty::OPT_VARINT | ty::POSE | ty::CAT_VARIANT | ty::FROG_VARIANT | ty::SNIFFER_STATE | ty::ARMADILLO_STATE => {
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
            ty::COMPONENT | ty::COMPOUND => Items::copy_nbt(r, w)?,
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
                        Items::copy_string(r, w)?;
                    }
                    s.holder_set(r, w, Reg::Same)
                })?;
            }
            ty::OPT_GLOBAL_POSITION => {
                if Items::copy_bool(r, w)? {
                    w.string(r.string()?);
                    w.i64(r.i64()?);
                }
            }
            ty::PAINTING_VARIANT => {
                // 1.21.2 adds an optional title and author to inline variants
                items.holder(r, w, Reg::Same, |_, r, w| {
                    Items::copy_varint(r, w)?;
                    Items::copy_varint(r, w)?;
                    Items::copy_string(r, w)?;
                    Items::copy_opt_nbt(r, &mut Writer::new())?;
                    Items::copy_opt_nbt(r, &mut Writer::new())
                })?;
            }
            other => return Err(err(&format!("unknown entity data type {other}"))),
        }
        Ok(())
    }

    /// LOGIN: drops the sea level; remembers the client's entity id.
    pub fn login(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let id = r.i32()?;
        self.client_id = id;
        self.clear();
        self.types.insert(id, new_entity("player"));
        w.i32(id);
        w.bool(r.bool()?);
        let n = r.varint()?;
        w.varint(n);
        for _ in 0..n.max(0) {
            w.string(r.string()?);
        }
        for _ in 0..3 {
            w.varint(r.varint()?);
        }
        w.raw(r.bytes(3)?);
        self.spawn_info(&mut r, &mut w)?;
        Ok(w.into_inner())
    }

    pub fn respawn(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        self.spawn_info(&mut r, &mut w)?;
        Ok(w.into_inner())
    }

    /// Common spawn info: dimension, world, seed, game modes, debug, flat, death location, portal
    /// cooldown, then the 1.21.2 sea level (dropped) and the rest.
    fn spawn_info(&mut self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        w.varint(r.varint()?);
        let world = r.string()?.to_string();
        if !self.world.is_empty() && world != self.world {
            let me = self.types.get(&self.client_id).copied();
            self.clear();
            if let Some(t) = me {
                self.types.insert(self.client_id, t);
            }
        }
        self.world = world.clone();
        w.string(&world);
        w.i64(r.i64()?);
        w.raw(r.bytes(4)?); // game mode, previous, debug, flat
        if Items::copy_bool(r, w)? {
            w.string(r.string()?);
            w.i64(r.i64()?);
        }
        w.varint(r.varint()?); // portal cooldown
        r.varint()?; // sea level
        w.raw(r.remaining());
        Ok(())
    }

    fn packed_angle(degrees: f32) -> u8 {
        (degrees * 256.0 / 360.0).floor() as i32 as u8
    }

    pub fn legacy_velocity(v: f64) -> i16 {
        (v.clamp(-3.9, 3.9) * 8000.0) as i16
    }

    fn motion(entity_id: i32, mx: f64, my: f64, mz: f64) -> Vec<u8> {
        let mut w = Writer::new();
        w.varint(entity_id);
        w.i16(Self::legacy_velocity(mx));
        w.i16(Self::legacy_velocity(my));
        w.i16(Self::legacy_velocity(mz));
        w.into_inner()
    }

    /// ENTITY_POSITION_SYNC → TELEPORT_ENTITY (the old client has no delta movement here).
    pub fn entity_position_sync(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        w.varint(r.varint()?);
        w.raw(r.bytes(24)?);
        r.bytes(24)?;
        w.u8(Self::packed_angle(r.f32()?));
        w.u8(Self::packed_angle(r.f32()?));
        w.raw(r.remaining()); // on ground
        Ok(w.into_inner())
    }

    /// MOVE_MINECART_ALONG_TRACK → TELEPORT_ENTITY to the last step + SET_ENTITY_MOTION.
    pub fn move_minecart(payload: &[u8], out: &mut Vec<(i32, Vec<u8>)>) -> Result<()> {
        let mut r = Reader::new(payload);
        let id = r.varint()?;
        let steps = r.varint()?;
        if steps <= 0 {
            return Ok(());
        }
        let (mut pos, mut motion, mut yaw, mut pitch) = ([0f64; 3], [0f64; 3], 0u8, 0u8);
        for _ in 0..steps {
            for p in &mut pos {
                *p = r.f64()?;
            }
            for m in &mut motion {
                *m = r.f64()?;
            }
            yaw = r.u8()?;
            pitch = r.u8()?;
            r.f32()?; // weight
        }
        let mut w = Writer::new();
        w.varint(id);
        for p in pos {
            w.f64(p);
        }
        w.u8(128u8.wrapping_sub(yaw));
        w.u8(pitch);
        w.bool(false);
        out.push((old::TELEPORT_ENTITY, w.into_inner()));
        out.push((old::SET_ENTITY_MOTION, Self::motion(id, motion[0], motion[1], motion[2])));
        Ok(())
    }

    /// TELEPORT_ENTITY (1.21.2: with delta movement and relative flags).
    pub fn teleport_entity(&mut self, payload: &[u8], out: &mut Vec<(i32, Vec<u8>)>) -> Result<()> {
        let mut r = Reader::new(payload);
        let id = r.varint()?;
        let (x, y, z) = (r.f64()?, r.f64()?, r.f64()?);
        let (mx, my, mz) = (r.f64()?, r.f64()?, r.f64()?);
        let (yaw, pitch) = (r.f32()?, r.f32()?);
        let relative = r.i32()?;
        let mut w = Writer::new();
        w.varint(id);
        let is_client = id == self.client_id;
        let before = self.player;
        let abs = if is_client { self.absolute(x, y, z, yaw, pitch, relative) } else { (x, y, z, yaw, pitch) };
        w.f64(abs.0);
        w.f64(abs.1);
        w.f64(abs.2);
        w.u8(Self::packed_angle(abs.3));
        w.u8(Self::packed_angle(abs.4));
        w.raw(r.remaining()); // on ground
        out.push((old::TELEPORT_ENTITY, w.into_inner()));
        if is_client {
            self.set_player(abs);
        }
        self.relative_motion(id, &before, abs.3, abs.4, relative, [mx, my, mz], out);
        Ok(())
    }

    fn set_player(&mut self, (x, y, z, yaw, pitch): (f64, f64, f64, f32, f32)) {
        self.player = PlayerState { x, y, z, yaw, pitch };
    }

    /// PLAYER_POSITION: 1.21.2 teleport id first, delta movement, int flags → 1.21 layout.
    pub fn player_position(&mut self, payload: &[u8], out: &mut Vec<(i32, Vec<u8>)>) -> Result<()> {
        let mut r = Reader::new(payload);
        let teleport_id = r.varint()?;
        let (x, y, z) = (r.f64()?, r.f64()?, r.f64()?);
        let (mx, my, mz) = (r.f64()?, r.f64()?, r.f64()?);
        let (yaw, pitch) = (r.f32()?, r.f32()?);
        let relative = r.i32()?;
        let mut w = Writer::new();
        w.f64(x);
        w.f64(y);
        w.f64(z);
        w.f32(yaw);
        w.f32(pitch);
        w.u8((relative & 0x1F) as u8);
        w.varint(teleport_id);
        out.push((old::PLAYER_POSITION, w.into_inner()));
        let before = self.player;
        let abs = self.absolute(x, y, z, yaw, pitch, relative);
        self.set_player(abs);
        let client = self.client_id;
        self.relative_motion(client, &before, abs.3, abs.4, relative, [mx, my, mz], out);
        Ok(())
    }

    /// Absolute position of the client after a (partly) relative teleport; updates the tracked state.
    fn absolute(&mut self, x: f64, y: f64, z: f64, yaw: f32, pitch: f32, relative: i32) -> (f64, f64, f64, f32, f32) {
        let p = self.player;
        let ax = if relative & REL_X != 0 { x + p.x } else { x };
        let ay = if relative & REL_Y != 0 { y + p.y } else { y };
        let az = if relative & REL_Z != 0 { z + p.z } else { z };
        let ayaw = if relative & REL_Y_ROT != 0 { yaw + p.yaw } else { yaw };
        let apitch = if relative & REL_X_ROT != 0 { pitch + p.pitch } else { pitch };
        (ax, ay, az, ayaw, apitch)
    }

    /// The delta-movement part of a 1.21.2 teleport, emulated with entity motion or a knockback.
    #[allow(clippy::too_many_arguments)]
    fn relative_motion(&self, entity_id: i32, before: &PlayerState, ayaw: f32, apitch: f32, relative: i32, motion: [f64; 3], out: &mut Vec<(i32, Vec<u8>)>) {
        let is_client = entity_id == self.client_id;
        let [mut mx, mut my, mut mz] = motion;
        if relative & REL_ROTATE_DELTA != 0 {
            let dyaw = f64::from(before.yaw - ayaw).to_radians();
            let (c, s) = (dyaw.cos(), dyaw.sin());
            let (nx, nz) = (mx * c + mz * s, mz * c - mx * s);
            mx = nx;
            mz = nz;
            let dpitch = f64::from(before.pitch - apitch).to_radians();
            let (c, s) = (dpitch.cos(), dpitch.sin());
            let (ny, nz) = (my * c + mz * s, mz * c - my * s);
            my = ny;
            mz = nz;
        }
        let dx = relative & REL_DELTA_X != 0;
        let dy = relative & REL_DELTA_Y != 0;
        let dz = relative & REL_DELTA_Z != 0;
        if dx && dy && dz {
            if !is_client || (mx == 0.0 && my == 0.0 && mz == 0.0) {
                return;
            }
            // add to the current velocity: an explosion far away with only knockback
            let mut w = Writer::new();
            w.f64(0.0);
            w.f64(20_000.0);
            w.f64(0.0);
            w.f32(0.0);
            w.varint(0);
            w.f32(mx as f32);
            w.f32(my as f32);
            w.f32(mz as f32);
            w.varint(0); // block interaction
            w.varint(0); // small explosion particle (no data)
            w.varint(0); // large explosion particle
            w.varint(0); // inline sound
            w.string("minecraft:intentionally_empty");
            w.bool(false);
            out.push((old::EXPLODE, w.into_inner()));
        } else if !dx && !dy && !dz {
            out.push((old::SET_ENTITY_MOTION, Self::motion(entity_id, mx, my, mz)));
        }
        // mixed relative/absolute deltas cannot be expressed for the old client
    }

    /// PLAYER_ROTATION → PLAYER_LOOK_AT a point in front of the player; also returns the rotation
    /// packet the old client would have sent.
    pub fn player_rotation(&mut self, payload: &[u8], out: &mut Vec<(i32, Vec<u8>)>) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let yaw = r.f32()?;
        let pitch = r.f32()?;
        let (yr, xr) = (f64::from(yaw).to_radians(), f64::from(pitch).to_radians());
        let factor = -(-xr).cos();
        let p = self.player;
        let mut w = Writer::new();
        w.varint(0); // from feet
        w.f64(p.x + (-yr - std::f64::consts::PI).sin() * factor);
        w.f64(p.y + (-xr).sin());
        w.f64(p.z + (-yr - std::f64::consts::PI).cos() * factor);
        w.bool(false);
        out.push((old::PLAYER_LOOK_AT, w.into_inner()));
        let mut s = Writer::new();
        s.f32(yaw);
        s.f32(pitch);
        s.u8(0);
        Ok(s.into_inner())
    }

    // ---- serverbound -----------------------------------------------------------------------

    /// Serverbound movement 1.21 → 1.21.2 (on-ground bool → flags byte), tracking the position and
    /// deriving the movement keys the 1.21.2 server expects in PLAYER_INPUT.
    pub fn movement(&mut self, id: i32, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let before = self.player;
        let (has_pos, has_rot) = match id {
            old::S_MOVE_PLAYER_POS => (true, false),
            old::S_MOVE_PLAYER_POS_ROT => (true, true),
            old::S_MOVE_PLAYER_ROT => (false, true),
            _ => (false, false),
        };
        if has_pos {
            self.player.x = r.f64()?;
            self.player.y = r.f64()?;
            self.player.z = r.f64()?;
            w.f64(self.player.x);
            w.f64(self.player.y);
            w.f64(self.player.z);
        }
        if has_rot {
            self.player.yaw = r.f32()?;
            self.player.pitch = r.f32()?;
            w.f32(self.player.yaw);
            w.f32(self.player.pitch);
        }
        let on_ground = r.bool()?;
        w.u8(u8::from(on_ground));
        if has_pos && self.vehicle.is_none() {
            self.move_input = Self::input_from_delta(&before, &self.player);
        }
        Ok(w.into_inner())
    }

    /// Movement keys guessed from the position change.
    fn input_from_delta(before: &PlayerState, now: &PlayerState) -> u8 {
        let (dx, dy, dz) = (now.x - before.x, now.y - before.y, now.z - before.z);
        let magnitude = (dx * dx + dz * dz).sqrt();
        let (mut dir_x, mut dir_z) = if magnitude > 0.0 { (dx / magnitude, dz / magnitude) } else { (0.0, 0.0) };
        dir_x = dir_x.clamp(-1.0, 1.0);
        dir_z = dir_z.clamp(-1.0, 1.0);
        let angle = f64::from(-now.yaw).to_radians();
        let new_x = dir_x * angle.cos() - dir_z * angle.sin();
        let new_z = dir_x * angle.sin() + dir_z * angle.cos();
        let mut flags = 0u8;
        if new_z >= 0.65 {
            flags |= 1;
        }
        if new_z <= -0.65 {
            flags |= 2;
        }
        if new_x >= 0.65 {
            flags |= 4;
        }
        if new_x <= -0.65 {
            flags |= 8;
        }
        if (dy - 0.42).abs() <= 1e-4 {
            flags |= 16;
        }
        flags
    }

    /// Current 1.21.2 PLAYER_INPUT flags, if they changed since the last one sent.
    pub fn input_update(&mut self) -> Option<Vec<u8>> {
        let mut flags = self.move_input;
        if self.sneaking {
            flags |= 32;
        }
        if self.sprinting {
            flags |= 64;
        }
        if flags == self.last_input {
            return None;
        }
        self.last_input = flags;
        Some(vec![flags])
    }

    pub fn move_vehicle(&mut self, payload: &[u8]) -> Result<()> {
        let mut r = Reader::new(payload);
        self.player.x = r.f64()?;
        self.player.y = r.f64()?;
        self.player.z = r.f64()?;
        Ok(())
    }

    /// PLAYER_COMMAND: tracks sneaking and sprinting (1.21.2 reads them from PLAYER_INPUT).
    pub fn player_command(&mut self, payload: &[u8]) -> Result<()> {
        let mut r = Reader::new(payload);
        r.varint()?;
        match r.varint()? {
            0 => self.sneaking = true,
            1 => self.sneaking = false,
            3 => self.sprinting = true,
            4 => self.sprinting = false,
            _ => {}
        }
        Ok(())
    }

    /// PLAYER_INPUT 1.21 (sideways, forward, jump|sneak) → 1.21.2 key flags. Also returns a player
    /// command when the sneak state changed (1.21.2 no longer takes it from the input).
    pub fn player_input(&mut self, payload: &[u8]) -> Result<(Vec<u8>, Option<Vec<u8>>)> {
        let mut r = Reader::new(payload);
        let sideways = r.f32()?;
        let forward = r.f32()?;
        let flags = r.u8()?;
        let mut keys = 0u8;
        if forward > 0.0 {
            keys |= 1;
        } else if forward < 0.0 {
            keys |= 2;
        }
        if sideways > 0.0 {
            keys |= 4;
        } else if sideways < 0.0 {
            keys |= 8;
        }
        if flags & 1 != 0 {
            keys |= 16;
        }
        let sneak = flags & 2 != 0;
        if sneak {
            keys |= 32;
        }
        self.move_input = keys & 0x1F;
        self.last_input = keys | if self.sprinting { 64 } else { 0 };
        let command = self.sneak_command(sneak);
        Ok((vec![self.last_input], command))
    }

    /// Player command packet (1.21.2 serverbound) for a sneak state change, if it changed.
    pub fn sneak_command(&mut self, sneaking: bool) -> Option<Vec<u8>> {
        if self.sneaking == sneaking {
            return None;
        }
        self.sneaking = sneaking;
        let mut w = Writer::new();
        w.varint(self.client_id);
        w.varint(if sneaking { 0 } else { 1 });
        w.varint(0);
        Some(w.into_inner())
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

    #[test]
    fn boat_spawn_adds_wood_type() {
        let items = Items::new(Dir::ToClient);
        let mut e = Entities::default();
        let birch = new_entity("birch_boat");
        let mut p = Writer::new();
        p.varint(9);
        p.uuid(1);
        p.varint(birch);
        p.raw(&[0u8; 27]);
        p.varint(0);
        p.raw(&[0u8; 6]);
        let mut out = Vec::new();
        e.add_entity(&items, &p.buf, &mut out).unwrap();
        assert_eq!(out.len(), 2);
        let mut r = Reader::new(&out[0].1);
        r.varint().unwrap();
        r.uuid().unwrap();
        let boat_old = d::OLD_ENTITY_NAMES.iter().position(|n| *n == "boat").unwrap() as i32;
        assert_eq!(r.varint().unwrap(), boat_old);
        let mut m = Reader::new(&out[1].1);
        assert_eq!(m.varint().unwrap(), 9);
        assert_eq!(m.u8().unwrap(), 11);
        assert_eq!(m.varint().unwrap(), ty::VARINT);
        assert_eq!(m.varint().unwrap(), 2);
        // boat data index 11 (paddle) moves to 12
        let mut data = Writer::new();
        data.varint(9);
        data.u8(11);
        data.varint(ty::BOOL);
        data.bool(true);
        data.u8(0xff);
        let mut out = Vec::new();
        e.set_entity_data(&items, &Misc::new(), &data.buf, &mut out).unwrap();
        assert_eq!(out[0].1, vec![9, 12, ty::BOOL as u8, 1, 0xff]);
    }

    #[test]
    fn baby_squid_is_scaled_and_index_removed() {
        let items = Items::new(Dir::ToClient);
        let mut e = Entities::default();
        e.types.insert(5, new_entity("squid"));
        let mut data = Writer::new();
        data.varint(5);
        data.u8(16);
        data.varint(ty::BOOL);
        data.bool(true);
        data.u8(17);
        data.varint(ty::VARINT);
        data.varint(3);
        data.u8(0xff);
        let mut out = Vec::new();
        e.set_entity_data(&items, &Misc::new(), &data.buf, &mut out).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0, old::UPDATE_ATTRIBUTES);
        assert_eq!(out[1].1, vec![5, 16, ty::VARINT as u8, 3, 0xff]);
    }

    #[test]
    fn player_position_layout() {
        let mut e = Entities::default();
        let mut p = Writer::new();
        p.varint(77);
        p.f64(1.0).f64(2.0).f64(3.0);
        p.f64(0.0).f64(0.0).f64(0.0);
        p.f32(90.0).f32(10.0);
        p.i32(REL_X | REL_DELTA_X);
        let mut out = Vec::new();
        e.player_position(&p.buf, &mut out).unwrap();
        assert_eq!(out.len(), 1); // mixed delta flags: no motion packet
        let mut r = Reader::new(&out[0].1);
        assert_eq!(r.f64().unwrap(), 1.0);
        r.bytes(16).unwrap();
        assert_eq!(r.f32().unwrap(), 90.0);
        assert_eq!(r.f32().unwrap(), 10.0);
        assert_eq!(r.u8().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), 77);
        assert!(r.is_empty());
    }

    #[test]
    fn movement_flags() {
        let mut e = Entities::default();
        let mut p = Writer::new();
        p.f64(0.0).f64(64.0).f64(0.3);
        p.bool(true);
        let out = e.movement(old::S_MOVE_PLAYER_POS, &p.buf).unwrap();
        assert_eq!(out.len(), 25);
        assert_eq!(out[24], 1);
        assert_eq!(e.move_input & 1, 1); // moved forward (+z at yaw 0)
        assert_eq!(e.input_update(), Some(vec![1]));
        assert_eq!(e.input_update(), None);
    }
}
