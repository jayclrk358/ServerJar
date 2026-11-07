//! Everything else 1.21.9 ↔ 1.21.7: particles, sounds, explosions, containers, merchants,
//! recipes, advancements, the configuration-phase code of conduct and serverbound packets.

use super::data as d;
use super::items::{Items, Reg};
use crate::proto::buf::{DecodeError, Reader, Result, Writer};
use crate::proto::nbt::{self, Tag};

fn err(msg: &str) -> DecodeError {
    DecodeError::Other(msg.to_string())
}

/// Custom click action id of the dialog that stands in for 1.21.9's code of conduct screen.
pub const ACK_CODE_OF_CONDUCT: &str = "multiversion:ack_code_of_conduct";

#[derive(Default)]
pub struct Misc;

impl Misc {
    pub fn new() -> Self {
        Self
    }

    // ---- particles -------------------------------------------------------------------------

    /// One particle (id + data). 1.21.9 gave dragon_breath a power, flash a colour and the
    /// (instant_)effect particles a colour and power; the old client reads no data for them.
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
            "sculk_charge" => Items::copy_bytes(r, w, 4)?,
            "shriek" => {
                Items::copy_varint(r, w)?;
            }
            "entity_effect" | "tinted_leaves" => Items::copy_bytes(r, w, 4)?,
            "trail" => {
                Items::copy_bytes(r, w, 28)?;
                Items::copy_varint(r, w)?;
            }
            "dragon_breath" | "flash" => {
                r.bytes(4)?;
            }
            "effect" | "instant_effect" => {
                r.bytes(8)?;
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

    /// EXPLODE: 1.21.9 added radius + block count after the center and a list of block
    /// particles at the end.
    pub fn explode(&self, items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        w.raw(r.bytes(24)?); // center
        r.bytes(8)?; // radius, affected block count
        if Items::copy_bool(&mut r, &mut w)? {
            w.raw(r.bytes(24)?); // knockback
        }
        self.particle(items, &mut r, &mut w)?;
        items.sound_event(&mut r, &mut w)?;
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
            w.raw(r.bytes(1 + 4 * 6)?); // disabled, uses, max uses, xp, special price, multiplier, demand
        }
        w.raw(r.remaining());
        Ok(w.into_inner())
    }

    /// Serverbound SET_CREATIVE_MODE_SLOT: i16 slot + length-prefixed stack.
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

    /// Serverbound DEBUG_SUBSCRIPTION_REQUEST: 1.21.7 asks for one sample type, 1.21.9 takes a
    /// list of subscriptions. Only the tick time sample (0 → subscription 0) has an equivalent.
    pub fn debug_subscription_request(&self, payload: &[u8]) -> Result<Option<Vec<u8>>> {
        if Reader::new(payload).varint()? != 0 {
            return Ok(None);
        }
        let mut w = Writer::new();
        w.varint(1);
        w.varint(0);
        Ok(Some(w.into_inner()))
    }

    // ---- recipes ---------------------------------------------------------------------------

    fn slot_display(&self, items: &Items, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let ty = Items::copy_varint(r, w)?;
        let name = d::NEW_SLOT_DISPLAY_NAMES.get(ty.max(0) as usize).copied().unwrap_or("");
        match name {
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
                2 // result, station
            }
            1 => {
                Items::copy_varint(r, w)?; // width
                Items::copy_varint(r, w)?; // height
                let n = Items::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    self.slot_display(items, r, w)?;
                }
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

    // ---- configuration ---------------------------------------------------------------------

    /// CODE_OF_CONDUCT (1.21.9 configuration) → SHOW_DIALOG with a notice the player must
    /// acknowledge; its custom click action is turned back into ACCEPT_CODE_OF_CONDUCT.
    pub fn code_of_conduct_dialog(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let text = Reader::new(payload).string()?.to_string();
        let text_tag = |s: &str| {
            let mut t = Tag::compound();
            t.insert("text", Tag::String(s.to_string()));
            t
        };
        let mut body = Tag::compound();
        body.insert("type", Tag::String("minecraft:plain_message".into()));
        body.insert("contents", text_tag(&text));
        let mut click = Tag::compound();
        click.insert("type", Tag::String("minecraft:custom".into()));
        click.insert("id", Tag::String(ACK_CODE_OF_CONDUCT.into()));
        let mut label = Tag::compound();
        label.insert("translate", Tag::String("gui.acknowledge".into()));
        let mut action = Tag::compound();
        action.insert("label", label);
        action.insert("action", click);
        let mut dialog = Tag::compound();
        dialog.insert("type", Tag::String("minecraft:notice".into()));
        dialog.insert("title", text_tag("Code of Conduct"));
        dialog.insert("body", body);
        dialog.insert("can_close_with_escape", Tag::Byte(0));
        dialog.insert("action", action);
        let mut w = Writer::new();
        nbt::write(&mut w, &dialog);
        Ok(w.into_inner())
    }

    /// Serverbound configuration CUSTOM_CLICK_ACTION: true when it acknowledges the code of
    /// conduct dialog created by `code_of_conduct_dialog`.
    pub fn is_code_of_conduct_ack(&self, payload: &[u8]) -> bool {
        Reader::new(payload).string().is_ok_and(|id| id == ACK_CODE_OF_CONDUCT)
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
    fn particle_data_added_in_1_21_9_is_removed() {
        let items = Items::new(Dir::ToClient);
        let m = Misc::new();
        let mut p = Writer::new();
        p.varint(particle_id("effect"));
        p.i32(-1);
        p.f32(1.0);
        p.varint(particle_id("flash"));
        p.i32(-1);
        let mut out = Writer::new();
        let mut r = Reader::new(&p.buf);
        m.particle(&items, &mut r, &mut out).unwrap();
        m.particle(&items, &mut r, &mut out).unwrap();
        assert!(r.is_empty());
        let mut o = Reader::new(&out.buf);
        assert_eq!(o.varint().unwrap(), d::PARTICLES[particle_id("effect") as usize] as i32);
        assert_eq!(o.varint().unwrap(), d::PARTICLES[particle_id("flash") as usize] as i32);
        assert!(o.is_empty());
    }

    #[test]
    fn explode_drops_radius_and_block_particles() {
        let items = Items::new(Dir::ToClient);
        let m = Misc::new();
        let mut p = Writer::new();
        p.raw(&[0u8; 24]);
        p.f32(4.0);
        p.i32(10);
        p.bool(false);
        p.varint(particle_id("explosion"));
        p.varint(1); // sound registry id 0
        p.varint(1); // one block particle
        p.varint(particle_id("poof"));
        p.f32(1.0);
        p.f32(1.0);
        p.varint(1);
        let out = m.explode(&items, &p.buf).unwrap();
        let mut r = Reader::new(&out);
        r.bytes(24).unwrap();
        assert!(!r.bool().unwrap());
        r.varint().unwrap();
        assert_eq!(r.varint().unwrap(), d::SOUNDS[0] as i32 + 1);
        assert!(r.is_empty());
    }

    #[test]
    fn code_of_conduct_roundtrip() {
        let m = Misc::new();
        let mut p = Writer::new();
        p.string("Be nice");
        let dialog = m.code_of_conduct_dialog(&p.buf).unwrap();
        let tag = nbt::read(&mut Reader::new(&dialog)).unwrap();
        assert_eq!(tag.get("type").and_then(Tag::as_str), Some("minecraft:notice"));
        let mut ack = Writer::new();
        ack.string(ACK_CODE_OF_CONDUCT);
        ack.bool(false);
        assert!(m.is_code_of_conduct_ack(&ack.buf));
    }

    #[test]
    fn debug_subscription_maps_tick_time_only() {
        let m = Misc::new();
        assert_eq!(m.debug_subscription_request(&[0]).unwrap(), Some(vec![1, 0]));
        assert_eq!(m.debug_subscription_request(&[1]).unwrap(), None);
    }
}
