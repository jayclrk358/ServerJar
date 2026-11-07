//! Item stacks and data components, 1.21.11 ↔ 1.21.9.
//!
//! Both versions share every component codec except the ones 1.21.11 added (dropped toward the
//! client) and two whose encoding changed: `consumable` gained an animation id (spear) and block
//! predicates tag each data component predicate with an "is predicate type" flag. Wire formats
//! follow the vanilla data component definitions for these versions.

use std::cell::Cell;

use super::super::data_1_21_9 as d;
use crate::proto::buf::{DecodeError, Reader, Result, Writer};
use crate::proto::nbt;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    /// 1.21.11 server → 1.21.9 client
    ToClient,
    /// 1.21.9 client → 1.21.11 server
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

/// Consumable animation ids: 1.21.11 has "trident" at 5 and "spear" at 11, 1.21.9 calls the
/// trident animation "spear" (5) and has nothing after "bundle" (10).
const ANIM_TRIDENT: i32 = 5;
const ANIM_SPEAR_NEW: i32 = 11;

pub struct Items {
    pub dir: Dir,
    /// Set while a nested parse wants its component count adjusted (unused here, kept for parity).
    extra_added: Cell<i32>,
}

impl Items {
    pub fn new(dir: Dir) -> Self {
        Self { dir, extra_added: Cell::new(0) }
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
    /// Component type id in the target version, or `None` if the target lacks it.
    pub fn component_type(&self, id: i32) -> Option<i32> {
        let t: &[u32] = if self.dir == Dir::ToClient { &d::COMPONENTS } else { &d::COMPONENTS_REVERSE };
        match t.get(usize::try_from(id).ok()?) {
            Some(&v) if v != d::COMPONENT_DROP => Some(v as i32),
            _ => None,
        }
    }
    fn source_component_name(&self, id: i32) -> Option<&'static str> {
        let names: &[&str] = if self.dir == Dir::ToClient { &d::NEW_COMPONENT_NAMES } else { &d::OLD_COMPONENT_NAMES };
        names.get(usize::try_from(id).ok()?).copied()
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
    /// Copies one network NBT value.
    pub fn copy_nbt(r: &mut Reader, w: &mut Writer) -> Result<()> {
        let start = r.position();
        nbt::skip(r)?;
        let end = r.position();
        r.set_position(start);
        w.raw(r.bytes(end - start)?);
        Ok(())
    }
    /// Bool-prefixed optional NBT.
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
    /// `Either<varint, string>` (registry id or key).
    fn either_id(r: &mut Reader, w: &mut Writer) -> Result<()> {
        if Self::copy_bool(r, w)? {
            Self::copy_varint(r, w)?;
        } else {
            Self::copy_string(r, w)?;
        }
        Ok(())
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
    /// `EitherHolder<T>`: bool, then a holder or a resource key.
    fn either_holder(&self, r: &mut Reader, w: &mut Writer, direct: impl FnOnce(&Self, &mut Reader, &mut Writer) -> Result<()>) -> Result<()> {
        if Self::copy_bool(r, w)? {
            self.holder(r, w, Reg::Same, direct)
        } else {
            Self::copy_string(r, w)
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
    pub fn sound_event(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        self.holder(r, w, Reg::Sound, |_, r, w| {
            Self::copy_string(r, w)?;
            if Self::copy_bool(r, w)? {
                Self::copy_bytes(r, w, 4)?;
            }
            Ok(())
        })
    }
    pub fn opt_sound_event(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        if Self::copy_bool(r, w)? {
            self.sound_event(r, w)?;
        }
        Ok(())
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
    pub fn item_stack_list(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let n = Self::copy_varint(r, w)?;
        for _ in 0..n.max(0) {
            self.item_stack(r, w)?;
        }
        Ok(())
    }
    /// Item cost (merchant offers): id, count, then exact components (varint n + components).
    pub fn item_cost(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let id = r.varint()?;
        w.varint(self.item(id));
        Self::copy_varint(r, w)?;
        self.component_list(r, w)
    }
    pub fn opt_item_cost(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        if Self::copy_bool(r, w)? {
            self.item_cost(r, w)?;
        }
        Ok(())
    }
    /// varint n + n full components, keeping only those the target knows.
    fn component_list(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let n = r.varint()?;
        let mut body = Writer::new();
        let mut kept = 0;
        for _ in 0..n.max(0) {
            if self.component(r, &mut body)? {
                kept += 1;
            }
        }
        kept += self.extra_added.replace(0);
        w.varint(kept);
        w.raw(&body.buf);
        Ok(())
    }
    /// Hashed stack (serverbound container clicks): remap the item and component type ids.
    pub fn hashed_stack(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        if !Self::copy_bool(r, w)? {
            return Ok(());
        }
        let id = r.varint()?;
        w.varint(self.item(id));
        Self::copy_varint(r, w)?; // count
        let added = r.varint()?;
        let mut body = Writer::new();
        let mut kept = 0;
        for _ in 0..added.max(0) {
            let ty = r.varint()?;
            let hash = r.i32()?;
            if let Some(t) = self.component_type(ty) {
                body.varint(t);
                body.i32(hash);
                kept += 1;
            }
        }
        w.varint(kept);
        w.raw(&body.buf);
        let removed = r.varint()?;
        let mut body = Writer::new();
        let mut kept = 0;
        for _ in 0..removed.max(0) {
            if let Some(t) = self.component_type(r.varint()?) {
                body.varint(t);
                kept += 1;
            }
        }
        w.varint(kept);
        w.raw(&body.buf);
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
        kept_added += self.extra_added.replace(0);
        let mut removed_body = Writer::new();
        let mut kept_removed = 0;
        for _ in 0..removed {
            if let Some(t) = self.component_type(r.varint()?) {
                removed_body.varint(t);
                kept_removed += 1;
            }
        }
        w.varint(kept_added);
        w.varint(kept_removed);
        w.raw(&body.buf);
        w.raw(&removed_body.buf);
        Ok(())
    }

    /// Reads one component (type id + value) and writes the converted one. Returns whether it was
    /// kept.
    pub fn component(&self, r: &mut Reader, w: &mut Writer) -> Result<bool> {
        let ty = r.varint()?;
        let name = self.source_component_name(ty).ok_or_else(|| err(&format!("unknown data component type {ty}")))?;
        let mut value = Writer::new();
        self.component_value(name, r, &mut value)?;
        match self.component_type(ty) {
            Some(target) => {
                w.varint(target);
                w.raw(&value.buf);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Parses one component payload named `name`, writing the target form.
    fn component_value(&self, name: &str, r: &mut Reader, w: &mut Writer) -> Result<()> {
        macro_rules! varint { () => {{ Self::copy_varint(r, w)?; }}; }
        macro_rules! f32 { () => {{ Self::copy_bytes(r, w, 4)?; }}; }
        macro_rules! bool { () => {{ Self::copy_bool(r, w)?; }}; }
        macro_rules! string { () => {{ Self::copy_string(r, w)?; }}; }
        macro_rules! nbt { () => {{ Self::copy_nbt(r, w)?; }}; }
        match name {
            "max_stack_size" | "max_damage" | "damage" | "repair_cost" | "map_id" | "map_post_processing" | "ominous_bottle_amplifier"
            | "enchantable" | "rarity" | "base_color" | "villager/variant" | "wolf/variant" | "wolf/sound_variant" | "wolf/collar"
            | "fox/variant" | "salmon/size" | "parrot/variant" | "tropical_fish/pattern" | "tropical_fish/base_color"
            | "tropical_fish/pattern_color" | "mooshroom/variant" | "rabbit/variant" | "pig/variant" | "cow/variant"
            | "frog/variant" | "horse/variant" | "llama/variant" | "axolotl/variant" | "cat/variant" | "cat/collar"
            | "sheep/color" | "shulker/color" => varint!(),
            "chicken/variant" | "zombie_nautilus/variant" | "damage_type" => Self::either_id(r, w)?,
            "potion_duration_scale" | "minimum_attack_charge" => f32!(),
            "unbreakable" | "glider" | "creative_slot_lock" => {}
            "enchantment_glint_override" => bool!(),
            "custom_data" | "custom_name" | "item_name" | "map_decorations" | "debug_stick_state" | "bucket_entity_data" | "recipes"
            | "container_loot" | "intangible_projectile" | "lock" => nbt!(),
            "lore" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    nbt!();
                }
            }
            "item_model" | "tooltip_style" | "note_block_sound" | "damage_resistant" | "provides_banner_patterns" => string!(),
            "use_effects" => {
                bool!();
                bool!();
                f32!();
            }
            "enchantments" | "stored_enchantments" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    varint!();
                    varint!();
                }
            }
            "can_place_on" | "can_break" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    self.block_predicate(r, w)?;
                }
            }
            "attribute_modifiers" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    varint!(); // attribute (same registry in both versions)
                    string!(); // id
                    Self::copy_bytes(r, w, 8)?; // amount
                    varint!(); // operation
                    varint!(); // slot
                    if Self::copy_varint(r, w)? == 2 {
                        nbt!(); // override text
                    }
                }
            }
            "custom_model_data" => {
                let n = Self::copy_varint(r, w)?;
                Self::copy_bytes(r, w, n.max(0) as usize * 4)?;
                let n = Self::copy_varint(r, w)?;
                Self::copy_bytes(r, w, n.max(0) as usize)?;
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    string!();
                }
                let n = Self::copy_varint(r, w)?;
                Self::copy_bytes(r, w, n.max(0) as usize * 4)?;
            }
            "tooltip_display" => {
                bool!();
                let n = r.varint()?;
                let mut ids = Vec::new();
                for _ in 0..n.max(0) {
                    if let Some(t) = self.component_type(r.varint()?) {
                        ids.push(t);
                    }
                }
                w.varint(ids.len() as i32);
                for id in ids {
                    w.varint(id);
                }
            }
            "food" => {
                varint!();
                f32!();
                bool!();
            }
            "consumable" => {
                f32!();
                let anim = r.varint()?;
                w.varint(if self.dir == Dir::ToClient && anim == ANIM_SPEAR_NEW { ANIM_TRIDENT } else { anim });
                self.sound_event(r, w)?;
                bool!();
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
                bool!();
            }
            "weapon" => {
                varint!();
                f32!();
            }
            "attack_range" => Self::copy_bytes(r, w, 24)?,
            "equippable" => {
                varint!(); // slot
                self.sound_event(r, w)?;
                Self::copy_opt_string(r, w)?; // asset
                Self::copy_opt_string(r, w)?; // camera overlay
                self.opt_holder_set(r, w, Reg::Entity)?;
                for _ in 0..5 {
                    bool!(); // dispensable, swappable, damage on hurt, equip on interact, can be sheared
                }
                self.sound_event(r, w)?; // shearing sound
            }
            "repairable" => self.holder_set(r, w, Reg::Item)?,
            "blocks_attacks" => {
                f32!();
                f32!();
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    f32!();
                    self.opt_holder_set(r, w, Reg::Same)?;
                    f32!();
                    f32!();
                }
                Self::copy_bytes(r, w, 12)?; // item damage function
                Self::copy_opt_string(r, w)?; // bypassed by (tag key)
                self.opt_sound_event(r, w)?;
                self.opt_sound_event(r, w)?;
            }
            "piercing_weapon" => {
                bool!();
                bool!();
                self.opt_sound_event(r, w)?;
                self.opt_sound_event(r, w)?;
            }
            "kinetic_weapon" => {
                varint!();
                varint!();
                for _ in 0..3 {
                    if Self::copy_bool(r, w)? {
                        varint!();
                        f32!();
                        f32!();
                    }
                }
                f32!();
                f32!();
                self.opt_sound_event(r, w)?;
                self.opt_sound_event(r, w)?;
            }
            "swing_animation" => {
                varint!();
                varint!();
            }
            "dyed_color" | "map_color" => Self::copy_bytes(r, w, 4)?,
            "charged_projectiles" | "bundle_contents" | "container" => self.item_stack_list(r, w)?,
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
                    if Self::copy_bool(r, w)? {
                        nbt!();
                    }
                }
                bool!();
            }
            "trim" => {
                self.holder(r, w, Reg::Same, |s, r, w| s.trim_material_direct(r, w))?;
                self.holder(r, w, Reg::Same, |_, r, w| {
                    Self::copy_string(r, w)?; // asset
                    Self::copy_nbt(r, w)?; // description
                    Self::copy_bool(r, w)?; // decal
                    Ok(())
                })?;
            }
            "provides_trim_material" => self.either_holder(r, w, |s, r, w| s.trim_material_direct(r, w))?,
            "entity_data" => {
                let ty = r.varint()?;
                w.varint(self.entity(ty));
                nbt!();
            }
            "block_entity_data" => {
                varint!(); // block entity types are identical
                nbt!();
            }
            "instrument" => self.either_holder(r, w, |s, r, w| {
                s.sound_event(r, w)?;
                Self::copy_bytes(r, w, 8)?; // use duration, range
                Self::copy_nbt(r, w)
            })?,
            "jukebox_playable" => self.either_holder(r, w, |s, r, w| {
                s.sound_event(r, w)?;
                Self::copy_nbt(r, w)?; // description
                Self::copy_bytes(r, w, 4)?; // length
                Self::copy_varint(r, w)?; // comparator output
                Ok(())
            })?,
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
            "profile" => self.resolvable_profile(r, w)?,
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
                    let ty = r.varint()?;
                    w.varint(self.entity(ty));
                    nbt!();
                    varint!();
                    varint!();
                }
            }
            "break_sound" => self.sound_event(r, w)?,
            "painting/variant" => self.holder(r, w, Reg::Same, |_, r, w| {
                Self::copy_varint(r, w)?;
                Self::copy_varint(r, w)?;
                Self::copy_string(r, w)?;
                Self::copy_opt_nbt(r, w)?;
                Self::copy_opt_nbt(r, w)
            })?,
            other => return Err(err(&format!("no codec for data component {other}"))),
        }
        Ok(())
    }

    /// Direct trim material (1.21.5+): asset name, override assets map, description.
    fn trim_material_direct(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        Self::copy_string(r, w)?;
        let n = Self::copy_varint(r, w)?;
        for _ in 0..n.max(0) {
            Self::copy_string(r, w)?;
            Self::copy_string(r, w)?;
        }
        Self::copy_nbt(r, w)
    }

    pub fn resolvable_profile(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        if Self::copy_bool(r, w)? {
            Self::copy_bytes(r, w, 16)?;
            Self::copy_string(r, w)?;
            self.profile_properties(r, w)?;
        } else {
            Self::copy_opt_string(r, w)?;
            if Self::copy_bool(r, w)? {
                Self::copy_bytes(r, w, 16)?;
            }
            self.profile_properties(r, w)?;
        }
        Self::copy_opt_string(r, w)?; // body texture
        Self::copy_opt_string(r, w)?; // cape
        Self::copy_opt_string(r, w)?; // elytra
        if Self::copy_bool(r, w)? {
            Self::copy_bool(r, w)?; // model type
        }
        Ok(())
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
            self.potion_effect_data(r, w)?;
        }
        Ok(())
    }

    fn potion_effect_data(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        Self::copy_varint(r, w)?; // amplifier
        Self::copy_varint(r, w)?; // duration
        Self::copy_bytes(r, w, 3)?; // ambient, particles, icon
        if Self::copy_bool(r, w)? {
            self.potion_effect_data(r, w)?;
        }
        Ok(())
    }

    fn firework_explosion(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        Self::copy_varint(r, w)?;
        for _ in 0..2 {
            let n = Self::copy_varint(r, w)?;
            Self::copy_bytes(r, w, n.max(0) as usize * 4)?;
        }
        Self::copy_bytes(r, w, 2)
    }

    fn profile_properties(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let n = Self::copy_varint(r, w)?;
        for _ in 0..n.max(0) {
            Self::copy_string(r, w)?;
            Self::copy_string(r, w)?;
            Self::copy_opt_string(r, w)?;
        }
        Ok(())
    }

    /// Block predicate (1.21.5+): optional blocks, optional state properties, optional NBT, then
    /// data component matchers (exact components + partial predicates).
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
        Self::copy_opt_nbt(r, w)?;
        self.component_list(r, w)?;
        // Partial predicates. 1.21.11: bool "is predicate type" + id + nbt (false = the id is a
        // data component type); 1.21.9: predicate type id + nbt only.
        let n = r.varint()?;
        let mut body = Writer::new();
        let mut kept = 0;
        for _ in 0..n.max(0) {
            let is_predicate_type = if self.dir == Dir::ToClient { r.bool()? } else { true };
            let id = r.varint()?;
            let start = r.position();
            nbt::skip(r)?;
            let end = r.position();
            r.set_position(start);
            let tag = r.bytes(end - start)?;
            if !is_predicate_type {
                continue; // 1.21.9 cannot express a bare component check here
            }
            if self.dir == Dir::ToServer {
                body.bool(true);
            }
            body.varint(id);
            body.raw(tag);
            kept += 1;
        }
        w.varint(kept);
        w.raw(&body.buf);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comp(names: &[&str], name: &str) -> i32 {
        names.iter().position(|n| *n == name).unwrap() as i32
    }

    #[test]
    fn stack_drops_new_components_and_remaps_ids() {
        let items = Items::new(Dir::ToClient);
        let spear = d::NEW_ITEM_NAMES.iter().position(|n| *n == "iron_spear").unwrap() as i32;
        let mut w = Writer::new();
        w.varint(1);
        w.varint(spear);
        w.varint(3); // added
        w.varint(0); // removed
        w.varint(comp(&d::NEW_COMPONENT_NAMES, "swing_animation"));
        w.varint(1);
        w.varint(6);
        w.varint(comp(&d::NEW_COMPONENT_NAMES, "attack_range"));
        w.raw(&[0u8; 24]);
        w.varint(comp(&d::NEW_COMPONENT_NAMES, "max_damage"));
        w.varint(250);
        let mut out = Writer::new();
        items.item_stack(&mut Reader::new(&w.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        assert_eq!(r.varint().unwrap(), 1);
        let mapped = r.varint().unwrap();
        assert_eq!(d::OLD_ITEM_NAMES[mapped as usize], "iron_sword");
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), comp(&d::OLD_COMPONENT_NAMES, "max_damage"));
        assert_eq!(r.varint().unwrap(), 250);
        assert!(r.is_empty());
    }

    #[test]
    fn consumable_spear_animation_falls_back() {
        let items = Items::new(Dir::ToClient);
        let mut w = Writer::new();
        w.f32(1.6);
        w.varint(ANIM_SPEAR_NEW);
        w.varint(5); // sound holder id 4
        w.bool(true);
        w.varint(0); // no effects
        let mut out = Writer::new();
        items.component_value("consumable", &mut Reader::new(&w.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        r.f32().unwrap();
        assert_eq!(r.varint().unwrap(), ANIM_TRIDENT);
        assert_eq!(r.varint().unwrap(), d::SOUNDS[4] as i32 + 1);
    }

    #[test]
    fn predicate_flags_are_converted() {
        let mut w = Writer::new();
        w.bool(false); // no blocks
        w.bool(false); // no properties
        w.bool(false); // no nbt
        w.varint(0); // no exact components
        w.varint(2);
        w.bool(true);
        w.varint(3);
        w.u8(0); // end tag
        w.bool(false);
        w.varint(9);
        w.u8(0);
        let mut out = Writer::new();
        Items::new(Dir::ToClient).block_predicate(&mut Reader::new(&w.buf), &mut out).unwrap();
        assert_eq!(out.buf, vec![0, 0, 0, 0, 1, 3, 0]);
        let mut back = Writer::new();
        Items::new(Dir::ToServer).block_predicate(&mut Reader::new(&out.buf), &mut back).unwrap();
        assert_eq!(back.buf, vec![0, 0, 0, 0, 1, 1, 3, 0]);
    }
}
