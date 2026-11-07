//! Item stacks and their data components, 1.21.7 ↔ 1.21.6.
//!
//! Both versions share the same 96 data components with identical wire formats (ViaVersion uses
//! `VersionedTypes.V1_21_6` on both sides), so nothing is dropped or converted. Every component is
//! still parsed with its exact 1.21.6 codec, because item and sound ids inside them must be remapped:
//! 1.21.7 added the "Lava Chicken" music disc and its sound, which shifts every later id.

use super::super::data_1_21_6 as d;
use crate::proto::buf::{DecodeError, Reader, Result, Writer};
use crate::proto::nbt;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    /// 1.21.7 server → 1.21.6 client
    ToClient,
    /// 1.21.6 client → 1.21.7 server
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
    pub fn attribute(&self, id: i32) -> i32 {
        Self::lookup(&d::ATTRIBUTES, &d::ATTRIBUTES_REVERSE, self.dir, id)
    }
    pub fn particle(&self, id: i32) -> i32 {
        Self::lookup(&d::PARTICLES, &d::PARTICLES_REVERSE, self.dir, id)
    }
    /// Component type id on the other side (the component lists are identical; kept table-driven).
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

    /// Renames a namespaced registry key through an id table (e.g. an item name in a cooldown
    /// group). Names that exist on both sides come back unchanged.
    fn rename(name: &str, from: &[&str], table: &[u32], to: &[&str]) -> Option<String> {
        let bare = name.strip_prefix("minecraft:").unwrap_or(name);
        let idx = from.iter().position(|n| *n == bare)?;
        let target = to.get(*table.get(idx)? as usize)?;
        if *target == bare {
            return None;
        }
        Some(if name.starts_with("minecraft:") { format!("minecraft:{target}") } else { (*target).to_string() })
    }
    pub fn item_name(&self, name: &str) -> String {
        let renamed = if self.dir == Dir::ToClient {
            Self::rename(name, &d::NEW_ITEM_NAMES, &d::ITEMS, &d::OLD_ITEM_NAMES)
        } else {
            Self::rename(name, &d::OLD_ITEM_NAMES, &d::ITEMS_REVERSE, &d::NEW_ITEM_NAMES)
        };
        renamed.unwrap_or_else(|| name.to_string())
    }
    pub fn sound_name(&self, name: &str) -> String {
        let renamed = if self.dir == Dir::ToClient {
            Self::rename(name, &d::NEW_SOUND_NAMES, &d::SOUNDS, &d::OLD_SOUND_NAMES)
        } else {
            Self::rename(name, &d::OLD_SOUND_NAMES, &d::SOUNDS_REVERSE, &d::NEW_SOUND_NAMES)
        };
        renamed.unwrap_or_else(|| name.to_string())
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
    fn copy_opt_bytes(r: &mut Reader, w: &mut Writer, n: usize) -> Result<()> {
        if Self::copy_bool(r, w)? {
            Self::copy_bytes(r, w, n)?;
        }
        Ok(())
    }
    /// Varint length + that many fixed-size elements.
    fn copy_array(r: &mut Reader, w: &mut Writer, elem: usize) -> Result<()> {
        let n = Self::copy_varint(r, w)?;
        Self::copy_bytes(r, w, n.max(0) as usize * elem)
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
    /// `EitherHolder<T>`: bool; true = holder, false = registry key string.
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
    /// `Holder<SoundEvent>`; inline sounds are referenced by name.
    pub fn sound_event(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        self.holder(r, w, Reg::Sound, |s, r, w| {
            let name = r.string()?;
            w.string(&s.sound_name(name));
            Self::copy_opt_bytes(r, w, 4) // fixed range
        })
    }
    pub fn opt_sound_event(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        if Self::copy_bool(r, w)? {
            self.sound_event(r, w)?;
        }
        Ok(())
    }

    // ---- item stacks -----------------------------------------------------------------------

    /// Item stack (1.20.5 codec, also used for item templates up to 1.21.11): count, then id and
    /// the component patch when count > 0.
    pub fn item_stack(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let count = Self::copy_varint(r, w)?;
        if count <= 0 {
            return Ok(());
        }
        let id = r.varint()?;
        w.varint(self.item(id));
        self.components(r, w, false)
    }
    /// Serverbound creative-slot stack: like `item_stack`, but each added component value is
    /// preceded by its byte length.
    pub fn length_prefixed_stack(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let count = Self::copy_varint(r, w)?;
        if count <= 0 {
            return Ok(());
        }
        let id = r.varint()?;
        w.varint(self.item(id));
        self.components(r, w, true)
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
    /// Hashed stack (serverbound container clicks): only ids need remapping, not payloads.
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
    fn components(&self, r: &mut Reader, w: &mut Writer, length_prefixed: bool) -> Result<()> {
        let added = r.varint()?;
        let removed = r.varint()?;
        if added < 0 || removed < 0 || added + removed > 512 {
            return Err(err("bad component counts"));
        }
        let mut body = Writer::new();
        let mut kept_added = 0;
        for _ in 0..added {
            if length_prefixed {
                let ty = r.varint()?;
                let len = r.varint()?;
                let raw = r.bytes(len.max(0) as usize)?;
                let mut inner = Reader::new(raw);
                let mut value = Writer::new();
                let name = self.source_component_name(ty).ok_or_else(|| err(&format!("unknown data component type {ty}")))?;
                self.component_value(name, &mut inner, &mut value)?;
                if let Some(t) = self.component_type(ty) {
                    body.varint(t);
                    body.varint(value.buf.len() as i32);
                    body.raw(&value.buf);
                    kept_added += 1;
                }
            } else if self.component(r, &mut body)? {
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

    /// Varint count + components (exact component matchers, item costs).
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

    /// Reads one component (type id + value) and writes the converted one. Returns whether it
    /// was written.
    pub fn component(&self, r: &mut Reader, w: &mut Writer) -> Result<bool> {
        let ty = r.varint()?;
        let name = self.source_component_name(ty).ok_or_else(|| err(&format!("unknown data component type {ty}")))?;
        let mut value = Writer::new();
        self.component_value(name, r, &mut value)?;
        let Some(t) = self.component_type(ty) else { return Ok(false) };
        w.varint(t);
        w.raw(&value.buf);
        Ok(true)
    }

    /// Parses one 1.21.6 component payload, writing it with remapped ids.
    fn component_value(&self, name: &str, r: &mut Reader, w: &mut Writer) -> Result<()> {
        macro_rules! varint { () => {{ Self::copy_varint(r, w)?; }}; }
        macro_rules! f32 { () => {{ Self::copy_bytes(r, w, 4)?; }}; }
        macro_rules! bool { () => {{ Self::copy_bool(r, w)?; }}; }
        macro_rules! string { () => {{ Self::copy_string(r, w)?; }}; }
        macro_rules! nbt { () => {{ Self::copy_nbt(r, w)?; }}; }
        match name {
            "max_stack_size" | "max_damage" | "damage" | "rarity" | "repair_cost" | "enchantable" | "map_id" | "map_post_processing"
            | "ominous_bottle_amplifier" | "base_color" | "villager/variant" | "wolf/variant" | "wolf/sound_variant" | "wolf/collar"
            | "fox/variant" | "salmon/size" | "parrot/variant" | "tropical_fish/pattern" | "tropical_fish/base_color"
            | "tropical_fish/pattern_color" | "mooshroom/variant" | "rabbit/variant" | "pig/variant" | "cow/variant" | "frog/variant"
            | "horse/variant" | "llama/variant" | "axolotl/variant" | "cat/variant" | "cat/collar" | "sheep/color" | "shulker/color" => varint!(),
            "unbreakable" | "creative_slot_lock" | "glider" => {}
            "enchantment_glint_override" => bool!(),
            "potion_duration_scale" => f32!(),
            "dyed_color" | "map_color" => Self::copy_bytes(r, w, 4)?,
            "custom_data" | "custom_name" | "item_name" | "intangible_projectile" | "map_decorations" | "debug_stick_state"
            | "entity_data" | "bucket_entity_data" | "block_entity_data" | "recipes" | "lock" | "container_loot" => nbt!(),
            "item_model" | "tooltip_style" | "note_block_sound" | "damage_resistant" | "provides_banner_patterns" => string!(),
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
                    string!(); // modifier id
                    Self::copy_bytes(r, w, 8)?; // amount
                    varint!(); // operation
                    varint!(); // slot group
                    if Self::copy_varint(r, w)? == 2 {
                        nbt!(); // override text
                    }
                }
            }
            "custom_model_data" => {
                Self::copy_array(r, w, 4)?; // floats
                Self::copy_array(r, w, 1)?; // booleans
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    string!();
                }
                Self::copy_array(r, w, 4)?; // colors
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
                    Self::copy_opt_bytes(r, w, 4)?; // speed
                    Self::copy_opt_bytes(r, w, 1)?; // correct for drops
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
                Self::copy_opt_string(r, w)?; // asset
                Self::copy_opt_string(r, w)?; // camera overlay
                self.opt_holder_set(r, w, Reg::Entity)?;
                Self::copy_bytes(r, w, 5)?; // dispensable, swappable, damage on hurt, equip on interact, can be sheared
                self.sound_event(r, w)?; // shearing sound
            }
            "repairable" => self.holder_set(r, w, Reg::Item)?,
            "blocks_attacks" => {
                f32!(); // block delay
                f32!(); // disable cooldown scale
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
                Self::copy_opt_bytes(r, w, 4)?; // custom color
                self.potion_effects(r, w)?;
                Self::copy_opt_string(r, w)?; // custom name
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
            "profile" => {
                Self::copy_opt_string(r, w)?; // name
                Self::copy_opt_bytes(r, w, 16)?; // uuid
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    string!();
                    string!();
                    Self::copy_opt_string(r, w)?;
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
            "bees" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    nbt!();
                    varint!();
                    varint!();
                }
            }
            "break_sound" => self.sound_event(r, w)?,
            "chicken/variant" => {
                if Self::copy_bool(r, w)? {
                    varint!();
                } else {
                    string!();
                }
            }
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

    /// Trim material holder (1.21.5 codec): asset name, per-armor overrides, description.
    fn trim_material(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        self.holder(r, w, Reg::Same, |_, r, w| {
            Self::copy_string(r, w)?;
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
            Self::potion_effect_data(r, w)?;
        }
        Ok(())
    }

    fn potion_effect_data(r: &mut Reader, w: &mut Writer) -> Result<()> {
        Self::copy_varint(r, w)?; // amplifier
        Self::copy_varint(r, w)?; // duration
        Self::copy_bytes(r, w, 3)?; // ambient, particles, icon
        if Self::copy_bool(r, w)? {
            Self::potion_effect_data(r, w)?;
        }
        Ok(())
    }

    fn firework_explosion(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        Self::copy_varint(r, w)?; // shape
        Self::copy_array(r, w, 4)?; // colors
        Self::copy_array(r, w, 4)?; // fade colors
        Self::copy_bytes(r, w, 2) // trail, twinkle
    }

    /// Block predicate (1.21.5 codec): blocks, state properties, nbt, then data component
    /// matchers (exact components + partial predicates as type id + nbt).
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
        let n = Self::copy_varint(r, w)?;
        for _ in 0..n.max(0) {
            Self::copy_varint(r, w)?; // predicate type
            Self::copy_nbt(r, w)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn component_id(name: &str) -> i32 {
        d::NEW_COMPONENT_NAMES.iter().position(|n| *n == name).unwrap() as i32
    }
    fn new_item(name: &str) -> i32 {
        d::NEW_ITEM_NAMES.iter().position(|n| *n == name).unwrap() as i32
    }
    fn old_item(name: &str) -> i32 {
        d::OLD_ITEM_NAMES.iter().position(|n| *n == name).unwrap() as i32
    }

    #[test]
    fn lava_chicken_disc_becomes_pigstep_and_later_ids_shift() {
        let items = Items::new(Dir::ToClient);
        assert_eq!(items.item(new_item("music_disc_lava_chicken")), old_item("music_disc_pigstep"));
        let last = d::NEW_ITEM_NAMES.len() - 1;
        assert_eq!(items.item(last as i32), old_item(d::NEW_ITEM_NAMES[last]));
        let back = Items::new(Dir::ToServer);
        assert_eq!(back.item(old_item(d::NEW_ITEM_NAMES[last])), last as i32);
        assert_eq!(items.item_name("minecraft:music_disc_lava_chicken"), "minecraft:music_disc_pigstep");
        assert_eq!(items.sound_name("minecraft:music_disc.lava_chicken"), "minecraft:music_disc.pigstep");
        assert_eq!(items.item_name("minecraft:stone"), "minecraft:stone");
    }

    #[test]
    fn nested_items_and_sounds_are_remapped() {
        let items = Items::new(Dir::ToClient);
        let last = d::NEW_ITEM_NAMES.len() as i32 - 1;
        let mut w = Writer::new();
        w.varint(1);
        w.varint(new_item("bundle"));
        w.varint(2); // two added components
        w.varint(0);
        w.varint(component_id("bundle_contents"));
        w.varint(1); // one stack inside
        w.varint(4);
        w.varint(last);
        w.varint(0);
        w.varint(0);
        w.varint(component_id("break_sound"));
        w.varint(0); // inline sound event
        w.string("minecraft:music_disc.lava_chicken");
        w.bool(false);
        let mut out = Writer::new();
        items.item_stack(&mut Reader::new(&w.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), old_item("bundle"));
        assert_eq!(r.varint().unwrap(), 2);
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), component_id("bundle_contents"));
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), 4);
        assert_eq!(r.varint().unwrap(), d::ITEMS[last as usize] as i32);
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), component_id("break_sound"));
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.string().unwrap(), "minecraft:music_disc.pigstep");
        assert!(!r.bool().unwrap());
        assert!(r.is_empty());
    }

    #[test]
    fn length_prefixed_creative_stack_is_rebuilt() {
        let items = Items::new(Dir::ToServer);
        let shifted = old_item(d::NEW_ITEM_NAMES[d::NEW_ITEM_NAMES.len() - 1]);
        let mut value = Writer::new();
        value.varint(1); // one pot decoration
        value.varint(shifted);
        let mut w = Writer::new();
        w.varint(1);
        w.varint(shifted);
        w.varint(1);
        w.varint(0);
        w.varint(component_id("pot_decorations"));
        w.varint(value.buf.len() as i32);
        w.raw(&value.buf);
        let mut out = Writer::new();
        items.length_prefixed_stack(&mut Reader::new(&w.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        let last = d::NEW_ITEM_NAMES.len() as i32 - 1;
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), last);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), component_id("pot_decorations"));
        let len = r.varint().unwrap();
        let mut inner = Reader::new(r.bytes(len as usize).unwrap());
        assert_eq!(inner.varint().unwrap(), 1);
        assert_eq!(inner.varint().unwrap(), last);
        assert!(r.is_empty());
    }
}
