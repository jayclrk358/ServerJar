//! Packet ids for 1.21.2 (protocol 768, server side of this step) and 1.21 / 1.21.1 (protocol 767).
//! Source: the vanilla packet lists for both versions, in both directions (position = id).

pub mod new {
    // clientbound play (1.21.2)
    pub const ADD_ENTITY: i32 = 0x01;
    pub const BLOCK_ENTITY_DATA: i32 = 0x07;
    pub const BLOCK_EVENT: i32 = 0x08;
    pub const BLOCK_UPDATE: i32 = 0x09;
    pub const CONTAINER_CLOSE: i32 = 0x12;
    pub const CONTAINER_SET_CONTENT: i32 = 0x13;
    pub const CONTAINER_SET_DATA: i32 = 0x14;
    pub const CONTAINER_SET_SLOT: i32 = 0x15;
    pub const COOLDOWN: i32 = 0x17;
    pub const DISGUISED_CHAT: i32 = 0x1E;
    pub const ENTITY_POSITION_SYNC: i32 = 0x20;
    pub const EXPLODE: i32 = 0x21;
    pub const HORSE_SCREEN_OPEN: i32 = 0x24;
    pub const LEVEL_CHUNK_WITH_LIGHT: i32 = 0x28;
    pub const LEVEL_EVENT: i32 = 0x29;
    pub const LEVEL_PARTICLES: i32 = 0x2A;
    pub const LOGIN: i32 = 0x2C;
    pub const MERCHANT_OFFERS: i32 = 0x2E;
    pub const MOVE_MINECART_ALONG_TRACK: i32 = 0x31;
    pub const OPEN_SCREEN: i32 = 0x35;
    pub const PLACE_GHOST_RECIPE: i32 = 0x39;
    pub const PLAYER_INFO_UPDATE: i32 = 0x40;
    pub const PLAYER_POSITION: i32 = 0x42;
    pub const PLAYER_ROTATION: i32 = 0x43;
    pub const RECIPE_BOOK_ADD: i32 = 0x44;
    pub const RECIPE_BOOK_REMOVE: i32 = 0x45;
    pub const RECIPE_BOOK_SETTINGS: i32 = 0x46;
    pub const REMOVE_ENTITIES: i32 = 0x47;
    pub const RESPAWN: i32 = 0x4C;
    pub const SECTION_BLOCKS_UPDATE: i32 = 0x4E;
    pub const SET_ACTION_BAR_TEXT: i32 = 0x51;
    pub const SET_CURSOR_ITEM: i32 = 0x5A;
    pub const SET_ENTITY_DATA: i32 = 0x5D;
    pub const SET_EQUIPMENT: i32 = 0x60;
    pub const SET_HELD_SLOT: i32 = 0x63;
    pub const SET_PASSENGERS: i32 = 0x65;
    pub const SET_PLAYER_INVENTORY: i32 = 0x66;
    pub const SET_SUBTITLE_TEXT: i32 = 0x6A;
    pub const SET_TIME: i32 = 0x6B;
    pub const SET_TITLE_TEXT: i32 = 0x6C;
    pub const SOUND_ENTITY: i32 = 0x6E;
    pub const SOUND: i32 = 0x6F;
    pub const SYSTEM_CHAT: i32 = 0x73;
    pub const TELEPORT_ENTITY: i32 = 0x77;
    pub const UPDATE_ADVANCEMENTS: i32 = 0x7B;
    pub const UPDATE_ATTRIBUTES: i32 = 0x7C;
    pub const UPDATE_RECIPES: i32 = 0x7E;
    pub const UPDATE_TAGS: i32 = 0x7F;
    // clientbound configuration (identical ids in 1.21 and 1.21.2)
    pub const CONFIG_REGISTRY_DATA: i32 = 0x07;
    pub const CONFIG_UPDATE_ENABLED_FEATURES: i32 = 0x0C;
    pub const CONFIG_UPDATE_TAGS: i32 = 0x0D;
    // serverbound play (1.21.2)
    pub const S_CLIENT_TICK_END: i32 = 0x0B;
    pub const S_MOVE_PLAYER_ROT: i32 = 0x1E;
    pub const S_PLAYER_COMMAND: i32 = 0x27;
    pub const S_PLAYER_INPUT: i32 = 0x28;
}

pub mod old {
    // clientbound play (1.21)
    pub const ADD_ENTITY: i32 = 0x01;
    pub const CONTAINER_SET_SLOT: i32 = 0x15;
    pub const EXPLODE: i32 = 0x20;
    pub const PLAYER_LOOK_AT: i32 = 0x3F;
    pub const PLAYER_POSITION: i32 = 0x40;
    pub const RECIPE: i32 = 0x41;
    pub const SET_ENTITY_DATA: i32 = 0x58;
    pub const SET_ENTITY_MOTION: i32 = 0x5A;
    pub const SET_EQUIPMENT: i32 = 0x5B;
    pub const TELEPORT_ENTITY: i32 = 0x70;
    pub const UPDATE_ATTRIBUTES: i32 = 0x75;
    pub const UPDATE_RECIPES: i32 = 0x77;
    // serverbound play (1.21 = 1.20.5 enum)
    pub const S_CLIENT_INFORMATION: i32 = 0x0A;
    pub const S_CONTAINER_CLICK: i32 = 0x0E;
    pub const S_CONTAINER_CLOSE: i32 = 0x0F;
    pub const S_MOVE_PLAYER_POS: i32 = 0x1A;
    pub const S_MOVE_PLAYER_POS_ROT: i32 = 0x1B;
    pub const S_MOVE_PLAYER_ROT: i32 = 0x1C;
    pub const S_MOVE_PLAYER_STATUS_ONLY: i32 = 0x1D;
    pub const S_MOVE_VEHICLE: i32 = 0x1E;
    pub const S_PLACE_RECIPE: i32 = 0x22;
    pub const S_PLAYER_COMMAND: i32 = 0x25;
    pub const S_PLAYER_INPUT: i32 = 0x26;
    pub const S_RECIPE_BOOK_SEEN_RECIPE: i32 = 0x29;
    pub const S_SET_CREATIVE_MODE_SLOT: i32 = 0x32;
    pub const S_USE_ITEM_ON: i32 = 0x38;
    // serverbound configuration
    pub const S_CONFIG_CLIENT_INFORMATION: i32 = 0x00;
}

/// Clientbound play id 1.21.2 → 1.21. `None` = no direct equivalent (special-cased or dropped).
pub fn play_clientbound(id: i32) -> Option<i32> {
    match id {
        0x00..=0x1F => Some(id),
        0x21..=0x30 => Some(id - 1),
        0x32..=0x42 => Some(id - 2),
        0x47..=0x57 => Some(id - 5),
        0x58..=0x59 => Some(id - 4),
        0x5B..=0x62 => Some(id - 5),
        new::SET_HELD_SLOT => Some(0x53),
        0x64..=0x65 => Some(id - 6),
        0x67..=0x82 => Some(id - 7),
        _ => None,
    }
}

/// Serverbound play id 1.21 → 1.21.2 (bundle_item_selected and client_tick_end were inserted).
pub fn play_serverbound(id: i32) -> Option<i32> {
    match id {
        0x00..=0x01 => Some(id),
        0x02..=0x09 => Some(id + 1),
        0x0A..=0x39 => Some(id + 2),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn id_maps_are_consistent() {
        assert_eq!(play_clientbound(new::EXPLODE), Some(old::EXPLODE));
        assert_eq!(play_clientbound(new::LOGIN), Some(0x2B));
        assert_eq!(play_clientbound(0x32), Some(0x30)); // move_entity_rot
        assert_eq!(play_clientbound(new::PLAYER_POSITION), Some(old::PLAYER_POSITION));
        assert_eq!(play_clientbound(new::REMOVE_ENTITIES), Some(0x42));
        assert_eq!(play_clientbound(new::RESPAWN), Some(0x47));
        assert_eq!(play_clientbound(0x57), Some(0x52)); // set_camera
        assert_eq!(play_clientbound(0x58), Some(0x54)); // set_chunk_cache_center
        assert_eq!(play_clientbound(new::SET_ENTITY_DATA), Some(old::SET_ENTITY_DATA));
        assert_eq!(play_clientbound(new::SET_EQUIPMENT), Some(old::SET_EQUIPMENT));
        assert_eq!(play_clientbound(0x62), Some(0x5D)); // set_health
        assert_eq!(play_clientbound(new::SET_HELD_SLOT), Some(0x53));
        assert_eq!(play_clientbound(new::SET_PASSENGERS), Some(0x5F));
        assert_eq!(play_clientbound(0x67), Some(0x60)); // set_player_team
        assert_eq!(play_clientbound(new::SOUND), Some(0x68));
        assert_eq!(play_clientbound(new::TELEPORT_ENTITY), Some(old::TELEPORT_ENTITY));
        assert_eq!(play_clientbound(new::UPDATE_ATTRIBUTES), Some(old::UPDATE_ATTRIBUTES));
        assert_eq!(play_clientbound(new::UPDATE_RECIPES), Some(old::UPDATE_RECIPES));
        assert_eq!(play_clientbound(new::UPDATE_TAGS), Some(0x78));
        assert_eq!(play_clientbound(0x82), Some(0x7B)); // server_links
        for special in [
            new::ENTITY_POSITION_SYNC,
            new::MOVE_MINECART_ALONG_TRACK,
            new::PLAYER_ROTATION,
            new::RECIPE_BOOK_ADD,
            new::RECIPE_BOOK_REMOVE,
            new::RECIPE_BOOK_SETTINGS,
            new::SET_CURSOR_ITEM,
            new::SET_PLAYER_INVENTORY,
        ] {
            assert_eq!(play_clientbound(special), None);
        }
        assert_eq!(play_serverbound(0x01), Some(0x01));
        assert_eq!(play_serverbound(0x02), Some(0x03)); // change_difficulty
        assert_eq!(play_serverbound(0x09), Some(0x0A)); // client_command
        assert_eq!(play_serverbound(old::S_CLIENT_INFORMATION), Some(0x0C));
        assert_eq!(play_serverbound(old::S_PLAYER_COMMAND), Some(new::S_PLAYER_COMMAND));
        assert_eq!(play_serverbound(old::S_PLAYER_INPUT), Some(new::S_PLAYER_INPUT));
        assert_eq!(play_serverbound(old::S_MOVE_PLAYER_ROT), Some(new::S_MOVE_PLAYER_ROT));
        assert_eq!(play_serverbound(0x39), Some(0x3B)); // use_item
    }
}
