//! Registry data and tags 1.21.11 → 1.21.9.
//!
//! Chunks, light, block updates and block entities need no changes: block states, blocks and
//! block entity types are identical in both versions. What changed is the registry content:
//! 1.21.11 moved many dimension type and biome fields into "environment attributes", added the
//! timeline and zombie nautilus variant registries and new enchantment effects. The handlers
//! follow ViaBackwards' Protocol1_21_11To1_21_9.

use super::items::Items;
use crate::proto::buf::{Reader, Result, Writer};
use crate::proto::nbt::{self, Tag};

/// Registries that exist in 1.21.11 but not 1.21.9.
pub const REMOVED_REGISTRIES: &[&str] = &["timeline", "zombie_nautilus_variant"];

/// Enchantment effect types the old client does not know; replaced by an empty `all_of`.
const REMOVED_ENCHANTMENT_EFFECTS: &[&str] = &["apply_exhaustion", "apply_impulse"];

const END_FOG_COLOR: i32 = 10_518_688;
const OVERWORLD_FOG_COLOR: i32 = -4_138_753;

fn strip(key: &str) -> &str {
    key.strip_prefix("minecraft:").unwrap_or(key)
}

/// `tag[key]` or `tag["minecraft:" + key]`.
fn get_namespaced<'a>(tag: &'a Tag, key: &str) -> Option<&'a Tag> {
    tag.get(key).or_else(|| tag.get(&format!("minecraft:{key}")))
}

/// Environment attribute colors: a float RGB list or a "#rrggbb" / "#aarrggbb" string → int.
fn map_color(t: &Tag) -> Tag {
    match t {
        Tag::List(l) if l.len() >= 3 => {
            let c = |i: usize| match &l[i] {
                Tag::Float(f) => *f,
                Tag::Double(f) => *f as f32,
                other => other.as_i32().unwrap_or(0) as f32,
            };
            let to = |f: f32| ((f.clamp(0.0, 1.0) * 255.0) as u32) & 0xff;
            Tag::Int((0xff00_0000 | (to(c(0)) << 16) | (to(c(1)) << 8) | to(c(2))) as i32)
        }
        Tag::String(s) => match u32::from_str_radix(s.trim_start_matches('#'), 16) {
            Ok(v) => Tag::Int(v as i32),
            Err(_) => t.clone(),
        },
        other => other.clone(),
    }
}

fn truthy(t: &Tag) -> bool {
    t.as_i32().is_some_and(|v| v != 0)
}

pub struct World;

impl World {
    /// UPDATE_TAGS: remap item and entity ids, drop tags of removed registries.
    pub fn update_tags(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let n = r.varint()?;
        let mut kept = 0;
        let mut body = Writer::new();
        for _ in 0..n.max(0) {
            let registry = r.string()?.to_string();
            let key = strip(&registry).to_string();
            let tags = r.varint()?;
            if REMOVED_REGISTRIES.contains(&key.as_str()) {
                for _ in 0..tags.max(0) {
                    r.string()?;
                    r.list(|r| r.varint())?;
                }
                continue;
            }
            body.string(&registry);
            body.varint(tags);
            for _ in 0..tags.max(0) {
                body.string(r.string()?);
                let ids = r.list(|r| r.varint())?;
                body.varint(ids.len() as i32);
                for id in ids {
                    body.varint(match key.as_str() {
                        "item" => items.item(id),
                        "entity_type" => items.entity(id),
                        "block" => items.block(id),
                        _ => id,
                    });
                }
            }
            kept += 1;
        }
        let mut w = Writer::new();
        w.varint(kept);
        w.raw(&body.buf);
        Ok(w.into_inner())
    }

    /// REGISTRY_DATA: returns `None` when the whole registry must be dropped for the old client.
    pub fn registry_data(payload: &[u8]) -> Result<Option<Vec<u8>>> {
        let mut r = Reader::new(payload);
        let registry = r.string()?.to_string();
        let key = strip(&registry).to_string();
        if REMOVED_REGISTRIES.contains(&key.as_str()) {
            return Ok(None);
        }
        let mut w = Writer::new();
        w.string(&registry);
        let n = r.varint()?;
        w.varint(n);
        for _ in 0..n.max(0) {
            let entry = r.string()?.to_string();
            w.string(&entry);
            let present = r.bool()?;
            w.bool(present);
            if present {
                let mut tag = nbt::read(&mut r)?;
                Self::registry_entry(&key, strip(&entry), &mut tag);
                nbt::write(&mut w, &tag);
            }
        }
        Ok(Some(w.into_inner()))
    }

    fn registry_entry(registry: &str, entry: &str, tag: &mut Tag) {
        match registry {
            "dimension_type" => Self::dimension_type(entry, tag),
            "worldgen/biome" => Self::biome(entry, tag),
            "enchantment" => Self::enchantment(tag),
            _ => {}
        }
    }

    /// Puts back the mandatory 1.21.9 dimension type fields from 1.21.11's attributes.
    fn dimension_type(entry: &str, tag: &mut Tag) {
        let (effects, natural) = match entry {
            "the_nether" => ("minecraft:the_nether", false),
            "the_end" => ("minecraft:the_end", false),
            _ => ("minecraft:overworld", true),
        };
        tag.insert("effects", Tag::String(effects.into()));
        tag.insert("natural", Tag::Byte(i8::from(natural)));
        let attributes = tag.get("attributes").cloned();
        let attr = |key: &str| attributes.as_ref().and_then(|a| get_namespaced(a, key)).cloned();
        if let Some(v) = attr("visual/cloud_height") {
            tag.insert("cloud_height", v);
        }
        tag.insert("has_raids", attr("gameplay/can_start_raid").unwrap_or(Tag::Byte(1)));
        let piglin_safe = attr("gameplay/piglins_zombify").map_or(0, |v| i8::from(!truthy(&v)));
        tag.insert("piglin_safe", Tag::Byte(piglin_safe));
        tag.insert("respawn_anchor_works", attr("gameplay/respawn_anchor_works").unwrap_or(Tag::Byte(0)));
        let bed_works = attr("gameplay/bed_rule").map_or(1, |rule| {
            let flag = |k: &str| rule.get(k).is_some_and(truthy);
            i8::from(flag("can_sleep") || flag("can_set_spawn"))
        });
        tag.insert("bed_works", Tag::Byte(bed_works));
        tag.insert("ultrawarm", attr("gameplay/fast_lava").unwrap_or(Tag::Byte(0)));
    }

    /// Moves biome colors, music, ambient sounds and particles back from attributes to effects.
    fn biome(entry: &str, tag: &mut Tag) {
        let attributes = tag.remove("attributes");
        if tag.get("effects").is_none() {
            tag.insert("effects", Tag::compound());
        }
        let Some(effects) = tag.get_mut("effects") else { return };
        for key in ["water_color", "foliage_color", "dry_foliage_color", "grass_color"] {
            if let Some(v) = get_namespaced(effects, key).cloned() {
                effects.insert(key, map_color(&v));
            }
        }
        if effects.get("water_color").is_none() {
            effects.insert("water_color", Tag::Int(4_159_204));
        }
        let attr = |key: &str| attributes.as_ref().and_then(|a| get_namespaced(a, key)).cloned();
        let fog = if entry == "the_end" { END_FOG_COLOR } else { OVERWORLD_FOG_COLOR };
        for (key, target, default) in [("visual/sky_color", "sky_color", 0), ("visual/water_fog_color", "water_fog_color", -16_448_205), ("visual/fog_color", "fog_color", fog)] {
            effects.insert(target, attr(key).map_or(Tag::Int(default), |v| map_color(&v)));
        }
        if attributes.is_none() {
            return;
        }
        if let Some(def) = attr("audio/background_music").and_then(|m| m.get("default").cloned()) {
            let mut data = Tag::compound();
            for k in ["max_delay", "min_delay", "sound", "replace_current_music"] {
                if let Some(v) = def.get(k) {
                    data.insert(k, v.clone());
                }
            }
            if data.get("replace_current_music").is_none() {
                data.insert("replace_current_music", Tag::Byte(0));
            }
            let mut music = Tag::compound();
            music.insert("data", data);
            music.insert("weight", Tag::Int(1));
            effects.insert("music", Tag::List(vec![music]));
        }
        if let Some(ambient) = attr("audio/ambient_sounds") {
            for (k, target) in [("loop", "ambient_sound"), ("mood", "mood_sound"), ("additions", "additions_sound")] {
                if let Some(v) = ambient.get(k) {
                    effects.insert(target, v.clone());
                }
            }
        }
        if let Some(Tag::List(particles)) = attr("visual/ambient_particles") {
            if let Some(first) = particles.first() {
                let mut options = Tag::compound();
                if let Some(p) = first.get("probability") {
                    options.insert("probability", p.clone());
                }
                if let Some(p) = first.get("particle") {
                    options.insert("options", p.clone());
                }
                effects.insert("particle", options);
            }
        }
    }

    /// Drops the new post_piercing_attack effect component, neutralizes unknown effect types and
    /// turns 1.21.11's sound lists in play_sound effects back into a single sound.
    fn enchantment(tag: &mut Tag) {
        let Some(Tag::Compound(effects)) = tag.get_mut("effects") else { return };
        effects.retain(|(k, _)| strip(k) != "post_piercing_attack");
        for (_, value) in effects.iter_mut() {
            match value {
                Tag::Compound(_) => Self::nested_effect(value),
                Tag::List(list) => list.iter_mut().for_each(Self::nested_effect),
                _ => {}
            }
        }
    }

    fn nested_effect(effects_tag: &mut Tag) {
        if let Some(effect) = effects_tag.get_mut("effect") {
            Self::effect(effect);
            if let Some(Tag::List(inner)) = effect.get_mut("effects") {
                inner.iter_mut().for_each(Self::effect);
            }
        }
    }

    fn effect(effect: &mut Tag) {
        let Some(kind) = effect.get("type").and_then(Tag::as_str).map(|s| strip(s).to_string()) else { return };
        if REMOVED_ENCHANTMENT_EFFECTS.contains(&kind.as_str()) {
            *effect = Tag::Compound(vec![("type".into(), Tag::String("minecraft:all_of".into())), ("effects".into(), Tag::List(Vec::new()))]);
            return;
        }
        if kind == "play_sound" {
            if let Some(Tag::List(sounds)) = effect.get("sound") {
                let first = sounds.first().cloned().unwrap_or_else(|| Tag::String("minecraft:intentionally_empty".into()));
                effect.insert("sound", first);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry_packet(registry: &str, entry: &str, tag: &Tag) -> Vec<u8> {
        let mut w = Writer::new();
        w.string(registry);
        w.varint(1);
        w.string(entry);
        w.bool(true);
        nbt::write(&mut w, tag);
        w.into_inner()
    }

    fn read_entry(payload: &[u8]) -> Tag {
        let mut r = Reader::new(payload);
        r.string().unwrap();
        r.varint().unwrap();
        r.string().unwrap();
        assert!(r.bool().unwrap());
        nbt::read(&mut r).unwrap()
    }

    #[test]
    fn removed_registries_are_dropped() {
        let p = registry_packet("minecraft:timeline", "minecraft:day", &Tag::compound());
        assert!(World::registry_data(&p).unwrap().is_none());
    }

    #[test]
    fn biome_colors_move_back_to_effects() {
        let mut attrs = Tag::compound();
        attrs.insert("minecraft:visual/sky_color", Tag::String("#78a7ff".into()));
        let mut biome = Tag::compound();
        biome.insert("attributes", attrs);
        let mut effects = Tag::compound();
        effects.insert("water_color", Tag::String("#3f76e4".into()));
        biome.insert("effects", effects);
        let out = World::registry_data(&registry_packet("minecraft:worldgen/biome", "minecraft:plains", &biome)).unwrap().unwrap();
        let tag = read_entry(&out);
        assert!(tag.get("attributes").is_none());
        let effects = tag.get("effects").unwrap();
        assert_eq!(effects.get("sky_color"), Some(&Tag::Int(0x78a7ff)));
        assert_eq!(effects.get("water_color"), Some(&Tag::Int(0x3f76e4)));
        assert_eq!(effects.get("fog_color"), Some(&Tag::Int(OVERWORLD_FOG_COLOR)));
    }

    #[test]
    fn dimension_type_gets_legacy_fields() {
        let mut attrs = Tag::compound();
        attrs.insert("minecraft:gameplay/piglins_zombify", Tag::Byte(0));
        let mut dim = Tag::compound();
        dim.insert("attributes", attrs);
        let out = World::registry_data(&registry_packet("minecraft:dimension_type", "minecraft:the_nether", &dim)).unwrap().unwrap();
        let tag = read_entry(&out);
        assert_eq!(tag.get("effects").and_then(Tag::as_str), Some("minecraft:the_nether"));
        assert_eq!(tag.get("piglin_safe"), Some(&Tag::Byte(1)));
        assert_eq!(tag.get("bed_works"), Some(&Tag::Byte(1)));
        assert_eq!(tag.get("natural"), Some(&Tag::Byte(0)));
    }

    #[test]
    fn unknown_enchantment_effects_are_neutralized() {
        let effect = Tag::Compound(vec![("type".into(), Tag::String("minecraft:apply_impulse".into()))]);
        let entry = Tag::Compound(vec![("effect".into(), effect)]);
        let effects = Tag::Compound(vec![("minecraft:post_attack".into(), Tag::List(vec![entry])), ("minecraft:post_piercing_attack".into(), Tag::List(Vec::new()))]);
        let ench = Tag::Compound(vec![("effects".into(), effects)]);
        let out = World::registry_data(&registry_packet("minecraft:enchantment", "minecraft:lunge", &ench)).unwrap().unwrap();
        let tag = read_entry(&out);
        let effects = tag.get("effects").unwrap();
        assert!(effects.get("minecraft:post_piercing_attack").is_none());
        let first = &effects.get("minecraft:post_attack").unwrap().as_list().unwrap()[0];
        assert_eq!(first.get("effect").unwrap().get("type").and_then(Tag::as_str), Some("minecraft:all_of"));
    }
}
