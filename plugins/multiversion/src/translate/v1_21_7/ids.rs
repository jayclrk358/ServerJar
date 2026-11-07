//! Packet ids for 1.21.9 (protocol 773, "new") and 1.21.7/1.21.8 (protocol 772, "old").
//! Source: ViaVersion's `ClientboundPackets1_21_9` / `ClientboundPackets1_21_6` enums (ordinal = id).
//!
//! 1.21.9 inserted DEBUG_BLOCK_VALUE..DEBUG_EVENT (26-29) and GAME_TEST_HIGHLIGHT_POS (39) into the
//! clientbound play list and appended CODE_OF_CONDUCT (19) / ACCEPT_CODE_OF_CONDUCT (9) to the
//! configuration lists. Serverbound play ids are identical in both versions.

pub mod new {
    // clientbound play (1.21.9)
    pub const BUNDLE_DELIMITER: i32 = 0;
    pub const ADD_ENTITY: i32 = 1;
    pub const BLOCK_ENTITY_DATA: i32 = 6;
    pub const BLOCK_EVENT: i32 = 7;
    pub const BLOCK_UPDATE: i32 = 8;
    pub const BOSS_EVENT: i32 = 9;
    pub const COMMANDS: i32 = 16;
    pub const CONTAINER_SET_CONTENT: i32 = 18;
    pub const CONTAINER_SET_SLOT: i32 = 20;
    pub const DEBUG_BLOCK_VALUE: i32 = 26;
    pub const DEBUG_CHUNK_VALUE: i32 = 27;
    pub const DEBUG_ENTITY_VALUE: i32 = 28;
    pub const DEBUG_EVENT: i32 = 29;
    pub const DISCONNECT: i32 = 32;
    pub const DISGUISED_CHAT: i32 = 33;
    pub const ENTITY_POSITION_SYNC: i32 = 35;
    pub const EXPLODE: i32 = 36;
    pub const GAME_TEST_HIGHLIGHT_POS: i32 = 39;
    pub const INITIALIZE_BORDER: i32 = 42;
    pub const LEVEL_CHUNK_WITH_LIGHT: i32 = 44;
    pub const LEVEL_EVENT: i32 = 45;
    pub const LEVEL_PARTICLES: i32 = 46;
    pub const LOGIN: i32 = 48;
    pub const MERCHANT_OFFERS: i32 = 50;
    pub const MOVE_ENTITY_POS: i32 = 51;
    pub const MOVE_ENTITY_POS_ROT: i32 = 52;
    pub const MOVE_ENTITY_ROT: i32 = 54;
    pub const OPEN_SCREEN: i32 = 57;
    pub const PLACE_GHOST_RECIPE: i32 = 61;
    pub const PLAYER_CHAT: i32 = 63;
    pub const PLAYER_INFO_UPDATE: i32 = 68;
    pub const PLAYER_ROTATION: i32 = 71;
    pub const RECIPE_BOOK_ADD: i32 = 72;
    pub const REMOVE_ENTITIES: i32 = 75;
    pub const RESPAWN: i32 = 80;
    pub const ROTATE_HEAD: i32 = 81;
    pub const SECTION_BLOCKS_UPDATE: i32 = 82;
    pub const SERVER_DATA: i32 = 84;
    pub const SET_ACTION_BAR_TEXT: i32 = 85;
    pub const SET_BORDER_CENTER: i32 = 86;
    pub const SET_CURSOR_ITEM: i32 = 94;
    pub const SET_DEFAULT_SPAWN_POSITION: i32 = 95;
    pub const SET_ENTITY_DATA: i32 = 97;
    pub const SET_ENTITY_MOTION: i32 = 99;
    pub const SET_EQUIPMENT: i32 = 100;
    pub const SET_OBJECTIVE: i32 = 104;
    pub const SET_PASSENGERS: i32 = 105;
    pub const SET_PLAYER_INVENTORY: i32 = 106;
    pub const SET_PLAYER_TEAM: i32 = 107;
    pub const SET_SUBTITLE_TEXT: i32 = 110;
    pub const SET_TITLE_TEXT: i32 = 112;
    pub const SOUND_ENTITY: i32 = 114;
    pub const SOUND: i32 = 115;
    pub const SYSTEM_CHAT: i32 = 119;
    pub const TAB_LIST: i32 = 120;
    pub const TELEPORT_ENTITY: i32 = 123;
    pub const UPDATE_ADVANCEMENTS: i32 = 128;
    pub const UPDATE_ATTRIBUTES: i32 = 129;
    pub const UPDATE_RECIPES: i32 = 131;
    pub const UPDATE_TAGS: i32 = 132;
    pub const PLAY_C_COUNT: i32 = 139;
    // clientbound config (1.21.9)
    pub const CONFIG_DISCONNECT: i32 = 2;
    pub const CONFIG_REGISTRY_DATA: i32 = 7;
    pub const CONFIG_UPDATE_TAGS: i32 = 13;
    pub const CONFIG_CODE_OF_CONDUCT: i32 = 19;
    pub const CONFIG_C_COUNT: i32 = 20;
    // serverbound play (1.21.9 == 1.21.7 list)
    pub const S_CONTAINER_CLICK: i32 = 17;
    pub const S_DEBUG_SUBSCRIPTION_REQUEST: i32 = 22;
    pub const S_MOVE_PLAYER_POS_ROT: i32 = 30;
    pub const S_MOVE_PLAYER_ROT: i32 = 31;
    pub const S_SET_CREATIVE_MODE_SLOT: i32 = 55;
    pub const S_PLAY_COUNT: i32 = 66;
    // serverbound config (1.21.9)
    pub const S_CONFIG_ACCEPT_CODE_OF_CONDUCT: i32 = 9;
    pub const S_CONFIG_COUNT: i32 = 10;
}

pub mod old {
    // clientbound play (1.21.7 == 1.21.6 list)
    pub const BUNDLE_DELIMITER: i32 = 0;
    pub const ADD_ENTITY: i32 = 1;
    pub const PLAYER_INFO_REMOVE: i32 = 62;
    pub const PLAYER_INFO_UPDATE: i32 = 63;
    pub const REMOVE_ENTITIES: i32 = 70;
    pub const SET_ENTITY_DATA: i32 = 92;
    pub const SET_EQUIPMENT: i32 = 95;
    pub const SET_PASSENGERS: i32 = 100;
    pub const PLAY_C_COUNT: i32 = 134;
    // serverbound play (1.21.7)
    pub const S_CONTAINER_CLICK: i32 = 17;
    pub const S_DEBUG_SUBSCRIPTION_REQUEST: i32 = 22;
    pub const S_MOVE_PLAYER_POS_ROT: i32 = 30;
    pub const S_MOVE_PLAYER_ROT: i32 = 31;
    pub const S_SET_CREATIVE_MODE_SLOT: i32 = 55;
    // serverbound config (1.21.7)
    pub const S_CONFIG_COUNT: i32 = 9;
}

/// Clientbound play id 1.21.9 → 1.21.7. `None` = the client has no such packet.
pub fn play_clientbound(id: i32) -> Option<i32> {
    match id {
        0..=25 => Some(id),
        26..=29 => None,
        30..=38 => Some(id - 4),
        39 => None,
        40..=138 => Some(id - 5),
        _ => None,
    }
}

/// Clientbound configuration id 1.21.9 → 1.21.7.
pub fn config_clientbound(id: i32) -> Option<i32> {
    match id {
        0..=18 => Some(id),
        _ => None,
    }
}

/// Serverbound play id 1.21.7 → 1.21.9 (identical lists).
pub fn play_serverbound(id: i32) -> Option<i32> {
    (0..new::S_PLAY_COUNT).contains(&id).then_some(id)
}

/// Serverbound configuration id 1.21.7 → 1.21.9 (1.21.9 only appends one packet).
pub fn config_serverbound(id: i32) -> Option<i32> {
    (0..old::S_CONFIG_COUNT).contains(&id).then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn id_maps_are_consistent() {
        // 1.21.6 enum: DEBUG_SAMPLE 26, DISCONNECT 28, GAME_EVENT 34, HORSE_SCREEN_OPEN 35, LOGIN 43,
        // SET_ENTITY_DATA 92, SOUND 110, UPDATE_TAGS 127, SHOW_DIALOG 133.
        assert_eq!(play_clientbound(30), Some(26));
        assert_eq!(play_clientbound(new::DISCONNECT), Some(28));
        assert_eq!(play_clientbound(38), Some(34));
        assert_eq!(play_clientbound(40), Some(35));
        assert_eq!(play_clientbound(new::LOGIN), Some(43));
        assert_eq!(play_clientbound(new::SET_ENTITY_DATA), Some(old::SET_ENTITY_DATA));
        assert_eq!(play_clientbound(new::SOUND), Some(110));
        assert_eq!(play_clientbound(new::UPDATE_TAGS), Some(127));
        assert_eq!(play_clientbound(138), Some(133));
        assert_eq!(play_clientbound(new::PLAYER_INFO_UPDATE), Some(old::PLAYER_INFO_UPDATE));
        assert_eq!(play_clientbound(new::REMOVE_ENTITIES), Some(old::REMOVE_ENTITIES));
        assert_eq!(play_clientbound(new::SET_EQUIPMENT), Some(old::SET_EQUIPMENT));
        assert_eq!(play_clientbound(new::SET_PASSENGERS), Some(old::SET_PASSENGERS));
        assert_eq!(play_clientbound(new::DEBUG_EVENT), None);
        assert_eq!(play_clientbound(new::GAME_TEST_HIGHLIGHT_POS), None);
        assert_eq!(config_clientbound(new::CONFIG_CODE_OF_CONDUCT), None);
        assert_eq!(config_clientbound(18), Some(18));
        assert_eq!(play_serverbound(65), Some(65));
        assert_eq!(play_serverbound(66), None);
        assert_eq!(config_serverbound(8), Some(8));
        assert_eq!(config_serverbound(9), None);
    }
}
