//! 1.21.6 (protocol 771) ↔ 1.21.5 (protocol 770).
//!
//! Packet layout differences follow the vanilla protocol changes between these versions; registry id tables are
//! generated from the game's registry mapping data (see `tools/gen_mappings.py`). Dialogs and waypoints
//! do not exist for the old client and are dropped.

pub mod entity;
pub mod ids;
pub mod items;
pub mod misc;
pub mod world;

use super::{Packet, State, TResult, Translator};
use crate::proto::buf::DecodeError;
use crate::proto::version::Version;
use entity::Entities;
use ids::new;
use items::{Dir, Items};
use misc::Misc;
use world::World;

pub struct V1_21_6To1_21_5 {
    to_client: Items,
    to_server: Items,
    entities: Entities,
    misc: Misc,
}

impl V1_21_6To1_21_5 {
    pub fn new() -> Self {
        Self { to_client: Items::new(Dir::ToClient), to_server: Items::new(Dir::ToServer), entities: Entities::default(), misc: Misc::new() }
    }

    fn play_clientbound(&mut self, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        let items = &self.to_client;
        let p = &pkt.payload[..];
        let e = |e: DecodeError| format!("clientbound 0x{:02x}: {e}", pkt.id);
        let translated: Option<Vec<u8>> = match pkt.id {
            new::ADD_ENTITY | new::SET_ENTITY_DATA => {
                let mut produced = Vec::new();
                if pkt.id == new::ADD_ENTITY {
                    self.entities.add_entity(items, p, &mut produced).map_err(e)?;
                } else {
                    self.entities.set_entity_data(items, &self.misc, p, &mut produced).map_err(e)?;
                }
                // Produced ids are already 1.21.5 ids (identical to 1.21.6 for these packets).
                out.extend(produced.into_iter().map(|(id, payload)| Packet::new(id, payload)));
                return Ok(());
            }
            new::REMOVE_ENTITIES => {
                self.entities.remove_entities(p).map_err(e)?;
                None
            }
            new::LOGIN | new::RESPAWN => {
                self.entities.clear();
                None
            }
            new::SET_EQUIPMENT => Some(Entities::set_equipment(items, p).map_err(e)?),
            new::LEVEL_CHUNK_WITH_LIGHT => Some(World::level_chunk_with_light(items, p).map_err(e)?),
            new::BLOCK_UPDATE => Some(World::block_update(items, p).map_err(e)?),
            new::SECTION_BLOCKS_UPDATE => Some(World::section_blocks_update(items, p).map_err(e)?),
            new::BLOCK_EVENT => Some(World::block_event(items, p).map_err(e)?),
            new::LEVEL_EVENT => Some(World::level_event(items, p).map_err(e)?),
            new::UPDATE_TAGS => Some(World::update_tags(items, p).map_err(e)?),
            new::LEVEL_PARTICLES => Some(self.misc.level_particles(items, p).map_err(e)?),
            new::SOUND | new::SOUND_ENTITY => match self.misc.sound(items, p).map_err(e)? {
                Some(v) => Some(v),
                None => return Ok(()),
            },
            new::STOP_SOUND => Some(self.misc.stop_sound(p).map_err(e)?),
            new::EXPLODE => Some(self.misc.explode(items, p).map_err(e)?),
            new::CHANGE_DIFFICULTY => Some(self.misc.change_difficulty(p).map_err(e)?),
            new::UPDATE_ATTRIBUTES => Some(self.misc.update_attributes(items, p).map_err(e)?),
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
            new::TRACKED_WAYPOINT | new::CLEAR_DIALOG | new::SHOW_DIALOG => return Ok(()),
            _ => None,
        };
        let Some(mapped) = ids::play_clientbound(pkt.id) else {
            return Err(format!("no 1.21.5 equivalent for clientbound play packet 0x{:02x}", pkt.id));
        };
        out.push(Packet::new(mapped, translated.unwrap_or_else(|| pkt.payload.clone())));
        Ok(())
    }

    fn config_clientbound(&mut self, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        let e = |e: DecodeError| format!("config 0x{:02x}: {e}", pkt.id);
        let payload = match pkt.id {
            new::CONFIG_REGISTRY_DATA => match World::registry_data(&pkt.payload).map_err(e)? {
                Some(v) => v,
                None => return Ok(()),
            },
            new::CONFIG_UPDATE_TAGS => World::update_tags(&self.to_client, &pkt.payload).map_err(e)?,
            new::CONFIG_CLEAR_DIALOG | new::CONFIG_SHOW_DIALOG => return Ok(()),
            _ => pkt.payload.clone(),
        };
        let mapped = ids::config_clientbound(pkt.id).ok_or_else(|| format!("no 1.21.5 config packet for 0x{:02x}", pkt.id))?;
        out.push(Packet::new(mapped, payload));
        Ok(())
    }

    fn play_serverbound(&mut self, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        let e = |e: DecodeError| format!("serverbound 0x{:02x}: {e}", pkt.id);
        let items = &self.to_server;
        let p = &pkt.payload[..];
        let payload = match pkt.id {
            ids::old::S_CHANGE_DIFFICULTY => self.misc.change_difficulty_serverbound(p).map_err(e)?,
            ids::old::S_PLAYER_COMMAND => match self.misc.player_command(p).map_err(e)? {
                Some(v) => v,
                None => return Ok(()),
            },
            ids::old::S_SET_CREATIVE_MODE_SLOT => self.misc.set_creative_slot(items, p).map_err(e)?,
            ids::old::S_CONTAINER_CLICK => self.misc.container_click(items, p).map_err(e)?,
            _ => pkt.payload.clone(),
        };
        let mapped = ids::play_serverbound(pkt.id).ok_or_else(|| format!("unknown serverbound play packet 0x{:02x}", pkt.id))?;
        out.push(Packet::new(mapped, payload));
        Ok(())
    }
}

impl Default for V1_21_6To1_21_5 {
    fn default() -> Self {
        Self::new()
    }
}

impl Translator for V1_21_6To1_21_5 {
    fn from(&self) -> Version {
        Version { protocol: 771 }
    }
    fn to(&self) -> Version {
        Version { protocol: 770 }
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
            // Configuration serverbound ids are identical; 1.21.6 only appended CUSTOM_CLICK_ACTION.
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
    fn dialogs_and_waypoints_are_dropped() {
        let mut t = V1_21_6To1_21_5::new();
        let mut out = Vec::new();
        for id in [new::TRACKED_WAYPOINT, new::SHOW_DIALOG, new::CLEAR_DIALOG] {
            t.clientbound(State::Play, &Packet::new(id, vec![0]), &mut out).unwrap();
        }
        t.clientbound(State::Configuration, &Packet::new(new::CONFIG_SHOW_DIALOG, vec![0]), &mut out).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn unchanged_packets_pass_through() {
        let mut t = V1_21_6To1_21_5::new();
        let mut out = Vec::new();
        t.clientbound(State::Play, &Packet::new(38, vec![1, 2, 3]), &mut out).unwrap(); // keep alive
        assert_eq!(out[0].id, 38);
        assert_eq!(out[0].payload, vec![1, 2, 3]);
        out.clear();
        t.serverbound(State::Play, &Packet::new(26, vec![4]), &mut out).unwrap(); // 1.21.5 keep alive
        assert_eq!(out[0].id, 27);
    }
}
