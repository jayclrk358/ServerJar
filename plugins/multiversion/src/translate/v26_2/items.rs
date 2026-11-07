//! Item stacks and their data components, 26.3 ↔ 26.2.
//!
//! Every component is parsed with its exact wire format (vanilla codecs as documented by
//! ViaVersion) so the stream stays in sync; registry ids inside are remapped, and components the
//! other side does not know are dropped or converted.

use std::cell::Cell;

use super::data_26_2 as d;
use crate::proto::buf::{DecodeError, Reader, Result, Writer};
use crate::proto::nbt;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    /// 26.3 server → 26.2 client
    ToClient,
    /// 26.2 client → 26.3 server
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
    /// Extra component entries produced while converting (swing → attack + interact).
    extra_added: Cell<i32>,
}

enum Kept {
    Drop,
    As(&'static str),
    Both(&'static str, &'static str),
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
            Reg::Attribute => self.attribute(id),
            Reg::Same => id,
        }
    }
    fn brick_target(&self) -> i32 {
        (if self.dir == Dir::ToClient { d::BRICK_ITEM_OLD } else { d::BRICK_ITEM_NEW }) as i32
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
    /// Copies one network NBT value (also used for "optional" tags: End tag = none).
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
    /// Creative-slot stack (1.21.5+): like `item_stack`, but every added component value carries a
    /// varint byte length.
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
            let mut inner = Reader::new(r.bytes(len.max(0) as usize)?);
            let name = self.source_component_name(ty).ok_or_else(|| err(&format!("unknown data component type {ty}")))?;
            let mut value = Writer::new();
            let targets: Vec<&'static str> = match self.component_value(name, &mut inner, &mut value)? {
                Kept::Drop => Vec::new(),
                Kept::As(t) => vec![t],
                Kept::Both(a, b) => vec![a, b],
            };
            for t in targets {
                if let Some(id) = self.target_component_id(t) {
                    body.varint(id);
                    body.varint(value.buf.len() as i32);
                    body.raw(&value.buf);
                    kept_added += 1;
                }
            }
        }
        // The removed list has the same shape in both encodings.
        let mut tail = Writer::new();
        tail.varint(0);
        tail.varint(removed);
        let header = tail.buf.len();
        tail.raw(r.remaining());
        let mut tr = Reader::new(&tail.buf);
        let mut fixed = Writer::new();
        self.components(&mut tr, &mut fixed)?;
        r.bytes(tr.position() - header)?;
        let mut fr = Reader::new(&fixed.buf);
        fr.varint()?;
        let kept_removed = fr.varint()?;
        w.varint(kept_added);
        w.varint(kept_removed);
        w.raw(&body.buf);
        w.raw(fr.remaining());
        Ok(())
    }
    /// Item template (26.1+): id, count, components. Used in recipes, pot decorations, containers.
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
        kept += self.extra_added.replace(0);
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
        kept_added += self.extra_added.replace(0);
        let mut removed_body = Writer::new();
        let mut kept_removed = 0;
        for _ in 0..removed {
            let ty = r.varint()?;
            match self.source_component_name(ty) {
                Some("swing_animation") if self.dir == Dir::ToServer => {
                    for n in ["attack_animation", "interact_animation"] {
                        if let Some(t) = self.target_component_id(n) {
                            removed_body.varint(t);
                            kept_removed += 1;
                        }
                    }
                }
                Some("attack_animation") if self.dir == Dir::ToClient => {
                    if let Some(t) = self.target_component_id("swing_animation") {
                        removed_body.varint(t);
                        kept_removed += 1;
                    }
                }
                _ => {
                    if let Some(t) = self.component_type(ty) {
                        removed_body.varint(t);
                        kept_removed += 1;
                    }
                }
            }
        }
        w.varint(kept_added);
        w.varint(kept_removed);
        w.raw(&body.buf);
        w.raw(&removed_body.buf);
        Ok(())
    }

    /// Reads one component (type id + value) and writes the converted one(s). Returns whether
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
            Kept::Both(a, b) => {
                let mut n = 0;
                for target in [a, b] {
                    if let Some(id) = self.target_component_id(target) {
                        w.varint(id);
                        w.raw(&value.buf);
                        n += 1;
                    }
                }
                if n > 1 {
                    self.extra_added.set(self.extra_added.get() + n - 1);
                }
                Ok(n > 0)
            }
        }
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
            | "pig/sound_variant" | "cow/variant" | "cow/sound_variant" | "chicken/variant" | "chicken/sound_variant"
            | "zombie_nautilus/variant" | "frog/variant" | "horse/variant" | "llama/variant" | "axolotl/variant" | "cat/variant"
            | "cat/sound_variant" | "cat/collar" | "sheep/color" | "shulker/color" | "damage_type" => {
                varint!();
                Ok(keep)
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
                    let ty = r.varint()?;
                    if let Some(t) = self.component_type(ty) {
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
            "use_remainder" | "sulfur_cube_content" => {
                self.item_template(r, w)?;
                Ok(keep)
            }
            "use_cooldown" => {
                f32!();
                Self::copy_opt_string(r, w)?;
                Ok(keep)
            }
            "damage_resistant" => {
                self.holder_set(r, w, Reg::Same)?;
                Ok(keep)
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
                self.opt_holder_set(r, w, Reg::Same)?; // bypassed by
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
            // 26.3 split swing_animation into attack + interact; 26.2 has only swing_animation.
            "attack_animation" => {
                varint!();
                varint!();
                Ok(Kept::As("swing_animation"))
            }
            "interact_animation" => {
                varint!();
                varint!();
                Ok(Kept::Drop)
            }
            "swing_animation" => {
                varint!();
                varint!();
                Ok(Kept::Both("attack_animation", "interact_animation"))
            }
            "dyed_color" => {
                Self::copy_bytes(r, w, 4)?;
                Ok(keep)
            }
            "map_color" => {
                Self::copy_bytes(r, w, 4)?;
                Ok(Kept::Drop) // 26.3 removed it
            }
            "charged_projectiles" | "bundle_contents" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    self.item_template(r, w)?;
                }
                Ok(keep)
            }
            "container" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    self.opt_item_template(r, w)?;
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
                self.trim_material(r, w)?;
                // pattern holder: direct = asset string, nbt description, bool decal
                self.holder(r, w, Reg::Same, |_, r, w| {
                    Self::copy_string(r, w)?;
                    Self::copy_nbt(r, w)?;
                    Self::copy_bool(r, w)?;
                    Ok(())
                })?;
                Ok(keep)
            }
            "provides_trim_material" => {
                self.trim_material(r, w)?;
                Ok(keep)
            }
            "entity_data" | "block_entity_data" => {
                let ty = r.varint()?;
                w.varint(if name == "entity_data" { self.entity(ty) } else { ty });
                nbt!();
                Ok(keep)
            }
            "instrument" => {
                self.holder(r, w, Reg::Same, |s, r, w| {
                    s.sound_event(r, w)?;
                    Self::copy_bytes(r, w, 8)?; // use duration, range
                    if s.dir == Dir::ToClient {
                        r.varint()?; // durability damage: 26.3 only
                    } else {
                        w.varint(0);
                    }
                    Self::copy_nbt(r, w)
                })?;
                Ok(keep)
            }
            "jukebox_playable" => {
                self.holder(r, w, Reg::Same, |s, r, w| {
                    s.sound_event(r, w)?;
                    Self::copy_nbt(r, w)?; // description
                    Self::copy_bytes(r, w, 4)?; // length
                    Self::copy_varint(r, w)?; // comparator output
                    Ok(())
                })?;
                Ok(keep)
            }
            "provides_banner_patterns" => {
                self.holder_set(r, w, Reg::Same)?;
                Ok(keep)
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
                if self.dir == Dir::ToClient {
                    // 26.3: four optional item templates → 26.2: varint array of item ids
                    let mut ids = Vec::with_capacity(4);
                    for _ in 0..4 {
                        if r.bool()? {
                            let id = r.varint()?;
                            r.varint()?; // count
                            let mut scratch = Writer::new();
                            self.components(r, &mut scratch)?;
                            self.extra_added.set(0);
                            ids.push(self.item(id));
                        } else {
                            ids.push(self.brick_target());
                        }
                    }
                    w.varint(4);
                    for id in ids {
                        w.varint(id);
                    }
                } else {
                    let n = r.varint()?;
                    let mut ids: Vec<i32> = (0..n).map(|_| r.varint()).collect::<Result<_>>()?;
                    ids.resize(4, self.brick_target());
                    for id in ids.iter().take(4) {
                        w.bool(true);
                        w.varint(self.item(*id));
                        w.varint(1);
                        w.varint(0);
                        w.varint(0);
                    }
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
            // 26.3-only data the 26.2 client can't use: consume and drop.
            "compostable" => {
                self.resolvable(r, w, 4)?;
                Ok(Kept::Drop)
            }
            "provides_pottery_pattern" | "block_transformer" | "villager_food" | "cushion/color" => {
                r.varint()?;
                Ok(Kept::Drop)
            }
            "cooking_fuel" | "brewing_fuel" => {
                self.resolvable(r, w, 4)?;
                self.resolvable(r, w, 4)?;
                Ok(Kept::Drop)
            }
            "mob_visibility" => {
                self.holder_set(r, w, Reg::Entity)?;
                f32!();
                Ok(Kept::Drop)
            }
            "sign_text_front" | "sign_text_back" => {
                for _ in 0..4 {
                    nbt!();
                }
                if Self::copy_bool(r, w)? {
                    for _ in 0..4 {
                        nbt!();
                    }
                }
                varint!();
                bool!();
                Ok(Kept::Drop)
            }
            "waxed" => Ok(Kept::Drop),
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

    fn resolvable(&self, r: &mut Reader, w: &mut Writer, size: usize) -> Result<()> {
        if Self::copy_bool(r, w)? {
            Self::copy_bytes(r, w, size)
        } else {
            Self::copy_string(r, w)
        }
    }

    fn trim_material(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        self.holder(r, w, Reg::Same, |s, r, w| {
            if s.dir == Dir::ToClient {
                // 26.3: palette id + description → 26.2: asset name, overrides (none), description
                let palette = r.string()?;
                w.string(palette.strip_prefix("trim/").unwrap_or(palette));
                w.varint(0);
                Self::copy_nbt(r, w)
            } else {
                let asset = r.string()?.to_string();
                let overrides = r.varint()?;
                for _ in 0..overrides {
                    r.string()?;
                    r.string()?;
                }
                w.string(&format!("trim/{asset}"));
                Self::copy_nbt(r, w)
            }
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
                3 => {
                    Self::copy_bytes(r, w, 4)?;
                    if self.dir == Dir::ToClient {
                        r.bool()?; // directional particles: 26.3 only
                    } else {
                        w.bool(false);
                    }
                }
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
        kept += self.extra_added.replace(0);
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

    #[test]
    fn plain_stack_roundtrips_and_remaps_id() {
        let items = Items::new(Dir::ToClient);
        let mut w = Writer::new();
        w.varint(3); // count
        w.varint(d::ITEMS.len() as i32 - 1); // last 26.3 item
        w.varint(1); // one added component: max_stack_size = 64
        w.varint(0);
        w.varint(d::NEW_COMPONENT_NAMES.iter().position(|n| *n == "max_stack_size").unwrap() as i32);
        w.varint(64);
        let mut out = Writer::new();
        items.item_stack(&mut Reader::new(&w.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        assert_eq!(r.varint().unwrap(), 3);
        assert_eq!(r.varint().unwrap(), d::ITEMS[d::ITEMS.len() - 1] as i32);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), d::OLD_COMPONENT_NAMES.iter().position(|n| *n == "max_stack_size").unwrap() as i32);
        assert_eq!(r.varint().unwrap(), 64);
        assert!(r.is_empty());
    }

    #[test]
    fn new_only_component_is_dropped() {
        let items = Items::new(Dir::ToClient);
        let mut w = Writer::new();
        w.varint(1);
        w.varint(1);
        w.varint(1);
        w.varint(0);
        w.varint(d::NEW_COMPONENT_NAMES.iter().position(|n| *n == "waxed").unwrap() as i32);
        let mut out = Writer::new();
        items.item_stack(&mut Reader::new(&w.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), 0); // no components kept
        assert_eq!(r.varint().unwrap(), 0);
        assert!(r.is_empty());
    }

    #[test]
    fn swing_animation_splits_when_going_to_server() {
        let items = Items::new(Dir::ToServer);
        let mut w = Writer::new();
        w.varint(1);
        w.varint(1);
        w.varint(1);
        w.varint(0);
        w.varint(d::OLD_COMPONENT_NAMES.iter().position(|n| *n == "swing_animation").unwrap() as i32);
        w.varint(1);
        w.varint(6);
        let mut out = Writer::new();
        items.item_stack(&mut Reader::new(&w.buf), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        r.varint().unwrap();
        r.varint().unwrap();
        assert_eq!(r.varint().unwrap(), 2); // attack + interact
        assert_eq!(r.varint().unwrap(), 0);
    }
}
