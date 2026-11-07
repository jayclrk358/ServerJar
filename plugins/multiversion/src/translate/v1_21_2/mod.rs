//! 1.21.4 (protocol 769) ↔ 1.21.2 / 1.21.3 (protocol 768).
//!
//! Packet layout differences follow the vanilla protocol changes between these versions; registry id tables are generated from the game's registry mapping data
//! (see `tools/gen_mappings.py`). Clientbound packet ids are identical in both versions; only
//! serverbound ids shifted (PICK_ITEM split in two, PLAYER_LOADED added).

pub mod entity;
pub mod ids;
pub mod items;
pub mod misc;
pub mod world;

use super::{data_1_21_2, Packet, State, TResult, Translator};
use crate::proto::buf::{Reader, Writer};
use crate::proto::version::Version;
use entity::Entities;
use ids::new;
use items::{Dir, Items};
use misc::Misc;
use world::World;

pub struct V1_21_4To1_21_2 {
    to_client: Items,
    to_server: Items,
    entities: Entities,
    misc: Misc,
    /// Set after LOGIN/RESPAWN: 1.21.4 servers wait for PLAYER_LOADED, which the 1.21.2 client
    /// never sends, so it goes out ahead of the client's next packet.
    player_loaded_pending: bool,
}

impl V1_21_4To1_21_2 {
    pub fn new() -> Self {
        Self {
            to_client: Items::new(Dir::ToClient),
            to_server: Items::new(Dir::ToServer),
            entities: Entities::default(),
            misc: Misc::new(),
            player_loaded_pending: false,
        }
    }

    fn play_clientbound(&mut self, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        let items = &self.to_client;
        let misc = &self.misc;
        let p = &pkt.payload[..];
        let e = |e: crate::proto::buf::DecodeError| format!("clientbound 0x{:02x}: {e}", pkt.id);
        let translated: Option<Vec<u8>> = match pkt.id {
            new::ADD_ENTITY => Some(self.entities.add_entity(items, p).map_err(e)?),
            new::REMOVE_ENTITIES => {
                self.entities.remove_entities(p).map_err(e)?;
                None
            }
            new::SET_ENTITY_DATA => Some(self.entities.set_entity_data(items, misc, p).map_err(e)?),
            new::SET_EQUIPMENT => Some(Entities::set_equipment(items, p).map_err(e)?),
            new::PLAYER_INFO_UPDATE => Some(Entities::player_info_update(p).map_err(e)?),
            new::LOGIN | new::RESPAWN => {
                if pkt.id == new::LOGIN {
                    self.entities.clear();
                }
                self.player_loaded_pending = true;
                None
            }
            new::LEVEL_CHUNK_WITH_LIGHT => Some(World::level_chunk_with_light(items, p).map_err(e)?),
            new::BLOCK_UPDATE => Some(World::block_update(items, p).map_err(e)?),
            new::SECTION_BLOCKS_UPDATE => Some(World::section_blocks_update(items, p).map_err(e)?),
            new::BLOCK_EVENT => Some(World::block_event(items, p).map_err(e)?),
            new::LEVEL_EVENT => Some(World::level_event(items, p).map_err(e)?),
            new::UPDATE_TAGS => Some(World::update_tags(items, p).map_err(e)?),
            new::LEVEL_PARTICLES => Some(misc.level_particles(items, p).map_err(e)?),
            new::SOUND | new::SOUND_ENTITY => Some(misc.sound(items, p).map_err(e)?),
            new::EXPLODE => Some(misc.explode(items, p).map_err(e)?),
            new::CONTAINER_SET_CONTENT => Some(misc.container_set_content(items, p).map_err(e)?),
            new::CONTAINER_SET_SLOT => Some(misc.container_set_slot(items, p).map_err(e)?),
            new::SET_CURSOR_ITEM => Some(misc.single_item(items, p, 0).map_err(e)?),
            new::SET_PLAYER_INVENTORY => Some(misc.single_item(items, p, 1).map_err(e)?),
            new::MERCHANT_OFFERS => Some(misc.merchant_offers(items, p).map_err(e)?),
            new::SET_HELD_SLOT => Some(misc.set_held_slot(p).map_err(e)?),
            new::RECIPE_BOOK_ADD => Some(misc.recipe_book_add(items, p).map_err(e)?),
            new::PLACE_GHOST_RECIPE => Some(misc.place_ghost_recipe(items, p).map_err(e)?),
            new::UPDATE_RECIPES => Some(misc.update_recipes(items, p).map_err(e)?),
            new::UPDATE_ADVANCEMENTS => Some(misc.update_advancements(items, p).map_err(e)?),
            new::SYSTEM_CHAT | new::DISGUISED_CHAT | new::SET_ACTION_BAR_TEXT | new::SET_TITLE_TEXT | new::SET_SUBTITLE_TEXT => {
                Some(misc.text_packet(p, 1).map_err(e)?)
            }
            new::TAB_LIST => Some(misc.text_packet(p, 2).map_err(e)?),
            _ => None,
        };
        let mapped = ids::play_clientbound(pkt.id).ok_or_else(|| format!("no 1.21.2 equivalent for clientbound play packet 0x{:02x}", pkt.id))?;
        out.push(Packet::new(mapped, translated.unwrap_or_else(|| pkt.payload.clone())));
        Ok(())
    }

    fn config_clientbound(&mut self, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        let e = |e: crate::proto::buf::DecodeError| format!("config 0x{:02x}: {e}", pkt.id);
        let payload = match pkt.id {
            new::CONFIG_REGISTRY_DATA => World::registry_data(&pkt.payload).map_err(e)?,
            new::CONFIG_UPDATE_TAGS => World::update_tags(&self.to_client, &pkt.payload).map_err(e)?,
            new::CONFIG_UPDATE_ENABLED_FEATURES => enabled_features(&pkt.payload).map_err(e)?,
            _ => pkt.payload.clone(),
        };
        let mapped = ids::config_clientbound(pkt.id).ok_or_else(|| format!("no 1.21.2 config packet for 0x{:02x}", pkt.id))?;
        out.push(Packet::new(mapped, payload));
        Ok(())
    }

    fn play_serverbound(&mut self, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        if std::mem::take(&mut self.player_loaded_pending) {
            out.push(Packet::new(new::S_PLAYER_LOADED, Vec::new()));
        }
        let e = |e: crate::proto::buf::DecodeError| format!("serverbound 0x{:02x}: {e}", pkt.id);
        let items = &self.to_server;
        let p = &pkt.payload[..];
        let Some(mapped) = ids::play_serverbound(pkt.id) else {
            // PICK_ITEM: the 1.21.4 server picks items itself from block/entity requests.
            return Ok(());
        };
        let payload = match pkt.id {
            ids::old::S_SET_CREATIVE_MODE_SLOT => self.misc.set_creative_slot(items, p).map_err(e)?,
            ids::old::S_CONTAINER_CLICK => self.misc.container_click(items, p).map_err(e)?,
            ids::old::S_MOVE_VEHICLE => self.misc.move_vehicle(p).map_err(e)?,
            _ => pkt.payload.clone(),
        };
        out.push(Packet::new(mapped, payload));
        Ok(())
    }
}

/// UPDATE_ENABLED_FEATURES: the 1.21.2 client needs the `winter_drop` feature flag to enable
/// the content 1.21.4 made part of vanilla (pale garden blocks, creaking...).
fn enabled_features(payload: &[u8]) -> crate::proto::buf::Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let mut flags: Vec<String> = r.list(|r| r.string().map(str::to_string))?;
    if !flags.iter().any(|f| f == "minecraft:winter_drop" || f == "winter_drop") {
        flags.push("minecraft:winter_drop".into());
    }
    let mut w = Writer::new();
    w.varint(flags.len() as i32);
    for f in &flags {
        w.string(f);
    }
    Ok(w.into_inner())
}

impl Default for V1_21_4To1_21_2 {
    fn default() -> Self {
        Self::new()
    }
}

impl Translator for V1_21_4To1_21_2 {
    fn from(&self) -> Version {
        Version { protocol: 769 }
    }
    fn to(&self) -> Version {
        Version { protocol: 768 }
    }
    fn clientbound(&mut self, state: State, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        match state {
            State::Play => self.play_clientbound(pkt, out),
            State::Configuration => self.config_clientbound(pkt, out),
        }
    }
    fn serverbound(&mut self, state: State, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        match state {
            State::Play => self.play_serverbound(pkt, out),
            // Configuration serverbound packets are identical in 1.21.2 and 1.21.4.
            State::Configuration => {
                out.push(pkt.clone());
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn winter_drop_flag_is_added_once() {
        let mut p = Writer::new();
        p.varint(1);
        p.string("minecraft:vanilla");
        let out = enabled_features(&p.buf).unwrap();
        let mut r = Reader::new(&out);
        assert_eq!(r.varint().unwrap(), 2);
        assert_eq!(r.string().unwrap(), "minecraft:vanilla");
        assert_eq!(r.string().unwrap(), "minecraft:winter_drop");
        assert_eq!(enabled_features(&out).unwrap(), out);
    }

    #[test]
    fn player_loaded_follows_login() {
        let mut t = V1_21_4To1_21_2::new();
        let mut out = Vec::new();
        t.clientbound(State::Play, &Packet::new(new::RESPAWN, vec![0; 4]), &mut out).unwrap();
        assert_eq!(out[0].id, new::RESPAWN);
        let mut sb = Vec::new();
        t.serverbound(State::Play, &Packet::new(0x0B, Vec::new()), &mut sb).unwrap(); // CLIENT_TICK_END
        assert_eq!(sb.len(), 2);
        assert_eq!(sb[0].id, new::S_PLAYER_LOADED);
        assert_eq!(sb[1].id, 0x0B);
        let mut sb = Vec::new();
        t.serverbound(State::Play, &Packet::new(0x0B, Vec::new()), &mut sb).unwrap();
        assert_eq!(sb.len(), 1);
    }

    #[test]
    fn pick_item_is_dropped_and_vehicle_gains_on_ground() {
        let mut t = V1_21_4To1_21_2::new();
        let mut sb = Vec::new();
        t.serverbound(State::Play, &Packet::new(ids::old::S_PICK_ITEM, vec![3]), &mut sb).unwrap();
        assert!(sb.is_empty());
        t.serverbound(State::Play, &Packet::new(ids::old::S_MOVE_VEHICLE, vec![0; 32]), &mut sb).unwrap();
        assert_eq!(sb[0].id, new::S_MOVE_VEHICLE);
        assert_eq!(sb[0].payload.len(), 33);
    }
}
