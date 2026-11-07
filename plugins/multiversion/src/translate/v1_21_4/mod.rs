//! 1.21.5 (protocol 770) ↔ 1.21.4 (protocol 769).
//!
//! Packet layout differences follow ViaBackwards' `Protocol1_21_5To1_21_4` (clientbound) and
//! ViaVersion's `Protocol1_21_4To1_21_5` (serverbound); registry id tables are generated from
//! ViaVersion's mapping files (see `tools/gen_mappings.py`).

pub mod entity;
pub mod hash;
pub mod ids;
pub mod items;
pub mod misc;
pub mod text;
pub mod world;

use super::{data_1_21_4, Packet, State, TResult, Translator};
use crate::proto::version::Version;
use entity::Entities;
use ids::new;
use items::{Dir, Items, Registries};
use misc::Misc;
use world::World;

pub struct V1_21_5To1_21_4 {
    to_client: Items,
    to_server: Items,
    entities: Entities,
    misc: Misc,
    registries: Registries,
}

impl V1_21_5To1_21_4 {
    pub fn new() -> Self {
        Self {
            to_client: Items::new(Dir::ToClient),
            to_server: Items::new(Dir::ToServer),
            entities: Entities::default(),
            misc: Misc::new(),
            registries: Registries::default(),
        }
    }

    fn play_clientbound(&mut self, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        let items = &self.to_client;
        let regs = &self.registries;
        let p = &pkt.payload[..];
        let e = |e: crate::proto::buf::DecodeError| format!("clientbound 0x{:02x}: {e}", pkt.id);
        let translated: Option<Vec<u8>> = match pkt.id {
            new::ADD_ENTITY => {
                let mut produced = Vec::new();
                self.entities.add_entity(items, p, &mut produced).map_err(e)?;
                out.extend(produced.into_iter().map(|(id, payload)| Packet::new(id, payload)));
                return Ok(());
            }
            new::SET_EQUIPMENT => {
                let mut produced = Vec::new();
                self.entities.set_equipment(items, p, &mut produced).map_err(e)?;
                out.extend(produced.into_iter().map(|(id, payload)| Packet::new(id, payload)));
                return Ok(());
            }
            new::REMOVE_ENTITIES => {
                self.entities.remove_entities(p).map_err(e)?;
                None
            }
            new::SET_ENTITY_DATA => match self.entities.set_entity_data(items, &self.misc, regs, p).map_err(e)? {
                Some(v) => Some(v),
                None => return Ok(()),
            },
            new::LOGIN | new::RESPAWN => {
                self.entities.clear();
                None
            }
            new::SET_PLAYER_TEAM => Some(Entities::set_player_team(p).map_err(e)?),
            new::LEVEL_CHUNK_WITH_LIGHT => Some(World::level_chunk_with_light(items, p).map_err(e)?),
            new::CHUNKS_BIOMES => Some(World::chunks_biomes(p).map_err(e)?),
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
            new::CONTAINER_SET_CONTENT => Some(self.misc.container_set_content(items, regs, p).map_err(e)?),
            new::CONTAINER_SET_SLOT => Some(self.misc.container_set_slot(items, regs, p).map_err(e)?),
            new::SET_CURSOR_ITEM => Some(self.misc.single_item(items, regs, p, 0).map_err(e)?),
            new::SET_PLAYER_INVENTORY => Some(self.misc.single_item(items, regs, p, 1).map_err(e)?),
            new::MERCHANT_OFFERS => Some(self.misc.merchant_offers(items, p).map_err(e)?),
            new::COMMANDS => Some(self.misc.commands(p).map_err(e)?),
            new::RECIPE_BOOK_ADD => Some(self.misc.recipe_book_add(items, p).map_err(e)?),
            new::PLACE_GHOST_RECIPE => Some(self.misc.place_ghost_recipe(items, p).map_err(e)?),
            new::UPDATE_RECIPES => Some(self.misc.update_recipes(items, p).map_err(e)?),
            new::UPDATE_ADVANCEMENTS => Some(self.misc.update_advancements(items, p).map_err(e)?),
            new::PLAYER_CHAT => Some(Misc::player_chat(p).map_err(e)?),
            new::DISGUISED_CHAT => Some(Misc::disguised_chat(p).map_err(e)?),
            new::SYSTEM_CHAT | new::SET_ACTION_BAR_TEXT | new::SET_TITLE_TEXT | new::SET_SUBTITLE_TEXT | new::DISCONNECT => {
                Some(Misc::leading_components(p, 1).map_err(e)?)
            }
            new::TAB_LIST => Some(Misc::leading_components(p, 2).map_err(e)?),
            new::OPEN_SCREEN => Some(Misc::open_screen(p).map_err(e)?),
            new::PLAYER_COMBAT_KILL => Some(Misc::player_combat_kill(p).map_err(e)?),
            new::BOSS_EVENT => Some(Misc::boss_event(p).map_err(e)?),
            new::SET_OBJECTIVE => Some(Misc::set_objective(p).map_err(e)?),
            new::TEST_INSTANCE_BLOCK_STATUS => return Ok(()),
            _ => None,
        };
        let Some(mapped) = ids::play_clientbound(pkt.id) else {
            return Err(format!("no 1.21.4 equivalent for clientbound play packet 0x{:02x}", pkt.id));
        };
        out.push(Packet::new(mapped, translated.unwrap_or_else(|| pkt.payload.clone())));
        Ok(())
    }

    fn config_clientbound(&mut self, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        let e = |e: crate::proto::buf::DecodeError| format!("config 0x{:02x}: {e}", pkt.id);
        let payload = match pkt.id {
            new::CONFIG_REGISTRY_DATA => match World::registry_data(&mut self.registries, &pkt.payload).map_err(e)? {
                Some(v) => v,
                None => return Ok(()),
            },
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
        let payload = match pkt.id {
            ids::old::S_SET_CREATIVE_MODE_SLOT => self.misc.set_creative_slot(items, p).map_err(e)?,
            ids::old::S_CONTAINER_CLICK => self.misc.container_click(items, p).map_err(e)?,
            ids::old::S_CHAT | ids::old::S_CHAT_COMMAND_SIGNED => Misc::append_checksum(p),
            _ => pkt.payload.clone(),
        };
        let mapped = ids::play_serverbound(pkt.id).ok_or_else(|| format!("unknown serverbound play packet 0x{:02x}", pkt.id))?;
        out.push(Packet::new(mapped, payload));
        Ok(())
    }
}

impl Default for V1_21_5To1_21_4 {
    fn default() -> Self {
        Self::new()
    }
}

impl Translator for V1_21_5To1_21_4 {
    fn from(&self) -> Version {
        Version { protocol: 770 }
    }
    fn to(&self) -> Version {
        Version { protocol: 769 }
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
            // Configuration serverbound ids are identical in 1.21.4 and 1.21.5.
            State::Configuration => {
                out.push(pkt.clone());
                Ok(())
            }
        }
    }
}

#[allow(dead_code)]
fn _data_used() -> usize {
    data_1_21_4::ITEMS.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instance_status_is_dropped_and_ids_shift() {
        let mut t = V1_21_5To1_21_4::new();
        let mut out = Vec::new();
        t.clientbound(State::Play, &Packet::new(new::TEST_INSTANCE_BLOCK_STATUS, vec![1, 2]), &mut out).unwrap();
        assert!(out.is_empty());
        // CLEAR_TITLES (1.21.5 id 14) has no payload changes → id + 1.
        t.clientbound(State::Play, &Packet::new(14, vec![0]), &mut out).unwrap();
        assert_eq!(out[0].id, 15);
    }

    #[test]
    fn serverbound_chat_gains_checksum() {
        let mut t = V1_21_5To1_21_4::new();
        let mut out = Vec::new();
        t.serverbound(State::Play, &Packet::new(ids::old::S_CHAT, vec![5]), &mut out).unwrap();
        assert_eq!(out[0].id, new::S_CHAT);
        assert_eq!(out[0].payload, vec![5, 0]);
    }
}
