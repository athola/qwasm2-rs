//! Item system — definitions, pickup handlers, and inventory management.
//!
//! Port of the item table and single-player pickups of `g_items.c`. Not
//! ported: the skill rule that refuses extra powerups, weapon auto-switch
//! (#16), item use, the player drop commands (`Drop_Weapon`, `Drop_Ammo`,
//! `Drop_General`; `Drop_Item` itself is ported), and deathmatch/coop
//! respawn.
//!
//! # C source reference
//! `~/Qwasm2/src/game/g_items.c`

use crate::constants::*;
use crate::entity::EntityKey;
use crate::world::GameWorld;
use q2_shared::types::*;

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

/// Maximum ammo counts (C `client_persistant_t` `max_bullets`..).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

impl AmmoMax {
    /// The cap for ammo with item `tag` (C `Add_Ammo`); 0 for non-ammo.
    pub fn for_tag(&self, tag: i32) -> i32 {
        match tag {
            t if t == AMMO_SHELLS => self.shells,
            t if t == AMMO_BULLETS => self.bullets,
            t if t == AMMO_CELLS => self.cells,
            t if t == AMMO_ROCKETS => self.rockets,
            t if t == AMMO_SLUGS => self.slugs,
            t if t == AMMO_GRENADES => self.grenades,
            _ => 0,
        }
    }

    /// Raise every cap to at least `to`'s, as Bandolier and Pack do.
    fn raise_to(&mut self, to: AmmoMax) {
        self.bullets = self.bullets.max(to.bullets);
        self.shells = self.shells.max(to.shells);
        self.rockets = self.rockets.max(to.rockets);
        self.grenades = self.grenades.max(to.grenades);
        self.cells = self.cells.max(to.cells);
        self.slugs = self.slugs.max(to.slugs);
    }
}

/// Ammo limits with Bandolier pickup (rockets and grenades unchanged).
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
    /// Entity effects of the item in the world (C `world_model_flags`).
    pub world_model_flags: u32,
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

// Ammo tags (C `ammo_t`)
pub const AMMO_BULLETS: i32 = 0;
pub const AMMO_SHELLS: i32 = 1;
pub const AMMO_ROCKETS: i32 = 2;
pub const AMMO_GRENADES: i32 = 3;
pub const AMMO_CELLS: i32 = 4;
pub const AMMO_SLUGS: i32 = 5;

// Health item styles (C g_items.c)
const HEALTH_IGNORE_MAX: i32 = 1;
const HEALTH_TIMED: i32 = 2;

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
        world_model_flags: 0,
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
            world_model_flags: q2_shared::constants::EF_ROTATE,
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
            world_model_flags: q2_shared::constants::EF_ROTATE,
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
            world_model_flags: q2_shared::constants::EF_ROTATE,
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
            world_model_flags: q2_shared::constants::EF_ROTATE,
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
            world_model_flags: q2_shared::constants::EF_ROTATE,
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
            world_model_flags: q2_shared::constants::EF_ROTATE,
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
            world_model_flags: 0,
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
            world_model_flags: q2_shared::constants::EF_ROTATE,
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
            world_model_flags: q2_shared::constants::EF_ROTATE,
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
            world_model_flags: q2_shared::constants::EF_ROTATE,
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
            world_model_flags: q2_shared::constants::EF_ROTATE,
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
            world_model_flags: 0,
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
            world_model_flags: q2_shared::constants::EF_ROTATE,
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
            world_model_flags: q2_shared::constants::EF_ROTATE,
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
            world_model_flags: q2_shared::constants::EF_ROTATE,
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
            world_model_flags: q2_shared::constants::EF_ROTATE,
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
            world_model_flags: q2_shared::constants::EF_ROTATE,
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
            world_model_flags: 0,
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
            world_model_flags: 0,
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
            world_model_flags: 0,
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
            world_model_flags: 0,
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
            world_model_flags: 0,
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
            world_model_flags: q2_shared::constants::EF_ROTATE,
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
            world_model_flags: q2_shared::constants::EF_ROTATE,
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
            world_model_flags: q2_shared::constants::EF_ROTATE,
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
            world_model_flags: q2_shared::constants::EF_ROTATE,
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
            world_model_flags: q2_shared::constants::EF_ROTATE,
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
            world_model_flags: q2_shared::constants::EF_ROTATE,
            icon: "i_fixme",
            pickup_name: "Ancient Head",
            quantity: 60,
            ammo: "",
            flags: ItemFlags::empty(),
            tag: 0,
            armor_info: None,
            pickup: Some(pickup_ancient_head),
            use_fn: None,
            drop_fn: None,
        },
        // [29] Adrenaline
        ItemDef {
            classname: "item_adrenaline",
            pickup_sound: "items/pkup.wav",
            world_model: "models/items/adrenal/tris.md2",
            world_model_flags: q2_shared::constants::EF_ROTATE,
            icon: "p_adrenaline",
            pickup_name: "Adrenaline",
            quantity: 60,
            ammo: "",
            flags: ItemFlags::empty(),
            tag: 0,
            armor_info: None,
            pickup: Some(pickup_adrenaline),
            use_fn: None,
            drop_fn: None,
        },
        // [30] Bandolier
        ItemDef {
            classname: "item_bandolier",
            pickup_sound: "items/pkup.wav",
            world_model: "models/items/band/tris.md2",
            world_model_flags: q2_shared::constants::EF_ROTATE,
            icon: "p_bandolier",
            pickup_name: "Bandolier",
            quantity: 60,
            ammo: "",
            flags: ItemFlags::empty(),
            tag: 0,
            armor_info: None,
            pickup: Some(pickup_bandolier),
            use_fn: None,
            drop_fn: None,
        },
        // [31] Ammo Pack
        ItemDef {
            classname: "item_pack",
            pickup_sound: "items/pkup.wav",
            world_model: "models/items/pack/tris.md2",
            world_model_flags: q2_shared::constants::EF_ROTATE,
            icon: "i_pack",
            pickup_name: "Ammo Pack",
            quantity: 180,
            ammo: "",
            flags: ItemFlags::empty(),
            tag: 0,
            armor_info: None,
            pickup: Some(pickup_pack),
            use_fn: None,
            drop_fn: None,
        },
        // [32-40] Keys — 9 key items
        ItemDef {
            classname: "key_data_cd",
            pickup_sound: "items/pkup.wav",
            world_model: "models/items/keys/data_cd/tris.md2",
            world_model_flags: q2_shared::constants::EF_ROTATE,
            icon: "k_datacd",
            pickup_name: "Data CD",
            quantity: 0,
            ammo: "",
            flags: ItemFlags::STAY_COOP | ItemFlags::KEY,
            tag: 0,
            armor_info: None,
            pickup: Some(pickup_powerup),
            use_fn: None,
            drop_fn: None,
        },
        ItemDef {
            classname: "key_power_cube",
            pickup_sound: "items/pkup.wav",
            world_model: "models/items/keys/power/tris.md2",
            world_model_flags: q2_shared::constants::EF_ROTATE,
            icon: "k_powercube",
            pickup_name: "Power Cube",
            quantity: 0,
            ammo: "",
            flags: ItemFlags::STAY_COOP | ItemFlags::KEY,
            tag: 0,
            armor_info: None,
            pickup: Some(pickup_powerup),
            use_fn: None,
            drop_fn: None,
        },
        ItemDef {
            classname: "key_pyramid",
            pickup_sound: "items/pkup.wav",
            world_model: "models/items/keys/pyramid/tris.md2",
            world_model_flags: q2_shared::constants::EF_ROTATE,
            icon: "k_pyramid",
            pickup_name: "Pyramid Key",
            quantity: 0,
            ammo: "",
            flags: ItemFlags::STAY_COOP | ItemFlags::KEY,
            tag: 0,
            armor_info: None,
            pickup: Some(pickup_powerup),
            use_fn: None,
            drop_fn: None,
        },
        ItemDef {
            classname: "key_data_spinner",
            pickup_sound: "items/pkup.wav",
            world_model: "models/items/keys/spinner/tris.md2",
            world_model_flags: q2_shared::constants::EF_ROTATE,
            icon: "k_dataspin",
            pickup_name: "Data Spinner",
            quantity: 0,
            ammo: "",
            flags: ItemFlags::STAY_COOP | ItemFlags::KEY,
            tag: 0,
            armor_info: None,
            pickup: Some(pickup_powerup),
            use_fn: None,
            drop_fn: None,
        },
        ItemDef {
            classname: "key_pass",
            pickup_sound: "items/pkup.wav",
            world_model: "models/items/keys/pass/tris.md2",
            world_model_flags: q2_shared::constants::EF_ROTATE,
            icon: "k_security",
            pickup_name: "Security Pass",
            quantity: 0,
            ammo: "",
            flags: ItemFlags::STAY_COOP | ItemFlags::KEY,
            tag: 0,
            armor_info: None,
            pickup: Some(pickup_powerup),
            use_fn: None,
            drop_fn: None,
        },
        ItemDef {
            classname: "key_blue_key",
            pickup_sound: "items/pkup.wav",
            world_model: "models/items/keys/key/tris.md2",
            world_model_flags: q2_shared::constants::EF_ROTATE,
            icon: "k_bluekey",
            pickup_name: "Blue Key",
            quantity: 0,
            ammo: "",
            flags: ItemFlags::STAY_COOP | ItemFlags::KEY,
            tag: 0,
            armor_info: None,
            pickup: Some(pickup_powerup),
            use_fn: None,
            drop_fn: None,
        },
        ItemDef {
            classname: "key_red_key",
            pickup_sound: "items/pkup.wav",
            world_model: "models/items/keys/red_key/tris.md2",
            world_model_flags: q2_shared::constants::EF_ROTATE,
            icon: "k_redkey",
            pickup_name: "Red Key",
            quantity: 0,
            ammo: "",
            flags: ItemFlags::STAY_COOP | ItemFlags::KEY,
            tag: 0,
            armor_info: None,
            pickup: Some(pickup_powerup),
            use_fn: None,
            drop_fn: None,
        },
        ItemDef {
            classname: "key_commander_head",
            pickup_sound: "items/pkup.wav",
            world_model: "models/monsters/commandr/head/tris.md2",
            world_model_flags: q2_shared::constants::EF_GIB,
            icon: "k_comhead",
            pickup_name: "Commander's Head",
            quantity: 0,
            ammo: "",
            flags: ItemFlags::STAY_COOP | ItemFlags::KEY,
            tag: 0,
            armor_info: None,
            pickup: Some(pickup_powerup),
            use_fn: None,
            drop_fn: None,
        },
        ItemDef {
            classname: "key_airstrike_target",
            pickup_sound: "items/pkup.wav",
            world_model: "models/items/keys/target/tris.md2",
            world_model_flags: q2_shared::constants::EF_ROTATE,
            icon: "i_airstrike",
            pickup_name: "Airstrike Marker",
            quantity: 0,
            ammo: "",
            flags: ItemFlags::STAY_COOP | ItemFlags::KEY,
            tag: 0,
            armor_info: None,
            pickup: Some(pickup_powerup),
            use_fn: None,
            drop_fn: None,
        },
        // [41] Health (generic — not spawned directly, used for health pickups)
        ItemDef {
            classname: "item_health",
            pickup_sound: "items/pkup.wav",
            world_model: "",
            world_model_flags: 0,
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

/// Pickup handler for armor items: a shard adds 2 to the worn armor (or
/// gives 2 jacket armor), new armor is worn as is, and otherwise the
/// better type is kept with some of the other salvaged into it.
/// C ref: `Pickup_Armor` (g_items.c:910-1011)
fn pickup_armor(world: &mut GameWorld, item_ent: EntityKey, player: EntityKey) -> bool {
    let Some(item_idx) = world.entities.get(item_ent).and_then(|e| e.game.item) else {
        return false;
    };
    let Some(item) = world.items.get(item_idx) else {
        return false;
    };
    let (tag, new_info) = (item.tag, item.armor_info);
    let old_index = world.armor_index(player);
    let jacket = world.find_item("item_armor_jacket");
    let old_info = old_index
        .and_then(|i| world.items.get(i))
        .and_then(|i| i.armor_info);
    let Some(inventory) = world.client_mut(player).map(|c| &mut c.pers.inventory) else {
        return false;
    };

    if tag == ARMOR_SHARD {
        match (old_index, jacket) {
            (Some(old), _) => inventory[old] += 2,
            (None, Some(jacket)) => inventory[jacket] = 2,
            (None, None) => return false,
        }
        return true;
    }
    let Some(new_info) = new_info else {
        return false;
    };
    let (Some(old), Some(old_info)) = (old_index, old_info) else {
        inventory[item_idx] = new_info.base_count;
        return true;
    };

    if new_info.normal_protection > old_info.normal_protection {
        let salvage = old_info.normal_protection / new_info.normal_protection;
        let salvage_count = (salvage * inventory[old] as f32) as i32;
        let new_count = (new_info.base_count + salvage_count).min(new_info.max_count);
        inventory[old] = 0;
        inventory[item_idx] = new_count;
    } else {
        let salvage = new_info.normal_protection / old_info.normal_protection;
        let salvage_count = (salvage * new_info.base_count as f32) as i32;
        let new_count = (inventory[old] + salvage_count).min(old_info.max_count);
        if inventory[old] >= new_count {
            return false; // already maxed out
        }
        inventory[old] = new_count;
    }
    true
}

/// Pickup handler for ammo items.
/// C ref: `Pickup_Ammo` (g_items.c:714-764)
fn pickup_ammo(world: &mut GameWorld, item_ent: EntityKey, player: EntityKey) -> bool {
    let item_idx = world.entities.get(item_ent).and_then(|e| e.game.item);
    let item_idx = match item_idx {
        Some(i) => i,
        None => return false,
    };

    let (quantity, tag) = match world.items.get(item_idx) {
        Some(item) => (item.quantity, item.tag),
        None => return false,
    };

    // C ITEM_INDEX: ammo counts live at the ammo item's own index.
    let inv_slot = item_idx;
    let Some(pers) = world.client(player).map(|c| &c.pers) else {
        return false;
    };
    let max = pers.ammo_max.for_tag(tag);
    if max == 0 {
        return false;
    }
    let current = pers.inventory[inv_slot];

    if current >= max {
        return false; // Already at max.
    }

    if let Some(client) = world.client_mut(player) {
        client.pers.inventory[inv_slot] = (current + quantity).min(max);
    }

    true
}

/// Raise the ammo caps to `caps`, then add one pickup's worth of each of
/// `ammo` (pickup names, as C `FindItem`), clamped to the new caps. The shared body of C
/// `Pickup_Bandolier` and `Pickup_Pack`.
fn pickup_ammo_carrier(
    world: &mut GameWorld,
    player: EntityKey,
    caps: AmmoMax,
    ammo: &[&str],
) -> bool {
    let gifts: Vec<(usize, i32, i32)> = ammo
        .iter()
        .filter_map(|&name| {
            let index = find_item_index_by_name(&world.items, name)?;
            let item = world.items.get(index)?;
            Some((index, item.quantity, item.tag))
        })
        .collect();
    let Some(pers) = world.client_mut(player).map(|c| &mut c.pers) else {
        return false;
    };
    pers.ammo_max.raise_to(caps);
    for (index, quantity, tag) in gifts {
        let max = pers.ammo_max.for_tag(tag);
        pers.inventory[index] = (pers.inventory[index] + quantity).min(max);
    }
    true
}

/// C ref: `Pickup_Bandolier` (g_items.c).
fn pickup_bandolier(world: &mut GameWorld, _item_ent: EntityKey, player: EntityKey) -> bool {
    pickup_ammo_carrier(world, player, AMMO_MAX_BANDOLIER, &["Bullets", "Shells"])
}

/// C ref: `Pickup_Pack` (g_items.c).
fn pickup_pack(world: &mut GameWorld, _item_ent: EntityKey, player: EntityKey) -> bool {
    pickup_ammo_carrier(
        world,
        player,
        AMMO_MAX_PACK,
        &["Bullets", "Shells", "Cells", "Grenades", "Rockets", "Slugs"],
    )
}

/// Pickup handler for weapon items.
/// C ref: `Pickup_Weapon` (player/weapon.c:268-336)
fn pickup_weapon(world: &mut GameWorld, item_ent: EntityKey, player: EntityKey) -> bool {
    let item_idx = world.entities.get(item_ent).and_then(|e| e.game.item);
    let item_idx = match item_idx {
        Some(i) => i,
        None => return false,
    };

    // Add weapon to inventory.
    if let Some(client) = world.client_mut(player) {
        client.pers.inventory[item_idx] += 1;
    }

    // Give some starting ammo.
    if let Some(item) = world.items.get(item_idx) {
        if !item.ammo.is_empty() {
            if let Some(ammo_idx) = find_item_index_by_name(&world.items, item.ammo) {
                if let Some(ammo_item) = world.items.get(ammo_idx) {
                    let slot = world.entities.get(player).and_then(|e| e.client);
                    if let Some(client) = slot.and_then(|s| world.clients.get_mut(s)) {
                        let max = client.pers.ammo_max.for_tag(ammo_item.tag);
                        let current = client.pers.inventory[ammo_idx];
                        client.pers.inventory[ammo_idx] = (current + ammo_item.quantity).min(max);
                    }
                }
            }
        }
    }

    true
}

/// Pickup handler for health items. Stimpacks and mega health
/// (`HEALTH_IGNORE_MAX`) go over max health; mega health (`HEALTH_TIMED`)
/// then rots back down while the item waits, unseen.
/// C ref: `Pickup_Health` (g_items.c:831-874)
fn pickup_health(world: &mut GameWorld, item_ent: EntityKey, player: EntityKey) -> bool {
    let Some((amount, style)) = world
        .entities
        .get(item_ent)
        .map(|e| (e.game.count, e.game.style))
    else {
        return false;
    };
    let Some(ent) = world.entities.get_mut(player) else {
        return false;
    };
    let ignore_max = style & HEALTH_IGNORE_MAX != 0;
    if !ignore_max && ent.game.health >= ent.game.max_health {
        return false;
    }
    ent.game.health += amount;
    if !ignore_max {
        ent.game.health = ent.game.health.min(ent.game.max_health);
    }

    if style & HEALTH_TIMED != 0 {
        let time = world.level.time;
        if let Some(item) = world.entities.get_mut(item_ent) {
            item.think = Some(megahealth_think);
            item.game.nextthink = time + 5.0;
            item.owner = Some(player);
            item.game.flags |= EntityFlags::RESPAWN;
            item.svflags |= q2_shared::constants::SVF_NOCLIENT;
            item.solid = Solid::Not;
        }
        world.link_entity(item_ent);
    }
    true
}

/// Drain the mega health bonus one point a second, then go away.
/// C ref: `MegaHealth_think` (single player: no respawn).
pub fn megahealth_think(world: &mut GameWorld, key: EntityKey) {
    let time = world.level.time;
    let owner = world.entities.get(key).and_then(|e| e.owner);
    if let Some(o) = owner.and_then(|o| world.entities.get_mut(o)) {
        if o.game.health > o.game.max_health {
            o.game.health -= 1;
            if let Some(ent) = world.entities.get_mut(key) {
                ent.game.nextthink = time + 1.0;
            }
            return;
        }
    }
    world.free_entity(key);
}

/// Adrenaline: one more max health, healed to it. C ref:
/// `Pickup_Adrenaline` (single player).
fn pickup_adrenaline(world: &mut GameWorld, _item_ent: EntityKey, player: EntityKey) -> bool {
    let Some(ent) = world.entities.get_mut(player) else {
        return false;
    };
    ent.game.max_health += 1;
    ent.game.health = ent.game.health.max(ent.game.max_health);
    true
}

/// Ancient Head: two more max health. C ref: `Pickup_AncientHead`.
fn pickup_ancient_head(world: &mut GameWorld, _item_ent: EntityKey, player: EntityKey) -> bool {
    let Some(ent) = world.entities.get_mut(player) else {
        return false;
    };
    ent.game.max_health += 2;
    true
}

/// Pickup handler for powerups and keys: one more in the inventory.
/// C ref: `Pickup_Powerup` and `Pickup_Key` (single player).
fn pickup_powerup(world: &mut GameWorld, item_ent: EntityKey, player: EntityKey) -> bool {
    let item_idx = world.entities.get(item_ent).and_then(|e| e.game.item);
    let item_idx = match item_idx {
        Some(i) => i,
        None => return false,
    };

    // Add to inventory.
    if let Some(client) = world.client_mut(player) {
        client.pers.inventory[item_idx] += 1;
    }

    true
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Find an item by classname. Returns the index in the item list.
pub fn find_item_index(items: &[ItemDef], classname: &str) -> Option<usize> {
    items.iter().position(|i| i.classname == classname)
}

/// Find an item by pickup_name. Returns the index in the item list.
pub fn find_item_index_by_name(items: &[ItemDef], name: &str) -> Option<usize> {
    items
        .iter()
        .position(|i| i.pickup_name.eq_ignore_ascii_case(name))
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

    /// Find an item index by pickup name (C `FindItem`).
    pub fn find_item_by_name(&self, name: &str) -> Option<usize> {
        find_item_index_by_name(&self.items, name)
    }
}

// ---------------------------------------------------------------------------
// Spawning and touching items in the world (g_items.c)
// ---------------------------------------------------------------------------

/// spawnflags (C g_items.c).
const ITEM_TRIGGER_SPAWN: i32 = 0x0000_0001;
const ITEM_NO_TOUCH: i32 = 0x0000_0002;
/// Set on an item thrown by a monster or player rather than placed by the
/// map (C `DROPPED_ITEM`).
pub const DROPPED_ITEM: i32 = 0x0001_0000;
const ITEM_TARGETS_USED: i32 = 0x0004_0000;
/// C `EV_ITEM_RESPAWN` (shared.h `entity_event_t`).
const EV_ITEM_RESPAWN: i32 = 1;

/// C's health spawns (SP_item_health*): the "Health" item with a model and
/// amount of its own.
pub fn health_spawn(classname: &str) -> Option<(&'static str, i32)> {
    Some(match classname {
        "item_health" => ("models/items/healing/medium/tris.md2", 10),
        "item_health_small" => ("models/items/healing/stimpack/tris.md2", 2),
        "item_health_large" => ("models/items/healing/large/tris.md2", 25),
        "item_health_mega" => ("models/items/mega_h/tris.md2", 100),
        _ => return None,
    })
}

impl GameWorld {
    /// Make `key` a world item that drops to the floor once the other
    /// solids are in. C ref: `SpawnItem`.
    pub fn spawn_item(&mut self, key: EntityKey, item: usize) {
        let flags = self.items.get(item).map_or(0, |it| it.world_model_flags);
        let time = self.level.time;
        let Some(ent) = self.entities.get_mut(key) else {
            return;
        };
        // C SP_item_health_small / SP_item_health_mega.
        match ent.game.classname.as_str() {
            "item_health_small" => ent.game.style = HEALTH_IGNORE_MAX,
            "item_health_mega" => ent.game.style = HEALTH_IGNORE_MAX | HEALTH_TIMED,
            _ => {}
        }
        if ent.game.spawnflags != 0 && ent.game.classname != "key_power_cube" {
            tracing::debug!(classname = %ent.game.classname, "item has invalid spawnflags set");
            ent.game.spawnflags = 0;
        }
        ent.game.item = Some(item);
        ent.game.nextthink = time + 2.0 * FRAMETIME; // items start after other solids
        ent.think = Some(droptofloor);
        ent.state.effects = flags;
        ent.state.renderfx = q2_shared::constants::RF_GLOW;
    }
}

/// C ref: `droptofloor`.
pub fn droptofloor(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.mins = Vec3f::splat(-15.0);
    ent.maxs = Vec3f::splat(15.0);
    let model = if ent.game.model.is_empty() {
        let world_model = ent.game.item.and_then(|i| world.items.get(i));
        world_model.map_or("", |it| it.world_model).to_string()
    } else {
        ent.game.model.clone()
    };
    world.set_model(key, &model);

    let Some(number) = world.number_of(key) else {
        return;
    };
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.solid = Solid::Trigger;
    ent.game.movetype = MoveType::Toss;
    ent.touch = Some(touch_item);
    let (origin, mins, maxs) = (ent.state.origin, ent.mins, ent.maxs);
    let dest = origin + Vec3f::new(0.0, 0.0, -128.0);
    let tr = world.gi.trace(
        origin,
        mins,
        maxs,
        dest,
        Some(number),
        q2_shared::constants::MASK_SOLID,
    );
    if tr.startsolid {
        tracing::debug!(?origin, "droptofloor: startsolid");
        world.free_entity(key);
        return;
    }
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.state.origin = tr.endpos;
    // A teamed item hides; the team master shows one member at random.
    if !ent.game.team.is_empty() {
        ent.game.flags.remove(EntityFlags::TEAMSLAVE);
        ent.game.chain = ent.game.teamchain.take();
        ent.svflags |= q2_shared::constants::SVF_NOCLIENT;
        ent.solid = Solid::Not;
        if ent.game.teammaster == Some(key) {
            ent.game.nextthink = world.level.time + FRAMETIME;
            ent.think = Some(do_respawn);
        }
    }
    if ent.game.spawnflags & ITEM_NO_TOUCH != 0 {
        ent.solid = Solid::Bbox;
        ent.touch = None;
        ent.state.effects &= !q2_shared::constants::EF_ROTATE;
        ent.state.renderfx &= !q2_shared::constants::RF_GLOW;
    }
    if ent.game.spawnflags & ITEM_TRIGGER_SPAWN != 0 {
        ent.svflags |= q2_shared::constants::SVF_NOCLIENT;
        ent.solid = Solid::Not;
        ent.use_fn = Some(use_item);
    }
    world.link_entity(key);
}

/// Show the item again, or for a teamed item a random member of the
/// master's `chain`. C ref: `DoRespawn`.
pub fn do_respawn(world: &mut GameWorld, key: EntityKey) {
    let mut key = key;
    let team = world.entities.get(key).filter(|e| !e.game.team.is_empty());
    if let Some(master) = team.and_then(|e| e.game.teammaster) {
        let chain =
            |world: &GameWorld, k: EntityKey| world.entities.get(k).and_then(|e| e.game.chain);
        let mut count = 0u32;
        let mut ent = Some(master);
        while let Some(k) = ent {
            ent = chain(world, k);
            count += 1;
        }
        let choice = if count != 0 {
            world.random_u32() % count
        } else {
            0
        };
        key = master;
        for _ in 0..choice {
            match chain(world, key) {
                Some(next) => key = next,
                None => break,
            }
        }
    }
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.svflags &= !q2_shared::constants::SVF_NOCLIENT;
    ent.solid = Solid::Trigger;
    world.link_entity(key);
    if let Some(ent) = world.entities.get_mut(key) {
        ent.state.event = EV_ITEM_RESPAWN;
    }
}

/// A trigger-spawned item appears. C ref: `Use_Item`.
pub fn use_item(world: &mut GameWorld, key: EntityKey, _other: EntityKey, _activator: EntityKey) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.svflags &= !q2_shared::constants::SVF_NOCLIENT;
    ent.use_fn = None;
    if ent.game.spawnflags & ITEM_NO_TOUCH != 0 {
        ent.solid = Solid::Bbox;
        ent.touch = None;
    } else {
        ent.solid = Solid::Trigger;
        ent.touch = Some(touch_item);
    }
    world.link_entity(key);
}

/// C ref: `Touch_Item` (single player: no deathmatch instant use, no coop
/// stay; pickup sounds and the screen flash are not ported).
pub fn touch_item(
    world: &mut GameWorld,
    key: EntityKey,
    other: EntityKey,
    _plane: Option<&Plane>,
    _surf: Option<&Surface>,
) {
    let Some(o) = world.entities.get(other) else {
        return;
    };
    if o.client.is_none() || o.game.health < 1 {
        return; // dead people can't pick up
    }
    let pickup = world
        .entities
        .get(key)
        .and_then(|e| e.game.item)
        .and_then(|i| world.items.get(i))
        .and_then(|it| it.pickup);
    let Some(pickup) = pickup else {
        return; // not a grabbable item
    };
    let taken = pickup(world, key, other);
    if taken {
        // Show the icon and name on the status bar.
        let item = world.entities.get(key).and_then(|e| e.game.item);
        let icon = item
            .and_then(|i| world.items.get(i))
            .map_or(0, |it| world.gi.image_index(it.icon));
        let time = world.level.time;
        if let Some(client) = world.client_mut(other) {
            client.ps.stats[q2_shared::constants::STAT_PICKUP_ICON] = icon as i16;
            if let Some(i) = item {
                client.ps.stats[q2_shared::constants::STAT_PICKUP_STRING] =
                    (q2_shared::constants::CS_ITEMS + i) as i16;
            }
            client.pickup_msg_time = time + 3.0;
        }
    }

    let targets_used = world
        .entities
        .get(key)
        .is_none_or(|e| e.game.spawnflags & ITEM_TARGETS_USED != 0);
    if !targets_used {
        world.use_targets(key, other);
        if let Some(e) = world.entities.get_mut(key) {
            e.game.spawnflags |= ITEM_TARGETS_USED;
        }
    }
    if !taken {
        return;
    }
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    if ent.game.flags.contains(EntityFlags::RESPAWN) {
        ent.game.flags.remove(EntityFlags::RESPAWN);
    } else {
        world.free_entity(key);
    }
}

impl GameWorld {
    /// Throw `item` out of `owner`: from its origin along its facing, or
    /// for a client from 24 units ahead of its view, clipped by solids.
    /// The owner cannot pick it up for the first second. C ref: `Drop_Item`
    /// (the client view is `ps.viewangles`, which ClientThink keeps equal
    /// to C `v_angle` while alive).
    pub fn drop_item(&mut self, owner: EntityKey, item: usize) -> Option<EntityKey> {
        let (classname, world_model, effects) = {
            let it = self.items.get(item)?;
            (it.classname, it.world_model, it.world_model_flags)
        };
        let owner_ent = self.entities.get(owner)?;
        let origin = owner_ent.state.origin;
        let view = self.client(owner).map(|c| c.ps.viewangles);
        let angles = owner_ent.state.angles;
        let owner_number = self.number_of(owner);

        let dropped = self.spawn()?;
        // C draws the sign first, then the size of the turn.
        let left = self.frandk() > 0.5;
        let turn = self.frandk() * 45.0;
        let yaw = if left { turn } else { -turn };
        let ent = self.entities.get_mut(dropped)?;
        ent.game.classname = classname.to_string();
        ent.game.item = Some(item);
        ent.game.spawnflags = DROPPED_ITEM;
        ent.state.effects = effects;
        ent.state.renderfx = q2_shared::constants::RF_GLOW;
        ent.state.angles.y += yaw;
        ent.mins = Vec3f::splat(-16.0);
        ent.maxs = Vec3f::splat(16.0);
        self.set_model(dropped, world_model);

        let forward = match view {
            Some(v_angle) => {
                let (forward, _, _) = crate::physics::angle_vectors(v_angle);
                // C G_ProjectSource with offset (24, 0, -16).
                let start = origin + forward * 24.0 + Vec3f::new(0.0, 0.0, -16.0);
                let tr = self.gi.trace(
                    origin,
                    Vec3f::splat(-16.0),
                    Vec3f::splat(16.0),
                    start,
                    owner_number,
                    q2_shared::constants::CONTENTS_SOLID,
                );
                self.entities.get_mut(dropped)?.state.origin = tr.endpos;
                forward
            }
            None => {
                self.entities.get_mut(dropped)?.state.origin = origin;
                crate::physics::angle_vectors(angles).0
            }
        };

        let time = self.level.time;
        let ent = self.entities.get_mut(dropped)?;
        ent.solid = Solid::Trigger;
        ent.game.movetype = MoveType::Toss;
        ent.touch = Some(drop_temp_touch);
        ent.owner = Some(owner);
        ent.velocity = forward * 100.0;
        ent.velocity.z = 300.0;
        ent.think = Some(drop_make_touchable);
        ent.game.nextthink = time + 1.0;
        self.link_entity(dropped);
        Some(dropped)
    }

    /// C `frandk()`: uniform in [0, 1].
    fn frandk(&mut self) -> f32 {
        (self.random_u32() & 0x7fff) as f32 / 0x7fff as f32
    }
}

/// A fresh drop is anyone's but its owner's. C ref: `drop_temp_touch`.
pub fn drop_temp_touch(
    world: &mut GameWorld,
    key: EntityKey,
    other: EntityKey,
    plane: Option<&Plane>,
    surf: Option<&Surface>,
) {
    if world.entities.get(key).and_then(|e| e.owner) == Some(other) {
        return;
    }
    touch_item(world, key, other, plane, surf);
}

/// A second after the throw, the owner may pick its drop back up; in
/// deathmatch the drop goes away 29 s later. C ref: `drop_make_touchable`.
pub fn drop_make_touchable(world: &mut GameWorld, key: EntityKey) {
    let deathmatch = world.game.deathmatch;
    let time = world.level.time;
    if let Some(ent) = world.entities.get_mut(key) {
        ent.touch = Some(touch_item);
        // Deathmatch removes drops nobody picked up.
        if deathmatch {
            ent.game.nextthink = time + 29.0;
            ent.think = Some(free_drop);
        }
    }
}

/// C `G_FreeEdict` as a dropped item's think.
pub fn free_drop(world: &mut GameWorld, key: EntityKey) {
    world.free_entity(key);
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::test_world;

    use crate::entity::TouchFn;
    use q2_shared::constants::{EF_ROTATE, RF_GLOW};
    use q2_shared::types::{Solid, Trace};

    fn spawned(entities: &str) -> (GameWorld, std::rc::Rc<crate::world::MockGameImport>) {
        let (mut world, gi) = crate::world::test_world_recording();
        world.init_game();
        world.spawn_entities(
            "t",
            &format!("{{ \"classname\" \"worldspawn\" }}\n{entities}"),
            "",
        );
        (world, gi)
    }

    fn player(world: &mut GameWorld, health: i32) -> EntityKey {
        let key = world.spawn_test_client(crate::entity::ClientData::default());
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.health = health;
        ent.game.max_health = 100;
        key
    }

    #[test]
    fn map_item_drops_to_the_floor_on_its_second_frame_with_its_world_model() {
        let (mut world, gi) =
            spawned("{ \"classname\" \"weapon_shotgun\" \"origin\" \"0 0 100\" }");
        let key = world
            .find_by_classname(None, "weapon_shotgun")
            .expect("items spawn");
        world.run_frame();
        assert_eq!(
            world.entities.get(key).unwrap().state.modelindex,
            0,
            "not yet"
        );
        *gi.next_trace.borrow_mut() = Some(Trace {
            fraction: 0.5,
            endpos: Vec3f::new(0.0, 0.0, 15.0),
            ..Default::default()
        });
        world.run_frame();
        let ent = world.entities.get(key).unwrap();
        let index = ent.state.modelindex as usize;
        assert_eq!(
            gi.models.borrow()[index - 1],
            "models/weapons/g_shotg/tris.md2"
        );
        assert_eq!(ent.state.origin, Vec3f::new(0.0, 0.0, 15.0));
        assert_eq!(ent.solid, Solid::Trigger);
        assert_eq!(
            (ent.mins, ent.maxs),
            (Vec3f::splat(-15.0), Vec3f::splat(15.0))
        );
        assert_eq!(ent.state.effects & EF_ROTATE, EF_ROTATE);
        assert_eq!(ent.state.renderfx & RF_GLOW, RF_GLOW);
    }

    #[test]
    fn world_model_flags_match_the_c_item_table() {
        use q2_shared::constants::{EF_GIB, EF_ROTATE};
        let still = [
            "",
            "item_health",
            "weapon_blaster",
            "ammo_grenades",
            "ammo_shells",
            "ammo_bullets",
            "ammo_cells",
            "ammo_rockets",
            "ammo_slugs",
        ];
        for item in build_item_list() {
            // C names no Health classname; its entry has no flags.
            let expected = match item.classname {
                c if still.contains(&c) => 0,
                "key_commander_head" => EF_GIB,
                _ => EF_ROTATE,
            };
            assert_eq!(item.world_model_flags, expected, "{}", item.pickup_name);
        }
    }

    #[test]
    fn teamed_items_hide_and_the_master_respawns_one_of_them() {
        // C droptofloor: a teamed item leaves FL_TEAMSLAVE, moves
        // teamchain to chain and hides; the master's DoRespawn shows one.
        let (mut world, _gi) = spawned(
            r#"{ "classname" "weapon_shotgun" "team" "t" "origin" "0 0 0" }
               { "classname" "weapon_supershotgun" "team" "t" "origin" "64 0 0" }"#,
        );
        let master = world.find_by_classname(None, "weapon_shotgun").unwrap();
        let slave = world
            .find_by_classname(None, "weapon_supershotgun")
            .unwrap();
        assert!(world
            .entities
            .get(slave)
            .unwrap()
            .game
            .flags
            .contains(EntityFlags::TEAMSLAVE));
        world.run_frame();
        world.run_frame(); // droptofloor
        for key in [master, slave] {
            let e = world.entities.get(key).unwrap();
            assert!(!e.game.flags.contains(EntityFlags::TEAMSLAVE));
            assert_eq!(e.game.teamchain, None);
            assert_eq!(
                (e.solid, e.svflags & q2_shared::constants::SVF_NOCLIENT != 0),
                (Solid::Not, true)
            );
        }
        assert_eq!(world.entities.get(master).unwrap().game.chain, Some(slave));
        world.run_frame(); // the master's DoRespawn
        let shown: Vec<_> = [master, slave]
            .into_iter()
            .filter(|&k| world.entities.get(k).unwrap().solid == Solid::Trigger)
            .collect();
        assert_eq!(shown.len(), 1);
        let e = world.entities.get(shown[0]).unwrap();
        assert_eq!(e.svflags & q2_shared::constants::SVF_NOCLIENT, 0);
        assert_eq!(e.state.event, EV_ITEM_RESPAWN);
    }

    #[test]
    fn item_stuck_in_solid_is_removed() {
        let (mut world, gi) = spawned("{ \"classname\" \"ammo_shells\" }");
        let key = world.find_by_classname(None, "ammo_shells").unwrap();
        world.run_frame();
        *gi.next_trace.borrow_mut() = Some(Trace {
            startsolid: true,
            ..Default::default()
        });
        world.run_frame();
        assert!(!world.entities.get(key).is_some_and(|e| e.in_use));
    }

    #[test]
    fn health_items_spawn_with_their_model_and_amount() {
        let (mut world, gi) = spawned("{ \"classname\" \"item_health_large\" }");
        let key = world.find_by_classname(None, "item_health_large").unwrap();
        run_frames(&mut world, 2);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.count, 25);
        let index = ent.state.modelindex as usize;
        assert_eq!(
            gi.models.borrow()[index - 1],
            "models/items/healing/large/tris.md2"
        );
        assert_eq!(ent.state.effects & EF_ROTATE, 0, "health does not spin");
    }

    fn run_frames(world: &mut GameWorld, n: usize) {
        for _ in 0..n {
            world.run_frame();
        }
    }

    #[test]
    fn touching_an_item_picks_it_up_fires_its_targets_and_removes_it() {
        let (mut world, _gi) = spawned(concat!(
            "{ \"classname\" \"weapon_shotgun\" \"target\" \"t\" }\n",
            "{ \"classname\" \"info_notnull\" \"targetname\" \"t\" }",
        ));
        let item = world.find_by_classname(None, "weapon_shotgun").unwrap();
        run_frames(&mut world, 2);
        let p = player(&mut world, 100);
        let touch = world.entities.get(item).unwrap().touch.expect("Touch_Item");
        touch(&mut world, item, p, None, None);
        let index = world.find_item("weapon_shotgun").unwrap();
        let client = world.client(p).unwrap();
        assert_eq!(client.pers.inventory[index], 1);
        assert!(!world.entities.get(item).is_some_and(|e| e.in_use));
    }

    #[test]
    fn dead_players_cannot_pick_items_up() {
        let (mut world, _gi) = spawned("{ \"classname\" \"weapon_shotgun\" }");
        let item = world.find_by_classname(None, "weapon_shotgun").unwrap();
        run_frames(&mut world, 2);
        let p = player(&mut world, 0);
        touch_item(&mut world, item, p, None, None);
        assert!(world.entities.get(item).is_some_and(|e| e.in_use));
    }

    fn assert_near(actual: Vec3f, expected: Vec3f) {
        assert!(
            (actual - expected).length() < 1e-3,
            "{actual:?} != {expected:?}"
        );
    }

    #[test]
    fn monster_drop_starts_at_its_origin_thrown_forward_and_up() {
        let (mut world, gi) = crate::world::test_world_recording();
        world.init_items();
        world.level.time = 5.0;
        let monster = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(monster).unwrap();
            ent.state.origin = Vec3f::new(10.0, 20.0, 30.0);
            ent.state.angles = Vec3f::new(0.0, 90.0, 0.0);
        }
        let shotgun = world.find_item("weapon_shotgun").unwrap();

        let dropped = world.drop_item(monster, shotgun).expect("Drop_Item");

        let ent = world.entities.get(dropped).unwrap();
        assert_eq!(ent.game.classname, "weapon_shotgun");
        assert_eq!(ent.game.item, Some(shotgun));
        assert_eq!(ent.game.spawnflags, DROPPED_ITEM);
        assert_eq!(ent.owner, Some(monster));
        assert_eq!(ent.state.origin, Vec3f::new(10.0, 20.0, 30.0));
        assert_near(ent.velocity, Vec3f::new(0.0, 100.0, 300.0));
        assert_eq!(
            (ent.mins, ent.maxs),
            (Vec3f::splat(-16.0), Vec3f::splat(16.0))
        );
        assert_eq!(ent.solid, Solid::Trigger);
        assert_eq!(ent.game.movetype, MoveType::Toss);
        assert_eq!(ent.state.effects, EF_ROTATE);
        assert_eq!(ent.state.renderfx, RF_GLOW);
        assert!(ent.state.angles.y.abs() <= 45.0, "random yaw within 45");
        let index = ent.state.modelindex as usize;
        assert_eq!(
            gi.models.borrow()[index - 1],
            "models/weapons/g_shotg/tris.md2"
        );
        assert!(ent.linkcount > 0, "linked");
        assert!(std::ptr::fn_addr_eq(
            ent.touch.unwrap(),
            drop_temp_touch as TouchFn
        ));
        assert!(std::ptr::fn_addr_eq(
            ent.think.unwrap(),
            drop_make_touchable as crate::entity::ThinkFn
        ));
        assert_eq!(ent.game.nextthink, 6.0);
    }

    #[test]
    fn player_drop_is_thrown_from_in_front_of_its_view_clipped_by_solids() {
        let (mut world, gi) = crate::world::test_world_recording();
        world.init_items();
        let p = player(&mut world, 100);
        {
            let ent = world.entities.get_mut(p).unwrap();
            ent.state.origin = Vec3f::new(100.0, 0.0, 50.0);
            // The body faces away; the drop follows the view.
            ent.state.angles = Vec3f::new(0.0, 180.0, 0.0);
        }
        world.client_mut(p).unwrap().ps.viewangles = Vec3f::ZERO;
        *gi.next_trace.borrow_mut() = Some(Trace {
            fraction: 0.5,
            endpos: Vec3f::new(112.0, 0.0, 42.0),
            ..Default::default()
        });
        let shells = world.find_item("ammo_shells").unwrap();

        let dropped = world.drop_item(p, shells).unwrap();

        let ent = world.entities.get(dropped).unwrap();
        assert_eq!(ent.state.origin, Vec3f::new(112.0, 0.0, 42.0));
        assert_near(ent.velocity, Vec3f::new(100.0, 0.0, 300.0));
        assert_eq!(ent.state.effects, 0, "ammo does not spin");
        let number = world.number_of(p);
        assert_eq!(gi.trace_pass_ents.borrow().last(), Some(&number));
        assert_eq!(
            gi.trace_masks.borrow().last(),
            Some(&q2_shared::constants::CONTENTS_SOLID)
        );
    }

    #[test]
    fn player_drop_traces_to_a_point_24_ahead_and_16_down() {
        let mut world = test_world();
        world.init_items();
        let p = player(&mut world, 100);
        {
            let ent = world.entities.get_mut(p).unwrap();
            ent.state.origin = Vec3f::new(100.0, 0.0, 50.0);
        }
        world.client_mut(p).unwrap().ps.viewangles = Vec3f::new(0.0, 90.0, 0.0);
        let shells = world.find_item("ammo_shells").unwrap();

        // The mock trace reaches its end point unobstructed.
        let dropped = world.drop_item(p, shells).unwrap();

        let origin = world.entities.get(dropped).unwrap().state.origin;
        assert_near(origin, Vec3f::new(100.0, 24.0, 34.0));
    }

    #[test]
    fn owner_cannot_pick_up_its_fresh_drop() {
        let mut world = test_world();
        world.init_items();
        let owner = player(&mut world, 100);
        let shotgun = world.find_item("weapon_shotgun").unwrap();
        let dropped = world.drop_item(owner, shotgun).unwrap();

        drop_temp_touch(&mut world, dropped, owner, None, None);

        let client = world.client(owner).unwrap();
        assert_eq!(client.pers.inventory[shotgun], 0);
        assert!(world.entities.get(dropped).is_some_and(|e| e.in_use));
    }

    #[test]
    fn another_player_can_pick_up_a_fresh_drop() {
        let mut world = test_world();
        world.set_max_clients(2);
        world.init_items();
        let owner = player(&mut world, 100);
        let other = player(&mut world, 100);
        let shotgun = world.find_item("weapon_shotgun").unwrap();
        let dropped = world.drop_item(owner, shotgun).unwrap();

        drop_temp_touch(&mut world, dropped, other, None, None);

        let client = world.client(other).unwrap();
        assert_eq!(client.pers.inventory[shotgun], 1);
        assert!(!world.entities.get(dropped).is_some_and(|e| e.in_use));
    }

    #[test]
    fn drop_becomes_touchable_by_its_owner_after_a_second() {
        let mut world = test_world();
        world.init_items();
        let owner = player(&mut world, 100);
        let shotgun = world.find_item("weapon_shotgun").unwrap();
        let dropped = world.drop_item(owner, shotgun).unwrap();

        drop_make_touchable(&mut world, dropped);

        let touch = world.entities.get(dropped).unwrap().touch.unwrap();
        assert!(std::ptr::fn_addr_eq(touch, touch_item as TouchFn));
        touch(&mut world, dropped, owner, None, None);
        let client = world.client(owner).unwrap();
        assert_eq!(client.pers.inventory[shotgun], 1);
        assert!(!world.entities.get(dropped).is_some_and(|e| e.in_use));
    }

    #[test]
    fn deathmatch_drop_is_freed_29_seconds_after_it_becomes_touchable() {
        let mut world = test_world();
        world.game.deathmatch = true;
        world.init_items();
        let owner = player(&mut world, 100);
        let shotgun = world.find_item("weapon_shotgun").unwrap();
        let dropped = world.drop_item(owner, shotgun).unwrap();
        world.level.time = 5.0;

        drop_make_touchable(&mut world, dropped);

        let ent = world.entities.get(dropped).unwrap();
        assert_eq!(ent.game.nextthink, 34.0);
        let think = ent.think.unwrap();
        think(&mut world, dropped);
        assert!(!world.entities.get(dropped).is_some_and(|e| e.in_use));
    }

    #[test]
    fn single_player_drop_stays_once_touchable() {
        let mut world = test_world();
        world.init_items();
        let owner = player(&mut world, 100);
        let shotgun = world.find_item("weapon_shotgun").unwrap();
        let dropped = world.drop_item(owner, shotgun).unwrap();
        world.level.time = 5.0;
        world.entities.get_mut(dropped).unwrap().game.nextthink = 0.0; // G_RunThink

        drop_make_touchable(&mut world, dropped);

        assert_eq!(world.entities.get(dropped).unwrap().game.nextthink, 0.0);
        assert!(world.entities.get(dropped).unwrap().in_use);
    }

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

        let player = world.spawn_test_client(crate::entity::ClientData::default());

        let item_ent = world.spawn().unwrap();
        world.entities.get_mut(item_ent).unwrap().game.item = Some(18); // shells

        let result = pickup_ammo(&mut world, item_ent, player);
        assert!(result);

        let inv = world.client(player).unwrap().pers.inventory;
        assert_eq!(inv[18], 10); // C ITEM_INDEX: shells are item 18, quantity = 10
    }

    #[test]
    fn pickup_ammo_rejects_when_full() {
        let mut world = test_world();
        world.init_items();

        let mut client = crate::entity::ClientData::default();
        client.pers.inventory[18] = 100; // shells (item 18) already at max
        let player = world.spawn_test_client(client);

        let item_ent = world.spawn().unwrap();
        world.entities.get_mut(item_ent).unwrap().game.item = Some(18);

        let result = pickup_ammo(&mut world, item_ent, player);
        assert!(!result); // Should reject — already at max
    }

    /// A player and a map item `classname`, both in `world`.
    fn player_and_item(world: &mut GameWorld, classname: &str) -> (EntityKey, EntityKey) {
        world.init_items();
        let player = world.spawn_test_client(crate::entity::ClientData::default());
        let item = world.spawn().unwrap();
        world.entities.get_mut(item).unwrap().game.item = world.find_item(classname);
        (player, item)
    }

    fn pers(world: &GameWorld, player: EntityKey) -> &crate::entity::ClientPersistent {
        &world.client(player).unwrap().pers
    }

    #[test]
    fn ammo_pickup_caps_at_the_clients_own_maximum() {
        let mut world = test_world();
        let (player, item) = player_and_item(&mut world, "ammo_shells");
        let shells = world.find_item("ammo_shells").unwrap();
        {
            let client = world.client_mut(player).unwrap();
            client.pers.ammo_max.shells = 150;
            client.pers.inventory[shells] = 145;
        }
        assert!(pickup_ammo(&mut world, item, player));
        assert_eq!(pers(&world, player).inventory[shells], 150);
    }

    #[test]
    fn bandolier_raises_four_caps_and_gives_bullets_and_shells() {
        let mut world = test_world();
        let (player, item) = player_and_item(&mut world, "item_bandolier");
        let pickup = world.items[world.find_item("item_bandolier").unwrap()]
            .pickup
            .unwrap();
        assert!(pickup(&mut world, item, player));
        let pers = pers(&world, player);
        let m = pers.ammo_max;
        assert_eq!(
            (m.bullets, m.shells, m.rockets, m.grenades, m.cells, m.slugs),
            (250, 150, 50, 50, 250, 75)
        );
        assert_eq!(pers.inventory[world.find_item("ammo_bullets").unwrap()], 50);
        assert_eq!(pers.inventory[world.find_item("ammo_shells").unwrap()], 10);
    }

    #[test]
    fn ammo_pack_raises_every_cap_and_gives_each_ammo() {
        let mut world = test_world();
        let (player, item) = player_and_item(&mut world, "item_pack");
        let rockets = world.find_item("ammo_rockets").unwrap();
        world.client_mut(player).unwrap().pers.inventory[rockets] = 98;
        let pickup = world.items[world.find_item("item_pack").unwrap()]
            .pickup
            .unwrap();
        assert!(pickup(&mut world, item, player));
        let pers = pers(&world, player);
        let m = pers.ammo_max;
        assert_eq!(
            (m.bullets, m.shells, m.rockets, m.grenades, m.cells, m.slugs),
            (300, 200, 100, 100, 300, 100)
        );
        assert_eq!(pers.inventory[rockets], 100, "98 + 5 capped at 100");
        assert_eq!(pers.inventory[world.find_item("ammo_slugs").unwrap()], 10);
    }

    #[test]
    fn new_client_starts_with_c_default_ammo_caps() {
        let m = crate::entity::ClientPersistent::default().ammo_max;
        assert_eq!(
            (m.bullets, m.shells, m.rockets, m.grenades, m.cells, m.slugs),
            (200, 100, 50, 50, 200, 50)
        );
    }

    #[test]
    fn pickup_weapon_adds_to_inventory() {
        let mut world = test_world();
        world.init_items();

        let player = world.spawn_test_client(crate::entity::ClientData::default());

        let item_ent = world.spawn().unwrap();
        world.entities.get_mut(item_ent).unwrap().game.item = Some(8); // shotgun

        let result = pickup_weapon(&mut world, item_ent, player);
        assert!(result);

        let inv = world.client(player).unwrap().pers.inventory;
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

    fn client_with(world: &mut GameWorld, items: &[(&str, i32)]) -> EntityKey {
        let mut client = crate::entity::ClientData::default();
        for &(classname, count) in items {
            client.pers.inventory[world.find_item(classname).unwrap()] = count;
        }
        let player = world.spawn_test_client(client);
        let ent = world.entities.get_mut(player).unwrap();
        ent.game.health = 100;
        ent.game.max_health = 100;
        player
    }

    fn item_entity(world: &mut GameWorld, classname: &str, count: i32) -> EntityKey {
        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().game.classname = classname.to_string();
        world.entities.get_mut(key).unwrap().game.count = count;
        let item = match health_spawn(classname) {
            Some(_) => world.find_item_by_name("Health"),
            None => world.find_item(classname),
        };
        world.spawn_item(key, item.unwrap());
        key
    }

    fn inventory(world: &GameWorld, player: EntityKey, classname: &str) -> i32 {
        let index = world.find_item(classname).unwrap();
        world.client(player).unwrap().pers.inventory[index]
    }

    fn armor_world() -> GameWorld {
        let mut world = test_world();
        world.init_items();
        world
    }

    #[test]
    fn pickup_armor_of_a_weaker_type_tops_up_the_worn_armor() {
        let mut world = armor_world();
        let player = client_with(&mut world, &[("item_armor_body", 100)]);
        let jacket = item_entity(&mut world, "item_armor_jacket", 0);

        assert!(pickup_armor(&mut world, jacket, player));

        // C: 100 + (0.3 / 0.8) * 25 salvaged.
        assert_eq!(inventory(&world, player, "item_armor_body"), 109);
        assert_eq!(inventory(&world, player, "item_armor_jacket"), 0);
    }

    #[test]
    fn pickup_armor_of_a_better_type_replaces_the_worn_armor() {
        let mut world = armor_world();
        let player = client_with(&mut world, &[("item_armor_jacket", 50)]);
        let body = item_entity(&mut world, "item_armor_body", 0);

        assert!(pickup_armor(&mut world, body, player));

        // C: 100 + (0.3 / 0.8) * 50 salvaged.
        assert_eq!(inventory(&world, player, "item_armor_body"), 118);
        assert_eq!(inventory(&world, player, "item_armor_jacket"), 0);
    }

    #[test]
    fn pickup_armor_is_refused_when_the_worn_armor_is_full() {
        let mut world = armor_world();
        let player = client_with(&mut world, &[("item_armor_jacket", 50)]);
        let jacket = item_entity(&mut world, "item_armor_jacket", 0);

        assert!(!pickup_armor(&mut world, jacket, player));
    }

    #[test]
    fn armor_shard_gives_jacket_armor_or_adds_to_the_worn_type() {
        let mut world = armor_world();
        world.set_max_clients(3);
        let bare = client_with(&mut world, &[]);
        let shard = item_entity(&mut world, "item_armor_shard", 0);
        assert!(pickup_armor(&mut world, shard, bare));
        assert_eq!(inventory(&world, bare, "item_armor_jacket"), 2);

        let armored = client_with(&mut world, &[("item_armor_combat", 50)]);
        assert!(pickup_armor(&mut world, shard, armored));
        assert_eq!(inventory(&world, armored, "item_armor_combat"), 52);
        assert_eq!(inventory(&world, armored, "item_armor_jacket"), 0);
    }

    #[test]
    fn stimpack_is_taken_at_full_health_and_goes_over_max() {
        let mut world = armor_world();
        let player = client_with(&mut world, &[]);
        let stimpack = item_entity(&mut world, "item_health_small", 2);

        assert!(pickup_health(&mut world, stimpack, player));
        assert_eq!(world.entities.get(player).unwrap().game.health, 102);
    }

    #[test]
    fn mega_health_goes_over_max_and_rots_back_down_one_per_second() {
        let mut world = armor_world();
        world.level.time = 10.0;
        let player = client_with(&mut world, &[]);
        let mega = item_entity(&mut world, "item_health_mega", 100);

        assert!(pickup_health(&mut world, mega, player));
        assert_eq!(world.entities.get(player).unwrap().game.health, 200);
        let ent = world.entities.get(mega).unwrap();
        assert_eq!(ent.game.nextthink, 15.0);
        assert_eq!(ent.solid, Solid::Not);
        assert!(ent.game.flags.contains(EntityFlags::RESPAWN));

        let think = ent.think.unwrap();
        think(&mut world, mega);
        assert_eq!(world.entities.get(player).unwrap().game.health, 199);
        assert_eq!(world.entities.get(mega).unwrap().game.nextthink, 11.0);

        world.entities.get_mut(player).unwrap().game.health = 100;
        think(&mut world, mega);
        assert!(!world.entities.get(mega).unwrap().in_use);
    }

    #[test]
    fn adrenaline_raises_max_health_by_one_and_heals_to_it() {
        let mut world = armor_world();
        let player = client_with(&mut world, &[]);
        world.entities.get_mut(player).unwrap().game.health = 40;
        let adrenaline = item_entity(&mut world, "item_adrenaline", 0);

        assert!(touch_pickup(&mut world, adrenaline, player));

        let ent = world.entities.get(player).unwrap();
        assert_eq!((ent.game.health, ent.game.max_health), (101, 101));
    }

    #[test]
    fn ancient_head_raises_max_health_by_two() {
        let mut world = armor_world();
        let player = client_with(&mut world, &[]);
        let head = item_entity(&mut world, "item_ancient_head", 0);

        assert!(touch_pickup(&mut world, head, player));

        let ent = world.entities.get(player).unwrap();
        assert_eq!((ent.game.health, ent.game.max_health), (100, 102));
    }

    fn touch_pickup(world: &mut GameWorld, item: EntityKey, player: EntityKey) -> bool {
        let index = world.entities.get(item).unwrap().game.item.unwrap();
        let pickup = world.items[index].pickup.unwrap();
        pickup(world, item, player)
    }

    #[test]
    fn weapon_pickup_ammo_goes_to_the_ammo_item_slot() {
        let mut world = armor_world();
        let player = client_with(&mut world, &[]);
        let shotgun = item_entity(&mut world, "weapon_shotgun", 0);

        assert!(pickup_weapon(&mut world, shotgun, player));

        assert_eq!(inventory(&world, player, "ammo_shells"), 10);
    }

    #[test]
    fn ammo_tags_match_c() {
        assert_eq!(
            [
                AMMO_BULLETS,
                AMMO_SHELLS,
                AMMO_ROCKETS,
                AMMO_GRENADES,
                AMMO_CELLS,
                AMMO_SLUGS
            ],
            [0, 1, 2, 3, 4, 5]
        );
    }
}
