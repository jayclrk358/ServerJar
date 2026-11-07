//! Packet ids for 1.21.7 (protocol 772) and 1.21.6 (protocol 771).
//!
//! Both versions use the same packet lists, so every id passes through unchanged in both states and
//! both directions. Only the packets that carry registry ids that differ between the two versions
//! are listed here.

// clientbound play
pub const ADD_ENTITY: i32 = 1;
pub const AWARD_STATS: i32 = 3;
pub const CONTAINER_SET_CONTENT: i32 = 18;
pub const CONTAINER_SET_SLOT: i32 = 20;
pub const COOLDOWN: i32 = 22;
pub const EXPLODE: i32 = 32;
pub const LEVEL_PARTICLES: i32 = 41;
pub const MERCHANT_OFFERS: i32 = 45;
pub const PLACE_GHOST_RECIPE: i32 = 56;
pub const RECIPE_BOOK_ADD: i32 = 67;
pub const SET_CURSOR_ITEM: i32 = 89;
pub const SET_ENTITY_DATA: i32 = 92;
pub const SET_EQUIPMENT: i32 = 95;
pub const SET_PLAYER_INVENTORY: i32 = 101;
pub const SOUND_ENTITY: i32 = 109;
pub const SOUND: i32 = 110;
pub const STOP_SOUND: i32 = 112;
pub const UPDATE_ADVANCEMENTS: i32 = 123;
pub const UPDATE_RECIPES: i32 = 126;
pub const UPDATE_TAGS: i32 = 127;

// clientbound configuration
pub const CONFIG_REGISTRY_DATA: i32 = 7;
pub const CONFIG_UPDATE_TAGS: i32 = 13;

// serverbound play
pub const S_CONTAINER_CLICK: i32 = 17;
pub const S_SET_CREATIVE_MODE_SLOT: i32 = 55;
