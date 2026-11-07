//! 26.1 (protocol 775) ↔ 1.21.11 (protocol 774).
//!
//! Packet layout differences follow the vanilla protocol changes in both directions; registry id
//! tables are generated from the game's registry mapping data (see `tools/gen_mappings.py`).

pub mod entity;
pub mod ids;
pub mod items;
pub mod misc;
pub mod world;

use super::{Packet, State, TResult, Translator};
use crate::proto::version::Version;
use entity::Entities;
use ids::new;
use items::{Dir, Items};
use misc::{DayTime, Misc};
use world::World;

pub struct V26_1To1_21_11 {
    to_client: Items,
    to_server: Items,
    entities: Entities,
    /// world_clock registry keys in id order, learned during configuration.
    clocks: Vec<String>,
    day_time: DayTime,
}

impl V26_1To1_21_11 {
    pub fn new() -> Self {
        Self {
            to_client: Items::new(Dir::ToClient),
            to_server: Items::new(Dir::ToServer),
            entities: Entities::new(),
            clocks: Vec::new(),
            day_time: DayTime::default(),
        }
    }

    fn play_clientbound(&mut self, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        let items = &self.to_client;
        let p = &pkt.payload[..];
        let e = |e: crate::proto::buf::DecodeError| format!("clientbound 0x{:02x}: {e}", pkt.id);
        let translated: Option<Vec<u8>> = match pkt.id {
            new::ADD_ENTITY => Some(self.entities.add_entity(items, p).map_err(e)?),
            new::REMOVE_ENTITIES => {
                self.entities.remove_entities(p).map_err(e)?;
                None
            }
            new::SET_ENTITY_DATA => Some(self.entities.set_entity_data(items, p).map_err(e)?),
            new::LOGIN => {
                self.entities.clear();
                self.entities.login(p).map_err(e)?;
                None
            }
            new::RESPAWN => {
                self.entities.clear();
                self.entities.respawn(p).map_err(e)?;
                None
            }
            new::GAME_EVENT => {
                self.entities.game_event(p).map_err(e)?;
                None
            }
            new::SET_EQUIPMENT => Some(Entities::set_equipment(items, p).map_err(e)?),
            new::LEVEL_CHUNK_WITH_LIGHT => Some(World::level_chunk_with_light(items, p).map_err(e)?),
            new::BLOCK_UPDATE => Some(World::block_update(items, p).map_err(e)?),
            new::SECTION_BLOCKS_UPDATE => Some(World::section_blocks_update(items, p).map_err(e)?),
            new::BLOCK_EVENT => Some(World::block_event(items, p).map_err(e)?),
            new::LEVEL_EVENT => Some(World::level_event(items, p).map_err(e)?),
            new::UPDATE_TAGS => Some(World::update_tags(items, p).map_err(e)?),
            new::LEVEL_PARTICLES => Some(Misc::level_particles(items, p).map_err(e)?),
            new::SOUND | new::SOUND_ENTITY => Some(Misc::sound(items, p).map_err(e)?),
            new::EXPLODE => Some(Misc::explode(items, p).map_err(e)?),
            new::CONTAINER_SET_CONTENT => Some(Misc::container_set_content(items, p).map_err(e)?),
            new::CONTAINER_SET_SLOT => Some(Misc::container_set_slot(items, p).map_err(e)?),
            new::SET_CURSOR_ITEM => Some(Misc::single_item(items, p, 0).map_err(e)?),
            new::SET_PLAYER_INVENTORY => Some(Misc::single_item(items, p, 1).map_err(e)?),
            new::MERCHANT_OFFERS => Some(Misc::merchant_offers(items, p).map_err(e)?),
            new::COMMANDS => Some(Misc::commands(p).map_err(e)?),
            new::RECIPE_BOOK_ADD => Some(Misc::recipe_book_add(items, p).map_err(e)?),
            new::PLACE_GHOST_RECIPE => Some(Misc::place_ghost_recipe(items, p).map_err(e)?),
            new::UPDATE_RECIPES => Some(Misc::update_recipes(items, p).map_err(e)?),
            new::UPDATE_ADVANCEMENTS => Some(Misc::update_advancements(items, p).map_err(e)?),
            new::SET_TIME => Some(Misc::set_time(&mut self.day_time, &self.clocks, p).map_err(e)?),
            new::GAME_RULE_VALUES | new::LOW_DISK_SPACE_WARNING => return Ok(()),
            _ => None,
        };
        let Some(mapped) = ids::play_clientbound(pkt.id) else {
            return Err(format!("no 1.21.11 equivalent for clientbound play packet 0x{:02x}", pkt.id));
        };
        out.push(Packet::new(mapped, translated.unwrap_or_else(|| pkt.payload.clone())));
        Ok(())
    }

    fn config_clientbound(&mut self, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        let e = |e: crate::proto::buf::DecodeError| format!("config 0x{:02x}: {e}", pkt.id);
        let payload = match pkt.id {
            new::CONFIG_REGISTRY_DATA => {
                let (registry, keys) = World::registry_keys(&pkt.payload).map_err(e)?;
                if registry.strip_prefix("minecraft:").unwrap_or(&registry) == "world_clock" {
                    self.clocks = keys;
                }
                match World::registry_data(&pkt.payload).map_err(e)? {
                    Some(v) => v,
                    None => return Ok(()),
                }
            }
            new::CONFIG_UPDATE_TAGS => World::update_tags(&self.to_client, &pkt.payload).map_err(e)?,
            _ => pkt.payload.clone(),
        };
        out.push(Packet::new(pkt.id, payload));
        Ok(())
    }

    fn play_serverbound(&mut self, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        let e = |e: crate::proto::buf::DecodeError| format!("serverbound 0x{:02x}: {e}", pkt.id);
        let items = &self.to_server;
        let p = &pkt.payload[..];
        match pkt.id {
            ids::old::S_INTERACT => {
                if let Some((id, payload)) = Misc::interact(p, self.entities.is_spectator()).map_err(e)? {
                    out.push(Packet::new(id, payload));
                }
                Ok(())
            }
            ids::old::S_SET_CREATIVE_MODE_SLOT => {
                out.push(Packet::new(new::S_SET_CREATIVE_MODE_SLOT, Misc::set_creative_slot(items, p).map_err(e)?));
                Ok(())
            }
            ids::old::S_CONTAINER_CLICK => {
                out.push(Packet::new(new::S_CONTAINER_CLICK, Misc::container_click(items, p).map_err(e)?));
                Ok(())
            }
            _ => {
                let mapped = ids::play_serverbound(pkt.id).ok_or_else(|| format!("unknown serverbound play packet 0x{:02x}", pkt.id))?;
                out.push(Packet::new(mapped, pkt.payload.clone()));
                Ok(())
            }
        }
    }
}

impl Default for V26_1To1_21_11 {
    fn default() -> Self {
        Self::new()
    }
}

impl Translator for V26_1To1_21_11 {
    fn from(&self) -> Version {
        Version { protocol: 775 }
    }
    fn to(&self) -> Version {
        Version { protocol: 774 }
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
            // Configuration serverbound packets are identical.
            State::Configuration => {
                out.push(pkt.clone());
                Ok(())
            }
        }
    }
}
