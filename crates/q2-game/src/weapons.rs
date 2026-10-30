//! Weapon fire system — hitscan traces, projectile spawning, impact handlers.
//!
//! Port of the fire functions of `g_weapon.c`. Damage values, speeds and
//! models match C. Not ported: temp-entity effects (impacts, trails,
//! explosions), sounds, `check_dodge`, water handling in hitscan traces, the
//! SURF_SKY checks, and the BFG laser think and `bfg_explode` frames.
//! Nothing outside tests fires yet: the player weapon state machine is #16.
//! Until it lands, the fire functions make p_weapon.c's PNOISE_WEAPON
//! `PlayerNoise` at the muzzle when a client fires (a no-op for monsters).
//!
//! # Weapon types
//! - **Hitscan**: fire_bullet, fire_shotgun, fire_rail, fire_hit — instant trace
//! - **Projectile**: fire_blaster, fire_rocket, fire_grenade, fire_bfg — spawns entity
//!
//! # C source reference
//! `~/Qwasm2/src/game/g_weapon.c`

use q2_shared::constants::{EF_ANIM_ALLFAST, EF_BFG, EF_GRENADE, EF_ROCKET};
use q2_shared::types::*;

use crate::constants::*;
use crate::entity::EntityKey;
use crate::physics::angle_vectors;
use crate::player::weapon::PlayerNoise;
use crate::world::GameWorld;

// ---------------------------------------------------------------------------
// Projectile touch callbacks
// ---------------------------------------------------------------------------

/// Blaster bolt impact handler. C ref: `blaster_touch` (g_weapon.c:378-445).
pub fn blaster_touch(
    world: &mut GameWorld,
    self_key: EntityKey,
    other_key: EntityKey,
    _plane: Option<&Plane>,
    _surface: Option<&Surface>,
) {
    // Don't hit owner.
    let owner = world.entities.get(self_key).and_then(|e| e.owner);
    if owner == Some(other_key) {
        return;
    }
    if let (Some(owner), Some(origin)) =
        (owner, world.entities.get(self_key).map(|e| e.state.origin))
    {
        world.player_noise(owner, origin, PlayerNoise::Impact);
    }

    let (dmg, mod_type) = {
        let Some(ent) = world.entities.get(self_key) else {
            return;
        };
        let m = if ent.game.spawnflags & 1 != 0 {
            MeansOfDeath::Hyperblaster
        } else {
            MeansOfDeath::Blaster
        };
        (ent.game.dmg, m)
    };

    // Apply damage if target takes damage.
    let takes_damage = world
        .entities
        .get(other_key)
        .map(|e| e.game.takedamage != TakeDamage::No)
        .unwrap_or(false);

    if takes_damage {
        let dir = world
            .entities
            .get(self_key)
            .map(|e| e.velocity.normalize_or_zero())
            .unwrap_or_default();
        world.t_damage(
            other_key,
            self_key,
            owner.unwrap_or(self_key),
            dir,
            world
                .entities
                .get(self_key)
                .map(|e| e.state.origin)
                .unwrap_or_default(),
            Vec3f::ZERO,
            dmg,
            1,
            DamageFlags::ENERGY,
            mod_type,
        );
    }

    // Remove the bolt.
    world.free_entity(self_key);
}

/// Rocket impact handler. C ref: `rocket_touch` (g_weapon.c:746-827).
pub fn rocket_touch(
    world: &mut GameWorld,
    self_key: EntityKey,
    other_key: EntityKey,
    _plane: Option<&Plane>,
    _surface: Option<&Surface>,
) {
    let owner = world.entities.get(self_key).and_then(|e| e.owner);
    if owner == Some(other_key) {
        return;
    }

    let (dmg, radius_dmg, dmg_radius, origin) = {
        let Some(ent) = world.entities.get(self_key) else {
            return;
        };
        (
            ent.game.dmg,
            ent.game.radius_dmg,
            ent.game.dmg_radius,
            ent.state.origin,
        )
    };
    if let Some(owner) = owner {
        world.player_noise(owner, origin, PlayerNoise::Impact);
    }

    // Direct damage to hit entity.
    let takes_damage = world
        .entities
        .get(other_key)
        .map(|e| e.game.takedamage != TakeDamage::No)
        .unwrap_or(false);

    if takes_damage {
        let dir = world
            .entities
            .get(self_key)
            .map(|e| e.velocity.normalize_or_zero())
            .unwrap_or_default();
        world.t_damage(
            other_key,
            self_key,
            owner.unwrap_or(self_key),
            dir,
            origin,
            Vec3f::ZERO,
            dmg,
            0,
            DamageFlags::empty(),
            MeansOfDeath::Rocket,
        );
    }

    // Radius damage to nearby entities.
    world.t_radius_damage(
        self_key,
        owner.unwrap_or(self_key),
        radius_dmg as f32,
        Some(other_key),
        dmg_radius,
        MeansOfDeath::RocketSplash,
    );

    // Remove the rocket.
    world.free_entity(self_key);
}

/// Grenade explosion — called when timer expires or on direct entity hit.
/// The entity a grenade touched (`enemy`) takes one radius-style hit and
/// is left out of the splash. C ref: `Grenade_Explode` (g_weapon.c:510-596).
pub fn grenade_explode(world: &mut GameWorld, self_key: EntityKey) {
    let (dmg, dmg_radius, owner, origin, enemy) = {
        let Some(ent) = world.entities.get(self_key) else {
            return;
        };
        (
            ent.game.dmg,
            ent.game.dmg_radius,
            ent.owner,
            ent.state.origin,
            ent.game.enemy,
        )
    };
    if let Some(owner) = owner {
        world.player_noise(owner, origin, PlayerNoise::Impact);
    }
    let attacker = owner.unwrap_or(self_key);

    let enemy_box = enemy
        .and_then(|k| world.entities.get(k))
        .map(|e| (e.state.origin, e.mins, e.maxs));
    if let (Some(enemy), Some((enemy_origin, mins, maxs))) = (enemy, enemy_box) {
        let center = enemy_origin + (mins + maxs) * 0.5;
        let points = dmg as f32 - 0.5 * (origin - center).length();
        world.t_damage(
            enemy,
            self_key,
            attacker,
            enemy_origin - origin,
            origin,
            Vec3f::ZERO,
            points as i32,
            points as i32,
            DamageFlags::RADIUS,
            MeansOfDeath::Grenade,
        );
    }

    world.t_radius_damage(
        self_key,
        attacker,
        dmg as f32,
        enemy,
        dmg_radius,
        MeansOfDeath::GrenadeSplash,
    );

    world.free_entity(self_key);
}

/// Grenade think callback — wraps `grenade_explode` for timer detonation.
fn grenade_think(world: &mut GameWorld, self_key: EntityKey) {
    grenade_explode(world, self_key);
}

/// Grenade touch handler — bounces off world, explodes on entities.
/// C ref: `Grenade_Touch` (g_weapon.c:598-644).
pub fn grenade_touch(
    world: &mut GameWorld,
    self_key: EntityKey,
    other_key: EntityKey,
    _plane: Option<&Plane>,
    _surface: Option<&Surface>,
) {
    let owner = world.entities.get(self_key).and_then(|e| e.owner);
    if owner == Some(other_key) {
        return;
    }

    let takes_damage = world
        .entities
        .get(other_key)
        .map(|e| e.game.takedamage != TakeDamage::No)
        .unwrap_or(false);
    if !takes_damage {
        return; // bounce (MOVETYPE_BOUNCE physics)
    }

    if let Some(ent) = world.entities.get_mut(self_key) {
        ent.game.enemy = Some(other_key);
    }
    grenade_explode(world, self_key);
}

/// BFG impact: the core explosion. C ref: `bfg_touch` (g_weapon.c). The
/// follow-up `bfg_explode` frames that damage everything in `dmg_radius`
/// are not ported, so the blast is removed at once.
pub fn bfg_touch(
    world: &mut GameWorld,
    self_key: EntityKey,
    other_key: EntityKey,
    plane: Option<&Plane>,
    _surface: Option<&Surface>,
) {
    let Some(ent) = world.entities.get(self_key) else {
        return;
    };
    let owner = ent.owner;
    if owner == Some(other_key) {
        return;
    }
    let (velocity, origin) = (ent.velocity, ent.state.origin);
    if let Some(owner) = owner {
        world.player_noise(owner, origin, PlayerNoise::Impact);
    }
    let attacker = owner.unwrap_or(self_key);

    let takes_damage = world
        .entities
        .get(other_key)
        .is_some_and(|e| e.game.takedamage != TakeDamage::No);
    if takes_damage {
        world.t_damage(
            other_key,
            self_key,
            attacker,
            velocity,
            origin,
            plane.map_or(Vec3f::ZERO, |p| p.normal),
            200,
            0,
            DamageFlags::empty(),
            MeansOfDeath::BfgBlast,
        );
    }
    world.t_radius_damage(
        self_key,
        attacker,
        200.0,
        Some(other_key),
        100.0,
        MeansOfDeath::BfgBlast,
    );

    world.free_entity(self_key);
}

/// C `8000 / speed`, an int division. C divides by zero on a zero
/// speed; this port gives that projectile 8000 seconds instead.
fn projectile_lifetime(speed: i32) -> f32 {
    8000_i32.checked_div(speed).unwrap_or(8000) as f32
}

/// Timeout cleanup — remove projectile entities that outlived their duration.
fn projectile_timeout(world: &mut GameWorld, self_key: EntityKey) {
    world.free_entity(self_key);
}

// ---------------------------------------------------------------------------
// Weapon fire functions
// ---------------------------------------------------------------------------

impl GameWorld {
    /// The C fields every projectile gets: model, facing and clip mask.
    fn launch_projectile(&mut self, key: EntityKey, dir: Vec3f, model: &str) {
        if let Some(ent) = self.entities.get_mut(key) {
            ent.state.angles = vectoangles(dir);
            ent.clipmask = MASK_SHOT;
        }
        self.set_model(key, model);
    }

    /// Fire a blaster bolt. Creates a projectile entity.
    /// C ref: `fire_blaster` (g_weapon.c:447-508).
    ///
    /// `effect`: the bolt's `EF_*` flags (`EF_BLASTER`, `EF_HYPERBLASTER`
    /// or 0). `hyper`: true for hyperblaster variant (different MOD).
    #[allow(clippy::too_many_arguments)]
    pub fn fire_blaster(
        &mut self,
        owner: EntityKey,
        start: Vec3f,
        dir: Vec3f,
        damage: i32,
        speed: i32,
        effect: u32,
        hyper: bool,
    ) {
        let dir = dir.normalize_or_zero();
        let Some(bolt_key) = self.spawn() else {
            return;
        };

        if let Some(bolt) = self.entities.get_mut(bolt_key) {
            // C: a dead monster, so player prediction does not clip
            // against bolts.
            bolt.svflags = q2_shared::constants::SVF_DEADMONSTER;
            bolt.state.renderfx |= q2_shared::constants::RF_NOSHADOW;
            bolt.game.classname = "bolt".into();
            bolt.game.movetype = MoveType::FlyMissile;
            bolt.solid = Solid::Bbox;
            bolt.owner = Some(owner);
            bolt.game.dmg = damage;
            bolt.game.spawnflags = if hyper { 1 } else { 0 };
            bolt.state.effects |= effect;

            bolt.state.origin = start;
            bolt.state.old_origin = start;
            bolt.velocity = dir * speed as f32;

            bolt.touch = Some(blaster_touch);
            bolt.think = Some(projectile_timeout);
            bolt.game.nextthink = self.level.time + 2.0;
        }
        self.launch_projectile(bolt_key, dir, "models/objects/laser/tris.md2");

        self.link_entity(bolt_key);
        self.player_noise(owner, start, PlayerNoise::Weapon);

        // A muzzle behind a wall or inside a target hits it at once.
        let Some(owner_origin) = self.entities.get(owner).map(|e| e.state.origin) else {
            return;
        };
        let trace = self.gi.trace(
            owner_origin,
            Vec3f::ZERO,
            Vec3f::ZERO,
            start,
            self.number_of(bolt_key),
            MASK_SHOT,
        );
        if trace.fraction < 1.0 {
            if let Some(bolt) = self.entities.get_mut(bolt_key) {
                bolt.state.origin -= dir * 10.0;
            }
            // C passes tr.ent, which is the world when no entity was hit.
            let other = trace.ent_index.unwrap_or(0);
            if let Some(other) = self.entities.key_for_number(other) {
                blaster_touch(self, bolt_key, other, None, None);
            }
        }
    }

    /// Fire a single bullet (machinegun/chaingun round).
    /// C ref: `fire_bullet` (g_weapon.c:338-349), via `fire_lead`.
    ///
    /// Hitscan — traces immediately, no projectile entity created.
    #[allow(clippy::too_many_arguments)]
    pub fn fire_bullet(
        &mut self,
        owner: EntityKey,
        start: Vec3f,
        dir: Vec3f,
        damage: i32,
        kick: i32,
        hspread: i32,
        vspread: i32,
        means_of_death: MeansOfDeath,
    ) {
        self.fire_lead(
            owner,
            start,
            dir,
            damage,
            kick,
            hspread,
            vspread,
            means_of_death,
        );
        self.player_noise(owner, start, PlayerNoise::Weapon);
    }

    /// C `fire_lead` without the water re-trace, splashes and impact
    /// effects.
    #[allow(clippy::too_many_arguments)]
    fn fire_lead(
        &mut self,
        owner: EntityKey,
        start: Vec3f,
        dir: Vec3f,
        damage: i32,
        kick: i32,
        hspread: i32,
        vspread: i32,
        means_of_death: MeansOfDeath,
    ) {
        let Some(owner_origin) = self.entities.get(owner).map(|e| e.state.origin) else {
            return;
        };
        let pass = self.number_of(owner);
        // A muzzle inside a wall hits the wall, not what is beyond it.
        let mut trace = self.gi.trace(
            owner_origin,
            Vec3f::ZERO,
            Vec3f::ZERO,
            start,
            pass,
            MASK_SHOT,
        );
        if trace.fraction >= 1.0 {
            let (forward, right, up) = angle_vectors(vectoangles(dir));
            let r = self.crandom() * hspread as f32;
            let u = self.crandom() * vspread as f32;
            let end = start + forward * 8192.0 + right * r + up * u;
            trace = self
                .gi
                .trace(start, Vec3f::ZERO, Vec3f::ZERO, end, pass, MASK_SHOT);
        }
        if trace.fraction >= 1.0 {
            return;
        }
        let Some(hit) = trace
            .ent_index
            .and_then(|n| self.entities.key_for_number(n))
        else {
            return;
        };
        let takes_damage = self
            .entities
            .get(hit)
            .is_some_and(|e| e.game.takedamage != TakeDamage::No);
        if takes_damage {
            self.t_damage(
                hit,
                owner,
                owner,
                dir,
                trace.endpos,
                trace.plane.normal,
                damage,
                kick,
                DamageFlags::BULLET,
                means_of_death,
            );
        } else {
            self.player_noise(owner, trace.endpos, PlayerNoise::Impact);
        }
    }

    /// Fire shotgun pellets. Fires `count` individual bullet traces.
    /// C ref: `fire_shotgun` (g_weapon.c:355-371).
    #[allow(clippy::too_many_arguments)]
    pub fn fire_shotgun(
        &mut self,
        owner: EntityKey,
        start: Vec3f,
        dir: Vec3f,
        damage: i32,
        kick: i32,
        hspread: i32,
        vspread: i32,
        count: i32,
        means_of_death: MeansOfDeath,
    ) {
        for _ in 0..count.max(0) {
            self.fire_lead(
                owner,
                start,
                dir,
                damage,
                kick,
                hspread,
                vspread,
                means_of_death,
            );
        }
        self.player_noise(owner, start, PlayerNoise::Weapon);
    }

    /// Fire a rocket. Creates a projectile entity.
    /// C ref: `fire_rocket` (g_weapon.c:829-868).
    #[allow(clippy::too_many_arguments)]
    pub fn fire_rocket(
        &mut self,
        owner: EntityKey,
        start: Vec3f,
        dir: Vec3f,
        damage: i32,
        speed: i32,
        damage_radius: f32,
        radius_damage: i32,
    ) {
        let Some(rocket_key) = self.spawn() else {
            return;
        };

        if let Some(rocket) = self.entities.get_mut(rocket_key) {
            rocket.game.classname = "rocket".into();
            rocket.game.movetype = MoveType::FlyMissile;
            rocket.solid = Solid::Bbox;
            rocket.owner = Some(owner);
            rocket.game.dmg = damage;
            rocket.game.radius_dmg = radius_damage;
            rocket.game.dmg_radius = damage_radius;
            rocket.state.effects |= EF_ROCKET;

            rocket.state.origin = start;
            rocket.velocity = dir.normalize_or_zero() * speed as f32;

            rocket.touch = Some(rocket_touch);
            rocket.think = Some(projectile_timeout);
            rocket.game.nextthink = self.level.time + projectile_lifetime(speed);
        }
        self.launch_projectile(rocket_key, dir, "models/objects/rocket/tris.md2");

        self.link_entity(rocket_key);
        self.player_noise(owner, start, PlayerNoise::Weapon);
    }

    /// Fire a grenade. Creates a bouncing projectile entity.
    /// C ref: `fire_grenade` (g_weapon.c:646-684). C adds `crandom() * 10`
    /// jitter to the upward and sideways push; there is no game RNG, so
    /// the push is the C mean.
    #[allow(clippy::too_many_arguments)]
    pub fn fire_grenade(
        &mut self,
        owner: EntityKey,
        start: Vec3f,
        dir: Vec3f,
        damage: i32,
        speed: i32,
        timer: f32,
        damage_radius: f32,
    ) {
        let Some(gren_key) = self.spawn() else {
            return;
        };

        let (_, _, up) = angle_vectors(vectoangles(dir));

        if let Some(gren) = self.entities.get_mut(gren_key) {
            gren.game.classname = "grenade".into();
            gren.game.movetype = MoveType::Bounce;
            gren.solid = Solid::Bbox;
            gren.owner = Some(owner);
            gren.game.dmg = damage;
            gren.game.dmg_radius = damage_radius;
            gren.state.effects |= EF_GRENADE;

            gren.state.origin = start;
            gren.velocity = dir * speed as f32 + up * 200.0;
            // Spin.
            gren.avelocity = Vec3f::new(300.0, 300.0, 300.0);

            gren.touch = Some(grenade_touch);
            gren.think = Some(grenade_think);
            gren.game.nextthink = self.level.time + timer;
        }
        self.launch_projectile(gren_key, dir, "models/objects/grenade/tris.md2");

        self.link_entity(gren_key);
        self.player_noise(owner, start, PlayerNoise::Weapon);
    }

    /// Fire a railgun shot. Instant hitscan that pierces monsters, clients
    /// and other bbox entities. C ref: `fire_rail` (g_weapon.c:870-946),
    /// without the slime/lava pass and the trail effect.
    pub fn fire_rail(
        &mut self,
        owner: EntityKey,
        start: Vec3f,
        dir: Vec3f,
        damage: i32,
        kick: i32,
    ) {
        let end = start + dir * 8192.0;
        let mut from = start;
        let mut ignore = Some(owner);
        self.player_noise(owner, start, PlayerNoise::Weapon);

        while let Some(pass) = ignore {
            let trace = self.gi.trace(
                from,
                Vec3f::ZERO,
                Vec3f::ZERO,
                end,
                self.number_of(pass),
                MASK_SHOT,
            );
            from = trace.endpos;
            ignore = None;
            if trace.fraction >= 1.0 {
                break;
            }
            let Some(hit) = trace
                .ent_index
                .and_then(|n| self.entities.key_for_number(n))
            else {
                break;
            };
            let Some(hit_ent) = self.entities.get(hit) else {
                break;
            };
            let pierces = hit_ent.svflags & SvFlags::MONSTER.bits() != 0
                || hit_ent.client.is_some()
                || hit_ent.solid == Solid::Bbox;
            if hit == owner || hit_ent.game.takedamage == TakeDamage::No {
                break;
            }
            if pierces {
                ignore = Some(hit);
            }
            self.t_damage(
                hit,
                owner,
                owner,
                dir,
                trace.endpos,
                trace.plane.normal,
                damage,
                kick,
                DamageFlags::empty(),
                MeansOfDeath::Railgun,
            );
        }
        self.player_noise(owner, from, PlayerNoise::Impact);
    }

    /// Fire a BFG projectile. C ref: `fire_bfg` (g_weapon.c:1188-1231),
    /// without the laser think (the blast flies until it hits or times out).
    pub fn fire_bfg(
        &mut self,
        owner: EntityKey,
        start: Vec3f,
        dir: Vec3f,
        damage: i32,
        speed: i32,
        damage_radius: f32,
    ) {
        let Some(bfg_key) = self.spawn() else {
            return;
        };

        if let Some(bfg) = self.entities.get_mut(bfg_key) {
            bfg.game.classname = "bfg blast".into();
            bfg.game.movetype = MoveType::FlyMissile;
            bfg.solid = Solid::Bbox;
            bfg.owner = Some(owner);
            bfg.game.dmg = 200; // Core explosion damage is hardcoded.
            bfg.game.radius_dmg = damage;
            bfg.game.dmg_radius = damage_radius;
            bfg.state.effects |= EF_BFG | EF_ANIM_ALLFAST;

            bfg.state.origin = start;
            bfg.velocity = dir.normalize_or_zero() * speed as f32;

            bfg.touch = Some(bfg_touch);
            // C then swaps this timeout for bfg_think every frame, so the
            // ball only dies on touch. Without bfg_think (#33) the ball
            // times out like a rocket.
            bfg.think = Some(projectile_timeout);
            bfg.game.nextthink = self.level.time + projectile_lifetime(speed);
        }
        self.launch_projectile(bfg_key, dir, "sprites/s_bfg1.sp2");

        self.link_entity(bfg_key);
        self.player_noise(owner, start, PlayerNoise::Weapon);
    }

    /// Melee attack on `owner`'s enemy. `aim` is (range, right offset, up
    /// offset). Returns whether a monster or client was hit.
    /// C ref: `fire_hit` (g_weapon.c:71-167).
    pub fn fire_hit(&mut self, owner: EntityKey, aim: Vec3f, damage: i32, kick: i32) -> bool {
        let Some(me) = self.entities.get(owner) else {
            return false;
        };
        let (origin, angles, my_mins, my_maxs) =
            (me.state.origin, me.state.angles, me.mins, me.maxs);
        let Some(enemy) = me.game.enemy else {
            return false;
        };
        let Some(en) = self.entities.get(enemy) else {
            return false;
        };
        let (enemy_origin, enemy_mins, enemy_maxs) = (en.state.origin, en.mins, en.maxs);

        let dir = enemy_origin - origin;
        let mut range = dir.length();
        if range > aim.x {
            return false;
        }
        let mut aim = aim;
        if aim.y > my_mins.x && aim.y < my_maxs.x {
            // Straight on: back the range up to the edge of their box.
            range -= enemy_maxs.x;
        } else {
            // Side hit: move the right offset out to the edge of their box.
            aim.y = if aim.y < 0.0 {
                enemy_mins.x
            } else {
                enemy_maxs.x
            };
        }

        let point = origin + dir * range;
        let trace = self.gi.trace(
            origin,
            Vec3f::ZERO,
            Vec3f::ZERO,
            point,
            self.number_of(owner),
            MASK_SHOT,
        );
        // C: an unobstructed trace reports the world, which takes no damage.
        let mut hit = None;
        if trace.fraction < 1.0 {
            let Some(target) = trace
                .ent_index
                .and_then(|n| self.entities.key_for_number(n))
            else {
                return false;
            };
            let Some(t) = self.entities.get(target) else {
                return false;
            };
            if t.game.takedamage == TakeDamage::No {
                return false;
            }
            // Hitting any client or monster hits the one we wanted.
            hit = Some(
                if t.svflags & SvFlags::MONSTER.bits() != 0 || t.client.is_some() {
                    enemy
                } else {
                    target
                },
            );
        }

        let (forward, right, up) = angle_vectors(angles);
        let point = origin + forward * range + right * aim.y + up * aim.z;
        let Some(hit) = hit else {
            return false;
        };
        self.t_damage(
            hit,
            owner,
            owner,
            point - enemy_origin,
            point,
            Vec3f::ZERO,
            damage,
            kick / 2,
            DamageFlags::NO_KNOCKBACK,
            MeansOfDeath::Hit,
        );
        let Some(h) = self.entities.get(hit) else {
            return false;
        };
        if h.svflags & SvFlags::MONSTER.bits() == 0 && h.client.is_none() {
            return false;
        }

        // Our special form of knockback.
        let Some(en) = self.entities.get_mut(enemy) else {
            return false;
        };
        let center = en.absmin + en.size * 0.5;
        en.velocity += (center - point).normalize_or_zero() * kick as f32;
        if en.velocity.z > 0.0 {
            en.game.ground_entity = None;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// C `vectoangles` (g_utils.c), including its truncation to whole degrees.
pub(crate) fn vectoangles(v: Vec3f) -> Vec3f {
    let (yaw, pitch) = if v.x == 0.0 && v.y == 0.0 {
        (0.0, if v.z > 0.0 { 90.0 } else { 270.0 })
    } else {
        let mut yaw = if v.x != 0.0 {
            (v.y.atan2(v.x).to_degrees() as i32) as f32
        } else if v.y > 0.0 {
            90.0
        } else {
            -90.0
        };
        if yaw < 0.0 {
            yaw += 360.0;
        }
        let forward = (v.x * v.x + v.y * v.y).sqrt();
        let mut pitch = (v.z.atan2(forward).to_degrees() as i32) as f32;
        if pitch < 0.0 {
            pitch += 360.0;
        }
        (yaw, pitch)
    };
    Vec3f::new(-pitch, yaw, 0.0)
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::test_world;

    #[test]
    fn fire_blaster_spawns_bolt() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();

        assert_eq!(world.entities.count(), 1);

        world.fire_blaster(
            owner,
            Vec3f::new(0.0, 0.0, 0.0),
            Vec3f::new(1.0, 0.0, 0.0),
            15,
            1000,
            q2_shared::constants::EF_BLASTER,
            false,
        );

        assert_eq!(world.entities.count(), 2);

        // Find the bolt.
        let bolt_key = world.find_by_classname(None, "bolt").unwrap();
        let bolt = world.entities.get(bolt_key).unwrap();
        assert_eq!(bolt.game.classname, "bolt");
        assert_eq!(bolt.game.movetype, MoveType::FlyMissile);
        assert_eq!(bolt.game.dmg, 15);
        assert!((bolt.velocity.x - 1000.0).abs() < 1.0);
        assert!(bolt.touch.is_some());
        assert!(bolt.think.is_some());
        assert_eq!(bolt.owner, Some(owner));
    }

    #[test]
    fn fire_blaster_hyper_sets_spawnflag() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();

        world.fire_blaster(
            owner,
            Vec3f::ZERO,
            Vec3f::X,
            20,
            1000,
            q2_shared::constants::EF_BLASTER,
            true,
        );

        let bolt_key = world.find_by_classname(None, "bolt").unwrap();
        let bolt = world.entities.get(bolt_key).unwrap();
        assert_eq!(bolt.game.spawnflags, 1);
    }

    #[test]
    fn projectiles_carry_their_c_light_and_trail_effects() {
        use q2_shared::constants::{
            EF_ANIM_ALLFAST, EF_BFG, EF_GRENADE, EF_HYPERBLASTER, EF_ROCKET,
        };
        let mut world = test_world();
        let owner = world.spawn().unwrap();
        world.fire_blaster(
            owner,
            Vec3f::new(8.0, 0.0, 0.0),
            Vec3f::X,
            15,
            1000,
            EF_HYPERBLASTER,
            true,
        );
        world.fire_rocket(owner, Vec3f::ZERO, Vec3f::X, 100, 650, 150.0, 120);
        world.fire_grenade(owner, Vec3f::ZERO, Vec3f::X, 120, 600, 2.5, 150.0);
        world.fire_bfg(owner, Vec3f::ZERO, Vec3f::X, 200, 400, 1000.0);
        let effects = |class: &str| {
            let key = world.find_by_classname(None, class).unwrap();
            world.entities.get(key).unwrap().state.effects
        };
        assert_eq!(effects("bolt"), EF_HYPERBLASTER);
        let bolt = world.find_by_classname(None, "bolt").unwrap();
        // C fire_blaster starts the bolt's lerp at the muzzle.
        assert_eq!(
            world.entities.get(bolt).unwrap().state.old_origin,
            Vec3f::new(8.0, 0.0, 0.0)
        );
        assert_eq!(effects("rocket"), EF_ROCKET);
        assert_eq!(effects("grenade"), EF_GRENADE);
        assert_eq!(effects("bfg blast"), EF_BFG | EF_ANIM_ALLFAST);
    }

    #[test]
    fn fire_rocket_spawns_rocket() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();

        world.fire_rocket(owner, Vec3f::ZERO, Vec3f::X, 100, 650, 150.0, 120);

        let rocket_key = world.find_by_classname(None, "rocket").unwrap();
        let rocket = world.entities.get(rocket_key).unwrap();
        assert_eq!(rocket.game.classname, "rocket");
        assert_eq!(rocket.game.movetype, MoveType::FlyMissile);
        assert_eq!(rocket.game.dmg, 100);
        assert_eq!(rocket.game.radius_dmg, 120);
        assert!((rocket.game.dmg_radius - 150.0).abs() < 0.01);
        assert!((rocket.velocity.x - 650.0).abs() < 1.0);
    }

    #[test]
    fn fire_grenade_spawns_bouncing_projectile() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();

        world.fire_grenade(owner, Vec3f::ZERO, Vec3f::X, 120, 600, 2.5, 150.0);

        let gren_key = world.find_by_classname(None, "grenade").unwrap();
        let gren = world.entities.get(gren_key).unwrap();
        assert_eq!(gren.game.movetype, MoveType::Bounce);
        assert_eq!(gren.game.dmg, 120);
        // Forward velocity + upward bias.
        assert!(gren.velocity.x > 0.0);
        assert!(gren.velocity.z > 0.0);
        // Spin.
        assert_eq!(gren.avelocity, Vec3f::new(300.0, 300.0, 300.0));
    }

    #[test]
    fn fire_bfg_spawns_projectile() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();

        world.fire_bfg(owner, Vec3f::ZERO, Vec3f::X, 200, 400, 1000.0);

        let bfg_key = world.find_by_classname(None, "bfg blast").unwrap();
        let bfg = world.entities.get(bfg_key).unwrap();
        assert_eq!(bfg.game.dmg, 200); // Hardcoded core damage.
        assert_eq!(bfg.game.radius_dmg, 200);
        assert!((bfg.game.dmg_radius - 1000.0).abs() < 0.01);
    }

    #[test]
    fn blaster_touch_damages_target() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();
        let target = world.spawn().unwrap();

        // Create bolt.
        world.fire_blaster(
            owner,
            Vec3f::ZERO,
            Vec3f::X,
            15,
            1000,
            q2_shared::constants::EF_BLASTER,
            false,
        );
        let bolt_key = world.find_by_classname(None, "bolt").unwrap();

        // Set up target.
        {
            let targ = world.entities.get_mut(target).unwrap();
            targ.game.health = 100;
            targ.game.takedamage = TakeDamage::Yes;
            targ.solid = Solid::Bbox;
        }

        // Simulate touch.
        blaster_touch(&mut world, bolt_key, target, None, None);

        // Target should have taken damage.
        assert_eq!(world.entities.get(target).unwrap().game.health, 85);
        // Bolt should be freed.
        assert!(!world.entities.get(bolt_key).is_some_and(|e| e.in_use));
    }

    #[test]
    fn blaster_touch_ignores_owner() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();

        world.fire_blaster(
            owner,
            Vec3f::ZERO,
            Vec3f::X,
            15,
            1000,
            q2_shared::constants::EF_BLASTER,
            false,
        );
        let bolt_key = world.find_by_classname(None, "bolt").unwrap();

        {
            let ent = world.entities.get_mut(owner).unwrap();
            ent.game.health = 100;
            ent.game.takedamage = TakeDamage::Yes;
        }

        // Touch with owner — should not damage.
        blaster_touch(&mut world, bolt_key, owner, None, None);

        assert_eq!(world.entities.get(owner).unwrap().game.health, 100);
        // Bolt should still exist (touch was ignored).
        assert!(world.entities.get(bolt_key).is_some());
    }

    #[test]
    fn rocket_touch_applies_radius_damage() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();
        let target = world.spawn().unwrap();
        let bystander = world.spawn().unwrap();

        // Fire rocket.
        world.fire_rocket(owner, Vec3f::ZERO, Vec3f::X, 100, 650, 150.0, 120);
        let rocket_key = world.find_by_classname(None, "rocket").unwrap();

        // Set up target and bystander.
        {
            let targ = world.entities.get_mut(target).unwrap();
            targ.state.origin = Vec3f::new(10.0, 0.0, 0.0);
            targ.game.health = 200;
            targ.game.takedamage = TakeDamage::Yes;
            targ.solid = Solid::Bbox;
        }
        {
            let by = world.entities.get_mut(bystander).unwrap();
            by.state.origin = Vec3f::new(50.0, 0.0, 0.0);
            by.game.health = 200;
            by.game.takedamage = TakeDamage::Yes;
            by.solid = Solid::Bbox;
        }

        // Simulate rocket hitting target.
        rocket_touch(&mut world, rocket_key, target, None, None);

        // Target takes direct damage (100).
        assert!(world.entities.get(target).unwrap().game.health < 200);
        // Bystander takes splash damage (within 150 radius).
        assert!(world.entities.get(bystander).unwrap().game.health < 200);
        // Rocket is freed.
        assert!(!world.entities.get(rocket_key).is_some_and(|e| e.in_use));
    }

    #[test]
    fn grenade_explode_does_radius_damage() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();
        let target = world.spawn().unwrap();

        // Manually create a grenade.
        let gren = world.spawn().unwrap();
        {
            let g = world.entities.get_mut(gren).unwrap();
            g.game.classname = "grenade".into();
            g.game.dmg = 120;
            g.game.dmg_radius = 150.0;
            g.owner = Some(owner);
            g.state.origin = Vec3f::ZERO;
        }
        {
            let t = world.entities.get_mut(target).unwrap();
            t.state.origin = Vec3f::new(30.0, 0.0, 0.0);
            t.game.health = 200;
            t.game.takedamage = TakeDamage::Yes;
            t.solid = Solid::Bbox;
        }

        grenade_explode(&mut world, gren);

        // Target should have taken splash damage.
        assert!(world.entities.get(target).unwrap().game.health < 200);
        // Grenade should be freed.
        assert!(!world.entities.get(gren).is_some_and(|e| e.in_use));
    }

    #[test]
    fn grenade_touch_explodes_on_damageable() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();

        world.fire_grenade(owner, Vec3f::ZERO, Vec3f::X, 120, 600, 2.5, 150.0);
        let gren_key = world.find_by_classname(None, "grenade").unwrap();

        let target = world.spawn().unwrap();
        {
            let t = world.entities.get_mut(target).unwrap();
            t.state.origin = Vec3f::new(5.0, 0.0, 0.0);
            t.game.health = 200;
            t.game.takedamage = TakeDamage::Yes;
            t.solid = Solid::Bbox;
        }

        grenade_touch(&mut world, gren_key, target, None, None);

        // Target should have taken damage.
        assert!(world.entities.get(target).unwrap().game.health < 200);
        // Grenade should be freed after explosion.
        assert!(!world.entities.get(gren_key).is_some_and(|e| e.in_use));
    }

    #[test]
    fn fire_shotgun_fires_multiple_pellets() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();

        // Shotgun fires 12 pellets. Each calls fire_bullet (hitscan, no entities).
        world.fire_shotgun(
            owner,
            Vec3f::ZERO,
            Vec3f::X,
            4,  // damage per pellet
            10, // kick
            500,
            500, // spread
            12,  // count
            MeansOfDeath::Shotgun,
        );

        // No projectile entities should be created (hitscan).
        assert_eq!(world.entities.count(), 1); // Only the owner.
    }

    #[test]
    fn projectile_timeout_frees_entity() {
        let mut world = test_world();
        let ent = world.spawn().unwrap();

        projectile_timeout(&mut world, ent);

        assert!(!world.entities.get(ent).is_some_and(|e| e.in_use));
    }

    fn target_at(world: &mut GameWorld, origin: Vec3f, health: i32) -> EntityKey {
        let key = world.spawn().unwrap();
        let t = world.entities.get_mut(key).unwrap();
        t.state.origin = origin;
        t.game.health = health;
        t.game.takedamage = TakeDamage::Yes;
        t.solid = Solid::Bbox;
        key
    }

    fn hit(world: &GameWorld, gi: &crate::world::MockGameImport, target: EntityKey) {
        *gi.next_trace.borrow_mut() = Some(Trace {
            fraction: 0.5,
            endpos: world.entities.get(target).unwrap().state.origin,
            ent_index: world.number_of(target),
            ..Trace::default()
        });
    }

    #[test]
    fn projectiles_clip_against_monsters_with_c_mask_shot() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();
        world.fire_blaster(
            owner,
            Vec3f::ZERO,
            Vec3f::X,
            15,
            1000,
            q2_shared::constants::EF_BLASTER,
            false,
        );
        world.fire_rocket(owner, Vec3f::ZERO, Vec3f::X, 100, 650, 150.0, 120);
        world.fire_grenade(owner, Vec3f::ZERO, Vec3f::X, 120, 600, 2.5, 150.0);
        world.fire_bfg(owner, Vec3f::ZERO, Vec3f::X, 200, 400, 1000.0);
        for class in ["bolt", "rocket", "grenade", "bfg blast"] {
            let key = world.find_by_classname(None, class).unwrap();
            // C MASK_SHOT: SOLID | WINDOW | MONSTER | DEADMONSTER.
            assert_eq!(
                world.entities.get(key).unwrap().clipmask,
                0x0600_0003,
                "{class}"
            );
        }
    }

    #[test]
    fn projectiles_carry_their_c_world_model_and_face_their_direction() {
        let (mut world, gi) = crate::world::test_world_recording();
        let owner = world.spawn().unwrap();
        world.fire_rocket(owner, Vec3f::ZERO, Vec3f::Y, 100, 650, 150.0, 120);
        world.fire_blaster(
            owner,
            Vec3f::ZERO,
            Vec3f::Y,
            15,
            1000,
            q2_shared::constants::EF_BLASTER,
            false,
        );
        world.fire_grenade(owner, Vec3f::ZERO, Vec3f::Y, 120, 600, 2.5, 150.0);
        world.fire_bfg(owner, Vec3f::ZERO, Vec3f::Y, 200, 400, 1000.0);
        for (class, model) in [
            ("rocket", "models/objects/rocket/tris.md2"),
            ("bolt", "models/objects/laser/tris.md2"),
            ("grenade", "models/objects/grenade/tris.md2"),
            ("bfg blast", "sprites/s_bfg1.sp2"),
        ] {
            let key = world.find_by_classname(None, class).unwrap();
            let ent = world.entities.get(key).unwrap();
            let index = gi.models.borrow().iter().position(|m| m == model);
            assert_eq!(
                Some(ent.state.modelindex),
                index.map(|i| i as i32 + 1),
                "{class}"
            );
        }
        let rocket = world.find_by_classname(None, "rocket").unwrap();
        assert_eq!(
            world.entities.get(rocket).unwrap().state.angles,
            Vec3f::new(0.0, 90.0, 0.0)
        );
    }

    #[test]
    fn grenade_direct_hit_damages_the_target_once() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();
        world.fire_grenade(owner, Vec3f::ZERO, Vec3f::X, 120, 600, 2.5, 150.0);
        let gren = world.find_by_classname(None, "grenade").unwrap();
        let target = target_at(&mut world, Vec3f::new(10.0, 0.0, 0.0), 500);

        grenade_touch(&mut world, gren, target, None, None);

        // C Grenade_Explode: one radius-style hit of 120 - 0.5 * 10, and the
        // splash ignores the enemy.
        assert_eq!(world.entities.get(target).unwrap().game.health, 385);
    }

    #[test]
    fn bfg_touch_hits_the_target_for_200_and_splashes_200_within_100() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();
        world.fire_bfg(owner, Vec3f::ZERO, Vec3f::X, 200, 400, 1000.0);
        let bfg = world.find_by_classname(None, "bfg blast").unwrap();
        let target = target_at(&mut world, Vec3f::new(10.0, 0.0, 0.0), 1000);
        let near = target_at(&mut world, Vec3f::new(50.0, 0.0, 0.0), 1000);
        let far = target_at(&mut world, Vec3f::new(150.0, 0.0, 0.0), 1000);

        bfg_touch(&mut world, bfg, target, None, None);

        let health = |k| world.entities.get(k).unwrap().game.health;
        assert_eq!(health(target), 800);
        assert_eq!(health(near), 1000 - 175);
        assert_eq!(health(far), 1000);
    }

    #[test]
    fn fire_bullet_damages_the_entity_the_trace_hits() {
        let (mut world, gi) = crate::world::test_world_recording();
        let owner = world.spawn().unwrap();
        let target = target_at(&mut world, Vec3f::new(100.0, 0.0, 0.0), 100);
        hit(&world, &gi, target);

        world.fire_bullet(
            owner,
            Vec3f::ZERO,
            Vec3f::X,
            8,
            2,
            0,
            0,
            MeansOfDeath::Machinegun,
        );

        assert_eq!(world.entities.get(target).unwrap().game.health, 92);
    }

    #[test]
    fn fire_rail_damages_the_hit_entity_and_traces_on_past_it() {
        let (mut world, gi) = crate::world::test_world_recording();
        let owner = world.spawn().unwrap();
        let target = target_at(&mut world, Vec3f::new(100.0, 0.0, 0.0), 200);
        world.entities.get_mut(target).unwrap().solid = Solid::Bbox;
        hit(&world, &gi, target);

        world.fire_rail(owner, Vec3f::ZERO, Vec3f::X, 100, 200);

        assert_eq!(world.entities.get(target).unwrap().game.health, 100);
        // The second trace skips the bbox entity it went through.
        assert_eq!(
            gi.trace_pass_ents.borrow().as_slice(),
            &[world.number_of(owner), world.number_of(target)]
        );
    }

    #[test]
    fn fire_hit_damages_the_enemy_in_melee_range() {
        let (mut world, gi) = crate::world::test_world_recording();
        let owner = target_at(&mut world, Vec3f::ZERO, 100);
        let enemy = target_at(&mut world, Vec3f::new(40.0, 0.0, 0.0), 100);
        world.entities.get_mut(enemy).unwrap().svflags |= SvFlags::MONSTER.bits();
        world.entities.get_mut(owner).unwrap().game.enemy = Some(enemy);
        hit(&world, &gi, enemy);

        assert!(world.fire_hit(owner, Vec3f::new(80.0, 0.0, 0.0), 10, 0));
        assert_eq!(world.entities.get(enemy).unwrap().game.health, 90);
    }

    #[test]
    fn fire_hit_misses_an_enemy_out_of_range() {
        let mut world = test_world();
        let owner = target_at(&mut world, Vec3f::ZERO, 100);
        let enemy = target_at(&mut world, Vec3f::new(200.0, 0.0, 0.0), 100);
        world.entities.get_mut(owner).unwrap().game.enemy = Some(enemy);

        assert!(!world.fire_hit(owner, Vec3f::new(80.0, 0.0, 0.0), 10, 0));
        assert_eq!(world.entities.get(enemy).unwrap().game.health, 100);
    }

    /// The two `crandom()` values a fresh shot draws from a world whose
    /// RNG is at `rand_state`.
    fn next_two_crandoms(rand_state: u32) -> (f32, f32) {
        let mut twin = test_world();
        twin.level.rand_state = rand_state;
        (twin.crandom(), twin.crandom())
    }

    #[test]
    fn fire_lead_spreads_with_game_crandom_at_c_end_point() {
        let (mut world, gi) = crate::world::test_world_recording();
        let owner = world.spawn().unwrap();
        world.level.rand_state = 12_345;
        let (cr, cu) = next_two_crandoms(12_345);
        let start = Vec3f::new(0.0, 0.0, 16.0);
        let dir = Vec3f::new(1.0, 1.0, 0.0).normalize();

        world.fire_bullet(owner, start, dir, 1, 0, 300, 500, MeansOfDeath::Machinegun);

        // C fire_lead: end = start + 8192 forward + r right + u up.
        let (forward, right, up) = angle_vectors(vectoangles(dir));
        let end = start + forward * 8192.0 + right * (cr * 300.0) + up * (cu * 500.0);
        assert_eq!(gi.trace_ends.borrow()[1], end);
    }

    #[test]
    fn fire_lead_draws_its_two_crandoms_even_without_spread() {
        let (mut world, gi) = crate::world::test_world_recording();
        let owner = world.spawn().unwrap();
        world.level.rand_state = 99;
        let mut twin = test_world();
        twin.level.rand_state = 99;
        twin.crandom();
        twin.crandom();

        world.fire_bullet(
            owner,
            Vec3f::ZERO,
            Vec3f::X,
            1,
            0,
            0,
            0,
            MeansOfDeath::Machinegun,
        );

        assert_eq!(world.level.rand_state, twin.level.rand_state);
        assert_eq!(gi.trace_ends.borrow()[1], Vec3f::new(8192.0, 0.0, 0.0));
    }

    #[test]
    fn shots_fired_in_the_same_frame_spread_differently() {
        let (mut world, gi) = crate::world::test_world_recording();
        let owner = world.spawn().unwrap();
        for _ in 0..2 {
            world.fire_bullet(
                owner,
                Vec3f::ZERO,
                Vec3f::X,
                1,
                0,
                500,
                500,
                MeansOfDeath::Machinegun,
            );
        }
        let ends = gi.trace_ends.borrow();
        // Each shot traces the muzzle, then the spread line.
        assert_ne!(ends[1], ends[3]);
    }

    #[test]
    fn fire_blaster_checks_the_path_from_the_shooter_to_the_muzzle() {
        let (mut world, gi) = crate::world::test_world_recording();
        let owner = world.spawn().unwrap();
        let start = Vec3f::new(16.0, 0.0, 0.0);

        world.fire_blaster(
            owner,
            start,
            Vec3f::X,
            15,
            1000,
            q2_shared::constants::EF_BLASTER,
            false,
        );

        let bolt = world.find_by_classname(None, "bolt").unwrap();
        // C traces from self->s.origin to the bolt, passing the bolt.
        assert_eq!(*gi.trace_ends.borrow(), vec![start]);
        assert_eq!(*gi.trace_masks.borrow(), vec![MASK_SHOT]);
        assert_eq!(*gi.trace_pass_ents.borrow(), vec![world.number_of(bolt)]);
        assert_eq!(world.entities.get(bolt).unwrap().state.origin, start);
    }

    #[test]
    fn fire_blaster_touches_what_blocks_the_muzzle_at_once() {
        let (mut world, gi) = crate::world::test_world_recording();
        let owner = world.spawn().unwrap();
        let target = target_at(&mut world, Vec3f::new(8.0, 0.0, 0.0), 100);
        hit(&world, &gi, target);

        world.fire_blaster(
            owner,
            Vec3f::new(16.0, 0.0, 0.0),
            Vec3f::X,
            15,
            1000,
            q2_shared::constants::EF_BLASTER,
            false,
        );

        assert_eq!(world.entities.get(target).unwrap().game.health, 85);
        assert!(world.find_by_classname(None, "bolt").is_none());
    }

    #[test]
    fn fire_blaster_explodes_at_once_when_the_muzzle_is_inside_a_wall() {
        let (mut world, gi) = crate::world::test_world_recording();
        world.entities.spawn_numbered(0).unwrap();
        let owner = world.spawn().unwrap();
        // No entity hit: C's tr.ent is the world.
        *gi.next_trace.borrow_mut() = Some(Trace {
            fraction: 0.5,
            ..Trace::default()
        });

        world.fire_blaster(
            owner,
            Vec3f::new(16.0, 0.0, 0.0),
            Vec3f::X,
            15,
            1000,
            q2_shared::constants::EF_BLASTER,
            false,
        );

        assert!(world.find_by_classname(None, "bolt").is_none());
    }

    #[test]
    fn blaster_bolts_are_dead_monsters_that_cast_no_shadow() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();
        world.fire_blaster(
            owner,
            Vec3f::ZERO,
            Vec3f::X,
            15,
            1000,
            q2_shared::constants::EF_BLASTER,
            false,
        );
        let bolt = world.find_by_classname(None, "bolt").unwrap();
        let bolt = world.entities.get(bolt).unwrap();
        // C: player prediction does not clip against bolts.
        assert_ne!(bolt.svflags & q2_shared::constants::SVF_DEADMONSTER, 0);
        assert_ne!(bolt.state.renderfx & q2_shared::constants::RF_NOSHADOW, 0);
    }

    #[test]
    fn rocket_lifetime_is_c_integer_division_of_8000_by_speed() {
        let mut world = test_world();
        world.level.time = 3.0;
        let owner = world.spawn().unwrap();
        world.fire_rocket(owner, Vec3f::ZERO, Vec3f::X, 100, 650, 150.0, 120);
        world.fire_bfg(owner, Vec3f::ZERO, Vec3f::X, 200, 400, 1000.0);
        for (class, life) in [("rocket", 12.0), ("bfg blast", 20.0)] {
            let key = world.find_by_classname(None, class).unwrap();
            assert_eq!(
                world.entities.get(key).unwrap().game.nextthink,
                3.0 + life,
                "{class}"
            );
        }
    }

    #[test]
    fn a_zero_speed_rocket_does_not_divide_by_zero() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();
        world.fire_rocket(owner, Vec3f::ZERO, Vec3f::X, 100, 0, 150.0, 120);
        let key = world.find_by_classname(None, "rocket").unwrap();
        assert!(world.entities.get(key).unwrap().game.nextthink.is_finite());
    }
}
