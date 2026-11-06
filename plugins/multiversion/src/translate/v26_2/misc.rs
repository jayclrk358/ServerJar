//! Everything else 26.3 ↔ 26.2: particles, sounds, commands, recipes, advancements, maps, chat,
//! signs, explosions, containers.

use std::cell::Cell;

use super::data_26_2 as d;
use super::items::{Items, Reg};
use crate::proto::buf::{DecodeError, Reader, Result, Writer};

fn err(msg: &str) -> DecodeError {
    DecodeError::Other(msg.to_string())
}

const BREWING_STAND_MENU: i32 = 11;

#[derive(Default)]
pub struct Misc {
    /// Container id of the open brewing stand, if any (its extra data slots must be hidden).
    brewing_stand: Cell<i32>,
}

impl Misc {
    pub fn new() -> Self {
        Self { brewing_stand: Cell::new(-1) }
    }

    // ---- particles -------------------------------------------------------------------------

    /// One particle (id + data). Also converts new 26.3 leaf particles to tinted leaves.
    pub fn particle(&self, items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let id = r.varint()?;
        let name = d::NEW_PARTICLE_NAMES.get(id as usize).copied().unwrap_or("");
        let mapped = items.particle(id);
        w.varint(mapped);
        let mapped_name = d::OLD_PARTICLE_NAMES.get(mapped as usize).copied().unwrap_or("");
        match name {
            "block" | "block_marker" | "falling_dust" | "dust_pillar" | "block_crumble" => {
                w.varint(items.block_state(r.varint()?));
            }
            "item" => items.item_stack(r, w)?,
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
            "sculk_charge" | "dragon_breath" => Items::copy_bytes(r, w, 4)?,
            "shriek" => {
                Items::copy_varint(r, w)?;
            }
            "entity_effect" | "tinted_leaves" | "flash" | "geyser" | "geyser_plume" => Items::copy_bytes(r, w, 4)?,
            "effect" | "instant_effect" | "geyser_base" | "geyser_poof" => Items::copy_bytes(r, w, 8)?,
            "trail" => {
                Items::copy_bytes(r, w, 28)?;
                Items::copy_varint(r, w)?;
            }
            // New leaf particles have no data; their fallbacks may need a color.
            "red_poplar_leaves" | "orange_poplar_leaves" | "yellow_poplar_leaves" => {
                if mapped_name == "tinted_leaves" {
                    w.i32(if name == "orange_poplar_leaves" { 0x00E0_7A1F } else { 0x00F2_C31C });
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// LEVEL_PARTICLES: 26.3 leads with the particle and has three max speeds + randomization.
    pub fn level_particles(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut particle = Writer::new();
        self.particle(items, &mut r, &mut particle)?;
        let flags = r.bytes(2)?.to_vec(); // override limiter, always show
        let pos = r.bytes(24)?.to_vec();
        let offsets = r.bytes(12)?.to_vec();
        let sx = r.f32()?;
        let sy = r.f32()?;
        let sz = r.f32()?;
        let count = r.varint()?;
        let randomization = r.varint()?;
        let single_speed = sx == sy && sy == sz;
        let mut w = Writer::with_capacity(payload.len());
        w.raw(&flags);
        w.raw(&pos);
        if count <= 0 && !single_speed {
            // Count 0: offsets × speed are the velocity; keep that product with speed 1.
            let mut o = Reader::new(&offsets);
            for s in [sx, sy, sz] {
                w.f32(o.f32()? * s);
            }
            w.f32(1.0);
        } else {
            w.raw(&offsets);
            w.f32(if randomization == 0 || count <= 3 { sx } else { sx.max(sy).max(sz) });
        }
        w.i32(count);
        w.raw(&particle.buf);
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
        // 26.3 appended a "play sound" flag; everything in between is unchanged.
        let rest = r.remaining();
        if let Some((_, body)) = rest.split_last() {
            w.raw(body);
        }
        Ok(w.into_inner())
    }

    // ---- containers ------------------------------------------------------------------------

    pub fn open_screen(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let container = r.varint()?;
        let menu = r.varint()?;
        self.brewing_stand.set(if menu == BREWING_STAND_MENU { container } else { -1 });
        Ok(payload.to_vec())
    }

    /// CONTAINER_SET_DATA: brewing stands gained two data slots the old client can't handle.
    pub fn container_set_data(&self, payload: &[u8]) -> Result<Option<Vec<u8>>> {
        let mut r = Reader::new(payload);
        let container = r.varint()?;
        let slot = r.i16()?;
        if slot >= 2 && container == self.brewing_stand.get() {
            return Ok(None);
        }
        Ok(Some(payload.to_vec()))
    }

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

    /// Serverbound SET_CREATIVE_MODE_SLOT: i16 slot + full stack (26.2 → 26.3).
    pub fn set_creative_slot(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.i16(r.i16()?);
        items.item_stack(&mut r, &mut w)?;
        Ok(w.into_inner())
    }

    /// Serverbound CONTAINER_CLICK with hashed stacks (26.2 → 26.3).
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
                let mapped_name = d::OLD_ARGUMENT_TYPE_NAMES.get(mapped as usize).copied().unwrap_or("");
                // Registry arguments for registries the old client lacks become plain strings.
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
                } else if mapped_name == "brigadier:string" && name != "brigadier:string" {
                    // New parser type with no 26.2 equivalent (no properties in 26.3).
                    w.varint(mapped);
                    w.varint(1);
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
        let ty = r.varint()?;
        let name = d::NEW_SLOT_DISPLAY_NAMES.get(ty as usize).copied().unwrap_or("");
        match name {
            "tag" => {
                // 26.3: holder set of items; 26.2: a tag key only.
                let v = r.varint()?;
                if v == 0 {
                    w.varint(ty);
                    w.string(r.string()?);
                } else {
                    let composite = d::NEW_SLOT_DISPLAY_NAMES.iter().position(|n| *n == "composite").unwrap_or(10) as i32;
                    let item = d::NEW_SLOT_DISPLAY_NAMES.iter().position(|n| *n == "item").unwrap_or(4) as i32;
                    w.varint(composite);
                    w.varint(v - 1);
                    for _ in 0..(v - 1) {
                        w.varint(item);
                        w.varint(items.item(r.varint()?));
                    }
                }
                return Ok(());
            }
            _ => {
                w.varint(ty);
            }
        }
        match name {
            "empty" | "any_fuel" => {}
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
            let has_display = Items::copy_bool(&mut r, &mut w)?;
            let mut display = Writer::new();
            if has_display {
                Items::copy_nbt(&mut r, &mut display)?; // title
                Items::copy_nbt(&mut r, &mut display)?; // description
                items.item_template(&mut r, &mut display)?; // icon
                Items::copy_varint(&mut r, &mut display)?; // frame
                let flags = r.i32()?;
                display.i32(flags);
                if flags & 1 != 0 {
                    display.string(r.string()?);
                }
            }
            let mut tail = Writer::new();
            let reqs = Items::copy_varint(&mut r, &mut tail)?;
            for _ in 0..reqs.max(0) {
                let m = Items::copy_varint(&mut r, &mut tail)?;
                for _ in 0..m.max(0) {
                    tail.string(r.string()?);
                }
            }
            tail.bool(r.bool()?); // telemetry
            let x = r.f32()?;
            let y = r.f32()?;
            if has_display {
                display.f32(x);
                display.f32(y);
            }
            w.raw(&display.buf);
            w.raw(&tail.buf);
        }
        w.raw(r.remaining()); // removed, progress
        Ok(w.into_inner())
    }

    // ---- misc ------------------------------------------------------------------------------

    pub fn map_item_data(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.varint(r.varint()?);
        w.raw(r.bytes(2)?); // scale, locked
        if Items::copy_bool(&mut r, &mut w)? {
            let n = Items::copy_varint(&mut r, &mut w)?;
            for _ in 0..n.max(0) {
                let ty = r.varint()?;
                w.varint(if ty > 34 { 4 } else { ty });
                w.raw(r.bytes(3)?);
                Items::copy_opt_nbt(&mut r, &mut w)?;
            }
        }
        w.raw(r.remaining());
        Ok(w.into_inner())
    }

    pub fn open_sign_editor(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        w.i64(r.i64()?);
        w.bool(r.varint()? == 1);
        Ok(w.into_inner())
    }

    /// Serverbound SIGN_UPDATE: 26.2 bool front first → 26.3 varint at the end.
    pub fn sign_update(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        w.i64(r.i64()?);
        let front = r.bool()?;
        for _ in 0..4 {
            w.string(r.string()?);
        }
        w.varint(i32::from(front));
        Ok(w.into_inner())
    }

    pub fn player_chat(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.varint(r.varint()?);
        w.uuid(r.uuid()?);
        w.varint(r.varint()?);
        if Items::copy_bool(&mut r, &mut w)? {
            w.raw(r.bytes(256)?);
        }
        w.string(r.string()?);
        w.raw(r.bytes(16)?); // timestamp, salt
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            let idx = Items::copy_varint(&mut r, &mut w)?;
            if idx == 0 {
                w.raw(r.bytes(256)?);
            }
        }
        Items::copy_opt_nbt(&mut r, &mut w)?; // unsigned content (optional)
        let filter = Items::copy_varint(&mut r, &mut w)?;
        if filter == 2 {
            super::world::World::bitset_to_longs(&mut r, &mut w)?;
        }
        w.raw(r.remaining()); // chat type holder, names
        Ok(w.into_inner())
    }
}
