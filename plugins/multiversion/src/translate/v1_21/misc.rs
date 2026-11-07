//! Everything else 1.21.2 ↔ 1.21: particles, sounds, explosions, containers, cooldowns,
//! advancements, player info, time, attributes and item hovers in chat.

use std::cell::Cell;

use super::data_1_21 as d;
use super::items::{Items, BACKUP_KEY};
use crate::proto::buf::{DecodeError, Reader, Result, Writer};
use crate::proto::nbt::{self, Tag};

fn err(msg: &str) -> DecodeError {
    DecodeError::Other(msg.to_string())
}

/// The smithing menu id (1.21 and 1.21.2).
const SMITHING_MENU: i32 = 21;

pub struct Misc {
    smithing_open: Cell<bool>,
    /// Last container state id seen (reused for synthesized slot updates).
    pub state_id: Cell<i32>,
}

impl Default for Misc {
    fn default() -> Self {
        Self::new()
    }
}

fn floats_from_rgb(w: &mut Writer, rgb: i32) {
    for shift in [16, 8, 0] {
        w.f32(((rgb >> shift) & 0xFF) as f32 / 255.0);
    }
}

impl Misc {
    pub fn new() -> Self {
        Self { smithing_open: Cell::new(false), state_id: Cell::new(0) }
    }

    // ---- particles -------------------------------------------------------------------------

    /// One particle (id + data), 1.21.2 → 1.21.
    pub fn particle(&self, items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let id = r.varint()?;
        let name = d::NEW_PARTICLE_NAMES.get(id.max(0) as usize).copied().unwrap_or("");
        w.varint(items.particle(id));
        match name {
            "block" | "block_marker" | "falling_dust" | "dust_pillar" | "block_crumble" => {
                w.varint(items.block_state(r.varint()?));
            }
            "item" => items.item_stack(r, w)?,
            "dust" => {
                floats_from_rgb(w, r.i32()?);
                Items::copy_bytes(r, w, 4)?; // scale
            }
            "dust_color_transition" => {
                floats_from_rgb(w, r.i32()?);
                floats_from_rgb(w, r.i32()?);
                Items::copy_bytes(r, w, 4)?;
            }
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
                // becomes entity_effect: only the color is kept
                r.bytes(24)?;
                Items::copy_bytes(r, w, 4)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// LEVEL_PARTICLES: same header in both versions, particle last.
    pub fn level_particles(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.raw(r.bytes(1 + 24 + 12 + 4 + 4)?);
        self.particle(items, &mut r, &mut w)?;
        Ok(w.into_inner())
    }

    // ---- sounds ----------------------------------------------------------------------------

    /// SOUND / SOUND_ENTITY; `None` for sounds the client does not have.
    pub fn sound(&self, items: &Items, payload: &[u8]) -> Result<Option<Vec<u8>>> {
        let mut r = Reader::new(payload);
        let v = r.varint()?;
        if v > 0 && Items::sound_unknown(v - 1) {
            return Ok(None);
        }
        r.set_position(0);
        let mut w = Writer::with_capacity(payload.len());
        items.sound_event(&mut r, &mut w)?;
        w.raw(r.remaining());
        Ok(Some(w.into_inner()))
    }

    /// EXPLODE 1.21.2 (center, optional knockback, one particle, sound) → 1.21 layout.
    pub fn explode(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len() + 32);
        w.raw(r.bytes(24)?); // center
        w.f32(0.0); // power: block changes arrive separately
        w.varint(0); // affected blocks
        let (mut kx, mut ky, mut kz) = (0.0, 0.0, 0.0);
        if r.bool()? {
            kx = r.f64()?;
            ky = r.f64()?;
            kz = r.f64()?;
        }
        w.f32(kx as f32);
        w.f32(ky as f32);
        w.f32(kz as f32);
        w.varint(0); // block interaction: keep
        let mut particle = Writer::new();
        self.particle(items, &mut r, &mut particle)?;
        w.raw(&particle.buf); // small
        w.raw(&particle.buf); // large
        items.sound_event(&mut r, &mut w)?;
        Ok(w.into_inner())
    }

    // ---- containers ------------------------------------------------------------------------

    pub fn open_screen(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        r.varint()?;
        self.smithing_open.set(r.varint()? == SMITHING_MENU);
        Ok(payload.to_vec())
    }

    /// varint container id → unsigned byte (CONTAINER_CLOSE, HORSE_SCREEN_OPEN).
    pub fn container_id_to_byte(&self, payload: &[u8], closes: bool) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.u8(r.varint()? as u8);
        w.raw(r.remaining());
        if closes {
            self.smithing_open.set(false);
        }
        Ok(w.into_inner())
    }

    /// CONTAINER_SET_DATA; smithing tables gained properties the old client cannot handle.
    pub fn container_set_data(&self, payload: &[u8]) -> Result<Option<Vec<u8>>> {
        if self.smithing_open.get() {
            return Ok(None);
        }
        self.container_id_to_byte(payload, false).map(Some)
    }

    pub fn container_set_content(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.u8(r.varint()? as u8);
        let state = Items::copy_varint(&mut r, &mut w)?;
        self.state_id.set(state);
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            items.item_stack(&mut r, &mut w)?;
        }
        items.item_stack(&mut r, &mut w)?; // carried
        Ok(w.into_inner())
    }

    /// CONTAINER_SET_SLOT. Returns the 1.21 packet, plus the player's slot index and item bytes so the
    /// caller can also send armor as equipment (1.21 only expects slot updates for hotbar and offhand).
    pub fn container_set_slot(&self, items: &Items, payload: &[u8]) -> Result<(Vec<u8>, Option<(i16, Vec<u8>)>)> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let container = r.varint()?;
        w.u8(container as u8);
        let state = Items::copy_varint(&mut r, &mut w)?;
        self.state_id.set(state);
        let slot = r.i16()?;
        w.i16(slot);
        let mut item = Writer::new();
        items.item_stack(&mut r, &mut item)?;
        w.raw(&item.buf);
        let armor = (container == 0 && (5..=8).contains(&slot)).then(|| (slot, item.into_inner()));
        Ok((w.into_inner(), armor))
    }

    /// SET_CURSOR_ITEM → CONTAINER_SET_SLOT(-1, last state, -1, item).
    pub fn set_cursor_item(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len() + 4);
        w.i8(-1);
        w.varint(self.state_id.get());
        w.i16(-1);
        items.item_stack(&mut r, &mut w)?;
        Ok(w.into_inner())
    }

    /// SET_PLAYER_INVENTORY → CONTAINER_SET_SLOT(-2, 0, slot, item).
    pub fn set_player_inventory(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len() + 4);
        let slot = r.varint()?;
        w.i8(-2);
        w.varint(0);
        w.i16(slot as i16);
        items.item_stack(&mut r, &mut w)?;
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

    /// COOLDOWN: 1.21.2 names the item (cooldown group); 1.21 wants the item id.
    pub fn cooldown(&self, items: &Items, payload: &[u8]) -> Result<Option<Vec<u8>>> {
        let mut r = Reader::new(payload);
        let name = r.string()?;
        let Some(id) = Items::new_item_by_name(name) else { return Ok(None) };
        let mut w = Writer::new();
        w.varint(items.item(id));
        w.raw(r.remaining());
        Ok(Some(w.into_inner()))
    }

    // ---- serverbound containers ------------------------------------------------------------

    /// CONTAINER_CLICK 1.21 (byte container, full stacks) → 1.21.2 (varint container).
    pub fn container_click(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len() + 2);
        w.varint(i32::from(r.i8()?));
        Items::copy_varint(&mut r, &mut w)?; // state id
        w.raw(r.bytes(3)?); // slot, button
        Items::copy_varint(&mut r, &mut w)?; // mode
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.clamp(0, 128) {
            w.raw(r.bytes(2)?);
            items.item_stack(&mut r, &mut w)?;
        }
        items.item_stack(&mut r, &mut w)?;
        Ok(w.into_inner())
    }

    /// CONTAINER_CLOSE 1.21 (byte) → 1.21.2 (varint).
    pub fn container_close(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        w.varint(i32::from(r.i8()?));
        self.smithing_open.set(false);
        Ok(w.into_inner())
    }

    pub fn set_creative_slot(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.i16(r.i16()?);
        items.item_stack(&mut r, &mut w)?;
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
                items.item_stack(&mut r, &mut w)?; // icon
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
        w.raw(r.remaining());
        Ok(w.into_inner())
    }

    // ---- players ---------------------------------------------------------------------------

    /// PLAYER_INFO_UPDATE: drops the 1.21.2 "list order" action (bit 6).
    pub fn player_info_update(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let actions = r.u8()?;
        w.u8(actions & 0x3F);
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            w.uuid(r.uuid()?);
            if actions & 1 != 0 {
                w.string(r.string()?);
                let p = Items::copy_varint(&mut r, &mut w)?;
                for _ in 0..p.max(0) {
                    w.string(r.string()?);
                    w.string(r.string()?);
                    Items::copy_opt_string(&mut r, &mut w)?;
                }
            }
            if actions & 2 != 0 && Items::copy_bool(&mut r, &mut w)? {
                w.uuid(r.uuid()?);
                w.i64(r.i64()?);
                w.byte_array(r.byte_array()?);
                w.byte_array(r.byte_array()?);
            }
            if actions & 4 != 0 {
                Items::copy_varint(&mut r, &mut w)?;
            }
            if actions & 8 != 0 {
                w.bool(r.bool()?);
            }
            if actions & 16 != 0 {
                Items::copy_varint(&mut r, &mut w)?;
            }
            if actions & 32 != 0 && Items::copy_bool(&mut r, &mut w)? {
                let mut tag = nbt::read(&mut r)?;
                fix_text(&mut tag);
                nbt::write(&mut w, &tag);
            }
            if actions & 64 != 0 {
                r.varint()?; // list order
            }
        }
        Ok(w.into_inner())
    }

    /// SET_TIME: 1.21.2 has an explicit "tick day time" flag; 1.21 uses a negative day time.
    pub fn set_time(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        w.i64(r.i64()?);
        let mut day = r.i64()?;
        if !r.bool()? {
            day = if day == 0 { -1 } else { -day };
        }
        w.i64(day);
        Ok(w.into_inner())
    }

    /// UPDATE_ATTRIBUTES: remaps attribute ids and drops attributes the client lacks.
    pub fn update_attributes(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.varint(r.varint()?);
        let n = r.varint()?;
        let mut body = Writer::new();
        let mut kept = 0;
        for _ in 0..n.max(0) {
            let attr = r.varint()?;
            let mut value = Writer::new();
            value.f64(r.f64()?);
            let m = Items::copy_varint(&mut r, &mut value)?;
            for _ in 0..m.max(0) {
                value.string(r.string()?);
                value.raw(r.bytes(8)?);
                value.u8(r.u8()?);
            }
            if Items::attribute_unknown(attr) {
                continue;
            }
            body.varint(items.attribute(attr));
            body.raw(&value.buf);
            kept += 1;
        }
        w.varint(kept);
        w.raw(&body.buf);
        Ok(w.into_inner())
    }

    /// Packets that start with one text component (system chat, action bar, titles, disguised chat).
    pub fn leading_text(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut tag = nbt::read(&mut r)?;
        fix_text(&mut tag);
        let mut w = Writer::with_capacity(payload.len());
        nbt::write(&mut w, &tag);
        w.raw(r.remaining());
        Ok(w.into_inner())
    }
}

/// Item hovers in text: components the 1.21 client cannot decode are removed, food gets its
/// 1.21 fields back.
pub fn fix_text(tag: &mut Tag) {
    match tag {
        Tag::Compound(entries) => {
            let is_show_item = entries.iter().any(|(k, v)| k == "action" && v.as_str().map(|s| s.trim_start_matches("minecraft:")) == Some("show_item"));
            if is_show_item {
                for (k, v) in entries.iter_mut() {
                    if k == "contents" || k == "value" {
                        if let Some(components) = v.get_mut("components") {
                            fix_hover_components(components);
                        }
                    }
                }
            }
            for (_, v) in entries.iter_mut() {
                fix_text(v);
            }
        }
        Tag::List(list) => list.iter_mut().for_each(fix_text),
        _ => {}
    }
}

fn fix_hover_components(components: &mut Tag) {
    let Tag::Compound(entries) = components else { return };
    let get = |entries: &Vec<(String, Tag)>, name: &str| entries.iter().position(|(k, _)| k.trim_start_matches("minecraft:") == name);
    let remainder = get(entries, "use_remainder").map(|i| entries[i].1.clone());
    if let Some(i) = get(entries, "food") {
        let food = &mut entries[i].1;
        if let Some(rem) = remainder {
            food.insert("using_converts_to", rem);
        }
        food.insert("eat_seconds", Tag::Float(1.6));
    }
    if let Some(i) = get(entries, "instrument") {
        entries[i].1.remove("description");
    }
    entries.retain(|(k, _)| {
        let n = k.trim_start_matches("minecraft:");
        !matches!(
            n,
            "item_model" | "consumable" | "use_remainder" | "use_cooldown" | "damage_resistant" | "enchantable" | "equippable" | "repairable"
                | "glider" | "tooltip_style" | "death_protection" | "lock"
        )
    });
    if let Some(i) = get(entries, "custom_data") {
        entries[i].1.remove(BACKUP_KEY);
    }
}

#[cfg(test)]
mod tests {
    use super::super::items::Dir;
    use super::*;

    #[test]
    fn dust_particle_colors_become_floats() {
        let items = Items::new(Dir::ToClient);
        let misc = Misc::new();
        let dust = d::NEW_PARTICLE_NAMES.iter().position(|n| *n == "dust").unwrap() as i32;
        let mut p = Writer::new();
        p.varint(dust);
        p.i32(0x00FF_0000);
        p.f32(1.5);
        let mut out = Writer::new();
        misc.particle(&items, &mut Reader::new(&p.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        r.varint().unwrap();
        assert_eq!(r.f32().unwrap(), 1.0);
        assert_eq!(r.f32().unwrap(), 0.0);
        assert_eq!(r.f32().unwrap(), 0.0);
        assert_eq!(r.f32().unwrap(), 1.5);
        assert!(r.is_empty());
    }

    #[test]
    fn explosion_layout() {
        let items = Items::new(Dir::ToClient);
        let mut p = Writer::new();
        p.f64(1.0).f64(2.0).f64(3.0);
        p.bool(true).f64(0.5).f64(0.0).f64(0.0);
        p.varint(d::NEW_PARTICLE_NAMES.iter().position(|n| *n == "explosion").unwrap() as i32);
        p.varint(1); // sound id 0
        let out = Misc::new().explode(&items, &p.buf).unwrap();
        let mut r = Reader::new(&out);
        r.bytes(24).unwrap();
        assert_eq!(r.f32().unwrap(), 0.0);
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.f32().unwrap(), 0.5);
        r.bytes(8).unwrap();
        assert_eq!(r.varint().unwrap(), 0);
        r.varint().unwrap();
        r.varint().unwrap();
        assert_eq!(r.varint().unwrap(), 1);
        assert!(r.is_empty());
    }

    #[test]
    fn time_and_player_info() {
        let m = Misc::new();
        let mut p = Writer::new();
        p.i64(5).i64(1000).bool(false);
        let out = m.set_time(&p.buf).unwrap();
        assert_eq!(Reader::new(&out[8..]).i64().unwrap(), -1000);
        let mut p = Writer::new();
        p.u8(64 | 8);
        p.varint(1);
        p.uuid(3);
        p.bool(true);
        p.varint(7);
        let out = m.player_info_update(&p.buf).unwrap();
        let mut r = Reader::new(&out);
        assert_eq!(r.u8().unwrap(), 8);
        r.varint().unwrap();
        r.uuid().unwrap();
        assert!(r.bool().unwrap());
        assert!(r.is_empty());
    }

    #[test]
    fn hover_components_are_cleaned() {
        let mut components = Tag::compound();
        components.insert("minecraft:consumable", Tag::compound());
        components.insert("minecraft:food", Tag::compound());
        let mut contents = Tag::compound();
        contents.insert("id", Tag::String("minecraft:apple".into()));
        contents.insert("components", components);
        let mut hover = Tag::compound();
        hover.insert("action", Tag::String("show_item".into()));
        hover.insert("contents", contents);
        let mut text = Tag::compound();
        text.insert("text", Tag::String("x".into()));
        text.insert("hoverEvent", hover);
        fix_text(&mut text);
        let c = text.get("hoverEvent").unwrap().get("contents").unwrap().get("components").unwrap();
        assert!(c.get("minecraft:consumable").is_none());
        assert_eq!(c.get("minecraft:food").unwrap().get("eat_seconds"), Some(&Tag::Float(1.6)));
    }
}
