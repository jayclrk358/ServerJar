//! Item stacks and data components, 1.21.2 ↔ 1.21.
//!
//! Every component is parsed with its exact wire format (vanilla codecs as documented by
//! vanilla) so the stream stays in sync. Components are collected first and written after, because
//! some 1.21 components are built from several 1.21.2 ones (food = food + consumable + use_remainder).
//!
//! Components the 1.21 client cannot represent are kept, in their original 1.21.2 encoding, inside
//! `custom_data` under [`BACKUP_KEY`]; when the item comes back (creative inventory, container clicks)
//! the backup is restored so nothing is lost. Items that only exist in 1.21.2 are shown as their
//! closest 1.21 item with a name, and get their real id back on the way to the server.

use super::data_1_21 as d;
use crate::proto::buf::{DecodeError, Reader, Result, Writer};
use crate::proto::nbt::{self, Tag};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    /// 1.21.2 server → 1.21 client
    ToClient,
    /// 1.21 client → 1.21.2 server
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

/// custom_data key holding the 1.21.2 data a 1.21 client cannot keep.
pub const BACKUP_KEY: &str = "mineversed:v1_21_2";

/// 1.21.2 components with no 1.21 equivalent.
const DROPPED: &[&str] = &[
    "item_model",
    "consumable",
    "use_remainder",
    "use_cooldown",
    "damage_resistant",
    "enchantable",
    "equippable",
    "repairable",
    "glider",
    "tooltip_style",
    "death_protection",
];

fn err(msg: &str) -> DecodeError {
    DecodeError::Other(msg.to_string())
}

/// Bytes consumed since `start`.
fn raw_since(r: &mut Reader, start: usize) -> Result<Vec<u8>> {
    let end = r.position();
    r.set_position(start);
    Ok(r.bytes(end - start)?.to_vec())
}

fn strip_ns(s: &str) -> &str {
    s.strip_prefix("minecraft:").unwrap_or(s)
}

/// "pale_oak_planks" → "Pale Oak Planks".
pub fn pretty_name(id: &str) -> String {
    strip_ns(id)
        .split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Unformatted text of a JSON text component (strings, {"text"}, "extra").
pub fn json_plain_text(json: &str) -> String {
    fn walk(v: &serde_json::Value, out: &mut String) {
        match v {
            serde_json::Value::String(s) => out.push_str(s),
            serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            serde_json::Value::Object(o) => {
                if let Some(serde_json::Value::String(t)) = o.get("text") {
                    out.push_str(t);
                }
                if let Some(e) = o.get("extra") {
                    walk(e, out);
                }
            }
            _ => {}
        }
    }
    match serde_json::from_str::<serde_json::Value>(json) {
        Ok(v) => {
            let mut s = String::new();
            walk(&v, &mut s);
            s
        }
        Err(_) => json.to_string(),
    }
}

/// Converted item patch, collected before writing.
#[derive(Default)]
struct Ctx {
    /// (target component name, target encoding)
    out: Vec<(&'static str, Vec<u8>)>,
    /// target component names
    removed: Vec<&'static str>,
    // ToClient: pieces for the 1.21 food component
    food: Option<(i32, f32, bool)>,
    eat_seconds: Option<f32>,
    /// each entry: potion effect (target encoding) + f32 probability
    food_effects: Vec<Vec<u8>>,
    /// use_remainder item, target encoding
    remainder: Option<Vec<u8>>,
    // ToClient: what goes into the backup
    backup: Vec<(&'static str, Vec<u8>)>,
    backup_removed: Vec<&'static str>,
    // ToServer
    restore: Option<Restore>,
    glint_lost: bool,
}

#[derive(Default)]
struct Restore {
    id: Option<i32>,
    remove_custom_name: bool,
    comps: Vec<(&'static str, Vec<u8>)>,
    removed: Vec<&'static str>,
    custom_data_was_added: bool,
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
    /// A 1.21.2 sound the 1.21 client has no mapping for (it would play the fallback sound).
    pub fn sound_unknown(id: i32) -> bool {
        id > 0 && d::SOUNDS.get(id as usize) == Some(&0)
    }
    /// A 1.21.2 attribute the 1.21 client lacks (tempt_range).
    pub fn attribute_unknown(id: i32) -> bool {
        id > 0 && d::ATTRIBUTES.get(id as usize) == Some(&0)
    }
    /// A 1.21.2 block entity type the 1.21 client lacks (creaking_heart).
    pub fn block_entity_unknown(id: i32) -> bool {
        id > 0 && d::BLOCK_ENTITIES.get(id as usize) == Some(&0)
    }
    pub fn block_entity(id: i32) -> i32 {
        d::BLOCK_ENTITIES.get(id.max(0) as usize).map_or(id, |v| *v as i32)
    }
    /// A 1.21.2 block the 1.21 client lacks (it maps to stone).
    pub fn block_unknown(id: i32) -> bool {
        d::BLOCKS.get(id.max(0) as usize) == Some(&1) && d::NEW_BLOCK_NAMES.get(id.max(0) as usize) != Some(&"stone")
    }
    /// 1.21.2 item that only exists as a stand-in on the 1.21 client.
    fn substituted(id: i32) -> bool {
        let Some(&mapped) = d::ITEMS.get(id.max(0) as usize) else { return false };
        d::OLD_ITEM_NAMES.get(mapped as usize) != d::NEW_ITEM_NAMES.get(id.max(0) as usize)
    }
    /// Item id by registry name in the server's (1.21.2) registry.
    pub fn new_item_by_name(name: &str) -> Option<i32> {
        let n = strip_ns(name);
        d::NEW_ITEM_NAMES.iter().position(|x| *x == n).map(|i| i as i32)
    }
    fn source_component_name(&self, id: i32) -> Option<&'static str> {
        let names: &[&'static str] = if self.dir == Dir::ToClient { &d::NEW_COMPONENT_NAMES } else { &d::OLD_COMPONENT_NAMES };
        names.get(usize::try_from(id).ok()?).copied()
    }
    fn target_component_id(&self, name: &str) -> Option<i32> {
        let names: &[&str] = if self.dir == Dir::ToClient { &d::OLD_COMPONENT_NAMES } else { &d::NEW_COMPONENT_NAMES };
        names.iter().position(|n| *n == name).map(|i| i as i32)
    }
    fn new_component_name(name: &str) -> Option<&'static str> {
        d::NEW_COMPONENT_NAMES.iter().find(|n| **n == strip_ns(name)).copied()
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
    /// One network NBT value.
    pub fn copy_nbt(r: &mut Reader, w: &mut Writer) -> Result<()> {
        let start = r.position();
        nbt::skip(r)?;
        w.raw(&raw_since(r, start)?);
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
    /// `HolderSet`: varint n+1 then n ids, or 0 then a tag key.
    pub fn holder_set(&self, r: &mut Reader, w: &mut Writer, reg: Reg) -> Result<()> {
        let v = r.varint()?;
        w.varint(v);
        if v == 0 {
            return Self::copy_string(r, w);
        }
        for _ in 0..(v - 1).max(0) {
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

    // ---- item stacks -----------------------------------------------------------------------

    /// Full item stack: count, then id + component patch when count > 0.
    pub fn item_stack(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let count = Self::copy_varint(r, w)?;
        if count <= 0 {
            return Ok(());
        }
        let id = r.varint()?;
        let mut ctx = Ctx::default();
        let added = r.varint()?;
        let removed = r.varint()?;
        if added < 0 || removed < 0 || added + removed > 512 {
            return Err(err("bad component counts"));
        }
        for _ in 0..added {
            self.component(r, &mut ctx)?;
        }
        for _ in 0..removed {
            let ty = r.varint()?;
            let name = self.source_component_name(ty).ok_or_else(|| err("unknown removed component"))?;
            self.removed_component(name, &mut ctx);
        }
        let id = self.finish(id, &mut ctx)?;
        w.varint(id);
        self.write_patch(&ctx, w);
        Ok(())
    }

    /// Merchant item cost: id, count, then exact components (no removals, no backup).
    pub fn item_cost(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let id = r.varint()?;
        w.varint(self.item(id));
        Self::copy_varint(r, w)?;
        let n = r.varint()?;
        let mut ctx = Ctx::default();
        for _ in 0..n.max(0) {
            self.component(r, &mut ctx)?;
        }
        self.finish_food(&mut ctx);
        let entries: Vec<(i32, &Vec<u8>)> = ctx.out.iter().filter_map(|(name, v)| self.target_component_id(name).map(|t| (t, v))).collect();
        w.varint(entries.len() as i32);
        for (t, v) in entries {
            w.varint(t);
            w.raw(v);
        }
        Ok(())
    }
    pub fn opt_item_cost(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        if Self::copy_bool(r, w)? {
            self.item_cost(r, w)?;
        }
        Ok(())
    }

    fn write_patch(&self, ctx: &Ctx, w: &mut Writer) {
        let mut seen: Vec<&str> = Vec::new();
        let mut added: Vec<(i32, &Vec<u8>)> = Vec::new();
        // later entries win (restored backups are pushed last)
        for (name, value) in ctx.out.iter().rev() {
            if seen.contains(name) {
                continue;
            }
            seen.push(name);
            if let Some(t) = self.target_component_id(name) {
                added.push((t, value));
            }
        }
        added.reverse();
        let mut removed: Vec<i32> = Vec::new();
        for name in &ctx.removed {
            if seen.contains(name) {
                continue;
            }
            if let Some(t) = self.target_component_id(name) {
                if !removed.contains(&t) {
                    removed.push(t);
                }
            }
        }
        w.varint(added.len() as i32);
        w.varint(removed.len() as i32);
        for (t, v) in added {
            w.varint(t);
            w.raw(v);
        }
        for t in removed {
            w.varint(t);
        }
    }

    fn removed_component(&self, name: &'static str, ctx: &mut Ctx) {
        match self.dir {
            Dir::ToClient => {
                if DROPPED.contains(&name) {
                    ctx.backup_removed.push(name);
                    if name == "damage_resistant" {
                        ctx.removed.push("fire_resistant");
                    }
                } else {
                    ctx.removed.push(name);
                }
            }
            Dir::ToServer => match name {
                "fire_resistant" => ctx.removed.push("damage_resistant"),
                "food" => {
                    ctx.removed.push("food");
                    ctx.removed.push("consumable");
                    ctx.removed.push("use_remainder");
                }
                other => ctx.removed.push(other),
            },
        }
    }

    /// Applies the post-processing that needs the whole patch; returns the target item id.
    fn finish(&self, id: i32, ctx: &mut Ctx) -> Result<i32> {
        match self.dir {
            Dir::ToClient => {
                self.finish_food(ctx);
                let substituted = Self::substituted(id);
                let has_name = ctx.out.iter().any(|(n, _)| *n == "custom_name");
                let mut added_name = false;
                if substituted && !has_name {
                    let name = d::NEW_ITEM_NAMES.get(id as usize).copied().unwrap_or("");
                    let mut text = Tag::compound();
                    text.insert("text", Tag::String(pretty_name(name)));
                    text.insert("italic", Tag::Byte(0));
                    let mut v = Writer::new();
                    nbt::write(&mut v, &text);
                    ctx.out.push(("custom_name", v.into_inner()));
                    added_name = true;
                }
                if substituted || !ctx.backup.is_empty() || !ctx.backup_removed.is_empty() {
                    let mut b = Tag::compound();
                    if substituted {
                        b.insert("id", Tag::Int(id));
                    }
                    if added_name {
                        b.insert("n", Tag::Byte(1));
                    }
                    if !ctx.backup.is_empty() {
                        let comps = ctx.backup.iter().map(|(n, raw)| (n.to_string(), Tag::ByteArray(raw.iter().map(|b| *b as i8).collect()))).collect();
                        b.insert("c", Tag::Compound(comps));
                    }
                    if !ctx.backup_removed.is_empty() {
                        b.insert("r", Tag::List(ctx.backup_removed.iter().map(|n| Tag::String(n.to_string())).collect()));
                    }
                    let pos = ctx.out.iter().position(|(n, _)| *n == "custom_data");
                    let mut data = match pos {
                        Some(p) => {
                            let bytes = ctx.out.remove(p).1;
                            nbt::read(&mut Reader::new(&bytes))?
                        }
                        None => {
                            b.insert("e", Tag::Byte(1));
                            Tag::compound()
                        }
                    };
                    if !matches!(data, Tag::Compound(_)) {
                        data = Tag::compound();
                    }
                    data.insert(BACKUP_KEY, b);
                    let mut v = Writer::new();
                    nbt::write(&mut v, &data);
                    ctx.out.push(("custom_data", v.into_inner()));
                }
                Ok(self.item(id))
            }
            Dir::ToServer => {
                let mut id = self.item(id);
                if ctx.glint_lost && !ctx.out.iter().any(|(n, _)| *n == "enchantment_glint_override") {
                    ctx.out.push(("enchantment_glint_override", vec![1]));
                }
                if let Some(restore) = ctx.restore.take() {
                    if let Some(orig) = restore.id {
                        id = orig;
                    }
                    if restore.remove_custom_name {
                        ctx.out.retain(|(n, _)| *n != "custom_name");
                    }
                    if restore.custom_data_was_added {
                        // the client only saw custom_data because of the backup; drop it when empty
                        if let Some(p) = ctx.out.iter().position(|(n, _)| *n == "custom_data") {
                            if ctx.out[p].1 == [10, 0] {
                                ctx.out.remove(p);
                            }
                        }
                    }
                    for name in &restore.removed {
                        ctx.out.retain(|(n, _)| n != name);
                        ctx.removed.push(name);
                    }
                    for (name, raw) in restore.comps {
                        ctx.out.retain(|(n, _)| *n != name);
                        ctx.removed.retain(|n| *n != name);
                        ctx.out.push((name, raw));
                    }
                }
                Ok(id)
            }
        }
    }

    /// Builds the 1.21 food component from 1.21.2 food + consumable + use_remainder.
    fn finish_food(&self, ctx: &mut Ctx) {
        if self.dir != Dir::ToClient {
            return;
        }
        let Some((nutrition, saturation, always)) = ctx.food.take() else { return };
        let mut w = Writer::new();
        w.varint(nutrition);
        w.f32(saturation);
        w.bool(always);
        w.f32(ctx.eat_seconds.unwrap_or(1.6));
        match ctx.remainder.take() {
            Some(item) => {
                w.bool(true);
                w.raw(&item);
            }
            None => {
                w.bool(false);
            }
        }
        w.varint(ctx.food_effects.len() as i32);
        for e in &ctx.food_effects {
            w.raw(e);
        }
        ctx.out.push(("food", w.into_inner()));
    }

    /// Reads one component (type id + value) and records its converted form in `ctx`.
    fn component(&self, r: &mut Reader, ctx: &mut Ctx) -> Result<()> {
        let ty = r.varint()?;
        let name = self.source_component_name(ty).ok_or_else(|| err(&format!("unknown data component type {ty}")))?;
        let start = r.position();
        let mut w = Writer::new();
        let keep = self.component_value(name, r, &mut w, ctx)?;
        if self.dir == Dir::ToClient {
            let lossy = match keep {
                Keep::Yes | Keep::Rebuilt => false,
                Keep::No | Keep::Lossy => true,
            };
            if lossy {
                let raw = raw_since(r, start)?;
                ctx.backup.push((name, raw));
            }
        }
        if matches!(keep, Keep::Yes | Keep::Lossy) {
            ctx.out.push((name, w.into_inner()));
        }
        Ok(())
    }

    /// Parses one component payload named `name` (source version), writing the target form.
    fn component_value(&self, name: &'static str, r: &mut Reader, w: &mut Writer, ctx: &mut Ctx) -> Result<Keep> {
        let to_client = self.dir == Dir::ToClient;
        macro_rules! varint { () => {{ Self::copy_varint(r, w)?; }}; }
        macro_rules! f32 { () => {{ Self::copy_bytes(r, w, 4)?; }}; }
        macro_rules! bool { () => {{ Self::copy_bool(r, w)?; }}; }
        macro_rules! string { () => {{ Self::copy_string(r, w)?; }}; }
        macro_rules! nbt { () => {{ Self::copy_nbt(r, w)?; }}; }
        match name {
            "max_stack_size" | "max_damage" | "damage" | "rarity" | "repair_cost" | "custom_model_data" | "map_id" | "map_post_processing"
            | "ominous_bottle_amplifier" | "base_color" => {
                varint!();
                Ok(Keep::Yes)
            }
            "unbreakable" | "enchantment_glint_override" => {
                bool!();
                Ok(Keep::Yes)
            }
            "hide_additional_tooltip" | "hide_tooltip" | "creative_slot_lock" => Ok(Keep::Yes),
            "custom_name" | "item_name" | "map_decorations" | "debug_stick_state" | "entity_data" | "bucket_entity_data" | "block_entity_data"
            | "recipes" | "container_loot" | "intangible_projectile" => {
                nbt!();
                Ok(Keep::Yes)
            }
            "custom_data" => {
                let tag = nbt::read(r)?;
                self.custom_data(tag, w, ctx);
                Ok(Keep::Yes)
            }
            "lore" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    nbt!();
                }
                Ok(Keep::Yes)
            }
            "enchantments" | "stored_enchantments" => {
                let n = r.varint()?;
                let mut kept = Vec::new();
                for _ in 0..n.max(0) {
                    let id = r.varint()?;
                    let level = r.varint()?;
                    if to_client || level != 0 {
                        kept.push((id, level));
                    }
                }
                let show = r.bool()?;
                if !to_client && name == "enchantments" && n > 0 && kept.is_empty() {
                    ctx.glint_lost = true;
                }
                w.varint(kept.len() as i32);
                for (id, level) in kept {
                    w.varint(id);
                    w.varint(level);
                }
                w.bool(show);
                Ok(Keep::Yes)
            }
            "can_place_on" | "can_break" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    self.block_predicate(r, w)?;
                }
                bool!();
                Ok(Keep::Yes)
            }
            "attribute_modifiers" => {
                let n = r.varint()?;
                let mut body = Writer::new();
                let mut kept = 0;
                for _ in 0..n.max(0) {
                    let attr = r.varint()?;
                    let id = r.string()?;
                    let amount = r.f64()?;
                    let op = r.varint()?;
                    let slot = r.varint()?;
                    if to_client && Self::attribute_unknown(attr) {
                        continue;
                    }
                    body.varint(self.attribute(attr));
                    body.string(id);
                    body.f64(amount);
                    body.varint(op);
                    body.varint(slot);
                    kept += 1;
                }
                w.varint(kept);
                w.raw(&body.buf);
                bool!();
                Ok(if kept < n { Keep::Lossy } else { Keep::Yes })
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
                Ok(Keep::Yes)
            }
            "dyed_color" => {
                Self::copy_bytes(r, w, 4)?;
                bool!();
                Ok(Keep::Yes)
            }
            "map_color" => {
                Self::copy_bytes(r, w, 4)?;
                Ok(Keep::Yes)
            }
            "charged_projectiles" | "bundle_contents" | "container" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    self.item_stack(r, w)?;
                }
                Ok(Keep::Yes)
            }
            "potion_contents" => {
                if Self::copy_bool(r, w)? {
                    varint!();
                }
                if Self::copy_bool(r, w)? {
                    Self::copy_bytes(r, w, 4)?;
                }
                self.potion_effects(r, w)?;
                if to_client {
                    // 1.21.2 custom name: the 1.21 client has none
                    let has_name = r.bool()?;
                    if has_name {
                        r.string()?;
                        return Ok(Keep::Lossy);
                    }
                } else {
                    w.bool(false);
                }
                Ok(Keep::Yes)
            }
            "suspicious_stew_effects" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    varint!();
                    varint!();
                }
                Ok(Keep::Yes)
            }
            "writable_book_content" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    string!();
                    Self::copy_opt_string(r, w)?;
                }
                Ok(Keep::Yes)
            }
            "written_book_content" => {
                string!();
                Self::copy_opt_string(r, w)?;
                string!();
                varint!();
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    nbt!();
                    Self::copy_opt_nbt(r, w)?;
                }
                bool!();
                Ok(Keep::Yes)
            }
            "trim" => {
                let mut lossy = false;
                self.holder(r, w, Reg::Same, |s, r, w| {
                    Self::copy_string(r, w)?; // asset name
                    let item = r.varint()?;
                    w.varint(s.item(item));
                    Self::copy_bytes(r, w, 4)?; // item model index
                    let n = r.varint()?;
                    for _ in 0..n.max(0) {
                        if s.dir == Dir::ToClient {
                            r.string()?;
                        } else {
                            r.varint()?;
                        }
                        r.string()?;
                    }
                    lossy = n > 0;
                    w.varint(0); // armor material overrides: keyed differently in each version
                    Self::copy_nbt(r, w)
                })?;
                self.holder(r, w, Reg::Same, |s, r, w| {
                    Self::copy_string(r, w)?; // asset id
                    let item = r.varint()?;
                    w.varint(s.item(item));
                    Self::copy_nbt(r, w)?;
                    Self::copy_bool(r, w).map(|_| ())
                })?;
                bool!();
                Ok(if lossy && to_client { Keep::Lossy } else { Keep::Yes })
            }
            "instrument" => {
                let mut direct = false;
                self.holder(r, w, Reg::Same, |s, r, w| {
                    direct = true;
                    s.sound_event(r, w)?;
                    if s.dir == Dir::ToClient {
                        let seconds = r.f32()?;
                        w.varint((seconds * 20.0) as i32);
                        Self::copy_bytes(r, w, 4)?; // range
                        nbt::skip(r) // description
                    } else {
                        let ticks = r.varint()?;
                        w.f32(ticks as f32 / 20.0);
                        Self::copy_bytes(r, w, 4)?;
                        nbt::write(w, &Tag::String(String::new()));
                        Ok(())
                    }
                })?;
                Ok(if direct && to_client { Keep::Lossy } else { Keep::Yes })
            }
            "jukebox_playable" => {
                if Self::copy_bool(r, w)? {
                    self.holder(r, w, Reg::Same, |s, r, w| {
                        s.sound_event(r, w)?;
                        Self::copy_nbt(r, w)?;
                        Self::copy_bytes(r, w, 4)?;
                        Self::copy_varint(r, w).map(|_| ())
                    })?;
                } else {
                    string!();
                }
                bool!();
                Ok(Keep::Yes)
            }
            "lodestone_tracker" => {
                if Self::copy_bool(r, w)? {
                    string!();
                    Self::copy_bytes(r, w, 8)?;
                }
                bool!();
                Ok(Keep::Yes)
            }
            "firework_explosion" => {
                Self::firework_explosion(r, w)?;
                Ok(Keep::Yes)
            }
            "fireworks" => {
                varint!();
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    Self::firework_explosion(r, w)?;
                }
                Ok(Keep::Yes)
            }
            "profile" => {
                Self::copy_opt_string(r, w)?;
                if Self::copy_bool(r, w)? {
                    Self::copy_bytes(r, w, 16)?;
                }
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    string!();
                    string!();
                    Self::copy_opt_string(r, w)?;
                }
                Ok(Keep::Yes)
            }
            "note_block_sound" => {
                string!();
                Ok(Keep::Yes)
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
                Ok(Keep::Yes)
            }
            "pot_decorations" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    let id = r.varint()?;
                    w.varint(self.item(id));
                }
                Ok(Keep::Yes)
            }
            "block_state" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    string!();
                    string!();
                }
                Ok(Keep::Yes)
            }
            "bees" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n.max(0) {
                    nbt!();
                    varint!();
                    varint!();
                }
                Ok(Keep::Yes)
            }
            "lock" => {
                let tag = nbt::read(r)?;
                if to_client {
                    // 1.21.2: item predicate {components:{custom_name:"<json>"}} → 1.21: plain string
                    let text = tag
                        .get("components")
                        .and_then(|c| c.get("minecraft:custom_name").or_else(|| c.get("custom_name")))
                        .and_then(Tag::as_str)
                        .filter(|s| *s != "null")
                        .map(json_plain_text);
                    match text {
                        Some(t) => {
                            nbt::write(w, &Tag::String(t));
                            Ok(Keep::Lossy)
                        }
                        None => Ok(Keep::No),
                    }
                } else {
                    let plain = tag.as_str().unwrap_or("").to_string();
                    let json = serde_json::json!({ "text": plain }).to_string();
                    let mut components = Tag::compound();
                    components.insert("custom_name", Tag::String(json));
                    let mut pred = Tag::compound();
                    pred.insert("components", components);
                    nbt::write(w, &pred);
                    Ok(Keep::Yes)
                }
            }
            // ---- 1.21.2 → 1.21 only ----
            "food" if to_client => {
                let nutrition = r.varint()?;
                let saturation = r.f32()?;
                let always = r.bool()?;
                ctx.food = Some((nutrition, saturation, always));
                Ok(Keep::Rebuilt)
            }
            "consumable" => {
                ctx.eat_seconds = Some(r.f32()?);
                r.varint()?; // animation
                self.sound_event(r, &mut Writer::new())?;
                r.bool()?; // particles
                let n = r.varint()?;
                for _ in 0..n.max(0) {
                    self.consume_effect(r, Some(&mut ctx.food_effects))?;
                }
                Ok(Keep::No)
            }
            "death_protection" => {
                let n = r.varint()?;
                for _ in 0..n.max(0) {
                    self.consume_effect(r, None)?;
                }
                Ok(Keep::No)
            }
            "use_remainder" => {
                let mut item = Writer::new();
                self.item_stack(r, &mut item)?;
                ctx.remainder = Some(item.into_inner());
                Ok(Keep::No)
            }
            "use_cooldown" => {
                r.f32()?;
                if r.bool()? {
                    r.string()?;
                }
                Ok(Keep::No)
            }
            "damage_resistant" => {
                let types = r.string()?;
                if strip_ns(types.trim_start_matches('#')) == "is_fire" {
                    ctx.out.push(("fire_resistant", Vec::new()));
                }
                Ok(Keep::No)
            }
            "enchantable" => {
                r.varint()?;
                Ok(Keep::No)
            }
            "equippable" => {
                let mut scratch = Writer::new();
                r.varint()?;
                self.sound_event(r, &mut scratch)?;
                Self::copy_opt_string(r, &mut scratch)?;
                Self::copy_opt_string(r, &mut scratch)?;
                self.opt_holder_set(r, &mut scratch, Reg::Same)?;
                r.bytes(3)?;
                Ok(Keep::No)
            }
            "repairable" => {
                self.holder_set(r, &mut Writer::new(), Reg::Same)?;
                Ok(Keep::No)
            }
            "glider" => Ok(Keep::No),
            "item_model" | "tooltip_style" => {
                r.string()?;
                Ok(Keep::No)
            }
            // ---- 1.21 → 1.21.2 only ----
            "food" => {
                // 1.21: nutrition, saturation, can_always_eat, eat_seconds, using_converts_to, effects
                let nutrition = r.varint()?;
                let saturation = r.f32()?;
                let always = r.bool()?;
                let seconds = r.f32()?;
                let mut remainder = Writer::new();
                let has_remainder = r.bool()?;
                if has_remainder {
                    self.item_stack(r, &mut remainder)?;
                }
                let n = r.varint()?;
                let mut effects = Writer::new();
                for _ in 0..n.max(0) {
                    // one apply_effects consume effect per food effect
                    effects.varint(0);
                    effects.varint(1);
                    Self::copy_varint(r, &mut effects)?;
                    self.potion_effect_data(r, &mut effects)?;
                    Self::copy_bytes(r, &mut effects, 4)?; // probability
                }
                w.varint(nutrition);
                w.f32(saturation);
                w.bool(always);
                let mut consumable = Writer::new();
                consumable.f32(seconds);
                consumable.varint(1); // eat animation
                match d::NEW_SOUND_NAMES.iter().position(|s| *s == "entity.generic.eat") {
                    Some(id) => {
                        consumable.varint(id as i32 + 1);
                    }
                    None => {
                        consumable.varint(0);
                        consumable.string("minecraft:entity.generic.eat");
                        consumable.bool(false);
                    }
                }
                consumable.bool(true);
                consumable.varint(n.max(0));
                consumable.raw(&effects.buf);
                ctx.out.push(("consumable", consumable.into_inner()));
                if has_remainder {
                    ctx.out.push(("use_remainder", remainder.into_inner()));
                }
                Ok(Keep::Yes)
            }
            "fire_resistant" => {
                let mut v = Writer::new();
                v.string("minecraft:is_fire");
                ctx.out.push(("damage_resistant", v.into_inner()));
                Ok(Keep::No)
            }
            other => Err(err(&format!("no codec for data component {other}"))),
        }
    }

    /// custom_data: on the way to the server, pulls our backup out of it.
    fn custom_data(&self, mut tag: Tag, w: &mut Writer, ctx: &mut Ctx) {
        if self.dir == Dir::ToServer {
            if let Some(b) = tag.remove(BACKUP_KEY) {
                let mut restore = Restore {
                    id: b.get("id").and_then(Tag::as_i32),
                    remove_custom_name: b.get("n").is_some(),
                    custom_data_was_added: b.get("e").is_some(),
                    ..Restore::default()
                };
                if let Some(Tag::Compound(comps)) = b.get("c") {
                    for (name, value) in comps {
                        if let (Some(static_name), Tag::ByteArray(raw)) = (Self::new_component_name(name), value) {
                            restore.comps.push((static_name, raw.iter().map(|b| *b as u8).collect()));
                        }
                    }
                }
                if let Some(Tag::List(list)) = b.get("r") {
                    for name in list.iter().filter_map(Tag::as_str) {
                        if let Some(static_name) = Self::new_component_name(name) {
                            restore.removed.push(static_name);
                        }
                    }
                }
                ctx.restore = Some(restore);
            }
        }
        nbt::write(w, &tag);
    }

    /// 1.21.2 consume effect. Apply-effects entries are collected as 1.21 food effects.
    fn consume_effect(&self, r: &mut Reader, mut food_effects: Option<&mut Vec<Vec<u8>>>) -> Result<()> {
        let ty = r.varint()?;
        match ty {
            0 => {
                let n = r.varint()?;
                let mut effects = Vec::new();
                for _ in 0..n.max(0) {
                    let mut e = Writer::new();
                    Self::copy_varint(r, &mut e)?;
                    self.potion_effect_data(r, &mut e)?;
                    effects.push(e.into_inner());
                }
                let probability = r.f32()?;
                if let Some(out) = food_effects.as_deref_mut() {
                    for mut e in effects {
                        e.extend_from_slice(&probability.to_be_bytes());
                        out.push(e);
                    }
                }
            }
            1 => self.holder_set(r, &mut Writer::new(), Reg::Same)?,
            2 => {}
            3 => {
                r.f32()?;
            }
            4 => self.sound_event(r, &mut Writer::new())?,
            _ => return Err(err("unknown consume effect")),
        }
        Ok(())
    }

    fn potion_effects(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let n = Self::copy_varint(r, w)?;
        for _ in 0..n.max(0) {
            Self::copy_varint(r, w)?;
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

    fn firework_explosion(r: &mut Reader, w: &mut Writer) -> Result<()> {
        Self::copy_varint(r, w)?;
        for _ in 0..2 {
            let n = Self::copy_varint(r, w)?;
            Self::copy_bytes(r, w, n.max(0) as usize * 4)?;
        }
        Self::copy_bytes(r, w, 2)
    }

    /// 1.20.5 block predicate: optional blocks, optional state properties, optional nbt.
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

/// What happens to a parsed component.
enum Keep {
    /// Converted without loss.
    Yes,
    /// Converted, but the original is also backed up.
    Lossy,
    /// Dropped; the original is backed up.
    No,
    /// Written later from several components, no backup needed (1.21 food).
    Rebuilt,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comp(names: &[&str], name: &str) -> i32 {
        names.iter().position(|n| *n == name).unwrap() as i32
    }

    fn stack(id: i32, comps: &[(i32, Vec<u8>)], removed: &[i32]) -> Vec<u8> {
        let mut w = Writer::new();
        w.varint(1);
        w.varint(id);
        w.varint(comps.len() as i32);
        w.varint(removed.len() as i32);
        for (t, v) in comps {
            w.varint(*t);
            w.raw(v);
        }
        for t in removed {
            w.varint(*t);
        }
        w.into_inner()
    }

    fn convert(items: &Items, bytes: &[u8]) -> Vec<u8> {
        let mut out = Writer::new();
        let mut r = Reader::new(bytes);
        items.item_stack(&mut r, &mut out).unwrap();
        assert!(r.is_empty(), "trailing bytes");
        out.into_inner()
    }

    #[test]
    fn food_is_rebuilt_from_consumable_and_restored() {
        let apple = Items::new_item_by_name("apple").unwrap();
        let mut food = Writer::new();
        food.varint(4).f32(2.4).bool(false);
        let mut consumable = Writer::new();
        consumable.f32(0.8).varint(1).varint(5).bool(true).varint(1);
        consumable.varint(0).varint(1).varint(10).varint(0).varint(100).bool(false).bool(true).bool(true).bool(false).f32(0.5);
        let input = stack(
            apple,
            &[(comp(&d::NEW_COMPONENT_NAMES, "food"), food.buf.clone()), (comp(&d::NEW_COMPONENT_NAMES, "consumable"), consumable.buf.clone())],
            &[],
        );
        let client = convert(&Items::new(Dir::ToClient), &input);
        let mut r = Reader::new(&client);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), d::ITEMS[apple as usize] as i32);
        assert_eq!(r.varint().unwrap(), 2); // food + custom_data backup
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), comp(&d::OLD_COMPONENT_NAMES, "food"));
        assert_eq!(r.varint().unwrap(), 4);
        assert_eq!(r.f32().unwrap(), 2.4);
        assert!(!r.bool().unwrap());
        assert_eq!(r.f32().unwrap(), 0.8); // eat seconds from consumable
        assert!(!r.bool().unwrap()); // no remainder
        assert_eq!(r.varint().unwrap(), 1); // one effect
        // back to the server: the original consumable comes back unchanged
        let server = convert(&Items::new(Dir::ToServer), &client);
        let mut r = Reader::new(&server);
        r.varint().unwrap();
        assert_eq!(r.varint().unwrap(), apple);
        let added = r.varint().unwrap();
        assert_eq!(r.varint().unwrap(), 0);
        let mut found = Vec::new();
        for _ in 0..added {
            let t = r.varint().unwrap();
            let name = d::NEW_COMPONENT_NAMES[t as usize];
            found.push(name);
            match name {
                "food" => {
                    assert_eq!(r.bytes(food.buf.len()).unwrap(), &food.buf[..]);
                }
                "consumable" => {
                    assert_eq!(r.bytes(consumable.buf.len()).unwrap(), &consumable.buf[..]);
                }
                other => panic!("unexpected {other}"),
            }
        }
        found.sort();
        assert_eq!(found, vec!["consumable", "food"]);
        assert!(r.is_empty());
    }

    #[test]
    fn dropped_component_and_new_item_roundtrip() {
        // an item that only exists in 1.21.2, with an item_model the 1.21 client lacks
        let id = (0..d::NEW_ITEM_NAMES.len() as i32).find(|i| Items::substituted(*i)).expect("a 1.21.2-only item");
        let mut model = Writer::new();
        model.string("minecraft:custom");
        let input = stack(id, &[(comp(&d::NEW_COMPONENT_NAMES, "item_model"), model.buf.clone())], &[]);
        let client = convert(&Items::new(Dir::ToClient), &input);
        let back = convert(&Items::new(Dir::ToServer), &client);
        assert_eq!(back, input);
    }

    #[test]
    fn fire_resistant_maps_both_ways() {
        let mut v = Writer::new();
        v.string("minecraft:is_fire");
        let input = stack(1, &[(comp(&d::NEW_COMPONENT_NAMES, "damage_resistant"), v.buf.clone())], &[]);
        let client = convert(&Items::new(Dir::ToClient), &input);
        let mut r = Reader::new(&client);
        r.varint().unwrap();
        r.varint().unwrap();
        assert_eq!(r.varint().unwrap(), 2); // fire_resistant + custom_data
        r.varint().unwrap();
        assert_eq!(r.varint().unwrap(), comp(&d::OLD_COMPONENT_NAMES, "fire_resistant"));
        // client-made fire resistant item (no backup) → damage_resistant
        let plain = stack(1, &[(comp(&d::OLD_COMPONENT_NAMES, "fire_resistant"), Vec::new())], &[]);
        let server = convert(&Items::new(Dir::ToServer), &plain);
        assert_eq!(server, input);
    }

    #[test]
    fn json_text() {
        assert_eq!(json_plain_text("\"key\""), "key");
        assert_eq!(json_plain_text(r#"{"text":"a","extra":[{"text":"b"},"c"]}"#), "abc");
        assert_eq!(pretty_name("minecraft:pale_oak_boat"), "Pale Oak Boat");
    }
}
