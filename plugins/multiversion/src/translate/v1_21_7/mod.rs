//! 1.21.9 (protocol 773) ↔ 1.21.7 / 1.21.8 (protocol 772).
//!
//! Packet layout differences follow the vanilla protocol changes between these versions; registry id tables are
//! generated from the game's registry mapping data (see `tools/gen_mappings.py`).

pub use super::data_1_21_7 as data;
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
use misc::Misc;
use world::World;

pub struct V1_21_9To1_21_7 {
    to_client: Items,
    to_server: Items,
    entities: Entities,
    misc: Misc,
}

impl V1_21_9To1_21_7 {
    pub fn new() -> Self {
        Self { to_client: Items::new(Dir::ToClient), to_server: Items::new(Dir::ToServer), entities: Entities::default(), misc: Misc::new() }
    }

    fn play_clientbound(&mut self, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        let items = &self.to_client;
        let p = &pkt.payload[..];
        let e = |e: crate::proto::buf::DecodeError| format!("clientbound 0x{:02x}: {e}", pkt.id);
        let translated: Option<Vec<u8>> = match pkt.id {
            new::ADD_ENTITY | new::REMOVE_ENTITIES => {
                let mut produced = Vec::new();
                if pkt.id == new::ADD_ENTITY {
                    self.entities.add_entity(items, p, &mut produced).map_err(e)?;
                } else {
                    self.entities.remove_entities(p, &mut produced).map_err(e)?;
                }
                out.extend(produced.into_iter().map(|(id, payload)| Packet::new(id, payload)));
                return Ok(());
            }
            new::SET_ENTITY_DATA => Some(self.entities.set_entity_data(items, &self.misc, p).map_err(e)?),
            new::SET_ENTITY_MOTION => Some(Entities::set_entity_motion(p).map_err(e)?),
            new::SET_EQUIPMENT => Some(Entities::set_equipment(items, p).map_err(e)?),
            new::PLAYER_ROTATION => Some(self.entities.player_rotation(p).map_err(e)?),
            new::SET_DEFAULT_SPAWN_POSITION => Some(Entities::default_spawn_position(p).map_err(e)?),
            new::INITIALIZE_BORDER | new::SET_BORDER_CENTER => Some(self.entities.border_center(p).map_err(e)?),
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
            new::LEVEL_CHUNK_WITH_LIGHT => Some(World::level_chunk_with_light(items, p).map_err(e)?),
            new::BLOCK_UPDATE => Some(World::block_update(items, p).map_err(e)?),
            new::SECTION_BLOCKS_UPDATE => Some(World::section_blocks_update(items, p).map_err(e)?),
            new::BLOCK_EVENT => Some(World::block_event(items, p).map_err(e)?),
            new::LEVEL_EVENT => Some(World::level_event(items, p).map_err(e)?),
            new::BLOCK_ENTITY_DATA => match World::block_entity_data(p).map_err(e)? {
                Some(v) => Some(v),
                None => return Ok(()),
            },
            new::UPDATE_TAGS => Some(World::update_tags(items, p).map_err(e)?),
            new::LEVEL_PARTICLES => Some(self.misc.level_particles(items, p).map_err(e)?),
            new::SOUND | new::SOUND_ENTITY => Some(self.misc.sound(items, p).map_err(e)?),
            new::EXPLODE => Some(self.misc.explode(items, p).map_err(e)?),
            new::CONTAINER_SET_CONTENT => Some(self.misc.container_set_content(items, p).map_err(e)?),
            new::CONTAINER_SET_SLOT => Some(self.misc.container_set_slot(items, p).map_err(e)?),
            new::SET_CURSOR_ITEM => Some(self.misc.single_item(items, p, 0).map_err(e)?),
            new::SET_PLAYER_INVENTORY => Some(self.misc.single_item(items, p, 1).map_err(e)?),
            new::MERCHANT_OFFERS => Some(self.misc.merchant_offers(items, p).map_err(e)?),
            new::RECIPE_BOOK_ADD => Some(self.misc.recipe_book_add(items, p).map_err(e)?),
            new::PLACE_GHOST_RECIPE => Some(self.misc.place_ghost_recipe(items, p).map_err(e)?),
            new::UPDATE_RECIPES => Some(self.misc.update_recipes(items, p).map_err(e)?),
            new::UPDATE_ADVANCEMENTS => Some(self.misc.update_advancements(items, p).map_err(e)?),
            new::DEBUG_BLOCK_VALUE | new::DEBUG_CHUNK_VALUE | new::DEBUG_ENTITY_VALUE | new::DEBUG_EVENT | new::GAME_TEST_HIGHLIGHT_POS => {
                return Ok(());
            }
            _ => None,
        };
        let Some(mapped) = ids::play_clientbound(pkt.id) else {
            return Err(format!("no 1.21.7 equivalent for clientbound play packet 0x{:02x}", pkt.id));
        };
        out.push(Packet::new(mapped, translated.unwrap_or_else(|| pkt.payload.clone())));
        Ok(())
    }

    fn config_clientbound(&mut self, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        let e = |e: crate::proto::buf::DecodeError| format!("config 0x{:02x}: {e}", pkt.id);
        let payload = match pkt.id {
            new::CONFIG_REGISTRY_DATA => {
                let (payload, scales) = World::registry_data(&pkt.payload).map_err(e)?;
                if let Some(scales) = scales {
                    self.entities.set_dimension_scales(scales);
                }
                payload
            }
            new::CONFIG_UPDATE_TAGS => World::update_tags(&self.to_client, &pkt.payload).map_err(e)?,
            new::CONFIG_CODE_OF_CONDUCT => {
                // 1.21.7 has no code of conduct screen: show it as a dialog instead.
                const SHOW_DIALOG: i32 = 18;
                out.push(Packet::new(SHOW_DIALOG, self.misc.code_of_conduct_dialog(&pkt.payload).map_err(e)?));
                return Ok(());
            }
            _ => pkt.payload.clone(),
        };
        let mapped = ids::config_clientbound(pkt.id).ok_or_else(|| format!("no 1.21.7 config packet for 0x{:02x}", pkt.id))?;
        out.push(Packet::new(mapped, payload));
        Ok(())
    }

    fn play_serverbound(&mut self, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        let e = |e: crate::proto::buf::DecodeError| format!("serverbound 0x{:02x}: {e}", pkt.id);
        let items = &self.to_server;
        let p = &pkt.payload[..];
        let payload = match pkt.id {
            ids::old::S_MOVE_PLAYER_POS_ROT => {
                self.entities.store_rotation(p, true);
                pkt.payload.clone()
            }
            ids::old::S_MOVE_PLAYER_ROT => {
                self.entities.store_rotation(p, false);
                pkt.payload.clone()
            }
            ids::old::S_SET_CREATIVE_MODE_SLOT => self.misc.set_creative_slot(items, p).map_err(e)?,
            ids::old::S_CONTAINER_CLICK => self.misc.container_click(items, p).map_err(e)?,
            ids::old::S_DEBUG_SUBSCRIPTION_REQUEST => match self.misc.debug_subscription_request(p).map_err(e)? {
                Some(v) => v,
                None => return Ok(()),
            },
            _ => pkt.payload.clone(),
        };
        let mapped = ids::play_serverbound(pkt.id).ok_or_else(|| format!("unknown serverbound play packet 0x{:02x}", pkt.id))?;
        out.push(Packet::new(mapped, payload));
        Ok(())
    }

    fn config_serverbound(&mut self, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        const CUSTOM_CLICK_ACTION: i32 = 8;
        if pkt.id == CUSTOM_CLICK_ACTION && self.misc.is_code_of_conduct_ack(&pkt.payload) {
            out.push(Packet::new(new::S_CONFIG_ACCEPT_CODE_OF_CONDUCT, Vec::new()));
            return Ok(());
        }
        let mapped = ids::config_serverbound(pkt.id).ok_or_else(|| format!("unknown serverbound config packet 0x{:02x}", pkt.id))?;
        out.push(Packet::new(mapped, pkt.payload.clone()));
        Ok(())
    }
}

impl Default for V1_21_9To1_21_7 {
    fn default() -> Self {
        Self::new()
    }
}

impl Translator for V1_21_9To1_21_7 {
    fn from(&self) -> Version {
        Version { protocol: 773 }
    }
    fn to(&self) -> Version {
        Version { protocol: 772 }
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
            State::Configuration => self.config_serverbound(pkt, out),
        }
    }
}
