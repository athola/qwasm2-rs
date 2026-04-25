//! Item system — definitions, pickup handlers, and inventory management.
//!
//! Faithful port of item definitions from `g_items.c` (2,712 lines).
//! All 41 items from the base Q2 game are defined with exact values.
//!
//! # C source reference
//! `~/Qwasm2/src/game/g_items.c`

use crate::constants::*;
use crate::entity::EntityKey;
use crate::world::GameWorld;

// ---------------------------------------------------------------------------
// Armor info — matches gitem_armor_t from local.h
// ---------------------------------------------------------------------------

/// Armor protection values. C ref: `gitem_armor_t`.
#[derive(Debug, Clone, Copy)]
pub struct ArmorInfo {
    /// Starting armor value when picked up.
    pub base_count: i32,
    /// Maximum armor value for this type.
    pub max_count: i32,
    /// Protection multiplier vs normal (ballistic/melee) damage.
    pub normal_protection: f32,
    /// Protection multiplier vs energy weapon damage.
    pub energy_protection: f32,
    /// Armor type tag.
    pub armor_type: ArmorType,
}

/// Jacket Armor: weak but common. C ref: `jacketarmor_info`.
pub const JACKET_ARMOR_INFO: ArmorInfo = ArmorInfo {
    base_count: 25,
    max_count: 50,
    normal_protection: 0.30,
    energy_protection: 0.00,
    armor_type: ArmorType::Jacket,
};

/// Combat Armor: medium tier. C ref: `combatarmor_info`.
pub const COMBAT_ARMOR_INFO: ArmorInfo = ArmorInfo {
    base_count: 50,
    max_count: 100,
    normal_protection: 0.60,
    energy_protection: 0.30,
    armor_type: ArmorType::Combat,
};

/// Body Armor: best tier. C ref: `bodyarmor_info`.
pub const BODY_ARMOR_INFO: ArmorInfo = ArmorInfo {
    base_count: 100,
    max_count: 200,
    normal_protection: 0.80,
    energy_protection: 0.60,
    armor_type: ArmorType::Body,
};

// ---------------------------------------------------------------------------
// Ammo maximums — default and with bandolier/pack
// ---------------------------------------------------------------------------

/// Maximum ammo counts. C ref: various cvar defaults in `g_items.c`.
#[derive(Debug, Clone, Copy)]
pub struct AmmoMax {
    pub bullets: i32,
    pub shells: i32,
    pub rockets: i32,
    pub grenades: i32,
    pub cells: i32,
    pub slugs: i32,
}

/// Default ammo limits.
pub const AMMO_MAX_DEFAULT: AmmoMax = AmmoMax {
    bullets: 200,
    shells: 100,
    rockets: 50,
    grenades: 50,
    cells: 200,
    slugs: 50,
};

/// Ammo limits with Bandolier pickup.
pub const AMMO_MAX_BANDOLIER: AmmoMax = AmmoMax {
    bullets: 250,
    shells: 150,
    rockets: 50,
    grenades: 50,
    cells: 250,
    slugs: 75,
};

/// Ammo limits with Ammo Pack pickup.
pub const AMMO_MAX_PACK: AmmoMax = AmmoMax {
    bullets: 300,
    shells: 200,
    rockets: 100,
    grenades: 100,
    cells: 300,
    slugs: 100,
};

// ---------------------------------------------------------------------------
// Item definition
// ---------------------------------------------------------------------------

/// Pickup callback: (world, item_entity, player_entity) → whether pickup succeeded.
pub type PickupFn = fn(&mut GameWorld, EntityKey, EntityKey) -> bool;

/// Use callback: (world, player_entity, item_index).
pub type UseItemFn = fn(&mut GameWorld, EntityKey, usize);

/// Drop callback: (world, player_entity, item_index).
pub type DropItemFn = fn(&mut GameWorld, EntityKey, usize);

/// A single item definition. Replaces `gitem_t` from C.
///
/// Items are stored in a global list on `GameWorld`. Each item is identified
/// by its index in this list, matching C's `ITEM_INDEX()` macro.
#[derive(Clone)]
pub struct ItemDef {
    /// Spawn classname (e.g., "weapon_shotgun").
    pub classname: &'static str,
    /// Sound played on pickup.
    pub pickup_sound: &'static str,
    /// World model path.
    pub world_model: &'static str,
    /// HUD icon name.
    pub icon: &'static str,
    /// Display name for pickup messages.
    pub pickup_name: &'static str,
    /// Ammo quantity per pickup, or ammo per shot for weapons.
    pub quantity: i32,
    /// Ammo type name (for weapons that consume ammo).
    pub ammo: &'static str,
    /// Item classification flags.
    pub flags: ItemFlags,
    /// Sub-type tag (ARMOR_*, AMMO_*, WEAP_*).
    pub tag: i32,
    /// Armor info (only for armor items).
    pub armor_info: Option<ArmorInfo>,
    /// Pickup handler.
    pub pickup: Option<PickupFn>,
    /// Use handler.
    pub use_fn: Option<UseItemFn>,
    /// Drop handler.
    pub drop_fn: Option<DropItemFn>,
}

impl std::fmt::Debug for ItemDef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ItemDef")
            .field("classname", &self.classname)
            .field("pickup_name", &self.pickup_name)
            .field("flags", &self.flags)
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// Tag constants (matching C defines)
// ---------------------------------------------------------------------------

// Armor tags
pub const ARMOR_SHARD: i32 = 4;

// Ammo tags — used as inventory indices offset
pub const AMMO_SHELLS: i32 = 1;
pub const AMMO_BULLETS: i32 = 2;
pub const AMMO_CELLS: i32 = 3;
pub const AMMO_ROCKETS: i32 = 4;
pub const AMMO_SLUGS: i32 = 5;
pub const AMMO_GRENADES: i32 = 6;

// Weapon model indices
pub const WEAP_BLASTER: i32 = 1;
pub const WEAP_SHOTGUN: i32 = 2;
pub const WEAP_SUPERSHOTGUN: i32 = 3;
pub const WEAP_MACHINEGUN: i32 = 4;
pub const WEAP_CHAINGUN: i32 = 5;
pub const WEAP_GRENADELAUNCHER: i32 = 7;
pub const WEAP_ROCKETLAUNCHER: i32 = 8;
pub const WEAP_HYPERBLASTER: i32 = 9;
pub const WEAP_RAILGUN: i32 = 10;
pub const WEAP_BFG: i32 = 11;

// ---------------------------------------------------------------------------
// Build the item list — all 42 items (index 0 = null)
// ---------------------------------------------------------------------------

/// Build the complete item list matching C's `itemlist[]`.
pub fn build_item_list() -> Vec<ItemDef> {
    let null_item = || ItemDef {
        classname: "",
        pickup_sound: "",
        world_model: "",
        icon: "",
        pickup_name: "",
        quantity: 0,
        ammo: "",
        flags: ItemFlags::empty(),
        tag: 0,
        armor_info: None,
        pickup: None,
        use_fn: None,
        drop_fn: None,
    };

    vec![
        // [0] null
        null_item(),
        // [1] Body Armor
        ItemDef {
            classname: "item_armor_body",
            pickup_sound: "misc/ar1_pkup.wav",
            world_model: "models/items/armor/body/tris.md2",
            icon: "i_bodyarmor",
            pickup_name: "Body Armor",
            quantity: 0,
            ammo: "",
            flags: ItemFlags::ARMOR,
            tag: ArmorType::Body as i32,
            armor_info: Some(BODY_ARMOR_INFO),
            pickup: Some(pickup_armor),
            use_fn: None,
            drop_fn: None,
        },
        // [2] Combat Armor
        ItemDef {
            classname: "item_armor_combat",
            pickup_sound: "misc/ar1_pkup.wav",
            world_model: "models/items/armor/combat/tris.md2",
            icon: "i_combatarmor",
            pickup_name: "Combat Armor",
            quantity: 0,
            ammo: "",
            flags: ItemFlags::ARMOR,
            tag: ArmorType::Combat as i32,
            armor_info: Some(COMBAT_ARMOR_INFO),
            pickup: Some(pickup_armor),
            use_fn: None,
            drop_fn: None,
        },
        // [3] Jacket Armor
        ItemDef {
            classname: "item_armor_jacket",
            pickup_sound: "misc/ar1_pkup.wav",
            world_model: "models/items/armor/jacket/tris.md2",
            icon: "i_jacketarmor",
            pickup_name: "Jacket Armor",
            quantity: 0,
            ammo: "",
            flags: ItemFlags::ARMOR,
            tag: ArmorType::Jacket as i32,
            armor_info: Some(JACKET_ARMOR_INFO),
            pickup: Some(pickup_armor),
            use_fn: None,
            drop_fn: None,
        },
        // [4] Armor Shard
        ItemDef {
            classname: "item_armor_shard",
            pickup_sound: "misc/ar2_pkup.wav",
            world_model: "models/items/armor/shard/tris.md2",
            icon: "i_jacketarmor",
            pickup_name: "Armor Shard",
            quantity: 2,
            ammo: "",
            flags: ItemFlags::ARMOR,
            tag: ARMOR_SHARD,
            armor_info: None,
            pickup: Some(pickup_armor),
            use_fn: None,
            drop_fn: None,
        },
        // [5] Power Screen
        ItemDef {
            classname: "item_power_screen",
            pickup_sound: "misc/ar3_pkup.wav",
            world_model: "models/items/armor/screen/tris.md2",
            icon: "i_powerscreen",
            pickup_name: "Power Screen",
            quantity: 60,
            ammo: "",
            flags: ItemFlags::ARMOR,
            tag: 0,
            armor_info: None,
            pickup: None,
            use_fn: None,
            drop_fn: None,
        },
        // [6] Power Shield
        ItemDef {
            classname: "item_power_shield",
            pickup_sound: "misc/ar3_pkup.wav",
            world_model: "models/items/armor/shield/tris.md2",
            icon: "i_powershield",
            pickup_name: "Power Shield",
            quantity: 60,
            ammo: "",
            flags: ItemFlags::ARMOR,
            tag: 0,
            armor_info: None,
            pickup: None,
            use_fn: None,
            drop_fn: None,
        },
        // [7] Blaster
        ItemDef {
            classname: "weapon_blaster",
            pickup_sound: "misc/w_pkup.wav",
            world_model: "",
            icon: "w_blaster",
            pickup_name: "Blaster",
            quantity: 0,
            ammo: "",
            flags: ItemFlags::WEAPON | ItemFlags::STAY_COOP,
            tag: WEAP_BLASTER,
            armor_info: None,
            pickup: Some(pickup_weapon),
            use_fn: None,
            drop_fn: None,
        },
        // [8] Shotgun
        ItemDef {
            classname: "weapon_shotgun",
            pickup_sound: "misc/w_pkup.wav",
            world_model: "models/weapons/g_shotg/tris.md2",
            icon: "w_shotgun",
            pickup_name: "Shotgun",
            quantity: 1,
            ammo: "Shells",
            flags: ItemFlags::WEAPON | ItemFlags::STAY_COOP,
            tag: WEAP_SHOTGUN,
            armor_info: None,
            pickup: Some(pickup_weapon),
            use_fn: None,
            drop_fn: None,
        },
        // [9] Super Shotgun
        ItemDef {
            classname: "weapon_supershotgun",
            pickup_sound: "misc/w_pkup.wav",
            world_model: "models/weapons/g_shotg2/tris.md2",
            icon: "w_sshotgun",
            pickup_name: "Super Shotgun",
            quantity: 2,
            ammo: "Shells",
            flags: ItemFlags::WEAPON | ItemFlags::STAY_COOP,
            tag: WEAP_SUPERSHOTGUN,
            armor_info: None,
            pickup: Some(pickup_weapon),
            use_fn: None,
            drop_fn: None,
        },
        // [10] Machinegun
        ItemDef {
            classname: "weapon_machinegun",
            pickup_sound: "misc/w_pkup.wav",
            world_model: "models/weapons/g_machn/tris.md2",
            icon: "w_machinegun",
            pickup_name: "Machinegun",
            quantity: 1,
            ammo: "Bullets",
            flags: ItemFlags::WEAPON | ItemFlags::STAY_COOP,
            tag: WEAP_MACHINEGUN,
            armor_info: None,
            pickup: Some(pickup_weapon),
            use_fn: None,
            drop_fn: None,
        },
        // [11] Chaingun
        ItemDef {
            classname: "weapon_chaingun",
            pickup_sound: "misc/w_pkup.wav",
            world_model: "models/weapons/g_chain/tris.md2",
            icon: "w_chaingun",
            pickup_name: "Chaingun",
            quantity: 1,
            ammo: "Bullets",
            flags: ItemFlags::WEAPON | ItemFlags::STAY_COOP,
            tag: WEAP_CHAINGUN,
            armor_info: None,
            pickup: Some(pickup_weapon),
            use_fn: None,
            drop_fn: None,
        },
        // [12] Grenades (ammo + weapon combo)
        ItemDef {
            classname: "ammo_grenades",
            pickup_sound: "misc/am_pkup.wav",
            world_model: "models/items/ammo/grenades/medium/tris.md2",
            icon: "a_grenades",
            pickup_name: "Grenades",
            quantity: 5,
            ammo: "grenades",
            flags: ItemFlags::AMMO | ItemFlags::WEAPON,
            tag: AMMO_GRENADES,
            armor_info: None,
            pickup: Some(pickup_ammo),
            use_fn: None,
            drop_fn: None,
        },
        // [13] Grenade Launcher
        ItemDef {
            classname: "weapon_grenadelauncher",
            pickup_sound: "misc/w_pkup.wav",
            world_model: "models/weapons/g_launch/tris.md2",
            icon: "w_glauncher",
            pickup_name: "Grenade Launcher",
            quantity: 1,
            ammo: "Grenades",
            flags: ItemFlags::WEAPON | ItemFlags::STAY_COOP,
            tag: WEAP_GRENADELAUNCHER,
            armor_info: None,
            pickup: Some(pickup_weapon),
            use_fn: None,
            drop_fn: None,
        },
        // [14] Rocket Launcher
        ItemDef {
            classname: "weapon_rocketlauncher",
            pickup_sound: "misc/w_pkup.wav",
            world_model: "models/weapons/g_rocket/tris.md2",
            icon: "w_rlauncher",
            pickup_name: "Rocket Launcher",
            quantity: 1,
            ammo: "Rockets",
            flags: ItemFlags::WEAPON | ItemFlags::STAY_COOP,
            tag: WEAP_ROCKETLAUNCHER,
            armor_info: None,
            pickup: Some(pickup_weapon),
            use_fn: None,
            drop_fn: None,
        },
        // [15] HyperBlaster
        ItemDef {
            classname: "weapon_hyperblaster",
            pickup_sound: "misc/w_pkup.wav",
            world_model: "models/weapons/g_hyperb/tris.md2",
            icon: "w_hyperblaster",
            pickup_name: "HyperBlaster",
            quantity: 1,
            ammo: "Cells",
            flags: ItemFlags::WEAPON | ItemFlags::STAY_COOP,
            tag: WEAP_HYPERBLASTER,
            armor_info: None,
            pickup: Some(pickup_weapon),
            use_fn: None,
            drop_fn: None,
        },
        // [16] Railgun
        ItemDef {
            classname: "weapon_railgun",
            pickup_sound: "misc/w_pkup.wav",
            world_model: "models/weapons/g_rail/tris.md2",
            icon: "w_railgun",
            pickup_name: "Railgun",
            quantity: 1,
            ammo: "Slugs",
            flags: ItemFlags::WEAPON | ItemFlags::STAY_COOP,
            tag: WEAP_RAILGUN,
            armor_info: None,
            pickup: Some(pickup_weapon),
            use_fn: None,
            drop_fn: None,
        },
        // [17] BFG10K
        ItemDef {
            classname: "weapon_bfg",
            pickup_sound: "misc/w_pkup.wav",
            world_model: "models/weapons/g_bfg/tris.md2",
            icon: "w_bfg",
            pickup_name: "BFG10K",
            quantity: 50,
            ammo: "Cells",
            flags: ItemFlags::WEAPON | ItemFlags::STAY_COOP,
            tag: WEAP_BFG,
            armor_info: None,
            pickup: Some(pickup_weapon),
            use_fn: None,
            drop_fn: None,
        },
        // [18] Shells
        ItemDef {
            classname: "ammo_shells",
            pickup_sound: "misc/am_pkup.wav",
            world_model: "models/items/ammo/shells/medium/tris.md2",
            icon: "a_shells",
            pickup_name: "Shells",
            quantity: 10,
            ammo: "",
            flags: ItemFlags::AMMO,
            tag: AMMO_SHELLS,
            armor_info: None,
            pickup: Some(pickup_ammo),
            use_fn: None,
            drop_fn: None,
        },
        // [19] Bullets
        ItemDef {
            classname: "ammo_bullets",
            pickup_sound: "misc/am_pkup.wav",
            world_model: "models/items/ammo/bullets/medium/tris.md2",
            icon: "a_bullets",
            pickup_name: "Bullets",
            quantity: 50,
            ammo: "",
            flags: ItemFlags::AMMO,
            tag: AMMO_BULLETS,
            armor_info: None,
            pickup: Some(pickup_ammo),
            use_fn: None,
            drop_fn: None,
        },
        // [20] Cells
        ItemDef {
            classname: "ammo_cells",
            pickup_sound: "misc/am_pkup.wav",
            world_model: "models/items/ammo/cells/medium/tris.md2",
            icon: "a_cells",
            pickup_name: "Cells",
            quantity: 50,
            ammo: "",
            flags: ItemFlags::AMMO,
            tag: AMMO_CELLS,
            armor_info: None,
            pickup: Some(pickup_ammo),
            use_fn: None,
            drop_fn: None,
        },
        // [21] Rockets
        ItemDef {
            classname: "ammo_rockets",
            pickup_sound: "misc/am_pkup.wav",
            world_model: "models/items/ammo/rockets/medium/tris.md2",
            icon: "a_rockets",
            pickup_name: "Rockets",
            quantity: 5,
            ammo: "",
            flags: ItemFlags::AMMO,
            tag: AMMO_ROCKETS,
            armor_info: None,
            pickup: Some(pickup_ammo),
            use_fn: None,
            drop_fn: None,
        },
        // [22] Slugs
        ItemDef {
            classname: "ammo_slugs",
            pickup_sound: "misc/am_pkup.wav",
            world_model: "models/items/ammo/slugs/medium/tris.md2",
            icon: "a_slugs",
            pickup_name: "Slugs",
            quantity: 10,
            ammo: "",
            flags: ItemFlags::AMMO,
            tag: AMMO_SLUGS,
            armor_info: None,
            pickup: Some(pickup_ammo),
            use_fn: None,
            drop_fn: None,
        },
        // [23] Quad Damage
        ItemDef {
            classname: "item_quad",
            pickup_sound: "items/pkup.wav",
            world_model: "models/items/quaddama/tris.md2",
            icon: "p_quad",
            pickup_name: "Quad Damage",
            quantity: 60,
            ammo: "",
            flags: ItemFlags::POWERUP | ItemFlags::INSTANT_USE,
            tag: 0,
            armor_info: None,
            pickup: Some(pickup_powerup),
            use_fn: None,
            drop_fn: None,
        },
        // [24] Invulnerability
        ItemDef {
            classname: "item_invulnerability",
            pickup_sound: "items/pkup.wav",
            world_model: "models/items/invulner/tris.md2",
            icon: "p_invulnerability",
            pickup_name: "Invulnerability",
            quantity: 300,
            ammo: "",
            flags: ItemFlags::POWERUP | ItemFlags::INSTANT_USE,
            tag: 0,
            armor_info: None,
            pickup: Some(pickup_powerup),
            use_fn: None,
            drop_fn: None,
        },
        // [25] Silencer
        ItemDef {
            classname: "item_silencer",
            pickup_sound: "items/pkup.wav",
            world_model: "models/items/silencer/tris.md2",
            icon: "p_silencer",
            pickup_name: "Silencer",
            quantity: 60,
            ammo: "",
            flags: ItemFlags::POWERUP | ItemFlags::INSTANT_USE,
            tag: 0,
            armor_info: None,
            pickup: Some(pickup_powerup),
            use_fn: None,
            drop_fn: None,
        },
        // [26] Rebreather
        ItemDef {
            classname: "item_breather",
            pickup_sound: "items/pkup.wav",
            world_model: "models/items/breather/tris.md2",
            icon: "p_rebreather",
            pickup_name: "Rebreather",
            quantity: 60,
            ammo: "",
            flags: ItemFlags::STAY_COOP | ItemFlags::POWERUP | ItemFlags::INSTANT_USE,
            tag: 0,
            armor_info: None,
            pickup: Some(pickup_powerup),
            use_fn: None,
            drop_fn: None,
        },
        // [27] Environment Suit
        ItemDef {
            classname: "item_enviro",
            pickup_sound: "items/pkup.wav",
            world_model: "models/items/enviro/tris.md2",
            icon: "p_envirosuit",
            pickup_name: "Environment Suit",
            quantity: 60,
            ammo: "",
            flags: ItemFlags::STAY_COOP | ItemFlags::POWERUP | ItemFlags::INSTANT_USE,
            tag: 0,
            armor_info: None,
            pickup: Some(pickup_powerup),
            use_fn: None,
            drop_fn: None,
        },
        // [28] Ancient Head
        ItemDef {
            classname: "item_ancient_head",
            pickup_sound: "items/pkup.wav",
            world_model: "models/items/c_head/tris.md2",
            icon: "i_fixme",
            pickup_name: "Ancient Head",
            quantity: 60,
            ammo: "",
            flags: ItemFlags::empty(),
            tag: 0,
            armor_info: None,
            pickup: Some(pickup_powerup),
            use_fn: None,
            drop_fn: None,
        },
        // [29] Adrenaline
        ItemDef {
            classname: "item_adrenaline",
            pickup_sound: "items/pkup.wav",
            world_model: "models/items/adrenal/tris.md2",
            icon: "p_adrenaline",
            pickup_name: "Adrenaline",
            quantity: 60,
            ammo: "",
            flags: ItemFlags::empty(),
            tag: 0,
            armor_info: None,
            pickup: Some(pickup_powerup),
            use_fn: None,
            drop_fn: None,
        },
        // [30] Bandolier
        ItemDef {
            classname: "item_bandolier",
            pickup_sound: "items/pkup.wav",
            world_model: "models/items/band/tris.md2",
            icon: "p_bandolier",
            pickup_name: "Bandolier",
            quantity: 60,
            ammo: "",
            flags: ItemFlags::empty(),
            tag: 0,
            armor_info: None,
            pickup: Some(pickup_powerup),
            use_fn: None,
            drop_fn: None,
        },
        // [31] Ammo Pack
        ItemDef {
            classname: "item_pack",
            pickup_sound: "items/pkup.wav",
            world_model: "models/items/pack/tris.md2",
            icon: "i_pack",
            pickup_name: "Ammo Pack",
            quantity: 180,
            ammo: "",
            flags: ItemFlags::empty(),
            tag: 0,
            armor_info: None,
            pickup: Some(pickup_powerup),
            use_fn: None,
            drop_fn: None,
        },
        // [32-40] Keys — 9 key items
        ItemDef {
            classname: "key_data_cd", pickup_sound: "items/pkup.wav",
            world_model: "models/items/keys/data_cd/tris.md2",
            icon: "k_datacd", pickup_name: "Data CD", quantity: 0, ammo: "",
            flags: ItemFlags::STAY_COOP | ItemFlags::KEY, tag: 0,
            armor_info: None, pickup: Some(pickup_key), use_fn: None, drop_fn: None,
        },
        ItemDef {
            classname: "key_power_cube", pickup_sound: "items/pkup.wav",
            world_model: "models/items/keys/power/tris.md2",
            icon: "k_powercube", pickup_name: "Power Cube", quantity: 0, ammo: "",
            flags: ItemFlags::STAY_COOP | ItemFlags::KEY, tag: 0,
            armor_info: None, pickup: Some(pickup_key), use_fn: None, drop_fn: None,
        },
        ItemDef {
            classname: "key_pyramid", pickup_sound: "items/pkup.wav",
            world_model: "models/items/keys/pyramid/tris.md2",
            icon: "k_pyramid", pickup_name: "Pyramid Key", quantity: 0, ammo: "",
            flags: ItemFlags::STAY_COOP | ItemFlags::KEY, tag: 0,
            armor_info: None, pickup: Some(pickup_key), use_fn: None, drop_fn: None,
        },
        ItemDef {
            classname: "key_data_spinner", pickup_sound: "items/pkup.wav",
            world_model: "models/items/keys/spinner/tris.md2",
            icon: "k_dataspin", pickup_name: "Data Spinner", quantity: 0, ammo: "",
            flags: ItemFlags::STAY_COOP | ItemFlags::KEY, tag: 0,
            armor_info: None, pickup: Some(pickup_key), use_fn: None, drop_fn: None,
        },
        ItemDef {
            classname: "key_pass", pickup_sound: "items/pkup.wav",
            world_model: "models/items/keys/pass/tris.md2",
            icon: "k_security", pickup_name: "Security Pass", quantity: 0, ammo: "",
            flags: ItemFlags::STAY_COOP | ItemFlags::KEY, tag: 0,
            armor_info: None, pickup: Some(pickup_key), use_fn: None, drop_fn: None,
        },
        ItemDef {
            classname: "key_blue_key", pickup_sound: "items/pkup.wav",
            world_model: "models/items/keys/key/tris.md2",
            icon: "k_bluekey", pickup_name: "Blue Key", quantity: 0, ammo: "",
            flags: ItemFlags::STAY_COOP | ItemFlags::KEY, tag: 0,
            armor_info: None, pickup: Some(pickup_key), use_fn: None, drop_fn: None,
        },
        ItemDef {
            classname: "key_red_key", pickup_sound: "items/pkup.wav",
            world_model: "models/items/keys/red_key/tris.md2",
            icon: "k_redkey", pickup_name: "Red Key", quantity: 0, ammo: "",
            flags: ItemFlags::STAY_COOP | ItemFlags::KEY, tag: 0,
            armor_info: None, pickup: Some(pickup_key), use_fn: None, drop_fn: None,
        },
        ItemDef {
            classname: "key_commander_head", pickup_sound: "items/pkup.wav",
            world_model: "models/monsters/commandr/head/tris.md2",
            icon: "k_comhead", pickup_name: "Commander's Head", quantity: 0, ammo: "",
            flags: ItemFlags::STAY_COOP | ItemFlags::KEY, tag: 0,
            armor_info: None, pickup: Some(pickup_key), use_fn: None, drop_fn: None,
        },
        ItemDef {
            classname: "key_airstrike_target", pickup_sound: "items/pkup.wav",
            world_model: "models/items/keys/target/tris.md2",
            icon: "i_airstrike", pickup_name: "Airstrike Marker", quantity: 0, ammo: "",
            flags: ItemFlags::STAY_COOP | ItemFlags::KEY, tag: 0,
            armor_info: None, pickup: Some(pickup_key), use_fn: None, drop_fn: None,
        },
        // [41] Health (generic — not spawned directly, used for health pickups)
        ItemDef {
            classname: "item_health",
            pickup_sound: "items/pkup.wav",
            world_model: "",
            icon: "i_health",
            pickup_name: "Health",
            quantity: 0,
            ammo: "",
            flags: ItemFlags::empty(),
            tag: 0,
            armor_info: None,
            pickup: Some(pickup_health),
            use_fn: None,
            drop_fn: None,
        },
    ]
}

// ---------------------------------------------------------------------------
// Pickup handlers
// ---------------------------------------------------------------------------

/// Pickup handler for armor items.
/// C ref: `Pickup_Armor` (g_items.c:910-1011)
fn pickup_armor(world: &mut GameWorld, item_ent: EntityKey, player: EntityKey) -> bool {
    let item_idx = world
        .entities
        .get(item_ent)
        .and_then(|e| e.game.item);
    let item_idx = match item_idx {
        Some(i) => i,
        None => return false,
    };

    let armor_info = match world.items.get(item_idx).and_then(|i| i.armor_info) {
        Some(info) => info,
        None => {
            // Armor shard: add 2 to current armor.
            if let Some(ent) = world.entities.get_mut(player) {
                if let Some(ref mut client) = ent.client {
                    client.pers.inventory[1] += 2;
                    return true;
                }
            }
            return false;
        }
    };

    let current_armor = world
        .entities
        .get(player)
        .and_then(|e| e.client.as_ref())
        .map(|c| c.pers.inventory[1])
        .unwrap_or(0);

    // Simple pickup: set to base_count if higher than current.
    let new_count = armor_info.base_count.max(current_armor);
    if new_count <= current_armor && current_armor >= armor_info.max_count {
        return false; // Already at or above max.
    }

    if let Some(ent) = world.entities.get_mut(player) {
        if let Some(ref mut client) = ent.client {
            client.pers.inventory[1] = new_count.min(armor_info.max_count);
        }
    }

    true
}

/// Pickup handler for ammo items.
/// C ref: `Pickup_Ammo` (g_items.c:714-764)
fn pickup_ammo(world: &mut GameWorld, item_ent: EntityKey, player: EntityKey) -> bool {
    let item_idx = world
        .entities
        .get(item_ent)
        .and_then(|e| e.game.item);
    let item_idx = match item_idx {
        Some(i) => i,
        None => return false,
    };

    let (quantity, tag) = match world.items.get(item_idx) {
        Some(item) => (item.quantity, item.tag),
        None => return false,
    };

    // Map ammo tag to inventory slot.
    let inv_slot = ammo_tag_to_slot(tag);
    if inv_slot == 0 {
        return false;
    }

    let max = ammo_max_for_tag(tag);
    let current = world
        .entities
        .get(player)
        .and_then(|e| e.client.as_ref())
        .map(|c| c.pers.inventory[inv_slot])
        .unwrap_or(0);

    if current >= max {
        return false; // Already at max.
    }

    if let Some(ent) = world.entities.get_mut(player) {
        if let Some(ref mut client) = ent.client {
            client.pers.inventory[inv_slot] = (current + quantity).min(max);
        }
    }

    true
}

/// Pickup handler for weapon items.
/// C ref: `Pickup_Weapon` (player/weapon.c:268-336)
fn pickup_weapon(world: &mut GameWorld, item_ent: EntityKey, player: EntityKey) -> bool {
    let item_idx = world
        .entities
        .get(item_ent)
        .and_then(|e| e.game.item);
    let item_idx = match item_idx {
        Some(i) => i,
        None => return false,
    };

    // Add weapon to inventory.
    if let Some(ent) = world.entities.get_mut(player) {
        if let Some(ref mut client) = ent.client {
            client.pers.inventory[item_idx] += 1;
        }
    }

    // Give some starting ammo.
    if let Some(item) = world.items.get(item_idx) {
        if !item.ammo.is_empty() {
            if let Some(ammo_idx) = find_item_index_by_name(&world.items, item.ammo) {
                if let Some(ammo_item) = world.items.get(ammo_idx) {
                    let inv_slot = ammo_tag_to_slot(ammo_item.tag);
                    if inv_slot > 0 {
                        let max = ammo_max_for_tag(ammo_item.tag);
                        if let Some(ent) = world.entities.get_mut(player) {
                            if let Some(ref mut client) = ent.client {
                                let current = client.pers.inventory[inv_slot];
                                client.pers.inventory[inv_slot] =
                                    (current + ammo_item.quantity).min(max);
                            }
                        }
                    }
                }
            }
        }
    }

    true
}

/// Pickup handler for health items.
/// C ref: `Pickup_Health` (g_items.c:831-874)
fn pickup_health(world: &mut GameWorld, item_ent: EntityKey, player: EntityKey) -> bool {
    let amount = world
        .entities
        .get(item_ent)
        .map(|e| e.game.count)
        .unwrap_or(10);

    let (health, max_health) = world
        .entities
        .get(player)
        .map(|e| (e.game.health, e.game.max_health))
        .unwrap_or((0, 100));

    if health >= max_health {
        return false;
    }

    if let Some(ent) = world.entities.get_mut(player) {
        ent.game.health = (health + amount).min(max_health);
    }

    true
}

/// Pickup handler for powerup items.
/// C ref: various Use_* functions in g_items.c
fn pickup_powerup(world: &mut GameWorld, item_ent: EntityKey, player: EntityKey) -> bool {
    let item_idx = world
        .entities
        .get(item_ent)
        .and_then(|e| e.game.item);
    let item_idx = match item_idx {
        Some(i) => i,
        None => return false,
    };

    // Add to inventory.
    if let Some(ent) = world.entities.get_mut(player) {
        if let Some(ref mut client) = ent.client {
            client.pers.inventory[item_idx] += 1;
        }
    }

    true
}

/// Pickup handler for key items.
fn pickup_key(world: &mut GameWorld, item_ent: EntityKey, player: EntityKey) -> bool {
    let item_idx = world
        .entities
        .get(item_ent)
        .and_then(|e| e.game.item);
    let item_idx = match item_idx {
        Some(i) => i,
        None => return false,
    };

    if let Some(ent) = world.entities.get_mut(player) {
        if let Some(ref mut client) = ent.client {
            client.pers.inventory[item_idx] += 1;
        }
    }

    true
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Find an item by classname. Returns the index in the item list.
pub fn find_item_index(items: &[ItemDef], classname: &str) -> Option<usize> {
    items
        .iter()
        .position(|i| i.classname == classname)
}

/// Find an item by pickup_name. Returns the index in the item list.
pub fn find_item_index_by_name(items: &[ItemDef], name: &str) -> Option<usize> {
    items
        .iter()
        .position(|i| i.pickup_name.eq_ignore_ascii_case(name))
}

/// Map an ammo tag to the inventory slot offset.
fn ammo_tag_to_slot(tag: i32) -> usize {
    match tag {
        t if t == AMMO_SHELLS => 50,   // inventory offset for shells
        t if t == AMMO_BULLETS => 51,
        t if t == AMMO_CELLS => 52,
        t if t == AMMO_ROCKETS => 53,
        t if t == AMMO_SLUGS => 54,
        t if t == AMMO_GRENADES => 55,
        _ => 0,
    }
}

/// Get max ammo count for a given ammo tag.
fn ammo_max_for_tag(tag: i32) -> i32 {
    match tag {
        t if t == AMMO_SHELLS => AMMO_MAX_DEFAULT.shells,
        t if t == AMMO_BULLETS => AMMO_MAX_DEFAULT.bullets,
        t if t == AMMO_CELLS => AMMO_MAX_DEFAULT.cells,
        t if t == AMMO_ROCKETS => AMMO_MAX_DEFAULT.rockets,
        t if t == AMMO_SLUGS => AMMO_MAX_DEFAULT.slugs,
        t if t == AMMO_GRENADES => AMMO_MAX_DEFAULT.grenades,
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// GameWorld item integration
// ---------------------------------------------------------------------------

impl GameWorld {
    /// Initialize the item list on the game world.
    pub fn init_items(&mut self) {
        self.items = build_item_list();
    }

    /// Find an item index by classname.
    pub fn find_item(&self, classname: &str) -> Option<usize> {
        find_item_index(&self.items, classname)
    }
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::test_world;

    #[test]
    fn item_list_has_correct_count() {
        let items = build_item_list();
        assert_eq!(items.len(), 42); // 0=null + 41 items
    }

    #[test]
    fn item_list_first_is_null() {
        let items = build_item_list();
        assert_eq!(items[0].classname, "");
    }

    #[test]
    fn item_list_armor_entries() {
        let items = build_item_list();
        assert_eq!(items[1].classname, "item_armor_body");
        assert_eq!(items[1].pickup_name, "Body Armor");
        assert!(items[1].flags.contains(ItemFlags::ARMOR));
        assert!(items[1].armor_info.is_some());

        let info = items[1].armor_info.unwrap();
        assert_eq!(info.base_count, 100);
        assert_eq!(info.max_count, 200);
        assert_eq!(info.normal_protection, 0.80);
        assert_eq!(info.energy_protection, 0.60);
    }

    #[test]
    fn item_list_weapon_entries() {
        let items = build_item_list();
        assert_eq!(items[8].classname, "weapon_shotgun");
        assert_eq!(items[8].pickup_name, "Shotgun");
        assert!(items[8].flags.contains(ItemFlags::WEAPON));
        assert_eq!(items[8].ammo, "Shells");
        assert_eq!(items[8].quantity, 1);
    }

    #[test]
    fn item_list_ammo_entries() {
        let items = build_item_list();
        assert_eq!(items[18].classname, "ammo_shells");
        assert_eq!(items[18].pickup_name, "Shells");
        assert!(items[18].flags.contains(ItemFlags::AMMO));
        assert_eq!(items[18].quantity, 10);
    }

    #[test]
    fn item_list_powerup_entries() {
        let items = build_item_list();
        assert_eq!(items[23].classname, "item_quad");
        assert_eq!(items[23].pickup_name, "Quad Damage");
        assert!(items[23].flags.contains(ItemFlags::POWERUP));
    }

    #[test]
    fn item_list_key_entries() {
        let items = build_item_list();
        assert_eq!(items[32].classname, "key_data_cd");
        assert!(items[32].flags.contains(ItemFlags::KEY));
    }

    #[test]
    fn find_item_by_classname() {
        let items = build_item_list();
        assert_eq!(find_item_index(&items, "weapon_shotgun"), Some(8));
        assert_eq!(find_item_index(&items, "ammo_rockets"), Some(21));
        assert_eq!(find_item_index(&items, "nonexistent"), None);
    }

    #[test]
    fn find_item_by_name() {
        let items = build_item_list();
        assert_eq!(find_item_index_by_name(&items, "Shells"), Some(18));
        assert_eq!(find_item_index_by_name(&items, "shells"), Some(18)); // case insensitive
        assert_eq!(find_item_index_by_name(&items, "Rockets"), Some(21));
    }

    #[test]
    fn armor_info_values_match_c() {
        assert_eq!(JACKET_ARMOR_INFO.base_count, 25);
        assert_eq!(JACKET_ARMOR_INFO.max_count, 50);
        assert_eq!(JACKET_ARMOR_INFO.normal_protection, 0.30);
        assert_eq!(JACKET_ARMOR_INFO.energy_protection, 0.00);

        assert_eq!(COMBAT_ARMOR_INFO.base_count, 50);
        assert_eq!(COMBAT_ARMOR_INFO.max_count, 100);
        assert_eq!(COMBAT_ARMOR_INFO.normal_protection, 0.60);
        assert_eq!(COMBAT_ARMOR_INFO.energy_protection, 0.30);

        assert_eq!(BODY_ARMOR_INFO.base_count, 100);
        assert_eq!(BODY_ARMOR_INFO.max_count, 200);
        assert_eq!(BODY_ARMOR_INFO.normal_protection, 0.80);
        assert_eq!(BODY_ARMOR_INFO.energy_protection, 0.60);
    }

    #[test]
    fn ammo_max_defaults() {
        assert_eq!(AMMO_MAX_DEFAULT.bullets, 200);
        assert_eq!(AMMO_MAX_DEFAULT.shells, 100);
        assert_eq!(AMMO_MAX_DEFAULT.rockets, 50);
        assert_eq!(AMMO_MAX_DEFAULT.cells, 200);
        assert_eq!(AMMO_MAX_DEFAULT.slugs, 50);
    }

    // -- Pickup handler tests --

    #[test]
    fn pickup_ammo_adds_to_inventory() {
        let mut world = test_world();
        world.init_items();

        let player = world.spawn().unwrap();
        world.entities.get_mut(player).unwrap().client =
            Some(crate::entity::ClientData::default());

        let item_ent = world.spawn().unwrap();
        world.entities.get_mut(item_ent).unwrap().game.item = Some(18); // shells

        let result = pickup_ammo(&mut world, item_ent, player);
        assert!(result);

        let inv = world.entities.get(player).unwrap().client.as_ref().unwrap().pers.inventory;
        assert_eq!(inv[50], 10); // slot 50 = shells, quantity = 10
    }

    #[test]
    fn pickup_ammo_rejects_when_full() {
        let mut world = test_world();
        world.init_items();

        let player = world.spawn().unwrap();
        let mut client = crate::entity::ClientData::default();
        client.pers.inventory[50] = 100; // shells already at max
        world.entities.get_mut(player).unwrap().client = Some(client);

        let item_ent = world.spawn().unwrap();
        world.entities.get_mut(item_ent).unwrap().game.item = Some(18);

        let result = pickup_ammo(&mut world, item_ent, player);
        assert!(!result); // Should reject — already at max
    }

    #[test]
    fn pickup_weapon_adds_to_inventory() {
        let mut world = test_world();
        world.init_items();

        let player = world.spawn().unwrap();
        world.entities.get_mut(player).unwrap().client =
            Some(crate::entity::ClientData::default());

        let item_ent = world.spawn().unwrap();
        world.entities.get_mut(item_ent).unwrap().game.item = Some(8); // shotgun

        let result = pickup_weapon(&mut world, item_ent, player);
        assert!(result);

        let inv = world.entities.get(player).unwrap().client.as_ref().unwrap().pers.inventory;
        assert_eq!(inv[8], 1); // shotgun in inventory
    }

    #[test]
    fn pickup_health_increases_health() {
        let mut world = test_world();
        world.init_items();

        let player = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(player).unwrap();
            ent.game.health = 50;
            ent.game.max_health = 100;
        }

        let item_ent = world.spawn().unwrap();
        world.entities.get_mut(item_ent).unwrap().game.count = 25;
        world.entities.get_mut(item_ent).unwrap().game.item = Some(41);

        let result = pickup_health(&mut world, item_ent, player);
        assert!(result);
        assert_eq!(world.entities.get(player).unwrap().game.health, 75);
    }

    #[test]
    fn pickup_health_caps_at_max() {
        let mut world = test_world();

        let player = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(player).unwrap();
            ent.game.health = 90;
            ent.game.max_health = 100;
        }

        let item_ent = world.spawn().unwrap();
        world.entities.get_mut(item_ent).unwrap().game.count = 25;

        let result = pickup_health(&mut world, item_ent, player);
        assert!(result);
        assert_eq!(world.entities.get(player).unwrap().game.health, 100);
    }

    #[test]
    fn pickup_health_rejects_when_full() {
        let mut world = test_world();

        let player = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(player).unwrap();
            ent.game.health = 100;
            ent.game.max_health = 100;
        }

        let item_ent = world.spawn().unwrap();
        world.entities.get_mut(item_ent).unwrap().game.count = 25;

        let result = pickup_health(&mut world, item_ent, player);
        assert!(!result);
    }

    #[test]
    fn gameworld_init_items() {
        let mut world = test_world();
        assert!(world.items.is_empty());

        world.init_items();
        assert_eq!(world.items.len(), 42);
    }

    #[test]
    fn gameworld_find_item() {
        let mut world = test_world();
        world.init_items();

        assert_eq!(world.find_item("weapon_railgun"), Some(16));
        assert_eq!(world.find_item("item_quad"), Some(23));
        assert!(world.find_item("nonexistent").is_none());
    }

    // Verify all weapons have correct ammo references
    #[test]
    fn all_weapons_have_valid_ammo() {
        let items = build_item_list();
        for (i, item) in items.iter().enumerate() {
            if item.flags.contains(ItemFlags::WEAPON) && !item.ammo.is_empty() {
                let ammo = find_item_index_by_name(&items, item.ammo);
                assert!(
                    ammo.is_some(),
                    "Weapon '{}' (index {}) references ammo '{}' which doesn't exist",
                    item.pickup_name,
                    i,
                    item.ammo
                );
            }
        }
    }

    // Verify all items have unique classnames (except null)
    #[test]
    fn all_items_have_unique_classnames() {
        let items = build_item_list();
        for i in 1..items.len() {
            for j in (i + 1)..items.len() {
                if !items[i].classname.is_empty() {
                    assert_ne!(
                        items[i].classname, items[j].classname,
                        "Duplicate classname '{}' at indices {} and {}",
                        items[i].classname, i, j
                    );
                }
            }
        }
    }
}
