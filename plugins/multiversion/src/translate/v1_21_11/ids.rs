//! Packet ids for 26.1 (protocol 775, server side) and 1.21.11 (protocol 774, client side).
//! Source: ViaVersion's ClientboundPackets26_1 / ServerboundPackets26_1 and the 1.21.11
//! clientbound / 1.21.6 serverbound enums (1.21.11 kept the 1.21.6 serverbound list).
//! Configuration ids are identical in both versions (1.21.9 lists).

pub mod new {
    // clientbound play (26.1)
    pub const ADD_ENTITY: i32 = 0x01;
    pub const BLOCK_EVENT: i32 = 0x07;
    pub const BLOCK_UPDATE: i32 = 0x08;
    pub const COMMANDS: i32 = 0x10;
    pub const CONTAINER_SET_CONTENT: i32 = 0x12;
    pub const CONTAINER_SET_SLOT: i32 = 0x14;
    pub const EXPLODE: i32 = 0x24;
    pub const GAME_EVENT: i32 = 0x26;
    pub const GAME_RULE_VALUES: i32 = 0x27;
    pub const LEVEL_CHUNK_WITH_LIGHT: i32 = 0x2D;
    pub const LEVEL_EVENT: i32 = 0x2E;
    pub const LEVEL_PARTICLES: i32 = 0x2F;
    pub const LOGIN: i32 = 0x31;
    pub const LOW_DISK_SPACE_WARNING: i32 = 0x32;
    pub const MERCHANT_OFFERS: i32 = 0x34;
    pub const PLACE_GHOST_RECIPE: i32 = 0x3F;
    pub const RECIPE_BOOK_ADD: i32 = 0x4A;
    pub const REMOVE_ENTITIES: i32 = 0x4D;
    pub const RESPAWN: i32 = 0x52;
    pub const SECTION_BLOCKS_UPDATE: i32 = 0x54;
    pub const SET_CURSOR_ITEM: i32 = 0x60;
    pub const SET_ENTITY_DATA: i32 = 0x63;
    pub const SET_EQUIPMENT: i32 = 0x66;
    pub const SET_PLAYER_INVENTORY: i32 = 0x6C;
    pub const SET_TIME: i32 = 0x71;
    pub const SOUND_ENTITY: i32 = 0x74;
    pub const SOUND: i32 = 0x75;
    pub const UPDATE_ADVANCEMENTS: i32 = 0x82;
    pub const UPDATE_ATTRIBUTES: i32 = 0x83;
    pub const UPDATE_RECIPES: i32 = 0x85;
    pub const UPDATE_TAGS: i32 = 0x86;
    pub const PLAY_C_LAST: i32 = 0x8C;
    // clientbound configuration (both)
    pub const CONFIG_REGISTRY_DATA: i32 = 0x07;
    pub const CONFIG_UPDATE_TAGS: i32 = 0x0D;
    // serverbound play (26.1)
    pub const S_ATTACK: i32 = 0x01;
    pub const S_CONTAINER_CLICK: i32 = 0x12;
    pub const S_INTERACT: i32 = 0x1A;
    pub const S_SET_CREATIVE_MODE_SLOT: i32 = 0x38;
    pub const S_SPECTATE_ENTITY: i32 = 0x3E;
}

pub mod old {
    // serverbound play (1.21.11 == 1.21.6 enum)
    pub const S_CONTAINER_CLICK: i32 = 0x11;
    pub const S_INTERACT: i32 = 0x19;
    pub const S_SET_CREATIVE_MODE_SLOT: i32 = 0x37;
}

/// Clientbound play id 26.1 → 1.21.11. `None` = the client has no such packet (drop).
pub fn play_clientbound(id: i32) -> Option<i32> {
    match id {
        0..=0x26 => Some(id),
        new::GAME_RULE_VALUES => None,
        0x28..=0x31 => Some(id - 1),
        new::LOW_DISK_SPACE_WARNING => None,
        0x33..=new::PLAY_C_LAST => Some(id - 2),
        _ => None,
    }
}

/// Serverbound play id 1.21.11 → 26.1 (26.1 inserted ATTACK, SET_GAME_RULE, SPECTATE_ENTITY).
pub fn play_serverbound(id: i32) -> Option<i32> {
    match id {
        0 => Some(0),
        0x01..=0x37 => Some(id + 1),
        0x38..=0x3B => Some(id + 2),
        0x3C..=0x41 => Some(id + 3),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn id_maps_match_the_enums() {
        // 1.21.11 clientbound: GAME_TEST_HIGHLIGHT_POS 0x27, LOGIN 0x30, MAP_ITEM_DATA 0x31, SHOW_DIALOG 0x8A
        assert_eq!(play_clientbound(0x28), Some(0x27));
        assert_eq!(play_clientbound(new::LOGIN), Some(0x30));
        assert_eq!(play_clientbound(0x33), Some(0x31));
        assert_eq!(play_clientbound(new::SET_ENTITY_DATA), Some(0x61));
        assert_eq!(play_clientbound(new::UPDATE_TAGS), Some(0x84));
        assert_eq!(play_clientbound(new::PLAY_C_LAST), Some(0x8A));
        assert_eq!(play_clientbound(new::GAME_RULE_VALUES), None);
        assert_eq!(play_clientbound(new::LOW_DISK_SPACE_WARNING), None);
        // serverbound: SET_JIGSAW_BLOCK 0x38 -> 0x3A, SIGN_UPDATE 0x3B -> 0x3D, SWING 0x3C -> 0x3F, CUSTOM_CLICK_ACTION 0x41 -> 0x44
        assert_eq!(play_serverbound(old::S_CONTAINER_CLICK), Some(new::S_CONTAINER_CLICK));
        assert_eq!(play_serverbound(old::S_INTERACT), Some(new::S_INTERACT));
        assert_eq!(play_serverbound(old::S_SET_CREATIVE_MODE_SLOT), Some(new::S_SET_CREATIVE_MODE_SLOT));
        assert_eq!(play_serverbound(0x38), Some(0x3A));
        assert_eq!(play_serverbound(0x3B), Some(0x3D));
        assert_eq!(play_serverbound(0x3C), Some(0x3F));
        assert_eq!(play_serverbound(0x41), Some(0x44));
    }
}
