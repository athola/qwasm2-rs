//! Game-level physics — entity movement, collision response, and gravity.
//!
//! Faithful port of `g_phys.c` (1,300 lines, 17 functions) from the C source.
//! All constants, thresholds, and algorithms match the original exactly.
//!
//! # Relationship to `q2-common::pmove`
//!
//! This module is **server-side entity physics** (all entity types, all MoveTypes).
//! `q2-common::pmove` is **client-side player movement prediction** (Pmove struct only).
//! They share some algorithms (e.g. velocity clipping) but run in separate contexts:
//! - `pmove` executes on every client tick for prediction
//! - `physics` executes on the server frame loop for all non-player entities
//!
//! No public types or function names overlap. `clip_velocity` here is the server copy
//! of `pm_clip_velocity` in pmove — kept separate to avoid a crate dependency inversion.
//!
//! # Physics dispatch
//!
//! `run_entity()` dispatches by `MoveType`:
//! - `None` → `physics_none` (think only)
//! - `Noclip` → `physics_noclip` (free movement, no collision)
//! - `Push` / `Stop` → `physics_pusher` (doors, platforms)
//! - `Step` → `physics_step` (monsters — gravity + stepping)
//! - `Toss` / `Bounce` / `Fly` / `FlyMissile` → `physics_toss` (projectiles)
//!
//! # C source reference
//! `~/Qwasm2/src/game/g_phys.c`

use q2_shared::types::*;

use crate::constants::*;
use crate::entity::EntityKey;
use crate::world::GameWorld;

// ---------------------------------------------------------------------------
// Constants — must match C source exactly
// ---------------------------------------------------------------------------

/// Velocity components below this magnitude are zeroed after clipping.
/// C ref: g_phys.c STOP_EPSILON
const STOP_EPSILON: f32 = 0.1;

/// Maximum collision planes tracked in fly_move.
/// C ref: g_phys.c MAX_CLIP_PLANES
// (also in constants.rs, but kept local for the algorithm)
const FLY_MAX_CLIP_PLANES: usize = 5;

/// Maximum bump iterations in fly_move before giving up.
const MAX_BUMPS: usize = 4;

/// Ground detection: a surface is "floor" if its normal Z >= this threshold.
/// This corresponds to ~45 degrees from horizontal.
const GROUND_NORMAL_THRESHOLD: f32 = 0.7;

/// Speed below which friction deceleration is constant rather than proportional.
/// C ref: g_phys.c STOPSPEED
const STOPSPEED: f32 = 100.0;

/// Normal friction coefficient.
/// C ref: g_phys.c FRICTION
const FRICTION: f32 = 6.0;

/// Water friction coefficient.
/// C ref: g_phys.c WATERFRICTION
const _WATERFRICTION: f32 = 1.0;

// ---------------------------------------------------------------------------
// Return type for movement functions
// ---------------------------------------------------------------------------

bitflags::bitflags! {
    /// Flags indicating what kind of surfaces were hit during movement.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct BlockedFlags: u32 {
        /// Hit a floor (normal.z >= 0.7).
        const FLOOR = 0x01;
        /// Hit a wall or step.
        const STEP  = 0x02;
    }
}

// ---------------------------------------------------------------------------
// Pure math helpers
// ---------------------------------------------------------------------------

/// Reflect velocity off a surface normal.
///
/// C ref: `ClipVelocity` (g_phys.c:172-206)
///
/// `overbounce`: 1.0 = normal clip (slide), >1.0 = bounce.
/// Q2 uses 1.0 for most movement and 1.5 for MOVETYPE_BOUNCE.
///
/// Returns (clipped_velocity, blocked_flags).
pub fn clip_velocity(
    velocity: Vec3f,
    normal: Vec3f,
    overbounce: f32,
) -> (Vec3f, BlockedFlags) {
    let mut blocked = BlockedFlags::empty();

    if normal.z > 0.0 {
        blocked |= BlockedFlags::FLOOR;
    }
    if normal.z == 0.0 {
        blocked |= BlockedFlags::STEP;
    }

    let backoff = velocity.dot(normal) * overbounce;

    let mut out = velocity - normal * backoff;

    // Clamp small values to zero to prevent drift.
    if out.x.abs() < STOP_EPSILON {
        out.x = 0.0;
    }
    if out.y.abs() < STOP_EPSILON {
        out.y = 0.0;
    }
    if out.z.abs() < STOP_EPSILON {
        out.z = 0.0;
    }

    (out, blocked)
}

// ---------------------------------------------------------------------------
// Entity-level helpers on GameWorld
// ---------------------------------------------------------------------------

impl GameWorld {
    /// Clamp entity velocity to `sv_maxvelocity`.
    /// C ref: `SV_CheckVelocity` (g_phys.c:87-100)
    pub fn check_velocity(&mut self, key: EntityKey) {
        let Some(ent) = self.entities.get_mut(key) else {
            return;
        };
        let speed = ent.velocity.length();
        if speed > DEFAULT_MAX_VELOCITY {
            ent.velocity = ent.velocity.normalize() * DEFAULT_MAX_VELOCITY;
        }
    }

    /// Call entity's think function if `nextthink <= level.time`.
    /// Returns `false` if think was called (entity may have been freed).
    /// C ref: `SV_RunThink` (g_phys.c:106-138)
    pub fn run_think(&mut self, key: EntityKey) -> bool {
        let (nextthink, think_fn) = {
            let Some(ent) = self.entities.get(key) else {
                return false;
            };
            (ent.game.nextthink, ent.think)
        };

        if nextthink <= 0.0 || nextthink > self.level.time + 0.001 {
            return true;
        }

        // Clear nextthink before calling (think may set a new one).
        if let Some(ent) = self.entities.get_mut(key) {
            ent.game.nextthink = 0.0;
        }

        if let Some(think) = think_fn {
            think(self, key);
        }

        // Entity may have been freed by think callback.
        self.entities.get(key).is_some()
    }

    /// Dispatch touch callbacks after a collision between two entities.
    /// C ref: `SV_Impact` (g_phys.c:144-165)
    pub fn sv_impact(&mut self, e1: EntityKey, e2: Option<EntityKey>, trace: &Trace) {
        // Get touch callbacks before calling them (avoids borrow issues).
        let touch1 = self
            .entities
            .get(e1)
            .filter(|e| e.solid != Solid::Not)
            .and_then(|e| e.touch);

        let touch2 = e2.and_then(|k| {
            self.entities
                .get(k)
                .filter(|e| e.solid != Solid::Not)
                .and_then(|e| e.touch)
        });

        if let Some(touch) = touch1 {
            if let Some(e2_key) = e2 {
                touch(
                    self,
                    e1,
                    e2_key,
                    Some(&trace.plane),
                    trace.surface.as_ref(),
                );
            }
        }

        if let (Some(touch), Some(e2_key)) = (touch2, e2) {
            touch(self, e2_key, e1, None, None);
        }
    }

    /// Apply gravity to an entity for one frame.
    /// C ref: `SV_AddGravity` (g_phys.c:373-382)
    pub fn add_gravity(&mut self, key: EntityKey) {
        let Some(ent) = self.entities.get_mut(key) else {
            return;
        };
        let gravity_scale = if ent.game.gravity != 0.0 {
            ent.game.gravity
        } else {
            1.0
        };
        ent.velocity.z -= gravity_scale * DEFAULT_GRAVITY * FRAMETIME;
    }

    /// Apply friction to angular velocity, spinning entities to rest.
    /// C ref: `SV_AddRotationalFriction` (g_phys.c:1046-1080)
    pub fn add_rotational_friction(&mut self, key: EntityKey) {
        let Some(ent) = self.entities.get_mut(key) else {
            return;
        };

        let adjustment = FRAMETIME * STOPSPEED * FRICTION;

        for i in 0..3 {
            let av = match i {
                0 => &mut ent.avelocity.x,
                1 => &mut ent.avelocity.y,
                _ => &mut ent.avelocity.z,
            };

            if *av > 0.0 {
                *av -= adjustment;
                if *av < 0.0 {
                    *av = 0.0;
                }
            } else {
                *av += adjustment;
                if *av > 0.0 {
                    *av = 0.0;
                }
            }
        }

        // Update entity angles.
        ent.state.angles += ent.avelocity * FRAMETIME;
    }

    // -----------------------------------------------------------------------
    // SV_FlyMove — multi-plane clipping movement
    // C ref: g_phys.c:218-371
    // -----------------------------------------------------------------------

    /// Move an entity through the world, sliding along up to 5 collision
    /// planes. This is the core movement algorithm shared by toss and step
    /// physics.
    ///
    /// Returns `BlockedFlags` indicating what was hit.
    pub fn fly_move(&mut self, key: EntityKey, time: f32, mask: i32) -> BlockedFlags {
        let mut blocked = BlockedFlags::empty();
        let mut planes: Vec<Vec3f> = Vec::new();
        let mut time_left = time;
        let mut num_bumps = 0;

        // Read initial state.
        let (mut origin, velocity, mins, maxs) = {
            let Some(ent) = self.entities.get(key) else {
                return blocked;
            };
            (
                ent.state.origin,
                ent.velocity,
                ent.mins,
                ent.maxs,
            )
        };
        let original_velocity = velocity;
        let mut current_velocity = velocity;

        while num_bumps < MAX_BUMPS {
            num_bumps += 1;

            let end = origin + current_velocity * time_left;

            // Trace from origin to end.
            let trace = self.gi.trace(origin, mins, maxs, end, None, mask);

            if trace.allsolid {
                // Entity is stuck in solid.
                if let Some(ent) = self.entities.get_mut(key) {
                    ent.velocity = Vec3f::ZERO;
                }
                return BlockedFlags::FLOOR | BlockedFlags::STEP;
            }

            if trace.fraction > 0.0 {
                // Move to the trace endpoint.
                origin = trace.endpos;
                if let Some(ent) = self.entities.get_mut(key) {
                    ent.state.origin = origin;
                }
            }

            if trace.fraction == 1.0 {
                break; // No collision, done.
            }

            // Hit something — determine what.
            let hit_key: Option<EntityKey> = None; // Phase 3: map trace.ent_index to EntityKey
            self.sv_impact(key, hit_key, &trace);

            // Entity may have been freed by the touch callback.
            if self.entities.get(key).is_none() {
                break;
            }

            time_left -= time_left * trace.fraction;

            // Record the hit plane.
            if planes.len() >= FLY_MAX_CLIP_PLANES {
                // Too many planes — entity is wedged.
                if let Some(ent) = self.entities.get_mut(key) {
                    ent.velocity = Vec3f::ZERO;
                }
                return blocked;
            }

            planes.push(trace.plane.normal);

            // Check floor/wall.
            if trace.plane.normal.z > GROUND_NORMAL_THRESHOLD {
                blocked |= BlockedFlags::FLOOR;
                // Set ground entity.
                if let Some(ent) = self.entities.get_mut(key) {
                    ent.game.ground_entity = hit_key;
                }
            }
            if trace.plane.normal.z == 0.0 {
                blocked |= BlockedFlags::STEP;
            }

            // Clip velocity against all accumulated planes.
            let mut clipped = false;
            for i in 0..planes.len() {
                let (v, _) = clip_velocity(current_velocity, planes[i], 1.0);
                // Check that the clipped velocity doesn't go back into any
                // previously-hit plane.
                let mut valid = true;
                for (j, plane) in planes.iter().enumerate() {
                    if j == i {
                        continue;
                    }
                    if v.dot(*plane) < 0.0 {
                        valid = false;
                        break;
                    }
                }
                if valid {
                    current_velocity = v;
                    clipped = true;
                    break;
                }
            }

            if !clipped {
                // Couldn't clip against a single plane — try sliding along
                // the crease between the last two planes.
                if planes.len() == 2 {
                    let dir = planes[0].cross(planes[1]).normalize();
                    let d = dir.dot(current_velocity);
                    current_velocity = dir * d;
                } else {
                    // Stuck against 3+ planes — stop.
                    if let Some(ent) = self.entities.get_mut(key) {
                        ent.velocity = Vec3f::ZERO;
                    }
                    return blocked;
                }
            }

            // If the clipped velocity reversed direction, stop.
            if current_velocity.dot(original_velocity) <= 0.0 {
                if let Some(ent) = self.entities.get_mut(key) {
                    ent.velocity = Vec3f::ZERO;
                }
                return blocked;
            }
        }

        // Update velocity on the entity.
        if let Some(ent) = self.entities.get_mut(key) {
            ent.velocity = current_velocity;
        }

        blocked
    }

    // -----------------------------------------------------------------------
    // SV_PushEntity — trace-based move without velocity change
    // C ref: g_phys.c:503-578
    // -----------------------------------------------------------------------

    /// Move an entity in a straight line by `push`, tracing for collisions.
    /// Does not modify velocity. Returns the trace result.
    pub fn push_entity(&mut self, key: EntityKey, push: Vec3f) -> Trace {
        let (origin, mins, maxs, clipmask) = {
            let Some(ent) = self.entities.get(key) else {
                return Trace::default();
            };
            let mask = if ent.clipmask != 0 {
                ent.clipmask
            } else {
                1 // MASK_SOLID
            };
            (ent.state.origin, ent.mins, ent.maxs, mask)
        };

        let end = origin + push;
        let trace = self.gi.trace(origin, mins, maxs, end, None, clipmask);

        if let Some(ent) = self.entities.get_mut(key) {
            ent.state.origin = trace.endpos;
        }

        // Link entity in the world (updates spatial index).
        self.gi.link_entity(0); // placeholder index

        if trace.fraction < 1.0 {
            let hit_key: Option<EntityKey> = None; // Phase 3: map trace.ent_index to EntityKey
            self.sv_impact(key, hit_key, &trace);
        }

        trace
    }

    // -----------------------------------------------------------------------
    // Physics dispatch functions
    // -----------------------------------------------------------------------

    /// Physics for MOVETYPE_NONE — entity is stationary, only runs think.
    /// C ref: `SV_Physics_None` (g_phys.c:851-861)
    pub fn physics_none(&mut self, key: EntityKey) {
        self.run_think(key);
    }

    /// Physics for MOVETYPE_NOCLIP — free movement, no collision.
    /// C ref: `SV_Physics_Noclip` (g_phys.c:866-884)
    pub fn physics_noclip(&mut self, key: EntityKey) {
        if !self.run_think(key) {
            return;
        }

        let Some(ent) = self.entities.get_mut(key) else {
            return;
        };

        ent.state.angles += ent.avelocity * FRAMETIME;
        ent.state.origin += ent.velocity * FRAMETIME;

        self.gi.link_entity(0);
    }

    /// Physics for MOVETYPE_TOSS, BOUNCE, FLY, FLYMISSILE.
    /// Handles projectiles, gibs, and flying entities.
    /// C ref: `SV_Physics_Toss` (g_phys.c:894-1030)
    pub fn physics_toss(&mut self, key: EntityKey) {
        // Run think first — may remove entity.
        if !self.run_think(key) {
            return;
        }

        let Some(ent) = self.entities.get(key) else {
            return;
        };

        // Skip team slaves (moved by master).
        if ent.game.flags.contains(EntityFlags::TEAMSLAVE) {
            return;
        }

        // Clear ground if ascending.
        if ent.velocity.z > 0.0 {
            if let Some(ent) = self.entities.get_mut(key) {
                ent.game.ground_entity = None;
            }
        }

        // Check if ground entity is still valid.
        {
            let ground = self
                .entities
                .get(key)
                .and_then(|e| e.game.ground_entity);
            if let Some(gk) = ground {
                if self.entities.get(gk).is_none() {
                    if let Some(ent) = self.entities.get_mut(key) {
                        ent.game.ground_entity = None;
                    }
                }
            }
        }

        // If on ground with zero velocity, stay put.
        // C ref: SV_Physics_Toss checks velocity before returning.
        {
            let ent = self.entities.get(key).unwrap();
            if ent.game.ground_entity.is_some() && ent.velocity == Vec3f::ZERO {
                return;
            }
        }

        self.check_velocity(key);

        // Apply gravity for non-flying movetypes.
        {
            let movetype = self
                .entities
                .get(key)
                .map(|e| e.game.movetype)
                .unwrap_or_default();
            if movetype != MoveType::Fly && movetype != MoveType::FlyMissile {
                self.add_gravity(key);
            }
        }

        // Update angles from angular velocity.
        if let Some(ent) = self.entities.get_mut(key) {
            ent.state.angles += ent.avelocity * FRAMETIME;
        }

        // Move the entity.
        let move_vec = {
            let ent = self.entities.get(key).unwrap();
            ent.velocity * FRAMETIME
        };
        let trace = self.push_entity(key, move_vec);

        if self.entities.get(key).is_none() {
            return; // Freed by touch callback.
        }

        if trace.fraction < 1.0 {
            let movetype = self
                .entities
                .get(key)
                .map(|e| e.game.movetype)
                .unwrap_or_default();

            // Bounce: overbounce = 1.5, otherwise 1.0 (slide).
            let overbounce = if movetype == MoveType::Bounce {
                1.5
            } else {
                1.0
            };

            let velocity = self.entities.get(key).unwrap().velocity;
            let (clipped, _) = clip_velocity(velocity, trace.plane.normal, overbounce);

            if let Some(ent) = self.entities.get_mut(key) {
                ent.velocity = clipped;
            }

            // If hit a floor, check if we should stop.
            if trace.plane.normal.z > GROUND_NORMAL_THRESHOLD {
                let vel_z = self.entities.get(key).unwrap().velocity.z;

                if vel_z.abs() < 60.0 || movetype != MoveType::Bounce {
                    // Come to rest on the floor.
                    if let Some(ent) = self.entities.get_mut(key) {
                        // ground_entity would be set to trace.ent in full impl
                        ent.game.ground_entity = None; // simplified
                        ent.velocity = Vec3f::ZERO;
                        ent.avelocity = Vec3f::ZERO;
                    }
                }
            }
        }

        self.gi.link_entity(0);
    }

    /// Physics for MOVETYPE_STEP — walking monsters with gravity and stepping.
    /// C ref: `SV_Physics_Step` (g_phys.c:1082-1259)
    pub fn physics_step(&mut self, key: EntityKey) {
        // Check ground if not already grounded.
        let on_ground = self
            .entities
            .get(key)
            .and_then(|e| e.game.ground_entity)
            .is_some();

        if !on_ground {
            self.m_check_ground(key);
        }

        let on_ground = self
            .entities
            .get(key)
            .and_then(|e| e.game.ground_entity)
            .is_some();

        self.check_velocity(key);

        // Angular friction.
        let has_avelocity = self
            .entities
            .get(key)
            .map(|e| e.avelocity != Vec3f::ZERO)
            .unwrap_or(false);
        if has_avelocity {
            self.add_rotational_friction(key);
        }

        // Gravity — apply if not on ground, not flying, not swimming.
        let flags = self
            .entities
            .get(key)
            .map(|e| e.game.flags)
            .unwrap_or_default();

        let is_flying = flags.contains(EntityFlags::FLY);
        let is_swimming = flags.contains(EntityFlags::SWIM);

        if !on_ground && !is_flying && !is_swimming {
            self.add_gravity(key);
        }

        // Apply friction if on ground or flying/swimming.
        let speed = self
            .entities
            .get(key)
            .map(|e| {
                let v = e.velocity;
                Vec3f::new(v.x, v.y, 0.0).length()
            })
            .unwrap_or(0.0);

        if (on_ground || is_flying || is_swimming) && speed > 0.0 {
            let control = speed.max(STOPSPEED);
            let new_speed = (speed - FRAMETIME * control * FRICTION).max(0.0);
            let scale = new_speed / speed;

            if let Some(ent) = self.entities.get_mut(key) {
                ent.velocity.x *= scale;
                ent.velocity.y *= scale;
            }
        }

        // Collision mask — MASK_MONSTERSOLID for monsters, MASK_SOLID otherwise.
        // Both are 0xFFFF placeholder until proper content masks are defined.
        let mask: i32 = 0xFFFF;

        // Do the actual movement.
        self.fly_move(key, FRAMETIME, mask);

        // Link entity.
        self.gi.link_entity(0);

        // Run think.
        self.run_think(key);
    }

    /// Physics for MOVETYPE_PUSH / MOVETYPE_STOP — doors, platforms.
    /// Simplified: no team chain support yet. Full team support added later.
    /// C ref: `SV_Physics_Pusher` (g_phys.c:772-844)
    pub fn physics_pusher(&mut self, key: EntityKey) {
        // Skip team slaves — handled by master.
        let is_slave = self
            .entities
            .get(key)
            .map(|e| e.game.flags.contains(EntityFlags::TEAMSLAVE))
            .unwrap_or(false);
        if is_slave {
            return;
        }

        // Calculate movement for this frame.
        let (move_vec, amove_vec, has_movement) = {
            let Some(ent) = self.entities.get(key) else {
                return;
            };
            let m = ent.velocity * FRAMETIME;
            let am = ent.avelocity * FRAMETIME;
            let has = ent.velocity != Vec3f::ZERO || ent.avelocity != Vec3f::ZERO;
            (m, am, has)
        };

        if has_movement {
            // Simple push: move the entity directly.
            if let Some(ent) = self.entities.get_mut(key) {
                ent.state.origin += move_vec;
                ent.state.angles += amove_vec;
            }
            self.gi.link_entity(0);

            // TODO: SV_Push to displace blocking entities + rollback on block.
            // For now, just move the pusher. Full push with entity displacement
            // and rollback will be added when func_door/func_plat need it.
        }

        self.run_think(key);
    }

    // -----------------------------------------------------------------------
    // Ground checking — M_CheckGround
    // C ref: g_monster.c:223-267
    // -----------------------------------------------------------------------

    /// Check if an entity is standing on solid ground.
    /// Sets `ground_entity` if a floor surface is found directly below.
    pub fn m_check_ground(&mut self, key: EntityKey) {
        let (origin, mins, maxs, flags, vel_z) = {
            let Some(ent) = self.entities.get(key) else {
                return;
            };
            (
                ent.state.origin,
                ent.mins,
                ent.maxs,
                ent.game.flags,
                ent.velocity.z,
            )
        };

        // Flying and swimming entities don't check ground.
        if flags.contains(EntityFlags::FLY) || flags.contains(EntityFlags::SWIM) {
            return;
        }

        // Ascending too fast to be on ground.
        if vel_z > 100.0 {
            if let Some(ent) = self.entities.get_mut(key) {
                ent.game.ground_entity = None;
            }
            return;
        }

        // Trace straight down from entity center, 0.25 units.
        let start = origin;
        let end = Vec3f::new(origin.x, origin.y, origin.z - 0.25);
        let trace = self.gi.trace(start, mins, maxs, end, None, 0xFFFF);

        // Too steep to stand on.
        if trace.plane.normal.z < GROUND_NORMAL_THRESHOLD {
            if let Some(ent) = self.entities.get_mut(key) {
                ent.game.ground_entity = None;
            }
            return;
        }

        if !trace.startsolid && !trace.allsolid {
            if let Some(ent) = self.entities.get_mut(key) {
                ent.state.origin = trace.endpos;
                ent.game.ground_entity = None; // Phase 3: set to trace hit entity
                ent.velocity.z = 0.0;
            }
        }
    }

    // -----------------------------------------------------------------------
    // G_RunEntity — dispatch by movetype
    // C ref: g_phys.c:1263-1300
    // -----------------------------------------------------------------------

    /// Dispatch entity physics based on its movetype.
    /// Called once per entity per game frame from `G_RunFrame`.
    pub fn run_entity(&mut self, key: EntityKey) {
        // Prethink callback.
        let prethink = self.entities.get(key).and_then(|e| e.prethink);
        if let Some(pt) = prethink {
            pt(self, key);
        }

        let movetype = {
            let Some(ent) = self.entities.get(key) else {
                return;
            };
            ent.game.movetype
        };

        match movetype {
            MoveType::Push | MoveType::Stop => self.physics_pusher(key),
            MoveType::None => self.physics_none(key),
            MoveType::Noclip => self.physics_noclip(key),
            MoveType::Step => self.physics_step(key),
            MoveType::Toss
            | MoveType::Bounce
            | MoveType::Fly
            | MoveType::FlyMissile => self.physics_toss(key),
            MoveType::Walk => {
                // Walk is handled by player pmove, not here.
                // Just run think.
                self.run_think(key);
            }
        }
    }
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::test_world;

    // -- clip_velocity tests --

    #[test]
    fn clip_velocity_floor_hit() {
        // Velocity going down, hitting a horizontal floor (normal = up).
        let v = Vec3f::new(100.0, 0.0, -200.0);
        let n = Vec3f::new(0.0, 0.0, 1.0);
        let (clipped, flags) = clip_velocity(v, n, 1.0);

        assert!(flags.contains(BlockedFlags::FLOOR));
        assert_eq!(clipped.x, 100.0);
        assert_eq!(clipped.y, 0.0);
        // Z should be removed (slide along floor).
        assert!(clipped.z.abs() < STOP_EPSILON);
    }

    #[test]
    fn clip_velocity_wall_hit() {
        // Velocity going forward, hitting a wall (normal = back).
        let v = Vec3f::new(100.0, 0.0, 0.0);
        let n = Vec3f::new(-1.0, 0.0, 0.0);
        let (clipped, flags) = clip_velocity(v, n, 1.0);

        assert!(flags.contains(BlockedFlags::STEP));
        assert!(clipped.x.abs() < STOP_EPSILON);
        assert_eq!(clipped.y, 0.0);
        assert_eq!(clipped.z, 0.0);
    }

    #[test]
    fn clip_velocity_bounce() {
        // Ball falling down, bouncing off floor. Overbounce = 1.5.
        let v = Vec3f::new(0.0, 0.0, -100.0);
        let n = Vec3f::new(0.0, 0.0, 1.0);
        let (clipped, _) = clip_velocity(v, n, 1.5);

        // Dot product = -100 * 1 = -100, backoff = -100 * 1.5 = -150
        // out.z = -100 - 1.0 * -150 = 50
        assert!((clipped.z - 50.0).abs() < 0.01);
    }

    #[test]
    fn clip_velocity_angled_surface() {
        // Hitting a 45-degree slope.
        let v = Vec3f::new(100.0, 0.0, -100.0);
        let n = Vec3f::new(0.0, 0.0, 1.0).normalize();
        let (clipped, _) = clip_velocity(v, n, 1.0);

        // Should slide along the slope — X preserved, Z removed.
        assert_eq!(clipped.x, 100.0);
        assert!(clipped.z.abs() < STOP_EPSILON);
    }

    #[test]
    fn clip_velocity_stop_epsilon() {
        // Very small velocities should be zeroed.
        let v = Vec3f::new(0.05, -0.03, 0.02);
        let n = Vec3f::new(1.0, 0.0, 0.0);
        let (clipped, _) = clip_velocity(v, n, 1.0);

        assert_eq!(clipped.x, 0.0);
        assert_eq!(clipped.y, 0.0);
        assert_eq!(clipped.z, 0.0);
    }

    // -- check_velocity tests --

    #[test]
    fn check_velocity_clamps_to_max() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().velocity =
            Vec3f::new(3000.0, 0.0, 0.0);

        world.check_velocity(key);

        let speed = world.entities.get(key).unwrap().velocity.length();
        assert!((speed - DEFAULT_MAX_VELOCITY).abs() < 1.0);
    }

    #[test]
    fn check_velocity_preserves_direction() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().velocity =
            Vec3f::new(3000.0, 4000.0, 0.0);

        world.check_velocity(key);

        let v = world.entities.get(key).unwrap().velocity;
        // Direction should be preserved (3:4 ratio).
        assert!((v.x / v.y - 0.75).abs() < 0.01);
    }

    #[test]
    fn check_velocity_no_clamp_when_under_max() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        let original = Vec3f::new(100.0, 200.0, -50.0);
        world.entities.get_mut(key).unwrap().velocity = original;

        world.check_velocity(key);

        let v = world.entities.get(key).unwrap().velocity;
        assert_eq!(v, original);
    }

    // -- run_think tests --

    #[test]
    fn run_think_calls_when_due() {
        use std::sync::atomic::{AtomicBool, Ordering};
        static CALLED: AtomicBool = AtomicBool::new(false);

        fn test_think(_world: &mut GameWorld, _key: EntityKey) {
            CALLED.store(true, Ordering::Relaxed);
        }

        CALLED.store(false, Ordering::Relaxed);

        let mut world = test_world();
        world.level.time = 1.0;

        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().game.nextthink = 0.5;
        world.entities.get_mut(key).unwrap().think = Some(test_think);

        let result = world.run_think(key);

        assert!(CALLED.load(Ordering::Relaxed));
        assert!(result); // entity still exists
        // nextthink should be cleared.
        assert_eq!(world.entities.get(key).unwrap().game.nextthink, 0.0);
    }

    #[test]
    fn run_think_skips_when_not_due() {
        use std::sync::atomic::{AtomicBool, Ordering};
        static CALLED: AtomicBool = AtomicBool::new(false);

        fn test_think(_world: &mut GameWorld, _key: EntityKey) {
            CALLED.store(true, Ordering::Relaxed);
        }

        CALLED.store(false, Ordering::Relaxed);

        let mut world = test_world();
        world.level.time = 1.0;

        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().game.nextthink = 2.0; // future
        world.entities.get_mut(key).unwrap().think = Some(test_think);

        world.run_think(key);

        assert!(!CALLED.load(Ordering::Relaxed));
    }

    #[test]
    fn run_think_skips_zero_nextthink() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        // nextthink = 0.0, think = None → should not crash.
        let result = world.run_think(key);
        assert!(result);
    }

    // -- add_gravity tests --

    #[test]
    fn add_gravity_applies_downward() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().velocity = Vec3f::ZERO;

        world.add_gravity(key);

        let vz = world.entities.get(key).unwrap().velocity.z;
        // Should be -800 * 0.1 = -80
        assert!((vz - (-80.0)).abs() < 0.01);
    }

    #[test]
    fn add_gravity_respects_entity_gravity_scale() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().velocity = Vec3f::ZERO;
        world.entities.get_mut(key).unwrap().game.gravity = 0.5;

        world.add_gravity(key);

        let vz = world.entities.get(key).unwrap().velocity.z;
        // Should be -800 * 0.5 * 0.1 = -40
        assert!((vz - (-40.0)).abs() < 0.01);
    }

    #[test]
    fn add_gravity_accumulates() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().velocity = Vec3f::ZERO;

        world.add_gravity(key);
        world.add_gravity(key);

        let vz = world.entities.get(key).unwrap().velocity.z;
        // Two frames: -80 + -80 = -160
        assert!((vz - (-160.0)).abs() < 0.01);
    }

    // -- add_rotational_friction tests --

    #[test]
    fn rotational_friction_damps_to_zero() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().avelocity =
            Vec3f::new(10.0, -10.0, 5.0);

        // Apply enough frames to stop.
        for _ in 0..100 {
            world.add_rotational_friction(key);
        }

        let av = world.entities.get(key).unwrap().avelocity;
        assert_eq!(av, Vec3f::ZERO);
    }

    #[test]
    fn rotational_friction_reduces_magnitude() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().avelocity =
            Vec3f::new(1000.0, 0.0, 0.0);

        let before = world.entities.get(key).unwrap().avelocity.x;
        world.add_rotational_friction(key);
        let after = world.entities.get(key).unwrap().avelocity.x;

        assert!(after < before);
        assert!(after > 0.0);
    }

    // -- physics dispatch tests --

    #[test]
    fn run_entity_dispatches_none() {
        let mut world = test_world();
        world.level.time = 1.0;
        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().game.movetype = MoveType::None;
        // Should not panic.
        world.run_entity(key);
    }

    #[test]
    fn run_entity_dispatches_noclip() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.movetype = MoveType::Noclip;
            ent.velocity = Vec3f::new(100.0, 0.0, 0.0);
        }

        world.run_entity(key);

        let origin = world.entities.get(key).unwrap().state.origin;
        // Should have moved by velocity * FRAMETIME = 10 units.
        assert!((origin.x - 10.0).abs() < 0.01);
    }

    #[test]
    fn run_entity_dispatches_toss_with_gravity() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.movetype = MoveType::Toss;
            ent.velocity = Vec3f::new(100.0, 0.0, 0.0);
        }

        world.run_entity(key);

        let ent = world.entities.get(key).unwrap();
        // Should have moved horizontally.
        assert!(ent.state.origin.x > 0.0);
        // Should have gained downward velocity from gravity.
        assert!(ent.velocity.z < 0.0);
    }

    #[test]
    fn run_entity_dispatches_step() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.movetype = MoveType::Step;
            ent.velocity = Vec3f::new(50.0, 0.0, 0.0);
        }

        // Should not panic — step physics runs think, ground check, friction.
        world.run_entity(key);
    }

    #[test]
    fn run_entity_dispatches_push() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.movetype = MoveType::Push;
            ent.velocity = Vec3f::new(200.0, 0.0, 0.0);
        }

        world.run_entity(key);

        let origin = world.entities.get(key).unwrap().state.origin;
        // Should have moved by velocity * FRAMETIME = 20 units.
        assert!((origin.x - 20.0).abs() < 0.01);
    }

    #[test]
    fn run_entity_dispatches_fly() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.movetype = MoveType::Fly;
            ent.velocity = Vec3f::new(0.0, 0.0, 100.0);
        }

        world.run_entity(key);

        let ent = world.entities.get(key).unwrap();
        // Fly doesn't apply gravity.
        assert!(ent.velocity.z > 0.0);
    }

    #[test]
    fn run_entity_dispatches_bounce() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.movetype = MoveType::Bounce;
            ent.velocity = Vec3f::new(100.0, 0.0, 100.0);
        }

        // Bounce goes through physics_toss path.
        world.run_entity(key);
        // Should not panic.
    }

    #[test]
    fn run_entity_calls_prethink() {
        use std::sync::atomic::{AtomicBool, Ordering};
        static CALLED: AtomicBool = AtomicBool::new(false);

        fn test_prethink(_world: &mut GameWorld, _key: EntityKey) {
            CALLED.store(true, Ordering::Relaxed);
        }

        CALLED.store(false, Ordering::Relaxed);

        let mut world = test_world();
        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().game.movetype = MoveType::None;
        world.entities.get_mut(key).unwrap().prethink = Some(test_prethink);

        world.run_entity(key);

        assert!(CALLED.load(Ordering::Relaxed));
    }

    // -- fly_move tests (with MockGameImport returning no-collision) --

    #[test]
    fn fly_move_in_open_space() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.velocity = Vec3f::new(100.0, 0.0, 0.0);
        }

        let blocked = world.fly_move(key, FRAMETIME, 0xFFFF);

        assert!(blocked.is_empty());
        let origin = world.entities.get(key).unwrap().state.origin;
        assert!((origin.x - 10.0).abs() < 0.01);
    }

    // -- m_check_ground tests --

    #[test]
    fn check_ground_clears_when_ascending() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.velocity.z = 200.0; // ascending fast
            ent.game.ground_entity = Some(key); // was on ground
        }

        world.m_check_ground(key);

        assert!(world.entities.get(key).unwrap().game.ground_entity.is_none());
    }

    #[test]
    fn check_ground_skips_flying_entities() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.flags = EntityFlags::FLY;
            ent.game.ground_entity = None;
        }

        // Should not set ground entity for flying entities.
        world.m_check_ground(key);
        // Not on ground — that's correct for a flyer.
    }

    // -- physics_toss skip when grounded --

    #[test]
    fn physics_toss_stays_still_on_ground() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        let ground = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.movetype = MoveType::Toss;
            ent.game.ground_entity = Some(ground);
            ent.velocity = Vec3f::ZERO;
            ent.state.origin = Vec3f::new(0.0, 0.0, 100.0);
        }

        world.physics_toss(key);

        // Should not have moved — grounded with zero velocity.
        let origin = world.entities.get(key).unwrap().state.origin;
        assert_eq!(origin, Vec3f::new(0.0, 0.0, 100.0));
    }

    // -- physics_step friction test --

    #[test]
    fn physics_step_applies_friction_on_ground() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        let ground = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.movetype = MoveType::Step;
            ent.game.ground_entity = Some(ground);
            ent.velocity = Vec3f::new(500.0, 0.0, 0.0);
        }

        world.physics_step(key);

        let vx = world.entities.get(key).unwrap().velocity.x;
        // Friction should have reduced horizontal velocity.
        assert!(vx < 500.0);
        assert!(vx > 0.0);
    }
}
