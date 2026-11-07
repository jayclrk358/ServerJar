//! Everything else 1.21.7 ↔ 1.21.6 that carries item or sound ids: particles, sounds, cooldowns,
//! containers, trades, recipes, advancements and statistics. Layouts are identical in both
//! versions; only ids are remapped.

use super::super::data_1_21_6 as d;
use super::items::{Items, Reg};
use crate::proto::buf::{DecodeError, Reader, Result, Writer};

fn err(msg: &str) -> DecodeError {
    DecodeError::Other(msg.to_string())
}

#[derive(Default)]
pub struct Misc;

impl Misc {
    pub fn new() -> Self {
        Self
    }

    // ---- particles -------------------------------------------------------------------------

    /// One particle (id + data), with the 1.21.5 data readers.
    pub fn particle(&self, items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let id = r.varint()?;
        let names: &[&str] = &d::NEW_PARTICLE_NAMES;
        let name = if items.dir == super::items::Dir::ToClient { names.get(id as usize).copied() } else { d::OLD_PARTICLE_NAMES.get(id as usize).copied() };
        w.varint(items.particle(id));
        match name.unwrap_or("") {
            "block" | "block_marker" | "falling_dust" | "dust_pillar" | "block_crumble" => {
                w.varint(items.block_state(r.varint()?));
            }
            "item" => items.item_stack(r, w)?,
            "dust" => Items::copy_bytes(r, w, 8)?,                   // color, scale
            "dust_color_transition" => Items::copy_bytes(r, w, 12)?, // from, to, scale
            "vibration" => {
                match Items::copy_varint(r, w)? {
                    0 => Items::copy_bytes(r, w, 8)?, // block position
                    1 => {
                        Items::copy_varint(r, w)?; // entity id
                        Items::copy_bytes(r, w, 4)?; // eye height
                    }
                    _ => return Err(err("unknown vibration source")),
                }
                Items::copy_varint(r, w)?; // arrival ticks
            }
            "sculk_charge" | "entity_effect" | "tinted_leaves" => Items::copy_bytes(r, w, 4)?,
            "shriek" => {
                Items::copy_varint(r, w)?;
            }
            "trail" => {
                Items::copy_bytes(r, w, 28)?; // target, color
                Items::copy_varint(r, w)?; // duration
            }
            _ => {}
        }
        Ok(())
    }

    /// LEVEL_PARTICLES: fixed header (flags, position, offsets, speed, count), then the particle.
    pub fn level_particles(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.raw(r.bytes(2 + 24 + 12 + 4 + 4)?);
        self.particle(items, &mut r, &mut w)?;
        Ok(w.into_inner())
    }

    pub fn explode(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.raw(r.bytes(24)?); // center
        if Items::copy_bool(&mut r, &mut w)? {
            w.raw(r.bytes(24)?); // player knockback
        }
        self.particle(items, &mut r, &mut w)?;
        items.sound_event(&mut r, &mut w)?;
        Ok(w.into_inner())
    }

    // ---- sounds ----------------------------------------------------------------------------

    /// SOUND / SOUND_ENTITY: sound holder first, everything after it is unchanged.
    pub fn sound(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        items.sound_event(&mut r, &mut w)?;
        w.raw(r.remaining());
        Ok(w.into_inner())
    }

    /// STOP_SOUND: flags, optional source, optional sound name.
    pub fn stop_sound(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let flags = r.u8()?;
        w.u8(flags);
        if flags & 1 != 0 {
            Items::copy_varint(&mut r, &mut w)?;
        }
        if flags & 2 != 0 {
            w.string(&items.sound_name(r.string()?));
        }
        Ok(w.into_inner())
    }

    /// COOLDOWN (1.21.2+): cooldown group name (an item id by default), ticks.
    pub fn cooldown(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.string(&items.item_name(r.string()?));
        w.raw(r.remaining());
        Ok(w.into_inner())
    }

    // ---- containers ------------------------------------------------------------------------

    pub fn container_set_content(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.varint(r.varint()?); // container
        w.varint(r.varint()?); // state id
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            items.item_stack(&mut r, &mut w)?;
        }
        items.item_stack(&mut r, &mut w)?; // carried
        Ok(w.into_inner())
    }

    pub fn container_set_slot(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.varint(r.varint()?);
        w.varint(r.varint()?);
        w.i16(r.i16()?);
        items.item_stack(&mut r, &mut w)?;
        Ok(w.into_inner())
    }

    pub fn single_item(&self, items: &Items, payload: &[u8], leading_varints: usize) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        for _ in 0..leading_varints {
            w.varint(r.varint()?);
        }
        items.item_stack(&mut r, &mut w)?;
        w.raw(r.remaining());
        Ok(w.into_inner())
    }

    pub fn merchant_offers(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
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
        w.raw(r.remaining()); // villager level, xp, show progress, can restock
        Ok(w.into_inner())
    }

    /// Serverbound SET_CREATIVE_MODE_SLOT: i16 slot + length-prefixed stack (1.21.6 → 1.21.7).
    pub fn set_creative_slot(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.i16(r.i16()?);
        items.length_prefixed_stack(&mut r, &mut w)?;
        Ok(w.into_inner())
    }

    /// Serverbound CONTAINER_CLICK with hashed stacks (1.21.6 → 1.21.7).
    pub fn container_click(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
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
        items.hashed_stack(&mut r, &mut w)?; // carried
        Ok(w.into_inner())
    }

    // ---- recipes ---------------------------------------------------------------------------

    fn slot_display(&self, items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let ty = r.varint()?;
        w.varint(d::SLOT_DISPLAYS.get(ty as usize).map_or(ty, |v| *v as i32));
        match d::NEW_SLOT_DISPLAY_NAMES.get(ty as usize).copied().unwrap_or("") {
            "empty" | "any_fuel" => {}
            "item" => {
                w.varint(items.item(r.varint()?));
            }
            "item_stack" => items.item_stack(r, w)?,
            "tag" => Items::copy_string(r, w)?,
            "smithing_trim" => {
                self.slot_display(items, r, w)?; // base
                self.slot_display(items, r, w)?; // material
                items.holder(r, w, Reg::Same, |_, r, w| {
                    Items::copy_string(r, w)?;
                    Items::copy_nbt(r, w)?;
                    Items::copy_bool(r, w)?;
                    Ok(())
                })?; // pattern
            }
            "with_remainder" => {
                self.slot_display(items, r, w)?;
                self.slot_display(items, r, w)?;
            }
            "composite" => self.slot_displays(items, r, w)?,
            other => return Err(err(&format!("unknown slot display {other}"))),
        }
        Ok(())
    }

    fn slot_displays(&self, items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let n = Items::copy_varint(r, w)?;
        for _ in 0..n.max(0) {
            self.slot_display(items, r, w)?;
        }
        Ok(())
    }

    fn recipe_display(&self, items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let ty = Items::copy_varint(r, w)?;
        let slots = match ty {
            0 => {
                self.slot_displays(items, r, w)?; // ingredients
                2 // result, station
            }
            1 => {
                Items::copy_varint(r, w)?; // width
                Items::copy_varint(r, w)?; // height
                self.slot_displays(items, r, w)?;
                2
            }
            2 => 4, // ingredient, fuel, result, station
            3 => 3, // input, result, station
            4 => 5, // template, base, addition, result, station
            other => return Err(err(&format!("unknown recipe display {other}"))),
        };
        for _ in 0..slots {
            self.slot_display(items, r, w)?;
        }
        if ty == 2 {
            Items::copy_varint(r, w)?; // duration
            Items::copy_bytes(r, w, 4)?; // experience
        }
        Ok(())
    }

    pub fn recipe_book_add(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            Items::copy_varint(&mut r, &mut w)?; // display id
            self.recipe_display(items, &mut r, &mut w)?;
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
        w.raw(r.remaining()); // replace
        Ok(w.into_inner())
    }

    pub fn place_ghost_recipe(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.varint(r.varint()?);
        self.recipe_display(items, &mut r, &mut w)?;
        Ok(w.into_inner())
    }

    pub fn update_recipes(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            w.string(r.string()?); // property set
            let ids = r.list(|r| r.varint())?;
            w.varint(ids.len() as i32);
            for id in ids {
                w.varint(items.item(id));
            }
        }
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            items.holder_set(&mut r, &mut w, Reg::Item)?; // stonecutter input
            self.slot_display(items, &mut r, &mut w)?;
        }
        Ok(w.into_inner())
    }

    // ---- advancements and statistics -------------------------------------------------------

    pub fn update_advancements(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.bool(r.bool()?); // reset
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            Items::copy_string(&mut r, &mut w)?; // id
            Items::copy_opt_string(&mut r, &mut w)?; // parent
            if Items::copy_bool(&mut r, &mut w)? {
                Items::copy_nbt(&mut r, &mut w)?; // title
                Items::copy_nbt(&mut r, &mut w)?; // description
                items.item_stack(&mut r, &mut w)?; // icon
                Items::copy_varint(&mut r, &mut w)?; // frame
                let flags = r.i32()?;
                w.i32(flags);
                if flags & 1 != 0 {
                    Items::copy_string(&mut r, &mut w)?; // background
                }
                Items::copy_bytes(&mut r, &mut w, 8)?; // x, y
            }
            let reqs = Items::copy_varint(&mut r, &mut w)?;
            for _ in 0..reqs.max(0) {
                let m = Items::copy_varint(&mut r, &mut w)?;
                for _ in 0..m.max(0) {
                    Items::copy_string(&mut r, &mut w)?;
                }
            }
            Items::copy_bool(&mut r, &mut w)?; // sends telemetry
        }
        w.raw(r.remaining()); // removed, progress, show advancements
        Ok(w.into_inner())
    }

    /// AWARD_STATS: (stat type, stat id, value); ids of block, item and entity stats are remapped.
    pub fn award_stats(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            let category = Items::copy_varint(&mut r, &mut w)?;
            let id = r.varint()?;
            w.varint(match category {
                0 => items.block(id),       // mined
                1..=5 => items.item(id),    // crafted, used, broken, picked up, dropped
                6 | 7 => items.entity(id),  // killed, killed by
                _ => id,                    // custom
            });
            Items::copy_varint(&mut r, &mut w)?;
        }
        Ok(w.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::super::items::Dir;
    use super::*;

    #[test]
    fn sound_holder_ids_are_remapped() {
        let items = Items::new(Dir::ToClient);
        let last = d::NEW_SOUND_NAMES.len() as i32 - 1;
        let mut p = Writer::new();
        p.varint(last + 1);
        p.varint(0); // source
        p.raw(&[9u8; 12 + 8 + 8]);
        let out = Misc::new().sound(&items, &p.buf).unwrap();
        let mut r = Reader::new(&out);
        assert_eq!(r.varint().unwrap(), d::SOUNDS[last as usize] as i32 + 1);
        assert_eq!(r.remaining(), &p.buf[p.buf.len() - 29..]);
    }

    #[test]
    fn item_particle_in_level_particles() {
        let items = Items::new(Dir::ToClient);
        let item_particle = d::NEW_PARTICLE_NAMES.iter().position(|n| *n == "item").unwrap() as i32;
        let disc = d::NEW_ITEM_NAMES.iter().position(|n| *n == "music_disc_lava_chicken").unwrap() as i32;
        let mut p = Writer::new();
        p.raw(&[0u8; 46]);
        p.varint(item_particle);
        p.varint(1);
        p.varint(disc);
        p.varint(0);
        p.varint(0);
        let out = Misc::new().level_particles(&items, &p.buf).unwrap();
        let mut r = Reader::new(&out);
        r.bytes(46).unwrap();
        assert_eq!(r.varint().unwrap(), d::PARTICLES[item_particle as usize] as i32);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), d::OLD_ITEM_NAMES.iter().position(|n| *n == "music_disc_pigstep").unwrap() as i32);
    }

    #[test]
    fn stop_sound_and_cooldown_names() {
        let items = Items::new(Dir::ToClient);
        let mut p = Writer::new();
        p.u8(2);
        p.string("minecraft:music_disc.lava_chicken");
        let out = Misc::new().stop_sound(&items, &p.buf).unwrap();
        let mut r = Reader::new(&out);
        assert_eq!(r.u8().unwrap(), 2);
        assert_eq!(r.string().unwrap(), "minecraft:music_disc.pigstep");
        let mut p = Writer::new();
        p.string("minecraft:music_disc_lava_chicken");
        p.varint(20);
        let out = Misc::new().cooldown(&items, &p.buf).unwrap();
        let mut r = Reader::new(&out);
        assert_eq!(r.string().unwrap(), "minecraft:music_disc_pigstep");
        assert_eq!(r.varint().unwrap(), 20);
    }
}
