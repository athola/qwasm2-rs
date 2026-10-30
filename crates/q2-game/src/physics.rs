//! Game-level physics — entity movement, collision response, and gravity.
//!
//! Port of `g_phys.c`. Constants and thresholds match the C source.
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

use q2_shared::constants::MASK_SOLID;
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
const WATERFRICTION: f32 = 1.0;

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
        /// Stopped dead against more than two planes.
        const DEAD_STOP = 0x04;
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
pub fn clip_velocity(velocity: Vec3f, normal: Vec3f, overbounce: f32) -> (Vec3f, BlockedFlags) {
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

/// C `AngleVectors`, with C's double-precision sin/cos, shared with pmove
/// so every game-side caller rounds the way the original does.
pub(crate) use q2_common::pmove::angle_vectors;

/// The world-space bounds of a (possibly rotated) bmodel's 8 corners, so a
/// rotating pusher only tests what it can really touch.
/// C ref: `RealBoundingBox` (g_phys.c).
fn real_bounding_box(ent: &crate::entity::Entity) -> (Vec3f, Vec3f) {
    let (forward, right, up) = angle_vectors(ent.state.angles);
    let mut mins = Vec3f::splat(f32::INFINITY);
    let mut maxs = Vec3f::splat(f32::NEG_INFINITY);
    for corner in 0..8 {
        let x = if corner & 1 != 0 {
            ent.maxs.x
        } else {
            ent.mins.x
        };
        let y = if corner & 2 != 0 {
            ent.maxs.y
        } else {
            ent.mins.y
        };
        let z = if corner & 4 != 0 {
            ent.maxs.z
        } else {
            ent.mins.z
        };
        let p = ent.state.origin + forward * x + right * -y + up * z;
        mins = mins.min(p);
        maxs = maxs.max(p);
    }
    (mins, maxs)
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
    /// Returns `false` if think was called (the entity may have been freed),
    /// `true` if it was not due.
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

        match think_fn {
            Some(think) => think(self, key),
            // C raises gi.error("NULL ent->think").
            None => tracing::warn!("run_think: entity with nextthink has no think"),
        }
        false
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

        let Some(e2) = e2 else {
            return;
        };

        if let Some(touch) = touch1 {
            touch(self, e1, e2, Some(&trace.plane), trace.surface.as_ref());
        }

        // Read after the first touch: it may have freed e2 (C re-reads
        // `e2->touch`, which G_FreeEdict cleared).
        let touch2 = self
            .entities
            .get(e2)
            .filter(|e| e.in_use && e.solid != Solid::Not)
            .and_then(|e| e.touch);
        if let Some(touch) = touch2 {
            touch(self, e2, e1, None, None);
        }
    }

    /// Apply gravity to an entity for one frame.
    /// C ref: `SV_AddGravity` (g_phys.c:373-382)
    pub fn add_gravity(&mut self, key: EntityKey) {
        let Some(ent) = self.entities.get_mut(key) else {
            return;
        };
        // G_InitEdict sets gravity 1.0; trigger_gravity may set 0.
        ent.velocity.z -= ent.game.gravity * DEFAULT_GRAVITY * FRAMETIME;
    }

    /// Apply friction to angular velocity, spinning entities to rest.
    /// C ref: `SV_AddRotationalFriction` (g_phys.c:1046-1080)
    pub fn add_rotational_friction(&mut self, key: EntityKey) {
        let Some(ent) = self.entities.get_mut(key) else {
            return;
        };

        // C advances the angles before reducing the angular velocity.
        ent.state.angles += ent.avelocity * FRAMETIME;
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
        let mut planes: Vec<Vec3f> = Vec::with_capacity(MAX_CLIP_PLANES);
        let mut time_left = time;

        let (primal_velocity, mut original_velocity) = {
            let Some(ent) = self.entities.get_mut(key) else {
                return blocked;
            };
            ent.game.ground_entity = None;
            (ent.velocity, ent.velocity)
        };

        for _ in 0..MAX_BUMPS {
            let Some(ent) = self.entities.get(key) else {
                break;
            };
            let (origin, velocity, mins, maxs) =
                (ent.state.origin, ent.velocity, ent.mins, ent.maxs);
            let end = origin + velocity * time_left;
            let trace = self
                .gi
                .trace(origin, mins, maxs, end, self.number_of(key), mask);

            if trace.allsolid {
                // Entity is trapped in another solid.
                if let Some(ent) = self.entities.get_mut(key) {
                    ent.velocity = Vec3f::ZERO;
                }
                return BlockedFlags::FLOOR | BlockedFlags::STEP;
            }

            if trace.fraction > 0.0 {
                // Actually covered some distance.
                if let Some(ent) = self.entities.get_mut(key) {
                    ent.state.origin = trace.endpos;
                    original_velocity = ent.velocity;
                }
                planes.clear();
            }

            if trace.fraction == 1.0 {
                break; // Moved the entire distance.
            }

            let hit_key = trace
                .ent_index
                .and_then(|n| self.entities.key_for_number(n));

            if trace.plane.normal.z > GROUND_NORMAL_THRESHOLD {
                blocked |= BlockedFlags::FLOOR;
                let bsp_hit = hit_key
                    .and_then(|k| self.entities.get(k).map(|h| (k, h)))
                    .filter(|(_, h)| h.solid == Solid::Bsp)
                    .map(|(k, h)| (k, h.linkcount));
                if let (Some((hit, linkcount)), Some(ent)) = (bsp_hit, self.entities.get_mut(key)) {
                    ent.game.ground_entity = Some(hit);
                    ent.game.groundentity_linkcount = linkcount;
                }
            }
            if trace.plane.normal.z == 0.0 {
                blocked |= BlockedFlags::STEP;
            }

            self.sv_impact(key, hit_key, &trace);
            if !self.entities.is_live(key) {
                break; // Removed by the impact function.
            }

            time_left -= time_left * trace.fraction;

            if planes.len() >= MAX_CLIP_PLANES {
                // This shouldn't really happen.
                if let Some(ent) = self.entities.get_mut(key) {
                    ent.velocity = Vec3f::ZERO;
                }
                return BlockedFlags::FLOOR | BlockedFlags::STEP;
            }
            planes.push(trace.plane.normal);

            // Find a plane whose clip of original_velocity does not go back
            // into any other (non-identical) plane.
            let along_plane = planes.iter().find_map(|&plane| {
                let (v, _) = clip_velocity(original_velocity, plane, 1.0);
                planes
                    .iter()
                    .all(|&other| other == plane || v.dot(other) >= 0.0)
                    .then_some(v)
            });

            let Some(ent) = self.entities.get_mut(key) else {
                break;
            };
            match along_plane {
                Some(v) => ent.velocity = v,
                None => {
                    // Go along the crease.
                    if planes.len() != 2 {
                        ent.velocity = Vec3f::ZERO;
                        return BlockedFlags::all();
                    }
                    let dir = planes[0].cross(planes[1]);
                    ent.velocity = dir * dir.dot(ent.velocity);
                }
            }

            // If velocity is against the original velocity, stop dead to
            // avoid tiny oscillations in sloping corners.
            if ent.velocity.dot(primal_velocity) <= 0.0 {
                ent.velocity = Vec3f::ZERO;
                return blocked;
            }
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
        let Some(ent) = self.entities.get(key) else {
            return Trace::default();
        };
        let start = ent.state.origin;
        let end = start + push;

        loop {
            let Some(ent) = self.entities.get(key) else {
                return Trace::default();
            };
            let (mins, maxs) = (ent.mins, ent.maxs);
            let mask = if ent.clipmask != 0 {
                ent.clipmask
            } else {
                MASK_SOLID
            };
            let pass = self.number_of(key);
            let mut trace = self.gi.trace(start, mins, maxs, end, pass, mask);
            // startsolid treats different-content volumes as continuous (a
            // monster box and an elevator floor), so retry against BSP only.
            if trace.startsolid && mask & !MASK_SOLID != 0 {
                trace = self.gi.trace(start, mins, maxs, end, pass, MASK_SOLID);
            }

            if let Some(ent) = self.entities.get_mut(key) {
                ent.state.origin = trace.endpos;
                // Push gibs, debris and dead monsters slightly off
                // non-horizontal planes so they don't render black.
                let nudge = (ent.game.classname.starts_with("monster_") && ent.game.health < 1)
                    || ent.game.classname == "debris"
                    || ent.state.effects & EF_GIB != 0;
                if trace.plane.plane_type != 2 && nudge {
                    ent.state.origin += trace.plane.normal;
                }
            }
            self.link_entity(key);

            if trace.fraction != 1.0 {
                let hit_key = trace
                    .ent_index
                    .and_then(|n| self.entities.key_for_number(n));
                self.sv_impact(key, hit_key, &trace);

                // If the pushed entity went away and the pusher is still
                // there, move the pusher back and try again.
                let hit_gone = hit_key.is_some_and(|k| !self.entities.is_live(k));
                if hit_gone && self.entities.is_live(key) {
                    if let Some(ent) = self.entities.get_mut(key) {
                        ent.state.origin = start;
                    }
                    self.link_entity(key);
                    continue;
                }
            }

            if self.entities.is_live(key) {
                self.touch_triggers(key);
            }
            return trace;
        }
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

        self.link_entity(key);
    }

    /// Physics for MOVETYPE_TOSS, BOUNCE, FLY, FLYMISSILE.
    /// Handles projectiles, gibs, and flying entities. When on ground, do
    /// nothing.
    /// C ref: `SV_Physics_Toss` (g_phys.c:894-1030)
    pub fn physics_toss(&mut self, key: EntityKey) {
        self.run_think(key);

        // Entities are very often freed during thinking.
        let Some(ent) = self.entities.get_mut(key).filter(|e| e.in_use) else {
            return;
        };

        // Team slaves are moved by their master.
        if ent.game.flags.contains(EntityFlags::TEAMSLAVE) {
            return;
        }

        if ent.velocity.z > 0.0 {
            ent.game.ground_entity = None;
        }

        // Check for the ground entity going away.
        if let Some(gk) = ent.game.ground_entity {
            if !self.entities.is_live(gk) {
                if let Some(ent) = self.entities.get_mut(key) {
                    ent.game.ground_entity = None;
                }
            }
        }

        let Some(ent) = self.entities.get_mut(key) else {
            return;
        };
        if ent.game.ground_entity.is_some() {
            return;
        }
        let movetype = ent.game.movetype;
        let old_origin = ent.state.origin;

        self.check_velocity(key);

        if movetype != MoveType::Fly && movetype != MoveType::FlyMissile {
            self.add_gravity(key);
        }

        let Some(ent) = self.entities.get_mut(key) else {
            return;
        };
        ent.state.angles += ent.avelocity * FRAMETIME;
        let move_vec = ent.velocity * FRAMETIME;

        let trace = self.push_entity(key, move_vec);
        if !self.entities.is_live(key) {
            return;
        }

        if trace.fraction < 1.0 {
            let hit = trace
                .ent_index
                .and_then(|n| self.entities.key_for_number(n));
            let hit_linkcount = hit
                .and_then(|k| self.entities.get(k))
                .map_or(0, |h| h.linkcount);
            let Some(ent) = self.entities.get_mut(key) else {
                return;
            };
            let backoff = if movetype == MoveType::Bounce {
                1.5
            } else {
                1.0
            };
            ent.velocity = clip_velocity(ent.velocity, trace.plane.normal, backoff).0;

            // Stop if on ground.
            if trace.plane.normal.z > GROUND_NORMAL_THRESHOLD
                && (ent.velocity.z < 60.0 || movetype != MoveType::Bounce)
            {
                ent.game.ground_entity = hit;
                ent.game.groundentity_linkcount = hit_linkcount;
                ent.velocity = Vec3f::ZERO;
                ent.avelocity = Vec3f::ZERO;
            }
        }

        // Check for water transition.
        let origin = self
            .entities
            .get(key)
            .map_or(Vec3f::ZERO, |e| e.state.origin);
        let contents = self.gi.point_contents(origin);
        let Some(ent) = self.entities.get_mut(key) else {
            return;
        };
        let was_in_water = ent.game.watertype & MASK_WATER != 0;
        ent.game.watertype = contents;
        let is_in_water = contents & MASK_WATER != 0;
        ent.game.waterlevel = i32::from(is_in_water);
        // The splash plays from the world where the entity entered (its
        // origin before the move) or left the water. Entities already in
        // water at level start make no splash.
        if is_in_water != was_in_water && (was_in_water || self.level.framenum > 3) {
            let splash = self.gi.sound_index("misc/h2ohit1.wav");
            let at = if is_in_water { old_origin } else { origin };
            self.gi.positioned_sound(at, 0, 0, splash, 1.0, 1.0, 0.0); // CHAN_AUTO
        }

        // Move team slaves.
        let mut slave = self.entities.get(key).and_then(|e| e.game.teamchain);
        while let Some(sk) = slave {
            let Some(s) = self.entities.get_mut(sk) else {
                break;
            };
            s.state.origin = origin;
            slave = s.game.teamchain;
            self.link_entity(sk);
        }
    }

    /// Physics for MOVETYPE_STEP: monsters free-fall when they have no
    /// ground entity, otherwise all movement is done with discrete steps.
    /// Also used for objects that came to rest but fall if the floor is
    /// pulled out from under them.
    /// C ref: `SV_Physics_Step` (g_phys.c:1082-1259)
    pub fn physics_step(&mut self, key: EntityKey) {
        // Airborne monsters should always check for ground.
        if self
            .entities
            .get(key)
            .is_some_and(|e| e.game.ground_entity.is_none())
        {
            self.m_check_ground(key);
        }

        let Some(ent) = self.entities.get(key) else {
            return;
        };
        let was_on_ground = ent.game.ground_entity.is_some();

        self.check_velocity(key);

        if self
            .entities
            .get(key)
            .is_some_and(|e| e.avelocity != Vec3f::ZERO)
        {
            self.add_rotational_friction(key);
        }

        let Some(ent) = self.entities.get(key) else {
            return;
        };
        let flags = ent.game.flags;
        let waterlevel = ent.game.waterlevel;
        let is_flying = flags.contains(EntityFlags::FLY);
        let is_swimming = flags.contains(EntityFlags::SWIM);

        // Add gravity except for flying monsters and swimming monsters who
        // are in the water.
        let mut hitsound = false;
        if !was_on_ground && !is_flying && !(is_swimming && waterlevel > 2) {
            hitsound = ent.velocity.z < DEFAULT_GRAVITY * -0.1;
            if waterlevel == 0 {
                self.add_gravity(key);
            }
        }

        let Some(ent) = self.entities.get_mut(key) else {
            return;
        };

        // Vertical friction for flying monsters given vertical velocity.
        if is_flying && ent.velocity.z != 0.0 {
            let speed = ent.velocity.z.abs();
            let control = speed.max(STOPSPEED);
            let newspeed = (speed - FRAMETIME * control * (FRICTION / 3.0)).max(0.0);
            ent.velocity.z *= newspeed / speed;
        }

        // Vertical friction for swimming monsters.
        if is_swimming && ent.velocity.z != 0.0 {
            let speed = ent.velocity.z.abs();
            let control = speed.max(STOPSPEED);
            let newspeed =
                (speed - FRAMETIME * control * WATERFRICTION * waterlevel as f32).max(0.0);
            ent.velocity.z *= newspeed / speed;
        }

        if ent.velocity != Vec3f::ZERO {
            // Apply friction. C also lets dead monsters that are not
            // completely on ground slide (M_CheckBottom, not ported yet).
            if was_on_ground || is_flying || is_swimming {
                let speed = Vec3f::new(ent.velocity.x, ent.velocity.y, 0.0).length();
                if speed != 0.0 {
                    let control = speed.max(STOPSPEED);
                    let newspeed = (speed - FRAMETIME * control * FRICTION).max(0.0) / speed;
                    ent.velocity.x *= newspeed;
                    ent.velocity.y *= newspeed;
                }
            }

            let mask = if ent.svflags & q2_shared::constants::SVF_MONSTER != 0 {
                MASK_MONSTERSOLID
            } else {
                MASK_SOLID
            };

            let old_origin = ent.state.origin;
            self.fly_move(key, FRAMETIME, mask);

            // Undo a move that left the entity stuck in the world (C works
            // around dead monsters falling through the world model).
            if let Some(ent) = self.entities.get(key) {
                if ent.state.origin != old_origin {
                    let (origin, mins, maxs) = (ent.state.origin, ent.mins, ent.maxs);
                    let tr = self
                        .gi
                        .trace(origin, mins, maxs, origin, self.number_of(key), mask);
                    if tr.startsolid {
                        if let Some(ent) = self.entities.get_mut(key) {
                            ent.state.origin = old_origin;
                        }
                    }
                }
            }

            self.link_entity(key);
            self.touch_triggers(key);
            if !self.entities.is_live(key) {
                return;
            }

            let landed = self
                .entities
                .get(key)
                .is_some_and(|e| e.game.ground_entity.is_some());
            if landed && !was_on_ground && hitsound {
                let land = self.gi.sound_index("world/land.wav");
                self.gi.sound(self.number_of(key), 0, land, 1.0, 1.0, 0.0);
            }
        }

        self.run_think(key);
    }

    /// `true` if the entity's box is stuck in something solid at its
    /// current origin. C ref: `SV_TestEntityPosition` (which returns the
    /// world edict instead of `true`).
    fn test_entity_position(&self, key: EntityKey) -> bool {
        let Some(ent) = self.entities.get(key) else {
            return false;
        };
        let mask = if ent.clipmask != 0 {
            ent.clipmask
        } else {
            MASK_SOLID
        };
        let origin = ent.state.origin;
        self.gi
            .trace(
                origin,
                ent.mins,
                ent.maxs,
                origin,
                self.number_of(key),
                mask,
            )
            .startsolid
    }

    /// Move `pusher` by `move_vec`/`amove` and push or carry every entity
    /// in its way. On a block, every entity in `pushed` (this whole team
    /// move) is put back and the obstacle is returned.
    /// C ref: `SV_Push` (g_phys.c).
    fn sv_push(
        &mut self,
        pusher: EntityKey,
        mut move_vec: Vec3f,
        amove: Vec3f,
        pushed: &mut Vec<(EntityKey, Vec3f, Vec3f)>,
    ) -> Result<(), EntityKey> {
        // Clamp the move to 1/8 units, so the position will be accurate
        // for client side prediction.
        for c in [&mut move_vec.x, &mut move_vec.y, &mut move_vec.z] {
            let temp = *c * 8.0;
            let temp = if temp > 0.0 { temp + 0.5 } else { temp - 0.5 };
            *c = 0.125 * (temp as i32) as f32;
        }

        // We need this for pushing things later.
        let (forward, right, up) = angle_vectors(-amove);

        // Save the pusher's original position, then move it.
        let Some(p) = self.entities.get_mut(pusher) else {
            return Ok(());
        };
        pushed.push((pusher, p.state.origin, p.state.angles));
        p.state.origin += move_vec;
        p.state.angles += amove;
        let pusher_movetype = p.game.movetype;
        self.link_entity(pusher);
        let Some(p) = self.entities.get(pusher) else {
            return Ok(());
        };
        let pusher_origin = p.state.origin;
        let (realmins, realmaxs) = real_bounding_box(p);

        // See if any solid entities are inside the final position.
        let checks: Vec<EntityKey> = self.entities.keys_from(1).collect();
        for check in checks {
            let Some(c) = self.entities.get(check) else {
                continue;
            };
            if !c.in_use
                || matches!(
                    c.game.movetype,
                    MoveType::Push | MoveType::Stop | MoveType::None | MoveType::Noclip
                )
                || c.linkcount == 0
            {
                continue; // Not a pushable, or not linked in anywhere.
            }

            // If the entity is standing on the pusher, it will definitely
            // be moved.
            let riding = c.game.ground_entity == Some(pusher);
            if !riding {
                // See if the ent needs to be tested.
                if c.absmin.x >= realmaxs.x
                    || c.absmin.y >= realmaxs.y
                    || c.absmin.z >= realmaxs.z
                    || c.absmax.x <= realmins.x
                    || c.absmax.y <= realmins.y
                    || c.absmax.z <= realmins.z
                {
                    continue;
                }
                // See if the ent's bbox is inside the pusher's final position.
                if !self.test_entity_position(check) {
                    continue;
                }
            }

            if pusher_movetype == MoveType::Push || riding {
                // Move this entity.
                let Some(c) = self.entities.get_mut(check) else {
                    continue;
                };
                pushed.push((check, c.state.origin, c.state.angles));
                c.state.origin += move_vec;
                // Figure movement due to the pusher's amove.
                let org = c.state.origin - pusher_origin;
                let org2 = Vec3f::new(org.dot(forward), -org.dot(right), org.dot(up));
                c.state.origin += org2 - org;
                // May have pushed them off an edge.
                if !riding {
                    c.game.ground_entity = None;
                }

                if !self.test_entity_position(check) {
                    // Pushed ok.
                    self.link_entity(check);
                    continue;
                }

                // If it is ok to leave in the old position, do it. This is
                // only relevant for riding entities, not pushed.
                if let Some(c) = self.entities.get_mut(check) {
                    c.state.origin -= move_vec;
                }
                if !self.test_entity_position(check) {
                    pushed.pop();
                    continue;
                }
            }

            // Move back any entities we already moved; go backwards, so if
            // the same entity was pushed twice it ends at its original
            // position.
            for &(k, origin, angles) in pushed.iter().rev() {
                if let Some(e) = self.entities.get_mut(k) {
                    e.state.origin = origin;
                    e.state.angles = angles;
                }
                self.link_entity(k);
            }
            return Err(check);
        }

        // See if anything we moved has touched a trigger.
        for &(k, _, _) in pushed.iter().rev() {
            self.touch_triggers(k);
        }
        Ok(())
    }

    /// Physics for MOVETYPE_PUSH / MOVETYPE_STOP — doors, platforms. Bmodels
    /// don't interact with each other, but push all box entities. The team
    /// master moves its whole team; if any part is blocked, every move is
    /// backed out, think times are bumped and the blocked part's `blocked`
    /// callback runs.
    /// C ref: `SV_Physics_Pusher` (g_phys.c:772-844)
    pub fn physics_pusher(&mut self, key: EntityKey) {
        // Not a team captain: movement is handled by the master.
        if self
            .entities
            .get(key)
            .is_none_or(|e| e.game.flags.contains(EntityFlags::TEAMSLAVE))
        {
            return;
        }

        let team = self.team_chain(key);
        let mut pushed = Vec::new();
        let mut blocked = None;
        for &part in &team {
            let Some(e) = self.entities.get(part) else {
                continue;
            };
            if e.velocity != Vec3f::ZERO || e.avelocity != Vec3f::ZERO {
                let (move_vec, amove) = (e.velocity * FRAMETIME, e.avelocity * FRAMETIME);
                if let Err(obstacle) = self.sv_push(part, move_vec, amove, &mut pushed) {
                    blocked = Some((part, obstacle));
                    break; // Move was blocked.
                }
            }
        }

        match blocked {
            Some((part, obstacle)) => {
                // The move failed: bump all nextthink times.
                for &mv in &team {
                    if let Some(e) = self.entities.get_mut(mv) {
                        if e.game.nextthink > 0.0 {
                            e.game.nextthink += FRAMETIME;
                        }
                    }
                }
                // Call the blocked function, or just stay in place until
                // the obstacle is gone.
                if let Some(blocked_fn) = self.entities.get(part).and_then(|e| e.blocked) {
                    blocked_fn(self, part, obstacle);
                }
            }
            None => {
                // The move succeeded, so call all think functions.
                for part in team {
                    self.run_think(part);
                }
            }
        }
    }

    /// `key`, then each `teamchain` member, stopping at a repeat.
    fn team_chain(&self, key: EntityKey) -> Vec<EntityKey> {
        let mut team = vec![key];
        let mut next = self.entities.get(key).and_then(|e| e.game.teamchain);
        while let Some(k) = next.filter(|k| !team.contains(k)) {
            team.push(k);
            next = self.entities.get(k).and_then(|e| e.game.teamchain);
        }
        team
    }

    // -----------------------------------------------------------------------
    // Ground checking — M_CheckGround
    // C ref: g_monster.c:223-267
    // -----------------------------------------------------------------------

    /// If the hull point a quarter unit down is solid, the entity is on
    /// ground: snap to it and record the ground entity.
    /// C ref: `M_CheckGround` (g_monster.c)
    pub fn m_check_ground(&mut self, key: EntityKey) {
        let Some(ent) = self.entities.get_mut(key) else {
            return;
        };
        if ent
            .game
            .flags
            .intersects(EntityFlags::FLY | EntityFlags::SWIM)
        {
            return;
        }
        if ent.velocity.z > 100.0 {
            ent.game.ground_entity = None;
            return;
        }

        let (origin, mins, maxs) = (ent.state.origin, ent.mins, ent.maxs);
        let point = Vec3f::new(origin.x, origin.y, origin.z - 0.25);
        let trace = self.gi.trace(
            origin,
            mins,
            maxs,
            point,
            self.number_of(key),
            MASK_MONSTERSOLID,
        );
        let hit = trace
            .ent_index
            .and_then(|n| self.entities.key_for_number(n));
        let hit_linkcount = hit
            .and_then(|k| self.entities.get(k))
            .map_or(0, |h| h.linkcount);
        let Some(ent) = self.entities.get_mut(key) else {
            return;
        };

        // Check steepness.
        if trace.plane.normal.z < GROUND_NORMAL_THRESHOLD && !trace.startsolid {
            ent.game.ground_entity = None;
            return;
        }

        if !trace.startsolid && !trace.allsolid {
            ent.state.origin = trace.endpos;
            ent.game.ground_entity = hit;
            ent.game.groundentity_linkcount = hit_linkcount;
            ent.velocity.z = 0.0;
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
            MoveType::Toss | MoveType::Bounce | MoveType::Fly | MoveType::FlyMissile => {
                self.physics_toss(key)
            }
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
    use crate::world::{test_world, test_world_recording};

    fn mark_touched(
        world: &mut GameWorld,
        self_key: EntityKey,
        _other: EntityKey,
        _plane: Option<&Plane>,
        _surf: Option<&Surface>,
    ) {
        if let Some(ent) = world.entities.get_mut(self_key) {
            ent.game.count = 99;
        }
    }

    #[test]
    fn push_entity_traces_with_its_own_number_as_pass_entity() {
        let (mut world, gi) = test_world_recording();
        let key = world.spawn().unwrap();
        let number = world.number_of(key);
        world.push_entity(key, Vec3f::new(10.0, 0.0, 0.0));
        assert_eq!(gi.trace_pass_ents.borrow().first(), Some(&number));
    }

    #[test]
    fn push_entity_touches_the_entity_the_trace_hit() {
        let (mut world, gi) = test_world_recording();
        let mover = world.spawn().unwrap();
        let wall = world.spawn().unwrap();
        world.entities.get_mut(mover).unwrap().solid = Solid::Bbox;
        {
            let w = world.entities.get_mut(wall).unwrap();
            w.solid = Solid::Bbox;
            w.touch = Some(mark_touched);
        }
        *gi.next_trace.borrow_mut() = Some(Trace {
            fraction: 0.5,
            ent_index: world.number_of(wall),
            ..Default::default()
        });
        world.push_entity(mover, Vec3f::new(10.0, 0.0, 0.0));
        assert_eq!(world.entities.get(wall).unwrap().game.count, 99);
    }

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
        world.entities.get_mut(key).unwrap().velocity = Vec3f::new(3000.0, 0.0, 0.0);

        world.check_velocity(key);

        let speed = world.entities.get(key).unwrap().velocity.length();
        assert!((speed - DEFAULT_MAX_VELOCITY).abs() < 1.0);
    }

    #[test]
    fn check_velocity_preserves_direction() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().velocity = Vec3f::new(3000.0, 4000.0, 0.0);

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
        fn test_think(world: &mut GameWorld, key: EntityKey) {
            world.entities.get_mut(key).unwrap().game.count += 1;
        }

        let mut world = test_world();
        world.level.time = 1.0;

        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().game.nextthink = 0.5;
        world.entities.get_mut(key).unwrap().think = Some(test_think);

        let result = world.run_think(key);

        assert!(world.entities.get(key).unwrap().game.count != 0);
        // C SV_RunThink returns false whenever it called think.
        assert!(!result);
        // nextthink should be cleared.
        assert_eq!(world.entities.get(key).unwrap().game.nextthink, 0.0);
    }

    #[test]
    fn run_think_skips_when_not_due() {
        fn test_think(world: &mut GameWorld, key: EntityKey) {
            world.entities.get_mut(key).unwrap().game.count += 1;
        }

        let mut world = test_world();
        world.level.time = 1.0;

        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().game.nextthink = 2.0; // future
        world.entities.get_mut(key).unwrap().think = Some(test_think);

        world.run_think(key);

        assert!(world.entities.get(key).unwrap().game.count == 0);
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
        world.entities.get_mut(key).unwrap().avelocity = Vec3f::new(10.0, -10.0, 5.0);

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
        world.entities.get_mut(key).unwrap().avelocity = Vec3f::new(1000.0, 0.0, 0.0);

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
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.movetype = MoveType::None;
            ent.velocity = Vec3f::new(100.0, 0.0, 0.0);
        }
        world.run_entity(key);
        // MOVETYPE_NONE only thinks: velocity never moves it.
        assert_eq!(world.entities.get(key).unwrap().state.origin, Vec3f::ZERO);
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

        world.run_entity(key);
        // Airborne (the mock world has no floor): moves and falls.
        let ent = world.entities.get(key).unwrap();
        assert!(ent.state.origin.x > 0.0);
        assert!(ent.velocity.z < 0.0);
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
        let ent = world.entities.get(key).unwrap();
        assert!((ent.state.origin.x - 10.0).abs() < 1e-3);
        assert!((ent.velocity.z - 20.0).abs() < 1e-3, "100 - 800 * 0.1");
    }

    #[test]
    fn run_entity_calls_prethink() {
        fn test_prethink(world: &mut GameWorld, key: EntityKey) {
            world.entities.get_mut(key).unwrap().game.count += 1;
        }

        let mut world = test_world();
        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().game.movetype = MoveType::None;
        world.entities.get_mut(key).unwrap().prethink = Some(test_prethink);

        world.run_entity(key);

        assert!(world.entities.get(key).unwrap().game.count != 0);
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

        let blocked = world.fly_move(key, FRAMETIME, MASK_SOLID);

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

        assert!(world
            .entities
            .get(key)
            .unwrap()
            .game
            .ground_entity
            .is_none());
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
        let ent = world.entities.get(key).unwrap();
        assert!(ent.game.ground_entity.is_none());
        assert_eq!(ent.state.origin, Vec3f::ZERO);
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
    // -- C-parity regressions --

    fn free_self(world: &mut GameWorld, key: EntityKey) {
        world.free_entity(key);
    }

    fn mark_other(
        world: &mut GameWorld,
        _self_key: EntityKey,
        other: EntityKey,
        _plane: Option<&Plane>,
        _surf: Option<&Surface>,
    ) {
        if let Some(ent) = world.entities.get_mut(other) {
            ent.game.count = 7;
        }
    }

    fn free_other(
        world: &mut GameWorld,
        _self_key: EntityKey,
        other: EntityKey,
        _plane: Option<&Plane>,
        _surf: Option<&Surface>,
    ) {
        world.free_entity(other);
    }

    fn record_other_velocity(
        world: &mut GameWorld,
        self_key: EntityKey,
        other: EntityKey,
        _plane: Option<&Plane>,
        _surf: Option<&Surface>,
    ) {
        let vx = world.entities.get(other).unwrap().velocity.x;
        world.entities.get_mut(self_key).unwrap().game.count = vx as i32;
    }

    /// A world entity at number 0, solid like the world model.
    fn spawn_world(world: &mut GameWorld) -> EntityKey {
        let key = world.entities.spawn_numbered(0).unwrap();
        world.entities.get_mut(key).unwrap().solid = Solid::Bsp;
        key
    }

    fn floor_hit(fraction: f32, ent_index: usize) -> Trace {
        Trace {
            fraction,
            ent_index: Some(ent_index),
            plane: Plane {
                normal: Vec3f::new(0.0, 0.0, 1.0),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn noclip_entity_does_not_move_on_the_frame_its_think_runs() {
        fn noop_think(_world: &mut GameWorld, _key: EntityKey) {}
        let mut world = test_world();
        world.level.time = 1.0;
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.movetype = MoveType::Noclip;
            ent.velocity = Vec3f::new(100.0, 0.0, 0.0);
            ent.game.nextthink = 1.0;
            ent.think = Some(noop_think);
        }
        world.run_entity(key);
        assert_eq!(world.entities.get(key).unwrap().state.origin, Vec3f::ZERO);
    }

    #[test]
    fn toss_entity_freed_by_its_think_is_not_moved() {
        let (mut world, gi) = test_world_recording();
        world.level.time = 1.0;
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.movetype = MoveType::Toss;
            ent.velocity = Vec3f::new(100.0, 0.0, 0.0);
            ent.game.nextthink = 1.0;
            ent.think = Some(free_self);
        }
        world.run_entity(key);
        assert!(gi.trace_pass_ents.borrow().is_empty(), "no move traced");
    }

    #[test]
    fn toss_entity_landing_on_the_world_records_it_and_stops_simulating() {
        let (mut world, gi) = test_world_recording();
        let ground = spawn_world(&mut world);
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.movetype = MoveType::Toss;
            ent.velocity = Vec3f::new(0.0, 0.0, -100.0);
        }
        *gi.next_trace.borrow_mut() = Some(floor_hit(0.5, 0));
        world.physics_toss(key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.ground_entity, Some(ground));
        assert_eq!(ent.velocity, Vec3f::ZERO);

        let traces = gi.trace_pass_ents.borrow().len();
        world.physics_toss(key);
        assert_eq!(
            gi.trace_pass_ents.borrow().len(),
            traces,
            "resting item not re-traced"
        );
    }

    #[test]
    fn toss_entity_on_a_freed_ground_entity_falls() {
        let mut world = test_world();
        let ground = world.spawn().unwrap();
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.movetype = MoveType::Toss;
            ent.game.ground_entity = Some(ground);
        }
        world.free_entity(ground);
        world.physics_toss(key);
        let ent = world.entities.get(key).unwrap();
        assert!(ent.game.ground_entity.is_none());
        assert!(ent.velocity.z < 0.0);
    }

    #[test]
    fn bouncing_entity_rising_off_the_floor_keeps_bouncing() {
        let (mut world, gi) = test_world_recording();
        spawn_world(&mut world);
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.movetype = MoveType::Bounce;
            ent.velocity = Vec3f::new(0.0, 0.0, -300.0);
        }
        *gi.next_trace.borrow_mut() = Some(floor_hit(0.5, 0));
        world.physics_toss(key);
        // (-380 after gravity) * -0.5 = +190 > 60: still bouncing.
        let ent = world.entities.get(key).unwrap();
        assert!(ent.game.ground_entity.is_none());
        assert!(ent.velocity.z > 60.0);
    }

    #[test]
    fn toss_entity_entering_water_records_its_water_level() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().game.movetype = MoveType::Toss;
        world.entities.get_mut(key).unwrap().game.watertype = CONTENTS_WATER;
        world.physics_toss(key);
        // The mock world is empty, so the entity has left the water.
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.watertype, 0);
        assert_eq!(ent.game.waterlevel, 0);
    }

    #[test]
    fn toss_entity_leaving_water_splashes_from_the_world_at_its_new_origin() {
        let (mut world, gi) = test_world_recording();
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.movetype = MoveType::Toss;
            ent.game.watertype = CONTENTS_WATER;
            ent.state.origin = Vec3f::new(0.0, 0.0, 100.0);
            ent.velocity = Vec3f::new(0.0, 0.0, 300.0);
        }
        world.physics_toss(key);
        let moved_to = world.entities.get(key).unwrap().state.origin;
        assert_ne!(moved_to, Vec3f::new(0.0, 0.0, 100.0));
        let splash = gi
            .sounds
            .borrow()
            .iter()
            .position(|s| s == "misc/h2ohit1.wav");
        let splash = splash.map(|p| p as i32 + 1).unwrap();
        assert_eq!(
            gi.positioned_sounds.borrow().clone(),
            vec![(moved_to, 0, 0, splash)]
        );
    }

    #[test]
    fn grounded_monster_knocked_upward_falls_back_under_gravity() {
        let mut world = test_world();
        let ground = spawn_world(&mut world);
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.movetype = MoveType::Step;
            ent.game.ground_entity = Some(ground);
            ent.velocity = Vec3f::new(0.0, 0.0, 50.0);
        }
        world.physics_step(key);
        assert!(
            world
                .entities
                .get(key)
                .unwrap()
                .game
                .ground_entity
                .is_none(),
            "SV_FlyMove clears the ground entity"
        );
        world.physics_step(key);
        assert!(world.entities.get(key).unwrap().velocity.z < 50.0);
    }

    #[test]
    fn fly_move_stands_on_bsp_floors_only() {
        let (mut world, gi) = test_world_recording();
        let ground = spawn_world(&mut world);
        let key = world.spawn().unwrap();
        let other = world.spawn().unwrap();
        world.entities.get_mut(other).unwrap().solid = Solid::Bbox;
        world.entities.get_mut(key).unwrap().velocity = Vec3f::new(0.0, 0.0, -100.0);
        let other_number = world.number_of(other).unwrap();

        *gi.next_trace.borrow_mut() = Some(floor_hit(0.5, other_number));
        let blocked = world.fly_move(key, FRAMETIME, MASK_SOLID);
        assert!(blocked.contains(BlockedFlags::FLOOR));
        assert!(world
            .entities
            .get(key)
            .unwrap()
            .game
            .ground_entity
            .is_none());

        world.entities.get_mut(key).unwrap().velocity = Vec3f::new(0.0, 0.0, -100.0);
        world.entities.get_mut(ground).unwrap().linkcount = 4;
        *gi.next_trace.borrow_mut() = Some(floor_hit(0.5, 0));
        world.fly_move(key, FRAMETIME, MASK_SOLID);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.ground_entity, Some(ground));
        assert_eq!(ent.game.groundentity_linkcount, 4);
    }

    #[test]
    fn fly_move_impact_sees_velocity_already_clipped_by_an_earlier_plane() {
        let (mut world, gi) = test_world_recording();
        let mover = world.spawn().unwrap();
        let wall = world.spawn().unwrap();
        let toucher = world.spawn().unwrap();
        world.entities.get_mut(mover).unwrap().velocity = Vec3f::new(100.0, 50.0, 0.0);
        for k in [wall, toucher] {
            world.entities.get_mut(k).unwrap().solid = Solid::Bbox;
        }
        {
            let t = world.entities.get_mut(toucher).unwrap();
            t.touch = Some(record_other_velocity);
            t.game.count = -1;
        }
        let wall_hit = Trace {
            fraction: 0.5,
            ent_index: world.number_of(wall),
            plane: Plane {
                normal: Vec3f::new(-1.0, 0.0, 0.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let second = Trace {
            fraction: 0.5,
            ent_index: world.number_of(toucher),
            plane: Plane {
                normal: Vec3f::new(0.0, -1.0, 0.0),
                ..Default::default()
            },
            ..Default::default()
        };
        gi.queued_traces.borrow_mut().extend([wall_hit, second]);
        world.fly_move(mover, FRAMETIME, MASK_SOLID);
        assert_eq!(world.entities.get(toucher).unwrap().game.count, 0);
    }

    #[test]
    fn walking_monster_moves_and_checks_ground_with_the_monster_mask() {
        let (mut world, gi) = test_world_recording();
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.movetype = MoveType::Step;
            ent.svflags |= q2_shared::constants::SVF_MONSTER;
            ent.velocity = Vec3f::new(50.0, 0.0, 0.0);
        }
        world.physics_step(key);
        let masks = gi.trace_masks.borrow();
        assert!(!masks.is_empty());
        assert!(masks.iter().all(|&m| m == MASK_MONSTERSOLID), "{masks:x?}");
        assert_ne!(
            MASK_MONSTERSOLID & q2_shared::constants::CONTENTS_MONSTER,
            0
        );
    }

    #[test]
    fn non_monster_step_entity_moves_with_the_solid_mask() {
        let (mut world, gi) = test_world_recording();
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.movetype = MoveType::Step;
            ent.game.flags = EntityFlags::FLY;
            ent.velocity = Vec3f::new(50.0, 0.0, 0.0);
        }
        world.physics_step(key);
        let masks = gi.trace_masks.borrow();
        assert!(!masks.is_empty());
        assert!(masks.iter().all(|&m| m == MASK_SOLID), "{masks:x?}");
    }

    #[test]
    fn push_entity_defaults_to_the_solid_mask_and_touches_triggers() {
        let (mut world, gi) = test_world_recording();
        let mover = world.spawn().unwrap();
        let trigger = world.spawn().unwrap();
        world.entities.get_mut(trigger).unwrap().touch = Some(mark_touched);
        gi.box_edicts
            .borrow_mut()
            .push(world.number_of(trigger).unwrap());
        world.push_entity(mover, Vec3f::new(10.0, 0.0, 0.0));
        assert_eq!(*gi.trace_masks.borrow(), [MASK_SOLID]);
        assert_eq!(world.entities.get(trigger).unwrap().game.count, 99);
    }

    #[test]
    fn push_entity_retries_when_the_entity_it_hit_was_freed() {
        let (mut world, gi) = test_world_recording();
        let mover = world.spawn().unwrap();
        let victim = world.spawn().unwrap();
        world.entities.get_mut(mover).unwrap().touch = Some(free_other);
        world.entities.get_mut(mover).unwrap().solid = Solid::Bbox;
        *gi.next_trace.borrow_mut() = Some(Trace {
            fraction: 0.5,
            ent_index: world.number_of(victim),
            ..Default::default()
        });
        let trace = world.push_entity(mover, Vec3f::new(10.0, 0.0, 0.0));
        assert_eq!(trace.fraction, 1.0);
        assert_eq!(world.entities.get(mover).unwrap().state.origin.x, 10.0);
    }

    #[test]
    fn impact_does_not_call_the_touch_of_an_entity_the_first_touch_freed() {
        let mut world = test_world();
        let e1 = world.spawn().unwrap();
        let e2 = world.spawn().unwrap();
        {
            let a = world.entities.get_mut(e1).unwrap();
            a.solid = Solid::Bbox;
            a.touch = Some(free_other);
        }
        {
            let b = world.entities.get_mut(e2).unwrap();
            b.solid = Solid::Bbox;
            b.touch = Some(mark_other);
        }
        world.sv_impact(e1, Some(e2), &Trace::default());
        assert_ne!(world.entities.get(e1).unwrap().game.count, 7);
    }

    #[test]
    fn zero_gravity_entity_does_not_fall() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().game.gravity = 0.0;
        world.add_gravity(key);
        assert_eq!(world.entities.get(key).unwrap().velocity.z, 0.0);
    }

    #[test]
    fn rotational_friction_turns_by_the_velocity_before_damping() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().avelocity = Vec3f::new(0.0, 1000.0, 0.0);
        world.add_rotational_friction(key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.state.angles.y, 100.0);
        assert_eq!(ent.avelocity.y, 940.0);
    }
}
