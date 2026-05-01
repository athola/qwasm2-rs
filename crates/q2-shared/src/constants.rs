// Per-level limits
pub const PROTOCOL_VERSION: i32 = 34;
pub const MAX_CLIENTS: usize = 256;
pub const MAX_MAP_AREAS: usize = 256;
pub const MAX_EDICTS: usize = 1024;
pub const MAX_MODELS: usize = 256;
pub const MAX_SOUNDS: usize = 256;
pub const MAX_IMAGES: usize = 256;
pub const MAX_ITEMS: usize = 256;
pub const MAX_LIGHTSTYLES: usize = 256;
pub const MAX_GENERAL: usize = MAX_CLIENTS * 2;
pub const MAX_CONFIGSTRINGS: usize = CS_GENERAL + MAX_GENERAL;

// String/path limits
pub const MAX_QPATH: usize = 64;
pub const MAX_OSPATH: usize = 4096;
pub const MAX_STRING_CHARS: usize = 2048;
pub const MAX_STRING_TOKENS: usize = 80;
pub const MAX_TOKEN_CHARS: usize = 1024;

// Network
pub const MAX_MSGLEN: usize = 1400;
pub const PACKET_HEADER: usize = 10;
pub const PORT_MASTER: u16 = 27900;
pub const PORT_CLIENT: u16 = 27901;
pub const PORT_SERVER: u16 = 27910;
pub const UPDATE_BACKUP: usize = 16;
pub const UPDATE_MASK: usize = UPDATE_BACKUP - 1;

// Print levels
pub const PRINT_LOW: i32 = 0;
pub const PRINT_MEDIUM: i32 = 1;
pub const PRINT_HIGH: i32 = 2;
pub const PRINT_CHAT: i32 = 3;
pub const PRINT_ALL: i32 = 0;
pub const PRINT_DEVELOPER: i32 = 1;

// Error codes (mapped to Q2Error variants, kept for protocol compat)
pub const ERR_FATAL: i32 = 0;
pub const ERR_DROP: i32 = 1;
pub const ERR_QUIT: i32 = 2;

// Entity flags
pub const SVF_NOCLIENT: u32 = 0x00000001;
pub const SVF_DEADMONSTER: u32 = 0x00000002;
pub const SVF_MONSTER: u32 = 0x00000004;

// Configstring offsets
pub const CS_NAME: usize = 0;
pub const CS_CDTRACK: usize = 1;
pub const CS_SKY: usize = 2;
pub const CS_SKYAXIS: usize = 3;
pub const CS_SKYROTATE: usize = 4;
pub const CS_STATUSBAR: usize = 5;
pub const CS_AIRACCEL: usize = 29;
pub const CS_MAXCLIENTS: usize = 30;
pub const CS_MAPCHECKSUM: usize = 31;
pub const CS_MODELS: usize = 32;
pub const CS_SOUNDS: usize = CS_MODELS + MAX_MODELS;
pub const CS_IMAGES: usize = CS_SOUNDS + MAX_SOUNDS;
pub const CS_LIGHTS: usize = CS_IMAGES + MAX_IMAGES;
pub const CS_ITEMS: usize = CS_LIGHTS + MAX_LIGHTSTYLES;
pub const CS_PLAYERSKINS: usize = CS_ITEMS + MAX_ITEMS;
pub const CS_GENERAL: usize = CS_PLAYERSKINS + MAX_CLIENTS;

// MAX_STATS for PlayerState
pub const MAX_STATS: usize = 32;

// Stat indices
pub const STAT_HEALTH_ICON: usize = 0;
pub const STAT_HEALTH: usize = 1;
pub const STAT_AMMO_ICON: usize = 2;
pub const STAT_AMMO: usize = 3;
pub const STAT_ARMOR_ICON: usize = 4;
pub const STAT_ARMOR: usize = 5;
pub const STAT_SELECTED_ICON: usize = 6;
pub const STAT_PICKUP_ICON: usize = 7;
pub const STAT_PICKUP_STRING: usize = 8;
pub const STAT_TIMER_ICON: usize = 9;
pub const STAT_TIMER: usize = 10;
pub const STAT_HELPICON: usize = 11;
pub const STAT_SELECTED_ITEM: usize = 12;
pub const STAT_LAYOUTS: usize = 13;
pub const STAT_FRAGS: usize = 14;
pub const STAT_FLASHES: usize = 15;
pub const STAT_CHASE: usize = 16;
pub const STAT_SPECTATOR: usize = 17;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_edicts_matches_protocol() {
        assert_eq!(MAX_EDICTS, 1024);
        assert_eq!(PROTOCOL_VERSION, 34);
    }

    #[test]
    fn configstring_offsets_are_contiguous() {
        assert_eq!(CS_SOUNDS, CS_MODELS + MAX_MODELS);
        assert_eq!(CS_IMAGES, CS_SOUNDS + MAX_SOUNDS);
        assert_eq!(CS_GENERAL + MAX_GENERAL, MAX_CONFIGSTRINGS);
    }

    #[test]
    fn update_mask_is_power_of_two_minus_one() {
        assert_eq!(UPDATE_MASK, 15);
        assert_eq!(UPDATE_BACKUP & UPDATE_MASK, 0);
    }
}
