//! Item stacks and their data components, 1.21.9 ↔ 1.21.7.
//!
//! Both versions have the same 96 component types in the same order. Four of them changed their
//! wire format in 1.21.9 (`entity_data`, `block_entity_data`, `bees`, `profile`); every other
//! component is still parsed with its exact codec so registry ids nested inside (items, blocks,
//! entities, sounds) can be remapped and the stream stays in sync.

use std::cell::Cell;

use super::data as d;
use crate::proto::buf::{DecodeError, Reader, Result, Writer};
use crate::proto::nbt::{self, Tag};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    /// 1.21.9 server → 1.21.7 client
    ToClient,
    /// 1.21.7 client → 1.21.9 server
    ToServer,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Reg {
    Item,
    Block,
    Entity,
    Sound,
    Attribute,
    /// Synced via registry data or otherwise identical on both sides.
    Same,
}

fn err(msg: &str) -> DecodeError {
    DecodeError::Other(msg.to_string())
}

pub struct Items {
    pub dir: Dir,
    /// Set while parsing a creative-slot stack, whose components are length-prefixed.
    length_prefixed: Cell<bool>,
}

impl Items {
    pub fn new(dir: Dir) -> Self {
        Self { dir, length_prefixed: Cell::new(false) }
    }

    // ---- id remaps -------------------------------------------------------------------------

    fn lookup(table: &[u32], reverse: &[u32], dir: Dir, id: i32) -> i32 {
        if id < 0 {
            return id;
        }
        let t = if dir == Dir::ToClient { table } else { reverse };
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
    pub fn attribute(&self, id: i32) -> i32 {
        Self::lookup(&d::ATTRIBUTES, &d::ATTRIBUTES_REVERSE, self.dir, id)
    }
    pub fn particle(&self, id: i32) -> i32 {
        Self::lookup(&d::PARTICLES, &d::PARTICLES_REVERSE, self.dir, id)
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
            Reg::Attribute => self.attribute(id),
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
    fn copy_int_array(r: &mut Reader, w: &mut Writer) -> Result<()> {
        let n = Self::copy_varint(r, w)?;
        Self::copy_bytes(r, w, n.max(0) as usize * 4)
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
    /// `EitherHolder<T>`: bool true + holder, or false + registry key string.
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
        w.varint(v);
        if v == 0 {
            return Self::copy_string(r, w);
        }
        for _ in 0..(v - 1) {
            let id = r.varint()?;
            w.varint(self.reg_id(reg, id));
        }
        Ok(())
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
    /// Serverbound creative slot stack: like `item_stack`, but every component value carries
    /// a varint byte length (1.21.5+).
    pub fn length_prefixed_stack(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        self.length_prefixed.set(true);
        let res = self.item_stack(r, w);
        self.length_prefixed.set(false);
        res
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
    /// Hashed stack (serverbound container clicks): component ids are identical in both
    /// versions, so only the item id changes.
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
            let ty = r.varint()?;
            if let Some(t) = self.component_type(ty) {
                body.varint(t);
                kept += 1;
            }
        }
        w.varint(kept);
        w.raw(&body.buf);
        Ok(())
    }

    /// `varint n` followed by n components (exact-match component lists).
    fn component_list(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
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
        let mut removed_body = Writer::new();
        let mut kept_removed = 0;
        for _ in 0..removed {
            let ty = r.varint()?;
            if let Some(t) = self.component_type(ty) {
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

    /// Reads one component (type id + value) and writes the converted one. Returns whether
    /// anything was written.
    pub fn component(&self, r: &mut Reader, w: &mut Writer) -> Result<bool> {
        let ty = r.varint()?;
        let name = self.source_component_name(ty).ok_or_else(|| err(&format!("unknown data component type {ty}")))?;
        let mut value = Writer::new();
        if self.length_prefixed.get() {
            let len = r.varint()?;
            let slice = r.bytes(usize::try_from(len).map_err(|_| err("negative component length"))?)?;
            let mut sub = Reader::new(slice);
            self.component_value(name, &mut sub, &mut value)?;
        } else {
            self.component_value(name, r, &mut value)?;
        }
        let Some(target) = self.component_type(ty) else { return Ok(false) };
        w.varint(target);
        if self.length_prefixed.get() {
            w.varint(value.buf.len() as i32);
        }
        w.raw(&value.buf);
        Ok(true)
    }

    /// Parses one component payload named `name` (source version), writing the target form.
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
            | "tropical_fish/pattern_color" | "mooshroom/variant" | "rabbit/variant" | "pig/variant" | "cow/variant" | "frog/variant"
            | "horse/variant" | "llama/variant" | "axolotl/variant" | "cat/variant" | "cat/collar" | "sheep/color" | "shulker/color" => varint!(),
            "chicken/variant" => {
                // Either<registry id, key>
                if Self::copy_bool(r, w)? {
                    varint!();
                } else {
                    string!();
                }
            }
            "potion_duration_scale" => f32!(),
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
                    let attr = r.varint()?;
                    w.varint(self.attribute(attr));
                    string!(); // id
                    Self::copy_bytes(r, w, 8)?; // amount
                    varint!(); // operation
                    varint!(); // slot group
                    if Self::copy_varint(r, w)? == 2 {
                        nbt!(); // override text
                    }
                }
            }
            "custom_model_data" => {
                Self::copy_int_array(r, w)?; // floats (4 bytes each)
                let n = Self::copy_varint(r, w)?;
                Self::copy_bytes(r, w, n.max(0) as usize)?; // booleans
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    string!();
                }
                Self::copy_int_array(r, w)?; // colors
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
                f32!(); // default mining speed
                varint!(); // damage per block
                bool!(); // can destroy blocks in creative
            }
            "weapon" => {
                varint!();
                f32!();
            }
            "equippable" => {
                varint!(); // slot
                self.sound_event(r, w)?;
                Self::copy_opt_string(r, w)?; // model
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
            "dyed_color" | "map_color" => Self::copy_bytes(r, w, 4)?,
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
                self.holder(r, w, Reg::Same, |_, r, w| {
                    Self::copy_string(r, w)?; // asset
                    Self::copy_nbt(r, w)?; // description
                    Self::copy_bool(r, w)?; // decal
                    Ok(())
                })?;
            }
            "provides_trim_material" => {
                if Self::copy_bool(r, w)? {
                    self.trim_material(r, w)?;
                } else {
                    string!();
                }
            }
            "instrument" => self.either_holder(r, w, |s, r, w| {
                s.sound_event(r, w)?;
                Self::copy_bytes(r, w, 8)?; // use duration, range
                Self::copy_nbt(r, w) // description
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
            "break_sound" => self.sound_event(r, w)?,
            "painting/variant" => self.holder(r, w, Reg::Same, |_, r, w| {
                Self::copy_varint(r, w)?;
                Self::copy_varint(r, w)?;
                Self::copy_string(r, w)?;
                Self::copy_opt_nbt(r, w)?;
                Self::copy_opt_nbt(r, w)
            })?,
            // ---- components whose format changed in 1.21.9 ----
            "entity_data" => self.typed_entity_data(r, w, true)?,
            "block_entity_data" => self.typed_entity_data(r, w, false)?,
            "bees" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    if self.dir == Dir::ToClient {
                        // 1.21.9: typed entity data → 1.21.7: plain tag with an id.
                        r.varint()?;
                        let mut tag = nbt::read(r)?;
                        tag.insert("id", Tag::String("minecraft:bee".into()));
                        nbt::write(w, &tag);
                    } else {
                        let tag = nbt::read(r)?;
                        w.varint(Self::name_index(&d::NEW_ENTITY_NAMES, "bee").unwrap_or(0));
                        nbt::write(w, &tag);
                    }
                    varint!(); // ticks in hive
                    varint!(); // min ticks in hive
                }
            }
            "profile" => {
                if self.dir == Dir::ToClient {
                    resolvable_profile_to_game_profile(r, w)?;
                } else {
                    // 1.21.7 game profile → 1.21.9 resolvable profile (dynamic, no skin patch).
                    w.bool(false);
                    copy_game_profile(r, w)?;
                    w.raw(&[0, 0, 0, 0]); // body, cape, elytra texture, model: absent
                }
            }
            other => return Err(err(&format!("no codec for data component {other}"))),
        }
        Ok(())
    }

    fn name_index(names: &[&str], name: &str) -> Option<i32> {
        let name = name.strip_prefix("minecraft:").unwrap_or(name);
        names.iter().position(|n| *n == name).map(|i| i as i32)
    }

    /// `entity_data` / `block_entity_data`: 1.21.9 sends `varint type + tag`, 1.21.7 a tag whose
    /// `id` string names the type.
    fn typed_entity_data(&self, r: &mut Reader, w: &mut Writer, entity: bool) -> Result<()> {
        if self.dir == Dir::ToClient {
            let ty = r.varint()?;
            let names: &[&str] = if entity { &d::NEW_ENTITY_NAMES } else { &d::NEW_BLOCK_ENTITY_NAMES };
            let mut tag = nbt::read(r)?;
            let fallback = if entity { "pig" } else { "furnace" };
            let name = names.get(ty.max(0) as usize).copied().unwrap_or(fallback);
            tag.insert("id", Tag::String(format!("minecraft:{name}")));
            nbt::write(w, &tag);
        } else {
            let tag = nbt::read(r)?;
            let names: &[&str] = if entity { &d::NEW_ENTITY_NAMES } else { &d::NEW_BLOCK_ENTITY_NAMES };
            let id = tag.get("id").and_then(Tag::as_str).and_then(|n| Self::name_index(names, n)).unwrap_or(0);
            w.varint(id);
            nbt::write(w, &tag);
        }
        Ok(())
    }

    fn trim_material(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        self.holder(r, w, Reg::Same, |_, r, w| {
            Self::copy_string(r, w)?; // asset name
            let n = Self::copy_varint(r, w)?;
            for _ in 0..n.max(0) {
                Self::copy_string(r, w)?;
                Self::copy_string(r, w)?;
            }
            Self::copy_nbt(r, w) // description
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
                3 => Self::copy_bytes(r, w, 4)?, // diameter
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
            self.potion_effect_data(r, w)?; // hidden effect
        }
        Ok(())
    }

    fn firework_explosion(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        Self::copy_varint(r, w)?; // shape
        Self::copy_int_array(r, w)?; // colors
        Self::copy_int_array(r, w)?; // fade colors
        Self::copy_bytes(r, w, 2) // trail, twinkle
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
        Self::copy_opt_nbt(r, w)?;
        // Data component matchers: exact components, then partial predicates (varint type + tag).
        self.component_list(r, w)?;
        let n = Self::copy_varint(r, w)?;
        for _ in 0..n.max(0) {
            Self::copy_varint(r, w)?;
            Self::copy_nbt(r, w)?;
        }
        Ok(())
    }
}

/// Game profile (1.21.7 `profile` component, and the dynamic half of a resolvable profile):
/// optional name, optional uuid, properties.
pub fn copy_game_profile(r: &mut Reader, w: &mut Writer) -> Result<()> {
    Items::copy_opt_string(r, w)?;
    if Items::copy_bool(r, w)? {
        w.uuid(r.uuid()?);
    }
    copy_properties(r, w)
}

pub fn copy_properties(r: &mut Reader, w: &mut Writer) -> Result<()> {
    let n = Items::copy_varint(r, w)?;
    for _ in 0..n.max(0) {
        Items::copy_string(r, w)?;
        Items::copy_string(r, w)?;
        Items::copy_opt_string(r, w)?;
    }
    Ok(())
}

/// Reads a 1.21.9 resolvable profile and writes the 1.21.7 game profile it contains; the
/// texture/model overrides have no older equivalent.
pub fn resolvable_profile_to_game_profile(r: &mut Reader, w: &mut Writer) -> Result<()> {
    if r.bool()? {
        // complete profile: uuid, name, properties
        let id = r.uuid()?;
        let name = r.string()?;
        w.bool(true);
        w.string(name);
        w.bool(true);
        w.uuid(id);
        copy_properties(r, w)?;
    } else {
        copy_game_profile(r, w)?;
    }
    let mut sink = Writer::new();
    for _ in 0..3 {
        Items::copy_opt_string(r, &mut sink)?;
    }
    if r.bool()? {
        r.bool()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comp(names: &[&str], name: &str) -> i32 {
        names.iter().position(|n| *n == name).unwrap() as i32
    }

    #[test]
    fn plain_stack_remaps_item_id() {
        let items = Items::new(Dir::ToClient);
        let last = d::ITEMS.len() as i32 - 1;
        let mut w = Writer::new();
        w.varint(3);
        w.varint(last);
        w.varint(1);
        w.varint(0);
        w.varint(comp(&d::NEW_COMPONENT_NAMES, "max_stack_size"));
        w.varint(64);
        let mut out = Writer::new();
        items.item_stack(&mut Reader::new(&w.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        assert_eq!(r.varint().unwrap(), 3);
        assert_eq!(r.varint().unwrap(), d::ITEMS[last as usize] as i32);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), comp(&d::OLD_COMPONENT_NAMES, "max_stack_size"));
        assert_eq!(r.varint().unwrap(), 64);
        assert!(r.is_empty());
    }

    #[test]
    fn entity_data_gains_id_and_loses_type() {
        let items = Items::new(Dir::ToClient);
        let mut w = Writer::new();
        w.varint(1);
        w.varint(1);
        w.varint(1);
        w.varint(0);
        w.varint(comp(&d::NEW_COMPONENT_NAMES, "entity_data"));
        w.varint(comp(&d::NEW_ENTITY_NAMES, "zombie"));
        nbt::write(&mut w, &Tag::compound());
        let mut out = Writer::new();
        items.item_stack(&mut Reader::new(&w.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        for _ in 0..4 {
            r.varint().unwrap();
        }
        assert_eq!(r.varint().unwrap(), comp(&d::OLD_COMPONENT_NAMES, "entity_data"));
        let tag = nbt::read(&mut r).unwrap();
        assert_eq!(tag.get("id").and_then(Tag::as_str), Some("minecraft:zombie"));
        assert!(r.is_empty());

        // and back
        let back = Items::new(Dir::ToServer);
        let mut s = Writer::new();
        s.varint(1);
        s.varint(1);
        s.varint(1);
        s.varint(0);
        s.varint(comp(&d::OLD_COMPONENT_NAMES, "entity_data"));
        nbt::write(&mut s, &tag);
        let mut out = Writer::new();
        back.item_stack(&mut Reader::new(&s.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        for _ in 0..5 {
            r.varint().unwrap();
        }
        assert_eq!(r.varint().unwrap(), comp(&d::NEW_ENTITY_NAMES, "zombie"));
    }

    #[test]
    fn complete_profile_becomes_game_profile() {
        let mut w = Writer::new();
        w.bool(true);
        w.uuid(42);
        w.string("Steve");
        w.varint(0);
        w.raw(&[1]);
        w.string("minecraft:skin");
        w.raw(&[0, 0, 1, 1]); // cape absent, elytra absent, model present = slim
        let mut out = Writer::new();
        let mut r = Reader::new(&w.buf);
        resolvable_profile_to_game_profile(&mut r, &mut out).unwrap();
        assert!(r.is_empty());
        let mut r = Reader::new(&out.buf);
        assert!(r.bool().unwrap());
        assert_eq!(r.string().unwrap(), "Steve");
        assert!(r.bool().unwrap());
        assert_eq!(r.uuid().unwrap(), 42);
        assert_eq!(r.varint().unwrap(), 0);
        assert!(r.is_empty());
    }

    #[test]
    fn creative_slot_components_are_length_prefixed() {
        let items = Items::new(Dir::ToServer);
        let mut w = Writer::new();
        w.varint(1);
        w.varint(1);
        w.varint(1);
        w.varint(0);
        w.varint(comp(&d::OLD_COMPONENT_NAMES, "profile"));
        let mut value = Writer::new();
        value.bool(true);
        value.string("Alex");
        value.bool(false);
        value.varint(0);
        w.varint(value.buf.len() as i32);
        w.raw(&value.buf);
        let mut out = Writer::new();
        let mut r = Reader::new(&w.buf);
        items.length_prefixed_stack(&mut r, &mut out).unwrap();
        assert!(r.is_empty());
        let mut r = Reader::new(&out.buf);
        for _ in 0..5 {
            r.varint().unwrap();
        }
        let len = r.varint().unwrap() as usize;
        assert_eq!(len, r.remaining().len());
        assert!(!r.bool().unwrap()); // dynamic profile
    }
}
