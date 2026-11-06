//! Packet ids for 26.3 (protocol 777, the backend) and 26.2 (protocol 776, the client).
//! Source: Pumpkin's assets/packets.json (26.3) and ViaVersion's 26.1/26.2 packet enums.

pub mod new {
    // clientbound play (26.3)
    pub const ADD_ENTITY: i32 = 1;
    pub const ANIMATE: i32 = 2;
    pub const BLOCK_ENTITY_DATA: i32 = 6;
    pub const BLOCK_EVENT: i32 = 7;
    pub const BLOCK_UPDATE: i32 = 8;
    pub const COMMANDS: i32 = 16;
    pub const CONTAINER_SET_CONTENT: i32 = 18;
    pub const CONTAINER_SET_DATA: i32 = 19;
    pub const CONTAINER_SET_SLOT: i32 = 20;
    pub const COOLDOWN: i32 = 22;
    pub const ENTITY_POSITION_SYNC: i32 = 35;
    pub const EXPLODE: i32 = 36;
    pub const ADD_TRANSIENT_BLOCK: i32 = 37;
    pub const LEVEL_CHUNK_WITH_LIGHT: i32 = 46;
    pub const LEVEL_EVENT: i32 = 47;
    pub const LEVEL_PARTICLES: i32 = 48;
    pub const LIGHT_UPDATE: i32 = 49;
    pub const LOGIN: i32 = 50;
    pub const MAP_ITEM_DATA: i32 = 52;
    pub const MERCHANT_OFFERS: i32 = 53;
    pub const MOVE_ENTITY_POS: i32 = 54;
    pub const MOVE_ENTITY_POS_ROT: i32 = 55;
    pub const MOVE_ENTITY_ROT: i32 = 57;
    pub const OPEN_SCREEN: i32 = 60;
    pub const OPEN_SIGN_EDITOR: i32 = 61;
    pub const PLACE_GHOST_RECIPE: i32 = 64;
    pub const PLAYER_CHAT: i32 = 66;
    pub const POST_EFFECTS: i32 = 83;
    pub const RESPAWN: i32 = 84;
    pub const SECTION_BLOCKS_UPDATE: i32 = 86;
    pub const RECIPE_BOOK_ADD: i32 = 75;
    pub const REMOVE_ENTITIES: i32 = 78;
    pub const SET_CURSOR_ITEM: i32 = 98;
    pub const SET_ENTITY_DATA: i32 = 101;
    pub const SET_EQUIPMENT: i32 = 104;
    pub const SET_PLAYER_INVENTORY: i32 = 110;
    pub const SOUND_ENTITY: i32 = 118;
    pub const SOUND: i32 = 119;
    pub const STOP_SOUND: i32 = 121;
    pub const SWING_ANIMATION: i32 = 123;
    pub const UPDATE_ADVANCEMENTS: i32 = 133;
    pub const UPDATE_RECIPES: i32 = 136;
    pub const UPDATE_TAGS: i32 = 137;
    pub const PLAY_C_COUNT: i32 = 144;
    // clientbound config (26.3)
    pub const CONFIG_REGISTRY_DATA: i32 = 7;
    pub const CONFIG_POST_EFFECTS: i32 = 10;
    pub const CONFIG_UPDATE_TAGS: i32 = 14;
    pub const CONFIG_C_COUNT: i32 = 21;
    // serverbound play (26.3)
    pub const S_ACCEPT_TELEPORTATION: i32 = 0;
    pub const S_CONTAINER_CLICK: i32 = 18;
    pub const S_MOVE_PLAYER_POS_ROT: i32 = 31;
    pub const S_PLAYER_ACTION: i32 = 41;
    pub const S_PUNCH: i32 = 46;
    pub const S_SET_CREATIVE_MODE_SLOT: i32 = 57;
    pub const S_SIGN_UPDATE: i32 = 62;
    pub const S_SPECTATOR_ACTION: i32 = 63;
}

pub mod old {
    // clientbound play (26.2 == 26.1 enum)
    pub const ANIMATE: i32 = 2;
    pub const LEVEL_PARTICLES: i32 = 0x2F;
    pub const SET_ENTITY_DATA: i32 = 0x63;
    // serverbound play (26.2)
    pub const S_ACCEPT_TELEPORTATION: i32 = 0;
    pub const S_CONTAINER_CLICK: i32 = 0x12;
    pub const S_MOVE_PLAYER_POS_ROT: i32 = 0x1F;
    pub const S_PLAYER_ACTION: i32 = 0x29;
    pub const S_SET_CREATIVE_MODE_SLOT: i32 = 0x38;
    pub const S_SIGN_UPDATE: i32 = 0x3D;
    pub const S_SPECTATE_ENTITY: i32 = 0x3E;
    pub const S_SWING: i32 = 0x3F;
}

/// Clientbound play id 26.3 → 26.2. `None` = the client has no such packet (drop or special-case).
pub fn play_clientbound(id: i32) -> Option<i32> {
    match id {
        0..=36 => Some(id),
        new::ADD_TRANSIENT_BLOCK => None,
        38..=82 => Some(id - 1),
        new::POST_EFFECTS => None,
        84..=122 => Some(id - 2),
        new::SWING_ANIMATION => None,
        124..=143 => Some(id - 3),
        _ => None,
    }
}

/// Clientbound configuration id 26.3 → 26.2.
pub fn config_clientbound(id: i32) -> Option<i32> {
    match id {
        0..=9 => Some(id),
        new::CONFIG_POST_EFFECTS => None,
        11..=20 => Some(id - 1),
        _ => None,
    }
}

/// Serverbound play id 26.2 → 26.3.
pub fn play_serverbound(id: i32) -> Option<i32> {
    match id {
        0..=0x2D => Some(id),
        0x2E..=0x3D => Some(id + 1),
        old::S_SPECTATE_ENTITY => Some(new::S_SPECTATOR_ACTION),
        old::S_SWING => Some(new::S_PUNCH),
        0x40..=0x44 => Some(id),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn id_maps_are_consistent() {
        // 26.1 clientbound: RESPAWN 0x52, SECTION_BLOCKS_UPDATE 0x54, SET_ENTITY_DATA 0x63, SOUND 0x75, UPDATE_TAGS 0x86, SHOW_DIALOG 0x8C
        assert_eq!(play_clientbound(new::RESPAWN), Some(0x52));
        assert_eq!(play_clientbound(new::SECTION_BLOCKS_UPDATE), Some(0x54));
        assert_eq!(play_clientbound(new::SET_ENTITY_DATA), Some(0x63));
        assert_eq!(play_clientbound(new::SOUND), Some(0x75));
        assert_eq!(play_clientbound(new::UPDATE_TAGS), Some(0x86));
        assert_eq!(play_clientbound(143), Some(0x8C));
        assert_eq!(play_clientbound(new::LEVEL_CHUNK_WITH_LIGHT), Some(0x2D));
        assert_eq!(play_clientbound(new::LOGIN), Some(0x31));
        // serverbound: 26.1 PONG 0x2D -> 0x2D, RECIPE_BOOK_CHANGE_SETTINGS 0x2E -> 0x2F, SIGN_UPDATE 0x3D -> 0x3E, TELEPORT_TO_ENTITY 0x40 -> 0x40
        assert_eq!(play_serverbound(0x2D), Some(0x2D));
        assert_eq!(play_serverbound(0x2E), Some(0x2F));
        assert_eq!(play_serverbound(old::S_SIGN_UPDATE), Some(new::S_SIGN_UPDATE));
        assert_eq!(play_serverbound(0x40), Some(0x40));
        assert_eq!(play_serverbound(old::S_PLAYER_ACTION), Some(new::S_PLAYER_ACTION));
        assert_eq!(play_serverbound(old::S_SET_CREATIVE_MODE_SLOT), Some(new::S_SET_CREATIVE_MODE_SLOT));
        assert_eq!(play_serverbound(old::S_CONTAINER_CLICK), Some(new::S_CONTAINER_CLICK));
        // config: UPDATE_TAGS 14 -> 0x0D, CODE_OF_CONDUCT 20 -> 0x13
        assert_eq!(config_clientbound(new::CONFIG_UPDATE_TAGS), Some(0x0D));
        assert_eq!(config_clientbound(20), Some(0x13));
        assert_eq!(config_clientbound(new::CONFIG_REGISTRY_DATA), Some(7));
    }
}
