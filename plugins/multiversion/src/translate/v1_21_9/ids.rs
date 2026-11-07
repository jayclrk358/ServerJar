//! Packet ids for 1.21.11 (protocol 774, server side) and 1.21.9 (protocol 773, client side).
//! Source: ViaVersion's ClientboundPackets1_21_11 / ClientboundPackets1_21_9 (same order; only
//! HORSE_SCREEN_OPEN was renamed MOUNT_SCREEN_OPEN), the shared 1.21.9 configuration enums and
//! ServerboundPackets1_21_6, which both versions use. Every id maps to itself.

pub mod new {
    // clientbound play
    pub const ADD_ENTITY: i32 = 1;
    pub const AWARD_STATS: i32 = 3;
    pub const COMMANDS: i32 = 16;
    pub const CONTAINER_SET_CONTENT: i32 = 18;
    pub const CONTAINER_SET_SLOT: i32 = 20;
    pub const EXPLODE: i32 = 36;
    pub const INITIALIZE_BORDER: i32 = 42;
    pub const LEVEL_PARTICLES: i32 = 46;
    pub const LOGIN: i32 = 48;
    pub const MERCHANT_OFFERS: i32 = 50;
    pub const PLACE_GHOST_RECIPE: i32 = 61;
    pub const RECIPE_BOOK_ADD: i32 = 72;
    pub const REMOVE_ENTITIES: i32 = 75;
    pub const REMOVE_MOB_EFFECT: i32 = 76;
    pub const RESPAWN: i32 = 80;
    pub const SET_BORDER_LERP_SIZE: i32 = 87;
    pub const SET_CURSOR_ITEM: i32 = 94;
    pub const SET_ENTITY_DATA: i32 = 97;
    pub const SET_EQUIPMENT: i32 = 100;
    pub const SET_PLAYER_INVENTORY: i32 = 106;
    pub const SET_TIME: i32 = 111;
    pub const SOUND_ENTITY: i32 = 114;
    pub const SOUND: i32 = 115;
    pub const UPDATE_ADVANCEMENTS: i32 = 128;
    pub const UPDATE_MOB_EFFECT: i32 = 130;
    pub const UPDATE_RECIPES: i32 = 131;
    pub const UPDATE_TAGS: i32 = 132;
    pub const PLAY_C_COUNT: i32 = 139;
    // clientbound configuration
    pub const CONFIG_REGISTRY_DATA: i32 = 7;
    pub const CONFIG_UPDATE_TAGS: i32 = 13;
    pub const CONFIG_C_COUNT: i32 = 20;
    // serverbound play
    pub const S_CLIENT_TICK_END: i32 = 12;
    pub const S_CONTAINER_CLICK: i32 = 17;
    pub const S_SET_CREATIVE_MODE_SLOT: i32 = 55;
    pub const S_PLAY_COUNT: i32 = 66;
}

/// Clientbound play id 1.21.11 → 1.21.9.
pub fn play_clientbound(id: i32) -> Option<i32> {
    (0..new::PLAY_C_COUNT).contains(&id).then_some(id)
}

/// Clientbound configuration id 1.21.11 → 1.21.9.
pub fn config_clientbound(id: i32) -> Option<i32> {
    (0..new::CONFIG_C_COUNT).contains(&id).then_some(id)
}

/// Serverbound play id 1.21.9 → 1.21.11.
pub fn play_serverbound(id: i32) -> Option<i32> {
    (0..new::S_PLAY_COUNT).contains(&id).then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ids_are_identity() {
        assert_eq!(play_clientbound(new::SET_ENTITY_DATA), Some(97));
        assert_eq!(play_clientbound(138), Some(138)); // SHOW_DIALOG
        assert_eq!(play_clientbound(139), None);
        assert_eq!(config_clientbound(19), Some(19)); // CODE_OF_CONDUCT
        assert_eq!(play_serverbound(new::S_SET_CREATIVE_MODE_SLOT), Some(55));
        assert_eq!(play_serverbound(66), None);
    }
}
