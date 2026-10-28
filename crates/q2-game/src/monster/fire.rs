//! Monster weapon fire: each fires through the player weapon code and
//! tells clients to draw the monster's muzzle flash.
//!
//! # C source reference
//! `~/Qwasm2/src/game/g_monster.c` (`monster_fire_*`).

use q2_shared::protocol::SvcOp;
use q2_shared::types::{Multicast, Vec3f};

use crate::constants::MeansOfDeath;
use crate::entity::EntityKey;
use crate::world::GameWorld;

/// Tell clients in the PVS of `start` to draw `self_key`'s muzzle flash
/// `flashtype` (an `MZ2_*` number). The tail every C `monster_fire_*`
/// shares.
fn muzzle_flash2(world: &GameWorld, self_key: EntityKey, start: Vec3f, flashtype: i32) {
    let number = world.number_of(self_key).map_or(0, |n| n as i32);
    world.gi.write_byte(SvcOp::MuzzleFlash2 as i32);
    world.gi.write_short(number);
    world.gi.write_byte(flashtype);
    world.gi.multicast(start, Multicast::PVS);
}

/// C ref: `monster_fire_bullet` (g_monster.c).
#[allow(clippy::too_many_arguments)]
pub fn monster_fire_bullet(
    world: &mut GameWorld,
    self_key: EntityKey,
    start: Vec3f,
    dir: Vec3f,
    damage: i32,
    kick: i32,
    hspread: i32,
    vspread: i32,
    flashtype: i32,
) {
    world.fire_bullet(
        self_key,
        start,
        dir,
        damage,
        kick,
        hspread,
        vspread,
        MeansOfDeath::Unknown,
    );
    muzzle_flash2(world, self_key, start, flashtype);
}

/// C ref: `monster_fire_shotgun` (g_monster.c).
#[allow(clippy::too_many_arguments)]
pub fn monster_fire_shotgun(
    world: &mut GameWorld,
    self_key: EntityKey,
    start: Vec3f,
    aimdir: Vec3f,
    damage: i32,
    kick: i32,
    hspread: i32,
    vspread: i32,
    count: i32,
    flashtype: i32,
) {
    world.fire_shotgun(
        self_key,
        start,
        aimdir,
        damage,
        kick,
        hspread,
        vspread,
        count,
        MeansOfDeath::Unknown,
    );
    muzzle_flash2(world, self_key, start, flashtype);
}

/// C ref: `monster_fire_blaster` (g_monster.c). `effect` is the bolt's
/// `EF_*` flags; monster bolts are never the hyperblaster kind.
#[allow(clippy::too_many_arguments)]
pub fn monster_fire_blaster(
    world: &mut GameWorld,
    self_key: EntityKey,
    start: Vec3f,
    dir: Vec3f,
    damage: i32,
    speed: i32,
    flashtype: i32,
    effect: u32,
) {
    world.fire_blaster(self_key, start, dir, damage, speed, effect, false);
    muzzle_flash2(world, self_key, start, flashtype);
}

/// C ref: `monster_fire_grenade` (g_monster.c): a 2.5 s fuse and a blast
/// radius of `damage + 40`.
pub fn monster_fire_grenade(
    world: &mut GameWorld,
    self_key: EntityKey,
    start: Vec3f,
    aimdir: Vec3f,
    damage: i32,
    speed: i32,
    flashtype: i32,
) {
    world.fire_grenade(
        self_key,
        start,
        aimdir,
        damage,
        speed,
        2.5,
        (damage + 40) as f32,
    );
    muzzle_flash2(world, self_key, start, flashtype);
}

/// C ref: `monster_fire_rocket` (g_monster.c): blast radius `damage + 20`,
/// splash damage `damage`.
pub fn monster_fire_rocket(
    world: &mut GameWorld,
    self_key: EntityKey,
    start: Vec3f,
    dir: Vec3f,
    damage: i32,
    speed: i32,
    flashtype: i32,
) {
    world.fire_rocket(
        self_key,
        start,
        dir,
        damage,
        speed,
        (damage + 20) as f32,
        damage,
    );
    muzzle_flash2(world, self_key, start, flashtype);
}

/// C ref: `monster_fire_railgun` (g_monster.c).
pub fn monster_fire_railgun(
    world: &mut GameWorld,
    self_key: EntityKey,
    start: Vec3f,
    aimdir: Vec3f,
    damage: i32,
    kick: i32,
    flashtype: i32,
) {
    world.fire_rail(self_key, start, aimdir, damage, kick);
    muzzle_flash2(world, self_key, start, flashtype);
}

/// C ref: `monster_fire_bfg` (g_monster.c). `kick` is unused, as in C.
#[allow(clippy::too_many_arguments)]
pub fn monster_fire_bfg(
    world: &mut GameWorld,
    self_key: EntityKey,
    start: Vec3f,
    aimdir: Vec3f,
    damage: i32,
    speed: i32,
    _kick: i32,
    damage_radius: f32,
    flashtype: i32,
) {
    world.fire_bfg(self_key, start, aimdir, damage, speed, damage_radius);
    muzzle_flash2(world, self_key, start, flashtype);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::*;
    use crate::misc::wire::{wire_world, Wire, WireImport};
    use crate::traits::GameImport;
    use q2_shared::constants::{EF_BLASTER, MZ2_SOLDIER_BLASTER_1};
    use q2_shared::protocol::SvcOp;
    use q2_shared::types::*;

    const START: Vec3f = Vec3f::new(10.0, 20.0, 30.0);
    const DIR: Vec3f = Vec3f::new(1.0, 0.0, 0.0);

    fn shooter() -> (GameWorld, std::rc::Rc<WireImport>, EntityKey) {
        let (mut world, gi) = wire_world();
        world.level.time = 1.0;
        let monster = world.spawn().unwrap();
        world.entities.get_mut(monster).unwrap().state.origin = Vec3f::new(0.0, 20.0, 30.0);
        (world, gi, monster)
    }

    /// C: `svc_muzzleflash2`, the monster's number, the flash, PVS of `start`.
    fn assert_muzzle_flash(world: &GameWorld, gi: &WireImport, monster: EntityKey, flash: i32) {
        let number = world.number_of(monster).unwrap() as i32;
        let wire = gi.wire.borrow();
        assert_eq!(
            wire[wire.len() - 4..],
            [
                Wire::Byte(SvcOp::MuzzleFlash2 as i32),
                Wire::Short(number),
                Wire::Byte(flash),
                Wire::Multicast(START, Multicast::PVS),
            ]
        );
    }

    fn projectile(world: &GameWorld, classname: &str) -> crate::entity::Entity {
        let key = world.find_by_classname(None, classname).unwrap();
        world.entities.get(key).unwrap().clone()
    }

    /// A damageable target the next shot trace hits, after a clear muzzle
    /// check.
    fn target_in_line(world: &mut GameWorld, gi: &WireImport) -> EntityKey {
        let victim = world.spawn().unwrap();
        {
            let e = world.entities.get_mut(victim).unwrap();
            e.game.health = 100;
            e.game.takedamage = TakeDamage::Yes;
            e.state.origin = Vec3f::new(200.0, 20.0, 30.0);
        }
        let clear = gi.inner.trace(START, START, START, START, None, 0);
        let hit = Trace {
            fraction: 0.5,
            ent_index: world.number_of(victim),
            endpos: Vec3f::new(200.0, 20.0, 30.0),
            ..clear.clone()
        };
        gi.inner.trace_masks.borrow_mut().clear();
        gi.inner.trace_pass_ents.borrow_mut().clear();
        gi.inner.queued_traces.borrow_mut().extend([clear, hit]);
        victim
    }

    #[test]
    fn monster_fire_bullet_hits_for_damage_and_flashes() {
        let (mut world, gi, monster) = shooter();
        let victim = target_in_line(&mut world, &gi);
        monster_fire_bullet(&mut world, monster, START, DIR, 4, 2, 0, 0, 7);
        assert_eq!(world.entities.get(victim).unwrap().game.health, 96);
        assert_muzzle_flash(&world, &gi, monster, 7);
    }

    #[test]
    fn monster_fire_shotgun_fires_count_pellets_and_flashes() {
        let (mut world, gi, monster) = shooter();
        monster_fire_shotgun(&mut world, monster, START, DIR, 2, 1, 500, 500, 9, 8);
        // Each pellet: the muzzle check, then the shot.
        assert_eq!(gi.inner.trace_masks.borrow().len(), 2 * 9);
        assert_muzzle_flash(&world, &gi, monster, 8);
    }

    #[test]
    fn monster_fire_blaster_launches_a_non_hyper_bolt_and_flashes() {
        let (mut world, gi, monster) = shooter();
        monster_fire_blaster(
            &mut world,
            monster,
            START,
            DIR,
            5,
            600,
            MZ2_SOLDIER_BLASTER_1,
            EF_BLASTER,
        );
        let bolt = projectile(&world, "bolt");
        assert_eq!(bolt.game.dmg, 5);
        assert_eq!(bolt.velocity, DIR * 600.0);
        assert_eq!(bolt.state.effects & EF_BLASTER, EF_BLASTER);
        assert_eq!(bolt.game.spawnflags, 0, "C passes hyper = false");
        assert_eq!(bolt.owner, Some(monster));
        assert_muzzle_flash(&world, &gi, monster, MZ2_SOLDIER_BLASTER_1);
    }

    #[test]
    fn monster_fire_grenade_uses_a_2_5_second_fuse_and_damage_plus_40_radius() {
        let (mut world, gi, monster) = shooter();
        monster_fire_grenade(&mut world, monster, START, DIR, 50, 600, 12);
        let gren = projectile(&world, "grenade");
        assert_eq!(gren.game.dmg, 50);
        assert_eq!(gren.game.dmg_radius, 90.0);
        assert_eq!(gren.game.nextthink, 1.0 + 2.5);
        assert_eq!(gren.velocity, DIR * 600.0 + Vec3f::new(0.0, 0.0, 200.0));
        assert_muzzle_flash(&world, &gi, monster, 12);
    }

    #[test]
    fn monster_fire_rocket_uses_damage_plus_20_radius_and_damage_as_splash() {
        let (mut world, gi, monster) = shooter();
        monster_fire_rocket(&mut world, monster, START, DIR, 50, 500, 13);
        let rocket = projectile(&world, "rocket");
        assert_eq!(rocket.game.dmg, 50);
        assert_eq!(rocket.game.dmg_radius, 70.0);
        assert_eq!(rocket.game.radius_dmg, 50);
        assert_eq!(rocket.velocity, DIR * 500.0);
        assert_muzzle_flash(&world, &gi, monster, 13);
    }

    #[test]
    fn monster_fire_railgun_hits_for_damage_and_flashes() {
        let (mut world, gi, monster) = shooter();
        let victim = target_in_line(&mut world, &gi);
        // The rail's first trace is the shot itself.
        gi.inner.queued_traces.borrow_mut().pop_front();
        monster_fire_railgun(&mut world, monster, START, DIR, 50, 100, 14);
        assert_eq!(world.entities.get(victim).unwrap().game.health, 50);
        assert_muzzle_flash(&world, &gi, monster, 14);
    }

    #[test]
    fn monster_fire_bfg_passes_damage_radius_and_ignores_kick() {
        let (mut world, gi, monster) = shooter();
        monster_fire_bfg(&mut world, monster, START, DIR, 50, 300, 100, 1000.0, 101);
        let bfg = projectile(&world, "bfg blast");
        assert_eq!(bfg.game.radius_dmg, 50);
        assert_eq!(bfg.game.dmg_radius, 1000.0);
        assert_eq!(bfg.velocity, DIR * 300.0);
        assert_muzzle_flash(&world, &gi, monster, 101);
    }
}
