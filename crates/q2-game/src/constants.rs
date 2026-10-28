//! Game constants, flag enums, and gameplay values.
//!
//! All values are copied verbatim from the C source (`local.h`, `shared.h`)
//! with line references. Numerical precision matters for client/server sync.

use bitflags::bitflags;

// ---------------------------------------------------------------------------
// Frame timing
// ---------------------------------------------------------------------------

/// Game tick interval in seconds (10 Hz game frame rate).
/// C ref: local.h:56
pub const FRAMETIME: f32 = 0.1;

// ---------------------------------------------------------------------------
// Movement types — local.h:188-202
// ---------------------------------------------------------------------------

/// How an entity moves each physics frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum MoveType {
    /// Never moves.
    #[default]
    None = 0,
    /// Origin and angles change with no interaction.
    Noclip = 1,
    /// No clip to world, push on box contact.
    Push = 2,
    /// No clip to world, stops on box contact.
    Stop = 3,
    /// Gravity (player walking).
    Walk = 4,
    /// Gravity, special edge handling (monsters).
    Step = 5,
    /// No gravity, free flight.
    Fly = 6,
    /// Gravity, stops on first contact.
    Toss = 7,
    /// Like Fly but with larger monster bbox.
    FlyMissile = 8,
    /// Gravity, bounces on contact.
    Bounce = 9,
}

// ---------------------------------------------------------------------------
// Entity flags (FL_*) — local.h:62-75
// ---------------------------------------------------------------------------

bitflags! {
    /// Per-entity flags. C: FL_* defines.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct EntityFlags: u32 {
        const FLY             = 0x0000_0001;
        const SWIM            = 0x0000_0002;
        const IMMUNE_LASER    = 0x0000_0004;
        const INWATER         = 0x0000_0008;
        const GODMODE         = 0x0000_0010;
        const NOTARGET        = 0x0000_0020;
        const IMMUNE_SLIME    = 0x0000_0040;
        const IMMUNE_LAVA     = 0x0000_0080;
        const PARTIALGROUND   = 0x0000_0100;
        const WATERJUMP       = 0x0000_0200;
        const TEAMSLAVE       = 0x0000_0400;
        const NO_KNOCKBACK    = 0x0000_0800;
        const POWER_ARMOR     = 0x0000_1000;
        const COOP_TAKEN      = 0x0000_2000;
        const RESPAWN         = 0x8000_0000;
    }
}

// ---------------------------------------------------------------------------
// Server visibility flags (SVF_*) — shared.h:38-40
// ---------------------------------------------------------------------------

bitflags! {
    /// Server-side visibility flags. C: SVF_* defines.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct SvFlags: u32 {
        const NOCLIENT    = 0x0000_0001;
        const DEADMONSTER = 0x0000_0002;
        const MONSTER     = 0x0000_0004;
    }
}

// ---------------------------------------------------------------------------
// Damage flags (DAMAGE_*) — local.h:671-676
// ---------------------------------------------------------------------------

bitflags! {
    /// Modifiers for how damage is applied. C: DAMAGE_* defines.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct DamageFlags: u32 {
        /// Damage was indirect (explosion splash).
        const RADIUS         = 0x0000_0001;
        /// Armor does not protect from this damage.
        const NO_ARMOR       = 0x0000_0002;
        /// Damage is from an energy-based weapon.
        const ENERGY         = 0x0000_0004;
        /// Do not affect velocity, just view angles.
        const NO_KNOCKBACK   = 0x0000_0008;
        /// Damage is from a bullet (used for ricochets).
        const BULLET         = 0x0000_0010;
        /// Armor, shields, invulnerability, godmode have no effect.
        const NO_PROTECTION  = 0x0000_0020;
    }
}

// ---------------------------------------------------------------------------
// Means of death (MOD_*) — local.h:453-488
// ---------------------------------------------------------------------------

/// Tracks what killed an entity, for obituary messages and stats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum MeansOfDeath {
    Unknown = 0,
    Blaster = 1,
    Shotgun = 2,
    SShotgun = 3,
    Machinegun = 4,
    Chaingun = 5,
    Grenade = 6,
    GrenadeSplash = 7,
    Rocket = 8,
    RocketSplash = 9,
    Hyperblaster = 10,
    Railgun = 11,
    BfgLaser = 12,
    BfgBlast = 13,
    BfgEffect = 14,
    Handgrenade = 15,
    HandgrenadeSplash = 16,
    Water = 17,
    Slime = 18,
    Lava = 19,
    Crush = 20,
    Telefrag = 21,
    Falling = 22,
    Suicide = 23,
    HeldGrenade = 24,
    Explosive = 25,
    Barrel = 26,
    Bomb = 27,
    Exit = 28,
    Splash = 29,
    TargetLaser = 30,
    TriggerHurt = 31,
    Hit = 32,
    TargetBlaster = 33,
}

/// Friendly fire flag OR'd onto MeansOfDeath. C: MOD_FRIENDLY_FIRE
pub const MOD_FRIENDLY_FIRE: i32 = 0x0800_0000;

// ---------------------------------------------------------------------------
// Item type flags (IT_*) — local.h:213-219
// ---------------------------------------------------------------------------

bitflags! {
    /// Classification flags for items. C: IT_* defines.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct ItemFlags: u32 {
        const WEAPON      = 1;
        const AMMO        = 2;
        const ARMOR       = 4;
        const STAY_COOP   = 8;
        const KEY         = 16;
        const POWERUP     = 32;
        const INSTANT_USE = 64;
    }
}

// ---------------------------------------------------------------------------
// Dead / take-damage states — local.h:204-211
// ---------------------------------------------------------------------------

/// Entity dead state. C: DEAD_* defines.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum DeadFlag {
    #[default]
    No = 0,
    Dying = 1,
    Dead = 2,
    Respawnable = 3,
}

/// Whether an entity can be damaged. C: DAMAGE_* (not DAMAGE_FLAGS).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum TakeDamage {
    #[default]
    No = 0,
    Yes = 1,
    Aim = 2,
}

// ---------------------------------------------------------------------------
// AI flags (AI_*) — local.h:131-160
// ---------------------------------------------------------------------------

bitflags! {
    /// Monster AI behavior flags. C: AI_* defines.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct AiFlags: u32 {
        const STAND_GROUND    = 0x0000_0001;
        const TEMP_STAND_GROUND = 0x0000_0002;
        const SOUND_TARGET    = 0x0000_0004;
        const LOST_SIGHT      = 0x0000_0008;
        const PURSUIT_LAST_SEEN = 0x0000_0010;
        const PURSUE_NEXT     = 0x0000_0020;
        const PURSUE_TEMP     = 0x0000_0040;
        const HOLD_FRAME      = 0x0000_0080;
        const GOOD_GUY        = 0x0000_0100;
        const BRUTAL          = 0x0000_0200;
        const NOSTEP           = 0x0000_0400;
        const DUCKED          = 0x0000_0800;
        const COMBAT_POINT    = 0x0000_1000;
        const MEDIC           = 0x0000_2000;
        const RESURRECTING    = 0x0000_4000;
    }
}

// ---------------------------------------------------------------------------
// Attack state — local.h:166-170
// ---------------------------------------------------------------------------

/// Monster attack state. C: AS_* defines.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum AttackState {
    #[default]
    Straight = 1,
    Sliding = 2,
    Melee = 3,
    Missile = 4,
}

// ---------------------------------------------------------------------------
// Armor type indices — local.h:222-225
// ---------------------------------------------------------------------------

/// Armor class index for damage absorption calculations.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum ArmorType {
    #[default]
    None = 0,
    Jacket = 1,
    Combat = 2,
    Body = 3,
}

// ---------------------------------------------------------------------------
// Physics constants — matching C source exactly
// ---------------------------------------------------------------------------

/// STEPSIZE and MAX_CLIP_PLANES (C g_phys.c / pmove.c share one value each).
pub use q2_common::pmove::{MAX_CLIP_PLANES, STEPSIZE};

/// Default gravity in units/sec^2. Can be overridden by sv_gravity cvar.
/// C ref: g_main.c
pub const DEFAULT_GRAVITY: f32 = 800.0;

/// Maximum entity velocity before clamping.
/// C ref: g_main.c (sv_maxvelocity default)
pub const DEFAULT_MAX_VELOCITY: f32 = 2000.0;

// ---------------------------------------------------------------------------
// Inventory constants
// ---------------------------------------------------------------------------

/// Maximum number of items in a player's inventory.
pub use q2_shared::constants::MAX_ITEMS;

// ---------------------------------------------------------------------------
// Contents, masks and effect flags game code uses — defined in q2-shared
// ---------------------------------------------------------------------------

pub use q2_shared::constants::{
    CONTENTS_LAVA, CONTENTS_MONSTERCLIP, CONTENTS_SLIME, CONTENTS_WATER, EF_GIB, MASK_MONSTERSOLID,
    MASK_OPAQUE, MASK_SHOT, MASK_WATER,
};

// Spawnflags every entity can carry (g_spawn.c / local.h).
pub const SPAWNFLAG_NOT_EASY: i32 = 0x0000_0100;
pub const SPAWNFLAG_NOT_MEDIUM: i32 = 0x0000_0200;
pub const SPAWNFLAG_NOT_HARD: i32 = 0x0000_0400;
pub const SPAWNFLAG_NOT_DEATHMATCH: i32 = 0x0000_0800;
pub const SPAWNFLAG_NOT_COOP: i32 = 0x0000_1000;

/// C `SFL_CROSS_TRIGGER_MASK`: `game.serverflags` bits a unit change clears.
pub const SFL_CROSS_TRIGGER_MASK: i32 = 0x0000_00ff;

// ---------------------------------------------------------------------------
// Sound channels and attenuations (shared.h), skill levels, default spreads
// and power armor types (local.h), temp entity types (shared.h
// temp_event_t) that game code writes
// ---------------------------------------------------------------------------

pub const CHAN_AUTO: i32 = 0;
pub const CHAN_WEAPON: i32 = 1;
pub const CHAN_VOICE: i32 = 2;
pub const CHAN_BODY: i32 = 4;
pub const CHAN_NO_PHS_ADD: i32 = 8;

pub const ATTN_NONE: f32 = 0.0;
pub const ATTN_NORM: f32 = 1.0;
pub const ATTN_IDLE: f32 = 2.0;
pub const ATTN_STATIC: f32 = 3.0;

pub const SKILL_EASY: i32 = 0;
pub const SKILL_MEDIUM: i32 = 1;
pub const SKILL_HARD: i32 = 2;
pub const SKILL_HARDPLUS: i32 = 3;

pub const DEFAULT_BULLET_HSPREAD: i32 = 300;
pub const DEFAULT_BULLET_VSPREAD: i32 = 500;
pub const DEFAULT_SHOTGUN_HSPREAD: i32 = 1000;
pub const DEFAULT_SHOTGUN_VSPREAD: i32 = 500;
pub const DEFAULT_SHOTGUN_COUNT: i32 = 12;

pub const POWER_ARMOR_NONE: i32 = 0;
pub const POWER_ARMOR_SCREEN: i32 = 1;
pub const POWER_ARMOR_SHIELD: i32 = 2;

pub const TE_EXPLOSION1: i32 = 5;
pub const TE_EXPLOSION2: i32 = 6;
pub const TE_SPLASH: i32 = 10;
pub const TE_PARASITE_ATTACK: i32 = 16;
pub const TE_MEDIC_CABLE_ATTACK: i32 = 19;
/// Used as 22 in maps (shared.h: "DON'T RENUMBER").
pub const TE_BOSSTPORT: i32 = 22;

/// C `ANIM_DEATH` (local.h): the death animation holds its last frame.
pub const ANIM_DEATH: i32 = 5;
/// Most gibs thrown in one frame (C `MAX_GIBS`, local.h).
pub const MAX_GIBS: i32 = 20;
/// Most debris chunks thrown in one frame (C `MAX_DEBRIS`, local.h).
pub const MAX_DEBRIS: i32 = 20;
/// Corpses kept in coop and deathmatch (C `BODY_QUEUE_SIZE`, local.h).
pub const BODY_QUEUE_SIZE: usize = 8;

// ---------------------------------------------------------------------------
// Range classification for AI — local.h:175-180
// ---------------------------------------------------------------------------

/// Distance classification for monster AI decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range {
    Melee = 0,
    Near = 1,
    Mid = 2,
    Far = 3,
}

// ---------------------------------------------------------------------------
// Water level — shared.h
// ---------------------------------------------------------------------------

/// How deep an entity is submerged.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
#[repr(i32)]
pub enum WaterLevel {
    #[default]
    None = 0,
    Feet = 1,
    Waist = 2,
    Head = 3,
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Flag value parity with C --

    #[test]
    fn entity_flags_match_c() {
        assert_eq!(EntityFlags::FLY.bits(), 0x01);
        assert_eq!(EntityFlags::SWIM.bits(), 0x02);
        assert_eq!(EntityFlags::GODMODE.bits(), 0x10);
        assert_eq!(EntityFlags::NO_KNOCKBACK.bits(), 0x0800);
        assert_eq!(EntityFlags::POWER_ARMOR.bits(), 0x1000);
        assert_eq!(EntityFlags::RESPAWN.bits(), 0x8000_0000);
    }

    #[test]
    fn svflags_match_c() {
        assert_eq!(SvFlags::NOCLIENT.bits(), 0x01);
        assert_eq!(SvFlags::DEADMONSTER.bits(), 0x02);
        assert_eq!(SvFlags::MONSTER.bits(), 0x04);
    }

    #[test]
    fn damage_flags_match_c() {
        assert_eq!(DamageFlags::RADIUS.bits(), 0x01);
        assert_eq!(DamageFlags::NO_ARMOR.bits(), 0x02);
        assert_eq!(DamageFlags::ENERGY.bits(), 0x04);
        assert_eq!(DamageFlags::NO_KNOCKBACK.bits(), 0x08);
        assert_eq!(DamageFlags::BULLET.bits(), 0x10);
        assert_eq!(DamageFlags::NO_PROTECTION.bits(), 0x20);
    }

    #[test]
    fn means_of_death_values() {
        assert_eq!(MeansOfDeath::Unknown as i32, 0);
        assert_eq!(MeansOfDeath::Blaster as i32, 1);
        assert_eq!(MeansOfDeath::Railgun as i32, 11);
        assert_eq!(MeansOfDeath::TargetBlaster as i32, 33);
    }

    #[test]
    fn item_flags_match_c() {
        assert_eq!(ItemFlags::WEAPON.bits(), 1);
        assert_eq!(ItemFlags::AMMO.bits(), 2);
        assert_eq!(ItemFlags::ARMOR.bits(), 4);
        assert_eq!(ItemFlags::POWERUP.bits(), 32);
        assert_eq!(ItemFlags::INSTANT_USE.bits(), 64);
    }

    #[test]
    fn ai_flags_match_c() {
        assert_eq!(AiFlags::STAND_GROUND.bits(), 0x01);
        assert_eq!(AiFlags::HOLD_FRAME.bits(), 0x80);
        assert_eq!(AiFlags::COMBAT_POINT.bits(), 0x1000);
        assert_eq!(AiFlags::RESURRECTING.bits(), 0x4000);
    }

    // -- Bitflag combinations --

    #[test]
    fn flag_combinations_work() {
        let flags = EntityFlags::FLY | EntityFlags::SWIM;
        assert!(flags.contains(EntityFlags::FLY));
        assert!(flags.contains(EntityFlags::SWIM));
        assert!(!flags.contains(EntityFlags::GODMODE));
    }

    #[test]
    fn damage_flag_combinations() {
        let flags = DamageFlags::RADIUS | DamageFlags::NO_ARMOR;
        assert!(flags.contains(DamageFlags::RADIUS));
        assert!(flags.contains(DamageFlags::NO_ARMOR));
        assert!(!flags.contains(DamageFlags::ENERGY));
    }

    // -- Enum defaults --

    #[test]
    fn movetype_default_is_none() {
        assert_eq!(MoveType::default(), MoveType::None);
    }

    #[test]
    fn deadflag_default_is_no() {
        assert_eq!(DeadFlag::default(), DeadFlag::No);
    }

    #[test]
    fn takedamage_default_is_no() {
        assert_eq!(TakeDamage::default(), TakeDamage::No);
    }

    // -- Physics constants --

    #[test]
    fn physics_constants_match_c() {
        assert_eq!(STEPSIZE, 18.0);
        assert_eq!(MAX_CLIP_PLANES, 5);
        assert_eq!(DEFAULT_GRAVITY, 800.0);
        assert_eq!(FRAMETIME, 0.1);
    }

    #[test]
    fn content_masks_match_c_shared_h() {
        assert_eq!(MASK_MONSTERSOLID, 0x0202_0003);
        assert_eq!(MASK_SHOT, 0x0600_0003);
        assert_eq!(MASK_WATER, 0x38);
    }

    #[test]
    fn mod_friendly_fire_flag() {
        assert_eq!(MOD_FRIENDLY_FIRE, 0x0800_0000);
        // Can be OR'd onto any MOD value
        let mod_with_friendly = MeansOfDeath::Blaster as i32 | MOD_FRIENDLY_FIRE;
        assert_eq!(mod_with_friendly & MOD_FRIENDLY_FIRE, MOD_FRIENDLY_FIRE);
        assert_eq!(
            mod_with_friendly & !MOD_FRIENDLY_FIRE,
            MeansOfDeath::Blaster as i32
        );
    }

    #[test]
    fn water_level_ordering() {
        assert!(WaterLevel::None < WaterLevel::Feet);
        assert!(WaterLevel::Feet < WaterLevel::Waist);
        assert!(WaterLevel::Waist < WaterLevel::Head);
    }
}
