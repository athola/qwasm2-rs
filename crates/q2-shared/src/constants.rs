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

// Brush content flags (shared.h)
pub const CONTENTS_SOLID: i32 = 1;
pub const CONTENTS_WINDOW: i32 = 2;
pub const CONTENTS_AUX: i32 = 4;
pub const CONTENTS_LAVA: i32 = 8;
pub const CONTENTS_SLIME: i32 = 16;
pub const CONTENTS_WATER: i32 = 32;
pub const CONTENTS_MIST: i32 = 64;
pub const CONTENTS_AREAPORTAL: i32 = 0x8000;
pub const CONTENTS_PLAYERCLIP: i32 = 0x10000;
pub const CONTENTS_MONSTERCLIP: i32 = 0x20000;
pub const CONTENTS_CURRENT_0: i32 = 0x40000;
pub const CONTENTS_CURRENT_90: i32 = 0x80000;
pub const CONTENTS_CURRENT_180: i32 = 0x10_0000;
pub const CONTENTS_CURRENT_270: i32 = 0x20_0000;
pub const CONTENTS_CURRENT_UP: i32 = 0x40_0000;
pub const CONTENTS_CURRENT_DOWN: i32 = 0x80_0000;
pub const CONTENTS_ORIGIN: i32 = 0x0100_0000;
pub const CONTENTS_MONSTER: i32 = 0x0200_0000;
pub const CONTENTS_DEADMONSTER: i32 = 0x0400_0000;
pub const CONTENTS_DETAIL: i32 = 0x0800_0000;
pub const CONTENTS_TRANSLUCENT: i32 = 0x1000_0000;
pub const CONTENTS_LADDER: i32 = 0x2000_0000;

// Content masks (shared.h)
pub const MASK_ALL: i32 = -1;
pub const MASK_SOLID: i32 = CONTENTS_SOLID | CONTENTS_WINDOW;
pub const MASK_PLAYERSOLID: i32 =
    CONTENTS_SOLID | CONTENTS_PLAYERCLIP | CONTENTS_WINDOW | CONTENTS_MONSTER;
pub const MASK_DEADSOLID: i32 = CONTENTS_SOLID | CONTENTS_PLAYERCLIP | CONTENTS_WINDOW;
pub const MASK_MONSTERSOLID: i32 =
    CONTENTS_SOLID | CONTENTS_MONSTERCLIP | CONTENTS_WINDOW | CONTENTS_MONSTER;
pub const MASK_WATER: i32 = CONTENTS_WATER | CONTENTS_LAVA | CONTENTS_SLIME;
pub const MASK_OPAQUE: i32 = CONTENTS_SOLID | CONTENTS_SLIME | CONTENTS_LAVA;
pub const MASK_SHOT: i32 =
    CONTENTS_SOLID | CONTENTS_MONSTER | CONTENTS_WINDOW | CONTENTS_DEADMONSTER;
pub const MASK_CURRENT: i32 = CONTENTS_CURRENT_0
    | CONTENTS_CURRENT_90
    | CONTENTS_CURRENT_180
    | CONTENTS_CURRENT_270
    | CONTENTS_CURRENT_UP
    | CONTENTS_CURRENT_DOWN;

// gi.BoxEdicts area types (shared.h), shared by both sides of GameImport.
pub const AREA_SOLID: i32 = 1;
pub const AREA_TRIGGERS: i32 = 2;

// entity_state_t effects and refresh flags (shared.h)
/// Bonus items spin.
pub const EF_ROTATE: u32 = 0x0000_0001;
/// Drawn only through a mirror or third-person camera: the viewer's own body.
pub const RF_VIEWERMODEL: i32 = 2;
pub const RF_WEAPONMODEL: i32 = 4;
pub const RF_FULLBRIGHT: i32 = 8;
pub const RF_TRANSLUCENT: i32 = 32;
/// Pulse lighting for bonus items.
pub const RF_GLOW: i32 = 512;
pub const RF_FRAMELERP: i32 = 64;
pub const RF_BEAM: i32 = 128;
/// Don't draw a shadow.
pub const RF_NOSHADOW: i32 = 8192;
/// Leave a blood trail.
pub const EF_GIB: u32 = 0x0000_0002;
/// Projectile effects: dynamic light and trail per weapon.
pub const EF_BLASTER: u32 = 0x0000_0008;
pub const EF_ROCKET: u32 = 0x0000_0010;
pub const EF_GRENADE: u32 = 0x0000_0020;
pub const EF_HYPERBLASTER: u32 = 0x0000_0040;
pub const EF_BFG: u32 = 0x0000_0080;
/// Client-side frame cycling: 0/1 and 2/3 at 2 Hz, all frames at 2 or 10 Hz.
pub const EF_ANIM01: u32 = 0x0000_0400;
pub const EF_ANIM23: u32 = 0x0000_0800;
pub const EF_ANIM_ALL: u32 = 0x0000_1000;
pub const EF_ANIM_ALLFAST: u32 = 0x0000_2000;
/// Particle fountain.
pub const EF_TELEPORTER: u32 = 0x0002_0000;

// entity_state_t events (shared.h entity_event_t)
pub const EV_PLAYER_TELEPORT: i32 = 6;
pub const EV_OTHER_TELEPORT: i32 = 7;

// Monster muzzle flashes (shared.h MZ2_*): the flash number in svc_muzzleflash2,
// which also indexes monster_flash_offset.
pub const MZ2_TANK_BLASTER_1: i32 = 1;
pub const MZ2_TANK_BLASTER_2: i32 = 2;
pub const MZ2_TANK_BLASTER_3: i32 = 3;
pub const MZ2_TANK_MACHINEGUN_1: i32 = 4;
pub const MZ2_TANK_MACHINEGUN_2: i32 = 5;
pub const MZ2_TANK_MACHINEGUN_3: i32 = 6;
pub const MZ2_TANK_MACHINEGUN_4: i32 = 7;
pub const MZ2_TANK_MACHINEGUN_5: i32 = 8;
pub const MZ2_TANK_MACHINEGUN_6: i32 = 9;
pub const MZ2_TANK_MACHINEGUN_7: i32 = 10;
pub const MZ2_TANK_MACHINEGUN_8: i32 = 11;
pub const MZ2_TANK_MACHINEGUN_9: i32 = 12;
pub const MZ2_TANK_MACHINEGUN_10: i32 = 13;
pub const MZ2_TANK_MACHINEGUN_11: i32 = 14;
pub const MZ2_TANK_MACHINEGUN_12: i32 = 15;
pub const MZ2_TANK_MACHINEGUN_13: i32 = 16;
pub const MZ2_TANK_MACHINEGUN_14: i32 = 17;
pub const MZ2_TANK_MACHINEGUN_15: i32 = 18;
pub const MZ2_TANK_MACHINEGUN_16: i32 = 19;
pub const MZ2_TANK_MACHINEGUN_17: i32 = 20;
pub const MZ2_TANK_MACHINEGUN_18: i32 = 21;
pub const MZ2_TANK_MACHINEGUN_19: i32 = 22;
pub const MZ2_TANK_ROCKET_1: i32 = 23;
pub const MZ2_TANK_ROCKET_2: i32 = 24;
pub const MZ2_TANK_ROCKET_3: i32 = 25;
pub const MZ2_INFANTRY_MACHINEGUN_1: i32 = 26;
pub const MZ2_INFANTRY_MACHINEGUN_2: i32 = 27;
pub const MZ2_INFANTRY_MACHINEGUN_3: i32 = 28;
pub const MZ2_INFANTRY_MACHINEGUN_4: i32 = 29;
pub const MZ2_INFANTRY_MACHINEGUN_5: i32 = 30;
pub const MZ2_INFANTRY_MACHINEGUN_6: i32 = 31;
pub const MZ2_INFANTRY_MACHINEGUN_7: i32 = 32;
pub const MZ2_INFANTRY_MACHINEGUN_8: i32 = 33;
pub const MZ2_INFANTRY_MACHINEGUN_9: i32 = 34;
pub const MZ2_INFANTRY_MACHINEGUN_10: i32 = 35;
pub const MZ2_INFANTRY_MACHINEGUN_11: i32 = 36;
pub const MZ2_INFANTRY_MACHINEGUN_12: i32 = 37;
pub const MZ2_INFANTRY_MACHINEGUN_13: i32 = 38;
pub const MZ2_SOLDIER_BLASTER_1: i32 = 39;
pub const MZ2_SOLDIER_BLASTER_2: i32 = 40;
pub const MZ2_SOLDIER_SHOTGUN_1: i32 = 41;
pub const MZ2_SOLDIER_SHOTGUN_2: i32 = 42;
pub const MZ2_SOLDIER_MACHINEGUN_1: i32 = 43;
pub const MZ2_SOLDIER_MACHINEGUN_2: i32 = 44;
pub const MZ2_GUNNER_MACHINEGUN_1: i32 = 45;
pub const MZ2_GUNNER_MACHINEGUN_2: i32 = 46;
pub const MZ2_GUNNER_MACHINEGUN_3: i32 = 47;
pub const MZ2_GUNNER_MACHINEGUN_4: i32 = 48;
pub const MZ2_GUNNER_MACHINEGUN_5: i32 = 49;
pub const MZ2_GUNNER_MACHINEGUN_6: i32 = 50;
pub const MZ2_GUNNER_MACHINEGUN_7: i32 = 51;
pub const MZ2_GUNNER_MACHINEGUN_8: i32 = 52;
pub const MZ2_GUNNER_GRENADE_1: i32 = 53;
pub const MZ2_GUNNER_GRENADE_2: i32 = 54;
pub const MZ2_GUNNER_GRENADE_3: i32 = 55;
pub const MZ2_GUNNER_GRENADE_4: i32 = 56;
pub const MZ2_CHICK_ROCKET_1: i32 = 57;
pub const MZ2_FLYER_BLASTER_1: i32 = 58;
pub const MZ2_FLYER_BLASTER_2: i32 = 59;
pub const MZ2_MEDIC_BLASTER_1: i32 = 60;
pub const MZ2_GLADIATOR_RAILGUN_1: i32 = 61;
pub const MZ2_HOVER_BLASTER_1: i32 = 62;
pub const MZ2_ACTOR_MACHINEGUN_1: i32 = 63;
pub const MZ2_SUPERTANK_MACHINEGUN_1: i32 = 64;
pub const MZ2_SUPERTANK_MACHINEGUN_2: i32 = 65;
pub const MZ2_SUPERTANK_MACHINEGUN_3: i32 = 66;
pub const MZ2_SUPERTANK_MACHINEGUN_4: i32 = 67;
pub const MZ2_SUPERTANK_MACHINEGUN_5: i32 = 68;
pub const MZ2_SUPERTANK_MACHINEGUN_6: i32 = 69;
pub const MZ2_SUPERTANK_ROCKET_1: i32 = 70;
pub const MZ2_SUPERTANK_ROCKET_2: i32 = 71;
pub const MZ2_SUPERTANK_ROCKET_3: i32 = 72;
pub const MZ2_BOSS2_MACHINEGUN_L1: i32 = 73;
pub const MZ2_BOSS2_MACHINEGUN_L2: i32 = 74;
pub const MZ2_BOSS2_MACHINEGUN_L3: i32 = 75;
pub const MZ2_BOSS2_MACHINEGUN_L4: i32 = 76;
pub const MZ2_BOSS2_MACHINEGUN_L5: i32 = 77;
pub const MZ2_BOSS2_ROCKET_1: i32 = 78;
pub const MZ2_BOSS2_ROCKET_2: i32 = 79;
pub const MZ2_BOSS2_ROCKET_3: i32 = 80;
pub const MZ2_BOSS2_ROCKET_4: i32 = 81;
pub const MZ2_FLOAT_BLASTER_1: i32 = 82;
pub const MZ2_SOLDIER_BLASTER_3: i32 = 83;
pub const MZ2_SOLDIER_SHOTGUN_3: i32 = 84;
pub const MZ2_SOLDIER_MACHINEGUN_3: i32 = 85;
pub const MZ2_SOLDIER_BLASTER_4: i32 = 86;
pub const MZ2_SOLDIER_SHOTGUN_4: i32 = 87;
pub const MZ2_SOLDIER_MACHINEGUN_4: i32 = 88;
pub const MZ2_SOLDIER_BLASTER_5: i32 = 89;
pub const MZ2_SOLDIER_SHOTGUN_5: i32 = 90;
pub const MZ2_SOLDIER_MACHINEGUN_5: i32 = 91;
pub const MZ2_SOLDIER_BLASTER_6: i32 = 92;
pub const MZ2_SOLDIER_SHOTGUN_6: i32 = 93;
pub const MZ2_SOLDIER_MACHINEGUN_6: i32 = 94;
pub const MZ2_SOLDIER_BLASTER_7: i32 = 95;
pub const MZ2_SOLDIER_SHOTGUN_7: i32 = 96;
pub const MZ2_SOLDIER_MACHINEGUN_7: i32 = 97;
pub const MZ2_SOLDIER_BLASTER_8: i32 = 98;
pub const MZ2_SOLDIER_SHOTGUN_8: i32 = 99;
pub const MZ2_SOLDIER_MACHINEGUN_8: i32 = 100;
pub const MZ2_MAKRON_BFG: i32 = 101;
pub const MZ2_MAKRON_BLASTER_1: i32 = 102;
pub const MZ2_MAKRON_BLASTER_2: i32 = 103;
pub const MZ2_MAKRON_BLASTER_3: i32 = 104;
pub const MZ2_MAKRON_BLASTER_4: i32 = 105;
pub const MZ2_MAKRON_BLASTER_5: i32 = 106;
pub const MZ2_MAKRON_BLASTER_6: i32 = 107;
pub const MZ2_MAKRON_BLASTER_7: i32 = 108;
pub const MZ2_MAKRON_BLASTER_8: i32 = 109;
pub const MZ2_MAKRON_BLASTER_9: i32 = 110;
pub const MZ2_MAKRON_BLASTER_10: i32 = 111;
pub const MZ2_MAKRON_BLASTER_11: i32 = 112;
pub const MZ2_MAKRON_BLASTER_12: i32 = 113;
pub const MZ2_MAKRON_BLASTER_13: i32 = 114;
pub const MZ2_MAKRON_BLASTER_14: i32 = 115;
pub const MZ2_MAKRON_BLASTER_15: i32 = 116;
pub const MZ2_MAKRON_BLASTER_16: i32 = 117;
pub const MZ2_MAKRON_BLASTER_17: i32 = 118;
pub const MZ2_MAKRON_RAILGUN_1: i32 = 119;
pub const MZ2_JORG_MACHINEGUN_L1: i32 = 120;
pub const MZ2_JORG_MACHINEGUN_L2: i32 = 121;
pub const MZ2_JORG_MACHINEGUN_L3: i32 = 122;
pub const MZ2_JORG_MACHINEGUN_L4: i32 = 123;
pub const MZ2_JORG_MACHINEGUN_L5: i32 = 124;
pub const MZ2_JORG_MACHINEGUN_L6: i32 = 125;
pub const MZ2_JORG_MACHINEGUN_R1: i32 = 126;
pub const MZ2_JORG_MACHINEGUN_R2: i32 = 127;
pub const MZ2_JORG_MACHINEGUN_R3: i32 = 128;
pub const MZ2_JORG_MACHINEGUN_R4: i32 = 129;
pub const MZ2_JORG_MACHINEGUN_R5: i32 = 130;
pub const MZ2_JORG_MACHINEGUN_R6: i32 = 131;
pub const MZ2_JORG_BFG_1: i32 = 132;
pub const MZ2_BOSS2_MACHINEGUN_R1: i32 = 133;
pub const MZ2_BOSS2_MACHINEGUN_R2: i32 = 134;
pub const MZ2_BOSS2_MACHINEGUN_R3: i32 = 135;
pub const MZ2_BOSS2_MACHINEGUN_R4: i32 = 136;
pub const MZ2_BOSS2_MACHINEGUN_R5: i32 = 137;
pub const MZ2_CARRIER_MACHINEGUN_L1: i32 = 138;
pub const MZ2_CARRIER_MACHINEGUN_R1: i32 = 139;
pub const MZ2_CARRIER_GRENADE: i32 = 140;
pub const MZ2_TURRET_MACHINEGUN: i32 = 141;
pub const MZ2_TURRET_ROCKET: i32 = 142;
pub const MZ2_TURRET_BLASTER: i32 = 143;
pub const MZ2_STALKER_BLASTER: i32 = 144;
pub const MZ2_DAEDALUS_BLASTER: i32 = 145;
pub const MZ2_MEDIC_BLASTER_2: i32 = 146;
pub const MZ2_CARRIER_RAILGUN: i32 = 147;
pub const MZ2_WIDOW_DISRUPTOR: i32 = 148;
pub const MZ2_WIDOW_BLASTER: i32 = 149;
pub const MZ2_WIDOW_RAIL: i32 = 150;
pub const MZ2_WIDOW_PLASMABEAM: i32 = 151;
pub const MZ2_CARRIER_MACHINEGUN_L2: i32 = 152;
pub const MZ2_CARRIER_MACHINEGUN_R2: i32 = 153;
pub const MZ2_WIDOW_RAIL_LEFT: i32 = 154;
pub const MZ2_WIDOW_RAIL_RIGHT: i32 = 155;
pub const MZ2_WIDOW_BLASTER_SWEEP1: i32 = 156;
pub const MZ2_WIDOW_BLASTER_SWEEP2: i32 = 157;
pub const MZ2_WIDOW_BLASTER_SWEEP3: i32 = 158;
pub const MZ2_WIDOW_BLASTER_SWEEP4: i32 = 159;
pub const MZ2_WIDOW_BLASTER_SWEEP5: i32 = 160;
pub const MZ2_WIDOW_BLASTER_SWEEP6: i32 = 161;
pub const MZ2_WIDOW_BLASTER_SWEEP7: i32 = 162;
pub const MZ2_WIDOW_BLASTER_SWEEP8: i32 = 163;
pub const MZ2_WIDOW_BLASTER_SWEEP9: i32 = 164;
pub const MZ2_WIDOW_BLASTER_100: i32 = 165;
pub const MZ2_WIDOW_BLASTER_90: i32 = 166;
pub const MZ2_WIDOW_BLASTER_80: i32 = 167;
pub const MZ2_WIDOW_BLASTER_70: i32 = 168;
pub const MZ2_WIDOW_BLASTER_60: i32 = 169;
pub const MZ2_WIDOW_BLASTER_50: i32 = 170;
pub const MZ2_WIDOW_BLASTER_40: i32 = 171;
pub const MZ2_WIDOW_BLASTER_30: i32 = 172;
pub const MZ2_WIDOW_BLASTER_20: i32 = 173;
pub const MZ2_WIDOW_BLASTER_10: i32 = 174;
pub const MZ2_WIDOW_BLASTER_0: i32 = 175;
pub const MZ2_WIDOW_BLASTER_10L: i32 = 176;
pub const MZ2_WIDOW_BLASTER_20L: i32 = 177;
pub const MZ2_WIDOW_BLASTER_30L: i32 = 178;
pub const MZ2_WIDOW_BLASTER_40L: i32 = 179;
pub const MZ2_WIDOW_BLASTER_50L: i32 = 180;
pub const MZ2_WIDOW_BLASTER_60L: i32 = 181;
pub const MZ2_WIDOW_BLASTER_70L: i32 = 182;
pub const MZ2_WIDOW_RUN_1: i32 = 183;
pub const MZ2_WIDOW_RUN_2: i32 = 184;
pub const MZ2_WIDOW_RUN_3: i32 = 185;
pub const MZ2_WIDOW_RUN_4: i32 = 186;
pub const MZ2_WIDOW_RUN_5: i32 = 187;
pub const MZ2_WIDOW_RUN_6: i32 = 188;
pub const MZ2_WIDOW_RUN_7: i32 = 189;
pub const MZ2_WIDOW_RUN_8: i32 = 190;
pub const MZ2_CARRIER_ROCKET_1: i32 = 191;
pub const MZ2_CARRIER_ROCKET_2: i32 = 192;
pub const MZ2_CARRIER_ROCKET_3: i32 = 193;
pub const MZ2_CARRIER_ROCKET_4: i32 = 194;
pub const MZ2_WIDOW2_BEAMER_1: i32 = 195;
pub const MZ2_WIDOW2_BEAMER_2: i32 = 196;
pub const MZ2_WIDOW2_BEAMER_3: i32 = 197;
pub const MZ2_WIDOW2_BEAMER_4: i32 = 198;
pub const MZ2_WIDOW2_BEAMER_5: i32 = 199;
pub const MZ2_WIDOW2_BEAM_SWEEP_1: i32 = 200;
pub const MZ2_WIDOW2_BEAM_SWEEP_2: i32 = 201;
pub const MZ2_WIDOW2_BEAM_SWEEP_3: i32 = 202;
pub const MZ2_WIDOW2_BEAM_SWEEP_4: i32 = 203;
pub const MZ2_WIDOW2_BEAM_SWEEP_5: i32 = 204;
pub const MZ2_WIDOW2_BEAM_SWEEP_6: i32 = 205;
pub const MZ2_WIDOW2_BEAM_SWEEP_7: i32 = 206;
pub const MZ2_WIDOW2_BEAM_SWEEP_8: i32 = 207;
pub const MZ2_WIDOW2_BEAM_SWEEP_9: i32 = 208;
pub const MZ2_WIDOW2_BEAM_SWEEP_10: i32 = 209;
pub const MZ2_WIDOW2_BEAM_SWEEP_11: i32 = 210;

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
    fn content_flags_match_c_shared_h() {
        assert_eq!(CONTENTS_SOLID, 1);
        assert_eq!(CONTENTS_WINDOW, 2);
        assert_eq!(CONTENTS_PLAYERCLIP, 0x10000);
        assert_eq!(CONTENTS_MONSTER, 0x0200_0000);
        assert_eq!(CONTENTS_DEADMONSTER, 0x0400_0000);
        assert_eq!(MASK_PLAYERSOLID, 0x0201_0003);
        assert_eq!(MASK_DEADSOLID, 0x0001_0003);
        assert_eq!(MASK_SOLID, 3);
    }

    #[test]
    fn content_flags_and_masks_match_c_shared_h_literals() {
        let flags = [
            (CONTENTS_AUX, 4),
            (CONTENTS_LAVA, 8),
            (CONTENTS_SLIME, 16),
            (CONTENTS_WATER, 32),
            (CONTENTS_MIST, 64),
            (CONTENTS_AREAPORTAL, 0x8000),
            (CONTENTS_MONSTERCLIP, 0x20000),
            (CONTENTS_CURRENT_0, 0x40000),
            (CONTENTS_CURRENT_90, 0x80000),
            (CONTENTS_CURRENT_180, 0x100000),
            (CONTENTS_CURRENT_270, 0x200000),
            (CONTENTS_CURRENT_UP, 0x400000),
            (CONTENTS_CURRENT_DOWN, 0x800000),
            (CONTENTS_ORIGIN, 0x1000000),
            (CONTENTS_DETAIL, 0x8000000),
            (CONTENTS_TRANSLUCENT, 0x10000000),
            (CONTENTS_LADDER, 0x20000000),
            (MASK_ALL, -1),
            (MASK_MONSTERSOLID, 0x0202_0003),
            (MASK_WATER, 0x38),
            (MASK_OPAQUE, 0x19),
            (MASK_SHOT, 0x0600_0003),
            (MASK_CURRENT, 0x00fc_0000),
            (AREA_SOLID, 1),
            (AREA_TRIGGERS, 2),
        ];
        for (i, (rust, c)) in flags.into_iter().enumerate() {
            assert_eq!(rust, c, "entry {i}");
        }
    }

    #[test]
    fn render_flags_match_c_shared_h() {
        assert_eq!(EF_ROTATE, 1);
        assert_eq!(RF_VIEWERMODEL, 2);
        assert_eq!(RF_WEAPONMODEL, 4);
        assert_eq!(RF_FULLBRIGHT, 8);
        assert_eq!(RF_TRANSLUCENT, 32);
        assert_eq!(RF_GLOW, 512);
        assert_eq!(RF_FRAMELERP, 64);
        assert_eq!(RF_BEAM, 128);
        let effects = [
            (EF_GIB, 0x2),
            (EF_BLASTER, 0x8),
            (EF_ROCKET, 0x10),
            (EF_GRENADE, 0x20),
            (EF_HYPERBLASTER, 0x40),
            (EF_BFG, 0x80),
            (EF_ANIM01, 0x400),
            (EF_ANIM23, 0x800),
            (EF_ANIM_ALL, 0x1000),
            (EF_ANIM_ALLFAST, 0x2000),
            (EF_TELEPORTER, 0x2_0000),
        ];
        for (i, (rust, c)) in effects.into_iter().enumerate() {
            assert_eq!(rust, c, "effect {i}");
        }
        assert_eq!((EV_PLAYER_TELEPORT, EV_OTHER_TELEPORT), (6, 7));
    }

    #[test]
    fn update_mask_is_power_of_two_minus_one() {
        assert_eq!(UPDATE_MASK, 15);
        assert_eq!(UPDATE_BACKUP & UPDATE_MASK, 0);
    }

    #[test]
    fn monster_muzzle_flash_numbers_match_c_shared_h() {
        assert_eq!(MZ2_TANK_BLASTER_1, 1);
        assert_eq!(MZ2_SOLDIER_BLASTER_1, 39);
        assert_eq!(MZ2_SOLDIER_MACHINEGUN_8, 100);
        assert_eq!(MZ2_MAKRON_BFG, 101);
        assert_eq!(MZ2_WIDOW2_BEAM_SWEEP_11, 210);
    }
}
