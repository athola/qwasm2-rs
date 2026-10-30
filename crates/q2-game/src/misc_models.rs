//! The model-only `g_misc.c` set pieces: banners, the satellite dish, dead
//! soldiers, the black hole, the easter-egg tank and chicks, the tank
//! commander's body, the vipers and their bomb, the strogg ship and the
//! debug `viewthing`.
//!
//! None of these is a monster in C (none runs `monster_start`), so none
//! gets `monsterinfo` here: `call_spawn` would take it for a monster.
//!
//! # C source reference
//! `~/Qwasm2/src/game/g_misc.c`

use std::collections::HashMap;

use q2_shared::constants::{EF_ROCKET, RF_FRAMELERP, RF_NOSHADOW, RF_TRANSLUCENT, SVF_NOCLIENT};
use q2_shared::types::*;

use crate::constants::*;
use crate::entity::{EntityKey, EntityStorage, MoveInfo};
use crate::func::{func_train_find, train_use};
use crate::misc::{become_explosion2, throw_gib, throw_head, GibType};
use crate::world::GameWorld;

/// Play `name` on the entity's body channel at full volume.
fn body_sound(world: &mut GameWorld, key: EntityKey, name: &str) {
    let index = world.gi.sound_index(name);
    world
        .gi
        .sound(world.number_of(key), CHAN_BODY, index, 1.0, ATTN_NORM, 0.0);
}

/// Think again in `delay` seconds.
fn think_in(world: &mut GameWorld, key: EntityKey, delay: f32) {
    let time = world.level.time;
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.nextthink = time + delay;
    }
}

/// Advance a frame; past `end - 1` restart at `restart`. Thinks every frame.
fn loop_frames(world: &mut GameWorld, key: EntityKey, end: i32, restart: i32) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.state.frame += 1;
        if ent.state.frame >= end {
            ent.state.frame = restart;
        }
    }
    think_in(world, key, FRAMETIME);
}

// ---------------------------------------------------------------------------
// misc_banner
// ---------------------------------------------------------------------------

/// Wave through the 16 frames. C ref: `misc_banner_think`.
pub fn misc_banner_think(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.state.frame = (ent.state.frame + 1) % 16;
    }
    think_in(world, key, FRAMETIME);
}

/// C ref: `SP_misc_banner` (the model comes from `spawn::SPAWNS`).
pub fn sp_misc_banner(storage: &mut EntityStorage, key: EntityKey, _: &HashMap<String, String>) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.movetype = MoveType::None;
        ent.solid = Solid::Not;
        ent.think = Some(misc_banner_think);
    }
}

/// The random start frame and first think of C `SP_misc_banner`.
pub fn finish_misc_banner(world: &mut GameWorld, key: EntityKey, _: &HashMap<String, String>) {
    let frame = (world.random_u32() % 16) as i32;
    if let Some(ent) = world.entities.get_mut(key) {
        ent.state.frame = frame;
    }
    think_in(world, key, FRAMETIME);
}

// ---------------------------------------------------------------------------
// misc_satellite_dish
// ---------------------------------------------------------------------------

/// Turn through frames 1..38, then stop. C ref: `misc_satellite_dish_think`.
pub fn misc_satellite_dish_think(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.state.frame += 1;
    if ent.state.frame < 38 {
        think_in(world, key, FRAMETIME);
    }
}

/// Start turning from frame 0. C ref: `misc_satellite_dish_use`.
pub fn misc_satellite_dish_use(world: &mut GameWorld, key: EntityKey, _: EntityKey, _: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.state.frame = 0;
        ent.think = Some(misc_satellite_dish_think);
    }
    think_in(world, key, FRAMETIME);
}

/// C ref: `SP_misc_satellite_dish`.
pub fn sp_misc_satellite_dish(
    storage: &mut EntityStorage,
    key: EntityKey,
    _: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.movetype = MoveType::None;
        ent.solid = Solid::Bbox;
        ent.mins = Vec3f::new(-64.0, -64.0, 0.0);
        ent.maxs = Vec3f::new(64.0, 64.0, 128.0);
        ent.use_fn = Some(misc_satellite_dish_use);
    }
}

// ---------------------------------------------------------------------------
// misc_deadsoldier
// ---------------------------------------------------------------------------

/// Gib once beaten below -80 health. C ref: `misc_deadsoldier_die`.
pub fn misc_deadsoldier_die(
    world: &mut GameWorld,
    key: EntityKey,
    _inflictor: EntityKey,
    _attacker: EntityKey,
    damage: i32,
    _point: Vec3f,
) {
    if world.entities.get(key).is_none_or(|e| e.game.health > -80) {
        return;
    }
    body_sound(world, key, "misc/udeath.wav");
    for _ in 0..4 {
        throw_gib(
            world,
            key,
            "models/objects/gibs/sm_meat/tris.md2",
            damage,
            GibType::Organic,
        );
    }
    throw_head(
        world,
        key,
        "models/objects/gibs/head2/tris.md2",
        damage,
        GibType::Organic,
    );
}

/// C ref: `SP_misc_deadsoldier`; the deathmatch removal is in
/// `finish_misc_deadsoldier`. C also sets `AI_GOOD_GUY`, which only
/// matters to a live monster: this corpse has no monsterinfo (see the
/// module doc).
pub fn sp_misc_deadsoldier(
    storage: &mut EntityStorage,
    key: EntityKey,
    _: &HashMap<String, String>,
) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    ent.game.movetype = MoveType::None;
    ent.solid = Solid::Bbox;
    // The first of ON_BACK..IMPALED picks the pose; frame 0 by default.
    let sf = ent.game.spawnflags;
    ent.state.frame = [2, 4, 8, 16, 32]
        .iter()
        .position(|bit| sf & bit != 0)
        .map_or(0, |i| i as i32 + 1);
    ent.mins = Vec3f::new(-16.0, -16.0, 0.0);
    ent.maxs = Vec3f::new(16.0, 16.0, 16.0);
    ent.game.deadflag = DeadFlag::Dead;
    ent.game.takedamage = TakeDamage::Yes;
    ent.svflags |= (SvFlags::MONSTER | SvFlags::DEADMONSTER).bits();
    ent.die = Some(misc_deadsoldier_die);
}

/// Dead soldiers are removed in deathmatch. C ref: `SP_misc_deadsoldier`.
pub fn finish_misc_deadsoldier(world: &mut GameWorld, key: EntityKey, _: &HashMap<String, String>) {
    if world.game.deathmatch {
        world.free_entity(key);
    }
}

// ---------------------------------------------------------------------------
// misc_blackhole
// ---------------------------------------------------------------------------

/// Using the black hole removes it. C ref: `misc_blackhole_use`.
pub fn misc_blackhole_use(world: &mut GameWorld, key: EntityKey, _: EntityKey, _: EntityKey) {
    world.free_entity(key);
}

/// Spin through frames 0..18. C ref: `misc_blackhole_think`.
pub fn misc_blackhole_think(world: &mut GameWorld, key: EntityKey) {
    loop_frames(world, key, 19, 0);
}

/// Drop the shadow before the first physics frame.
/// C ref: `misc_blackhole_transparent`.
pub fn misc_blackhole_transparent(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.state.renderfx = RF_TRANSLUCENT | RF_NOSHADOW;
        ent.prethink = None;
    }
    world.link_entity(key);
}

/// C ref: `SP_misc_blackhole`.
pub fn sp_misc_blackhole(storage: &mut EntityStorage, key: EntityKey, _: &HashMap<String, String>) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.movetype = MoveType::None;
        ent.solid = Solid::Not;
        ent.mins = Vec3f::new(-64.0, -64.0, 0.0);
        ent.maxs = Vec3f::new(64.0, 64.0, 8.0);
        ent.state.renderfx = RF_TRANSLUCENT;
        ent.use_fn = Some(misc_blackhole_use);
        ent.think = Some(misc_blackhole_think);
        ent.prethink = Some(misc_blackhole_transparent);
    }
}

/// The first think of the C black hole and easter eggs, two frames in.
pub fn finish_think_in_two_frames(
    world: &mut GameWorld,
    key: EntityKey,
    _: &HashMap<String, String>,
) {
    think_in(world, key, 2.0 * FRAMETIME);
}

// ---------------------------------------------------------------------------
// misc_eastertank, misc_easterchick, misc_easterchick2
// ---------------------------------------------------------------------------

/// Loop the tank's frames 254..292. C ref: `misc_eastertank_think`.
pub fn misc_eastertank_think(world: &mut GameWorld, key: EntityKey) {
    loop_frames(world, key, 293, 254);
}

/// Loop the chick's frames 208..246. C ref: `misc_easterchick_think`.
pub fn misc_easterchick_think(world: &mut GameWorld, key: EntityKey) {
    loop_frames(world, key, 247, 208);
}

/// Loop the chick's frames 248..286. C ref: `misc_easterchick2_think`.
pub fn misc_easterchick2_think(world: &mut GameWorld, key: EntityKey) {
    loop_frames(world, key, 287, 248);
}

/// The shared body of the easter-egg spawns: a solid animated model.
fn easter_egg(
    storage: &mut EntityStorage,
    key: EntityKey,
    mins: Vec3f,
    frame: i32,
    think: crate::entity::ThinkFn,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.movetype = MoveType::None;
        ent.solid = Solid::Bbox;
        ent.mins = mins;
        ent.maxs = Vec3f::splat(32.0);
        ent.state.frame = frame;
        ent.think = Some(think);
    }
}

/// C ref: `SP_misc_eastertank`.
pub fn sp_misc_eastertank(
    storage: &mut EntityStorage,
    key: EntityKey,
    _: &HashMap<String, String>,
) {
    let mins = Vec3f::new(-32.0, -32.0, -16.0);
    easter_egg(storage, key, mins, 254, misc_eastertank_think);
}

/// C ref: `SP_misc_easterchick`.
pub fn sp_misc_easterchick(
    storage: &mut EntityStorage,
    key: EntityKey,
    _: &HashMap<String, String>,
) {
    let mins = Vec3f::new(-32.0, -32.0, 0.0);
    easter_egg(storage, key, mins, 208, misc_easterchick_think);
}

/// C ref: `SP_misc_easterchick2`.
pub fn sp_misc_easterchick2(
    storage: &mut EntityStorage,
    key: EntityKey,
    _: &HashMap<String, String>,
) {
    let mins = Vec3f::new(-32.0, -32.0, 0.0);
    easter_egg(storage, key, mins, 248, misc_easterchick2_think);
}

// ---------------------------------------------------------------------------
// monster_commander_body
// ---------------------------------------------------------------------------

const COMMANDER_MODEL: &str = "models/monsters/commandr/tris.md2";

/// Topple through frames 1..24, thudding at 22. C ref: `commander_body_think`.
pub fn commander_body_think(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.state.frame += 1;
    let frame = ent.state.frame;
    if frame < 24 {
        think_in(world, key, FRAMETIME);
    } else {
        ent.game.nextthink = 0.0;
    }
    if frame == 22 {
        body_sound(world, key, "tank/thud.wav");
    }
}

/// Start toppling. C ref: `commander_body_use`.
pub fn commander_body_use(world: &mut GameWorld, key: EntityKey, _: EntityKey, _: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.think = Some(commander_body_think);
    }
    think_in(world, key, FRAMETIME);
    body_sound(world, key, "tank/pain.wav");
}

/// Let the body fall to the floor. C ref: `commander_body_drop`.
pub fn commander_body_drop(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.movetype = MoveType::Toss;
        ent.state.origin.z += 2.0;
    }
}

/// With `g_commanderbody_nogod`, the body can be gibbed.
/// C ref: `commander_body_die`.
pub fn commander_body_die(
    world: &mut GameWorld,
    key: EntityKey,
    _inflictor: EntityKey,
    _attacker: EntityKey,
    damage: i32,
    _point: Vec3f,
) {
    if world
        .entities
        .get(key)
        .is_none_or(|e| e.game.health > e.game.gib_health)
    {
        return;
    }
    body_sound(world, key, "tank/pain.wav");
    throw_gib(
        world,
        key,
        "models/objects/gibs/sm_meat/tris.md2",
        damage,
        GibType::Organic,
    );
    for _ in 0..4 {
        throw_gib(
            world,
            key,
            "models/objects/gibs/sm_metal/tris.md2",
            damage,
            GibType::Metallic,
        );
    }
    throw_gib(
        world,
        key,
        "models/objects/gibs/chest/tris.md2",
        damage,
        GibType::Organic,
    );
    throw_head(
        world,
        key,
        "models/objects/gibs/gear/tris.md2",
        damage,
        GibType::Metallic,
    );
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.deadflag = DeadFlag::Dead;
    }
}

/// C ref: `SP_monster_commander_body`; the `g_commanderbody_nogod` choice,
/// sounds and drop are in `finish_commander_body`.
pub fn sp_monster_commander_body(
    storage: &mut EntityStorage,
    key: EntityKey,
    _: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.movetype = MoveType::None;
        ent.solid = Solid::Bbox;
        ent.game.model = COMMANDER_MODEL.to_string();
        ent.mins = Vec3f::new(-32.0, -32.0, 0.0);
        ent.maxs = Vec3f::new(32.0, 32.0, 48.0);
        ent.use_fn = Some(commander_body_use);
        ent.game.takedamage = TakeDamage::Yes;
        ent.state.renderfx |= RF_FRAMELERP;
        ent.think = Some(commander_body_drop);
    }
}

/// The rest of C `SP_monster_commander_body`: shootable only with the
/// `g_commanderbody_nogod` cvar (default 0, else god mode), sounds
/// precached, and dropped to the floor five frames in.
pub fn finish_commander_body(world: &mut GameWorld, key: EntityKey, _: &HashMap<String, String>) {
    let nogod = q2_common::cvar::parse_value(&world.gi.cvar("g_commanderbody_nogod", "0")) != 0.0;
    if let Some(ent) = world.entities.get_mut(key) {
        if nogod {
            ent.game.deadflag = DeadFlag::Dead;
            ent.svflags |= (SvFlags::MONSTER | SvFlags::DEADMONSTER).bits();
            ent.die = Some(commander_body_die);
        } else {
            ent.game.flags = EntityFlags::GODMODE;
        }
    }
    world.gi.sound_index("tank/thud.wav");
    world.gi.sound_index("tank/pain.wav");
    think_in(world, key, 5.0 * FRAMETIME);
}

// ---------------------------------------------------------------------------
// misc_viper, misc_strogg_ship, misc_bigviper
// ---------------------------------------------------------------------------

/// Appear and start flying the path. C ref: `misc_viper_use` (and the
/// identical `misc_strogg_ship_use`).
pub fn misc_viper_use(
    world: &mut GameWorld,
    key: EntityKey,
    other: EntityKey,
    activator: EntityKey,
) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.svflags &= !SVF_NOCLIENT;
    ent.use_fn = Some(train_use);
    train_use(world, key, other, activator);
}

/// The shared body of C `SP_misc_viper` and `SP_misc_strogg_ship`: a
/// hidden, non-solid train that waits to be used.
fn flyby_ship(storage: &mut EntityStorage, key: EntityKey) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    if ent.game.target.is_empty() {
        tracing::debug!(
            "{} without a target at {:?}",
            ent.game.classname,
            ent.absmin
        );
        storage.soft_free(key);
        return;
    }
    if ent.game.speed == 0.0 {
        ent.game.speed = 300.0;
    }
    ent.game.movetype = MoveType::Push;
    ent.solid = Solid::Not;
    ent.mins = Vec3f::new(-16.0, -16.0, 0.0);
    ent.maxs = Vec3f::new(16.0, 16.0, 32.0);
    ent.think = Some(func_train_find);
    ent.use_fn = Some(misc_viper_use);
    ent.svflags |= SVF_NOCLIENT;
    let speed = ent.game.speed;
    ent.moveinfo = Some(Box::new(MoveInfo {
        speed,
        accel: speed,
        decel: speed,
        ..Default::default()
    }));
}

/// C ref: `SP_misc_viper`; `finish_think_next_frame` schedules finding
/// its path.
pub fn sp_misc_viper(storage: &mut EntityStorage, key: EntityKey, _: &HashMap<String, String>) {
    flyby_ship(storage, key);
}

/// C ref: `SP_misc_strogg_ship`; `finish_think_next_frame` schedules
/// finding its path.
pub fn sp_misc_strogg_ship(
    storage: &mut EntityStorage,
    key: EntityKey,
    _: &HashMap<String, String>,
) {
    flyby_ship(storage, key);
}

/// The first think of the C flyby ships: `func_train_find` one frame in,
/// once the path corners have spawned.
pub fn finish_think_next_frame(world: &mut GameWorld, key: EntityKey, _: &HashMap<String, String>) {
    think_in(world, key, FRAMETIME);
}

/// A large stationary viper. C ref: `SP_misc_bigviper`.
pub fn sp_misc_bigviper(storage: &mut EntityStorage, key: EntityKey, _: &HashMap<String, String>) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.movetype = MoveType::None;
        ent.solid = Solid::Bbox;
        ent.mins = Vec3f::new(-176.0, -120.0, -24.0);
        ent.maxs = Vec3f::new(176.0, 120.0, 72.0);
    }
}

// ---------------------------------------------------------------------------
// misc_viper_bomb
// ---------------------------------------------------------------------------

/// Fire the bomb's targets, then blow up at floor level.
/// C ref: `misc_viper_bomb_touch`.
pub fn misc_viper_bomb_touch(
    world: &mut GameWorld,
    key: EntityKey,
    _other: EntityKey,
    _plane: Option<&Plane>,
    _surf: Option<&Surface>,
) {
    // misc_viper_bomb_use set the activator before setting this touch.
    let Some(activator) = world
        .entities
        .get(key)
        .map(|e| e.game.activator.unwrap_or(key))
    else {
        return;
    };
    world.use_targets(key, activator);
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.state.origin.z = ent.absmin.z + 1.0;
    let dmg = ent.game.dmg as f32;
    world.t_radius_damage(key, key, dmg, None, dmg + 40.0, MeansOfDeath::Bomb);
    become_explosion2(world, key);
}

/// Pitch the falling bomb from the viper's heading towards straight down
/// over a second, rolling 10 degrees a frame.
/// C ref: `misc_viper_bomb_prethink`.
pub fn misc_viper_bomb_prethink(world: &mut GameWorld, key: EntityKey) {
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.game.ground_entity = None;
    let diff = (ent.game.timestamp - time).max(-1.0);
    let dir = ent.moveinfo.as_ref().map_or(Vec3f::ZERO, |mi| mi.dir);
    let mut v = dir * (1.0 + diff);
    v.z = diff;
    let roll = ent.state.angles.z;
    ent.state.angles = crate::weapons::vectoangles(v);
    ent.state.angles.z = roll + 10.0;
}

/// Drop from the viper at its speed and heading. C does not check that a
/// misc_viper exists; without one the bomb falls straight down.
/// C ref: `misc_viper_bomb_use`.
pub fn misc_viper_bomb_use(
    world: &mut GameWorld,
    key: EntityKey,
    _: EntityKey,
    activator: EntityKey,
) {
    let (dir, speed) = world
        .find_by_classname(None, "misc_viper")
        .and_then(|v| world.entities.get(v))
        .and_then(|v| v.moveinfo.as_ref())
        .map_or((Vec3f::ZERO, 0.0), |mi| (mi.dir, mi.speed));
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.solid = Solid::Bbox;
    ent.svflags &= !SVF_NOCLIENT;
    ent.state.effects |= EF_ROCKET;
    ent.use_fn = None;
    ent.game.movetype = MoveType::Toss;
    ent.prethink = Some(misc_viper_bomb_prethink);
    ent.touch = Some(misc_viper_bomb_touch);
    ent.game.activator = Some(activator);
    ent.velocity = dir * speed;
    ent.game.timestamp = time;
    ent.moveinfo.get_or_insert_with(Box::default).dir = dir;
}

/// C ref: `SP_misc_viper_bomb`.
pub fn sp_misc_viper_bomb(
    storage: &mut EntityStorage,
    key: EntityKey,
    _: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.movetype = MoveType::None;
        ent.solid = Solid::Not;
        ent.mins = Vec3f::splat(-8.0);
        ent.maxs = Vec3f::splat(8.0);
        if ent.game.dmg == 0 {
            ent.game.dmg = 1000;
        }
        ent.use_fn = Some(misc_viper_bomb_use);
        ent.svflags |= SVF_NOCLIENT;
    }
}

// ---------------------------------------------------------------------------
// viewthing
// ---------------------------------------------------------------------------

/// Step through 7 frames. C ref: `TH_viewthing`.
pub fn th_viewthing(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.state.frame = (ent.state.frame + 1) % 7;
    }
    think_in(world, key, FRAMETIME);
}

/// A debugging model (the banner). C ref: `SP_viewthing`.
pub fn sp_viewthing(storage: &mut EntityStorage, key: EntityKey, _: &HashMap<String, String>) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.movetype = MoveType::None;
        ent.solid = Solid::Bbox;
        ent.state.renderfx = RF_FRAMELERP;
        ent.mins = Vec3f::new(-16.0, -16.0, -24.0);
        ent.maxs = Vec3f::new(16.0, 16.0, 32.0);
        ent.think = Some(th_viewthing);
    }
}

/// The message and half-second first think of C `SP_viewthing`.
pub fn finish_viewthing(world: &mut GameWorld, key: EntityKey, _: &HashMap<String, String>) {
    world.gi.dprintf("viewthing spawned\n");
    think_in(world, key, 0.5);
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{DieFn, ThinkFn, UseFn};
    use crate::world::{test_world_recording, MockGameImport};
    use q2_shared::constants::EF_GIB;
    use std::rc::Rc;

    /// A world spawned from a worldspawn plus `entities`, after `setup`
    /// prepares the import.
    fn spawned_with(
        setup: impl FnOnce(&MockGameImport),
        entities: &str,
    ) -> (GameWorld, Rc<MockGameImport>) {
        let (mut world, gi) = test_world_recording();
        setup(&gi);
        world.init_game();
        world.spawn_entities(
            "base1",
            &format!("{{ \"classname\" \"worldspawn\" }}\n{entities}"),
            "",
        );
        (world, gi)
    }

    fn spawned(entities: &str) -> (GameWorld, Rc<MockGameImport>) {
        spawned_with(|_| {}, entities)
    }

    fn one(world: &GameWorld, classname: &str) -> EntityKey {
        world.find_by_classname(None, classname).unwrap()
    }

    fn think_is(world: &GameWorld, key: EntityKey, f: ThinkFn) -> bool {
        world
            .entities
            .get(key)
            .and_then(|e| e.think)
            .is_some_and(|t| std::ptr::fn_addr_eq(t, f))
    }

    fn use_is(world: &GameWorld, key: EntityKey, f: UseFn) -> bool {
        world
            .entities
            .get(key)
            .and_then(|e| e.use_fn)
            .is_some_and(|u| std::ptr::fn_addr_eq(u, f))
    }

    fn run_think(world: &mut GameWorld, key: EntityKey) {
        let think = world.entities.get(key).and_then(|e| e.think).unwrap();
        think(world, key);
    }

    fn frame(world: &GameWorld, key: EntityKey) -> i32 {
        world.entities.get(key).unwrap().state.frame
    }

    fn set_frame(world: &mut GameWorld, key: EntityKey, frame: i32) {
        world.entities.get_mut(key).unwrap().state.frame = frame;
    }

    fn nextthink(world: &GameWorld, key: EntityKey) -> f32 {
        world.entities.get(key).unwrap().game.nextthink
    }

    fn thinks_after(world: &GameWorld, key: EntityKey, delay: f32) -> bool {
        (nextthink(world, key) - (world.level.time + delay)).abs() < 1e-5
    }

    fn sounds_played(gi: &MockGameImport) -> Vec<String> {
        let names = gi.sounds.borrow();
        gi.sound_calls
            .borrow()
            .iter()
            .map(|c| names[c.2 as usize - 1].clone())
            .collect()
    }

    fn gib_count(world: &GameWorld) -> usize {
        world
            .entities
            .iter()
            .filter(|(_, e)| e.in_use && e.state.effects & EF_GIB != 0)
            .count()
    }

    fn hit(world: &mut GameWorld, target: EntityKey, damage: i32) {
        let attacker = world.spawn().unwrap();
        world.t_damage(
            target,
            attacker,
            attacker,
            Vec3f::ZERO,
            Vec3f::ZERO,
            Vec3f::ZERO,
            damage,
            0,
            DamageFlags::empty(),
            MeansOfDeath::Unknown,
        );
    }

    #[test]
    fn banner_waves_through_sixteen_frames_from_a_random_one() {
        let (mut world, gi) = spawned(r#"{ "classname" "misc_banner" }"#);
        let banner = one(&world, "misc_banner");
        let e = world.entities.get(banner).unwrap();
        assert_eq!((e.solid, e.game.movetype), (Solid::Not, MoveType::None));
        assert!((0..16).contains(&e.state.frame));
        let model = "models/objects/banner/tris.md2";
        assert!(gi.models.borrow().iter().any(|m| m == model));
        assert!(think_is(&world, banner, misc_banner_think));
        assert!(thinks_after(&world, banner, FRAMETIME));
        set_frame(&mut world, banner, 15);
        run_think(&mut world, banner);
        assert_eq!(frame(&world, banner), 0);
    }

    #[test]
    fn satellite_dish_turns_once_through_38_frames_when_used() {
        let (mut world, _) = spawned(r#"{ "classname" "misc_satellite_dish" }"#);
        let dish = one(&world, "misc_satellite_dish");
        let e = world.entities.get(dish).unwrap();
        assert_eq!(e.solid, Solid::Bbox);
        assert_eq!(
            (e.mins, e.maxs),
            (Vec3f::new(-64.0, -64.0, 0.0), Vec3f::new(64.0, 64.0, 128.0))
        );
        assert!(e.think.is_none());
        set_frame(&mut world, dish, 20);
        misc_satellite_dish_use(&mut world, dish, dish, dish);
        assert_eq!(frame(&world, dish), 0);
        assert!(think_is(&world, dish, misc_satellite_dish_think));
        assert!(thinks_after(&world, dish, FRAMETIME));
        set_frame(&mut world, dish, 36);
        world.entities.get_mut(dish).unwrap().game.nextthink = 0.0;
        run_think(&mut world, dish);
        assert!(nextthink(&world, dish) > 0.0, "frame 37 thinks on");
        world.entities.get_mut(dish).unwrap().game.nextthink = 0.0;
        run_think(&mut world, dish);
        assert_eq!(frame(&world, dish), 38);
        assert_eq!(nextthink(&world, dish), 0.0, "stops at 38");
    }

    #[test]
    fn deadsoldier_pose_follows_the_first_pose_spawnflag() {
        for (flags, pose) in [
            (0, 0),
            (1, 0),
            (2, 1),
            (4, 2),
            (8, 3),
            (16, 4),
            (32, 5),
            (6, 1),
        ] {
            let (world, _) = spawned(&format!(
                r#"{{ "classname" "misc_deadsoldier" "spawnflags" "{flags}" }}"#
            ));
            let body = one(&world, "misc_deadsoldier");
            assert_eq!(frame(&world, body), pose, "spawnflags {flags}");
        }
    }

    #[test]
    fn deadsoldier_is_a_shootable_dead_monster_that_is_not_counted() {
        let (world, gi) = spawned(r#"{ "classname" "misc_deadsoldier" }"#);
        let body = one(&world, "misc_deadsoldier");
        let e = world.entities.get(body).unwrap();
        assert_eq!(
            (e.solid, e.game.takedamage, e.game.movetype),
            (Solid::Bbox, TakeDamage::Yes, MoveType::None)
        );
        assert_eq!(e.game.deadflag, DeadFlag::Dead);
        let flags = (SvFlags::MONSTER | SvFlags::DEADMONSTER).bits();
        assert_eq!(e.svflags & flags, flags);
        assert_eq!(
            (e.mins, e.maxs),
            (Vec3f::new(-16.0, -16.0, 0.0), Vec3f::new(16.0, 16.0, 16.0))
        );
        let model = "models/deadbods/dude/tris.md2";
        assert!(gi.models.borrow().iter().any(|m| m == model));
        assert!(e.monsterinfo.is_none());
        assert_eq!(world.level.total_monsters, 0);
    }

    #[test]
    fn deadsoldier_gibs_only_below_minus_80_health() {
        let (mut world, gi) = spawned(r#"{ "classname" "misc_deadsoldier" }"#);
        let body = one(&world, "misc_deadsoldier");
        hit(&mut world, body, 50);
        assert_eq!(gib_count(&world), 0, "health -50");
        assert!(sounds_played(&gi).iter().all(|s| s != "misc/udeath.wav"));
        hit(&mut world, body, 40);
        // Four meat gibs, and the body itself becomes the head.
        assert_eq!(gib_count(&world), 5);
        assert!(sounds_played(&gi).iter().any(|s| s == "misc/udeath.wav"));
        assert_eq!(world.level.killed_monsters, 0);
    }

    #[test]
    fn deathmatch_removes_dead_soldiers() {
        let (mut world, _) = test_world_recording();
        world.init_game();
        world.game.deathmatch = true;
        world.spawn_entities(
            "base1",
            r#"{ "classname" "worldspawn" } { "classname" "misc_deadsoldier" }"#,
            "",
        );
        assert!(world.find_by_classname(None, "misc_deadsoldier").is_none());
    }

    #[test]
    fn blackhole_spins_turns_shadowless_and_vanishes_when_used() {
        let (mut world, _) = spawned(r#"{ "classname" "misc_blackhole" }"#);
        let hole = one(&world, "misc_blackhole");
        let e = world.entities.get(hole).unwrap();
        assert_eq!((e.solid, e.state.renderfx), (Solid::Not, RF_TRANSLUCENT));
        assert_eq!(
            (e.mins, e.maxs),
            (Vec3f::new(-64.0, -64.0, 0.0), Vec3f::new(64.0, 64.0, 8.0))
        );
        assert!(thinks_after(&world, hole, 2.0 * FRAMETIME));
        world.run_frame();
        let e = world.entities.get(hole).unwrap();
        assert_eq!(e.state.renderfx, RF_TRANSLUCENT | RF_NOSHADOW);
        assert!(e.prethink.is_none());
        set_frame(&mut world, hole, 18);
        run_think(&mut world, hole);
        assert_eq!(frame(&world, hole), 0);
        assert!(thinks_after(&world, hole, FRAMETIME));
        assert!(use_is(&world, hole, misc_blackhole_use));
        misc_blackhole_use(&mut world, hole, hole, hole);
        assert!(!world.entities.is_live(hole));
    }

    #[test]
    fn easter_eggs_loop_their_c_frame_ranges() {
        for (class, mins_z, start, last) in [
            ("misc_eastertank", -16.0, 254, 292),
            ("misc_easterchick", 0.0, 208, 246),
            ("misc_easterchick2", 0.0, 248, 286),
        ] {
            let (mut world, _) = spawned(&format!(r#"{{ "classname" "{class}" }}"#));
            let egg = one(&world, class);
            let e = world.entities.get(egg).unwrap();
            assert_eq!(e.solid, Solid::Bbox, "{class}");
            assert_eq!(
                (e.mins, e.maxs),
                (Vec3f::new(-32.0, -32.0, mins_z), Vec3f::splat(32.0)),
                "{class}"
            );
            assert_eq!(e.state.frame, start, "{class}");
            assert!(thinks_after(&world, egg, 2.0 * FRAMETIME), "{class}");
            set_frame(&mut world, egg, last - 1);
            run_think(&mut world, egg);
            assert_eq!(frame(&world, egg), last, "{class}");
            run_think(&mut world, egg);
            assert_eq!(frame(&world, egg), start, "{class}");
            assert_eq!(world.level.total_monsters, 0, "{class}");
        }
    }

    #[test]
    fn commander_body_is_godmode_by_default_and_drops_five_frames_in() {
        let (mut world, gi) =
            spawned(r#"{ "classname" "monster_commander_body" "origin" "0 0 10" }"#);
        let body = one(&world, "monster_commander_body");
        let e = world.entities.get(body).unwrap();
        assert_eq!(e.game.flags, EntityFlags::GODMODE);
        assert_eq!((e.solid, e.game.takedamage), (Solid::Bbox, TakeDamage::Yes));
        assert_eq!(
            (e.mins, e.maxs),
            (Vec3f::new(-32.0, -32.0, 0.0), Vec3f::new(32.0, 32.0, 48.0))
        );
        assert_ne!(e.state.renderfx & RF_FRAMELERP, 0);
        assert_eq!(e.game.model, COMMANDER_MODEL);
        assert!(e.die.is_none() && e.monsterinfo.is_none());
        assert!(use_is(&world, body, commander_body_use));
        assert_eq!(world.level.total_monsters, 0);
        for s in ["tank/thud.wav", "tank/pain.wav"] {
            assert!(gi.sounds.borrow().iter().any(|n| n == s), "{s}");
        }
        assert!(think_is(&world, body, commander_body_drop));
        assert!(thinks_after(&world, body, 5.0 * FRAMETIME));
        run_think(&mut world, body);
        let e = world.entities.get(body).unwrap();
        assert_eq!((e.game.movetype, e.state.origin.z), (MoveType::Toss, 12.0));
    }

    #[test]
    fn using_the_commander_body_topples_it_with_a_thud_at_frame_22() {
        let (mut world, gi) = spawned(r#"{ "classname" "monster_commander_body" }"#);
        let body = one(&world, "monster_commander_body");
        commander_body_use(&mut world, body, body, body);
        assert!(think_is(&world, body, commander_body_think));
        assert!(thinks_after(&world, body, FRAMETIME));
        assert_eq!(sounds_played(&gi), ["tank/pain.wav"]);
        set_frame(&mut world, body, 21);
        run_think(&mut world, body);
        assert_eq!(sounds_played(&gi), ["tank/pain.wav", "tank/thud.wav"]);
        assert!(nextthink(&world, body) > 0.0);
        set_frame(&mut world, body, 23);
        run_think(&mut world, body);
        assert_eq!(nextthink(&world, body), 0.0, "stops at frame 24");
    }

    #[test]
    fn commander_body_with_nogod_gibs_as_a_dead_monster() {
        let (mut world, gi) = spawned_with(
            |gi| {
                let mut cvars = gi.cvars.borrow_mut();
                cvars.insert("g_commanderbody_nogod".into(), "1".into());
            },
            r#"{ "classname" "monster_commander_body" }"#,
        );
        let body = one(&world, "monster_commander_body");
        let e = world.entities.get(body).unwrap();
        assert!(!e.game.flags.contains(EntityFlags::GODMODE));
        assert_eq!(e.game.deadflag, DeadFlag::Dead);
        let flags = (SvFlags::MONSTER | SvFlags::DEADMONSTER).bits();
        assert_eq!(e.svflags & flags, flags);
        assert!(e
            .die
            .is_some_and(|d| std::ptr::fn_addr_eq(d, commander_body_die as DieFn)));
        assert_eq!(world.level.total_monsters, 0);
        hit(&mut world, body, 10);
        // Meat, four metal, chest, and the body becomes the gear head.
        assert_eq!(gib_count(&world), 7);
        assert!(sounds_played(&gi).iter().any(|s| s == "tank/pain.wav"));
        assert_eq!(world.level.killed_monsters, 0);
    }

    const VIPER_PATH: &str = r#"
        { "classname" "path_corner" "targetname" "v1" "target" "v2" "origin" "0 0 0" }
        { "classname" "path_corner" "targetname" "v2" "origin" "1000 0 0" }
    "#;

    #[test]
    fn viper_and_strogg_ship_wait_hidden_on_their_path_until_used() {
        for class in ["misc_viper", "misc_strogg_ship"] {
            let (mut world, _) = spawned(&format!(
                r#"{VIPER_PATH} {{ "classname" "{class}" "target" "v1" "targetname" "go" }}"#
            ));
            let ship = one(&world, class);
            let e = world.entities.get(ship).unwrap();
            assert_eq!(
                (e.solid, e.game.movetype),
                (Solid::Not, MoveType::Push),
                "{class}"
            );
            assert_eq!(
                (e.mins, e.maxs),
                (Vec3f::new(-16.0, -16.0, 0.0), Vec3f::new(16.0, 16.0, 32.0))
            );
            assert_ne!(e.svflags & SVF_NOCLIENT, 0, "{class}");
            assert_eq!(e.game.speed, 300.0);
            let mi = e.moveinfo.as_ref().unwrap();
            assert_eq!((mi.speed, mi.accel, mi.decel), (300.0, 300.0, 300.0));
            assert!(think_is(&world, ship, func_train_find));
            assert!(thinks_after(&world, ship, FRAMETIME));
            assert!(use_is(&world, ship, misc_viper_use));
            world.run_frame();
            let origin = world.entities.get(ship).unwrap().state.origin;
            assert_eq!(origin, Vec3f::new(16.0, 16.0, 0.0), "mins on v1");
            world.run_frame();
            let e = world.entities.get(ship).unwrap();
            assert_eq!(e.velocity, Vec3f::ZERO, "targeted: parked");

            let player = world.spawn().unwrap();
            misc_viper_use(&mut world, ship, player, player);
            let e = world.entities.get(ship).unwrap();
            assert_eq!(e.svflags & SVF_NOCLIENT, 0, "{class}");
            assert!(use_is(&world, ship, train_use));
            let dir = e.moveinfo.as_ref().unwrap().dir;
            assert_eq!(dir, Vec3f::new(1.0, 0.0, 0.0));
            for _ in 0..3 {
                world.run_frame();
            }
            let x = world.entities.get(ship).unwrap().state.origin.x;
            assert!(x > 16.0 && x < 1016.0, "{class} flies towards v2: {x}");
        }
    }

    #[test]
    fn flyby_ships_keep_their_speed_and_need_a_target() {
        let (world, _) = spawned(&format!(
            r#"{VIPER_PATH} {{ "classname" "misc_viper" "target" "v1" "speed" "500" }}
               {{ "classname" "misc_strogg_ship" }} {{ "classname" "misc_viper" }}"#
        ));
        let viper = one(&world, "misc_viper");
        let mi = world
            .entities
            .get(viper)
            .unwrap()
            .moveinfo
            .as_ref()
            .unwrap();
        assert_eq!((mi.speed, mi.decel), (500.0, 500.0));
        assert!(world.find_by_classname(Some(viper), "misc_viper").is_none());
        assert!(world.find_by_classname(None, "misc_strogg_ship").is_none());
    }

    #[test]
    fn bigviper_is_a_large_solid_model() {
        let (world, _) = spawned(r#"{ "classname" "misc_bigviper" }"#);
        let e = world.entities.get(one(&world, "misc_bigviper")).unwrap();
        assert_eq!((e.solid, e.game.movetype), (Solid::Bbox, MoveType::None));
        assert_eq!(
            (e.mins, e.maxs),
            (
                Vec3f::new(-176.0, -120.0, -24.0),
                Vec3f::new(176.0, 120.0, 72.0)
            )
        );
    }

    #[test]
    fn viper_bomb_drops_along_the_vipers_heading_and_tips_over() {
        let (mut world, _) = spawned(
            r#"{ "classname" "misc_viper_bomb" "targetname" "b" }
               { "classname" "misc_viper" "target" "v" }"#,
        );
        let bomb = one(&world, "misc_viper_bomb");
        let e = world.entities.get(bomb).unwrap();
        assert_eq!((e.solid, e.game.dmg), (Solid::Not, 1000));
        assert_eq!((e.mins, e.maxs), (Vec3f::splat(-8.0), Vec3f::splat(8.0)));
        assert_ne!(e.svflags & SVF_NOCLIENT, 0);
        assert!(use_is(&world, bomb, misc_viper_bomb_use));
        let viper = one(&world, "misc_viper");
        let mi = world.entities.get_mut(viper).unwrap().moveinfo.as_mut();
        mi.unwrap().dir = Vec3f::new(0.0, 1.0, 0.0);
        world.level.time = 3.0;
        let player = world.spawn().unwrap();
        misc_viper_bomb_use(&mut world, bomb, player, player);
        let e = world.entities.get(bomb).unwrap();
        assert_eq!((e.solid, e.game.movetype), (Solid::Bbox, MoveType::Toss));
        assert_eq!(e.svflags & SVF_NOCLIENT, 0);
        assert_ne!(e.state.effects & EF_ROCKET, 0);
        assert!(e.use_fn.is_none() && e.touch.is_some() && e.prethink.is_some());
        assert_eq!(e.velocity, Vec3f::new(0.0, 300.0, 0.0));
        assert_eq!((e.game.timestamp, e.game.activator), (3.0, Some(player)));

        // Level along the heading at the drop, rolling 10 a frame.
        misc_viper_bomb_prethink(&mut world, bomb);
        let a = world.entities.get(bomb).unwrap().state.angles;
        assert_eq!(a, Vec3f::new(0.0, 90.0, 10.0));
        // A second or more on, it points straight down.
        world.level.time = 5.0;
        misc_viper_bomb_prethink(&mut world, bomb);
        let e = world.entities.get(bomb).unwrap();
        assert_eq!(e.state.angles, Vec3f::new(-270.0, 0.0, 20.0));
        assert!(e.game.ground_entity.is_none());
    }

    #[test]
    fn viper_bomb_explodes_at_floor_level_and_fires_its_targets() {
        let (mut world, gi) = spawned(
            r#"{ "classname" "misc_viper_bomb" "target" "boom" "dmg" "200" }
               { "classname" "func_clock" "targetname" "boom" "target" "x" "spawnflags" "4" }"#,
        );
        let bomb = one(&world, "misc_viper_bomb");
        let player = world.spawn().unwrap();
        misc_viper_bomb_use(&mut world, bomb, player, player);
        world.link_entity(bomb);
        let victim = world.spawn().unwrap();
        let v = world.entities.get_mut(victim).unwrap();
        v.game.health = 1000;
        v.game.takedamage = TakeDamage::Yes;
        v.solid = Solid::Bbox;
        v.state.origin = Vec3f::new(0.0, 0.0, -8.0);
        world.link_entity(victim);
        let clock = one(&world, "func_clock");
        misc_viper_bomb_touch(&mut world, bomb, victim, None, None);
        assert!(world.entities.get(victim).unwrap().game.health < 1000);
        assert!(!world.entities.is_live(bomb));
        let activator = world.entities.get(clock).unwrap().game.activator;
        assert_eq!(activator, Some(player), "targets fired by the bomber");
        // absmin.z is -8 - 1 (the link epsilon), so the bomb is at -8.
        let at = Vec3f::new(0.0, 0.0, -8.0);
        assert!(gi.vectors.borrow().contains(&at), "at absmin.z + 1");
    }

    #[test]
    fn viewthing_cycles_seven_frames_of_the_banner() {
        let (mut world, gi) = spawned(r#"{ "classname" "viewthing" }"#);
        let thing = one(&world, "viewthing");
        let e = world.entities.get(thing).unwrap();
        assert_eq!((e.solid, e.state.renderfx), (Solid::Bbox, RF_FRAMELERP));
        assert_eq!(
            (e.mins, e.maxs),
            (
                Vec3f::new(-16.0, -16.0, -24.0),
                Vec3f::new(16.0, 16.0, 32.0)
            )
        );
        let model = "models/objects/banner/tris.md2";
        assert!(gi.models.borrow().iter().any(|m| m == model));
        assert!(gi
            .dprints
            .borrow()
            .iter()
            .any(|m| m == "viewthing spawned\n"));
        assert!(thinks_after(&world, thing, 0.5));
        set_frame(&mut world, thing, 6);
        run_think(&mut world, thing);
        assert_eq!(frame(&world, thing), 0);
        assert!(thinks_after(&world, thing, FRAMETIME));
    }
}
