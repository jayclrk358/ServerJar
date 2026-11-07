//! Item stacks and their data components, 1.21.4 ↔ 1.21.2.
//!
//! Both versions share the same component list (same ids). Only two payloads differ:
//! `custom_model_data` (1.21.4: floats/flags/strings/colors lists, 1.21.2: one varint) and the
//! direct trim material (1.21.2 has an extra `item_model_index` float). Every component is still
//! parsed with its exact wire format so registry ids inside (items, blocks, entities, sounds) can
//! be remapped and the stream stays in sync.

use super::data_1_21_2 as d;
use crate::proto::buf::{DecodeError, Reader, Result, Writer};
use crate::proto::nbt;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    /// 1.21.4 server → 1.21.2 client
    ToClient,
    /// 1.21.2 client → 1.21.4 server
    ToServer,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Reg {
    Item,
    Block,
    Entity,
    Sound,
    /// Synced via registry data or otherwise identical on both sides.
    Same,
}

fn err(msg: &str) -> DecodeError {
    DecodeError::Other(msg.to_string())
}

pub struct Items {
    pub dir: Dir,
}

impl Items {
    pub fn new(dir: Dir) -> Self {
        Self { dir }
    }

    // ---- id remaps -------------------------------------------------------------------------

    fn lookup(table: &[u32], reverse: &[u32], dir: Dir, id: i32) -> i32 {
        let t = if dir == Dir::ToClient { table } else { reverse };
        if id < 0 {
            return id;
        }
        t.get(id as usize).map_or(id, |v| *v as i32)
    }
    pub fn item(&self, id: i32) -> i32 {
        Self::lookup(&d::ITEMS, &d::ITEMS_REVERSE, self.dir, id)
    }
    pub fn block(&self, id: i32) -> i32 {
        Self::lookup(&d::BLOCKS, &d::BLOCKS_REVERSE, self.dir, id)
    }
    pub fn block_state(&self, id: i32) -> i32 {
        Self::lookup(&d::BLOCK_STATES, &d::BLOCK_STATES_REVERSE, self.dir, id)
    }
    pub fn entity(&self, id: i32) -> i32 {
        Self::lookup(&d::ENTITIES, &d::ENTITIES_REVERSE, self.dir, id)
    }
    pub fn sound(&self, id: i32) -> i32 {
        Self::lookup(&d::SOUNDS, &d::SOUNDS_REVERSE, self.dir, id)
    }
    pub fn particle(&self, id: i32) -> i32 {
        Self::lookup(&d::PARTICLES, &d::PARTICLES_REVERSE, self.dir, id)
    }
    fn component_name(id: i32) -> Option<&'static str> {
        // The component registries of 1.21.2 and 1.21.4 are identical.
        d::NEW_COMPONENT_NAMES.get(usize::try_from(id).ok()?).copied()
    }
    fn reg_id(&self, reg: Reg, id: i32) -> i32 {
        match reg {
            Reg::Item => self.item(id),
            Reg::Block => self.block(id),
            Reg::Entity => self.entity(id),
            Reg::Sound => self.sound(id),
            Reg::Same => id,
        }
    }

    // ---- generic pieces --------------------------------------------------------------------

    pub fn copy_varint(r: &mut Reader, w: &mut Writer) -> Result<i32> {
        let v = r.varint()?;
        w.varint(v);
        Ok(v)
    }
    pub fn copy_string(r: &mut Reader, w: &mut Writer) -> Result<()> {
        w.string(r.string()?);
        Ok(())
    }
    pub fn copy_bool(r: &mut Reader, w: &mut Writer) -> Result<bool> {
        let b = r.bool()?;
        w.bool(b);
        Ok(b)
    }
    pub fn copy_bytes(r: &mut Reader, w: &mut Writer, n: usize) -> Result<()> {
        w.raw(r.bytes(n)?);
        Ok(())
    }
    /// Copies one network NBT value (End tag included, for "optional" tags).
    pub fn copy_nbt(r: &mut Reader, w: &mut Writer) -> Result<()> {
        let start = r.position();
        nbt::skip(r)?;
        let end = r.position();
        r.set_position(start);
        w.raw(r.bytes(end - start)?);
        Ok(())
    }
    /// Bool-prefixed optional NBT (vanilla's `Optional<Tag>` codec).
    pub fn copy_opt_nbt(r: &mut Reader, w: &mut Writer) -> Result<()> {
        if Self::copy_bool(r, w)? {
            Self::copy_nbt(r, w)?;
        }
        Ok(())
    }
    pub fn copy_opt_string(r: &mut Reader, w: &mut Writer) -> Result<()> {
        if Self::copy_bool(r, w)? {
            Self::copy_string(r, w)?;
        }
        Ok(())
    }
    fn copy_varint_list(r: &mut Reader, w: &mut Writer, elem_bytes: usize) -> Result<()> {
        let n = Self::copy_varint(r, w)?;
        if n < 0 {
            return Err(err("negative list length"));
        }
        Self::copy_bytes(r, w, n as usize * elem_bytes)
    }

    /// `Holder<T>`: varint id+1, or 0 followed by the inline value.
    pub fn holder(&self, r: &mut Reader, w: &mut Writer, reg: Reg, direct: impl FnOnce(&Self, &mut Reader, &mut Writer) -> Result<()>) -> Result<()> {
        let v = r.varint()?;
        if v == 0 {
            w.varint(0);
            direct(self, r, w)
        } else {
            w.varint(self.reg_id(reg, v - 1) + 1);
            Ok(())
        }
    }
    /// `HolderSet`: varint n+1 then n ids, or 0 then a tag key string.
    pub fn holder_set(&self, r: &mut Reader, w: &mut Writer, reg: Reg) -> Result<()> {
        let v = r.varint()?;
        if v == 0 {
            w.varint(0);
            Self::copy_string(r, w)
        } else {
            w.varint(v);
            for _ in 0..(v - 1) {
                let id = r.varint()?;
                w.varint(self.reg_id(reg, id));
            }
            Ok(())
        }
    }
    pub fn opt_holder_set(&self, r: &mut Reader, w: &mut Writer, reg: Reg) -> Result<()> {
        if Self::copy_bool(r, w)? {
            self.holder_set(r, w, reg)?;
        }
        Ok(())
    }
    /// Sound event holder: registry id, or inline location + optional fixed range.
    pub fn sound_event(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        self.holder(r, w, Reg::Sound, |_, r, w| {
            Self::copy_string(r, w)?;
            if Self::copy_bool(r, w)? {
                Self::copy_bytes(r, w, 4)?;
            }
            Ok(())
        })
    }

    // ---- item stacks -----------------------------------------------------------------------

    /// Full item stack: count, then id + component patch when count > 0.
    pub fn item_stack(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let count = Self::copy_varint(r, w)?;
        if count <= 0 {
            return Ok(());
        }
        let id = r.varint()?;
        w.varint(self.item(id));
        self.components(r, w)
    }

    /// Item cost (merchant offers): id, count, then a list of exact components.
    pub fn item_cost(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let id = r.varint()?;
        w.varint(self.item(id));
        Self::copy_varint(r, w)?;
        let n = r.varint()?;
        let mut body = Writer::new();
        let mut kept = 0;
        for _ in 0..n.max(0) {
            if self.component(r, &mut body)? {
                kept += 1;
            }
        }
        w.varint(kept);
        w.raw(&body.buf);
        Ok(())
    }
    pub fn opt_item_cost(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        if Self::copy_bool(r, w)? {
            self.item_cost(r, w)?;
        }
        Ok(())
    }

    /// Component patch: varint added, varint removed, added components, removed type ids.
    pub fn components(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let added = r.varint()?;
        let removed = r.varint()?;
        if added < 0 || removed < 0 || added + removed > 512 {
            return Err(err("bad component counts"));
        }
        let mut body = Writer::new();
        let mut kept_added = 0;
        for _ in 0..added {
            if self.component(r, &mut body)? {
                kept_added += 1;
            }
        }
        w.varint(kept_added);
        w.varint(removed);
        w.raw(&body.buf);
        for _ in 0..removed {
            Self::copy_varint(r, w)?;
        }
        Ok(())
    }

    /// Reads one component (type id + value) and writes the converted one. Returns whether it
    /// was kept.
    pub fn component(&self, r: &mut Reader, w: &mut Writer) -> Result<bool> {
        let ty = r.varint()?;
        let name = Self::component_name(ty).ok_or_else(|| err(&format!("unknown data component type {ty}")))?;
        let mut value = Writer::new();
        if !self.component_value(name, r, &mut value)? {
            return Ok(false);
        }
        w.varint(ty);
        w.raw(&value.buf);
        Ok(true)
    }

    /// Parses one component payload, writing the target form. Returns false to drop it.
    fn component_value(&self, name: &str, r: &mut Reader, w: &mut Writer) -> Result<bool> {
        macro_rules! varint { () => {{ Self::copy_varint(r, w)?; }}; }
        macro_rules! f32 { () => {{ Self::copy_bytes(r, w, 4)?; }}; }
        macro_rules! bool { () => {{ Self::copy_bool(r, w)?; }}; }
        macro_rules! string { () => {{ Self::copy_string(r, w)?; }}; }
        macro_rules! nbt { () => {{ Self::copy_nbt(r, w)?; }}; }
        match name {
            "max_stack_size" | "max_damage" | "damage" | "rarity" | "repair_cost" | "enchantable" | "map_id"
            | "map_post_processing" | "ominous_bottle_amplifier" | "base_color" => varint!(),
            "unbreakable" | "enchantment_glint_override" => bool!(),
            "hide_additional_tooltip" | "hide_tooltip" | "creative_slot_lock" | "glider" => {}
            "custom_data" | "custom_name" | "item_name" | "intangible_projectile" | "map_decorations" | "debug_stick_state"
            | "entity_data" | "bucket_entity_data" | "block_entity_data" | "recipes" | "lock" | "container_loot" => nbt!(),
            "item_model" | "tooltip_style" | "note_block_sound" | "damage_resistant" => string!(),
            "lore" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    nbt!();
                }
            }
            "enchantments" | "stored_enchantments" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    varint!();
                    varint!();
                }
                bool!(); // show in tooltip
            }
            "can_place_on" | "can_break" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    self.block_predicate(r, w)?;
                }
                bool!();
            }
            "attribute_modifiers" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    varint!(); // attribute (same registry in both versions)
                    string!(); // id
                    Self::copy_bytes(r, w, 8)?; // amount
                    varint!(); // operation
                    varint!(); // slot group
                }
                bool!();
            }
            "custom_model_data" => return self.custom_model_data(r, w),
            "food" => {
                varint!();
                f32!();
                bool!();
            }
            "consumable" => {
                f32!(); // seconds
                varint!(); // animation
                self.sound_event(r, w)?;
                bool!(); // particles
                self.consume_effects(r, w)?;
            }
            "death_protection" => self.consume_effects(r, w)?,
            "use_remainder" => self.item_stack(r, w)?,
            "use_cooldown" => {
                f32!();
                Self::copy_opt_string(r, w)?;
            }
            "tool" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    self.holder_set(r, w, Reg::Block)?;
                    if Self::copy_bool(r, w)? {
                        f32!();
                    }
                    if Self::copy_bool(r, w)? {
                        bool!();
                    }
                }
                f32!();
                varint!();
            }
            "equippable" => {
                varint!(); // slot
                self.sound_event(r, w)?;
                Self::copy_opt_string(r, w)?; // model
                Self::copy_opt_string(r, w)?; // camera overlay
                self.opt_holder_set(r, w, Reg::Entity)?;
                for _ in 0..3 {
                    bool!(); // dispensable, swappable, damage on hurt
                }
            }
            "repairable" => self.holder_set(r, w, Reg::Item)?,
            "dyed_color" => {
                Self::copy_bytes(r, w, 4)?;
                bool!();
            }
            "map_color" => Self::copy_bytes(r, w, 4)?,
            "charged_projectiles" | "bundle_contents" | "container" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    self.item_stack(r, w)?;
                }
            }
            "potion_contents" => {
                if Self::copy_bool(r, w)? {
                    varint!();
                }
                if Self::copy_bool(r, w)? {
                    Self::copy_bytes(r, w, 4)?;
                }
                self.potion_effects(r, w)?;
                Self::copy_opt_string(r, w)?;
            }
            "suspicious_stew_effects" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    varint!();
                    varint!();
                }
            }
            "writable_book_content" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    string!();
                    Self::copy_opt_string(r, w)?;
                }
            }
            "written_book_content" => {
                string!();
                Self::copy_opt_string(r, w)?;
                string!(); // author
                varint!(); // generation
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    nbt!();
                    Self::copy_opt_nbt(r, w)?;
                }
                bool!(); // resolved
            }
            "trim" => {
                self.trim_material(r, w)?;
                self.holder(r, w, Reg::Same, |s, r, w| {
                    Self::copy_string(r, w)?; // asset
                    let item = r.varint()?;
                    w.varint(s.item(item));
                    Self::copy_nbt(r, w)?; // description
                    Self::copy_bool(r, w).map(|_| ()) // decal
                })?;
                bool!(); // show in tooltip
            }
            "instrument" => {
                self.holder(r, w, Reg::Same, |s, r, w| {
                    s.sound_event(r, w)?;
                    Self::copy_bytes(r, w, 8)?; // use duration, range
                    Self::copy_nbt(r, w)
                })?;
            }
            "jukebox_playable" => {
                if Self::copy_bool(r, w)? {
                    self.holder(r, w, Reg::Same, |s, r, w| {
                        s.sound_event(r, w)?;
                        Self::copy_nbt(r, w)?; // description
                        Self::copy_bytes(r, w, 4)?; // length
                        Self::copy_varint(r, w).map(|_| ()) // comparator output
                    })?;
                } else {
                    string!(); // song key
                }
                bool!(); // show in tooltip
            }
            "lodestone_tracker" => {
                if Self::copy_bool(r, w)? {
                    string!();
                    Self::copy_bytes(r, w, 8)?;
                }
                bool!();
            }
            "firework_explosion" => self.firework_explosion(r, w)?,
            "fireworks" => {
                varint!();
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    self.firework_explosion(r, w)?;
                }
            }
            "profile" => {
                Self::copy_opt_string(r, w)?;
                if Self::copy_bool(r, w)? {
                    Self::copy_bytes(r, w, 16)?;
                }
                self.profile_properties(r, w)?;
            }
            "banner_patterns" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    self.holder(r, w, Reg::Same, |_, r, w| {
                        Self::copy_string(r, w)?;
                        Self::copy_string(r, w)
                    })?;
                    varint!();
                }
            }
            "pot_decorations" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    let id = r.varint()?;
                    w.varint(self.item(id));
                }
            }
            "block_state" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    string!();
                    string!();
                }
            }
            "bees" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    nbt!();
                    varint!();
                    varint!();
                }
            }
            other => return Err(err(&format!("no codec for data component {other}"))),
        }
        Ok(true)
    }

    /// 1.21.4 `{floats, flags, strings, colors}` ↔ 1.21.2 single varint. Going down, the first
    /// float is the common replacement; with no floats the component is
    /// dropped. Going up, the varint becomes a single float.
    fn custom_model_data(&self, r: &mut Reader, w: &mut Writer) -> Result<bool> {
        if self.dir == Dir::ToServer {
            let v = r.varint()?;
            w.varint(1);
            w.f32(v as f32);
            w.varint(0);
            w.varint(0);
            w.varint(0);
            return Ok(true);
        }
        let floats: Vec<f32> = (0..r.varint()?.max(0)).map(|_| r.f32()).collect::<Result<_>>()?;
        let flags = r.varint()?.max(0) as usize;
        r.bytes(flags)?;
        for _ in 0..r.varint()?.max(0) {
            r.string()?;
        }
        let colors = r.varint()?.max(0) as usize;
        r.bytes(colors * 4)?;
        match floats.first() {
            Some(f) => {
                w.varint(*f as i32);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Trim material holder. Direct form: asset name, ingredient item, [1.21.2: item model
    /// index], armor material overrides, description.
    fn trim_material(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        self.holder(r, w, Reg::Same, |s, r, w| {
            let asset = r.string()?.to_string();
            let item = r.varint()?;
            if s.dir == Dir::ToClient {
                let shown = if asset == "resin" { "gold" } else { asset.as_str() };
                w.string(shown);
                w.varint(s.item(item));
                w.f32(item_model_index(&asset));
            } else {
                r.f32()?; // item model index: gone in 1.21.4
                w.string(&asset);
                w.varint(s.item(item));
            }
            let n = Self::copy_varint(r, w)?;
            for _ in 0..n.max(0) {
                Self::copy_string(r, w)?;
                Self::copy_string(r, w)?;
            }
            Self::copy_nbt(r, w)
        })
    }

    fn consume_effects(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let n = Self::copy_varint(r, w)?;
        for _ in 0..n.max(0) {
            match Self::copy_varint(r, w)? {
                0 => {
                    self.potion_effects(r, w)?;
                    Self::copy_bytes(r, w, 4)?; // probability
                }
                1 => self.holder_set(r, w, Reg::Same)?,
                2 => {}
                3 => Self::copy_bytes(r, w, 4)?, // teleport diameter
                4 => self.sound_event(r, w)?,
                _ => return Err(err("unknown consume effect")),
            }
        }
        Ok(())
    }

    fn potion_effects(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let n = Self::copy_varint(r, w)?;
        for _ in 0..n.max(0) {
            Self::copy_varint(r, w)?; // effect
            self.potion_effect_data(r, w, 0)?;
        }
        Ok(())
    }

    fn potion_effect_data(&self, r: &mut Reader, w: &mut Writer, depth: u32) -> Result<()> {
        if depth > 64 {
            return Err(err("potion effect nesting too deep"));
        }
        Self::copy_varint(r, w)?; // amplifier
        Self::copy_varint(r, w)?; // duration
        Self::copy_bytes(r, w, 3)?; // ambient, particles, icon
        if Self::copy_bool(r, w)? {
            self.potion_effect_data(r, w, depth + 1)?;
        }
        Ok(())
    }

    fn firework_explosion(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        Self::copy_varint(r, w)?; // shape
        Self::copy_varint_list(r, w, 4)?; // colors
        Self::copy_varint_list(r, w, 4)?; // fade colors
        Self::copy_bytes(r, w, 2) // trail, twinkle
    }

    pub fn profile_properties(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let n = Self::copy_varint(r, w)?;
        for _ in 0..n.max(0) {
            Self::copy_string(r, w)?;
            Self::copy_string(r, w)?;
            Self::copy_opt_string(r, w)?;
        }
        Ok(())
    }

    fn block_predicate(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        self.opt_holder_set(r, w, Reg::Block)?;
        if Self::copy_bool(r, w)? {
            let n = Self::copy_varint(r, w)?;
            for _ in 0..n.max(0) {
                Self::copy_string(r, w)?;
                if Self::copy_bool(r, w)? {
                    Self::copy_string(r, w)?;
                } else {
                    Self::copy_opt_string(r, w)?;
                    Self::copy_opt_string(r, w)?;
                }
            }
        }
        Self::copy_opt_nbt(r, w)
    }
}

/// Trim material `item_model_index` values from vanilla 1.21.2.
pub fn item_model_index(asset: &str) -> f32 {
    match asset.strip_prefix("minecraft:").unwrap_or(asset) {
        "amethyst" => 1.0,
        "copper" => 0.5,
        "diamond" => 0.8,
        "emerald" => 0.7,
        "gold" | "resin" => 0.6,
        "iron" => 0.2,
        "lapis" => 0.9,
        "netherite" => 0.3,
        "quartz" => 0.1,
        "redstone" => 0.4,
        _ => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comp(name: &str) -> i32 {
        d::NEW_COMPONENT_NAMES.iter().position(|n| *n == name).unwrap() as i32
    }

    #[test]
    fn plain_stack_remaps_id() {
        let items = Items::new(Dir::ToClient);
        let last = d::ITEMS.len() as i32 - 1;
        let mut w = Writer::new();
        w.varint(3);
        w.varint(last);
        w.varint(1);
        w.varint(0);
        w.varint(comp("max_stack_size"));
        w.varint(64);
        let mut out = Writer::new();
        items.item_stack(&mut Reader::new(&w.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        assert_eq!(r.varint().unwrap(), 3);
        assert_eq!(r.varint().unwrap(), d::ITEMS[last as usize] as i32);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), comp("max_stack_size"));
        assert_eq!(r.varint().unwrap(), 64);
        assert!(r.is_empty());
    }

    #[test]
    fn custom_model_data_downgrades_to_first_float() {
        let items = Items::new(Dir::ToClient);
        let mut w = Writer::new();
        w.varint(1);
        w.varint(1);
        w.varint(2); // two components
        w.varint(0);
        w.varint(comp("custom_model_data"));
        w.varint(2);
        w.f32(7.0);
        w.f32(9.0);
        w.varint(1);
        w.u8(1);
        w.varint(1);
        w.string("x");
        w.varint(0);
        w.varint(comp("custom_model_data"));
        w.varint(0); // no floats: dropped
        w.varint(0);
        w.varint(0);
        w.varint(0);
        let mut out = Writer::new();
        items.item_stack(&mut Reader::new(&w.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        r.varint().unwrap();
        r.varint().unwrap();
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), comp("custom_model_data"));
        assert_eq!(r.varint().unwrap(), 7);
        assert!(r.is_empty());
    }

    #[test]
    fn custom_model_data_upgrades_to_float_list() {
        let items = Items::new(Dir::ToServer);
        let mut w = Writer::new();
        w.varint(comp("custom_model_data"));
        w.varint(12);
        let mut out = Writer::new();
        assert!(items.component(&mut Reader::new(&w.buf), &mut out).unwrap());
        let mut r = Reader::new(&out.buf);
        assert_eq!(r.varint().unwrap(), comp("custom_model_data"));
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.f32().unwrap(), 12.0);
        assert_eq!((r.varint().unwrap(), r.varint().unwrap(), r.varint().unwrap()), (0, 0, 0));
        assert!(r.is_empty());
    }

    #[test]
    fn direct_trim_material_gains_item_model_index() {
        let items = Items::new(Dir::ToClient);
        let mut w = Writer::new();
        w.varint(comp("trim"));
        w.varint(0); // direct material
        w.string("resin");
        w.varint(5);
        w.varint(0); // no overrides
        nbt::write(&mut w, &nbt::Tag::String("Resin".into()));
        w.varint(3); // pattern by id (2)
        w.bool(true);
        let mut out = Writer::new();
        assert!(items.component(&mut Reader::new(&w.buf), &mut out).unwrap());
        let mut r = Reader::new(&out.buf);
        assert_eq!(r.varint().unwrap(), comp("trim"));
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.string().unwrap(), "gold");
        assert_eq!(r.varint().unwrap(), d::ITEMS[5] as i32);
        assert_eq!(r.f32().unwrap(), 0.6);
        assert_eq!(r.varint().unwrap(), 0);
        nbt::skip(&mut r).unwrap();
        assert_eq!(r.varint().unwrap(), 3);
        assert!(r.bool().unwrap());
        assert!(r.is_empty());

        // and back up again: the float disappears
        let up = Items::new(Dir::ToServer);
        let mut back = Writer::new();
        assert!(up.component(&mut Reader::new(&out.buf), &mut back).unwrap());
        let mut r = Reader::new(&back.buf);
        r.varint().unwrap();
        r.varint().unwrap();
        assert_eq!(r.string().unwrap(), "gold");
        r.varint().unwrap();
        assert_eq!(r.varint().unwrap(), 0); // overrides count directly after the item
    }
}
