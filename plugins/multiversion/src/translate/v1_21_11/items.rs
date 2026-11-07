//! Item stacks and their data components, 26.1 ↔ 1.21.11.
//!
//! 26.1 made several registry references plain holders (1.21.11 used "either id or key"),
//! turned some tag keys into holder sets and introduced item templates (id, count, components)
//! where 1.21.11 still uses full item stacks. Everything is parsed with its exact wire format so
//! the stream stays in sync; components the target lacks are dropped.

use super::super::data_1_21_11 as d;
use crate::proto::buf::{DecodeError, Reader, Result, Writer};
use crate::proto::nbt;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    /// 26.1 server → 1.21.11 client
    ToClient,
    /// 1.21.11 client → 26.1 server
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

enum Kept {
    Drop,
    As(&'static str),
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
    pub fn attribute(&self, id: i32) -> i32 {
        Self::lookup(&d::ATTRIBUTES, &d::ATTRIBUTES_REVERSE, self.dir, id)
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
    fn target_component_id(&self, name: &str) -> Option<i32> {
        let names: &[&str] = if self.dir == Dir::ToClient { &d::OLD_COMPONENT_NAMES } else { &d::NEW_COMPONENT_NAMES };
        names.iter().position(|n| *n == name).map(|i| i as i32)
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
    /// Reads a holder set and returns its tag key, or `None` when it lists ids.
    fn holder_set_tag(r: &mut Reader) -> Result<Option<String>> {
        let v = r.varint()?;
        if v == 0 {
            return Ok(Some(r.string()?.to_string()));
        }
        for _ in 0..(v - 1) {
            r.varint()?;
        }
        Ok(None)
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

    /// 26.1 `Holder<T>` ↔ 1.21.11 `EitherHolder<T>` (bool: true = holder, false = resource key).
    /// Returns false when a key-only value can't be resolved for the 26.1 server (drop it).
    fn either_holder(&self, r: &mut Reader, w: &mut Writer, direct: impl FnOnce(&Self, &mut Reader, &mut Writer) -> Result<()>) -> Result<bool> {
        if self.dir == Dir::ToClient {
            w.bool(true);
            self.holder(r, w, Reg::Same, direct)?;
            Ok(true)
        } else if r.bool()? {
            self.holder(r, w, Reg::Same, direct)?;
            Ok(true)
        } else {
            r.string()?;
            Ok(false)
        }
    }

    /// 26.1 synchronized registry id (varint) ↔ 1.21.11 `Either<varint, key>`.
    fn either_id(&self, r: &mut Reader, w: &mut Writer) -> Result<bool> {
        if self.dir == Dir::ToClient {
            w.bool(true);
            Self::copy_varint(r, w)?;
            Ok(true)
        } else if r.bool()? {
            Self::copy_varint(r, w)?;
            Ok(true)
        } else {
            r.string()?;
            Ok(false)
        }
    }

    /// 26.1 holder set ↔ 1.21.11 tag key. Id lists have no 1.21.11 form (drop).
    fn tag_or_holder_set(&self, r: &mut Reader, w: &mut Writer) -> Result<bool> {
        if self.dir == Dir::ToClient {
            match Self::holder_set_tag(r)? {
                Some(tag) => {
                    w.string(&tag);
                    Ok(true)
                }
                None => Ok(false),
            }
        } else {
            w.varint(0);
            Self::copy_string(r, w)?;
            Ok(true)
        }
    }

    // ---- item stacks -----------------------------------------------------------------------

    /// Full item stack: count, then id + components when count > 0. Same layout in both.
    pub fn item_stack(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let count = Self::copy_varint(r, w)?;
        if count <= 0 {
            return Ok(());
        }
        let id = r.varint()?;
        w.varint(self.item(id));
        self.components(r, w)
    }

    /// 26.1 item template (id, count, components) ↔ 1.21.11 item stack (count, id, components).
    pub fn template(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        if self.dir == Dir::ToClient {
            let id = r.varint()?;
            let count = r.varint()?;
            w.varint(count.max(1));
            w.varint(self.item(id));
            self.components(r, w)
        } else {
            let count = r.varint()?;
            if count <= 0 {
                // Templates can't be empty; air keeps the stream valid.
                w.varint(0);
                w.varint(1);
                w.varint(0);
                w.varint(0);
                return Ok(());
            }
            let id = r.varint()?;
            w.varint(self.item(id));
            w.varint(count);
            self.components(r, w)
        }
    }

    /// 26.1 optional template (bool + template) ↔ 1.21.11 item stack (count 0 = empty).
    pub fn opt_template(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        if self.dir == Dir::ToClient {
            if r.bool()? {
                self.template(r, w)
            } else {
                w.varint(0);
                Ok(())
            }
        } else {
            let count = r.varint()?;
            if count <= 0 {
                w.bool(false);
                return Ok(());
            }
            w.bool(true);
            let id = r.varint()?;
            w.varint(self.item(id));
            w.varint(count);
            self.components(r, w)
        }
    }

    /// Item cost (merchant offers): id, count, then only "added" components. Same in both.
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

    /// Hashed stack (serverbound container clicks): only ids need remapping.
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

    /// Length-prefixed item stack (serverbound creative slot, 1.21.5+): every component value
    /// is preceded by its byte length.
    pub fn length_prefixed_stack(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let count = Self::copy_varint(r, w)?;
        if count <= 0 {
            return Ok(());
        }
        let id = r.varint()?;
        w.varint(self.item(id));
        let added = r.varint()?;
        let removed = r.varint()?;
        if added < 0 || removed < 0 || added + removed > 512 {
            return Err(err("bad component counts"));
        }
        let mut body = Writer::new();
        let mut kept_added = 0;
        for _ in 0..added {
            let ty = r.varint()?;
            let len = r.varint()?;
            let data = r.bytes(len.max(0) as usize)?;
            let Some(name) = self.source_component_name(ty) else { continue };
            let mut value = Writer::new();
            let mut sub = Reader::new(data);
            if let Kept::As(target) = self.component_value(name, &mut sub, &mut value)? {
                if let Some(t) = self.target_component_id(target) {
                    body.varint(t);
                    body.varint(value.buf.len() as i32);
                    body.raw(&value.buf);
                    kept_added += 1;
                }
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

    /// Reads one component (type id + value) and writes the converted one. Returns whether
    /// anything was written.
    pub fn component(&self, r: &mut Reader, w: &mut Writer) -> Result<bool> {
        let ty = r.varint()?;
        let name = self.source_component_name(ty).ok_or_else(|| err(&format!("unknown data component type {ty}")))?;
        let mut value = Writer::new();
        match self.component_value(name, r, &mut value)? {
            Kept::Drop => Ok(false),
            Kept::As(target_name) => {
                let Some(id) = self.target_component_id(target_name) else { return Ok(false) };
                w.varint(id);
                w.raw(&value.buf);
                Ok(true)
            }
        }
    }

    fn keep_if(name: &'static str, ok: bool) -> Kept {
        if ok { Kept::As(name) } else { Kept::Drop }
    }

    /// Parses one component payload named `name` (in the source version), writing the target form.
    fn component_value(&self, name: &'static str, r: &mut Reader, w: &mut Writer) -> Result<Kept> {
        let keep = Kept::As(name);
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
            | "pig/sound_variant" | "cow/variant" | "cow/sound_variant" | "chicken/sound_variant" | "frog/variant"
            | "horse/variant" | "llama/variant" | "axolotl/variant" | "cat/variant" | "cat/sound_variant" | "cat/collar"
            | "sheep/color" | "shulker/color" => {
                varint!();
                Ok(keep)
            }
            // 26.1 plain registry ids; 1.21.11 "either id or key".
            "chicken/variant" | "zombie_nautilus/variant" | "damage_type" => {
                let ok = self.either_id(r, w)?;
                Ok(Self::keep_if(name, ok))
            }
            "potion_duration_scale" | "minimum_attack_charge" => {
                f32!();
                Ok(keep)
            }
            "unbreakable" | "glider" | "creative_slot_lock" => Ok(keep),
            "enchantment_glint_override" => {
                bool!();
                Ok(keep)
            }
            "custom_data" | "custom_name" | "item_name" | "map_decorations" | "debug_stick_state" | "bucket_entity_data" | "recipes"
            | "container_loot" | "intangible_projectile" | "lock" => {
                nbt!();
                Ok(keep)
            }
            "lore" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    nbt!();
                }
                Ok(keep)
            }
            "item_model" | "tooltip_style" | "note_block_sound" => {
                string!();
                Ok(keep)
            }
            "use_effects" => {
                bool!();
                bool!();
                f32!();
                Ok(keep)
            }
            "enchantments" | "stored_enchantments" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    varint!();
                    varint!();
                }
                Ok(keep)
            }
            "can_place_on" | "can_break" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    self.block_predicate(r, w)?;
                }
                Ok(keep)
            }
            "attribute_modifiers" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    let attr = r.varint()?;
                    w.varint(self.attribute(attr));
                    string!(); // id
                    Self::copy_bytes(r, w, 8)?; // amount (f64)
                    varint!(); // operation
                    varint!(); // slot
                    let display = Self::copy_varint(r, w)?;
                    if display == 2 {
                        nbt!(); // override text
                    }
                }
                Ok(keep)
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
                Ok(keep)
            }
            "tooltip_display" => {
                bool!();
                let n = r.varint()?;
                let mut ids = Vec::new();
                for _ in 0..n {
                    if let Some(t) = self.component_type(r.varint()?) {
                        ids.push(t);
                    }
                }
                w.varint(ids.len() as i32);
                for id in ids {
                    w.varint(id);
                }
                Ok(keep)
            }
            "food" => {
                varint!();
                f32!();
                bool!();
                Ok(keep)
            }
            "consumable" => {
                f32!(); // seconds
                varint!(); // animation
                self.sound_event(r, w)?;
                bool!(); // particles
                self.consume_effects(r, w)?;
                Ok(keep)
            }
            "death_protection" => {
                self.consume_effects(r, w)?;
                Ok(keep)
            }
            "use_remainder" => {
                self.template(r, w)?;
                Ok(keep)
            }
            "use_cooldown" => {
                f32!();
                Self::copy_opt_string(r, w)?;
                Ok(keep)
            }
            "damage_resistant" | "provides_banner_patterns" => {
                let ok = self.tag_or_holder_set(r, w)?;
                Ok(Self::keep_if(name, ok))
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
                Ok(keep)
            }
            "weapon" => {
                varint!();
                f32!();
                Ok(keep)
            }
            "attack_range" => {
                Self::copy_bytes(r, w, 24)?;
                Ok(keep)
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
                Ok(keep)
            }
            "repairable" => {
                self.holder_set(r, w, Reg::Item)?;
                Ok(keep)
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
                // bypassed by: 26.1 optional holder set, 1.21.11 optional tag key
                if self.dir == Dir::ToClient {
                    if r.bool()? {
                        match Self::holder_set_tag(r)? {
                            Some(tag) => {
                                w.bool(true);
                                w.string(&tag);
                            }
                            None => {
                                w.bool(false);
                            }
                        }
                    } else {
                        w.bool(false);
                    }
                } else if r.bool()? {
                    w.bool(true);
                    w.varint(0);
                    string!();
                } else {
                    w.bool(false);
                }
                self.opt_sound_event(r, w)?;
                self.opt_sound_event(r, w)?;
                Ok(keep)
            }
            "piercing_weapon" => {
                bool!();
                bool!();
                self.opt_sound_event(r, w)?;
                self.opt_sound_event(r, w)?;
                Ok(keep)
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
                Ok(keep)
            }
            "swing_animation" => {
                varint!();
                varint!();
                Ok(keep)
            }
            "dyed_color" | "map_color" => {
                Self::copy_bytes(r, w, 4)?;
                Ok(keep)
            }
            "charged_projectiles" | "bundle_contents" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    self.template(r, w)?;
                }
                Ok(keep)
            }
            "container" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    self.opt_template(r, w)?;
                }
                Ok(keep)
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
                Ok(keep)
            }
            "suspicious_stew_effects" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    varint!();
                    varint!();
                }
                Ok(keep)
            }
            "writable_book_content" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    string!();
                    Self::copy_opt_string(r, w)?;
                }
                Ok(keep)
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
                Ok(keep)
            }
            "trim" => {
                self.holder(r, w, Reg::Same, |_, r, w| Self::trim_material_direct(r, w))?;
                // pattern: asset string, nbt description, bool decal
                self.holder(r, w, Reg::Same, |_, r, w| {
                    Self::copy_string(r, w)?;
                    Self::copy_nbt(r, w)?;
                    Self::copy_bool(r, w)?;
                    Ok(())
                })?;
                Ok(keep)
            }
            "provides_trim_material" => {
                let ok = self.either_holder(r, w, |_, r, w| Self::trim_material_direct(r, w))?;
                Ok(Self::keep_if(name, ok))
            }
            "entity_data" | "block_entity_data" => {
                let ty = r.varint()?;
                w.varint(if name == "entity_data" { self.entity(ty) } else { ty });
                nbt!();
                Ok(keep)
            }
            "instrument" => {
                let ok = self.either_holder(r, w, |s, r, w| {
                    s.sound_event(r, w)?;
                    Self::copy_bytes(r, w, 8)?; // use duration, range
                    Self::copy_nbt(r, w)
                })?;
                Ok(Self::keep_if(name, ok))
            }
            "jukebox_playable" => {
                let ok = self.either_holder(r, w, |s, r, w| {
                    s.sound_event(r, w)?;
                    Self::copy_nbt(r, w)?; // description
                    Self::copy_bytes(r, w, 4)?; // length
                    Self::copy_varint(r, w)?; // comparator output
                    Ok(())
                })?;
                Ok(Self::keep_if(name, ok))
            }
            "lodestone_tracker" => {
                if Self::copy_bool(r, w)? {
                    string!();
                    Self::copy_bytes(r, w, 8)?;
                }
                bool!();
                Ok(keep)
            }
            "firework_explosion" => {
                self.firework_explosion(r, w)?;
                Ok(keep)
            }
            "fireworks" => {
                varint!();
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    self.firework_explosion(r, w)?;
                }
                Ok(keep)
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
                Ok(keep)
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
                Ok(keep)
            }
            "pot_decorations" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    let id = r.varint()?;
                    w.varint(self.item(id));
                }
                Ok(keep)
            }
            "block_state" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    string!();
                    string!();
                }
                Ok(keep)
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
                Ok(keep)
            }
            "break_sound" => {
                self.sound_event(r, w)?;
                Ok(keep)
            }
            "painting/variant" => {
                self.holder(r, w, Reg::Same, |_, r, w| {
                    Self::copy_varint(r, w)?;
                    Self::copy_varint(r, w)?;
                    Self::copy_string(r, w)?;
                    Self::copy_opt_nbt(r, w)?;
                    Self::copy_opt_nbt(r, w)
                })?;
                Ok(keep)
            }
            other => Err(err(&format!("no codec for data component {other}"))),
        }
    }

    /// Trim material (1.21.5+ shape): asset name, override map, description.
    fn trim_material_direct(r: &mut Reader, w: &mut Writer) -> Result<()> {
        Self::copy_string(r, w)?;
        let n = Self::copy_varint(r, w)?;
        for _ in 0..n.max(0) {
            Self::copy_string(r, w)?;
            Self::copy_string(r, w)?;
        }
        Self::copy_nbt(r, w)
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
        Self::copy_opt_nbt(r, w)?;
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

    fn new_comp(name: &str) -> i32 {
        d::NEW_COMPONENT_NAMES.iter().position(|n| *n == name).unwrap() as i32
    }
    fn old_comp(name: &str) -> i32 {
        d::OLD_COMPONENT_NAMES.iter().position(|n| *n == name).unwrap() as i32
    }

    #[test]
    fn shifted_component_ids_are_remapped() {
        let items = Items::new(Dir::ToClient);
        let mut w = Writer::new();
        w.varint(1).varint(1).varint(1).varint(0);
        w.varint(new_comp("stored_enchantments")).varint(0);
        let mut out = Writer::new();
        items.item_stack(&mut Reader::new(&w.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        assert_eq!(r.varint().unwrap(), 1);
        r.varint().unwrap();
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), old_comp("stored_enchantments"));
        assert_eq!(r.varint().unwrap(), 0);
        assert!(r.is_empty());
    }

    #[test]
    fn new_only_components_are_dropped() {
        let items = Items::new(Dir::ToClient);
        let mut w = Writer::new();
        w.varint(1).varint(1).varint(2).varint(0);
        w.varint(new_comp("dye")).varint(3);
        w.varint(new_comp("cat/sound_variant")).varint(1);
        let mut out = Writer::new();
        items.item_stack(&mut Reader::new(&w.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        r.varint().unwrap();
        r.varint().unwrap();
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), 0);
        assert!(r.is_empty());
    }

    #[test]
    fn holder_becomes_either_holder() {
        let items = Items::new(Dir::ToClient);
        let mut w = Writer::new();
        w.varint(1).varint(1).varint(1).varint(0);
        w.varint(new_comp("jukebox_playable")).varint(4); // registry id 3
        let mut out = Writer::new();
        items.item_stack(&mut Reader::new(&w.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        r.varint().unwrap();
        r.varint().unwrap();
        r.varint().unwrap();
        r.varint().unwrap();
        assert_eq!(r.varint().unwrap(), old_comp("jukebox_playable"));
        assert!(r.bool().unwrap());
        assert_eq!(r.varint().unwrap(), 4);
        assert!(r.is_empty());
    }

    #[test]
    fn key_only_either_is_dropped_towards_server() {
        let items = Items::new(Dir::ToServer);
        let mut w = Writer::new();
        w.varint(1).varint(1).varint(1).varint(0);
        w.varint(old_comp("chicken/variant")).bool(false).string("minecraft:cold");
        let mut out = Writer::new();
        items.item_stack(&mut Reader::new(&w.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        r.varint().unwrap();
        r.varint().unwrap();
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), 0);
        assert!(r.is_empty());
    }

    #[test]
    fn damage_resistant_tag_becomes_key_and_ids_are_dropped() {
        let items = Items::new(Dir::ToClient);
        let mut w = Writer::new();
        w.varint(1).varint(1).varint(2).varint(0);
        w.varint(new_comp("damage_resistant")).varint(0).string("minecraft:is_fire");
        w.varint(new_comp("provides_banner_patterns")).varint(2).varint(5);
        let mut out = Writer::new();
        items.item_stack(&mut Reader::new(&w.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        r.varint().unwrap();
        r.varint().unwrap();
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), old_comp("damage_resistant"));
        assert_eq!(r.string().unwrap(), "minecraft:is_fire");
        assert!(r.is_empty());
    }

    #[test]
    fn container_templates_become_stacks_and_back() {
        let items = Items::new(Dir::ToClient);
        let mut w = Writer::new();
        w.varint(new_comp("container"));
        w.varint(2);
        w.bool(false);
        w.bool(true).varint(1).varint(5).varint(0).varint(0); // template: id 1 (stone), count 5
        let mut out = Writer::new();
        assert!(items.component(&mut Reader::new(&w.buf), &mut out).unwrap());
        let mut r = Reader::new(&out.buf);
        assert_eq!(r.varint().unwrap(), old_comp("container"));
        assert_eq!(r.varint().unwrap(), 2);
        assert_eq!(r.varint().unwrap(), 0); // empty stack
        assert_eq!(r.varint().unwrap(), 5); // count
        assert_eq!(r.varint().unwrap(), d::ITEMS[1] as i32);
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), 0);
        assert!(r.is_empty());

        // and the other way
        let back = Items::new(Dir::ToServer);
        let mut out2 = Writer::new();
        assert!(back.component(&mut Reader::new(&out.buf), &mut out2).unwrap());
        let mut r = Reader::new(&out2.buf);
        assert_eq!(r.varint().unwrap(), new_comp("container"));
        assert_eq!(r.varint().unwrap(), 2);
        assert!(!r.bool().unwrap());
        assert!(r.bool().unwrap());
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), 5);
    }

    #[test]
    fn length_prefixed_stack_is_rewritten() {
        let items = Items::new(Dir::ToServer);
        let mut w = Writer::new();
        w.varint(1).varint(1).varint(1).varint(0);
        w.varint(old_comp("stored_enchantments")).varint(1).varint(0);
        let mut out = Writer::new();
        items.length_prefixed_stack(&mut Reader::new(&w.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        r.varint().unwrap();
        r.varint().unwrap();
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), new_comp("stored_enchantments"));
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), 0);
        assert!(r.is_empty());
    }
}
