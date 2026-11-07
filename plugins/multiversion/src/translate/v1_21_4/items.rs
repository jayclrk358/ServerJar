//! Item stacks and data components, 1.21.5 ↔ 1.21.4 (ViaBackwards' `BlockItemPacketRewriter1_21_5`
//! and ViaVersion's `BlockItemPacketRewriter1_21_5`).
//!
//! Main differences: 1.21.5 replaced the per-component `show_in_tooltip` flags and the
//! hide_tooltip / hide_additional_tooltip components with `tooltip_display`, added 29 components
//! (entity variants, weapon, blocks_attacks, ...), and made clients send hashed stacks in
//! container clicks. Components the old client can't hold are backed up in custom_data and
//! restored when the item comes back; the server's component hashes are stored the same way so
//! container clicks can be answered with the hashes the server expects.

use std::collections::HashMap;

use super::data_1_21_4 as d;
use super::hash::{self, full_key, ident, Hv, MapB, UNKNOWN_HASH};
use super::text;
use crate::proto::buf::{DecodeError, Reader, Result, Writer};
use crate::proto::nbt::{self, Tag};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    /// 1.21.5 server → 1.21.4 client
    ToClient,
    /// 1.21.4 client → 1.21.5 server
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

/// custom_data keys used to carry data through the old client.
pub const HASHES_KEY: &str = "MV|hashes";
pub const BACKUP_KEY: &str = "MV|backup";
pub const MARKER_KEY: &str = "MV|custom_data";

/// Components whose 1.21.5 value can't survive the trip through a 1.21.4 client unchanged; their
/// raw 1.21.5 bytes are backed up in custom_data (besides every component 1.21.4 lacks).
const LOSSY: &[&str] = &["tool", "equippable", "tooltip_display", "instrument", "trim"];

/// 1.21.5's hide_additional_tooltip replacement: these components hidden = the old flag set.
const HIDE_ADDITIONAL_KEYS: &[&str] = &[
    "banner_patterns", "bees", "block_entity_data", "block_state", "bundle_contents", "charged_projectiles", "container",
    "container_loot", "firework_explosion", "fireworks", "instrument", "map_id", "painting/variant", "pot_decorations",
    "potion_contents", "tropical_fish/pattern", "written_book_content",
];

/// Entries of the data-driven registries the server sent, in id order (for hashing and for
/// converting registry-based entity variants).
#[derive(Default)]
pub struct Registries {
    keys: HashMap<String, Vec<String>>,
}

impl Registries {
    pub fn set(&mut self, registry: &str, keys: Vec<String>) {
        self.keys.insert(strip_ns(registry).to_string(), keys);
    }
    pub fn key(&self, registry: &str, id: i32) -> Option<&str> {
        self.keys.get(registry)?.get(usize::try_from(id).ok()?).map(String::as_str)
    }
}

fn strip_ns(s: &str) -> &str {
    s.strip_prefix("minecraft:").unwrap_or(s)
}

/// What a component turned into.
enum Kind {
    Keep,
    Drop,
    /// ToClient: written without its trailing show_in_tooltip flag, appended later.
    KeepShow(&'static str),
    /// ToServer: the old show_in_tooltip flag that was read.
    Shown(&'static str, bool),
    /// ToClient: tooltip_display (hide everything, hidden component names).
    Tooltip(bool, Vec<&'static str>),
    /// ToServer: hide_tooltip / hide_additional_tooltip markers.
    HideTooltip,
    HideAdditional,
    /// ToClient: one of the three tropical fish bucket components (index, value).
    Tropical(usize, i32),
}

struct Out {
    name: &'static str,
    bytes: Vec<u8>,
    show: Option<&'static str>,
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
    fn source_names(&self) -> &'static [&'static str] {
        if self.dir == Dir::ToClient { &d::NEW_COMPONENT_NAMES } else { &d::OLD_COMPONENT_NAMES }
    }
    fn target_names(&self) -> &'static [&'static str] {
        if self.dir == Dir::ToClient { &d::OLD_COMPONENT_NAMES } else { &d::NEW_COMPONENT_NAMES }
    }
    fn source_component_name(&self, id: i32) -> Option<&'static str> {
        self.source_names().get(usize::try_from(id).ok()?).copied()
    }
    fn target_component_id(&self, name: &str) -> Option<i32> {
        self.target_names().iter().position(|n| *n == name).map(|i| i as i32)
    }
    /// Component type id in the target version, or `None` if the target lacks it.
    pub fn component_type(&self, id: i32) -> Option<i32> {
        self.target_component_id(self.source_component_name(id)?)
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
    pub fn copy_nbt(r: &mut Reader, w: &mut Writer) -> Result<()> {
        let start = r.position();
        nbt::skip(r)?;
        let end = r.position();
        r.set_position(start);
        w.raw(r.bytes(end - start)?);
        Ok(())
    }
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
    /// A text component: downgraded on the way to the client.
    fn text(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        if self.dir == Dir::ToClient { text::copy_component(r, w) } else { Self::copy_nbt(r, w) }
    }
    fn opt_text(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        if Self::copy_bool(r, w)? {
            self.text(r, w)?;
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
        w.varint(v);
        if v == 0 {
            Self::copy_string(r, w)
        } else {
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

    /// Full item stack (count, then id + component patch when count > 0).
    pub fn item_stack(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        self.item_stack_tracked(r, w, None)
    }

    /// Like [`item_stack`]; with `regs`, the server's component hashes are stored on the item so
    /// a later container click can report them (inventory packets only, like ViaVersion).
    pub fn item_stack_tracked(&self, r: &mut Reader, w: &mut Writer, regs: Option<&Registries>) -> Result<()> {
        let count = Self::copy_varint(r, w)?;
        if count <= 0 {
            return Ok(());
        }
        let id = r.varint()?;
        w.varint(self.item(id));
        if self.dir == Dir::ToClient {
            self.patch_to_client(r, w, id, regs)
        } else {
            self.patch_to_server(r, w, false).map(|_| ())
        }
    }

    /// 1.21.4 item → 1.21.5 item with length-prefixed components (serverbound creative slot).
    pub fn item_stack_length_prefixed(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let count = Self::copy_varint(r, w)?;
        if count <= 0 {
            return Ok(());
        }
        let id = r.varint()?;
        w.varint(self.item(id));
        self.patch_to_server(r, w, true).map(|_| ())
    }

    /// 1.21.4 item → 1.21.5 hashed stack (serverbound container click).
    pub fn hashed_stack(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let count = r.varint()?;
        if count <= 0 {
            w.bool(false);
            return Ok(());
        }
        let id = r.varint()?;
        let mut scratch = Writer::new();
        let hashes = self.patch_to_server(r, &mut scratch, false)?;
        w.bool(true);
        match hashes {
            Some(h) => {
                w.varint(h.id);
                w.varint(count);
                w.varint(h.hashes.len() as i32);
                for (ty, hash) in &h.hashes {
                    w.varint(*ty);
                    w.i32(*hash);
                }
                w.varint(h.removed.len() as i32);
                for ty in &h.removed {
                    w.varint(*ty);
                }
            }
            None => {
                // No stored hashes: the item had no component patch (or came from elsewhere).
                w.varint(self.item(id));
                w.varint(count);
                w.varint(0);
                w.varint(0);
            }
        }
        Ok(())
    }

    /// Item template / cost (merchant offers): id, count, then only added components.
    pub fn item_cost(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let id = r.varint()?;
        w.varint(self.item(id));
        Self::copy_varint(r, w)?;
        let n = r.varint()?;
        let mut outs = Vec::new();
        let mut hidden = Vec::new();
        for _ in 0..n {
            let ty = r.varint()?;
            let name = self.source_component_name(ty).ok_or_else(|| err(&format!("unknown data component type {ty}")))?;
            let mut v = Writer::new();
            match self.value(name, r, &mut v)? {
                Kind::Keep => outs.push(Out { name, bytes: v.buf, show: None }),
                Kind::KeepShow(s) => outs.push(Out { name, bytes: v.buf, show: Some(s) }),
                Kind::Shown(s, false) => {
                    hidden.push(s);
                    outs.push(Out { name, bytes: v.buf, show: None });
                }
                Kind::Shown(..) => outs.push(Out { name, bytes: v.buf, show: None }),
                _ => {}
            }
        }
        let outs: Vec<Out> = outs.into_iter().filter(|o| self.target_component_id(o.name).is_some()).collect();
        w.varint(outs.len() as i32);
        for o in outs {
            w.varint(self.target_component_id(o.name).unwrap_or(0));
            w.raw(&o.bytes);
            if o.show.is_some() {
                w.bool(true);
            }
        }
        Ok(())
    }
    pub fn opt_item_cost(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        if Self::copy_bool(r, w)? {
            self.item_cost(r, w)?;
        }
        Ok(())
    }

    fn patch_to_client(&self, r: &mut Reader, w: &mut Writer, orig_item: i32, regs: Option<&Registries>) -> Result<()> {
        let added = r.varint()?;
        let removed = r.varint()?;
        if added < 0 || removed < 0 || added + removed > 512 {
            return Err(err("bad component counts"));
        }
        let mut outs: Vec<Out> = Vec::new();
        let mut hashes: Vec<(i32, i32)> = Vec::new();
        let mut backup: Vec<(&'static str, Vec<u8>)> = Vec::new();
        let mut tooltip: Option<(bool, Vec<&'static str>)> = None;
        let mut tropical: [Option<i32>; 3] = [None; 3];
        for _ in 0..added {
            let ty = r.varint()?;
            let name = self.source_component_name(ty).ok_or_else(|| err(&format!("unknown data component type {ty}")))?;
            let rest = r.remaining();
            let start = r.position();
            let mut v = Writer::new();
            let kind = self.value(name, r, &mut v)?;
            let raw = &rest[..r.position() - start];
            if let Some(regs) = regs {
                hashes.push((ty, component_hash(name, raw, regs).unwrap_or(UNKNOWN_HASH)));
            }
            if self.target_component_id(name).is_none() || LOSSY.contains(&name) {
                backup.push((name, raw.to_vec()));
            }
            match kind {
                Kind::Keep => outs.push(Out { name, bytes: v.buf, show: None }),
                Kind::KeepShow(s) => outs.push(Out { name, bytes: v.buf, show: Some(s) }),
                Kind::Tooltip(hide, names) => tooltip = Some((hide, names)),
                Kind::Tropical(i, value) => tropical[i] = Some(value),
                _ => {}
            }
        }
        let mut removed_out = Vec::new();
        let mut removed_new = Vec::new();
        for _ in 0..removed {
            let ty = r.varint()?;
            removed_new.push(ty);
            if let Some(t) = self.component_type(ty) {
                removed_out.push(t);
            }
        }
        // Tooltip flags and markers.
        let (hide, hidden) = tooltip.unwrap_or((false, Vec::new()));
        if hide {
            outs.push(Out { name: "hide_tooltip", bytes: Vec::new(), show: None });
        }
        let additional: Vec<&&str> = HIDE_ADDITIONAL_KEYS.iter().filter(|k| self.target_component_id(k).is_some()).collect();
        if !hidden.is_empty() && additional.iter().all(|k| hidden.contains(k)) {
            outs.push(Out { name: "hide_additional_tooltip", bytes: Vec::new(), show: None });
        }
        for o in &mut outs {
            if let Some(s) = o.show.take() {
                o.bytes.push(u8::from(!hidden.contains(&s)));
            }
        }
        // Tropical fish buckets: 1.21.4 packs the variant into the bucket entity data.
        if tropical.iter().any(Option::is_some) {
            let base = tropical[1].unwrap_or(16_777_215);
            let pattern_color = tropical[2].unwrap_or(16_777_215);
            let packed = ((pattern_color & 0xff) << 24) | ((base & 0xff) << 16) | (tropical[0].unwrap_or(0) & 0xffff);
            edit_nbt_component(&mut outs, "bucket_entity_data", |tag| tag.insert("BucketVariantTag", Tag::Int(packed)));
        }
        // Backups and hashes ride along in custom_data.
        let track = regs.is_some() && (added > 0 || removed > 0);
        if track || !backup.is_empty() {
            edit_nbt_component(&mut outs, "custom_data", |tag| {
                if track {
                    let mut h = vec![("id".to_string(), Tag::Int(orig_item))];
                    for (ty, hash) in &hashes {
                        h.push((ty.to_string(), Tag::Int(*hash)));
                    }
                    h.push(("removed".to_string(), Tag::IntArray(removed_new.clone())));
                    tag.insert(HASHES_KEY, Tag::Compound(h));
                }
                if !backup.is_empty() {
                    let b = backup.iter().map(|(n, bytes)| (n.to_string(), Tag::ByteArray(bytes.iter().map(|b| *b as i8).collect()))).collect();
                    tag.insert(BACKUP_KEY, Tag::Compound(b));
                }
            });
        }
        let outs: Vec<Out> = outs.into_iter().filter(|o| self.target_component_id(o.name).is_some()).collect();
        w.varint(outs.len() as i32);
        w.varint(removed_out.len() as i32);
        for o in &outs {
            w.varint(self.target_component_id(o.name).unwrap_or(0));
            w.raw(&o.bytes);
        }
        for t in removed_out {
            w.varint(t);
        }
        Ok(())
    }

    /// Converts a 1.21.4 patch to 1.21.5. Returns the hashes stored on the item, if any.
    fn patch_to_server(&self, r: &mut Reader, w: &mut Writer, length_prefixed: bool) -> Result<Option<StoredHashes>> {
        let added = r.varint()?;
        let removed = r.varint()?;
        if added < 0 || removed < 0 || added + removed > 512 {
            return Err(err("bad component counts"));
        }
        let mut outs: Vec<(i32, Vec<u8>)> = Vec::new();
        let mut hidden: Vec<&'static str> = Vec::new();
        let mut hide = false;
        let mut hide_additional = false;
        let mut stored: Option<StoredHashes> = None;
        let mut backup: Vec<(String, Vec<u8>)> = Vec::new();
        for _ in 0..added {
            let ty = r.varint()?;
            let name = self.source_component_name(ty).ok_or_else(|| err(&format!("unknown data component type {ty}")))?;
            let mut v = Writer::new();
            if name == "custom_data" {
                let mut tag = nbt::read(r)?;
                stored = take_stored_hashes(&mut tag);
                backup = take_backup(&mut tag);
                let created = tag.remove(MARKER_KEY).is_some();
                if created && matches!(&tag, Tag::Compound(e) if e.is_empty()) {
                    continue;
                }
                nbt::write(&mut v, &tag);
            } else {
                match self.value(name, r, &mut v)? {
                    Kind::Shown(s, show) => {
                        if !show {
                            hidden.push(s);
                        }
                    }
                    Kind::HideTooltip => {
                        hide = true;
                        continue;
                    }
                    Kind::HideAdditional => {
                        hide_additional = true;
                        continue;
                    }
                    Kind::Drop => continue,
                    _ => {}
                }
            }
            if let Some(t) = self.target_component_id(name) {
                outs.push((t, v.buf));
            }
        }
        // Tropical fish bucket variant → 1.21.5 components.
        if let Some(bucket) = self.target_component_id("bucket_entity_data") {
            if let Some(pos) = outs.iter().position(|(t, _)| *t == bucket) {
                let mut tag = nbt::read(&mut Reader::new(&outs[pos].1))?;
                if let Some(packed) = tag.remove("BucketVariantTag").and_then(|t| t.as_i32()) {
                    let mut b = Writer::new();
                    nbt::write(&mut b, &tag);
                    outs[pos].1 = b.buf;
                    for (name, value) in [("tropical_fish/base_color", (packed >> 16) & 0xff), ("tropical_fish/pattern_color", (packed >> 24) & 0xff), ("tropical_fish/pattern", packed & 0xffff)] {
                        if let Some(t) = self.target_component_id(name) {
                            let mut v = Writer::new();
                            v.varint(value);
                            outs.push((t, v.buf));
                        }
                    }
                }
            }
        }
        // Restore what the old client couldn't hold.
        let mut restored_tooltip = false;
        for (name, bytes) in backup {
            if let Some(t) = self.target_component_id(&name) {
                outs.retain(|(id, _)| *id != t);
                outs.push((t, bytes));
                restored_tooltip |= name == "tooltip_display";
            }
        }
        if !restored_tooltip && (hide || hide_additional || !hidden.is_empty()) {
            if let Some(t) = self.target_component_id("tooltip_display") {
                let mut ids: Vec<i32> = hidden.iter().filter_map(|n| self.target_component_id(n)).collect();
                if hide_additional {
                    ids.extend(HIDE_ADDITIONAL_KEYS.iter().filter_map(|n| self.target_component_id(n)));
                }
                ids.sort_unstable();
                ids.dedup();
                let mut v = Writer::new();
                v.bool(hide);
                v.varint(ids.len() as i32);
                for id in ids {
                    v.varint(id);
                }
                outs.push((t, v.buf));
            }
        }
        let mut removed_out = Vec::new();
        for _ in 0..removed {
            let ty = r.varint()?;
            if let Some(t) = self.component_type(ty) {
                removed_out.push(t);
            }
        }
        w.varint(outs.len() as i32);
        w.varint(removed_out.len() as i32);
        for (t, bytes) in outs {
            w.varint(t);
            if length_prefixed {
                w.varint(bytes.len() as i32);
            }
            w.raw(&bytes);
        }
        for t in removed_out {
            w.varint(t);
        }
        Ok(stored)
    }

    /// Reads one component value named `name` (source version) and writes the target form.
    fn value(&self, name: &'static str, r: &mut Reader, w: &mut Writer) -> Result<Kind> {
        let to_client = self.dir == Dir::ToClient;
        macro_rules! varint { () => {{ Self::copy_varint(r, w)?; }}; }
        macro_rules! f32 { () => {{ Self::copy_bytes(r, w, 4)?; }}; }
        macro_rules! bool { () => {{ Self::copy_bool(r, w)?; }}; }
        macro_rules! string { () => {{ Self::copy_string(r, w)?; }}; }
        macro_rules! nbt { () => {{ Self::copy_nbt(r, w)?; }}; }
        // Trailing show_in_tooltip flag: dropped going to the client (re-added from
        // tooltip_display), read going to the server.
        macro_rules! show {
            ($n:expr) => {{
                if to_client { Ok(Kind::KeepShow($n)) } else { Ok(Kind::Shown($n, r.bool()?)) }
            }};
        }
        match name {
            "max_stack_size" | "max_damage" | "damage" | "repair_cost" | "map_id" | "map_post_processing" | "ominous_bottle_amplifier"
            | "enchantable" | "rarity" | "base_color" => {
                varint!();
                Ok(Kind::Keep)
            }
            "custom_data" | "map_decorations" | "debug_stick_state" | "entity_data" | "bucket_entity_data" | "block_entity_data"
            | "recipes" | "lock" | "container_loot" | "intangible_projectile" => {
                nbt!();
                Ok(Kind::Keep)
            }
            "custom_name" | "item_name" => {
                self.text(r, w)?;
                Ok(Kind::Keep)
            }
            "lore" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    self.text(r, w)?;
                }
                Ok(Kind::Keep)
            }
            "item_model" | "tooltip_style" | "note_block_sound" | "damage_resistant" => {
                string!();
                Ok(Kind::Keep)
            }
            "creative_slot_lock" | "glider" => Ok(Kind::Keep),
            "hide_tooltip" => Ok(Kind::HideTooltip),
            "hide_additional_tooltip" => Ok(Kind::HideAdditional),
            "unbreakable" => show!("unbreakable"),
            "enchantment_glint_override" => {
                bool!();
                Ok(Kind::Keep)
            }
            "enchantments" | "stored_enchantments" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    varint!();
                    varint!();
                }
                show!(name)
            }
            "can_place_on" | "can_break" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    self.block_predicate(r, w)?;
                }
                show!(name)
            }
            "attribute_modifiers" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    let attr = r.varint()?;
                    w.varint(self.attribute(attr));
                    string!();
                    Self::copy_bytes(r, w, 8)?;
                    varint!();
                    varint!();
                }
                show!("attribute_modifiers")
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
                Ok(Kind::Keep)
            }
            "tooltip_display" => {
                let hide = r.bool()?;
                let n = r.varint()?;
                let mut names = Vec::new();
                for _ in 0..n {
                    if let Some(name) = d::NEW_COMPONENT_NAMES.get(r.varint()?.max(0) as usize) {
                        names.push(*name);
                    }
                }
                Ok(Kind::Tooltip(hide, names))
            }
            "food" => {
                varint!();
                f32!();
                bool!();
                Ok(Kind::Keep)
            }
            "consumable" => {
                f32!();
                varint!();
                self.sound_event(r, w)?;
                bool!();
                self.consume_effects(r, w)?;
                Ok(Kind::Keep)
            }
            "death_protection" => {
                self.consume_effects(r, w)?;
                Ok(Kind::Keep)
            }
            "use_remainder" => {
                self.item_stack(r, w)?;
                Ok(Kind::Keep)
            }
            "use_cooldown" => {
                f32!();
                Self::copy_opt_string(r, w)?;
                Ok(Kind::Keep)
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
                if to_client {
                    r.bool()?; // can_destroy_blocks_in_creative: 1.21.5 only
                } else {
                    w.bool(true);
                }
                Ok(Kind::Keep)
            }
            "equippable" => {
                varint!();
                self.sound_event(r, w)?;
                Self::copy_opt_string(r, w)?;
                Self::copy_opt_string(r, w)?;
                self.opt_holder_set(r, w, Reg::Entity)?;
                for _ in 0..3 {
                    bool!();
                }
                if to_client {
                    r.bool()?; // equip_on_interact: 1.21.5 only
                } else {
                    w.bool(false);
                }
                Ok(Kind::Keep)
            }
            "repairable" => {
                self.holder_set(r, w, Reg::Item)?;
                Ok(Kind::Keep)
            }
            "dyed_color" => {
                Self::copy_bytes(r, w, 4)?;
                show!("dyed_color")
            }
            "map_color" => {
                Self::copy_bytes(r, w, 4)?;
                Ok(Kind::Keep)
            }
            "charged_projectiles" | "bundle_contents" | "container" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    self.item_stack(r, w)?;
                }
                Ok(Kind::Keep)
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
                Ok(Kind::Keep)
            }
            "suspicious_stew_effects" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    varint!();
                    varint!();
                }
                Ok(Kind::Keep)
            }
            "writable_book_content" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    string!();
                    Self::copy_opt_string(r, w)?;
                }
                Ok(Kind::Keep)
            }
            "written_book_content" => {
                string!();
                Self::copy_opt_string(r, w)?;
                string!();
                varint!();
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    self.text(r, w)?;
                    self.opt_text(r, w)?;
                }
                bool!();
                Ok(Kind::Keep)
            }
            "trim" => {
                self.holder(r, w, Reg::Same, |s, r, w| {
                    Self::copy_string(r, w)?; // asset name
                    if s.dir == Dir::ToClient {
                        w.varint(0); // ingredient item: 1.21.4 only
                    } else {
                        r.varint()?;
                    }
                    let n = Self::copy_varint(r, w)?;
                    for _ in 0..n {
                        Self::copy_string(r, w)?;
                        Self::copy_string(r, w)?;
                    }
                    s.text(r, w)
                })?;
                self.holder(r, w, Reg::Same, |s, r, w| {
                    Self::copy_string(r, w)?;
                    if s.dir == Dir::ToClient {
                        w.varint(0); // template item: 1.21.4 only
                    } else {
                        r.varint()?;
                    }
                    s.text(r, w)?;
                    Self::copy_bool(r, w)?;
                    Ok(())
                })?;
                show!("trim")
            }
            "instrument" => {
                if to_client {
                    // 1.21.5: either a holder or a registry key; 1.21.4 only takes the holder.
                    if !r.bool()? {
                        r.string()?;
                        return Ok(Kind::Drop);
                    }
                } else {
                    w.bool(true);
                }
                self.holder(r, w, Reg::Same, |s, r, w| {
                    s.sound_event(r, w)?;
                    Self::copy_bytes(r, w, 8)?;
                    s.text(r, w)
                })?;
                Ok(Kind::Keep)
            }
            "jukebox_playable" => {
                if Self::copy_bool(r, w)? {
                    self.holder(r, w, Reg::Same, |s, r, w| {
                        s.sound_event(r, w)?;
                        s.text(r, w)?;
                        Self::copy_bytes(r, w, 4)?;
                        Self::copy_varint(r, w)?;
                        Ok(())
                    })?;
                } else {
                    string!();
                }
                show!("jukebox_playable")
            }
            "lodestone_tracker" => {
                if Self::copy_bool(r, w)? {
                    string!();
                    Self::copy_bytes(r, w, 8)?;
                }
                bool!();
                Ok(Kind::Keep)
            }
            "firework_explosion" => {
                self.firework_explosion(r, w)?;
                Ok(Kind::Keep)
            }
            "fireworks" => {
                varint!();
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    self.firework_explosion(r, w)?;
                }
                Ok(Kind::Keep)
            }
            "profile" => {
                Self::copy_opt_string(r, w)?;
                if Self::copy_bool(r, w)? {
                    Self::copy_bytes(r, w, 16)?;
                }
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    string!();
                    string!();
                    Self::copy_opt_string(r, w)?;
                }
                Ok(Kind::Keep)
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
                Ok(Kind::Keep)
            }
            "pot_decorations" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    let id = r.varint()?;
                    w.varint(self.item(id));
                }
                Ok(Kind::Keep)
            }
            "block_state" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    string!();
                    string!();
                }
                Ok(Kind::Keep)
            }
            "bees" => {
                let n = Self::copy_varint(r, w)?;
                for _ in 0..n {
                    nbt!();
                    varint!();
                    varint!();
                }
                Ok(Kind::Keep)
            }
            // ---- 1.21.5-only components: consumed, then dropped (backed up by the caller) ----
            "tropical_fish/pattern" => Ok(Kind::Tropical(0, r.varint()?)),
            "tropical_fish/base_color" => Ok(Kind::Tropical(1, r.varint()?)),
            "tropical_fish/pattern_color" => Ok(Kind::Tropical(2, r.varint()?)),
            "villager/variant" | "wolf/variant" | "wolf/sound_variant" | "wolf/collar" | "fox/variant" | "salmon/size"
            | "parrot/variant" | "mooshroom/variant" | "rabbit/variant" | "pig/variant" | "cow/variant" | "frog/variant"
            | "horse/variant" | "llama/variant" | "axolotl/variant" | "cat/variant" | "cat/collar" | "sheep/color"
            | "shulker/color" => {
                r.varint()?;
                Ok(Kind::Drop)
            }
            "chicken/variant" => {
                if r.bool()? {
                    r.varint()?;
                } else {
                    r.string()?;
                }
                Ok(Kind::Drop)
            }
            "painting/variant" => {
                let mut scratch = Writer::new();
                self.holder(r, &mut scratch, Reg::Same, |_, r, w| {
                    Self::copy_varint(r, w)?;
                    Self::copy_varint(r, w)?;
                    Self::copy_string(r, w)?;
                    Self::copy_opt_nbt(r, w)?;
                    Self::copy_opt_nbt(r, w)
                })?;
                Ok(Kind::Drop)
            }
            "potion_duration_scale" => {
                r.bytes(4)?;
                Ok(Kind::Drop)
            }
            "weapon" => {
                r.varint()?;
                r.bytes(4)?;
                Ok(Kind::Drop)
            }
            "provides_banner_patterns" => {
                r.string()?;
                Ok(Kind::Drop)
            }
            "break_sound" => {
                self.sound_event(r, &mut Writer::new())?;
                Ok(Kind::Drop)
            }
            "provides_trim_material" => {
                if r.bool()? {
                    let mut scratch = Writer::new();
                    self.holder(r, &mut scratch, Reg::Same, |_, r, w| {
                        Self::copy_string(r, w)?;
                        let n = Self::copy_varint(r, w)?;
                        for _ in 0..n {
                            Self::copy_string(r, w)?;
                            Self::copy_string(r, w)?;
                        }
                        Self::copy_nbt(r, w)
                    })?;
                } else {
                    r.string()?;
                }
                Ok(Kind::Drop)
            }
            "blocks_attacks" => {
                let mut s = Writer::new();
                Self::copy_bytes(r, &mut s, 8)?;
                let n = r.varint()?;
                for _ in 0..n {
                    r.bytes(4)?;
                    self.opt_holder_set(r, &mut s, Reg::Same)?;
                    r.bytes(8)?;
                }
                r.bytes(12)?;
                Self::copy_opt_string(r, &mut s)?;
                self.opt_sound_event(r, &mut s)?;
                self.opt_sound_event(r, &mut s)?;
                Ok(Kind::Drop)
            }
            other => Err(err(&format!("no codec for data component {other}"))),
        }
    }

    /// 1.21.5 block predicate has data component matchers after the NBT; 1.21.4 doesn't.
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
        if self.dir == Dir::ToClient {
            let exact = r.varint()?;
            for _ in 0..exact {
                let ty = r.varint()?;
                let name = self.source_component_name(ty).ok_or_else(|| err("unknown component in block predicate"))?;
                self.value(name, r, &mut Writer::new())?;
            }
            let partial = r.varint()?;
            for _ in 0..partial {
                r.varint()?;
                nbt::skip(r)?;
            }
        } else {
            w.varint(0);
            w.varint(0);
        }
        Ok(())
    }

    fn consume_effects(&self, r: &mut Reader, w: &mut Writer) -> Result<()> {
        let n = Self::copy_varint(r, w)?;
        for _ in 0..n {
            match Self::copy_varint(r, w)? {
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
            Self::copy_varint(r, w)?;
            Self::potion_effect_data(r, w)?;
        }
        Ok(())
    }

    fn potion_effect_data(r: &mut Reader, w: &mut Writer) -> Result<()> {
        Self::copy_varint(r, w)?;
        Self::copy_varint(r, w)?;
        Self::copy_bytes(r, w, 3)?;
        if Self::copy_bool(r, w)? {
            Self::potion_effect_data(r, w)?;
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
}

/// Hashes the server stored on an item, read back from custom_data.
pub struct StoredHashes {
    pub id: i32,
    pub hashes: Vec<(i32, i32)>,
    pub removed: Vec<i32>,
}

fn take_stored_hashes(tag: &mut Tag) -> Option<StoredHashes> {
    let h = tag.remove(HASHES_KEY)?;
    let Tag::Compound(entries) = h else { return None };
    let mut out = StoredHashes { id: 0, hashes: Vec::new(), removed: Vec::new() };
    for (k, v) in entries {
        match (k.as_str(), v) {
            ("id", Tag::Int(id)) => out.id = id,
            ("removed", Tag::IntArray(ids)) => out.removed = ids,
            (k, Tag::Int(hash)) => {
                if let Ok(ty) = k.parse::<i32>() {
                    out.hashes.push((ty, hash));
                }
            }
            _ => {}
        }
    }
    Some(out)
}

fn take_backup(tag: &mut Tag) -> Vec<(String, Vec<u8>)> {
    match tag.remove(BACKUP_KEY) {
        Some(Tag::Compound(entries)) => entries
            .into_iter()
            .filter_map(|(k, v)| match v {
                Tag::ByteArray(b) => Some((k, b.into_iter().map(|x| x as u8).collect())),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Edits (or creates) an NBT-valued component among the converted outputs.
fn edit_nbt_component(outs: &mut Vec<Out>, name: &'static str, f: impl FnOnce(&mut Tag)) {
    let existing = outs.iter().position(|o| o.name == name);
    let mut tag = existing
        .and_then(|i| nbt::read(&mut Reader::new(&outs[i].bytes)).ok())
        .filter(|t| matches!(t, Tag::Compound(_)))
        .unwrap_or_else(|| {
            let mut t = Tag::compound();
            if name == "custom_data" {
                t.insert(MARKER_KEY, Tag::Byte(1));
            }
            t
        });
    f(&mut tag);
    let mut w = Writer::new();
    nbt::write(&mut w, &tag);
    match existing {
        Some(i) => outs[i].bytes = w.buf,
        None => outs.push(Out { name, bytes: w.buf, show: None }),
    }
}

// ---- component hashes (1.21.5 values) -------------------------------------------------------

const DYE: &[&str] = &["white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray", "light_gray", "cyan", "purple", "blue", "brown", "green", "red", "black"];
const RARITY: &[&str] = &["common", "uncommon", "rare", "epic"];
const EQUIPMENT_SLOT: &[&str] = &["mainhand", "feet", "legs", "chest", "head", "offhand", "body", "saddle"];
const SLOT_GROUPS: &[&str] = &["any", "mainhand", "offhand", "hand", "feet", "legs", "chest", "head", "armor", "body", "saddle"];
const OPERATION: &[&str] = &["add_value", "add_multiplied_base", "add_multiplied_total"];
const USE_ANIMATION: &[&str] = &["none", "eat", "drink", "block", "bow", "spear", "crossbow", "spyglass", "toot_horn", "brush", "bundle"];
const SHAPES: &[&str] = &["small_ball", "large_ball", "star", "creeper", "burst"];
const VILLAGER_TYPE: &[&str] = &["desert", "jungle", "plains", "savanna", "snow", "swamp", "taiga"];
const RABBIT: &[&str] = &["brown", "white", "black", "white_splotched", "gold", "salt"];
const TROPICAL_PATTERNS: &[(&str, i32)] = &[
    ("kob", 0), ("sunstreak", 1 << 8), ("snooper", 2 << 8), ("dasher", 3 << 8), ("brinely", 4 << 8), ("spotty", 5 << 8),
    ("flopper", 1), ("stripey", 1 | (1 << 8)), ("glitter", 1 | (2 << 8)), ("blockfish", 1 | (3 << 8)), ("betty", 1 | (4 << 8)), ("clayfish", 1 | (5 << 8)),
];
const POTIONS: &[&str] = &[
    "water", "mundane", "thick", "awkward", "night_vision", "long_night_vision", "invisibility", "long_invisibility", "leaping",
    "long_leaping", "strong_leaping", "fire_resistance", "long_fire_resistance", "swiftness", "long_swiftness", "strong_swiftness",
    "slowness", "long_slowness", "strong_slowness", "turtle_master", "long_turtle_master", "strong_turtle_master", "water_breathing",
    "long_water_breathing", "healing", "strong_healing", "harming", "strong_harming", "poison", "long_poison", "strong_poison",
    "regeneration", "long_regeneration", "strong_regeneration", "strength", "long_strength", "strong_strength", "weakness",
    "long_weakness", "luck", "slow_falling", "long_slow_falling", "wind_charged", "weaving", "oozing", "infested",
];
const MOB_EFFECTS: &[&str] = &[
    "speed", "slowness", "haste", "mining_fatigue", "strength", "instant_health", "instant_damage", "jump_boost", "nausea",
    "regeneration", "resistance", "fire_resistance", "water_breathing", "invisibility", "blindness", "night_vision", "hunger",
    "weakness", "poison", "wither", "health_boost", "absorption", "saturation", "glowing", "levitation", "luck", "unluck",
    "slow_falling", "conduit_power", "dolphins_grace", "bad_omen", "hero_of_the_village", "darkness", "trial_omen", "raid_omen",
    "wind_charged", "weaving", "oozing", "infested",
];

fn enum_name(names: &[&str], id: i32, clamp: bool) -> Hv {
    let i = if clamp { id.clamp(0, names.len() as i32 - 1) } else if id < 0 || id as usize >= names.len() { 0 } else { id };
    Hv::Str(names[i as usize].to_string())
}

fn named<'a>(names: &'a [&'a str], id: i32) -> Option<&'a str> {
    names.get(usize::try_from(id).ok()?).copied()
}

/// Hash of one 1.21.5 component value, `None` if this plugin can't encode it.
pub fn component_hash(name: &str, raw: &[u8], regs: &Registries) -> Option<i32> {
    let mut r = Reader::new(raw);
    let v = component_hv(name, &mut r, regs)?;
    Some(hash::hash(&v))
}

fn tag_hv(tag: &Tag, text: bool, key: Option<&str>) -> Hv {
    const BOOLEAN_KEYS: &[&str] = &["bold", "italic", "underlined", "strikethrough", "obfuscated", "interpret"];
    match tag {
        Tag::End => Hv::Nothing,
        Tag::Byte(b) => {
            if text && key.is_some_and(|k| BOOLEAN_KEYS.contains(&k)) { Hv::Bool(*b != 0) } else { Hv::Byte(*b) }
        }
        Tag::Short(v) => Hv::Short(*v),
        Tag::Int(v) => Hv::Int(*v),
        Tag::Long(v) => Hv::Long(*v),
        Tag::Float(v) => Hv::Float(*v),
        Tag::Double(v) => Hv::Double(*v),
        Tag::ByteArray(v) => Hv::Bytes(v.clone()),
        Tag::String(s) => Hv::Str(s.clone()),
        Tag::List(items) => Hv::List(items.iter().map(|t| tag_hv(t, text, None)).collect()),
        Tag::Compound(entries) => Hv::Map(entries.iter().map(|(k, v)| (Hv::Str(k.clone()), tag_hv(v, text, Some(k)))).collect()),
        Tag::IntArray(v) => Hv::Ints(v.clone()),
        Tag::LongArray(v) => Hv::Longs(v.clone()),
    }
}

fn read_tag_hv(r: &mut Reader, text: bool) -> Option<Hv> {
    Some(tag_hv(&nbt::read(r).ok()?, text, None))
}

fn opt<T>(r: &mut Reader, f: impl FnOnce(&mut Reader) -> Option<T>) -> Option<Option<T>> {
    if r.bool().ok()? { f(r).map(Some) } else { Some(None) }
}

fn int_hv(r: &mut Reader) -> Option<Hv> {
    Some(Hv::Int(r.varint().ok()?))
}

fn registry_ident(regs: &Registries, registry: &str, id: i32) -> Option<Hv> {
    Some(ident(regs.key(registry, id)?))
}

fn sound_hv(r: &mut Reader) -> Option<Hv> {
    let v = r.varint().ok()?;
    if v == 0 {
        let id = r.string().ok()?.to_string();
        let range = opt(r, |r| r.f32().ok())?;
        Some(MapB::new().put("sound_id", ident(&id)).opt("range", range.map(Hv::Float)).build())
    } else {
        Some(ident(named(&d::NEW_SOUND_NAMES, v - 1)?))
    }
}

/// Sound holder plus whether it is the registry entry `default` (for optional fields).
fn sound_hv_is(r: &mut Reader, default: &str) -> Option<(Hv, bool)> {
    let start = r.position();
    let v = r.varint().ok()?;
    let is_default = v > 0 && named(&d::NEW_SOUND_NAMES, v - 1) == Some(default);
    r.set_position(start);
    Some((sound_hv(r)?, is_default))
}

fn holder_set_hv(r: &mut Reader, names: &[&str]) -> Option<Hv> {
    let v = r.varint().ok()?;
    if v == 0 {
        return Some(Hv::Str(format!("#{}", full_key(r.string().ok()?))));
    }
    let ids: Vec<i32> = (0..v - 1).map(|_| r.varint()).collect::<Result<_>>().ok()?;
    if ids.len() == 1 {
        return Some(ident(named(names, ids[0])?));
    }
    Some(Hv::List(ids.iter().map(|id| named(names, *id).map(ident)).collect::<Option<_>>()?))
}

fn item_hv(r: &mut Reader, regs: &Registries) -> Option<Option<Hv>> {
    let count = r.varint().ok()?;
    if count <= 0 {
        return Some(None);
    }
    let id = r.varint().ok()?;
    let comps = patch_hv(r, regs)?;
    let mut m = MapB::new().put("id", ident(named(&d::NEW_ITEM_NAMES, id)?)).put("count", Hv::Int(count));
    if let Some(c) = comps {
        m = m.put("components", c);
    }
    Some(Some(m.build()))
}

fn patch_hv(r: &mut Reader, regs: &Registries) -> Option<Option<Hv>> {
    let added = r.varint().ok()?;
    let removed = r.varint().ok()?;
    if added == 0 && removed == 0 {
        return Some(None);
    }
    let mut entries = Vec::new();
    for _ in 0..added {
        let name = named(&d::NEW_COMPONENT_NAMES, r.varint().ok()?)?;
        let v = component_hv(name, r, regs)?;
        entries.push((Hv::Str(format!("minecraft:{name}")), v));
    }
    for _ in 0..removed {
        let name = named(&d::NEW_COMPONENT_NAMES, r.varint().ok()?)?;
        entries.push((Hv::Str(format!("!minecraft:{name}")), Hv::Nothing));
    }
    Some(Some(Hv::Map(entries)))
}

fn filterable_hv(r: &mut Reader, value: impl Fn(&mut Reader) -> Option<Hv>) -> Option<Hv> {
    let raw = value(r)?;
    let filtered = opt(r, &value)?;
    Some(MapB::new().put("raw", raw).opt("filtered", filtered).build())
}

fn potion_effect_data_hv(r: &mut Reader) -> Option<Vec<(Hv, Hv)>> {
    let amplifier = r.varint().ok()?;
    let duration = r.varint().ok()?;
    let ambient = r.bool().ok()?;
    let particles = r.bool().ok()?;
    let icon = r.bool().ok()?;
    let hidden = opt(r, |r| potion_effect_data_hv(r).map(Hv::Map))?;
    Some(
        MapB::new()
            .def("amplifier", Hv::Byte(amplifier as u8 as i8), &Hv::Byte(0))
            .def("duration", Hv::Int(duration), &Hv::Int(0))
            .def("ambient", Hv::Bool(ambient), &Hv::Bool(false))
            .def("show_particles", Hv::Bool(particles), &Hv::Bool(true))
            .put("show_icon", Hv::Bool(icon))
            .opt("hidden_effect", hidden)
            .0,
    )
}

fn potion_effect_hv(r: &mut Reader) -> Option<Hv> {
    let id = r.varint().ok()?;
    let mut entries = vec![(Hv::Str("id".into()), ident(named(MOB_EFFECTS, id)?))];
    entries.extend(potion_effect_data_hv(r)?);
    Some(Hv::Map(entries))
}

fn list_hv(r: &mut Reader, mut f: impl FnMut(&mut Reader) -> Option<Hv>) -> Option<Vec<Hv>> {
    let n = r.varint().ok()?;
    (0..n).map(|_| f(r)).collect()
}

fn either_holder_hv(r: &mut Reader, regs: &Registries, registry: &str, direct: impl FnOnce(&mut Reader) -> Option<Hv>) -> Option<Hv> {
    if r.bool().ok()? {
        let v = r.varint().ok()?;
        if v == 0 { direct(r) } else { registry_ident(regs, registry, v - 1) }
    } else {
        Some(ident(r.string().ok()?))
    }
}

fn trim_material_hv(r: &mut Reader) -> Option<Hv> {
    let asset = r.string().ok()?.to_string();
    let n = r.varint().ok()?;
    let mut overrides = Vec::new();
    for _ in 0..n {
        let k = r.string().ok()?.to_string();
        let v = r.string().ok()?.to_string();
        overrides.push((ident(&k), Hv::Str(v)));
    }
    let desc = read_tag_hv(r, true)?;
    let mut m = MapB::new().put("asset_name", Hv::Str(asset));
    if !overrides.is_empty() {
        m = m.put("override_armor_assets", Hv::Map(overrides));
    }
    Some(m.put("description", desc).build())
}

fn empty_list() -> Hv {
    Hv::List(Vec::new())
}

fn component_hv(name: &str, r: &mut Reader, regs: &Registries) -> Option<Hv> {
    Some(match name {
        "custom_data" | "map_decorations" | "debug_stick_state" | "entity_data" | "bucket_entity_data" | "block_entity_data"
        | "lock" | "container_loot" | "recipes" | "intangible_projectile" => read_tag_hv(r, false)?,
        "custom_name" | "item_name" => read_tag_hv(r, true)?,
        "lore" => Hv::List(list_hv(r, |r| read_tag_hv(r, true))?),
        "max_stack_size" | "max_damage" | "damage" | "repair_cost" | "map_id" | "map_post_processing" | "ominous_bottle_amplifier" => int_hv(r)?,
        "enchantable" => MapB::new().put("value", int_hv(r)?).build(),
        "rarity" => enum_name(RARITY, r.varint().ok()?, false),
        "base_color" | "wolf/collar" | "cat/collar" | "sheep/color" | "shulker/color" | "tropical_fish/base_color"
        | "tropical_fish/pattern_color" => enum_name(DYE, r.varint().ok()?, false),
        "fox/variant" => enum_name(&["red", "snow"], r.varint().ok()?, false),
        "salmon/size" => enum_name(&["small", "medium", "large"], r.varint().ok()?, true),
        "parrot/variant" => enum_name(&["red_blue", "blue", "green", "yellow_blue", "gray"], r.varint().ok()?, true),
        "mooshroom/variant" => enum_name(&["red", "brown"], r.varint().ok()?, true),
        "horse/variant" => {
            let names = ["white", "creamy", "chestnut", "brown", "black", "gray", "dark_brown"];
            Hv::Str(names[r.varint().ok()?.rem_euclid(names.len() as i32) as usize].to_string())
        }
        "llama/variant" => enum_name(&["creamy", "white", "brown", "gray"], r.varint().ok()?, true),
        "axolotl/variant" => enum_name(&["lucy", "wild", "gold", "cyan", "blue"], r.varint().ok()?, false),
        "rabbit/variant" => {
            let v = r.varint().ok()?;
            Hv::Str(if v == 99 { "evil".to_string() } else { named(RABBIT, v).unwrap_or("brown").to_string() })
        }
        "villager/variant" => ident(named(VILLAGER_TYPE, r.varint().ok()?)?),
        "wolf/variant" => registry_ident(regs, "wolf_variant", r.varint().ok()?)?,
        "wolf/sound_variant" => registry_ident(regs, "wolf_sound_variant", r.varint().ok()?)?,
        "pig/variant" => registry_ident(regs, "pig_variant", r.varint().ok()?)?,
        "cow/variant" => registry_ident(regs, "cow_variant", r.varint().ok()?)?,
        "frog/variant" => registry_ident(regs, "frog_variant", r.varint().ok()?)?,
        "cat/variant" => registry_ident(regs, "cat_variant", r.varint().ok()?)?,
        "chicken/variant" => {
            if r.bool().ok()? { registry_ident(regs, "chicken_variant", r.varint().ok()?)? } else { ident(r.string().ok()?) }
        }
        "tropical_fish/pattern" => {
            let v = r.varint().ok()?;
            Hv::Str(TROPICAL_PATTERNS.iter().find(|(_, id)| *id == v).map_or("kob", |(n, _)| n).to_string())
        }
        "unbreakable" | "creative_slot_lock" | "glider" => Hv::Map(Vec::new()),
        "enchantment_glint_override" => Hv::Bool(r.bool().ok()?),
        "item_model" | "tooltip_style" | "note_block_sound" => ident(r.string().ok()?),
        "enchantments" | "stored_enchantments" => {
            let n = r.varint().ok()?;
            let mut entries = Vec::new();
            for _ in 0..n {
                let id = r.varint().ok()?;
                let level = r.varint().ok()?;
                entries.push((registry_ident(regs, "enchantment", id)?, Hv::Int(level)));
            }
            Hv::Map(entries)
        }
        "attribute_modifiers" => Hv::List(list_hv(r, |r| {
            let attr = r.varint().ok()?;
            let id = r.string().ok()?.to_string();
            let amount = r.f64().ok()?;
            let op = r.varint().ok()?;
            let slot = r.varint().ok()?;
            Some(
                MapB::new()
                    .put("type", ident(named(&d::NEW_ATTRIBUTE_NAMES, attr)?))
                    .put("id", ident(&id))
                    .put("amount", Hv::Double(amount))
                    .put("operation", Hv::Str(named(OPERATION, op)?.to_string()))
                    .def("slot", Hv::Str(named(SLOT_GROUPS, slot)?.to_string()), &Hv::Str("any".into()))
                    .build(),
            )
        })?),
        "custom_model_data" => {
            let floats = list_hv(r, |r| r.f32().ok().map(Hv::Float))?;
            let flags = list_hv(r, |r| r.bool().ok().map(Hv::Bool))?;
            let strings = list_hv(r, |r| r.string().ok().map(|s| Hv::Str(s.to_string())))?;
            let colors = list_hv(r, |r| r.i32().ok().map(Hv::Int))?;
            MapB::new()
                .def("floats", Hv::List(floats), &empty_list())
                .def("flags", Hv::List(flags), &empty_list())
                .def("strings", Hv::List(strings), &empty_list())
                .def("colors", Hv::List(colors), &empty_list())
                .build()
        }
        "tooltip_display" => {
            let hide = r.bool().ok()?;
            let hidden = list_hv(r, |r| named(&d::NEW_COMPONENT_NAMES, r.varint().ok()?).map(ident))?;
            MapB::new().def("hide_tooltip", Hv::Bool(hide), &Hv::Bool(false)).def("hidden_components", Hv::List(hidden), &empty_list()).build()
        }
        "food" => {
            let nutrition = int_hv(r)?;
            let saturation = Hv::Float(r.f32().ok()?);
            let always = Hv::Bool(r.bool().ok()?);
            MapB::new().put("nutrition", nutrition).put("saturation", saturation).def("can_always_eat", always, &Hv::Bool(false)).build()
        }
        "consumable" => {
            let seconds = r.f32().ok()?;
            let anim = r.varint().ok()?;
            let (sound, default_sound) = sound_hv_is(r, "entity.generic.eat")?;
            let particles = r.bool().ok()?;
            if r.varint().ok()? != 0 {
                return None; // consume effect encoding not implemented
            }
            let mut m = MapB::new().def("consume_seconds", Hv::Float(seconds), &Hv::Float(1.6));
            if anim != 1 {
                m = m.put("animation", Hv::Str(named(USE_ANIMATION, anim)?.to_string()));
            }
            if !default_sound {
                m = m.put("sound", sound);
            }
            m.def("has_consume_particles", Hv::Bool(particles), &Hv::Bool(true)).build()
        }
        "death_protection" => {
            if r.varint().ok()? != 0 {
                return None;
            }
            Hv::Map(Vec::new())
        }
        "use_remainder" => item_hv(r, regs)??,
        "use_cooldown" => {
            let seconds = Hv::Float(r.f32().ok()?);
            let group = opt(r, |r| r.string().ok().map(ident))?;
            MapB::new().put("seconds", seconds).opt("cooldown_group", group).build()
        }
        "damage_resistant" => MapB::new().put("types", Hv::Str(format!("#{}", full_key(r.string().ok()?)))).build(),
        "tool" => {
            let rules = list_hv(r, |r| {
                let blocks = holder_set_hv(r, &d::NEW_BLOCK_NAMES)?;
                let speed = opt(r, |r| r.f32().ok().map(Hv::Float))?;
                let drops = opt(r, |r| r.bool().ok().map(Hv::Bool))?;
                Some(MapB::new().put("blocks", blocks).opt("speed", speed).opt("correct_for_drops", drops).build())
            })?;
            let speed = Hv::Float(r.f32().ok()?);
            let damage = Hv::Int(r.varint().ok()?);
            let creative = Hv::Bool(r.bool().ok()?);
            MapB::new()
                .put("rules", Hv::List(rules))
                .def("default_mining_speed", speed, &Hv::Float(1.0))
                .def("damage_per_block", damage, &Hv::Int(1))
                .def("can_destroy_blocks_in_creative", creative, &Hv::Bool(true))
                .build()
        }
        "weapon" => {
            let dmg = Hv::Int(r.varint().ok()?);
            let disable = Hv::Float(r.f32().ok()?);
            MapB::new().def("item_damage_per_attack", dmg, &Hv::Int(1)).def("disable_blocking_for_seconds", disable, &Hv::Float(0.0)).build()
        }
        "equippable" => {
            let slot = Hv::Str(named(EQUIPMENT_SLOT, r.varint().ok()?)?.to_string());
            let (sound, default_sound) = sound_hv_is(r, "item.armor.equip_generic")?;
            let asset = opt(r, |r| r.string().ok().map(ident))?;
            let overlay = opt(r, |r| r.string().ok().map(ident))?;
            let entities = opt(r, |r| holder_set_hv(r, &d::NEW_ENTITY_NAMES))?;
            let mut m = MapB::new().put("slot", slot);
            if !default_sound {
                m = m.put("equip_sound", sound);
            }
            m.opt("asset_id", asset)
                .opt("camera_overlay", overlay)
                .opt("allowed_entities", entities)
                .def("dispensable", Hv::Bool(r.bool().ok()?), &Hv::Bool(true))
                .def("swappable", Hv::Bool(r.bool().ok()?), &Hv::Bool(true))
                .def("damage_on_hurt", Hv::Bool(r.bool().ok()?), &Hv::Bool(true))
                .def("equip_on_interact", Hv::Bool(r.bool().ok()?), &Hv::Bool(false))
                .build()
        }
        "repairable" => MapB::new().put("items", holder_set_hv(r, &d::NEW_ITEM_NAMES)?).build(),
        "dyed_color" | "map_color" => Hv::Int(r.i32().ok()?),
        "charged_projectiles" | "bundle_contents" => Hv::List(list_hv(r, |r| item_hv(r, regs).map(|i| i.unwrap_or(Hv::Nothing)))?),
        "container" => {
            let n = r.varint().ok()?;
            let mut slots = Vec::new();
            for i in 0..n {
                if let Some(item) = item_hv(r, regs)? {
                    slots.push(MapB::new().put("slot", Hv::Int(i)).put("item", item).build());
                }
            }
            Hv::List(slots)
        }
        "potion_contents" => {
            let potion = opt(r, |r| named(POTIONS, r.varint().ok()?).map(ident))?;
            let color = opt(r, |r| r.i32().ok().map(Hv::Int))?;
            let effects = list_hv(r, potion_effect_hv)?;
            let custom = opt(r, |r| r.string().ok().map(|s| Hv::Str(s.to_string())))?;
            MapB::new().opt("potion", potion).opt("custom_color", color).def("custom_effects", Hv::List(effects), &empty_list()).opt("custom_name", custom).build()
        }
        "potion_duration_scale" => Hv::Float(r.f32().ok()?),
        "suspicious_stew_effects" => Hv::List(list_hv(r, |r| {
            let id = ident(named(MOB_EFFECTS, r.varint().ok()?)?);
            let duration = Hv::Int(r.varint().ok()?);
            Some(MapB::new().put("id", id).def("duration", duration, &Hv::Int(160)).build())
        })?),
        "writable_book_content" => {
            let pages = list_hv(r, |r| filterable_hv(r, |r| r.string().ok().map(|s| Hv::Str(s.to_string()))))?;
            MapB::new().def("pages", Hv::List(pages), &empty_list()).build()
        }
        "written_book_content" => {
            let title = filterable_hv(r, |r| r.string().ok().map(|s| Hv::Str(s.to_string())))?;
            let author = Hv::Str(r.string().ok()?.to_string());
            let generation = Hv::Int(r.varint().ok()?);
            let pages = list_hv(r, |r| filterable_hv(r, |r| read_tag_hv(r, true)))?;
            let resolved = Hv::Bool(r.bool().ok()?);
            MapB::new()
                .put("title", title)
                .put("author", author)
                .def("generation", generation, &Hv::Int(0))
                .def("pages", Hv::List(pages), &empty_list())
                .def("resolved", resolved, &Hv::Bool(false))
                .build()
        }
        "trim" => {
            let mv = r.varint().ok()?;
            let material = if mv == 0 { trim_material_hv(r)? } else { registry_ident(regs, "trim_material", mv - 1)? };
            let pv = r.varint().ok()?;
            let pattern = if pv == 0 {
                let asset = ident(r.string().ok()?);
                let desc = read_tag_hv(r, true)?;
                let decal = Hv::Bool(r.bool().ok()?);
                MapB::new().put("asset_id", asset).put("description", desc).put("decal", decal).build()
            } else {
                registry_ident(regs, "trim_pattern", pv - 1)?
            };
            MapB::new().put("material", material).put("pattern", pattern).build()
        }
        "instrument" => either_holder_hv(r, regs, "instrument", |r| {
            let sound = sound_hv(r)?;
            let duration = Hv::Float(r.f32().ok()?);
            let range = Hv::Float(r.f32().ok()?);
            let desc = read_tag_hv(r, true)?;
            Some(MapB::new().put("sound_event", sound).put("use_duration", duration).put("range", range).put("description", desc).build())
        })?,
        "provides_trim_material" => either_holder_hv(r, regs, "trim_material", trim_material_hv)?,
        "jukebox_playable" => either_holder_hv(r, regs, "jukebox_song", |r| {
            let sound = sound_hv(r)?;
            let desc = read_tag_hv(r, false)?;
            let length = Hv::Float(r.f32().ok()?);
            let output = Hv::Int(r.varint().ok()?);
            Some(MapB::new().put("sound_event", sound).put("description", desc).put("length_in_seconds", length).put("comparator_output", output).build())
        })?,
        "provides_banner_patterns" => Hv::Str(format!("#{}", full_key(r.string().ok()?))),
        "lodestone_tracker" => {
            let target = opt(r, |r| {
                let dim = ident(r.string().ok()?);
                let p = r.i64().ok()?;
                let (x, y, z) = ((p >> 38) as i32, ((p << 52) >> 52) as i32, ((p << 26) >> 38) as i32);
                Some(MapB::new().put("dimension", dim).put("pos", Hv::Ints(vec![x, y, z])).build())
            })?;
            let tracked = Hv::Bool(r.bool().ok()?);
            MapB::new().opt("target", target).def("tracked", tracked, &Hv::Bool(true)).build()
        }
        "firework_explosion" => firework_hv(r)?,
        "fireworks" => {
            let flight = r.varint().ok()?;
            let explosions = list_hv(r, firework_hv)?;
            MapB::new().def("flight_duration", Hv::Byte(flight as u8 as i8), &Hv::Byte(0)).def("explosions", Hv::List(explosions), &empty_list()).build()
        }
        "profile" => {
            let name = opt(r, |r| r.string().ok().map(|s| Hv::Str(s.to_string())))?;
            let id = opt(r, |r| {
                let u = r.uuid().ok()?;
                Some(Hv::Ints(vec![(u >> 96) as i32, (u >> 64) as i32, (u >> 32) as i32, u as i32]))
            })?;
            let props = list_hv(r, |r| {
                let n = Hv::Str(r.string().ok()?.to_string());
                let v = Hv::Str(r.string().ok()?.to_string());
                let s = opt(r, |r| r.string().ok().map(|s| Hv::Str(s.to_string())))?;
                Some(MapB::new().put("name", n).put("value", v).opt("signature", s).build())
            })?;
            MapB::new().opt("name", name).opt("id", id).def("properties", Hv::List(props), &empty_list()).build()
        }
        "banner_patterns" => Hv::List(list_hv(r, |r| {
            let v = r.varint().ok()?;
            let pattern = if v == 0 {
                let asset = ident(r.string().ok()?);
                let key = Hv::Str(r.string().ok()?.to_string());
                MapB::new().put("asset_id", asset).put("translation_key", key).build()
            } else {
                registry_ident(regs, "banner_pattern", v - 1)?
            };
            let color = enum_name(DYE, r.varint().ok()?, false);
            Some(MapB::new().put("pattern", pattern).put("color", color).build())
        })?),
        "pot_decorations" => {
            let ids = list_hv(r, |r| named(&d::NEW_ITEM_NAMES, r.varint().ok()?).map(ident))?;
            Hv::List((0..4).map(|i| ids.get(i).cloned().unwrap_or_else(|| ident("brick"))).collect())
        }
        "block_state" => {
            let n = r.varint().ok()?;
            let mut entries = Vec::new();
            for _ in 0..n {
                let k = Hv::Str(r.string().ok()?.to_string());
                let v = Hv::Str(r.string().ok()?.to_string());
                entries.push((k, v));
            }
            Hv::Map(entries)
        }
        "bees" => Hv::List(list_hv(r, |r| {
            let data = read_tag_hv(r, false)?;
            let ticks = Hv::Int(r.varint().ok()?);
            let min = Hv::Int(r.varint().ok()?);
            Some(MapB::new().def("entity_data", data, &Hv::Map(Vec::new())).put("ticks_in_hive", ticks).put("min_ticks_in_hive", min).build())
        })?),
        "break_sound" => sound_hv(r)?,
        "painting/variant" => {
            let v = r.varint().ok()?;
            if v == 0 {
                let width = Hv::Int(r.varint().ok()?);
                let height = Hv::Int(r.varint().ok()?);
                let asset = ident(r.string().ok()?);
                let title = opt(r, |r| read_tag_hv(r, true))?;
                let author = opt(r, |r| read_tag_hv(r, true))?;
                MapB::new().put("width", width).put("height", height).put("asset_id", asset).opt("title", title).opt("author", author).build()
            } else {
                registry_ident(regs, "painting_variant", v - 1)?
            }
        }
        // Not encodable with ops in ViaVersion either (can_place_on/can_break) or not implemented.
        _ => return None,
    })
}

fn firework_hv(r: &mut Reader) -> Option<Hv> {
    let shape = Hv::Str(named(SHAPES, r.varint().ok()?)?.to_string());
    let colors = list_hv(r, |r| r.i32().ok().map(Hv::Int))?;
    let fades = list_hv(r, |r| r.i32().ok().map(Hv::Int))?;
    let trail = Hv::Bool(r.bool().ok()?);
    let twinkle = Hv::Bool(r.bool().ok()?);
    Some(
        MapB::new()
            .put("shape", shape)
            .def("colors", Hv::List(colors), &empty_list())
            .def("fade_colors", Hv::List(fades), &empty_list())
            .def("has_trail", trail, &Hv::Bool(false))
            .def("has_twinkle", twinkle, &Hv::Bool(false))
            .build(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comp_id(names: &[&str], name: &str) -> i32 {
        names.iter().position(|n| *n == name).unwrap() as i32
    }

    /// 1.21.5 stack with the given (component id, payload) entries.
    fn stack(item: i32, comps: &[(i32, Vec<u8>)]) -> Vec<u8> {
        let mut w = Writer::new();
        w.varint(1);
        w.varint(item);
        w.varint(comps.len() as i32);
        w.varint(0);
        for (id, bytes) in comps {
            w.varint(*id);
            w.raw(bytes);
        }
        w.buf
    }

    fn read_patch(r: &mut Reader) -> Vec<(i32, Vec<u8>)> {
        // Only for test outputs built from simple components.
        let added = r.varint().unwrap();
        let _removed = r.varint().unwrap();
        let mut out = Vec::new();
        for _ in 0..added {
            let id = r.varint().unwrap();
            let name = d::OLD_COMPONENT_NAMES[id as usize];
            let start = r.position();
            let mut scratch = Writer::new();
            Items::new(Dir::ToServer).value(name, r, &mut scratch).unwrap();
            let end = r.position();
            r.set_position(start);
            out.push((id, r.bytes(end - start).unwrap().to_vec()));
        }
        out
    }

    #[test]
    fn enchantments_get_show_flag_from_tooltip_display() {
        let items = Items::new(Dir::ToClient);
        let mut ench = Writer::new();
        ench.varint(1);
        ench.varint(5);
        ench.varint(2);
        let mut tooltip = Writer::new();
        tooltip.bool(false);
        tooltip.varint(1);
        tooltip.varint(comp_id(&d::NEW_COMPONENT_NAMES, "enchantments"));
        let input = stack(1, &[(comp_id(&d::NEW_COMPONENT_NAMES, "enchantments"), ench.buf), (comp_id(&d::NEW_COMPONENT_NAMES, "tooltip_display"), tooltip.buf)]);
        let mut out = Writer::new();
        items.item_stack(&mut Reader::new(&input), &mut out).unwrap();
        let mut r = Reader::new(&out.buf);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), d::ITEMS[1] as i32);
        let comps = read_patch(&mut r);
        let ench = comps.iter().find(|(id, _)| *id == comp_id(&d::OLD_COMPONENT_NAMES, "enchantments")).unwrap();
        assert_eq!(ench.1, vec![1, 5, 2, 0]); // hidden → show_in_tooltip false
        // tooltip_display was backed up into custom_data
        assert!(comps.iter().any(|(id, _)| *id == comp_id(&d::OLD_COMPONENT_NAMES, "custom_data")));
        assert!(r.is_empty());
    }

    #[test]
    fn round_trip_restores_backed_up_components() {
        let mut weapon = Writer::new();
        weapon.varint(3);
        weapon.f32(1.5);
        let weapon_id = comp_id(&d::NEW_COMPONENT_NAMES, "weapon");
        let input = stack(5, &[(weapon_id, weapon.buf.clone())]);
        let mut client = Writer::new();
        Items::new(Dir::ToClient).item_stack(&mut Reader::new(&input), &mut client).unwrap();
        let mut server = Writer::new();
        Items::new(Dir::ToServer).item_stack(&mut Reader::new(&client.buf), &mut server).unwrap();
        // custom_data held only our marker + backup, so it's gone; weapon is back.
        assert_eq!(server.buf, input);
    }

    #[test]
    fn container_click_reports_stored_hashes() {
        let mut regs = Registries::default();
        regs.set("minecraft:enchantment", vec!["minecraft:protection".into(), "minecraft:sharpness".into()]);
        let mut ench = Writer::new();
        ench.varint(1);
        ench.varint(1);
        ench.varint(3);
        let ench_id = comp_id(&d::NEW_COMPONENT_NAMES, "enchantments");
        let input = stack(7, &[(ench_id, ench.buf.clone())]);
        let mut client = Writer::new();
        Items::new(Dir::ToClient).item_stack_tracked(&mut Reader::new(&input), &mut client, Some(&regs)).unwrap();
        let mut hashed = Writer::new();
        Items::new(Dir::ToServer).hashed_stack(&mut Reader::new(&client.buf), &mut hashed).unwrap();
        let mut r = Reader::new(&hashed.buf);
        assert!(r.bool().unwrap());
        assert_eq!(r.varint().unwrap(), 7);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), ench_id);
        let expected = hash::hash(&Hv::Map(vec![(ident("sharpness"), Hv::Int(3))]));
        assert_eq!(r.i32().unwrap(), expected);
        assert_eq!(r.varint().unwrap(), 0);
        assert!(r.is_empty());
    }

    #[test]
    fn plain_item_hashes_to_id_and_count() {
        let mut client = Writer::new();
        client.varint(4);
        client.varint(d::ITEMS[9] as i32);
        client.varint(0);
        client.varint(0);
        let mut hashed = Writer::new();
        Items::new(Dir::ToServer).hashed_stack(&mut Reader::new(&client.buf), &mut hashed).unwrap();
        let mut expected = Writer::new();
        expected.bool(true);
        expected.varint(d::ITEMS_REVERSE[d::ITEMS[9] as usize] as i32);
        expected.varint(4);
        expected.varint(0);
        expected.varint(0);
        assert_eq!(hashed.buf, expected.buf);
    }

    #[test]
    fn creative_slot_is_length_prefixed_and_tooltip_restored() {
        // 1.21.4 client: unbreakable(show=false) + hide_tooltip
        let mut client = Writer::new();
        client.varint(1);
        client.varint(d::ITEMS[2] as i32);
        client.varint(2);
        client.varint(0);
        client.varint(comp_id(&d::OLD_COMPONENT_NAMES, "unbreakable"));
        client.bool(false);
        client.varint(comp_id(&d::OLD_COMPONENT_NAMES, "hide_tooltip"));
        let mut server = Writer::new();
        Items::new(Dir::ToServer).item_stack_length_prefixed(&mut Reader::new(&client.buf), &mut server).unwrap();
        let mut r = Reader::new(&server.buf);
        assert_eq!(r.varint().unwrap(), 1);
        r.varint().unwrap();
        assert_eq!(r.varint().unwrap(), 2);
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), comp_id(&d::NEW_COMPONENT_NAMES, "unbreakable"));
        assert_eq!(r.varint().unwrap(), 0); // empty payload
        assert_eq!(r.varint().unwrap(), comp_id(&d::NEW_COMPONENT_NAMES, "tooltip_display"));
        assert_eq!(r.varint().unwrap(), 3);
        assert!(r.bool().unwrap());
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), comp_id(&d::NEW_COMPONENT_NAMES, "unbreakable"));
        assert!(r.is_empty());
    }
}
