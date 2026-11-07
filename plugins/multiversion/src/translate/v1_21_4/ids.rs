//! Packet ids for 1.21.5 (protocol 770, server side of this step) and 1.21.4 (protocol 769).
//! Source: ViaVersion's ClientboundPackets1_21_5 / ClientboundPackets1_21_2 and the serverbound
//! enums (enum ordinal = packet id). Configuration ids are identical in both versions.

pub mod new {
    // clientbound play (1.21.5)
    pub const ADD_ENTITY: i32 = 1;
    pub const BLOCK_ENTITY_DATA: i32 = 6;
    pub const BLOCK_EVENT: i32 = 7;
    pub const BLOCK_UPDATE: i32 = 8;
    pub const BOSS_EVENT: i32 = 9;
    pub const CHUNKS_BIOMES: i32 = 13;
    pub const COMMANDS: i32 = 16;
    pub const CONTAINER_SET_CONTENT: i32 = 18;
    pub const CONTAINER_SET_SLOT: i32 = 20;
    pub const DISCONNECT: i32 = 28;
    pub const DISGUISED_CHAT: i32 = 29;
    pub const EXPLODE: i32 = 32;
    pub const LEVEL_CHUNK_WITH_LIGHT: i32 = 39;
    pub const LEVEL_EVENT: i32 = 40;
    pub const LEVEL_PARTICLES: i32 = 41;
    pub const LOGIN: i32 = 43;
    pub const MERCHANT_OFFERS: i32 = 45;
    pub const OPEN_SCREEN: i32 = 52;
    pub const PLACE_GHOST_RECIPE: i32 = 56;
    pub const PLAYER_CHAT: i32 = 58;
    pub const PLAYER_COMBAT_KILL: i32 = 61;
    pub const RECIPE_BOOK_ADD: i32 = 67;
    pub const REMOVE_ENTITIES: i32 = 70;
    pub const RESPAWN: i32 = 75;
    pub const SECTION_BLOCKS_UPDATE: i32 = 77;
    pub const SET_ACTION_BAR_TEXT: i32 = 80;
    pub const SET_CURSOR_ITEM: i32 = 89;
    pub const SET_ENTITY_DATA: i32 = 92;
    pub const SET_EQUIPMENT: i32 = 95;
    pub const SET_OBJECTIVE: i32 = 99;
    pub const SET_PLAYER_INVENTORY: i32 = 101;
    pub const SET_PLAYER_TEAM: i32 = 102;
    pub const SET_SUBTITLE_TEXT: i32 = 105;
    pub const SET_TITLE_TEXT: i32 = 107;
    pub const SOUND_ENTITY: i32 = 109;
    pub const SOUND: i32 = 110;
    pub const SYSTEM_CHAT: i32 = 114;
    pub const TAB_LIST: i32 = 115;
    pub const TEST_INSTANCE_BLOCK_STATUS: i32 = 119;
    pub const UPDATE_ADVANCEMENTS: i32 = 123;
    pub const UPDATE_RECIPES: i32 = 126;
    pub const UPDATE_TAGS: i32 = 127;
    pub const PLAY_C_COUNT: i32 = 131;
    // clientbound configuration (same ids in 1.21.4)
    pub const CONFIG_REGISTRY_DATA: i32 = 7;
    pub const CONFIG_UPDATE_TAGS: i32 = 13;
    // serverbound play (1.21.5)
    pub const S_CHAT_COMMAND_SIGNED: i32 = 6;
    pub const S_CHAT: i32 = 7;
    pub const S_CONTAINER_CLICK: i32 = 16;
    pub const S_SET_CREATIVE_MODE_SLOT: i32 = 54;
}

pub mod old {
    // clientbound play (1.21.4)
    pub const ADD_EXPERIENCE_ORB: i32 = 2;
    pub const SET_ENTITY_DATA: i32 = 93;
    pub const SET_ENTITY_MOTION: i32 = 95;
    // serverbound play (1.21.4)
    pub const S_CHAT_COMMAND_SIGNED: i32 = 6;
    pub const S_CHAT: i32 = 7;
    pub const S_CONTAINER_CLICK: i32 = 16;
    pub const S_SET_CREATIVE_MODE_SLOT: i32 = 54;
}

/// Clientbound play id 1.21.5 → 1.21.4. 1.21.5 dropped ADD_EXPERIENCE_ORB (old id 2) and added
/// TEST_INSTANCE_BLOCK_STATUS (new id 119, no old equivalent).
pub fn play_clientbound(id: i32) -> Option<i32> {
    match id {
        0..=1 => Some(id),
        2..=118 => Some(id + 1),
        new::TEST_INSTANCE_BLOCK_STATUS => None,
        120..=130 => Some(id),
        _ => None,
    }
}

/// Serverbound play id 1.21.4 → 1.21.5. 1.21.5 inserted SET_TEST_BLOCK (57) and
/// TEST_INSTANCE_BLOCK_ACTION (61).
pub fn play_serverbound(id: i32) -> Option<i32> {
    match id {
        0..=56 => Some(id),
        57..=59 => Some(id + 1),
        60..=61 => Some(id + 2),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn id_maps_match_viaversion_enums() {
        // 1.21.5 → 1.21.4 clientbound
        assert_eq!(play_clientbound(new::ADD_ENTITY), Some(1));
        assert_eq!(play_clientbound(2), Some(3)); // ANIMATE
        assert_eq!(play_clientbound(new::LEVEL_CHUNK_WITH_LIGHT), Some(40));
        assert_eq!(play_clientbound(new::LOGIN), Some(44));
        assert_eq!(play_clientbound(new::SET_ENTITY_DATA), Some(old::SET_ENTITY_DATA));
        assert_eq!(play_clientbound(94), Some(old::SET_ENTITY_MOTION));
        assert_eq!(play_clientbound(118), Some(119)); // TELEPORT_ENTITY
        assert_eq!(play_clientbound(new::TEST_INSTANCE_BLOCK_STATUS), None);
        assert_eq!(play_clientbound(120), Some(120)); // TICKING_STATE
        assert_eq!(play_clientbound(new::UPDATE_TAGS), Some(127));
        assert_eq!(play_clientbound(130), Some(130)); // SERVER_LINKS
        // 1.21.4 → 1.21.5 serverbound
        assert_eq!(play_serverbound(old::S_CONTAINER_CLICK), Some(new::S_CONTAINER_CLICK));
        assert_eq!(play_serverbound(old::S_SET_CREATIVE_MODE_SLOT), Some(new::S_SET_CREATIVE_MODE_SLOT));
        assert_eq!(play_serverbound(56), Some(56)); // SET_STRUCTURE_BLOCK
        assert_eq!(play_serverbound(57), Some(58)); // SIGN_UPDATE
        assert_eq!(play_serverbound(59), Some(60)); // TELEPORT_TO_ENTITY
        assert_eq!(play_serverbound(60), Some(62)); // USE_ITEM_ON
        assert_eq!(play_serverbound(61), Some(63)); // USE_ITEM
    }
}
