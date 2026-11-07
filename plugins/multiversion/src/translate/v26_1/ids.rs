//! Packet ids for 26.2 (protocol 776, server side) and 26.1 (protocol 775, client side).
//!
//! Both versions share ViaVersion's `ClientboundPackets26_1` / `ServerboundPackets26_1` enums
//! and the 1.21.9 configuration enums, so every id is identical; only payloads differ.

pub mod new {
    // clientbound play (26.2 == 26.1)
    pub const ADD_ENTITY: i32 = 1;
    pub const BLOCK_ENTITY_DATA: i32 = 6;
    pub const BLOCK_EVENT: i32 = 7;
    pub const BLOCK_UPDATE: i32 = 8;
    pub const COMMANDS: i32 = 16;
    pub const CONTAINER_SET_CONTENT: i32 = 18;
    pub const CONTAINER_SET_SLOT: i32 = 20;
    pub const EXPLODE: i32 = 36;
    pub const LEVEL_CHUNK_WITH_LIGHT: i32 = 45;
    pub const LEVEL_EVENT: i32 = 46;
    pub const LEVEL_PARTICLES: i32 = 47;
    pub const LOGIN: i32 = 49;
    pub const MERCHANT_OFFERS: i32 = 52;
    pub const PLACE_GHOST_RECIPE: i32 = 63;
    pub const RECIPE_BOOK_ADD: i32 = 74;
    pub const REMOVE_ENTITIES: i32 = 77;
    pub const RESPAWN: i32 = 82;
    pub const SECTION_BLOCKS_UPDATE: i32 = 84;
    pub const SET_CURSOR_ITEM: i32 = 96;
    pub const SET_ENTITY_DATA: i32 = 99;
    pub const SET_EQUIPMENT: i32 = 102;
    pub const SET_PLAYER_INVENTORY: i32 = 108;
    pub const SET_PLAYER_TEAM: i32 = 109;
    pub const SOUND_ENTITY: i32 = 116;
    pub const SOUND: i32 = 117;
    pub const UPDATE_ADVANCEMENTS: i32 = 130;
    pub const UPDATE_ATTRIBUTES: i32 = 131;
    pub const UPDATE_RECIPES: i32 = 133;
    pub const UPDATE_TAGS: i32 = 134;
    pub const PLAY_C_COUNT: i32 = 141;
    // clientbound config (1.21.9 enum, shared)
    pub const CONFIG_REGISTRY_DATA: i32 = 7;
    pub const CONFIG_UPDATE_TAGS: i32 = 13;
    pub const CONFIG_C_COUNT: i32 = 20;
    // serverbound play (26.2 == 26.1)
    pub const S_CONTAINER_CLICK: i32 = 18;
    pub const S_SET_CREATIVE_MODE_SLOT: i32 = 56;
    pub const S_SPECTATE_ENTITY: i32 = 62;
    pub const S_PLAY_COUNT: i32 = 69;
}

pub mod old {
    pub const BLOCK_ENTITY_DATA: i32 = super::new::BLOCK_ENTITY_DATA;
    pub const SET_ENTITY_DATA: i32 = super::new::SET_ENTITY_DATA;
    pub const S_CONTAINER_CLICK: i32 = super::new::S_CONTAINER_CLICK;
    pub const S_SET_CREATIVE_MODE_SLOT: i32 = super::new::S_SET_CREATIVE_MODE_SLOT;
    pub const S_SPECTATE_ENTITY: i32 = super::new::S_SPECTATE_ENTITY;
}

/// Clientbound play id 26.2 → 26.1 (identity for every known packet).
pub fn play_clientbound(id: i32) -> Option<i32> {
    (0..new::PLAY_C_COUNT).contains(&id).then_some(id)
}

/// Clientbound configuration id 26.2 → 26.1 (identity).
pub fn config_clientbound(id: i32) -> Option<i32> {
    (0..new::CONFIG_C_COUNT).contains(&id).then_some(id)
}

/// Serverbound play id 26.1 → 26.2 (identity).
pub fn play_serverbound(id: i32) -> Option<i32> {
    (0..new::S_PLAY_COUNT).contains(&id).then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ids_are_identity() {
        // 26.1 enum: SET_PLAYER_TEAM 0x6D, LOGIN 0x31, SHOW_DIALOG 0x8C is the last clientbound id.
        assert_eq!(play_clientbound(new::SET_PLAYER_TEAM), Some(0x6D));
        assert_eq!(play_clientbound(new::LOGIN), Some(0x31));
        assert_eq!(play_clientbound(0x8C), Some(0x8C));
        assert_eq!(play_clientbound(0x8D), None);
        assert_eq!(config_clientbound(new::CONFIG_UPDATE_TAGS), Some(0x0D));
        assert_eq!(config_clientbound(19), Some(19));
        assert_eq!(play_serverbound(new::S_SPECTATE_ENTITY), Some(0x3E));
        assert_eq!(play_serverbound(68), Some(68));
        assert_eq!(play_serverbound(69), None);
    }
}
