//! 1.21.11 (protocol 774) ↔ 1.21.9 (protocol 773, also used by 1.21.10).
//!
//! Packet layout differences follow the vanilla protocol changes between these versions; registry id tables
//! are generated from the game's registry mapping data (see `tools/gen_mappings.py`). Packet ids are
//! identical, and so are block states, so chunks pass through untouched.

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

pub struct V1_21_11To1_21_9 {
    to_client: Items,
    to_server: Items,
    entities: Entities,
    misc: Misc,
    /// Server game time, advanced by the client's tick-end packets (for anger timers).
    game_time: i64,
}

impl V1_21_11To1_21_9 {
    pub fn new() -> Self {
        Self {
            to_client: Items::new(Dir::ToClient),
            to_server: Items::new(Dir::ToServer),
            entities: Entities::default(),
            misc: Misc::new(),
            game_time: 0,
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
            new::LOGIN | new::RESPAWN => {
                self.entities.clear();
                None
            }
            new::SET_ENTITY_DATA => Some(self.entities.set_entity_data(items, &self.misc, self.game_time, p).map_err(e)?),
            new::SET_EQUIPMENT => Some(Entities::set_equipment(items, p).map_err(e)?),
            new::UPDATE_MOB_EFFECT | new::REMOVE_MOB_EFFECT => match Entities::mob_effect(p).map_err(e)? {
                Some(v) => Some(v),
                None => return Ok(()),
            },
            new::SET_TIME => {
                self.game_time = self.misc.game_time(p).map_err(e)?;
                None
            }
            new::SET_BORDER_LERP_SIZE => Some(self.misc.border_lerp(p, 2).map_err(e)?),
            new::INITIALIZE_BORDER => Some(self.misc.border_lerp(p, 4).map_err(e)?),
            new::LEVEL_PARTICLES => Some(self.misc.level_particles(items, p).map_err(e)?),
            new::SOUND | new::SOUND_ENTITY => Some(self.misc.sound(items, p).map_err(e)?),
            new::EXPLODE => Some(self.misc.explode(items, p).map_err(e)?),
            new::AWARD_STATS => Some(self.misc.award_stats(items, p).map_err(e)?),
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
            new::UPDATE_TAGS => Some(World::update_tags(items, p).map_err(e)?),
            _ => None,
        };
        let mapped = ids::play_clientbound(pkt.id).ok_or_else(|| format!("no 1.21.9 equivalent for clientbound play packet 0x{:02x}", pkt.id))?;
        out.push(Packet::new(mapped, translated.unwrap_or_else(|| pkt.payload.clone())));
        Ok(())
    }

    fn config_clientbound(&mut self, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        let e = |e: crate::proto::buf::DecodeError| format!("config 0x{:02x}: {e}", pkt.id);
        let payload = match pkt.id {
            new::CONFIG_REGISTRY_DATA => match World::registry_data(&pkt.payload).map_err(e)? {
                Some(v) => v,
                None => return Ok(()),
            },
            new::CONFIG_UPDATE_TAGS => World::update_tags(&self.to_client, &pkt.payload).map_err(e)?,
            _ => pkt.payload.clone(),
        };
        let mapped = ids::config_clientbound(pkt.id).ok_or_else(|| format!("no 1.21.9 config packet for 0x{:02x}", pkt.id))?;
        out.push(Packet::new(mapped, payload));
        Ok(())
    }

    fn play_serverbound(&mut self, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        let e = |e: crate::proto::buf::DecodeError| format!("serverbound 0x{:02x}: {e}", pkt.id);
        let items = &self.to_server;
        let p = &pkt.payload[..];
        let payload = match pkt.id {
            new::S_CLIENT_TICK_END => {
                self.game_time += 1;
                pkt.payload.clone()
            }
            new::S_SET_CREATIVE_MODE_SLOT => self.misc.set_creative_slot(items, p).map_err(e)?,
            new::S_CONTAINER_CLICK => self.misc.container_click(items, p).map_err(e)?,
            _ => pkt.payload.clone(),
        };
        let mapped = ids::play_serverbound(pkt.id).ok_or_else(|| format!("unknown serverbound play packet 0x{:02x}", pkt.id))?;
        out.push(Packet::new(mapped, payload));
        Ok(())
    }
}

impl Default for V1_21_11To1_21_9 {
    fn default() -> Self {
        Self::new()
    }
}

impl Translator for V1_21_11To1_21_9 {
    fn from(&self) -> Version {
        Version { protocol: 774 }
    }
    fn to(&self) -> Version {
        Version { protocol: 773 }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::buf::{Reader, Writer};

    #[test]
    fn anger_timer_uses_tracked_game_time() {
        let mut t = V1_21_11To1_21_9::new();
        let mut time = Writer::new();
        time.i64(500);
        time.i64(500);
        time.bool(true);
        let mut out = Vec::new();
        t.clientbound(State::Play, &Packet::new(new::SET_TIME, time.into_inner()), &mut out).unwrap();
        for _ in 0..10 {
            t.serverbound(State::Play, &Packet::new(new::S_CLIENT_TICK_END, Vec::new()), &mut out).unwrap();
        }
        assert_eq!(t.game_time, 510);
        let mut effect = Writer::new();
        effect.varint(1);
        effect.varint(39);
        out.clear();
        t.clientbound(State::Play, &Packet::new(new::UPDATE_MOB_EFFECT, effect.into_inner()), &mut out).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn creative_slot_remaps_back_to_server() {
        let mut t = V1_21_11To1_21_9::new();
        let mut p = Writer::new();
        p.i16(36);
        p.varint(1);
        p.varint(super::super::data_1_21_9::ITEMS_REVERSE.len() as i32 - 1);
        p.varint(0);
        p.varint(0);
        let mut out = Vec::new();
        t.serverbound(State::Play, &Packet::new(new::S_SET_CREATIVE_MODE_SLOT, p.into_inner()), &mut out).unwrap();
        let mut r = Reader::new(&out[0].payload);
        r.i16().unwrap();
        r.varint().unwrap();
        let id = r.varint().unwrap();
        let last = super::super::data_1_21_9::ITEMS_REVERSE.len() - 1;
        assert_eq!(id, super::super::data_1_21_9::ITEMS_REVERSE[last] as i32);
    }
}
