//! Item stacks and their data components, 26.2 ↔ 26.1.
//!
//! 26.1 has every 26.2 component except `sulfur_cube_content`, and all shared components keep
//! their wire format, so this codec mostly walks the payload to remap the registry ids inside
//! (items, blocks, entities, sounds, attributes) and drops what the old client lacks.

use super::data_26_1 as d;
use crate::proto::buf::{DecodeError, Reader, Result, Writer};
use crate::proto::nbt;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    /// 26.2 server → 26.1 client
    ToClient,
    /// 26.1 client → 26.2 server
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
    /// Attribute id in the target version, or `None` if the target lacks it (26.2 added five).
    pub fn attribute(&self, id: i32) -> Option<i32> {
        let (names, target): (&[&str], &[&str]) = if self.dir == Dir::ToClient {
            (&d::NEW_ATTRIBUTE_NAMES, &d::OLD_ATTRIBUTE_NAMES)
        } else {
            (&d::OLD_ATTRIBUTE_NAMES, &d::NEW_ATTRIBUTE_NAMES)
        };
        let name = names.get(usize::try_from(id).ok()?)?;
        target.iter().position(|n| n == name).map(|p| p as i32)
    }
    /// Block entity type id in the target version, or `None` if the target lacks it.
    pub fn block_entity(&self, id: i32) -> Option<i32> {
        let (names, target): (&[&str], &[&str]) = if self.dir == Dir::ToClient {
            (&d::NEW_BLOCK_ENTITY_NAMES, &d::OLD_BLOCK_ENTITY_NAMES)
        } else {
            (&d::OLD_BLOCK_ENTITY_NAMES, &d::NEW_BLOCK_ENTITY_NAMES)
        };
        let name = names.get(usize::try_from(id).ok()?)?;
        target.iter().position(|n| n == name).map(|p| p as i32)
    }
    /// Sound name in the target version for a source sound name (used for registry NBT).
    pub fn sound_name(&self, name: &str) -> Option<String> {
        let key = name.strip_prefix("minecraft:").unwrap_or(name);
        let (names, table, target): (&[&str], &[u32], &[&str]) = if self.dir == Dir::ToClient {
            (&d::NEW_SOUND_NAMES, &d::SOUNDS, &d::OLD_SOUND_NAMES)
        } else {
            (&d::OLD_SOUND_NAMES, &d::SOUNDS_REVERSE, &d::NEW_SOUND_NAMES)
        };
        let idx = names.iter().position(|n| *n == key)?;
        let mapped = *table.get(idx)? as usize;
        target.get(mapped).map(|n| format!("minecraft:{n}"))
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

    /// Full item stack: count, then id + components when count > 0.
    pub fn item_stack(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let count = Self::copy_varint(r, w)?;
        if count <= 0 {
            return Ok(());
        }
        let id = r.varint()?;
        w.varint(self.item(id));
        self.components(r, w)
    }
    /// Item template (26.1+): id, count, components. Used in recipes, containers, advancements.
    pub fn item_template(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let id = r.varint()?;
        w.varint(self.item(id));
        Self::copy_varint(r, w)?;
        self.components(r, w)
    }
    pub fn opt_item_template(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        if Self::copy_bool(r, w)? {
            self.item_template(r, w)?;
        }
        Ok(())
    }
    /// Item cost (merchant offers): id, count, then only "added" components (varint n + comps).
    pub fn item_cost(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let id = r.varint()?;
        w.varint(self.item(id));
        Self::copy_varint(r, w)?;
        let n = r.varint()?;
        let mut body = Writer::new();
        let mut kept = 0;
        for _ in 0..n {
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
        for _ in 0..added {
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
        for _ in 0..removed {
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
        let keep = self.component_value(name, r, &mut value)?;
        let Some(id) = self.component_type(ty) else { return Ok(false) };
        if !keep {
            return Ok(false);
        }
        w.varint(id);
        w.raw(&value.buf);
        Ok(true)
    }

    /// Parses one component payload named `name`, writing the target form. Returns `false` when
    /// the component must be dropped.
    fn component_value(&self, name: &str, r: &mut Reader, w: &mut Writer) -> Result<bool> {
        macro_rules! varint { () => {{ Self::copy_varint(r, w)?; }}; }
        macro_rules! f32 { () => {{ Self::copy_bytes(r, w, 4)?; }}; }
        macro_rules! bool { () => {{ Self::copy_bool(r, w)?; }}; }
        macro_rules! string { () => {{ Self::copy_string(r, w)?; }}; }
        macro_rules! nbt { () => {{ Self::copy_nbt(r, w)?; }}; }
        match name {
            "max_stack_size" | "max_damage" | "damage" | "repair_cost" | "map_id" | "map_post_processing" | "ominous_bottle_amplifier"
            | "additional_trade_cost" | "enchantable" | "rarity" | "dye" | "base_color" | "villager/variant" | "wolf/variant"
            | "wolf/sound_variant" | "wolf/collar" | "fox/variant" | "salmon/size" | "parrot/variant" | "tropical_fish/pattern"
            | "tropical_fish/base_color" | "tropical_fish/pattern_color" | "mooshroom/variant" | "rabbit/variant" | "pig/variant"
            | "pig/sound_variant" | "cow/variant" | "cow/sound_variant" | "chicken/variant" | "chicken/sound_variant"
            | "zombie_nautilus/variant" | "frog/variant" | "horse/variant" | "llama/variant" | "axolotl/variant" | "cat/variant"
            | "cat/sound_variant" | "cat/collar" | "sheep/color" | "shulker/color" | "damage_type" => {
                varint!();
            }
            "potion_duration_scale" | "minimum_attack_charge" => {
                f32!();
            }
            "unbreakable" | "glider" | "creative_slot_lock" => {}
            "enchantment_glint_override" => {
                bool!();
            }
            "custom_data" | "custom_name" | "item_name" | "map_decorations" | "debug_stick_state" | "bucket_entity_data" | "recipes"
            | "container_loot" | "intangible_projectile" | "lock" => {
                nbt!();
            }
            "lore" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    nbt!();
                }
            }
            "item_model" | "tooltip_style" | "note_block_sound" => {
                string!();
            }
            "use_effects" => {
                bool!();
                bool!();
                f32!();
            }
            "enchantments" | "stored_enchantments" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    varint!();
                    varint!();
                }
            }
            "can_place_on" | "can_break" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    self.block_predicate(r, w)?;
                }
            }
            "attribute_modifiers" => {
                // Entries for attributes the old client lacks are dropped.
                let n = r.varint()?;
                let mut body = Writer::new();
                let mut kept = 0;
                for _ in 0..n {
                    let attr = r.varint()?;
                    let mut entry = Writer::new();
                    entry.string(r.string()?); // id
                    entry.raw(r.bytes(8)?); // amount (f64)
                    entry.varint(r.varint()?); // operation
                    entry.varint(r.varint()?); // slot
                    let display = r.varint()?;
                    entry.varint(display);
                    if display == 2 {
                        Self::copy_nbt(r, &mut entry)?; // override text
                    }
                    if let Some(mapped) = self.attribute(attr) {
                        body.varint(mapped);
                        body.raw(&entry.buf);
                        kept += 1;
                    }
                }
                w.varint(kept);
                w.raw(&body.buf);
            }
            "custom_model_data" => {
                let n = Self::copy_varint(r, w)?;
                Self::copy_bytes(r, w, n.max(0) as usize * 4)?;
                let n = Self::copy_varint(r, w)?;
                Self::copy_bytes(r, w, n.max(0) as usize)?;
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    string!();
                }
                let n = Self::copy_varint(r, w)?;
                Self::copy_bytes(r, w, n.max(0) as usize * 4)?;
            }
            "tooltip_display" => {
                bool!();
                let n = r.varint()?;
                let mut ids = Vec::new();
                for _ in 0..n {
                    let ty = r.varint()?;
                    if let Some(t) = self.component_type(ty) {
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
            "death_protection" => {
                self.consume_effects(r, w)?;
            }
            "use_remainder" => {
                self.item_template(r, w)?;
            }
            "sulfur_cube_content" => {
                // 26.2 only: consume and drop.
                self.item_template(r, w)?;
                return Ok(false);
            }
            "use_cooldown" => {
                f32!();
                Self::copy_opt_string(r, w)?;
            }
            "damage_resistant" => {
                self.holder_set(r, w, Reg::Same)?;
            }
            "tool" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
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
            "attack_range" => {
                Self::copy_bytes(r, w, 24)?;
            }
            "equippable" => {
                varint!(); // slot
                self.sound_event(r, w)?;
                Self::copy_opt_string(r, w)?; // model
                Self::copy_opt_string(r, w)?; // camera overlay
                self.opt_holder_set(r, w, Reg::Entity)?;
                for _ in 0..5 {
                    bool!();
                }
                self.sound_event(r, w)?; // shearing sound
            }
            "repairable" => {
                self.holder_set(r, w, Reg::Item)?;
            }
            "blocks_attacks" => {
                f32!();
                f32!();
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    f32!();
                    self.opt_holder_set(r, w, Reg::Same)?;
                    f32!();
                    f32!();
                }
                Self::copy_bytes(r, w, 12)?; // item damage function
                self.opt_holder_set(r, w, Reg::Same)?; // bypassed by
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
            "dyed_color" | "map_color" => {
                Self::copy_bytes(r, w, 4)?;
            }
            "charged_projectiles" | "bundle_contents" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    self.item_template(r, w)?;
                }
            }
            "container" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    self.opt_item_template(r, w)?;
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
                for _ in 0..n {
                    varint!();
                    varint!();
                }
            }
            "writable_book_content" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
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
                for _ in 0..n {
                    nbt!();
                    if Self::copy_bool(r, w)? {
                        nbt!();
                    }
                }
                bool!();
            }
            "trim" => {
                self.trim_material(r, w)?;
                // pattern holder: direct = asset string, nbt description, bool decal
                self.holder(r, w, Reg::Same, |_, r, w| {
                    Self::copy_string(r, w)?;
                    Self::copy_nbt(r, w)?;
                    Self::copy_bool(r, w)?;
                    Ok(())
                })?;
            }
            "provides_trim_material" => {
                self.trim_material(r, w)?;
            }
            "entity_data" | "block_entity_data" => {
                let ty = r.varint()?;
                w.varint(if name == "entity_data" { self.entity(ty) } else { self.block_entity(ty).unwrap_or(ty) });
                nbt!();
            }
            "instrument" => {
                self.holder(r, w, Reg::Same, |s, r, w| {
                    s.sound_event(r, w)?;
                    Self::copy_bytes(r, w, 8)?; // use duration, range
                    Self::copy_nbt(r, w)
                })?;
            }
            "jukebox_playable" => {
                self.holder(r, w, Reg::Same, |s, r, w| {
                    s.sound_event(r, w)?;
                    Self::copy_nbt(r, w)?; // description
                    Self::copy_bytes(r, w, 4)?; // length
                    Self::copy_varint(r, w)?; // comparator output
                    Ok(())
                })?;
            }
            "provides_banner_patterns" => {
                self.holder_set(r, w, Reg::Same)?;
            }
            "lodestone_tracker" => {
                if Self::copy_bool(r, w)? {
                    string!();
                    Self::copy_bytes(r, w, 8)?;
                }
                bool!();
            }
            "firework_explosion" => {
                self.firework_explosion(r, w)?;
            }
            "fireworks" => {
                varint!();
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    self.firework_explosion(r, w)?;
                }
            }
            "profile" => {
                if Self::copy_bool(r, w)? {
                    Self::copy_bytes(r, w, 16)?;
                    string!();
                    self.profile_properties(r, w)?;
                } else {
                    Self::copy_opt_string(r, w)?;
                    if Self::copy_bool(r, w)? {
                        Self::copy_bytes(r, w, 16)?;
                    }
                    self.profile_properties(r, w)?;
                }
                Self::copy_opt_string(r, w)?;
                Self::copy_opt_string(r, w)?;
                Self::copy_opt_string(r, w)?;
                if Self::copy_bool(r, w)? {
                    bool!();
                }
            }
            "banner_patterns" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    self.holder(r, w, Reg::Same, |_, r, w| {
                        Self::copy_string(r, w)?;
                        Self::copy_string(r, w)
                    })?;
                    varint!();
                }
            }
            "pot_decorations" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    w.varint(self.item(r.varint()?));
                }
            }
            "block_state" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    string!();
                    string!();
                }
            }
            "bees" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    let ty = r.varint()?;
                    w.varint(self.entity(ty));
                    nbt!();
                    varint!();
                    varint!();
                }
            }
            "break_sound" => {
                self.sound_event(r, w)?;
            }
            "painting/variant" => {
                self.holder(r, w, Reg::Same, |_, r, w| {
                    Self::copy_varint(r, w)?;
                    Self::copy_varint(r, w)?;
                    Self::copy_string(r, w)?;
                    Self::copy_opt_nbt(r, w)?;
                    Self::copy_opt_nbt(r, w)
                })?;
            }
            other => return Err(err(&format!("no codec for data component {other}"))),
        }
        Ok(true)
    }

    fn trim_material(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        self.holder(r, w, Reg::Same, |_, r, w| {
            Self::copy_string(r, w)?; // asset name
            let overrides = Self::copy_varint(r, w)?;
            for _ in 0..overrides {
                Self::copy_string(r, w)?;
                Self::copy_string(r, w)?;
            }
            Self::copy_nbt(r, w) // description
        })
    }

    fn consume_effects(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let n = Self::copy_varint(r, w)?;
        for _ in 0..n {
            let ty = Self::copy_varint(r, w)?;
            match ty {
                0 => {
                    self.potion_effects(r, w)?;
                    Self::copy_bytes(r, w, 4)?;
                }
                1 => self.holder_set(r, w, Reg::Same)?,
                2 => {}
                3 => Self::copy_bytes(r, w, 4)?,
                4 => self.sound_event(r, w)?,
                _ => return Err(err("unknown consume effect")),
            }
        }
        Ok(())
    }

    fn potion_effects(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let n = Self::copy_varint(r, w)?;
        for _ in 0..n {
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
        for _ in 0..n {
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
            for _ in 0..n {
                Self::copy_string(r, w)?;
                if Self::copy_bool(r, w)? {
                    Self::copy_string(r, w)?;
                } else {
                    Self::copy_opt_string(r, w)?;
                    Self::copy_opt_string(r, w)?;
                }
            }
        }
        Self::copy_opt_nbt(r, w)?; // optional nbt
        // Data component matchers: exact components, then partial predicates.
        let n = r.varint()?;
        let mut body = Writer::new();
        let mut kept = 0;
        for _ in 0..n {
            if self.component(r, &mut body)? {
                kept += 1;
            }
        }
        w.varint(kept);
        w.raw(&body.buf);
        let n = r.varint()?;
        let mut body = Writer::new();
        let mut kept = 0;
        for _ in 0..n {
            let is_predicate_type = r.bool()?;
            let id = r.varint()?;
            let start = r.position();
            nbt::skip(r)?;
            let end = r.position();
            let mapped = if is_predicate_type { Some(id) } else { self.component_type(id) };
            if let Some(m) = mapped {
                body.bool(is_predicate_type);
                body.varint(m);
                r.set_position(start);
                body.raw(r.bytes(end - start)?);
                kept += 1;
            }
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
    fn every_26_2_component_has_a_codec_arm() {
        // Each component with an empty payload must either parse or fail on data, never on an
        // unknown name: feed every name a zero-filled payload and reject "no codec" errors.
        let items = Items::new(Dir::ToClient);
        for name in d::NEW_COMPONENT_NAMES.iter() {
            let zeros = [0u8; 64];
            let mut w = Writer::new();
            if let Err(DecodeError::Other(msg)) = items.component_value(name, &mut Reader::new(&zeros), &mut w) {
                assert!(!msg.starts_with("no codec"), "{name}: {msg}");
            }
        }
    }

    #[test]
    fn plain_stack_remaps_item_id_and_keeps_components() {
        let items = Items::new(Dir::ToClient);
        let sulfur = comp(&d::NEW_ITEM_NAMES, "sulfur");
        let mut w = Writer::new();
        w.varint(3); // count
        w.varint(sulfur);
        w.varint(1); // one added component: max_stack_size = 64
        w.varint(0);
        w.varint(comp(&d::NEW_COMPONENT_NAMES, "max_stack_size"));
        w.varint(64);
        let mut out = Writer::new();
        items.item_stack(&mut Reader::new(&w.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        assert_eq!(r.varint().unwrap(), 3);
        let mapped = r.varint().unwrap();
        assert_eq!(d::OLD_ITEM_NAMES[mapped as usize], "sandstone"); // material fallback
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), comp(&d::OLD_COMPONENT_NAMES, "max_stack_size"));
        assert_eq!(r.varint().unwrap(), 64);
        assert!(r.is_empty());
    }

    #[test]
    fn sulfur_cube_content_is_dropped() {
        let items = Items::new(Dir::ToClient);
        let mut w = Writer::new();
        w.varint(1);
        w.varint(1);
        w.varint(2); // sulfur_cube_content + unbreakable
        w.varint(1); // one removed: sulfur_cube_content
        w.varint(comp(&d::NEW_COMPONENT_NAMES, "sulfur_cube_content"));
        w.varint(5); // template: item 5, count 1, no components
        w.varint(1);
        w.varint(0);
        w.varint(0);
        w.varint(comp(&d::NEW_COMPONENT_NAMES, "unbreakable"));
        w.varint(comp(&d::NEW_COMPONENT_NAMES, "sulfur_cube_content"));
        let mut out = Writer::new();
        items.item_stack(&mut Reader::new(&w.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), 1); // only unbreakable kept
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), comp(&d::OLD_COMPONENT_NAMES, "unbreakable"));
        assert!(r.is_empty());
    }

    #[test]
    fn unknown_attribute_modifiers_are_dropped() {
        let items = Items::new(Dir::ToClient);
        let bounciness = comp(&d::NEW_ATTRIBUTE_NAMES, "bounciness");
        let armor = comp(&d::NEW_ATTRIBUTE_NAMES, "armor");
        let mut w = Writer::new();
        w.varint(2);
        for attr in [bounciness, armor] {
            w.varint(attr);
            w.string("minecraft:x");
            w.f64(1.5);
            w.varint(0);
            w.varint(0);
            w.varint(0);
        }
        let mut out = Writer::new();
        assert!(items.component_value("attribute_modifiers", &mut Reader::new(&w.buf), &mut out).unwrap());
        let mut r = Reader::new(&out.buf);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), comp(&d::OLD_ATTRIBUTE_NAMES, "armor"));
        assert_eq!(items.attribute(bounciness), None);
    }

    #[test]
    fn sound_names_follow_the_diff() {
        let items = Items::new(Dir::ToClient);
        assert_eq!(items.sound_name("minecraft:music_disc.bounce").as_deref(), Some("minecraft:music_disc.wait"));
        assert_eq!(items.sound_name("minecraft:entity.slime.jump").as_deref(), Some("minecraft:entity.slime.jump"));
        assert_eq!(items.sound_name("custom:thing"), None);
    }
}
