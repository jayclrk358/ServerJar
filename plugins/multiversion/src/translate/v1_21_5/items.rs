//! Item stacks and their data components, 1.21.6 ↔ 1.21.5.
//!
//! Both versions have the same 96 component types in the same order. Only two encodings changed:
//! `equippable` gained "can be sheared" + a shearing sound, and every `attribute_modifiers` entry
//! gained a display mode. Everything else is parsed with its exact wire format so the stream stays
//! in sync while registry ids inside are remapped (vanilla wire formats).

use super::super::data_1_21_5 as d;
use crate::proto::buf::{DecodeError, Reader, Result, Writer};
use crate::proto::nbt;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    /// 1.21.6 server → 1.21.5 client
    ToClient,
    /// 1.21.5 client → 1.21.6 server
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

pub fn err(msg: &str) -> DecodeError {
    DecodeError::Other(msg.to_string())
}

// ---- small copy helpers shared by the whole step ------------------------------------------

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
/// Copies one network NBT value (an End tag counts as a value).
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
    if copy_bool(r, w)? {
        copy_nbt(r, w)?;
    }
    Ok(())
}
pub fn copy_opt_string(r: &mut Reader, w: &mut Writer) -> Result<()> {
    if copy_bool(r, w)? {
        copy_string(r, w)?;
    }
    Ok(())
}
fn copy_int_list(r: &mut Reader, w: &mut Writer, elem_size: usize) -> Result<()> {
    let n = copy_varint(r, w)?;
    copy_bytes(r, w, n.max(0) as usize * elem_size)
}

pub struct Items {
    pub dir: Dir,
}

impl Items {
    pub fn new(dir: Dir) -> Self {
        Self { dir }
    }

    // ---- id remaps -------------------------------------------------------------------------

    fn lookup(&self, table: &[u32], reverse: &[u32], id: i32) -> i32 {
        let t = if self.dir == Dir::ToClient { table } else { reverse };
        if id < 0 {
            return id;
        }
        t.get(id as usize).map_or(id, |v| *v as i32)
    }
    pub fn item(&self, id: i32) -> i32 {
        self.lookup(&d::ITEMS, &d::ITEMS_REVERSE, id)
    }
    pub fn block(&self, id: i32) -> i32 {
        self.lookup(&d::BLOCKS, &d::BLOCKS_REVERSE, id)
    }
    pub fn block_state(&self, id: i32) -> i32 {
        self.lookup(&d::BLOCK_STATES, &d::BLOCK_STATES_REVERSE, id)
    }
    pub fn entity(&self, id: i32) -> i32 {
        self.lookup(&d::ENTITIES, &d::ENTITIES_REVERSE, id)
    }
    pub fn sound(&self, id: i32) -> i32 {
        self.lookup(&d::SOUNDS, &d::SOUNDS_REVERSE, id)
    }
    pub fn attribute(&self, id: i32) -> i32 {
        self.lookup(&d::ATTRIBUTES, &d::ATTRIBUTES_REVERSE, id)
    }
    pub fn particle(&self, id: i32) -> i32 {
        self.lookup(&d::PARTICLES, &d::PARTICLES_REVERSE, id)
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

    /// Whether a registry entry exists on the target side (the tables use a fallback otherwise).
    fn mapped(&self, table: &[u32], new_names: &[&str], old_names: &[&str], fallback: usize, id: i32) -> bool {
        if self.dir == Dir::ToServer {
            return true; // everything the old client knows exists in 1.21.6
        }
        let Ok(i) = usize::try_from(id) else { return false };
        match (table.get(i), new_names.get(i)) {
            (Some(&v), Some(name)) => v as usize != fallback || old_names.get(fallback) == Some(name),
            _ => false,
        }
    }
    pub fn sound_mapped(&self, id: i32) -> bool {
        self.mapped(&d::SOUNDS, &d::NEW_SOUND_NAMES, &d::OLD_SOUND_NAMES, 0, id)
    }
    pub fn attribute_mapped(&self, id: i32) -> bool {
        self.mapped(&d::ATTRIBUTES, &d::NEW_ATTRIBUTE_NAMES, &d::OLD_ATTRIBUTE_NAMES, 0, id)
    }
    pub fn block_mapped(&self, id: i32) -> bool {
        self.mapped(&d::BLOCKS, &d::NEW_BLOCK_NAMES, &d::OLD_BLOCK_NAMES, 1, id)
    }
    pub fn item_mapped(&self, id: i32) -> bool {
        self.mapped(&d::ITEMS, &d::NEW_ITEM_NAMES, &d::OLD_ITEM_NAMES, 0, id)
    }

    /// Component type id on the target side (identical lists in both versions).
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

    // ---- registry value shapes -------------------------------------------------------------

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
    /// `EitherHolder<T>`: bool true + holder, or false + registry key string.
    fn either_holder(&self, r: &mut Reader, w: &mut Writer, direct: impl FnOnce(&Self, &mut Reader, &mut Writer) -> Result<()>) -> Result<()> {
        if copy_bool(r, w)? {
            self.holder(r, w, Reg::Same, direct)
        } else {
            copy_string(r, w)
        }
    }
    /// `HolderSet`: varint n+1 then n ids, or 0 then a tag key string.
    pub fn holder_set(&self, r: &mut Reader, w: &mut Writer, reg: Reg) -> Result<()> {
        let v = r.varint()?;
        w.varint(v);
        if v == 0 {
            return copy_string(r, w);
        }
        for _ in 0..(v - 1) {
            let id = r.varint()?;
            w.varint(self.reg_id(reg, id));
        }
        Ok(())
    }
    pub fn opt_holder_set(&self, r: &mut Reader, w: &mut Writer, reg: Reg) -> Result<()> {
        if copy_bool(r, w)? {
            self.holder_set(r, w, reg)?;
        }
        Ok(())
    }
    pub fn sound_event(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        self.holder(r, w, Reg::Sound, |_, r, w| {
            copy_string(r, w)?;
            if copy_bool(r, w)? {
                copy_bytes(r, w, 4)?;
            }
            Ok(())
        })
    }
    pub fn opt_sound_event(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        if copy_bool(r, w)? {
            self.sound_event(r, w)?;
        }
        Ok(())
    }

    // ---- item stacks -----------------------------------------------------------------------

    /// Full item stack: count, then id + component patch when count > 0.
    pub fn item_stack(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        self.item_stack_with(r, w, false)
    }
    /// Serverbound creative-slot stacks carry a byte length before every component value.
    pub fn length_prefixed_item_stack(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        self.item_stack_with(r, w, true)
    }
    fn item_stack_with(&self, r: &mut Reader, w: &mut Writer, length_prefixed: bool) -> Result<()> {
        let count = copy_varint(r, w)?;
        if count <= 0 {
            return Ok(());
        }
        let id = r.varint()?;
        w.varint(self.item(id));
        self.components(r, w, length_prefixed)
    }
    /// Item cost (merchant offers): id, count, then only "added" components (varint n + comps).
    pub fn item_cost(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let id = r.varint()?;
        w.varint(self.item(id));
        copy_varint(r, w)?;
        self.component_list(r, w)
    }
    pub fn opt_item_cost(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        if copy_bool(r, w)? {
            self.item_cost(r, w)?;
        }
        Ok(())
    }
    /// varint count + that many components (no removals).
    fn component_list(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let n = r.varint()?;
        if !(0..=512).contains(&n) {
            return Err(err("bad component count"));
        }
        let mut body = Writer::new();
        let mut kept = 0;
        for _ in 0..n {
            if self.component(r, &mut body, false)? {
                kept += 1;
            }
        }
        w.varint(kept);
        w.raw(&body.buf);
        Ok(())
    }
    /// Hashed stack (serverbound container clicks): only ids need remapping, not payloads.
    pub fn hashed_stack(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        if !copy_bool(r, w)? {
            return Ok(());
        }
        let id = r.varint()?;
        w.varint(self.item(id));
        copy_varint(r, w)?; // count
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
    pub fn components(&self, r: &mut Reader, w: &mut Writer, length_prefixed: bool) -> Result<()> {
        let added = r.varint()?;
        let removed = r.varint()?;
        if added < 0 || removed < 0 || added + removed > 512 {
            return Err(err("bad component counts"));
        }
        let mut body = Writer::new();
        let mut kept_added = 0;
        for _ in 0..added {
            if self.component(r, &mut body, length_prefixed)? {
                kept_added += 1;
            }
        }
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

    /// Reads one component (type id [+ byte length] + value) and writes the converted one.
    /// Returns whether anything was written.
    pub fn component(&self, r: &mut Reader, w: &mut Writer, length_prefixed: bool) -> Result<bool> {
        let ty = r.varint()?;
        let name = self.source_component_name(ty).ok_or_else(|| err(&format!("unknown data component type {ty}")))?;
        let mut value = Writer::new();
        let keep = if length_prefixed {
            let len = r.varint()?;
            let bytes = r.bytes(usize::try_from(len).map_err(|_| err("negative component length"))?)?;
            let mut sub = Reader::new(bytes);
            let keep = self.component_value(name, &mut sub, &mut value)?;
            if !sub.is_empty() {
                return Err(err(&format!("trailing bytes in component {name}")));
            }
            keep
        } else {
            self.component_value(name, r, &mut value)?
        };
        let Some(id) = keep.then(|| self.component_type(ty)).flatten() else { return Ok(false) };
        w.varint(id);
        if length_prefixed {
            w.varint(value.buf.len() as i32);
        }
        w.raw(&value.buf);
        Ok(true)
    }

    /// Parses one component payload named `name`, writing the target form. Returns whether the
    /// component is kept.
    fn component_value(&self, name: &str, r: &mut Reader, w: &mut Writer) -> Result<bool> {
        macro_rules! varint { () => {{ copy_varint(r, w)?; }}; }
        macro_rules! f32 { () => {{ copy_bytes(r, w, 4)?; }}; }
        macro_rules! bool { () => {{ copy_bool(r, w)?; }}; }
        macro_rules! string { () => {{ copy_string(r, w)?; }}; }
        macro_rules! nbt { () => {{ copy_nbt(r, w)?; }}; }
        match name {
            "max_stack_size" | "max_damage" | "damage" | "repair_cost" | "map_id" | "map_post_processing" | "ominous_bottle_amplifier"
            | "enchantable" | "rarity" | "base_color" | "villager/variant" | "wolf/variant" | "wolf/sound_variant" | "wolf/collar"
            | "fox/variant" | "salmon/size" | "parrot/variant" | "tropical_fish/pattern" | "tropical_fish/base_color"
            | "tropical_fish/pattern_color" | "mooshroom/variant" | "rabbit/variant" | "pig/variant" | "cow/variant" | "frog/variant"
            | "horse/variant" | "llama/variant" | "axolotl/variant" | "cat/variant" | "cat/collar" | "sheep/color" | "shulker/color" => varint!(),
            "potion_duration_scale" => f32!(),
            "dyed_color" | "map_color" => copy_bytes(r, w, 4)?,
            "unbreakable" | "glider" | "creative_slot_lock" => {}
            "enchantment_glint_override" => bool!(),
            "custom_data" | "custom_name" | "item_name" | "map_decorations" | "debug_stick_state" | "bucket_entity_data" | "recipes"
            | "container_loot" | "intangible_projectile" | "lock" | "entity_data" | "block_entity_data" => nbt!(),
            "lore" => {
                let n = copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    nbt!();
                }
            }
            "item_model" | "tooltip_style" | "note_block_sound" | "damage_resistant" | "provides_banner_patterns" => string!(),
            "chicken/variant" => {
                // Either<registry id, key>
                if copy_bool(r, w)? {
                    varint!();
                } else {
                    string!();
                }
            }
            "enchantments" | "stored_enchantments" | "suspicious_stew_effects" => {
                let n = copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    varint!();
                    varint!();
                }
            }
            "can_place_on" | "can_break" => {
                let n = copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    self.block_predicate(r, w)?;
                }
            }
            "attribute_modifiers" => self.attribute_modifiers(r, w)?,
            "custom_model_data" => {
                copy_int_list(r, w, 4)?; // floats
                copy_int_list(r, w, 1)?; // booleans
                let n = copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    string!();
                }
                copy_int_list(r, w, 4)?; // colors
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
                copy_opt_string(r, w)?;
            }
            "tool" => {
                let n = copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    self.holder_set(r, w, Reg::Block)?;
                    if copy_bool(r, w)? {
                        f32!();
                    }
                    if copy_bool(r, w)? {
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
            "equippable" => self.equippable(r, w)?,
            "repairable" => self.holder_set(r, w, Reg::Item)?,
            "blocks_attacks" => {
                f32!();
                f32!();
                let n = copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    f32!();
                    self.opt_holder_set(r, w, Reg::Same)?;
                    f32!();
                    f32!();
                }
                copy_bytes(r, w, 12)?; // item damage function
                copy_opt_string(r, w)?; // bypassed-by tag
                self.opt_sound_event(r, w)?;
                self.opt_sound_event(r, w)?;
            }
            "charged_projectiles" | "bundle_contents" | "container" => {
                let n = copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    self.item_stack(r, w)?;
                }
            }
            "potion_contents" => {
                if copy_bool(r, w)? {
                    varint!();
                }
                if copy_bool(r, w)? {
                    copy_bytes(r, w, 4)?;
                }
                self.potion_effects(r, w)?;
                copy_opt_string(r, w)?;
            }
            "writable_book_content" => {
                let n = copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    string!();
                    copy_opt_string(r, w)?;
                }
            }
            "written_book_content" => {
                string!();
                copy_opt_string(r, w)?;
                string!(); // author
                varint!(); // generation
                let n = copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    nbt!();
                    copy_opt_nbt(r, w)?;
                }
                bool!();
            }
            "trim" => {
                self.holder(r, w, Reg::Same, Self::trim_material)?;
                self.holder(r, w, Reg::Same, |_, r, w| {
                    copy_string(r, w)?; // asset
                    copy_nbt(r, w)?; // description
                    copy_bool(r, w)?; // decal
                    Ok(())
                })?;
            }
            "provides_trim_material" => self.either_holder(r, w, Self::trim_material)?,
            "instrument" => self.either_holder(r, w, |s, r, w| {
                s.sound_event(r, w)?;
                copy_bytes(r, w, 8)?; // use duration, range
                copy_nbt(r, w) // description
            })?,
            "jukebox_playable" => self.either_holder(r, w, |s, r, w| {
                s.sound_event(r, w)?;
                copy_nbt(r, w)?; // description
                copy_bytes(r, w, 4)?; // length
                copy_varint(r, w)?; // comparator output
                Ok(())
            })?,
            "lodestone_tracker" => {
                if copy_bool(r, w)? {
                    string!();
                    copy_bytes(r, w, 8)?;
                }
                bool!();
            }
            "firework_explosion" => self.firework_explosion(r, w)?,
            "fireworks" => {
                varint!();
                let n = copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    self.firework_explosion(r, w)?;
                }
            }
            "profile" => {
                copy_opt_string(r, w)?;
                if copy_bool(r, w)? {
                    copy_bytes(r, w, 16)?;
                }
                let n = copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    string!();
                    string!();
                    copy_opt_string(r, w)?;
                }
            }
            "banner_patterns" => {
                let n = copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    self.holder(r, w, Reg::Same, |_, r, w| {
                        copy_string(r, w)?;
                        copy_string(r, w)
                    })?;
                    varint!();
                }
            }
            "pot_decorations" => {
                let n = copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    let id = r.varint()?;
                    w.varint(self.item(id));
                }
            }
            "block_state" => {
                let n = copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    string!();
                    string!();
                }
            }
            "bees" => {
                let n = copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    nbt!();
                    varint!();
                    varint!();
                }
            }
            "break_sound" => self.sound_event(r, w)?,
            "painting/variant" => self.holder(r, w, Reg::Same, |_, r, w| {
                copy_varint(r, w)?;
                copy_varint(r, w)?;
                copy_string(r, w)?;
                copy_opt_nbt(r, w)?;
                copy_opt_nbt(r, w)
            })?,
            other => return Err(err(&format!("no codec for data component {other}"))),
        }
        Ok(true)
    }

    /// 1.21.6 equippable = 1.21.5 equippable + bool "can be sheared" + shearing sound.
    fn equippable(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        copy_varint(r, w)?; // slot
        self.sound_event(r, w)?;
        copy_opt_string(r, w)?; // model
        copy_opt_string(r, w)?; // camera overlay
        self.opt_holder_set(r, w, Reg::Entity)?;
        copy_bytes(r, w, 4)?; // dispensable, swappable, damage on hurt, equip on interact
        if self.dir == Dir::ToClient {
            r.bool()?;
            let mut scratch = Writer::new();
            self.sound_event(r, &mut scratch)?;
        } else {
            w.bool(false);
            let snip = d::NEW_SOUND_NAMES.iter().position(|n| *n == "item.shears.snip").map_or(0, |p| p as i32);
            w.varint(snip + 1);
        }
        Ok(())
    }

    /// 1.21.6 adds a display mode (0 default, 1 hidden, 2 override text + component) per entry.
    /// Attributes the old client lacks are dropped.
    fn attribute_modifiers(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let n = r.varint()?;
        let mut body = Writer::new();
        let mut kept = 0;
        for _ in 0..n.max(0) {
            let attr = r.varint()?;
            let mut e = Writer::new();
            e.varint(self.attribute(attr));
            copy_string(r, &mut e)?; // modifier id
            copy_bytes(r, &mut e, 8)?; // amount (f64)
            copy_varint(r, &mut e)?; // operation
            copy_varint(r, &mut e)?; // slot group
            if self.dir == Dir::ToClient {
                if r.varint()? == 2 {
                    nbt::skip(r)?;
                }
            } else {
                e.varint(0);
            }
            if self.attribute_mapped(attr) {
                body.raw(&e.buf);
                kept += 1;
            }
        }
        w.varint(kept);
        w.raw(&body.buf);
        Ok(())
    }

    /// Trim material (1.21.5 direct form): asset name, overrides, description.
    fn trim_material(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        copy_string(r, w)?;
        let n = copy_varint(r, w)?;
        for _ in 0..n.max(0) {
            copy_string(r, w)?;
            copy_string(r, w)?;
        }
        copy_nbt(r, w)
    }

    fn consume_effects(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let n = copy_varint(r, w)?;
        for _ in 0..n.max(0) {
            match copy_varint(r, w)? {
                0 => {
                    self.potion_effects(r, w)?;
                    copy_bytes(r, w, 4)?; // probability
                }
                1 => self.holder_set(r, w, Reg::Same)?,
                2 => {}
                3 => copy_bytes(r, w, 4)?, // teleport diameter
                4 => self.sound_event(r, w)?,
                _ => return Err(err("unknown consume effect")),
            }
        }
        Ok(())
    }

    pub fn potion_effects(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let n = copy_varint(r, w)?;
        for _ in 0..n.max(0) {
            copy_varint(r, w)?; // effect
            Self::potion_effect_data(r, w)?;
        }
        Ok(())
    }

    fn potion_effect_data(r: &mut Reader, w: &mut Writer) -> Result<()> {
        copy_varint(r, w)?; // amplifier
        copy_varint(r, w)?; // duration
        copy_bytes(r, w, 3)?; // ambient, particles, icon
        if copy_bool(r, w)? {
            Self::potion_effect_data(r, w)?;
        }
        Ok(())
    }

    fn firework_explosion(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        copy_varint(r, w)?;
        copy_int_list(r, w, 4)?;
        copy_int_list(r, w, 4)?;
        copy_bytes(r, w, 2)
    }

    /// Block predicate (1.21.5 form): blocks, state properties, nbt, exact components, partial
    /// predicates (varint type + nbt, no "is predicate type" flag before 1.21.11).
    fn block_predicate(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        self.opt_holder_set(r, w, Reg::Block)?;
        if copy_bool(r, w)? {
            let n = copy_varint(r, w)?;
            for _ in 0..n.max(0) {
                copy_string(r, w)?;
                if copy_bool(r, w)? {
                    copy_string(r, w)?;
                } else {
                    copy_opt_string(r, w)?;
                    copy_opt_string(r, w)?;
                }
            }
        }
        copy_opt_nbt(r, w)?;
        self.component_list(r, w)?;
        let n = copy_varint(r, w)?;
        for _ in 0..n.max(0) {
            copy_varint(r, w)?;
            copy_nbt(r, w)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comp(name: &str) -> i32 {
        d::NEW_COMPONENT_NAMES.iter().position(|n| *n == name).unwrap() as i32
    }

    #[test]
    fn stack_remaps_item_id() {
        let items = Items::new(Dir::ToClient);
        let brick = d::BRICK_ITEM_NEW as i32;
        let mut w = Writer::new();
        w.varint(3);
        w.varint(brick);
        w.varint(1);
        w.varint(0);
        w.varint(comp("max_stack_size"));
        w.varint(64);
        let mut out = Writer::new();
        items.item_stack(&mut Reader::new(&w.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        assert_eq!(r.varint().unwrap(), 3);
        assert_eq!(r.varint().unwrap(), d::BRICK_ITEM_OLD as i32);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), comp("max_stack_size"));
        assert_eq!(r.varint().unwrap(), 64);
        assert!(r.is_empty());
    }

    fn equippable_body(w: &mut Writer, with_1_21_6_tail: bool) {
        w.varint(4); // slot: head
        w.varint(11); // sound holder id 10
        w.bool(false); // model
        w.bool(false); // camera overlay
        w.bool(false); // allowed entities
        w.raw(&[1, 1, 1, 0]);
        if with_1_21_6_tail {
            w.bool(true);
            w.varint(5);
        }
    }

    #[test]
    fn equippable_loses_shearing_fields_for_the_client() {
        let items = Items::new(Dir::ToClient);
        let mut w = Writer::new();
        equippable_body(&mut w, true);
        let mut out = Writer::new();
        assert!(items.component_value("equippable", &mut Reader::new(&w.buf), &mut out).unwrap());
        let mut r = Reader::new(&out.buf);
        assert_eq!(r.varint().unwrap(), 4);
        assert_eq!(r.varint().unwrap(), items.sound(10) + 1);
        assert_eq!(r.bytes(3).unwrap(), &[0, 0, 0]);
        assert_eq!(r.bytes(4).unwrap(), &[1, 1, 1, 0]);
        assert!(r.is_empty());
    }

    #[test]
    fn equippable_gains_shearing_fields_for_the_server() {
        let items = Items::new(Dir::ToServer);
        let mut w = Writer::new();
        equippable_body(&mut w, false);
        let mut out = Writer::new();
        items.component_value("equippable", &mut Reader::new(&w.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        r.varint().unwrap();
        r.varint().unwrap();
        r.bytes(7).unwrap();
        assert!(!r.bool().unwrap());
        assert!(r.varint().unwrap() > 0);
        assert!(r.is_empty());
    }

    #[test]
    fn attribute_modifier_display_is_stripped_and_new_attributes_dropped() {
        let items = Items::new(Dir::ToClient);
        let armor = d::NEW_ATTRIBUTE_NAMES.iter().position(|n| *n == "armor").unwrap() as i32;
        let camera = d::NEW_ATTRIBUTE_NAMES.iter().position(|n| *n == "camera_distance").unwrap() as i32;
        let mut w = Writer::new();
        w.varint(2);
        for (attr, display) in [(armor, 2), (camera, 0)] {
            w.varint(attr);
            w.string("minecraft:test");
            w.f64(1.5);
            w.varint(0);
            w.varint(0);
            w.varint(display);
            if display == 2 {
                nbt::write(&mut w, &nbt::Tag::String("hi".into()));
            }
        }
        let mut out = Writer::new();
        items.component_value("attribute_modifiers", &mut Reader::new(&w.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), items.attribute(armor));
        assert_eq!(r.string().unwrap(), "minecraft:test");
        assert_eq!(r.f64().unwrap(), 1.5);
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), 0);
        assert!(r.is_empty());
    }

    #[test]
    fn length_prefixed_components_are_reframed() {
        let items = Items::new(Dir::ToServer);
        let mut value = Writer::new();
        equippable_body(&mut value, false);
        let mut w = Writer::new();
        w.varint(1);
        w.varint(d::BRICK_ITEM_OLD as i32);
        w.varint(1);
        w.varint(0);
        w.varint(comp("equippable"));
        w.varint(value.buf.len() as i32);
        w.raw(&value.buf);
        let mut out = Writer::new();
        items.length_prefixed_item_stack(&mut Reader::new(&w.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), d::BRICK_ITEM_NEW as i32);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), comp("equippable"));
        let len = r.varint().unwrap() as usize;
        assert_eq!(len, r.remaining().len());
        assert!(len > value.buf.len());
    }
}
