//! Packet ids for 1.21.4 (protocol 769, server side) and 1.21.2/1.21.3 (protocol 768, client side).
//! Source: ViaVersion's `ClientboundPackets1_21_2` (shared by both versions),
//! `ServerboundPackets1_21_4` and `ServerboundPackets1_21_2`, plus the configuration enums
//! `ClientboundConfigurationPackets1_21` / `ServerboundConfigurationPackets1_20_5` (identical on
//! both sides).

/// 1.21.4 ids. Clientbound ids (play and configuration) are the same in 1.21.2.
pub mod new {
    // clientbound play
    pub const ADD_ENTITY: i32 = 0x01;
    pub const BLOCK_ENTITY_DATA: i32 = 0x07;
    pub const BLOCK_EVENT: i32 = 0x08;
    pub const BLOCK_UPDATE: i32 = 0x09;
    pub const COMMANDS: i32 = 0x11;
    pub const CONTAINER_SET_CONTENT: i32 = 0x13;
    pub const CONTAINER_SET_SLOT: i32 = 0x15;
    pub const EXPLODE: i32 = 0x21;
    pub const LEVEL_CHUNK_WITH_LIGHT: i32 = 0x28;
    pub const LEVEL_EVENT: i32 = 0x29;
    pub const LEVEL_PARTICLES: i32 = 0x2A;
    pub const LIGHT_UPDATE: i32 = 0x2B;
    pub const LOGIN: i32 = 0x2C;
    pub const MERCHANT_OFFERS: i32 = 0x2E;
    pub const PLACE_GHOST_RECIPE: i32 = 0x39;
    pub const PLAYER_INFO_UPDATE: i32 = 0x40;
    pub const RECIPE_BOOK_ADD: i32 = 0x44;
    pub const REMOVE_ENTITIES: i32 = 0x47;
    pub const RESPAWN: i32 = 0x4C;
    pub const SECTION_BLOCKS_UPDATE: i32 = 0x4E;
    pub const SET_CURSOR_ITEM: i32 = 0x5A;
    pub const SET_ENTITY_DATA: i32 = 0x5D;
    pub const SET_EQUIPMENT: i32 = 0x60;
    pub const SET_HELD_SLOT: i32 = 0x63;
    pub const SET_PLAYER_INVENTORY: i32 = 0x66;
    pub const SOUND_ENTITY: i32 = 0x6E;
    pub const SOUND: i32 = 0x6F;
    pub const UPDATE_ADVANCEMENTS: i32 = 0x7B;
    pub const UPDATE_ATTRIBUTES: i32 = 0x7C;
    pub const UPDATE_RECIPES: i32 = 0x7E;
    pub const UPDATE_TAGS: i32 = 0x7F;
    pub const DISGUISED_CHAT: i32 = 0x1E;
    pub const SET_ACTION_BAR_TEXT: i32 = 0x51;
    pub const SET_SUBTITLE_TEXT: i32 = 0x6A;
    pub const SET_TITLE_TEXT: i32 = 0x6C;
    pub const SYSTEM_CHAT: i32 = 0x73;
    pub const TAB_LIST: i32 = 0x74;
    pub const PLAY_C_COUNT: i32 = 0x83;
    // clientbound configuration
    pub const CONFIG_REGISTRY_DATA: i32 = 0x07;
    pub const CONFIG_UPDATE_ENABLED_FEATURES: i32 = 0x0C;
    pub const CONFIG_UPDATE_TAGS: i32 = 0x0D;
    pub const CONFIG_C_COUNT: i32 = 0x11;
    // serverbound play (1.21.4)
    pub const S_CONTAINER_CLICK: i32 = 0x10;
    pub const S_MOVE_VEHICLE: i32 = 0x20;
    pub const S_PLAYER_LOADED: i32 = 0x2A;
    pub const S_SET_CREATIVE_MODE_SLOT: i32 = 0x36;
    pub const S_COUNT: i32 = 0x3E;
}

/// 1.21.2 ids where they differ from 1.21.4 (serverbound only).
pub mod old {
    pub const S_CONTAINER_CLICK: i32 = 0x10;
    pub const S_MOVE_VEHICLE: i32 = 0x20;
    pub const S_PICK_ITEM: i32 = 0x22;
    pub const S_SET_CREATIVE_MODE_SLOT: i32 = 0x34;
    pub const S_COUNT: i32 = 0x3C;
}

/// Clientbound play id 1.21.4 → 1.21.2 (identical enums).
pub fn play_clientbound(id: i32) -> Option<i32> {
    (0..new::PLAY_C_COUNT).contains(&id).then_some(id)
}

/// Clientbound configuration id 1.21.4 → 1.21.2 (identical enums).
pub fn config_clientbound(id: i32) -> Option<i32> {
    (0..new::CONFIG_C_COUNT).contains(&id).then_some(id)
}

/// Serverbound play id 1.21.2 → 1.21.4. `None` = 1.21.4 has no such packet (PICK_ITEM).
pub fn play_serverbound(id: i32) -> Option<i32> {
    match id {
        0x00..=0x21 => Some(id),
        old::S_PICK_ITEM => None,
        // PICK_ITEM_FROM_BLOCK/ENTITY took PICK_ITEM's place (+1), PLAYER_LOADED was added at 0x2A (+2).
        0x23..=0x28 => Some(id + 1),
        0x29..=0x3B => Some(id + 2),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn serverbound_shift_matches_viaversion_enums() {
        // 1.21.2 PING_REQUEST 0x23 -> 1.21.4 0x24; PLAYER_INPUT 0x28 -> 0x29; PONG 0x29 -> 0x2B
        assert_eq!(play_serverbound(0x23), Some(0x24));
        assert_eq!(play_serverbound(0x28), Some(0x29));
        assert_eq!(play_serverbound(0x29), Some(0x2B));
        assert_eq!(play_serverbound(old::S_SET_CREATIVE_MODE_SLOT), Some(new::S_SET_CREATIVE_MODE_SLOT));
        assert_eq!(play_serverbound(old::S_CONTAINER_CLICK), Some(new::S_CONTAINER_CLICK));
        assert_eq!(play_serverbound(old::S_MOVE_VEHICLE), Some(new::S_MOVE_VEHICLE));
        assert_eq!(play_serverbound(old::S_PICK_ITEM), None);
        assert_eq!(play_serverbound(old::S_COUNT - 1), Some(new::S_COUNT - 1)); // USE_ITEM
        assert_eq!(play_serverbound(old::S_COUNT), None);
        assert_eq!(play_clientbound(new::PLAY_C_COUNT - 1), Some(0x82));
        assert_eq!(play_clientbound(new::PLAY_C_COUNT), None);
        assert_eq!(config_clientbound(new::CONFIG_UPDATE_TAGS), Some(0x0D));
    }
}
