//! 1.21.7 (protocol 772) ↔ 1.21.6 (protocol 771).
//!
//! Follows ViaBackwards' `Protocol1_21_7To1_21_6`: packet ids, packet layouts, data components and
//! entity data types are identical, so the step only remaps registry ids. The one content change is
//! the "Lava Chicken" music disc and its sound, which map to Pigstep and shift every later item and
//! sound id. Registry id tables are generated from ViaVersion's mapping files
//! (see `tools/gen_mappings.py`).

pub mod entity;
pub mod ids;
pub mod items;
pub mod misc;
pub mod world;

use super::{Packet, State, TResult, Translator};
use crate::proto::version::Version;
use entity::Entities;
use items::{Dir, Items};
use misc::Misc;
use world::World;

pub struct V1_21_7To1_21_6 {
    to_client: Items,
    to_server: Items,
    misc: Misc,
}

impl V1_21_7To1_21_6 {
    pub fn new() -> Self {
        Self { to_client: Items::new(Dir::ToClient), to_server: Items::new(Dir::ToServer), misc: Misc::new() }
    }

    fn play_clientbound(&mut self, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        let items = &self.to_client;
        let p = &pkt.payload[..];
        let e = |e: crate::proto::buf::DecodeError| format!("clientbound 0x{:02x}: {e}", pkt.id);
        let payload = match pkt.id {
            ids::ADD_ENTITY => Entities::add_entity(items, p).map_err(e)?,
            ids::SET_ENTITY_DATA => Entities::set_entity_data(items, &self.misc, p).map_err(e)?,
            ids::SET_EQUIPMENT => Entities::set_equipment(items, p).map_err(e)?,
            ids::AWARD_STATS => self.misc.award_stats(items, p).map_err(e)?,
            ids::CONTAINER_SET_CONTENT => self.misc.container_set_content(items, p).map_err(e)?,
            ids::CONTAINER_SET_SLOT => self.misc.container_set_slot(items, p).map_err(e)?,
            ids::SET_CURSOR_ITEM => self.misc.single_item(items, p, 0).map_err(e)?,
            ids::SET_PLAYER_INVENTORY => self.misc.single_item(items, p, 1).map_err(e)?,
            ids::COOLDOWN => self.misc.cooldown(items, p).map_err(e)?,
            ids::MERCHANT_OFFERS => self.misc.merchant_offers(items, p).map_err(e)?,
            ids::LEVEL_PARTICLES => self.misc.level_particles(items, p).map_err(e)?,
            ids::EXPLODE => self.misc.explode(items, p).map_err(e)?,
            ids::SOUND | ids::SOUND_ENTITY => self.misc.sound(items, p).map_err(e)?,
            ids::STOP_SOUND => self.misc.stop_sound(items, p).map_err(e)?,
            ids::RECIPE_BOOK_ADD => self.misc.recipe_book_add(items, p).map_err(e)?,
            ids::PLACE_GHOST_RECIPE => self.misc.place_ghost_recipe(items, p).map_err(e)?,
            ids::UPDATE_RECIPES => self.misc.update_recipes(items, p).map_err(e)?,
            ids::UPDATE_ADVANCEMENTS => self.misc.update_advancements(items, p).map_err(e)?,
            ids::UPDATE_TAGS => World::update_tags(items, p).map_err(e)?,
            _ => {
                out.push(pkt.clone());
                return Ok(());
            }
        };
        out.push(Packet::new(pkt.id, payload));
        Ok(())
    }

    fn config_clientbound(&mut self, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        let e = |e: crate::proto::buf::DecodeError| format!("config 0x{:02x}: {e}", pkt.id);
        let payload = match pkt.id {
            ids::CONFIG_REGISTRY_DATA => World::registry_data(&self.to_client, &pkt.payload).map_err(e)?,
            ids::CONFIG_UPDATE_TAGS => World::update_tags(&self.to_client, &pkt.payload).map_err(e)?,
            _ => {
                out.push(pkt.clone());
                return Ok(());
            }
        };
        out.push(Packet::new(pkt.id, payload));
        Ok(())
    }

    fn play_serverbound(&mut self, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        let items = &self.to_server;
        let p = &pkt.payload[..];
        let e = |e: crate::proto::buf::DecodeError| format!("serverbound 0x{:02x}: {e}", pkt.id);
        let payload = match pkt.id {
            ids::S_SET_CREATIVE_MODE_SLOT => self.misc.set_creative_slot(items, p).map_err(e)?,
            ids::S_CONTAINER_CLICK => self.misc.container_click(items, p).map_err(e)?,
            _ => {
                out.push(pkt.clone());
                return Ok(());
            }
        };
        out.push(Packet::new(pkt.id, payload));
        Ok(())
    }
}

impl Default for V1_21_7To1_21_6 {
    fn default() -> Self {
        Self::new()
    }
}

impl Translator for V1_21_7To1_21_6 {
    fn from(&self) -> Version {
        Version { protocol: 772 }
    }
    fn to(&self) -> Version {
        Version { protocol: 771 }
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
            // Configuration serverbound packets carry no registry ids that differ.
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
    fn unrelated_packets_pass_through() {
        let mut t = V1_21_7To1_21_6::new();
        let pkt = Packet::new(38, vec![1, 2, 3]); // keep alive
        let mut out = Vec::new();
        t.clientbound(State::Play, &pkt, &mut out).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, 38);
        assert_eq!(out[0].payload, vec![1, 2, 3]);
        let mut out = Vec::new();
        t.serverbound(State::Play, &Packet::new(27, vec![9]), &mut out).unwrap();
        assert_eq!(out[0].payload, vec![9]);
    }
}
