//! Everything else 26.2 → 26.1: particles, sounds, explosions, containers, commands, recipes,
//! advancements, teams, spectating.

use super::data_26_1 as d;
use super::items::{Items, Reg};
use crate::proto::buf::{DecodeError, Reader, Result, Writer};

fn err(msg: &str) -> DecodeError {
    DecodeError::Other(msg.to_string())
}

/// Team color 26.1 shows when 26.2 sends "no color".
const WHITE_TEAM_COLOR: i32 = 15;

#[derive(Default)]
pub struct Misc;

impl Misc {
    pub fn new() -> Self {
        Self
    }

    // ---- particles -------------------------------------------------------------------------

    /// One particle (id + data). Particles 26.1 lacks are replaced and their data dropped.
    pub fn particle(&self, items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let id = r.varint()?;
        let name = d::NEW_PARTICLE_NAMES.get(id as usize).copied().unwrap_or("");
        let mapped = items.particle(id);
        w.varint(mapped);
        let mapped_name = d::OLD_PARTICLE_NAMES.get(mapped as usize).copied().unwrap_or("");
        let mut data = Writer::new();
        match name {
            "block" | "block_marker" | "falling_dust" | "dust_pillar" | "block_crumble" => {
                data.varint(items.block_state(r.varint()?));
            }
            "item" => items.item_stack(r, &mut data)?,
            "dust" => Items::copy_bytes(r, &mut data, 8)?,
            "dust_color_transition" => Items::copy_bytes(r, &mut data, 12)?,
            "vibration" => {
                let source = Items::copy_varint(r, &mut data)?;
                match source {
                    0 => Items::copy_bytes(r, &mut data, 8)?,
                    1 => {
                        Items::copy_varint(r, &mut data)?;
                        Items::copy_bytes(r, &mut data, 4)?;
                    }
                    _ => return Err(err("unknown vibration source")),
                }
                Items::copy_varint(r, &mut data)?;
            }
            "sculk_charge" | "dragon_breath" | "entity_effect" | "tinted_leaves" | "flash" | "geyser" | "geyser_plume" => {
                Items::copy_bytes(r, &mut data, 4)?
            }
            "shriek" => {
                Items::copy_varint(r, &mut data)?;
            }
            "effect" | "instant_effect" | "geyser_base" | "geyser_poof" => Items::copy_bytes(r, &mut data, 8)?,
            "trail" => {
                Items::copy_bytes(r, &mut data, 28)?;
                Items::copy_varint(r, &mut data)?;
            }
            _ => {}
        }
        // Data only survives when the client sees the same particle kind.
        if mapped_name == name {
            w.raw(&data.buf);
        }
        Ok(())
    }

    /// LEVEL_PARTICLES: same layout in both versions; only the particle changes.
    pub fn level_particles(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.raw(r.bytes(2 + 24 + 12 + 4 + 4)?); // flags, position, offsets, speed, count
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

    pub fn explode(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.raw(r.bytes(24)?); // center
        w.raw(r.bytes(8)?); // radius, block count
        if Items::copy_bool(&mut r, &mut w)? {
            w.raw(r.bytes(24)?); // knockback
        }
        self.particle(items, &mut r, &mut w)?;
        items.sound_event(&mut r, &mut w)?;
        // Weighted block particles: particle, scaling, speed, weight.
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            self.particle(items, &mut r, &mut w)?;
            w.raw(r.bytes(8)?);
            w.varint(r.varint()?);
        }
        w.raw(r.remaining());
        Ok(w.into_inner())
    }

    // ---- containers ------------------------------------------------------------------------

    pub fn container_set_content(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
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
        w.raw(r.remaining());
        Ok(w.into_inner())
    }

    /// Serverbound SET_CREATIVE_MODE_SLOT: i16 slot + full stack.
    pub fn set_creative_slot(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.i16(r.i16()?);
        items.length_prefixed_stack(&mut r, &mut w)?;
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

    /// Serverbound SPECTATE_ENTITY: 26.1 plain varint → 26.2 optional varint.
    pub fn spectate_entity(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        w.bool(true);
        w.varint(r.varint()?);
        Ok(w.into_inner())
    }

    // ---- teams -----------------------------------------------------------------------------

    /// SET_PLAYER_TEAM: 26.2 reordered the team parameters and made the color optional.
    pub fn set_player_team(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.string(r.string()?);
        let action = r.i8()?;
        w.i8(action);
        if action == 0 || action == 2 {
            Items::copy_nbt(&mut r, &mut w)?; // display name
            let mut prefix = Writer::new();
            Items::copy_nbt(&mut r, &mut prefix)?;
            let mut suffix = Writer::new();
            Items::copy_nbt(&mut r, &mut suffix)?;
            let visibility = r.varint()?;
            let collision = r.varint()?;
            let color = if r.bool()? { r.varint()? } else { WHITE_TEAM_COLOR };
            let flags = r.i8()?;
            w.i8(flags);
            w.varint(visibility);
            w.varint(collision);
            w.varint(color);
            w.raw(&prefix.buf);
            w.raw(&suffix.buf);
        }
        w.raw(r.remaining()); // entities
        Ok(w.into_inner())
    }

    // ---- commands --------------------------------------------------------------------------

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

    fn slot_display(&self, items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let ty = Items::copy_varint(r, w)?;
        let name = d::NEW_SLOT_DISPLAY_NAMES.get(ty as usize).copied().unwrap_or("");
        match name {
            "empty" | "any_fuel" => {}
            "tag" => Items::copy_string(r, w)?,
            "with_any_potion" => self.slot_display(items, r, w)?,
            "only_with_component" => {
                self.slot_display(items, r, w)?;
                let c = r.varint()?;
                w.varint(items.component_type(c).unwrap_or(0));
            }
            "item" => {
                w.varint(items.item(r.varint()?));
            }
            "item_stack" => items.item_template(r, w)?,
            "dyed" | "with_remainder" => {
                self.slot_display(items, r, w)?;
                self.slot_display(items, r, w)?;
            }
            "smithing_trim" => {
                for _ in 0..3 {
                    self.slot_display(items, r, w)?;
                }
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
        w.bool(r.bool()?);
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            w.string(r.string()?);
            Items::copy_opt_string(&mut r, &mut w)?;
            if Items::copy_bool(&mut r, &mut w)? {
                Items::copy_nbt(&mut r, &mut w)?; // title
                Items::copy_nbt(&mut r, &mut w)?; // description
                items.item_template(&mut r, &mut w)?; // icon
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
}

#[cfg(test)]
mod tests {
    use super::super::items::Dir;
    use super::*;

    fn particle_id(names: &[&str], name: &str) -> i32 {
        names.iter().position(|n| *n == name).unwrap() as i32
    }

    #[test]
    fn new_particle_data_is_dropped() {
        let items = Items::new(Dir::ToClient);
        let misc = Misc::new();
        let mut p = Writer::new();
        p.varint(particle_id(&d::NEW_PARTICLE_NAMES, "geyser_base"));
        p.i32(3);
        p.f32(0.5);
        let mut out = Writer::new();
        let mut r = Reader::new(&p.buf);
        misc.particle(&items, &mut r, &mut out).unwrap();
        assert!(r.is_empty());
        let mut o = Reader::new(&out.buf);
        assert_eq!(o.varint().unwrap(), particle_id(&d::OLD_PARTICLE_NAMES, "poof"));
        assert!(o.is_empty());
    }

    #[test]
    fn block_particle_keeps_remapped_state() {
        let items = Items::new(Dir::ToClient);
        let misc = Misc::new();
        let mut p = Writer::new();
        p.varint(particle_id(&d::NEW_PARTICLE_NAMES, "block"));
        p.varint(1);
        let mut out = Writer::new();
        misc.particle(&items, &mut Reader::new(&p.buf), &mut out).unwrap();
        let mut o = Reader::new(&out.buf);
        assert_eq!(o.varint().unwrap(), particle_id(&d::OLD_PARTICLE_NAMES, "block"));
        assert_eq!(o.varint().unwrap(), 1);
    }

    #[test]
    fn team_parameters_are_reordered() {
        let misc = Misc::new();
        let mut p = Writer::new();
        p.string("red");
        p.i8(0);
        p.u8(8); // display name: TAG_String ""
        p.u16(0);
        p.u8(8); // prefix
        p.u16(0);
        p.u8(8); // suffix
        p.u16(0);
        p.varint(1); // visibility
        p.varint(2); // collision
        p.bool(false); // no color
        p.i8(3); // flags
        p.varint(0); // entities
        let out = misc.set_player_team(&p.buf).unwrap();
        let mut r = Reader::new(&out);
        assert_eq!(r.string().unwrap(), "red");
        assert_eq!(r.i8().unwrap(), 0);
        r.bytes(3).unwrap();
        assert_eq!(r.i8().unwrap(), 3);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), 2);
        assert_eq!(r.varint().unwrap(), WHITE_TEAM_COLOR);
        r.bytes(6).unwrap();
        assert_eq!(r.varint().unwrap(), 0);
        assert!(r.is_empty());
    }

    #[test]
    fn team_color_argument_becomes_color() {
        let misc = Misc::new();
        let mut p = Writer::new();
        p.varint(1);
        p.u8(0x02); // argument node
        p.varint(0);
        p.string("c");
        p.varint(particle_id(&d::NEW_ARGUMENT_TYPE_NAMES, "team_color"));
        p.varint(0); // root
        let out = misc.commands(&p.buf).unwrap();
        let mut r = Reader::new(&out);
        r.varint().unwrap();
        r.u8().unwrap();
        r.varint().unwrap();
        r.string().unwrap();
        let mapped = r.varint().unwrap();
        assert_eq!(d::OLD_ARGUMENT_TYPE_NAMES[mapped as usize], "color");
    }

    #[test]
    fn spectate_becomes_optional() {
        let misc = Misc::new();
        let mut p = Writer::new();
        p.varint(77);
        let out = misc.spectate_entity(&p.buf).unwrap();
        assert_eq!(out, vec![1, 77]);
    }
}
