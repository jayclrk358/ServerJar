//! 1.21.2 (protocol 768) ↔ 1.21 / 1.21.1 (protocol 767).
//!
//! Packet layout differences follow the vanilla protocol changes between these versions, in both directions; registry id tables are generated from
//! the game's registry mapping data (see `tools/gen_mappings.py`).

pub mod entity;
pub mod ids;
pub mod items;
pub mod misc;
pub mod recipes;
pub mod world;

use super::{data_1_21, Packet, State, TResult, Translator};
use crate::proto::buf::Writer;
use crate::proto::version::Version;
use entity::Entities;
use ids::{new, old};
use items::{Dir, Items};
use misc::Misc;
use recipes::Recipes;
use world::World;

pub struct V1_21_2To1_21 {
    to_client: Items,
    to_server: Items,
    entities: Entities,
    misc: Misc,
    recipes: Recipes,
    /// Serverbound packets produced while handling clientbound ones; sent before the client's next.
    pending_to_server: Vec<Packet>,
}

impl V1_21_2To1_21 {
    pub fn new() -> Self {
        Self {
            to_client: Items::new(Dir::ToClient),
            to_server: Items::new(Dir::ToServer),
            entities: Entities::default(),
            misc: Misc::new(),
            recipes: Recipes::default(),
            pending_to_server: Vec::new(),
        }
    }

    fn queue_stop_sneaking(&mut self) {
        if let Some(cmd) = self.entities.sneak_command(false) {
            self.pending_to_server.push(Packet::new(new::S_PLAYER_COMMAND, cmd));
        }
        if let Some(input) = self.entities.input_update() {
            self.pending_to_server.push(Packet::new(new::S_PLAYER_INPUT, input));
        }
    }

    fn play_clientbound(&mut self, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        let items = &self.to_client;
        let p = &pkt.payload[..];
        let e = |e: crate::proto::buf::DecodeError| format!("clientbound 0x{:02x}: {e}", pkt.id);
        let mut produced: Vec<(i32, Vec<u8>)> = Vec::new();
        let translated: Option<Vec<u8>> = match pkt.id {
            new::ADD_ENTITY => {
                self.entities.add_entity(items, p, &mut produced).map_err(e)?;
                None
            }
            new::REMOVE_ENTITIES => {
                if self.entities.remove_entities(p).map_err(e)? {
                    self.queue_stop_sneaking();
                }
                Some(p.to_vec())
            }
            new::SET_PASSENGERS => {
                if self.entities.set_passengers(p).map_err(e)? {
                    self.queue_stop_sneaking();
                }
                Some(p.to_vec())
            }
            new::SET_ENTITY_DATA => {
                self.entities.set_entity_data(items, &self.misc, p, &mut produced).map_err(e)?;
                None
            }
            new::LOGIN => Some(self.entities.login(p).map_err(e)?),
            new::RESPAWN => Some(self.entities.respawn(p).map_err(e)?),
            new::ENTITY_POSITION_SYNC => {
                produced.push((old::TELEPORT_ENTITY, Entities::entity_position_sync(p).map_err(e)?));
                None
            }
            new::MOVE_MINECART_ALONG_TRACK => {
                Entities::move_minecart(p, &mut produced).map_err(e)?;
                None
            }
            new::TELEPORT_ENTITY => {
                self.entities.teleport_entity(p, &mut produced).map_err(e)?;
                None
            }
            new::PLAYER_POSITION => {
                self.entities.player_position(p, &mut produced).map_err(e)?;
                None
            }
            new::PLAYER_ROTATION => {
                let rot = self.entities.player_rotation(p, &mut produced).map_err(e)?;
                self.pending_to_server.push(Packet::new(new::S_MOVE_PLAYER_ROT, rot));
                None
            }
            new::SET_EQUIPMENT => Some(Entities::set_equipment(items, p).map_err(e)?),
            new::LEVEL_CHUNK_WITH_LIGHT => Some(World::level_chunk_with_light(items, p).map_err(e)?),
            new::BLOCK_UPDATE => Some(World::block_update(items, p).map_err(e)?),
            new::SECTION_BLOCKS_UPDATE => Some(World::section_blocks_update(items, p).map_err(e)?),
            new::BLOCK_EVENT => Some(World::block_event(items, p).map_err(e)?),
            new::LEVEL_EVENT => Some(World::level_event(items, p).map_err(e)?),
            new::BLOCK_ENTITY_DATA => match World::block_entity_data(p).map_err(e)? {
                Some(v) => Some(v),
                None => return Ok(()),
            },
            new::UPDATE_TAGS => Some(World::update_tags(items, p, &mut self.recipes.item_tags).map_err(e)?),
            new::LEVEL_PARTICLES => Some(self.misc.level_particles(items, p).map_err(e)?),
            new::SOUND | new::SOUND_ENTITY => match self.misc.sound(items, p).map_err(e)? {
                Some(v) => Some(v),
                None => return Ok(()),
            },
            new::EXPLODE => Some(self.misc.explode(items, p).map_err(e)?),
            new::OPEN_SCREEN => Some(self.misc.open_screen(p).map_err(e)?),
            new::CONTAINER_CLOSE => Some(self.misc.container_id_to_byte(p, true).map_err(e)?),
            new::HORSE_SCREEN_OPEN => Some(self.misc.container_id_to_byte(p, false).map_err(e)?),
            new::CONTAINER_SET_DATA => match self.misc.container_set_data(p).map_err(e)? {
                Some(v) => Some(v),
                None => return Ok(()),
            },
            new::CONTAINER_SET_CONTENT => Some(self.misc.container_set_content(items, p).map_err(e)?),
            new::CONTAINER_SET_SLOT => {
                let (slot_packet, armor) = self.misc.container_set_slot(items, p).map_err(e)?;
                produced.push((old::CONTAINER_SET_SLOT, slot_packet));
                if let Some((slot, item)) = armor {
                    // 1.21 only reads hotbar/offhand slot updates for the worn equipment
                    let mut w = Writer::new();
                    w.varint(self.entities.client_id);
                    w.i8((10 - slot) as i8);
                    w.raw(&item);
                    produced.push((old::SET_EQUIPMENT, w.into_inner()));
                }
                None
            }
            new::SET_CURSOR_ITEM => {
                produced.push((old::CONTAINER_SET_SLOT, self.misc.set_cursor_item(items, p).map_err(e)?));
                None
            }
            new::SET_PLAYER_INVENTORY => {
                produced.push((old::CONTAINER_SET_SLOT, self.misc.set_player_inventory(items, p).map_err(e)?));
                None
            }
            new::MERCHANT_OFFERS => Some(self.misc.merchant_offers(items, p).map_err(e)?),
            new::COOLDOWN => match self.misc.cooldown(items, p).map_err(e)? {
                Some(v) => Some(v),
                None => return Ok(()),
            },
            new::RECIPE_BOOK_ADD => {
                self.recipes.recipe_book_add(items, p, &mut produced).map_err(e)?;
                None
            }
            new::RECIPE_BOOK_REMOVE => {
                produced.push((old::RECIPE, self.recipes.recipe_book_remove(p).map_err(e)?));
                None
            }
            new::RECIPE_BOOK_SETTINGS => {
                self.recipes.recipe_book_settings(p).map_err(e)?;
                return Ok(());
            }
            new::UPDATE_RECIPES => {
                self.recipes.update_recipes(items, p, &mut produced).map_err(e)?;
                None
            }
            // a recipe display cannot be turned into the old ghost recipe reliably
            new::PLACE_GHOST_RECIPE => return Ok(()),
            new::UPDATE_ADVANCEMENTS => Some(self.misc.update_advancements(items, p).map_err(e)?),
            new::PLAYER_INFO_UPDATE => Some(self.misc.player_info_update(p).map_err(e)?),
            new::SET_TIME => Some(self.misc.set_time(p).map_err(e)?),
            new::UPDATE_ATTRIBUTES => Some(self.misc.update_attributes(items, p).map_err(e)?),
            new::SYSTEM_CHAT | new::DISGUISED_CHAT | new::SET_ACTION_BAR_TEXT | new::SET_TITLE_TEXT | new::SET_SUBTITLE_TEXT => {
                Some(self.misc.leading_text(p).map_err(e)?)
            }
            _ => Some(p.to_vec()),
        };
        if let Some(payload) = translated {
            let Some(mapped) = ids::play_clientbound(pkt.id) else {
                return Err(format!("no 1.21 equivalent for clientbound play packet 0x{:02x}", pkt.id));
            };
            out.push(Packet::new(mapped, payload));
        }
        out.extend(produced.into_iter().map(|(id, payload)| Packet::new(id, payload)));
        Ok(())
    }

    fn config_clientbound(&mut self, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        let e = |e: crate::proto::buf::DecodeError| format!("config 0x{:02x}: {e}", pkt.id);
        let payload = match pkt.id {
            new::CONFIG_REGISTRY_DATA => match World::registry_data(&pkt.payload).map_err(e)? {
                Some(v) => v,
                None => return Ok(()),
            },
            new::CONFIG_UPDATE_TAGS => World::update_tags(&self.to_client, &pkt.payload, &mut self.recipes.item_tags).map_err(e)?,
            new::CONFIG_UPDATE_ENABLED_FEATURES => World::enabled_features(&pkt.payload).map_err(e)?,
            _ => pkt.payload.clone(),
        };
        out.push(Packet::new(pkt.id, payload));
        Ok(())
    }

    fn play_serverbound(&mut self, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        let e = |e: crate::proto::buf::DecodeError| format!("serverbound 0x{:02x}: {e}", pkt.id);
        out.append(&mut self.pending_to_server);
        let items = &self.to_server;
        let p = &pkt.payload[..];
        let payload = match pkt.id {
            old::S_CLIENT_INFORMATION => with_particle_status(p),
            old::S_CONTAINER_CLICK => self.misc.container_click(items, p).map_err(e)?,
            old::S_CONTAINER_CLOSE => self.misc.container_close(p).map_err(e)?,
            old::S_SET_CREATIVE_MODE_SLOT => self.misc.set_creative_slot(items, p).map_err(e)?,
            old::S_USE_ITEM_ON => {
                let mut v = p.to_vec();
                v.push(0); // world border hit
                v
            }
            old::S_PLACE_RECIPE => match recipes::Recipes::place_recipe(p).map_err(e)? {
                Some(v) => v,
                None => return Ok(()),
            },
            old::S_RECIPE_BOOK_SEEN_RECIPE => match recipes::Recipes::seen_recipe(p).map_err(e)? {
                Some(v) => v,
                None => return Ok(()),
            },
            old::S_MOVE_PLAYER_POS | old::S_MOVE_PLAYER_POS_ROT | old::S_MOVE_PLAYER_ROT | old::S_MOVE_PLAYER_STATUS_ONLY => {
                let moved = self.entities.movement(pkt.id, p).map_err(e)?;
                out.push(Packet::new(ids::play_serverbound(pkt.id).unwrap_or(pkt.id), moved));
                if let Some(input) = self.entities.input_update() {
                    out.push(Packet::new(new::S_PLAYER_INPUT, input));
                }
                // 1.21 clients do not send tick ends; one movement packet per tick is the norm
                out.push(Packet::new(new::S_CLIENT_TICK_END, Vec::new()));
                return Ok(());
            }
            old::S_MOVE_VEHICLE => {
                self.entities.move_vehicle(p).map_err(e)?;
                p.to_vec()
            }
            old::S_PLAYER_COMMAND => {
                self.entities.player_command(p).map_err(e)?;
                out.push(Packet::new(new::S_PLAYER_COMMAND, p.to_vec()));
                if let Some(input) = self.entities.input_update() {
                    out.push(Packet::new(new::S_PLAYER_INPUT, input));
                }
                return Ok(());
            }
            old::S_PLAYER_INPUT => {
                let (input, command) = self.entities.player_input(p).map_err(e)?;
                out.push(Packet::new(new::S_PLAYER_INPUT, input));
                if let Some(cmd) = command {
                    out.push(Packet::new(new::S_PLAYER_COMMAND, cmd));
                }
                return Ok(());
            }
            _ => p.to_vec(),
        };
        let mapped = ids::play_serverbound(pkt.id).ok_or_else(|| format!("unknown serverbound play packet 0x{:02x}", pkt.id))?;
        out.push(Packet::new(mapped, payload));
        Ok(())
    }
}

/// CLIENT_INFORMATION: 1.21.2 appends the particle setting (0 = all).
fn with_particle_status(p: &[u8]) -> Vec<u8> {
    let mut w = Writer::with_capacity(p.len() + 1);
    w.raw(p);
    w.varint(0);
    w.into_inner()
}

impl Default for V1_21_2To1_21 {
    fn default() -> Self {
        Self::new()
    }
}

impl Translator for V1_21_2To1_21 {
    fn from(&self) -> Version {
        Version { protocol: 768 }
    }
    fn to(&self) -> Version {
        Version { protocol: 767 }
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
            State::Configuration => {
                // configuration serverbound ids are identical
                let payload = if pkt.id == old::S_CONFIG_CLIENT_INFORMATION { with_particle_status(&pkt.payload) } else { pkt.payload.clone() };
                out.push(Packet::new(pkt.id, payload));
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_drops_sea_level() {
        let mut t = V1_21_2To1_21::new();
        let mut p = Writer::new();
        p.i32(7).bool(false);
        p.varint(1).string("minecraft:overworld");
        p.varint(20).varint(10).varint(10);
        p.bool(false).bool(true).bool(false);
        p.varint(0).string("minecraft:overworld").i64(42);
        p.u8(0).u8(0xff).bool(false).bool(false);
        p.bool(false); // no death location
        p.varint(0); // portal cooldown
        p.varint(63); // sea level
        p.bool(true); // enforces secure chat
        let mut out = Vec::new();
        t.clientbound(State::Play, &Packet::new(new::LOGIN, p.buf.clone()), &mut out).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, 0x2B);
        assert_eq!(out[0].payload.len(), p.buf.len() - 1);
        assert_eq!(*out[0].payload.last().unwrap(), 1);
        assert_eq!(t.entities.client_id, 7);
    }

    #[test]
    fn movement_gets_flags_and_tick_end() {
        let mut t = V1_21_2To1_21::new();
        let mut p = Writer::new();
        p.bool(true);
        let mut out = Vec::new();
        t.serverbound(State::Play, &Packet::new(old::S_MOVE_PLAYER_STATUS_ONLY, p.buf), &mut out).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].id, 0x1F);
        assert_eq!(out[0].payload, vec![1]);
        assert_eq!(out[1].id, new::S_CLIENT_TICK_END);
    }

    #[test]
    fn client_information_gets_particle_status() {
        let mut t = V1_21_2To1_21::new();
        let mut out = Vec::new();
        t.serverbound(State::Configuration, &Packet::new(0, vec![1, 2, 3]), &mut out).unwrap();
        assert_eq!(out[0].payload, vec![1, 2, 3, 0]);
    }
}
