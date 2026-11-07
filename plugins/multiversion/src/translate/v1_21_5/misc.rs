//! Everything else 1.21.6 ↔ 1.21.5: particles, sounds, difficulty, commands, recipes,
//! advancements, attributes, containers and merchant offers.

use super::super::data_1_21_5 as d;
use super::items::{copy_bool, copy_bytes, copy_nbt, copy_opt_string, copy_string, copy_varint, err, Items, Reg};
use crate::proto::buf::{Reader, Result, Writer};

/// Sound source 10 ("ui") is new in 1.21.6; the old client gets "master".
const UI_SOURCE: i32 = 10;

#[derive(Default)]
pub struct Misc;

impl Misc {
    pub fn new() -> Self {
        Self
    }

    // ---- particles -------------------------------------------------------------------------

    /// One particle: id + data (1.21.5 and 1.21.6 data formats are identical).
    pub fn particle(&self, items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let id = r.varint()?;
        let names: &[&str] = &d::NEW_PARTICLE_NAMES;
        let name = if items.dir == super::items::Dir::ToClient { names.get(id as usize).copied().unwrap_or("") } else { "" };
        w.varint(items.particle(id));
        match name {
            "block" | "block_marker" | "falling_dust" | "dust_pillar" | "block_crumble" => {
                w.varint(items.block_state(r.varint()?));
            }
            "item" => items.item_stack(r, w)?,
            "dust" => copy_bytes(r, w, 8)?,
            "dust_color_transition" => copy_bytes(r, w, 12)?,
            "vibration" => {
                match copy_varint(r, w)? {
                    0 => copy_bytes(r, w, 8)?,
                    1 => {
                        copy_varint(r, w)?;
                        copy_bytes(r, w, 4)?;
                    }
                    _ => return Err(err("unknown vibration source")),
                }
                copy_varint(r, w)?;
            }
            "sculk_charge" | "entity_effect" | "tinted_leaves" => copy_bytes(r, w, 4)?,
            "shriek" => {
                copy_varint(r, w)?;
            }
            "trail" => {
                copy_bytes(r, w, 28)?;
                copy_varint(r, w)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// LEVEL_PARTICLES: flags, position, offsets, speed, count, then the particle.
    pub fn level_particles(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        copy_bytes(&mut r, &mut w, 2 + 24 + 12 + 4 + 4)?;
        self.particle(items, &mut r, &mut w)?;
        Ok(w.into_inner())
    }

    // ---- sounds ----------------------------------------------------------------------------

    /// SOUND / SOUND_ENTITY: `None` drops a sound the old client does not have.
    pub fn sound(&self, items: &Items, payload: &[u8]) -> Result<Option<Vec<u8>>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let holder = r.varint()?;
        if holder != 0 && !items.sound_mapped(holder - 1) {
            return Ok(None);
        }
        r.set_position(0);
        items.sound_event(&mut r, &mut w)?;
        let source = r.varint()?;
        w.varint(if source == UI_SOURCE { 0 } else { source });
        w.raw(r.remaining());
        Ok(Some(w.into_inner()))
    }

    pub fn stop_sound(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let flags = r.u8()?;
        w.u8(flags);
        if flags & 1 != 0 {
            let source = r.varint()?;
            w.varint(if source == UI_SOURCE { 0 } else { source });
        }
        w.raw(r.remaining());
        Ok(w.into_inner())
    }

    /// EXPLODE (1.21.2-1.21.8 layout): center, optional knockback, particle, sound.
    pub fn explode(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        copy_bytes(&mut r, &mut w, 24)?;
        if copy_bool(&mut r, &mut w)? {
            copy_bytes(&mut r, &mut w, 24)?;
        }
        self.particle(items, &mut r, &mut w)?;
        items.sound_event(&mut r, &mut w)?;
        w.raw(r.remaining());
        Ok(w.into_inner())
    }

    // ---- difficulty ------------------------------------------------------------------------

    /// Clientbound CHANGE_DIFFICULTY: 1.21.6 varint → 1.21.5 unsigned byte (then the lock flag).
    pub fn change_difficulty(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        w.u8(r.varint()?.clamp(0, 255) as u8);
        w.raw(r.remaining());
        Ok(w.into_inner())
    }

    /// Serverbound CHANGE_DIFFICULTY: unsigned byte → varint.
    pub fn change_difficulty_serverbound(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        w.varint(i32::from(r.u8()?));
        Ok(w.into_inner())
    }

    /// Serverbound PLAYER_COMMAND: 1.21.6 removed press/release shift (sneaking comes from the
    /// player input packet, which the 1.21.5 client also sends). `None` drops the packet.
    pub fn player_command(&self, payload: &[u8]) -> Result<Option<Vec<u8>>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        copy_varint(&mut r, &mut w)?;
        let action = r.varint()?;
        if action < 2 {
            return Ok(None);
        }
        w.varint(action - 2);
        w.raw(r.remaining());
        Ok(Some(w.into_inner()))
    }

    // ---- attributes ------------------------------------------------------------------------

    /// UPDATE_ATTRIBUTES: remap ids, drop attributes the old client lacks.
    pub fn update_attributes(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        copy_varint(&mut r, &mut w)?;
        let n = r.varint()?;
        let mut body = Writer::with_capacity(payload.len());
        let mut kept = 0;
        for _ in 0..n.max(0) {
            let attr = r.varint()?;
            let mut e = Writer::new();
            e.varint(items.attribute(attr));
            copy_bytes(&mut r, &mut e, 8)?; // base
            let m = copy_varint(&mut r, &mut e)?;
            for _ in 0..m.max(0) {
                copy_string(&mut r, &mut e)?;
                copy_bytes(&mut r, &mut e, 9)?; // amount, operation
            }
            if items.attribute_mapped(attr) {
                body.raw(&e.buf);
                kept += 1;
            }
        }
        w.varint(kept);
        w.raw(&body.buf);
        Ok(w.into_inner())
    }

    // ---- containers ------------------------------------------------------------------------

    pub fn container_set_content(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        copy_varint(&mut r, &mut w)?; // container
        copy_varint(&mut r, &mut w)?; // state
        let n = copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            items.item_stack(&mut r, &mut w)?;
        }
        items.item_stack(&mut r, &mut w)?; // carried
        Ok(w.into_inner())
    }

    pub fn container_set_slot(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        copy_varint(&mut r, &mut w)?;
        copy_varint(&mut r, &mut w)?;
        copy_bytes(&mut r, &mut w, 2)?;
        items.item_stack(&mut r, &mut w)?;
        Ok(w.into_inner())
    }

    /// Packets that are some leading varints followed by one item stack.
    pub fn single_item(&self, items: &Items, payload: &[u8], leading_varints: usize) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        for _ in 0..leading_varints {
            copy_varint(&mut r, &mut w)?;
        }
        items.item_stack(&mut r, &mut w)?;
        w.raw(r.remaining());
        Ok(w.into_inner())
    }

    pub fn merchant_offers(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        copy_varint(&mut r, &mut w)?;
        let n = copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            items.item_cost(&mut r, &mut w)?;
            items.item_stack(&mut r, &mut w)?;
            items.opt_item_cost(&mut r, &mut w)?;
            copy_bytes(&mut r, &mut w, 1 + 4 * 6)?; // disabled, uses, max uses, xp, special price, multiplier, demand
        }
        w.raw(r.remaining());
        Ok(w.into_inner())
    }

    /// Serverbound SET_CREATIVE_MODE_SLOT: slot + length-prefixed stack.
    pub fn set_creative_slot(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len() + 8);
        copy_bytes(&mut r, &mut w, 2)?;
        items.length_prefixed_item_stack(&mut r, &mut w)?;
        Ok(w.into_inner())
    }

    /// Serverbound CONTAINER_CLICK with hashed stacks.
    pub fn container_click(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        copy_varint(&mut r, &mut w)?; // container
        copy_varint(&mut r, &mut w)?; // state
        copy_bytes(&mut r, &mut w, 3)?; // slot, button
        copy_varint(&mut r, &mut w)?; // mode
        let n = copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            copy_bytes(&mut r, &mut w, 2)?;
            items.hashed_stack(&mut r, &mut w)?;
        }
        items.hashed_stack(&mut r, &mut w)?;
        Ok(w.into_inner())
    }

    // ---- commands --------------------------------------------------------------------------

    /// COMMANDS: remap parser ids; `hex_color` and `dialog` become single-word strings.
    pub fn commands(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let n = copy_varint(&mut r, &mut w)?;
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
                copy_varint(&mut r, &mut w)?;
            }
            let node_type = flags & 0x03;
            if node_type == 1 || node_type == 2 {
                copy_string(&mut r, &mut w)?;
            }
            if node_type == 2 {
                let parser = r.varint()?;
                let name = d::NEW_ARGUMENT_TYPE_NAMES.get(parser as usize).copied().unwrap_or("");
                match name {
                    "hex_color" | "dialog" => {
                        w.varint(string_parser);
                        w.varint(0); // single word
                    }
                    "resource" | "resource_key" | "resource_or_tag" | "resource_or_tag_key" | "resource_selector" => {
                        let registry = r.string()?;
                        let key = registry.strip_prefix("minecraft:").unwrap_or(registry);
                        if super::world::REMOVED_REGISTRIES.contains(&key) {
                            w.varint(string_parser);
                            w.varint(1); // quotable phrase
                        } else {
                            w.varint(d::ARGUMENT_TYPES.get(parser as usize).map_or(string_parser, |m| *m as i32));
                            w.string(registry);
                        }
                    }
                    _ => {
                        w.varint(d::ARGUMENT_TYPES.get(parser as usize).map_or(string_parser, |m| *m as i32));
                        Self::parser_properties(name, &mut r, &mut w)?;
                    }
                }
            }
            if flags & 0x10 != 0 {
                copy_string(&mut r, &mut w)?;
            }
        }
        copy_varint(&mut r, &mut w)?; // root index
        Ok(w.into_inner())
    }

    fn parser_properties(name: &str, r: &mut Reader, w: &mut Writer) -> Result<()> {
        match name {
            "brigadier:float" | "brigadier:integer" | "brigadier:double" | "brigadier:long" => {
                let size = if name == "brigadier:float" || name == "brigadier:integer" { 4 } else { 8 };
                let flags = r.u8()?;
                w.u8(flags);
                if flags & 1 != 0 {
                    copy_bytes(r, w, size)?;
                }
                if flags & 2 != 0 {
                    copy_bytes(r, w, size)?;
                }
            }
            "brigadier:string" => {
                copy_varint(r, w)?;
            }
            "entity" | "score_holder" => {
                w.u8(r.u8()?);
            }
            "time" => copy_bytes(r, w, 4)?,
            _ => {}
        }
        Ok(())
    }

    // ---- recipes ---------------------------------------------------------------------------

    fn slot_display(&self, items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let ty = copy_varint(r, w)?;
        match d::NEW_SLOT_DISPLAY_NAMES.get(ty as usize).copied().unwrap_or("") {
            "empty" | "any_fuel" => {}
            "item" => {
                let id = r.varint()?;
                w.varint(items.item(id));
            }
            "item_stack" => items.item_stack(r, w)?,
            "tag" => copy_string(r, w)?,
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
                let n = copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    self.slot_display(items, r, w)?;
                }
            }
            other => return Err(err(&format!("unknown slot display {other}"))),
        }
        Ok(())
    }

    fn recipe_display(&self, items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let ty = copy_varint(r, w)?;
        let slots = match ty {
            0 | 1 => {
                if ty == 1 {
                    copy_varint(r, w)?; // width
                    copy_varint(r, w)?; // height
                }
                let n = copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    self.slot_display(items, r, w)?;
                }
                2 // result, station
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
            copy_varint(r, w)?; // duration
            copy_bytes(r, w, 4)?; // experience
        }
        Ok(())
    }

    pub fn recipe_book_add(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let n = copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            copy_varint(&mut r, &mut w)?; // display id
            self.recipe_display(items, &mut r, &mut w)?;
            copy_varint(&mut r, &mut w)?; // optional group
            copy_varint(&mut r, &mut w)?; // category
            if copy_bool(&mut r, &mut w)? {
                let m = copy_varint(&mut r, &mut w)?;
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
        copy_varint(&mut r, &mut w)?;
        self.recipe_display(items, &mut r, &mut w)?;
        Ok(w.into_inner())
    }

    pub fn update_recipes(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let n = copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            copy_string(&mut r, &mut w)?;
            let ids = r.list(|r| r.varint())?;
            w.varint(ids.len() as i32);
            for id in ids {
                w.varint(items.item(id));
            }
        }
        let n = copy_varint(&mut r, &mut w)?;
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
        copy_bool(&mut r, &mut w)?; // reset
        let n = copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            copy_string(&mut r, &mut w)?; // id
            copy_opt_string(&mut r, &mut w)?; // parent
            if copy_bool(&mut r, &mut w)? {
                copy_nbt(&mut r, &mut w)?; // title
                copy_nbt(&mut r, &mut w)?; // description
                items.item_stack(&mut r, &mut w)?; // icon
                copy_varint(&mut r, &mut w)?; // frame
                let flags = r.i32()?;
                w.i32(flags);
                if flags & 1 != 0 {
                    copy_string(&mut r, &mut w)?; // background
                }
                copy_bytes(&mut r, &mut w, 8)?; // x, y
            }
            let reqs = copy_varint(&mut r, &mut w)?;
            for _ in 0..reqs.max(0) {
                let m = copy_varint(&mut r, &mut w)?;
                for _ in 0..m.max(0) {
                    copy_string(&mut r, &mut w)?;
                }
            }
            copy_bool(&mut r, &mut w)?; // telemetry
        }
        w.raw(r.remaining()); // removed, progress
        Ok(w.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::super::items::Dir;
    use super::*;

    #[test]
    fn difficulty_both_ways() {
        let m = Misc::new();
        assert_eq!(m.change_difficulty(&[2, 1]).unwrap(), vec![2, 1]);
        assert_eq!(m.change_difficulty_serverbound(&[3]).unwrap(), vec![3]);
    }

    #[test]
    fn ui_sound_source_becomes_master() {
        let items = Items::new(Dir::ToClient);
        let mut p = Writer::new();
        p.varint(1); // sound id 0 (mapped)
        p.varint(UI_SOURCE);
        p.raw(&[9; 8]);
        let out = Misc::new().sound(&items, &p.buf).unwrap().unwrap();
        let mut r = Reader::new(&out);
        assert_eq!(r.varint().unwrap(), items.sound(0) + 1);
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.remaining(), &[9; 8]);
    }

    #[test]
    fn new_sounds_are_dropped() {
        let items = Items::new(Dir::ToClient);
        let id = d::NEW_SOUND_NAMES.iter().position(|n| *n == "entity.happy_ghast.riding").unwrap() as i32;
        let mut p = Writer::new();
        p.varint(id + 1);
        p.varint(0);
        assert!(Misc::new().sound(&items, &p.buf).unwrap().is_none());
    }

    #[test]
    fn shift_commands_are_dropped() {
        let m = Misc::new();
        assert!(m.player_command(&[7, 0, 0]).unwrap().is_none());
        assert!(m.player_command(&[7, 1, 0]).unwrap().is_none());
        assert_eq!(m.player_command(&[7, 3, 0]).unwrap(), Some(vec![7, 1, 0]));
    }

    #[test]
    fn hex_color_argument_becomes_word() {
        let hex = d::NEW_ARGUMENT_TYPE_NAMES.iter().position(|n| *n == "hex_color").unwrap() as i32;
        let mut p = Writer::new();
        p.varint(2);
        p.u8(0); // root
        p.varint(1);
        p.varint(1);
        p.u8(2); // argument
        p.varint(0);
        p.string("color");
        p.varint(hex);
        p.varint(0);
        let out = Misc::new().commands(&p.buf).unwrap();
        let mut r = Reader::new(&out);
        r.bytes(4).unwrap();
        assert_eq!(r.u8().unwrap(), 2);
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.string().unwrap(), "color");
        assert_eq!(r.varint().unwrap(), 5);
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), 0);
        assert!(r.is_empty());
    }

    #[test]
    fn new_attributes_are_dropped() {
        let items = Items::new(Dir::ToClient);
        let camera = d::NEW_ATTRIBUTE_NAMES.iter().position(|n| *n == "camera_distance").unwrap() as i32;
        let scale = d::NEW_ATTRIBUTE_NAMES.iter().position(|n| *n == "scale").unwrap() as i32;
        let mut p = Writer::new();
        p.varint(1);
        p.varint(2);
        for a in [camera, scale] {
            p.varint(a);
            p.f64(4.0);
            p.varint(0);
        }
        let out = Misc::new().update_attributes(&items, &p.buf).unwrap();
        let mut r = Reader::new(&out);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), items.attribute(scale));
    }
}
