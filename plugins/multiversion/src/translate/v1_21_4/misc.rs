//! Everything else 1.21.5 ↔ 1.21.4: particles, sounds, explosions, containers, merchants,
//! commands, recipes, advancements and chat.

use super::data_1_21_4 as d;
use super::items::{Items, Reg, Registries};
use super::text;
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

    /// One particle (id + data). tinted_leaves (1.21.5) carries a color its 1.21.4 stand-in lacks.
    pub fn particle(&self, items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let id = r.varint()?;
        let name = d::NEW_PARTICLE_NAMES.get(id.max(0) as usize).copied().unwrap_or("");
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
                Items::copy_bytes(r, w, 28)?;
                Items::copy_varint(r, w)?;
            }
            "tinted_leaves" => {
                r.bytes(4)?; // color; its fallback particle has no data
            }
            _ => {}
        }
        Ok(())
    }

    /// LEVEL_PARTICLES: same layout in both versions, particle last.
    pub fn level_particles(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.raw(r.bytes(2 + 24 + 12 + 4 + 4)?);
        self.particle(items, &mut r, &mut w)?;
        Ok(w.into_inner())
    }

    pub fn sound(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        items.sound_event(&mut r, &mut w)?;
        w.raw(r.remaining());
        Ok(w.into_inner())
    }

    /// EXPLODE (1.21.2+): center, optional knockback, particle, sound.
    pub fn explode(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.raw(r.bytes(24)?);
        if Items::copy_bool(&mut r, &mut w)? {
            w.raw(r.bytes(24)?);
        }
        self.particle(items, &mut r, &mut w)?;
        items.sound_event(&mut r, &mut w)?;
        Ok(w.into_inner())
    }

    // ---- containers ------------------------------------------------------------------------

    pub fn container_set_content(&self, items: &Items, regs: &Registries, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.varint(r.varint()?);
        w.varint(r.varint()?);
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            items.item_stack_tracked(&mut r, &mut w, Some(regs))?;
        }
        items.item_stack_tracked(&mut r, &mut w, Some(regs))?;
        Ok(w.into_inner())
    }

    pub fn container_set_slot(&self, items: &Items, regs: &Registries, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.varint(r.varint()?);
        w.varint(r.varint()?);
        w.i16(r.i16()?);
        items.item_stack_tracked(&mut r, &mut w, Some(regs))?;
        Ok(w.into_inner())
    }

    /// SET_CURSOR_ITEM (no prefix) / SET_PLAYER_INVENTORY (varint slot).
    pub fn single_item(&self, items: &Items, regs: &Registries, payload: &[u8], leading_varints: usize) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        for _ in 0..leading_varints {
            w.varint(r.varint()?);
        }
        items.item_stack_tracked(&mut r, &mut w, Some(regs))?;
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
            w.raw(r.bytes(1 + 4 + 4 + 4 + 4 + 4 + 4)?);
        }
        w.raw(r.remaining());
        Ok(w.into_inner())
    }

    /// Serverbound SET_CREATIVE_MODE_SLOT: 1.21.5 wants length-prefixed components.
    pub fn set_creative_slot(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.i16(r.i16()?);
        items.item_stack_length_prefixed(&mut r, &mut w)?;
        Ok(w.into_inner())
    }

    /// Serverbound CONTAINER_CLICK: full stacks → hashed stacks.
    pub fn container_click(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.varint(r.varint()?); // container
        w.varint(r.varint()?); // state
        w.i16(r.i16()?); // slot
        w.i8(r.i8()?); // button
        w.varint(r.varint()?); // mode
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.clamp(0, 128) {
            w.i16(r.i16()?);
            items.hashed_stack(&mut r, &mut w)?;
        }
        items.hashed_stack(&mut r, &mut w)?;
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
                let name = d::NEW_ARGUMENT_TYPE_NAMES.get(parser.max(0) as usize).copied().unwrap_or("");
                let mapped = d::ARGUMENT_TYPES.get(parser.max(0) as usize).map_or(string_parser, |m| *m as i32);
                match name {
                    "resource_selector" => {
                        r.string()?;
                        w.varint(string_parser);
                        w.varint(1); // quotable phrase
                    }
                    "resource" | "resource_key" | "resource_or_tag" | "resource_or_tag_key" => {
                        let registry = r.string()?;
                        w.varint(mapped);
                        // test_instance has no 1.21.4 registry; any existing one keeps the node.
                        w.string(if registry.trim_start_matches("minecraft:") == "test_instance" { "minecraft:item" } else { registry });
                    }
                    _ => {
                        w.varint(mapped);
                        Self::parser_properties(name, &mut r, &mut w)?;
                    }
                }
            }
            if flags & 0x10 != 0 {
                w.string(r.string()?);
            }
        }
        w.varint(r.varint()?);
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
        match d::NEW_SLOT_DISPLAY_NAMES.get(ty.max(0) as usize).copied().unwrap_or("") {
            "empty" | "any_fuel" => {}
            "item" => {
                w.varint(items.item(r.varint()?));
            }
            "item_stack" => items.item_stack(r, w)?,
            "tag" => Items::copy_string(r, w)?,
            "smithing_trim" => {
                self.slot_display(items, r, w)?; // base
                self.slot_display(items, r, w)?; // material
                // 1.21.5: trim pattern holder; 1.21.4: a third slot display.
                items.holder(r, &mut Writer::new(), Reg::Same, |s, r, w| {
                    Items::copy_string(r, w)?;
                    let _ = s;
                    Items::copy_nbt(r, w)?;
                    Items::copy_bool(r, w)?;
                    Ok(())
                })?;
                w.varint(0); // empty
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
            Items::copy_varint(&mut r, &mut w)?;
            self.recipe_display(items, &mut r, &mut w)?;
            Items::copy_varint(&mut r, &mut w)?;
            Items::copy_varint(&mut r, &mut w)?;
            if Items::copy_bool(&mut r, &mut w)? {
                let m = Items::copy_varint(&mut r, &mut w)?;
                for _ in 0..m.max(0) {
                    items.holder_set(&mut r, &mut w, Reg::Item)?;
                }
            }
            w.u8(r.u8()?);
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

    /// UPDATE_ADVANCEMENTS: background is a texture path again, and the trailing
    /// "show advancements" flag is gone.
    pub fn update_advancements(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.bool(r.bool()?);
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            w.string(r.string()?);
            Items::copy_opt_string(&mut r, &mut w)?;
            if Items::copy_bool(&mut r, &mut w)? {
                text::copy_component(&mut r, &mut w)?;
                text::copy_component(&mut r, &mut w)?;
                items.item_stack(&mut r, &mut w)?;
                Items::copy_varint(&mut r, &mut w)?;
                let flags = r.i32()?;
                w.i32(flags);
                if flags & 1 != 0 {
                    let bg = r.string()?;
                    let (ns, path) = bg.split_once(':').unwrap_or(("minecraft", bg));
                    w.string(&format!("{ns}:textures/{path}.png"));
                }
                w.raw(r.bytes(8)?);
            }
            let reqs = Items::copy_varint(&mut r, &mut w)?;
            for _ in 0..reqs.max(0) {
                let m = Items::copy_varint(&mut r, &mut w)?;
                for _ in 0..m.max(0) {
                    w.string(r.string()?);
                }
            }
            w.bool(r.bool()?);
        }
        let removed = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..removed.max(0) {
            w.string(r.string()?);
        }
        let progress = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..progress.max(0) {
            w.string(r.string()?);
            let c = Items::copy_varint(&mut r, &mut w)?;
            for _ in 0..c.max(0) {
                w.string(r.string()?);
                if Items::copy_bool(&mut r, &mut w)? {
                    w.raw(r.bytes(8)?);
                }
            }
        }
        // 1.21.5 "show advancements" flag: dropped.
        Ok(w.into_inner())
    }

    // ---- chat and other text ---------------------------------------------------------------

    /// Packets that start with a text component (system chat, titles, disconnect, ...).
    pub fn leading_components(payload: &[u8], count: usize) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        for _ in 0..count {
            text::copy_component(&mut r, &mut w)?;
        }
        w.raw(r.remaining());
        Ok(w.into_inner())
    }

    /// PLAYER_CHAT: 1.21.5 added a leading global message index.
    pub fn player_chat(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        r.varint()?;
        w.uuid(r.uuid()?);
        w.varint(r.varint()?);
        if Items::copy_bool(&mut r, &mut w)? {
            w.raw(r.bytes(256)?);
        }
        w.string(r.string()?);
        w.raw(r.bytes(16)?);
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            if Items::copy_varint(&mut r, &mut w)? == 0 {
                w.raw(r.bytes(256)?);
            }
        }
        text::copy_opt_component(&mut r, &mut w)?;
        if Items::copy_varint(&mut r, &mut w)? == 2 {
            w.long_array(&r.long_array()?);
        }
        Self::chat_type_and_names(&mut r, &mut w)?;
        Ok(w.into_inner())
    }

    pub fn disguised_chat(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        text::copy_component(&mut r, &mut w)?;
        Self::chat_type_and_names(&mut r, &mut w)?;
        Ok(w.into_inner())
    }

    /// Chat type holder + sender name + optional target name. Names are only rewritten when
    /// the chat type is a registry reference (an inline one is copied untouched).
    fn chat_type_and_names(r: &mut Reader, w: &mut Writer) -> Result<()> {
        let chat_type = Items::copy_varint(r, w)?;
        if chat_type == 0 {
            w.raw(r.remaining());
            r.set_position(r.position() + r.len());
            return Ok(());
        }
        text::copy_component(r, w)?;
        text::copy_opt_component(r, w)?;
        Ok(())
    }

    pub fn open_screen(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.varint(r.varint()?);
        w.varint(r.varint()?);
        text::copy_component(&mut r, &mut w)?;
        Ok(w.into_inner())
    }

    pub fn player_combat_kill(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.varint(r.varint()?);
        text::copy_component(&mut r, &mut w)?;
        Ok(w.into_inner())
    }

    pub fn boss_event(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.uuid(r.uuid()?);
        let action = Items::copy_varint(&mut r, &mut w)?;
        if action == 0 || action == 3 {
            text::copy_component(&mut r, &mut w)?;
        }
        w.raw(r.remaining());
        Ok(w.into_inner())
    }

    pub fn set_objective(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.string(r.string()?);
        let mode = r.i8()?;
        w.i8(mode);
        if mode == 0 || mode == 2 {
            text::copy_component(&mut r, &mut w)?;
        }
        w.raw(r.remaining());
        Ok(w.into_inner())
    }

    /// Serverbound CHAT / CHAT_COMMAND_SIGNED: 1.21.5 appends a checksum byte (0 = none).
    pub fn append_checksum(payload: &[u8]) -> Vec<u8> {
        let mut v = payload.to_vec();
        v.push(0);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translate::v1_21_4::items::Dir;

    #[test]
    fn advancement_background_becomes_texture_path() {
        let mut p = Writer::new();
        p.bool(true);
        p.varint(1);
        p.string("a");
        p.bool(false);
        p.bool(true);
        p.u8(8); // title: string tag
        p.u16(1);
        p.raw(b"t");
        p.u8(8);
        p.u16(1);
        p.raw(b"d");
        p.varint(0); // empty icon
        p.varint(0);
        p.i32(1);
        p.string("minecraft:block/stone");
        p.f32(0.0);
        p.f32(0.0);
        p.varint(0);
        p.bool(false);
        p.varint(0);
        p.varint(0);
        p.bool(true); // show advancements
        let out = Misc::new().update_advancements(&Items::new(Dir::ToClient), &p.buf).unwrap();
        assert!(String::from_utf8_lossy(&out).contains("minecraft:textures/block/stone.png"));
        assert_ne!(out.last(), Some(&1)); // trailing flag removed
        assert_eq!(out.len(), p.buf.len() - 1 + "textures/.png".len());
    }

    #[test]
    fn chat_gets_checksum() {
        assert_eq!(Misc::append_checksum(&[1, 2]), vec![1, 2, 0]);
    }
}
