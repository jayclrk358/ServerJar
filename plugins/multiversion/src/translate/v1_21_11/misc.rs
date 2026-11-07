//! Everything else 26.1 ↔ 1.21.11: particles, sounds, explosions, containers, commands,
//! recipes, advancements, world time and entity interaction.

use super::super::data_1_21_11 as d;
use super::items::{Items, Reg};
use crate::proto::buf::{DecodeError, Reader, Result, Writer};

fn err(msg: &str) -> DecodeError {
    DecodeError::Other(msg.to_string())
}

/// Day time bookkeeping for SET_TIME (26.1 sends clocks, 1.21.11 a day time + "advance" flag).
#[derive(Default)]
pub struct DayTime {
    game_time: i64,
    day_time: i64,
    advance: bool,
}

pub struct Misc;

impl Misc {
    // ---- particles -------------------------------------------------------------------------

    /// One particle (id + data). In 26.1 the item particle carries an item template.
    pub fn particle(items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let id = r.varint()?;
        let name = d::NEW_PARTICLE_NAMES.get(id as usize).copied().unwrap_or("");
        w.varint(items.particle(id));
        match name {
            "block" | "block_marker" | "falling_dust" | "dust_pillar" | "block_crumble" => {
                w.varint(items.block_state(r.varint()?));
            }
            "item" => items.template(r, w)?,
            "dust" => Items::copy_bytes(r, w, 8)?,
            "dust_color_transition" => Items::copy_bytes(r, w, 12)?,
            "vibration" => {
                let source = Items::copy_varint(r, w)?;
                match source {
                    0 => Items::copy_bytes(r, w, 8)?,
                    1 => {
                        Items::copy_varint(r, w)?;
                        Items::copy_bytes(r, w, 4)?;
                    }
                    _ => return Err(err("unknown vibration source")),
                }
                Items::copy_varint(r, w)?;
            }
            "sculk_charge" | "dragon_breath" | "entity_effect" | "tinted_leaves" | "flash" => Items::copy_bytes(r, w, 4)?,
            "shriek" => {
                Items::copy_varint(r, w)?;
            }
            "effect" | "instant_effect" => Items::copy_bytes(r, w, 8)?,
            "trail" => {
                Items::copy_bytes(r, w, 28)?;
                Items::copy_varint(r, w)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// LEVEL_PARTICLES: flags, position, offsets, speed, count, then the particle.
    pub fn level_particles(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.raw(r.bytes(2 + 24 + 12 + 4 + 4)?);
        Self::particle(items, &mut r, &mut w)?;
        Ok(w.into_inner())
    }

    // ---- sounds / explosions ---------------------------------------------------------------

    pub fn sound(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        items.sound_event(&mut r, &mut w)?;
        w.raw(r.remaining());
        Ok(w.into_inner())
    }

    pub fn explode(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.raw(r.bytes(24 + 4 + 4)?); // center, power, block count
        if Items::copy_bool(&mut r, &mut w)? {
            w.raw(r.bytes(24)?); // knockback
        }
        Self::particle(items, &mut r, &mut w)?;
        items.sound_event(&mut r, &mut w)?;
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            Self::particle(items, &mut r, &mut w)?;
            w.raw(r.bytes(8)?); // scaling, speed
            Items::copy_varint(&mut r, &mut w)?; // weight
        }
        Ok(w.into_inner())
    }

    // ---- containers ------------------------------------------------------------------------

    pub fn container_set_content(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.varint(r.varint()?);
        w.varint(r.varint()?);
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            items.item_stack(&mut r, &mut w)?;
        }
        items.item_stack(&mut r, &mut w)?; // carried
        Ok(w.into_inner())
    }

    pub fn container_set_slot(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.varint(r.varint()?);
        w.varint(r.varint()?);
        w.i16(r.i16()?);
        items.item_stack(&mut r, &mut w)?;
        Ok(w.into_inner())
    }

    pub fn single_item(items: &Items, payload: &[u8], leading_varints: usize) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        for _ in 0..leading_varints {
            w.varint(r.varint()?);
        }
        items.item_stack(&mut r, &mut w)?;
        w.raw(r.remaining());
        Ok(w.into_inner())
    }

    pub fn merchant_offers(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.varint(r.varint()?);
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            items.item_cost(&mut r, &mut w)?;
            items.item_stack(&mut r, &mut w)?;
            items.opt_item_cost(&mut r, &mut w)?;
            w.raw(r.bytes(1 + 4 + 4 + 4 + 4 + 4 + 4)?); // disabled, uses, max uses, xp, special price, multiplier, demand
        }
        w.raw(r.remaining());
        Ok(w.into_inner())
    }

    /// Serverbound SET_CREATIVE_MODE_SLOT: i16 slot + length-prefixed stack.
    pub fn set_creative_slot(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.i16(r.i16()?);
        items.length_prefixed_stack(&mut r, &mut w)?;
        Ok(w.into_inner())
    }

    /// Serverbound CONTAINER_CLICK with hashed stacks.
    pub fn container_click(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.varint(r.varint()?); // container
        w.varint(r.varint()?); // state
        w.i16(r.i16()?); // slot
        w.i8(r.i8()?); // button
        w.varint(r.varint()?); // mode
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            w.i16(r.i16()?);
            items.hashed_stack(&mut r, &mut w)?;
        }
        items.hashed_stack(&mut r, &mut w)?;
        Ok(w.into_inner())
    }

    // ---- commands --------------------------------------------------------------------------

    pub fn commands(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let n = Items::copy_varint(&mut r, &mut w)?;
        let string_parser = d::OLD_ARGUMENT_TYPE_NAMES.iter().position(|n| *n == "brigadier:string").unwrap_or(5) as i32;
        for _ in 0..n.max(0) {
            let flags = r.u8()?;
            w.u8(flags);
            let children = r.list(|r| r.varint())?;
            w.varint(children.len() as i32);
            for c in children {
                w.varint(c);
            }
            if flags & 0x08 != 0 {
                w.varint(r.varint()?);
            }
            let node_type = flags & 0x03;
            if node_type == 1 || node_type == 2 {
                w.string(r.string()?);
            }
            if node_type == 2 {
                let parser = r.varint()?;
                let name = d::NEW_ARGUMENT_TYPE_NAMES.get(parser as usize).copied().unwrap_or("");
                let mapped = d::ARGUMENT_TYPES.get(parser as usize).map_or(string_parser, |m| *m as i32);
                let registry_arg = matches!(name, "resource" | "resource_key" | "resource_or_tag" | "resource_or_tag_key" | "resource_selector");
                if registry_arg {
                    let registry = r.string()?;
                    let key = registry.strip_prefix("minecraft:").unwrap_or(registry);
                    if super::world::REMOVED_REGISTRIES.contains(&key) {
                        w.varint(string_parser);
                        w.varint(1); // quotable phrase
                    } else {
                        w.varint(mapped);
                        w.string(registry);
                    }
                } else {
                    w.varint(mapped);
                    Self::parser_properties(name, &mut r, &mut w)?;
                }
            }
            if flags & 0x10 != 0 {
                w.string(r.string()?);
            }
        }
        w.varint(r.varint()?); // root index
        Ok(w.into_inner())
    }

    fn parser_properties(name: &str, r: &mut Reader, w: &mut Writer) -> Result<()> {
        match name {
            "brigadier:float" | "brigadier:integer" => {
                let flags = r.u8()?;
                w.u8(flags);
                if flags & 1 != 0 {
                    Items::copy_bytes(r, w, 4)?;
                }
                if flags & 2 != 0 {
                    Items::copy_bytes(r, w, 4)?;
                }
            }
            "brigadier:double" | "brigadier:long" => {
                let flags = r.u8()?;
                w.u8(flags);
                if flags & 1 != 0 {
                    Items::copy_bytes(r, w, 8)?;
                }
                if flags & 2 != 0 {
                    Items::copy_bytes(r, w, 8)?;
                }
            }
            "brigadier:string" => {
                Items::copy_varint(r, w)?;
            }
            "entity" | "score_holder" => {
                w.u8(r.u8()?);
            }
            "time" => Items::copy_bytes(r, w, 4)?,
            _ => {}
        }
        Ok(())
    }

    // ---- recipes ---------------------------------------------------------------------------

    fn slot_display(items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let ty = r.varint()?;
        let name = d::NEW_SLOT_DISPLAY_NAMES.get(ty as usize).copied().unwrap_or("");
        if matches!(name, "with_any_potion" | "only_with_component" | "dyed") {
            // No 1.21.11 equivalent: show an empty slot, consume the payload.
            w.varint(0);
            let mut scratch = Writer::new();
            return Self::slot_display_body(items, name, r, &mut scratch);
        }
        w.varint(d::SLOT_DISPLAYS.get(ty as usize).map_or(0, |m| *m as i32));
        Self::slot_display_body(items, name, r, w)
    }

    fn slot_display_body(items: &Items, name: &str, r: &mut Reader, w: &mut Writer) -> Result<()> {
        match name {
            "empty" | "any_fuel" => {}
            "with_any_potion" => Self::slot_display(items, r, w)?,
            "only_with_component" => {
                Self::slot_display(items, r, w)?;
                let c = r.varint()?;
                w.varint(items.component_type(c).unwrap_or(0));
            }
            "item" => {
                w.varint(items.item(r.varint()?));
            }
            "item_stack" => items.template(r, w)?,
            "tag" => Items::copy_string(r, w)?,
            "dyed" | "with_remainder" => {
                Self::slot_display(items, r, w)?;
                Self::slot_display(items, r, w)?;
            }
            "smithing_trim" => {
                for _ in 0..3 {
                    Self::slot_display(items, r, w)?;
                }
            }
            "composite" => {
                let n = Items::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    Self::slot_display(items, r, w)?;
                }
            }
            other => return Err(err(&format!("unknown slot display {other}"))),
        }
        Ok(())
    }

    fn recipe_display(items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let ty = Items::copy_varint(r, w)?;
        let slots = match ty {
            0 => {
                let n = Items::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    Self::slot_display(items, r, w)?;
                }
                2
            }
            1 => {
                Items::copy_varint(r, w)?;
                Items::copy_varint(r, w)?;
                let n = Items::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    Self::slot_display(items, r, w)?;
                }
                2
            }
            2 => 4,
            3 => 3,
            4 => 5,
            other => return Err(err(&format!("unknown recipe display {other}"))),
        };
        for _ in 0..slots {
            Self::slot_display(items, r, w)?;
        }
        if ty == 2 {
            Items::copy_varint(r, w)?; // duration
            Items::copy_bytes(r, w, 4)?; // experience
        }
        Ok(())
    }

    pub fn recipe_book_add(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            Items::copy_varint(&mut r, &mut w)?; // display id
            Self::recipe_display(items, &mut r, &mut w)?;
            Items::copy_varint(&mut r, &mut w)?; // optional group
            Items::copy_varint(&mut r, &mut w)?; // category
            if Items::copy_bool(&mut r, &mut w)? {
                let m = Items::copy_varint(&mut r, &mut w)?;
                for _ in 0..m.max(0) {
                    items.holder_set(&mut r, &mut w, Reg::Item)?;
                }
            }
            w.u8(r.u8()?); // flags
        }
        w.raw(r.remaining());
        Ok(w.into_inner())
    }

    pub fn place_ghost_recipe(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.varint(r.varint()?);
        Self::recipe_display(items, &mut r, &mut w)?;
        Ok(w.into_inner())
    }

    pub fn update_recipes(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            w.string(r.string()?);
            let ids = r.list(|r| r.varint())?;
            w.varint(ids.len() as i32);
            for id in ids {
                w.varint(items.item(id));
            }
        }
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            items.holder_set(&mut r, &mut w, Reg::Item)?;
            Self::slot_display(items, &mut r, &mut w)?;
        }
        Ok(w.into_inner())
    }

    // ---- advancements ----------------------------------------------------------------------

    /// UPDATE_ADVANCEMENTS: the display icon is an item template in 26.1, a stack in 1.21.11.
    pub fn update_advancements(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.bool(r.bool()?);
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            w.string(r.string()?);
            Items::copy_opt_string(&mut r, &mut w)?;
            if Items::copy_bool(&mut r, &mut w)? {
                Items::copy_nbt(&mut r, &mut w)?; // title
                Items::copy_nbt(&mut r, &mut w)?; // description
                items.template(&mut r, &mut w)?; // icon
                Items::copy_varint(&mut r, &mut w)?; // frame
                let flags = r.i32()?;
                w.i32(flags);
                if flags & 1 != 0 {
                    w.string(r.string()?);
                }
                w.raw(r.bytes(8)?); // x, y
            }
            let reqs = Items::copy_varint(&mut r, &mut w)?;
            for _ in 0..reqs.max(0) {
                let m = Items::copy_varint(&mut r, &mut w)?;
                for _ in 0..m.max(0) {
                    w.string(r.string()?);
                }
            }
            w.bool(r.bool()?); // telemetry
        }
        w.raw(r.remaining()); // removed, progress
        Ok(w.into_inner())
    }

    // ---- world time ------------------------------------------------------------------------

    /// SET_TIME: 26.1 game time + clock list → 1.21.11 game time, day time, advance flag.
    /// `clocks` are the world_clock registry keys in id order (from configuration).
    pub fn set_time(state: &mut DayTime, clocks: &[String], payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let game_time = r.i64()?;
        let n = r.varint()?;
        let mut overworld: Option<(i64, bool)> = None;
        for _ in 0..n.max(0) {
            let clock = r.varint()?;
            let ticks = r.varlong()?;
            r.f32()?; // partial tick
            let rate = r.f32()?;
            let key = clocks.get(clock as usize).map(String::as_str).unwrap_or(if clock == 0 { "minecraft:overworld" } else { "" });
            if key.strip_prefix("minecraft:").unwrap_or(key) == "overworld" {
                overworld = Some((ticks, rate != 0.0));
            }
        }
        match overworld {
            Some((ticks, advance)) => {
                state.game_time = game_time;
                state.day_time = ticks;
                state.advance = advance;
            }
            None => {
                if state.advance {
                    state.day_time += game_time - state.game_time;
                }
                state.game_time = game_time;
            }
        }
        let mut w = Writer::new();
        w.i64(game_time);
        w.i64(state.day_time);
        w.bool(state.advance);
        Ok(w.into_inner())
    }

    // ---- interaction (serverbound) ---------------------------------------------------------

    /// 1.21.11 INTERACT (one packet with an action) → 26.1 INTERACT / ATTACK / SPECTATE_ENTITY.
    /// Returns (26.1 packet id, payload) or `None` to drop (plain "interact" always follows an
    /// "interact at" from vanilla clients, which carries the same information plus the location).
    pub fn interact(payload: &[u8], spectator: bool) -> Result<Option<(i32, Vec<u8>)>> {
        use super::ids::new;
        let mut r = Reader::new(payload);
        let entity = r.varint()?;
        let action = r.varint()?;
        let mut w = Writer::new();
        w.varint(entity);
        match action {
            0 => Ok(None),
            1 => Ok(Some((if spectator { new::S_SPECTATE_ENTITY } else { new::S_ATTACK }, w.into_inner()))),
            2 => {
                let x = f64::from(r.f32()?);
                let y = f64::from(r.f32()?);
                let z = f64::from(r.f32()?);
                w.varint(r.varint()?); // hand
                write_low_precision_vector(&mut w, x, y, z);
                w.bool(r.bool()?); // secondary action
                Ok(Some((new::S_INTERACT, w.into_inner())))
            }
            _ => Err(err("invalid interact action")),
        }
    }
}

/// Vanilla's low-precision vector (1.21.9+): 15 bits per axis with a shared integer scale.
pub fn write_low_precision_vector(w: &mut Writer, x: f64, y: f64, z: f64) {
    const MAX_Q: f64 = 32766.0;
    const ABS_MAX: f64 = ((1u64 << 34) - 1) as f64;
    let clean = |v: f64| if v.is_nan() { 0.0 } else { v.clamp(-ABS_MAX, ABS_MAX) };
    let (x, y, z) = (clean(x), clean(y), clean(z));
    let max_part = x.abs().max(y.abs()).max(z.abs());
    if max_part < 1.0 / MAX_Q {
        w.u8(0);
        return;
    }
    let scale = max_part.ceil() as i64;
    let too_large = (scale & 3) != scale;
    let scale_bits = if too_large { (scale & 3) | 4 } else { scale };
    let pack = |v: f64| ((v * 0.5 + 0.5) * MAX_Q).round() as i64;
    let s = scale as f64;
    let packed = scale_bits | (pack(x / s) << 3) | (pack(y / s) << 18) | (pack(z / s) << 33);
    w.u8(packed as u8);
    w.u8((packed >> 8) as u8);
    w.i32((packed >> 16) as i32);
    if too_large {
        w.varint((scale >> 2) as i32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_time_uses_the_overworld_clock() {
        let clocks = vec!["minecraft:overworld".to_string(), "minecraft:the_end".to_string()];
        let mut st = DayTime::default();
        let mut p = Writer::new();
        p.i64(100).varint(2);
        p.varint(1).varlong(5).f32(0.0).f32(1.0);
        p.varint(0).varlong(6000).f32(0.0).f32(1.0);
        let out = Misc::set_time(&mut st, &clocks, &p.buf).unwrap();
        let mut r = Reader::new(&out);
        assert_eq!(r.i64().unwrap(), 100);
        assert_eq!(r.i64().unwrap(), 6000);
        assert!(r.bool().unwrap());
        // no clocks: day time advances with game time
        let mut p = Writer::new();
        p.i64(150).varint(0);
        let out = Misc::set_time(&mut st, &clocks, &p.buf).unwrap();
        let mut r = Reader::new(&out);
        r.i64().unwrap();
        assert_eq!(r.i64().unwrap(), 6050);
    }

    #[test]
    fn interact_at_becomes_interact_with_vector() {
        let mut p = Writer::new();
        p.varint(9).varint(2).f32(0.5).f32(1.0).f32(-0.25).varint(0).bool(true);
        let (id, out) = Misc::interact(&p.buf, false).unwrap().unwrap();
        assert_eq!(id, super::super::ids::new::S_INTERACT);
        let mut r = Reader::new(&out);
        assert_eq!(r.varint().unwrap(), 9);
        assert_eq!(r.varint().unwrap(), 0);
        let first = r.u8().unwrap();
        assert_eq!(first & 3, 1); // scale 1
        r.bytes(5).unwrap();
        assert!(r.bool().unwrap());
        assert!(r.is_empty());
        // attack / plain interact
        let mut p = Writer::new();
        p.varint(9).varint(1).bool(false);
        assert_eq!(Misc::interact(&p.buf, true).unwrap().unwrap().0, super::super::ids::new::S_SPECTATE_ENTITY);
        let mut p = Writer::new();
        p.varint(9).varint(0).varint(0).bool(false);
        assert!(Misc::interact(&p.buf, false).unwrap().is_none());
    }

    #[test]
    fn low_precision_zero_and_large() {
        let mut w = Writer::new();
        write_low_precision_vector(&mut w, 0.0, 0.0, 0.0);
        assert_eq!(w.buf, vec![0]);
        let mut w = Writer::new();
        write_low_precision_vector(&mut w, 10.0, 0.0, 0.0);
        assert_eq!(w.buf[0] & 0b100, 0b100); // continuation flag for scale 10
        assert_eq!(w.buf.len(), 7);
    }

    #[test]
    fn item_particle_template_becomes_stack() {
        let items = Items::new(super::super::items::Dir::ToClient);
        let item_particle = d::NEW_PARTICLE_NAMES.iter().position(|n| *n == "item").unwrap() as i32;
        let mut p = Writer::new();
        p.varint(item_particle).varint(1).varint(1).varint(0).varint(0);
        let mut out = Writer::new();
        Misc::particle(&items, &mut Reader::new(&p.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        assert_eq!(r.varint().unwrap(), d::PARTICLES[item_particle as usize] as i32);
        assert_eq!(r.varint().unwrap(), 1); // count first now
        assert_eq!(r.varint().unwrap(), d::ITEMS[1] as i32);
    }
}
