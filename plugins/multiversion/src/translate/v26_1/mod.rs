//! 26.2 (protocol 776) ↔ 26.1 (protocol 775).
//!
//! Packet layout differences follow the vanilla protocol changes between these versions; registry id tables are
//! generated from the game's registry mapping data (see `tools/gen_mappings.py`). Both versions use
//! the same packet id enums, so only payloads change.

pub mod entity;
pub mod ids;
pub mod items;
pub mod misc;
pub mod world;

use super::{data_26_1, Packet, State, TResult, Translator};
use crate::proto::version::Version;
use entity::Entities;
use ids::new;
use items::{Dir, Items};
use misc::Misc;
use world::World;

pub struct V26_2To26_1 {
    to_client: Items,
    to_server: Items,
    entities: Entities,
    world: World,
    misc: Misc,
}

impl V26_2To26_1 {
    pub fn new() -> Self {
        Self {
            to_client: Items::new(Dir::ToClient),
            to_server: Items::new(Dir::ToServer),
            entities: Entities::default(),
            world: World::new(),
            misc: Misc::new(),
        }
    }

    fn play_clientbound(&mut self, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        let items = &self.to_client;
        let p = &pkt.payload[..];
        let e = |e: crate::proto::buf::DecodeError| format!("clientbound 0x{:02x}: {e}", pkt.id);
        let translated: Option<Vec<u8>> = match pkt.id {
            new::ADD_ENTITY => {
                let mut produced = Vec::new();
                self.entities.add_entity(items, p, &mut produced).map_err(e)?;
                for (id, payload) in produced {
                    out.push(Packet::new(id, payload));
                }
                return Ok(());
            }
            new::REMOVE_ENTITIES => {
                self.entities.remove_entities(p).map_err(e)?;
                None
            }
            new::SET_ENTITY_DATA => Some(self.entities.set_entity_data(items, &self.misc, p).map_err(e)?),
            new::LOGIN => {
                self.entities.clear();
                let (payload, dim) = Entities::login(p).map_err(e)?;
                self.world.set_dimension(dim);
                Some(payload)
            }
            new::RESPAWN => {
                self.entities.clear();
                let (payload, dim) = Entities::respawn(p).map_err(e)?;
                self.world.set_dimension(dim);
                Some(payload)
            }
            new::SET_EQUIPMENT => Some(Entities::set_equipment(items, p).map_err(e)?),
            new::UPDATE_ATTRIBUTES => Some(Entities::update_attributes(items, p).map_err(e)?),
            new::LEVEL_CHUNK_WITH_LIGHT => Some(self.world.level_chunk_with_light(items, p).map_err(e)?),
            new::BLOCK_UPDATE | new::SECTION_BLOCKS_UPDATE => {
                let (payload, beds) = if pkt.id == new::BLOCK_UPDATE {
                    World::block_update(items, p).map_err(e)?
                } else {
                    World::section_blocks_update(items, p).map_err(e)?
                };
                out.push(Packet::new(pkt.id, payload));
                for bed in beds {
                    out.push(Packet::new(ids::old::BLOCK_ENTITY_DATA, bed));
                }
                return Ok(());
            }
            new::BLOCK_EVENT => Some(World::block_event(items, p).map_err(e)?),
            new::LEVEL_EVENT => Some(World::level_event(items, p).map_err(e)?),
            new::BLOCK_ENTITY_DATA => match World::block_entity_data(items, p).map_err(e)? {
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
            new::COMMANDS => Some(self.misc.commands(p).map_err(e)?),
            new::RECIPE_BOOK_ADD => Some(self.misc.recipe_book_add(items, p).map_err(e)?),
            new::PLACE_GHOST_RECIPE => Some(self.misc.place_ghost_recipe(items, p).map_err(e)?),
            new::UPDATE_RECIPES => Some(self.misc.update_recipes(items, p).map_err(e)?),
            new::UPDATE_ADVANCEMENTS => Some(self.misc.update_advancements(items, p).map_err(e)?),
            new::SET_PLAYER_TEAM => Some(self.misc.set_player_team(p).map_err(e)?),
            _ => None,
        };
        let Some(mapped) = ids::play_clientbound(pkt.id) else {
            return Err(format!("no 26.1 equivalent for clientbound play packet 0x{:02x}", pkt.id));
        };
        out.push(Packet::new(mapped, translated.unwrap_or_else(|| pkt.payload.clone())));
        Ok(())
    }

    fn config_clientbound(&mut self, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        let e = |e: crate::proto::buf::DecodeError| format!("config 0x{:02x}: {e}", pkt.id);
        let payload = match pkt.id {
            new::CONFIG_REGISTRY_DATA => match self.world.registry_data(&self.to_client, &pkt.payload).map_err(e)? {
                Some(v) => v,
                None => return Ok(()),
            },
            new::CONFIG_UPDATE_TAGS => World::update_tags(&self.to_client, &pkt.payload).map_err(e)?,
            _ => pkt.payload.clone(),
        };
        let mapped = ids::config_clientbound(pkt.id).ok_or_else(|| format!("no 26.1 config packet for 0x{:02x}", pkt.id))?;
        out.push(Packet::new(mapped, payload));
        Ok(())
    }

    fn play_serverbound(&mut self, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        let e = |e: crate::proto::buf::DecodeError| format!("serverbound 0x{:02x}: {e}", pkt.id);
        let items = &self.to_server;
        let p = &pkt.payload[..];
        let payload = match pkt.id {
            ids::old::S_SET_CREATIVE_MODE_SLOT => self.misc.set_creative_slot(items, p).map_err(e)?,
            ids::old::S_CONTAINER_CLICK => self.misc.container_click(items, p).map_err(e)?,
            ids::old::S_SPECTATE_ENTITY => self.misc.spectate_entity(p).map_err(e)?,
            _ => pkt.payload.clone(),
        };
        let mapped = ids::play_serverbound(pkt.id).ok_or_else(|| format!("unknown serverbound play packet 0x{:02x}", pkt.id))?;
        out.push(Packet::new(mapped, payload));
        Ok(())
    }
}

impl Default for V26_2To26_1 {
    fn default() -> Self {
        Self::new()
    }
}

impl Translator for V26_2To26_1 {
    fn from(&self) -> Version {
        Version { protocol: 776 }
    }
    fn to(&self) -> Version {
        Version { protocol: 775 }
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
            // Configuration serverbound packets are identical in 26.1 and 26.2.
            State::Configuration => {
                out.push(pkt.clone());
                Ok(())
            }
        }
    }
}

#[allow(dead_code)]
fn _data_used() -> usize {
    data_26_1::ITEMS.len()
}
