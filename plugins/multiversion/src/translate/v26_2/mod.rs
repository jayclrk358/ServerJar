//! 26.3 (protocol 777) ↔ 26.2 (protocol 776).
//!
//! Packet layout differences follow ViaBackwards' `Protocol26_3To26_2`; registry id tables are
//! generated from ViaVersion's mapping files (see `tools/gen_mappings.py`).

pub mod entity;
pub mod ids;
pub mod items;
pub mod misc;
pub mod world;

use super::{data_26_2, Packet, State, TResult, Translator};
use crate::proto::buf::{Reader, Writer};
use crate::proto::version::Version;
use entity::Entities;
use ids::new;
use items::{Dir, Items};
use misc::Misc;
use world::World;

pub struct V26_3To26_2 {
    to_client: Items,
    to_server: Items,
    entities: Entities,
    misc: Misc,
    /// Teleport id the client acknowledged; sent to the server with its next position packet.
    pending_teleport: Option<i32>,
}

impl V26_3To26_2 {
    pub fn new() -> Self {
        Self {
            to_client: Items::new(Dir::ToClient),
            to_server: Items::new(Dir::ToServer),
            entities: Entities::default(),
            misc: Misc::new(),
            pending_teleport: None,
        }
    }

    fn play_clientbound(&mut self, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        let items = &self.to_client;
        let p = &pkt.payload[..];
        let e = |e: crate::proto::buf::DecodeError| format!("clientbound 0x{:02x}: {e}", pkt.id);
        // Packets that change shape or carry ids.
        let translated: Option<Vec<u8>> = match pkt.id {
            new::ADD_ENTITY => {
                let mut produced = Vec::new();
                self.entities.add_entity(items, p, &mut produced).map_err(e)?;
                for (id, payload) in produced {
                    let mapped = ids::play_clientbound(id).ok_or("unmapped id")?;
                    out.push(Packet::new(mapped, payload));
                }
                return Ok(());
            }
            new::REMOVE_ENTITIES => {
                self.entities.remove_entities(p).map_err(e)?;
                Some(p.to_vec())
            }
            new::SET_ENTITY_DATA => Some(self.entities.set_entity_data(items, &self.misc, p).map_err(e)?),
            new::MOVE_ENTITY_POS => Some(Entities::move_entity(p, false).map_err(e)?),
            new::MOVE_ENTITY_POS_ROT => Some(Entities::move_entity(p, true).map_err(e)?),
            new::MOVE_ENTITY_ROT => Some(Entities::move_entity_rot(p).map_err(e)?),
            new::ENTITY_POSITION_SYNC => Some(Entities::entity_position_sync(p).map_err(e)?),
            new::LOGIN => {
                self.entities.clear();
                Some(Entities::login(p).map_err(e)?)
            }
            new::RESPAWN => {
                self.entities.clear();
                Some(Entities::respawn(p).map_err(e)?)
            }
            new::ANIMATE => Some(Entities::animate(p).map_err(e)?),
            new::SWING_ANIMATION => {
                if let Some(payload) = Entities::swing_animation(p).map_err(e)? {
                    out.push(Packet::new(ids::old::ANIMATE, payload));
                }
                return Ok(());
            }
            new::SET_EQUIPMENT => Some(Entities::set_equipment(items, p).map_err(e)?),
            new::LEVEL_CHUNK_WITH_LIGHT => Some(World::level_chunk_with_light(items, p).map_err(e)?),
            new::LIGHT_UPDATE => Some(World::light_update(p).map_err(e)?),
            new::BLOCK_UPDATE => Some(World::block_update(items, p).map_err(e)?),
            new::SECTION_BLOCKS_UPDATE => Some(World::section_blocks_update(items, p).map_err(e)?),
            new::BLOCK_EVENT => Some(World::block_event(items, p).map_err(e)?),
            new::LEVEL_EVENT => Some(World::level_event(items, p).map_err(e)?),
            new::BLOCK_ENTITY_DATA => Some(World::block_entity_data(p).map_err(e)?),
            new::UPDATE_TAGS => Some(World::update_tags(items, p).map_err(e)?),
            new::LEVEL_PARTICLES => match self.misc.level_particles(items, p) {
                Ok(v) => Some(v),
                Err(err) => {
                    // A particle is only a visual effect: drop it rather than disconnect the player.
                    let id = crate::proto::buf::Reader::new(p).varint().unwrap_or(-1);
                    let name = super::data_26_2::NEW_PARTICLE_NAMES.get(id.max(0) as usize).copied().unwrap_or("?");
                    crate::log::warn_once(&format!("particle:{name}"), &format!("dropped a {name} particle the 26.3 -> 26.2 step could not decode ({err}); please report this"));
                    return Ok(());
                }
            },
            new::SOUND | new::SOUND_ENTITY => Some(self.misc.sound(items, p).map_err(e)?),
            new::EXPLODE => Some(self.misc.explode(items, p).map_err(e)?),
            new::OPEN_SCREEN => Some(self.misc.open_screen(p).map_err(e)?),
            new::CONTAINER_SET_DATA => match self.misc.container_set_data(p).map_err(e)? {
                Some(v) => Some(v),
                None => return Ok(()),
            },
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
            new::MAP_ITEM_DATA => Some(self.misc.map_item_data(p).map_err(e)?),
            new::OPEN_SIGN_EDITOR => Some(self.misc.open_sign_editor(p).map_err(e)?),
            new::PLAYER_CHAT => Some(self.misc.player_chat(p).map_err(e)?),
            new::ADD_TRANSIENT_BLOCK | new::POST_EFFECTS => return Ok(()),
            _ => None,
        };
        let Some(mapped) = ids::play_clientbound(pkt.id) else {
            return Err(format!("no 26.2 equivalent for clientbound play packet 0x{:02x}", pkt.id));
        };
        out.push(Packet::new(mapped, translated.unwrap_or_else(|| pkt.payload.clone())));
        Ok(())
    }

    fn config_clientbound(&mut self, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        let e = |e: crate::proto::buf::DecodeError| format!("config 0x{:02x}: {e}", pkt.id);
        let payload = match pkt.id {
            new::CONFIG_REGISTRY_DATA => match World::registry_data(&self.to_client, &pkt.payload).map_err(e)? {
                Some(v) => v,
                None => return Ok(()),
            },
            new::CONFIG_UPDATE_TAGS => World::update_tags(&self.to_client, &pkt.payload).map_err(e)?,
            new::CONFIG_POST_EFFECTS => return Ok(()),
            _ => pkt.payload.clone(),
        };
        let mapped = ids::config_clientbound(pkt.id).ok_or_else(|| format!("no 26.2 config packet for 0x{:02x}", pkt.id))?;
        out.push(Packet::new(mapped, payload));
        Ok(())
    }

    fn play_serverbound(&mut self, pkt: &Packet, out: &mut Vec<Packet>) -> TResult {
        let e = |e: crate::proto::buf::DecodeError| format!("serverbound 0x{:02x}: {e}", pkt.id);
        let items = &self.to_server;
        let p = &pkt.payload[..];
        match pkt.id {
            ids::old::S_SWING => {
                // 26.3 "punch" has no hand; off-hand swings have no equivalent.
                let hand = Reader::new(p).varint().map_err(e)?;
                if hand == 0 {
                    out.push(Packet::new(new::S_PUNCH, Vec::new()));
                }
                Ok(())
            }
            ids::old::S_PLAYER_ACTION => {
                let mut r = Reader::new(p);
                let action = r.varint().map_err(e)?;
                let mut w = Writer::new();
                w.varint(if action >= 1 { action + 1 } else { action });
                w.raw(r.remaining());
                out.push(Packet::new(new::S_PLAYER_ACTION, w.into_inner()));
                Ok(())
            }
            ids::old::S_ACCEPT_TELEPORTATION => {
                // 26.3 wants the position with the ack; the client sends it right after.
                self.pending_teleport = Some(Reader::new(p).varint().map_err(e)?);
                Ok(())
            }
            ids::old::S_MOVE_PLAYER_POS_ROT => {
                if let Some(id) = self.pending_teleport.take() {
                    let mut r = Reader::new(p);
                    let mut w = Writer::new();
                    w.varint(id);
                    w.raw(r.bytes(24 + 8).map_err(e)?); // x y z yaw pitch
                    out.push(Packet::new(new::S_ACCEPT_TELEPORTATION, w.into_inner()));
                } else {
                    out.push(Packet::new(new::S_MOVE_PLAYER_POS_ROT, p.to_vec()));
                }
                Ok(())
            }
            ids::old::S_SIGN_UPDATE => {
                out.push(Packet::new(new::S_SIGN_UPDATE, self.misc.sign_update(p).map_err(e)?));
                Ok(())
            }
            ids::old::S_SET_CREATIVE_MODE_SLOT => {
                out.push(Packet::new(new::S_SET_CREATIVE_MODE_SLOT, self.misc.set_creative_slot(items, p).map_err(e)?));
                Ok(())
            }
            ids::old::S_CONTAINER_CLICK => {
                out.push(Packet::new(new::S_CONTAINER_CLICK, self.misc.container_click(items, p).map_err(e)?));
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

impl Default for V26_3To26_2 {
    fn default() -> Self {
        Self::new()
    }
}

impl Translator for V26_3To26_2 {
    fn from(&self) -> Version {
        Version { protocol: 777 }
    }
    fn to(&self) -> Version {
        Version { protocol: 776 }
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
            // Configuration serverbound ids are identical in 26.2 and 26.3.
            State::Configuration => {
                out.push(pkt.clone());
                Ok(())
            }
        }
    }
}

#[allow(dead_code)]
fn _data_used() -> usize {
    data_26_2::ITEMS.len()
}
