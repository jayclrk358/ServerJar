//! Everything else 1.21.11 ↔ 1.21.9: particles, sounds, explosions, containers, commands,
//! recipes, advancements, statistics and world border timing.

use super::super::data_1_21_9 as d;
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

    /// One particle (id + data). Particle ids and data layouts are identical in both versions;
    /// only the item inside "item" particles needs remapping.
    pub fn particle(&self, items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let id = Items::copy_varint(r, w)?;
        let name = d::NEW_PARTICLE_NAMES.get(id as usize).copied().unwrap_or("");
        match name {
            "block" | "block_marker" | "falling_dust" | "dust_pillar" | "block_crumble" => {
                Items::copy_varint(r, w)?;
            }
            "item" => items.item_stack(r, w)?,
            "dust" => Items::copy_bytes(r, w, 8)?,
            "dust_color_transition" => Items::copy_bytes(r, w, 12)?,
            "vibration" => {
                match Items::copy_varint(r, w)? {
                    0 => Items::copy_bytes(r, w, 8)?,
                    1 => {
                        Items::copy_varint(r, w)?;
                        Items::copy_bytes(r, w, 4)?;
                    }
                    _ => return Err(err("unknown vibration source")),
                }
                Items::copy_varint(r, w)?;
            }
            "sculk_charge" | "dragon_breath" => Items::copy_bytes(r, w, 4)?,
            "shriek" => {
                Items::copy_varint(r, w)?;
            }
            "entity_effect" | "tinted_leaves" | "flash" => Items::copy_bytes(r, w, 4)?,
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
    pub fn level_particles(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.raw(r.bytes(2 + 24 + 12 + 4 + 4)?);
        self.particle(items, &mut r, &mut w)?;
        Ok(w.into_inner())
    }

    // ---- sounds ----------------------------------------------------------------------------

    pub fn sound(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        items.sound_event(&mut r, &mut w)?;
        w.raw(r.remaining());
        Ok(w.into_inner())
    }

    /// EXPLODE: center, radius, block count, optional knockback, particle, sound, block particles.
    pub fn explode(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.raw(r.bytes(24 + 4 + 4)?);
        if Items::copy_bool(&mut r, &mut w)? {
            w.raw(r.bytes(24)?);
        }
        self.particle(items, &mut r, &mut w)?;
        items.sound_event(&mut r, &mut w)?;
        w.raw(r.remaining()); // weighted block particles: ids identical
        Ok(w.into_inner())
    }

    // ---- world border / time ---------------------------------------------------------------

    /// SET_BORDER_LERP_SIZE / INITIALIZE_BORDER: 1.21.11 sends the lerp time in ticks, 1.21.9 in
    /// milliseconds. `doubles` is how many doubles precede it.
    pub fn border_lerp(&self, payload: &[u8], doubles: usize) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len() + 2);
        w.raw(r.bytes(doubles * 8)?);
        w.varlong(r.varlong()?.saturating_mul(50));
        w.raw(r.remaining());
        Ok(w.into_inner())
    }

    pub fn game_time(&self, payload: &[u8]) -> Result<i64> {
        Reader::new(payload).i64()
    }

    // ---- containers ------------------------------------------------------------------------

    pub fn container_set_content(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.varint(r.varint()?);
        w.varint(r.varint()?);
        items.item_stack_list(&mut r, &mut w)?;
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
            w.raw(r.bytes(1 + 4 * 6)?); // disabled, uses, max uses, xp, special price, multiplier, demand
        }
        w.raw(r.remaining());
        Ok(w.into_inner())
    }

    /// Serverbound SET_CREATIVE_MODE_SLOT: i16 slot + full stack.
    pub fn set_creative_slot(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.i16(r.i16()?);
        items.item_stack(&mut r, &mut w)?;
        Ok(w.into_inner())
    }

    /// Serverbound CONTAINER_CLICK with hashed stacks.
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
        items.hashed_stack(&mut r, &mut w)?;
        Ok(w.into_inner())
    }

    // ---- statistics ------------------------------------------------------------------------

    /// AWARD_STATS: stat categories hold block, item or entity ids.
    pub fn award_stats(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            let category = Items::copy_varint(&mut r, &mut w)?;
            let id = r.varint()?;
            w.varint(match category {
                0 => items.block(id),
                1..=5 => items.item(id),
                6 | 7 => items.entity(id),
                _ => id,
            });
            Items::copy_varint(&mut r, &mut w)?;
        }
        Ok(w.into_inner())
    }

    // ---- commands --------------------------------------------------------------------------

    /// COMMANDS: argument types are identical; only registry arguments that point at registries
    /// the old client lacks become plain strings.
    pub fn commands(&self, payload: &[u8]) -> Result<Vec<u8>> {
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
                if matches!(name, "resource" | "resource_key" | "resource_or_tag" | "resource_or_tag_key" | "resource_selector") {
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
                for bit in [1, 2] {
                    if flags & bit != 0 {
                        Items::copy_bytes(r, w, 4)?;
                    }
                }
            }
            "brigadier:double" | "brigadier:long" => {
                let flags = r.u8()?;
                w.u8(flags);
                for bit in [1, 2] {
                    if flags & bit != 0 {
                        Items::copy_bytes(r, w, 8)?;
                    }
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

    fn slot_display(&self, items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let ty = Items::copy_varint(r, w)?;
        let name = d::NEW_SLOT_DISPLAY_NAMES.get(ty as usize).copied().unwrap_or("");
        match name {
            "empty" | "any_fuel" => {}
            "item" => {
                let id = r.varint()?;
                w.varint(items.item(id));
            }
            "item_stack" => items.item_stack(r, w)?,
            "tag" => Items::copy_string(r, w)?,
            "smithing_trim" => {
                self.slot_display(items, r, w)?;
                self.slot_display(items, r, w)?;
                items.holder(r, w, Reg::Same, |_, r, w| {
                    Items::copy_string(r, w)?;
                    Items::copy_nbt(r, w)?;
                    Items::copy_bool(r, w)?;
                    Ok(())
                })?;
            }
            "with_remainder" => {
                self.slot_display(items, r, w)?;
                self.slot_display(items, r, w)?;
            }
            "composite" => {
                let n = Items::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    self.slot_display(items, r, w)?;
                }
            }
            other => return Err(err(&format!("unknown slot display {other}"))),
        }
        Ok(())
    }

    fn recipe_display(&self, items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let ty = Items::copy_varint(r, w)?;
        let slots = match ty {
            0 => {
                let n = Items::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    self.slot_display(items, r, w)?;
                }
                2
            }
            1 => {
                Items::copy_varint(r, w)?;
                Items::copy_varint(r, w)?;
                let n = Items::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    self.slot_display(items, r, w)?;
                }
                2
            }
            2 => 4,
            3 => 3,
            4 => 5,
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
        w.raw(r.remaining());
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
            self.slot_display(items, &mut r, &mut w)?;
        }
        Ok(w.into_inner())
    }

    // ---- advancements ----------------------------------------------------------------------

    pub fn update_advancements(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.bool(r.bool()?); // reset
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            w.string(r.string()?);
            Items::copy_opt_string(&mut r, &mut w)?; // parent
            if Items::copy_bool(&mut r, &mut w)? {
                Items::copy_nbt(&mut r, &mut w)?; // title
                Items::copy_nbt(&mut r, &mut w)?; // description
                items.item_stack(&mut r, &mut w)?; // icon
                Items::copy_varint(&mut r, &mut w)?; // frame
                let flags = r.i32()?;
                w.i32(flags);
                if flags & 1 != 0 {
                    w.string(r.string()?); // background
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translate::v1_21_9::items::Dir;

    #[test]
    fn border_lerp_ticks_to_millis() {
        let mut p = Writer::new();
        p.f64(10.0);
        p.f64(20.0);
        p.varlong(40);
        let out = Misc::new().border_lerp(&p.buf, 2).unwrap();
        let mut r = Reader::new(&out);
        r.f64().unwrap();
        r.f64().unwrap();
        assert_eq!(r.varlong().unwrap(), 2000);
        assert!(r.is_empty());
    }

    #[test]
    fn explode_remaps_sound() {
        let items = Items::new(Dir::ToClient);
        let mut p = Writer::new();
        p.raw(&[0u8; 32]);
        p.bool(false);
        let poof = d::NEW_PARTICLE_NAMES.iter().position(|n| *n == "explosion").unwrap() as i32;
        p.varint(poof);
        let last = d::SOUNDS.len() as i32 - 1;
        p.varint(last + 1);
        p.varint(0); // no block particles
        let out = Misc::new().explode(&items, &p.buf).unwrap();
        let mut r = Reader::new(&out);
        r.bytes(32).unwrap();
        assert!(!r.bool().unwrap());
        assert_eq!(r.varint().unwrap(), poof);
        assert_eq!(r.varint().unwrap(), d::SOUNDS[last as usize] as i32 + 1);
        assert_eq!(r.varint().unwrap(), 0);
        assert!(r.is_empty());
    }

    #[test]
    fn stats_remap_items() {
        let items = Items::new(Dir::ToClient);
        let spear = d::NEW_ITEM_NAMES.iter().position(|n| *n == "wooden_spear").unwrap() as i32;
        let mut p = Writer::new();
        p.varint(1);
        p.varint(1); // crafted
        p.varint(spear);
        p.varint(3);
        let out = Misc::new().award_stats(&items, &p.buf).unwrap();
        let mut r = Reader::new(&out);
        r.varint().unwrap();
        r.varint().unwrap();
        assert_eq!(d::OLD_ITEM_NAMES[r.varint().unwrap() as usize], "wooden_sword");
    }
}
