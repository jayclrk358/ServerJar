//! World data 1.21.7 → 1.21.6: tags and registry data.
//!
//! Block, block state, block entity and biome ids are identical in both versions, so chunks, light,
//! block updates and block entities pass through untouched. Tags carry item ids, and some synced
//! registries reference sounds by name.

use super::items::Items;
use crate::proto::buf::{Reader, Result, Writer};
use crate::proto::nbt::{self, Tag};

pub struct World;

impl World {
    /// UPDATE_TAGS (play and configuration): remap ids of id-based registries.
    pub fn update_tags(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            let registry = r.string()?;
            w.string(registry);
            let key = registry.strip_prefix("minecraft:").unwrap_or(registry);
            let tags = Items::copy_varint(&mut r, &mut w)?;
            for _ in 0..tags.max(0) {
                Items::copy_string(&mut r, &mut w)?;
                let ids = r.list(|r| r.varint())?;
                w.varint(ids.len() as i32);
                for id in ids {
                    w.varint(match key {
                        "item" => items.item(id),
                        "block" => items.block(id),
                        "entity_type" => items.entity(id),
                        _ => id,
                    });
                }
            }
        }
        Ok(w.into_inner())
    }

    /// REGISTRY_DATA: entries reference sounds by name (jukebox songs, biome ambience, ...);
    /// sounds the old client lacks are renamed to their fallback.
    pub fn registry_data(items: &Items, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::with_capacity(payload.len());
        let registry = r.string()?;
        w.string(registry);
        let key = registry.strip_prefix("minecraft:").unwrap_or(registry);
        let rewrite = matches!(key, "jukebox_song" | "worldgen/biome" | "instrument" | "dimension_type");
        let n = Items::copy_varint(&mut r, &mut w)?;
        for _ in 0..n.max(0) {
            Items::copy_string(&mut r, &mut w)?;
            // Optional<Tag>: bool prefix, then the tag.
            if Items::copy_bool(&mut r, &mut w)? {
                if rewrite {
                    let mut tag = nbt::read(&mut r)?;
                    Self::rename_sounds(items, &mut tag);
                    nbt::write(&mut w, &tag);
                } else {
                    Items::copy_nbt(&mut r, &mut w)?;
                }
            }
        }
        Ok(w.into_inner())
    }

    fn rename_sounds(items: &Items, tag: &mut Tag) {
        match tag {
            Tag::String(s) => {
                let renamed = items.sound_name(s);
                if renamed != *s {
                    *s = renamed;
                }
            }
            Tag::Compound(entries) => {
                for (_, v) in entries.iter_mut() {
                    Self::rename_sounds(items, v);
                }
            }
            Tag::List(list) => {
                for v in list.iter_mut() {
                    Self::rename_sounds(items, v);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::data_1_21_6 as d;
    use super::super::items::Dir;
    use super::*;

    #[test]
    fn item_tags_are_remapped() {
        let items = Items::new(Dir::ToClient);
        let disc = d::NEW_ITEM_NAMES.iter().position(|n| *n == "music_disc_lava_chicken").unwrap() as i32;
        let mut p = Writer::new();
        p.varint(1);
        p.string("minecraft:item");
        p.varint(1);
        p.string("minecraft:creeper_drop_music_discs");
        p.varint(1);
        p.varint(disc);
        let out = World::update_tags(&items, &p.buf).unwrap();
        let mut r = Reader::new(&out);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.string().unwrap(), "minecraft:item");
        assert_eq!(r.varint().unwrap(), 1);
        r.string().unwrap();
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), d::OLD_ITEM_NAMES.iter().position(|n| *n == "music_disc_pigstep").unwrap() as i32);
        assert!(r.is_empty());
    }

    #[test]
    fn jukebox_song_sound_is_renamed() {
        let items = Items::new(Dir::ToClient);
        let mut song = Tag::compound();
        song.insert("sound_event", Tag::String("minecraft:music_disc.lava_chicken".into()));
        let mut p = Writer::new();
        p.string("minecraft:jukebox_song");
        p.varint(1);
        p.string("minecraft:lava_chicken");
        p.bool(true);
        nbt::write(&mut p, &song);
        let out = World::registry_data(&items, &p.buf).unwrap();
        let mut r = Reader::new(&out);
        r.string().unwrap();
        r.varint().unwrap();
        r.string().unwrap();
        assert!(r.bool().unwrap());
        let tag = nbt::read(&mut r).unwrap();
        assert_eq!(tag.get("sound_event").and_then(Tag::as_str), Some("minecraft:music_disc.pigstep"));
    }
}
