//! Everything else 1.21.4 ↔ 1.21.2: particles, sounds, explosions, containers, merchants,
//! recipes, advancements, chat hover items, held slot, vehicle movement.

use super::data_1_21_2 as d;
use super::items::{Items, Reg};
use crate::proto::buf::{DecodeError, Reader, Result, Writer};
use crate::proto::nbt::{self, Tag};

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

    /// One particle (id + data). The `trail` particle lost its duration in 1.21.2.
    pub fn particle(&self, items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let id = r.varint()?;
        let name = d::NEW_PARTICLE_NAMES.get(id as usize).copied().unwrap_or("");
        w.varint(items.particle(id));
        match name {
            "block" | "block_marker" | "falling_dust" | "dust_pillar" | "block_crumble" => {
                w.varint(items.block_state(r.varint()?));
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
            "sculk_charge" | "entity_effect" => Items::copy_bytes(r, w, 4)?,
            "shriek" => {
                Items::copy_varint(r, w)?;
            }
            "trail" => {
                Items::copy_bytes(r, w, 28)?; // target xyz, color
                r.varint()?; // duration (1.21.4 only)
            }
            _ => {}
        }
        Ok(())
    }

    /// LEVEL_PARTICLES: 1.21.4 added "always show" after "override limiter".
    pub fn level_particles(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.bool(r.bool()?); // override limiter
        r.bool()?; // always show
        w.raw(r.bytes(24 + 12 + 4 + 4)?); // xyz, offsets, speed, count
        self.particle(items, &mut r, &mut w)?;
        Ok(w.into_inner())
    }

    // ---- sounds and explosions -------------------------------------------------------------

    pub fn sound(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        items.sound_event(&mut r, &mut w)?;
        w.raw(r.remaining());
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

    /// A payload made of `leading_varints` varints followed by one item stack.
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
            // disabled, uses, max uses, xp, special price, price multiplier, demand
            w.raw(r.bytes(1 + 4 * 6)?);
        }
        w.raw(r.remaining()); // level, xp, regular, can restock
        Ok(w.into_inner())
    }

    /// SET_HELD_SLOT: 1.21.4 varint → 1.21.2 byte.
    pub fn set_held_slot(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let slot = Reader::new(payload).varint()?;
        Ok(vec![slot as u8])
    }

    /// Serverbound SET_CREATIVE_MODE_SLOT: i16 slot + full stack.
    pub fn set_creative_slot(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.i16(r.i16()?);
        items.item_stack(&mut r, &mut w)?;
        Ok(w.into_inner())
    }

    /// Serverbound CONTAINER_CLICK with full stacks.
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
            items.item_stack(&mut r, &mut w)?;
        }
        items.item_stack(&mut r, &mut w)?; // carried
        Ok(w.into_inner())
    }

    /// Serverbound MOVE_VEHICLE: 1.21.4 appended an on-ground flag.
    pub fn move_vehicle(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len() + 1);
        w.raw(r.bytes(24 + 8)?); // xyz, yaw, pitch
        w.bool(true);
        Ok(w.into_inner())
    }

    // ---- recipes ---------------------------------------------------------------------------

    fn slot_display(&self, items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let ty = Items::copy_varint(r, w)?;
        match d::NEW_SLOT_DISPLAY_NAMES.get(ty as usize).copied().unwrap_or("") {
            "empty" | "any_fuel" => {}
            "item" => {
                w.varint(items.item(r.varint()?));
            }
            "item_stack" => items.item_stack(r, w)?,
            "tag" => Items::copy_string(r, w)?,
            "smithing_trim" => {
                for _ in 0..3 {
                    self.slot_display(items, r, w)?;
                }
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
                // crafting_shapeless: ingredients, result, station
                let n = Items::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    self.slot_display(items, r, w)?;
                }
                2
            }
            1 => {
                // crafting_shaped: width, height, ingredients, result, station
                Items::copy_varint(r, w)?;
                Items::copy_varint(r, w)?;
                let n = Items::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    self.slot_display(items, r, w)?;
                }
                2
            }
            2 => 4, // furnace: ingredient, fuel, result, station (+ duration, xp)
            3 => 3, // stonecutter: input, result, station
            4 => 5, // smithing: template, base, addition, result, station
            other => return Err(err(&format!("unknown recipe display {other}"))),
        };
        for _ in 0..slots {
            self.slot_display(items, r, w)?;
        }
        if ty == 2 {
            Items::copy_varint(r, w)?;
            Items::copy_bytes(r, w, 4)?;
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
        w.bool(r.bool()?);
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

    // ---- text components -------------------------------------------------------------------

    /// Rewrites the first `components` NBT text components of a packet, keeping the rest.
    pub fn text_packet(&self, payload: &[u8], components: usize) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        for _ in 0..components {
            let mut tag = nbt::read(&mut r)?;
            strip_hover_items(&mut tag);
            nbt::write(&mut w, &tag);
        }
        w.raw(r.remaining());
        Ok(w.into_inner())
    }
}

/// `show_item` hover events carry item components in their 1.21.4 NBT form; the 1.21.2 client
/// fails to decode `custom_model_data` and `trim` there, so they are removed.
fn strip_hover_items(tag: &mut Tag) {
    match tag {
        Tag::Compound(entries) => {
            let is_show_item = entries.iter().any(|(k, v)| k == "action" && v.as_str().is_some_and(|a| a.ends_with("show_item")));
            for (key, value) in entries.iter_mut() {
                if is_show_item && key == "contents" {
                    if let Some(components) = value.get_mut("components") {
                        for k in ["minecraft:custom_model_data", "custom_model_data", "minecraft:trim", "trim"] {
                            components.remove(k);
                        }
                    }
                }
                strip_hover_items(value);
            }
        }
        Tag::List(list) => list.iter_mut().for_each(strip_hover_items),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::super::items::Dir;
    use super::*;

    fn particle_id(name: &str) -> i32 {
        d::NEW_PARTICLE_NAMES.iter().position(|n| *n == name).unwrap() as i32
    }

    #[test]
    fn level_particles_drop_always_show_and_trail_duration() {
        let items = Items::new(Dir::ToClient);
        let misc = Misc::new();
        let mut p = Writer::new();
        p.bool(true);
        p.bool(true); // always show
        p.raw(&[0u8; 44]);
        p.varint(particle_id("trail"));
        p.raw(&[1u8; 28]);
        p.varint(40);
        let out = misc.level_particles(&items, &p.buf).unwrap();
        let mut r = Reader::new(&out);
        assert!(r.bool().unwrap());
        r.bytes(44).unwrap();
        assert_eq!(r.varint().unwrap(), items.particle(particle_id("trail")));
        assert_eq!(r.bytes(28).unwrap(), &[1u8; 28]);
        assert!(r.is_empty());
    }

    #[test]
    fn held_slot_and_vehicle() {
        let misc = Misc::new();
        let mut p = Writer::new();
        p.varint(8);
        assert_eq!(misc.set_held_slot(&p.buf).unwrap(), vec![8]);
        let v = misc.move_vehicle(&[0u8; 32]).unwrap();
        assert_eq!(v.len(), 33);
        assert_eq!(v[32], 1);
    }

    #[test]
    fn hover_item_components_are_stripped() {
        let misc = Misc::new();
        let tag = Tag::Compound(vec![
            ("text".into(), Tag::String("x".into())),
            (
                "hoverEvent".into(),
                Tag::Compound(vec![
                    ("action".into(), Tag::String("show_item".into())),
                    (
                        "contents".into(),
                        Tag::Compound(vec![
                            ("id".into(), Tag::String("minecraft:stick".into())),
                            (
                                "components".into(),
                                Tag::Compound(vec![
                                    ("minecraft:custom_model_data".into(), Tag::compound()),
                                    ("minecraft:damage".into(), Tag::Int(1)),
                                ]),
                            ),
                        ]),
                    ),
                ]),
            ),
        ]);
        let mut p = Writer::new();
        nbt::write(&mut p, &tag);
        p.bool(false); // overlay
        let out = misc.text_packet(&p.buf, 1).unwrap();
        let mut r = Reader::new(&out);
        let back = nbt::read(&mut r).unwrap();
        let comps = back.get("hoverEvent").unwrap().get("contents").unwrap().get("components").unwrap();
        assert!(comps.get("minecraft:custom_model_data").is_none());
        assert!(comps.get("minecraft:damage").is_some());
        assert!(!r.bool().unwrap());
        assert!(r.is_empty());
    }
}
