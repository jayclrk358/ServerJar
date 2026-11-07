//! Packet ids for 1.21.6 (protocol 771, the server side of this step) and 1.21.5 (protocol 770,
//! the client side). Source: ViaVersion's ClientboundPackets1_21_6 / ClientboundPackets1_21_5 and
//! the matching serverbound and configuration enums (the enum ordinal is the packet id).

pub mod new {
    // clientbound play (1.21.6)
    pub const ADD_ENTITY: i32 = 1;
    pub const BLOCK_ENTITY_DATA: i32 = 6;
    pub const BLOCK_EVENT: i32 = 7;
    pub const BLOCK_UPDATE: i32 = 8;
    pub const CHANGE_DIFFICULTY: i32 = 10;
    pub const COMMANDS: i32 = 16;
    pub const CONTAINER_SET_CONTENT: i32 = 18;
    pub const CONTAINER_SET_SLOT: i32 = 20;
    pub const EXPLODE: i32 = 32;
    pub const LEVEL_CHUNK_WITH_LIGHT: i32 = 39;
    pub const LEVEL_EVENT: i32 = 40;
    pub const LEVEL_PARTICLES: i32 = 41;
    pub const LOGIN: i32 = 43;
    pub const MERCHANT_OFFERS: i32 = 45;
    pub const PLACE_GHOST_RECIPE: i32 = 56;
    pub const RECIPE_BOOK_ADD: i32 = 67;
    pub const REMOVE_ENTITIES: i32 = 70;
    pub const RESPAWN: i32 = 75;
    pub const SECTION_BLOCKS_UPDATE: i32 = 77;
    pub const SET_CURSOR_ITEM: i32 = 89;
    pub const SET_ENTITY_DATA: i32 = 92;
    pub const SET_ENTITY_MOTION: i32 = 94;
    pub const SET_EQUIPMENT: i32 = 95;
    pub const SET_PLAYER_INVENTORY: i32 = 101;
    pub const SOUND_ENTITY: i32 = 109;
    pub const SOUND: i32 = 110;
    pub const STOP_SOUND: i32 = 112;
    pub const UPDATE_ADVANCEMENTS: i32 = 123;
    pub const UPDATE_ATTRIBUTES: i32 = 124;
    pub const UPDATE_RECIPES: i32 = 126;
    pub const UPDATE_TAGS: i32 = 127;
    pub const TRACKED_WAYPOINT: i32 = 131;
    pub const CLEAR_DIALOG: i32 = 132;
    pub const SHOW_DIALOG: i32 = 133;
    pub const PLAY_C_COUNT: i32 = 134;
    // clientbound configuration (1.21.6)
    pub const CONFIG_REGISTRY_DATA: i32 = 7;
    pub const CONFIG_UPDATE_TAGS: i32 = 13;
    pub const CONFIG_CLEAR_DIALOG: i32 = 17;
    pub const CONFIG_SHOW_DIALOG: i32 = 18;
    pub const CONFIG_C_COUNT: i32 = 19;
    // serverbound play (1.21.6)
    pub const S_CHANGE_DIFFICULTY: i32 = 3;
    pub const S_CONTAINER_CLICK: i32 = 17;
    pub const S_PLAYER_COMMAND: i32 = 41;
    pub const S_SET_CREATIVE_MODE_SLOT: i32 = 55;
}

pub mod old {
    // clientbound play (1.21.5): identical to 1.21.6 up to SERVER_LINKS (130)
    pub const LAST_PLAY_C: i32 = 130;
    pub const LAST_CONFIG_C: i32 = 16;
    pub const SET_ENTITY_DATA: i32 = 92;
    pub const SET_ENTITY_MOTION: i32 = 94;
    pub const UPDATE_ATTRIBUTES: i32 = 124;
    // serverbound play (1.21.5)
    pub const S_CHANGE_DIFFICULTY: i32 = 3;
    pub const S_CONTAINER_CLICK: i32 = 16;
    pub const S_PLAYER_COMMAND: i32 = 40;
    pub const S_SET_CREATIVE_MODE_SLOT: i32 = 54;
    pub const LAST_PLAY_S: i32 = 63;
}

/// Clientbound play id 1.21.6 → 1.21.5. `None` = the client has no such packet.
pub fn play_clientbound(id: i32) -> Option<i32> {
    match id {
        0..=old::LAST_PLAY_C => Some(id),
        _ => None,
    }
}

/// Clientbound configuration id 1.21.6 → 1.21.5.
pub fn config_clientbound(id: i32) -> Option<i32> {
    match id {
        0..=old::LAST_CONFIG_C => Some(id),
        _ => None,
    }
}

/// Serverbound play id 1.21.5 → 1.21.6: CHANGE_GAME_MODE was inserted at 4.
pub fn play_serverbound(id: i32) -> Option<i32> {
    match id {
        0..=3 => Some(id),
        4..=old::LAST_PLAY_S => Some(id + 1),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn id_maps_are_consistent() {
        assert_eq!(play_clientbound(new::UPDATE_TAGS), Some(127));
        assert_eq!(play_clientbound(130), Some(130));
        assert_eq!(play_clientbound(new::TRACKED_WAYPOINT), None);
        assert_eq!(play_clientbound(new::SHOW_DIALOG), None);
        assert_eq!(config_clientbound(new::CONFIG_UPDATE_TAGS), Some(13));
        assert_eq!(config_clientbound(new::CONFIG_SHOW_DIALOG), None);
        assert_eq!(play_serverbound(old::S_CHANGE_DIFFICULTY), Some(new::S_CHANGE_DIFFICULTY));
        assert_eq!(play_serverbound(old::S_CONTAINER_CLICK), Some(new::S_CONTAINER_CLICK));
        assert_eq!(play_serverbound(old::S_PLAYER_COMMAND), Some(new::S_PLAYER_COMMAND));
        assert_eq!(play_serverbound(old::S_SET_CREATIVE_MODE_SLOT), Some(new::S_SET_CREATIVE_MODE_SLOT));
        // 1.21.5 USE_ITEM (63) → 1.21.6 USE_ITEM (64)
        assert_eq!(play_serverbound(63), Some(64));
        assert_eq!(play_serverbound(64), None);
    }
}
