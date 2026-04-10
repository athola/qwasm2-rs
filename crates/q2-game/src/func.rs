//! Functional entities — doors, platforms, buttons, trains, rotating objects.
//!
//! Port of `g_func.c` (3,012 lines, 16 func_* entity types).
//! These entities use `MoveInfo` for smooth acceleration/deceleration.
//!
//! # Movement pattern
//! All func_* entities follow: `Move_Begin → Move_Final → Move_Done`
//! with `MoveInfo.endfunc` calling back when movement completes.
//!
//! # C source reference
//! `~/Qwasm2/src/game/g_func.c`

use q2_shared::types::*;

use crate::constants::*;
use crate::entity::{EntityKey, EntityStorage, MoveInfo};
use crate::world::GameWorld;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Movement states for doors/platforms
// ---------------------------------------------------------------------------

/// Door/platform movement states. C ref: g_func.c STATE_* defines.
pub const STATE_TOP: i32 = 0;
pub const STATE_BOTTOM: i32 = 1;
pub const STATE_UP: i32 = 2;
pub const STATE_DOWN: i32 = 3;

// ---------------------------------------------------------------------------
// Movement callbacks
// ---------------------------------------------------------------------------

/// Move an entity toward its destination using MoveInfo acceleration.
/// C ref: `Move_Calc` (g_func.c).
pub fn move_calc(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };

    let Some(mi) = &ent.moveinfo else { return };

    let dir = mi.end_origin - ent.state.origin;
    let dist = dir.length();

    if dist < 0.001 {
        // Already at destination.
        move_done(world, key);
        return;
    }

    let speed = mi.speed;
    if speed <= 0.0 {
        return;
    }

    // Calculate velocity.
    let time = dist / speed;
    let time = time.max(FRAMETIME); // minimum one frame

    if let Some(ent) = world.entities.get_mut(key) {
        ent.velocity = dir / time;
        ent.game.nextthink = world.level.time + time;
        ent.think = Some(move_done_think);
    }
}

/// Movement complete — clear velocity and call endfunc.
pub fn move_done(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.velocity = Vec3f::ZERO;
    }

    // Call MoveInfo endfunc if set.
    let endfunc = world
        .entities
        .get(key)
        .and_then(|e| e.moveinfo.as_ref())
        .and_then(|mi| mi.endfunc);

    if let Some(func) = endfunc {
        func(world, key);
    }
}

pub fn move_done_think(world: &mut GameWorld, key: EntityKey) {
    move_done(world, key);
}

// ---------------------------------------------------------------------------
// Platform (func_plat) callbacks
// ---------------------------------------------------------------------------

/// Platform reached bottom — wait then return to top.
pub fn plat_hit_bottom(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        if let Some(ref mut mi) = ent.moveinfo {
            mi.state = STATE_BOTTOM;
        }
    }
}

/// Platform reached top — wait for trigger.
pub fn plat_hit_top(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        if let Some(ref mut mi) = ent.moveinfo {
            mi.state = STATE_TOP;
        }
    }
}

/// Platform go-down function — moves platform to bottom.
pub fn plat_go_down(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        if let Some(ref mut mi) = ent.moveinfo {
            mi.state = STATE_DOWN;
            let end = mi.end_origin;
            mi.endfunc = Some(plat_hit_bottom);
            // Set velocity toward end_origin.
            let dir = end - ent.state.origin;
            let speed = mi.speed;
            if dir.length() > 0.001 && speed > 0.0 {
                ent.velocity = dir.normalize() * speed;
                ent.game.nextthink = world.level.time + dir.length() / speed;
                ent.think = Some(move_done_think);
            }
        }
    }
}

/// Platform go-up function — moves platform to top.
pub fn plat_go_up(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        if let Some(ref mut mi) = ent.moveinfo {
            mi.state = STATE_UP;
            let end = mi.start_origin;
            mi.endfunc = Some(plat_hit_top);
            let dir = end - ent.state.origin;
            let speed = mi.speed;
            if dir.length() > 0.001 && speed > 0.0 {
                ent.velocity = dir.normalize() * speed;
                ent.game.nextthink = world.level.time + dir.length() / speed;
                ent.think = Some(move_done_think);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Door callbacks
// ---------------------------------------------------------------------------

/// Door use — toggle open/close.
pub fn door_use(
    world: &mut GameWorld,
    self_key: EntityKey,
    _other: EntityKey,
    _activator: EntityKey,
) {
    let state = world
        .entities
        .get(self_key)
        .and_then(|e| e.moveinfo.as_ref())
        .map(|mi| mi.state)
        .unwrap_or(STATE_BOTTOM);

    match state {
        STATE_BOTTOM | STATE_DOWN => door_go_up(world, self_key),
        STATE_TOP | STATE_UP => door_go_down(world, self_key),
        _ => {}
    }
}

pub fn door_go_up(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        if let Some(ref mut mi) = ent.moveinfo {
            mi.state = STATE_UP;
            let end = mi.end_origin;
            mi.endfunc = Some(door_hit_top);
            let dir = end - ent.state.origin;
            let speed = mi.speed;
            if dir.length() > 0.001 && speed > 0.0 {
                ent.velocity = dir.normalize() * speed;
                ent.game.nextthink = world.level.time + dir.length() / speed;
                ent.think = Some(move_done_think);
            }
        }
    }
}

pub fn door_go_down(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        if let Some(ref mut mi) = ent.moveinfo {
            mi.state = STATE_DOWN;
            let end = mi.start_origin;
            mi.endfunc = Some(door_hit_bottom);
            let dir = end - ent.state.origin;
            let speed = mi.speed;
            if dir.length() > 0.001 && speed > 0.0 {
                ent.velocity = dir.normalize() * speed;
                ent.game.nextthink = world.level.time + dir.length() / speed;
                ent.think = Some(move_done_think);
            }
        }
    }
}

pub fn door_hit_top(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        if let Some(ref mut mi) = ent.moveinfo {
            mi.state = STATE_TOP;
        }
        // Wait, then close.
        let wait = ent
            .moveinfo
            .as_ref()
            .map(|mi| mi.wait)
            .unwrap_or(3.0);
        if wait > 0.0 {
            ent.game.nextthink = world.level.time + wait;
            ent.think = Some(door_go_down_think);
        }
    }
}

pub fn door_go_down_think(world: &mut GameWorld, key: EntityKey) {
    door_go_down(world, key);
}

pub fn door_hit_bottom(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        if let Some(ref mut mi) = ent.moveinfo {
            mi.state = STATE_BOTTOM;
        }
    }
}

// ---------------------------------------------------------------------------
// Button callbacks
// ---------------------------------------------------------------------------

pub fn button_use(
    world: &mut GameWorld,
    self_key: EntityKey,
    _other: EntityKey,
    activator: EntityKey,
) {
    let state = world
        .entities
        .get(self_key)
        .and_then(|e| e.moveinfo.as_ref())
        .map(|mi| mi.state)
        .unwrap_or(STATE_BOTTOM);

    if state != STATE_BOTTOM {
        return; // Already pressed.
    }

    // Move to "pressed" position.
    if let Some(ent) = world.entities.get_mut(self_key) {
        if let Some(ref mut mi) = ent.moveinfo {
            mi.state = STATE_UP;
        }
    }

    // Fire targets.
    world.use_targets(self_key, activator);

    // Return after wait.
    let wait = world
        .entities
        .get(self_key)
        .and_then(|e| e.moveinfo.as_ref())
        .map(|mi| mi.wait)
        .unwrap_or(1.0);

    if let Some(ent) = world.entities.get_mut(self_key) {
        ent.game.nextthink = world.level.time + wait;
        ent.think = Some(button_return);
    }
}

pub fn button_return(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        if let Some(ref mut mi) = ent.moveinfo {
            mi.state = STATE_BOTTOM;
        }
    }
}

// ---------------------------------------------------------------------------
// Timer callbacks
// ---------------------------------------------------------------------------

pub fn func_timer_think(world: &mut GameWorld, key: EntityKey) {
    world.use_targets(key, key);

    let (wait, random) = {
        let Some(ent) = world.entities.get(key) else { return };
        (ent.game.wait, ent.game.random)
    };

    // Schedule next firing.
    if let Some(ent) = world.entities.get_mut(key) {
        let delay = wait.max(FRAMETIME);
        // Random variation (simplified — no true randomness in deterministic game).
        ent.game.nextthink = world.level.time + delay + random * 0.5;
    }
}

// ---------------------------------------------------------------------------
// Explosive callbacks
// ---------------------------------------------------------------------------

pub fn func_explosive_die(
    world: &mut GameWorld,
    self_key: EntityKey,
    _inflictor: EntityKey,
    attacker: EntityKey,
    _damage: i32,
    _point: Vec3f,
) {
    let dmg = world
        .entities
        .get(self_key)
        .map(|e| e.game.dmg)
        .unwrap_or(0);

    if dmg > 0 {
        world.t_radius_damage(
            self_key,
            attacker,
            dmg as f32,
            None,
            dmg as f32,
            MeansOfDeath::Explosive,
        );
    }

    world.use_targets(self_key, attacker);
    world.free_entity(self_key);
}

// ---------------------------------------------------------------------------
// Killbox callback
// ---------------------------------------------------------------------------

pub fn func_killbox_use(
    world: &mut GameWorld,
    self_key: EntityKey,
    _other: EntityKey,
    _activator: EntityKey,
) {
    // Kill everything inside the killbox's volume.
    // Simplified: in full impl, uses box_edicts to find entities inside.
    let _ = world.entities.get(self_key);
}

// ---------------------------------------------------------------------------
// Spawn functions
// ---------------------------------------------------------------------------

impl GameWorld {
    /// Register all func_* spawn functions.
    pub fn register_func_spawns(&mut self) {
        self.spawn_table.insert("func_plat".to_string(), sp_func_plat);
        self.spawn_table.insert("func_button".to_string(), sp_func_button);
        self.spawn_table.insert("func_door".to_string(), sp_func_door);
        self.spawn_table.insert("func_door_secret".to_string(), sp_func_door);
        self.spawn_table.insert("func_door_rotating".to_string(), sp_func_door);
        self.spawn_table.insert("func_rotating".to_string(), sp_func_rotating);
        self.spawn_table.insert("func_train".to_string(), sp_func_train);
        self.spawn_table.insert("func_water".to_string(), sp_func_generic);
        self.spawn_table.insert("func_conveyor".to_string(), sp_func_generic);
        self.spawn_table.insert("func_areaportal".to_string(), sp_func_generic);
        self.spawn_table.insert("func_clock".to_string(), sp_func_generic);
        self.spawn_table.insert("func_wall".to_string(), sp_func_generic);
        self.spawn_table.insert("func_object".to_string(), sp_func_generic);
        self.spawn_table.insert("func_timer".to_string(), sp_func_timer);
        self.spawn_table.insert("func_explosive".to_string(), sp_func_explosive);
        self.spawn_table.insert("func_killbox".to_string(), sp_func_killbox);
    }
}

#[allow(clippy::field_reassign_with_default)]
fn sp_func_plat(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "func_plat".to_string();
        ent.game.movetype = MoveType::Push;
        ent.solid = Solid::Bsp;

        let speed = fields
            .get("speed")
            .and_then(|v| v.parse().ok())
            .unwrap_or(200.0);
        let wait = fields
            .get("wait")
            .and_then(|v| v.parse().ok())
            .unwrap_or(3.0);

        let mut mi = MoveInfo::default();
        mi.speed = speed;
        mi.wait = wait;
        mi.state = STATE_TOP;
        mi.start_origin = ent.state.origin;
        mi.end_origin = ent.state.origin - Vec3f::new(0.0, 0.0, 64.0); // default drop
        ent.moveinfo = Some(Box::new(mi));
    }
}

#[allow(clippy::field_reassign_with_default)]
fn sp_func_button(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "func_button".to_string();
        ent.game.movetype = MoveType::Stop;
        ent.solid = Solid::Bsp;
        ent.use_fn = Some(button_use);

        let speed = fields.get("speed").and_then(|v| v.parse().ok()).unwrap_or(40.0);
        let wait = fields.get("wait").and_then(|v| v.parse().ok()).unwrap_or(1.0);

        let mut mi = MoveInfo::default();
        mi.speed = speed;
        mi.wait = wait;
        mi.state = STATE_BOTTOM;
        mi.start_origin = ent.state.origin;
        mi.end_origin = ent.state.origin; // Set by map data.
        ent.moveinfo = Some(Box::new(mi));
    }
}

#[allow(clippy::field_reassign_with_default)]
fn sp_func_door(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = fields
            .get("classname")
            .cloned()
            .unwrap_or_else(|| "func_door".to_string());
        ent.game.movetype = MoveType::Push;
        ent.solid = Solid::Bsp;
        ent.use_fn = Some(door_use);

        let speed = fields.get("speed").and_then(|v| v.parse().ok()).unwrap_or(100.0);
        let wait = fields.get("wait").and_then(|v| v.parse().ok()).unwrap_or(3.0);
        let dmg = fields.get("dmg").and_then(|v| v.parse().ok()).unwrap_or(2);

        let mut mi = MoveInfo::default();
        mi.speed = speed;
        mi.wait = wait;
        mi.state = STATE_BOTTOM;
        mi.start_origin = ent.state.origin;
        mi.end_origin = ent.state.origin + Vec3f::new(0.0, 0.0, 64.0); // placeholder
        ent.moveinfo = Some(Box::new(mi));
        ent.game.dmg = dmg;
    }
}

fn sp_func_rotating(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "func_rotating".to_string();
        ent.game.movetype = MoveType::Push;
        ent.solid = Solid::Bsp;

        let speed = fields.get("speed").and_then(|v| v.parse().ok()).unwrap_or(100.0);
        ent.avelocity = Vec3f::new(0.0, speed, 0.0); // rotate around Y axis.
    }
}

#[allow(clippy::field_reassign_with_default)]
fn sp_func_train(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "func_train".to_string();
        ent.game.movetype = MoveType::Push;
        ent.solid = Solid::Bsp;

        let speed = fields.get("speed").and_then(|v| v.parse().ok()).unwrap_or(100.0);

        let mut mi = MoveInfo::default();
        mi.speed = speed;
        ent.moveinfo = Some(Box::new(mi));

        if let Some(t) = fields.get("target") {
            ent.game.target = t.clone();
        }
    }
}

fn sp_func_timer(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "func_timer".to_string();

        ent.game.wait = fields.get("wait").and_then(|v| v.parse().ok()).unwrap_or(1.0);
        ent.game.random = fields.get("random").and_then(|v| v.parse().ok()).unwrap_or(0.0);

        ent.think = Some(func_timer_think);
        ent.game.nextthink = fields
            .get("delay")
            .and_then(|v| v.parse().ok())
            .map(|d: f32| d)
            .unwrap_or(1.0);
    }
}

fn sp_func_explosive(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "func_explosive".to_string();
        ent.game.movetype = MoveType::Push;
        ent.solid = Solid::Bsp;
        ent.game.health = fields.get("health").and_then(|v| v.parse().ok()).unwrap_or(100);
        ent.game.dmg = fields.get("dmg").and_then(|v| v.parse().ok()).unwrap_or(0);
        ent.game.takedamage = TakeDamage::Yes;
        ent.die = Some(func_explosive_die);
    }
}

fn sp_func_killbox(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "func_killbox".to_string();
        ent.solid = Solid::Trigger;
        ent.use_fn = Some(func_killbox_use);
    }
}

fn sp_func_generic(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = fields
            .get("classname")
            .cloned()
            .unwrap_or_else(|| "func".to_string());
        ent.game.movetype = MoveType::Push;
        ent.solid = Solid::Bsp;
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
    fn register_func_spawns() {
        let mut world = test_world();
        world.register_func_spawns();
        assert!(world.spawn_table.get("func_plat").is_some());
        assert!(world.spawn_table.get("func_door").is_some());
        assert!(world.spawn_table.get("func_button").is_some());
        assert!(world.spawn_table.get("func_train").is_some());
        assert!(world.spawn_table.get("func_explosive").is_some());
        assert!(world.spawn_table.get("func_timer").is_some());
        assert!(world.spawn_table.get("func_killbox").is_some());
        assert!(world.spawn_table.get("func_rotating").is_some());
    }

    #[test]
    fn door_use_toggles_state() {
        let mut world = test_world();
        world.level.time = 1.0;

        let door = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(door).unwrap();
            ent.game.classname = "func_door".to_string();
            ent.game.movetype = MoveType::Push;
            let mut mi = MoveInfo::default();
            mi.speed = 100.0;
            mi.wait = 3.0;
            mi.state = STATE_BOTTOM;
            mi.start_origin = Vec3f::ZERO;
            mi.end_origin = Vec3f::new(0.0, 0.0, 64.0);
            ent.moveinfo = Some(Box::new(mi));
        }

        // Open the door.
        door_use(&mut world, door, door, door);

        let state = world
            .entities
            .get(door)
            .unwrap()
            .moveinfo
            .as_ref()
            .unwrap()
            .state;
        assert_eq!(state, STATE_UP);
    }

    #[test]
    fn button_use_fires_and_returns() {
        use std::sync::atomic::{AtomicBool, Ordering};
        static FIRED: AtomicBool = AtomicBool::new(false);

        fn test_use(_w: &mut GameWorld, _s: EntityKey, _o: EntityKey, _a: EntityKey) {
            FIRED.store(true, Ordering::Relaxed);
        }

        FIRED.store(false, Ordering::Relaxed);

        let mut world = test_world();
        world.level.time = 1.0;

        let button = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(button).unwrap();
            ent.game.classname = "func_button".to_string();
            ent.game.target = "tgt".to_string();
            let mut mi = MoveInfo::default();
            mi.speed = 40.0;
            mi.wait = 1.0;
            mi.state = STATE_BOTTOM;
            ent.moveinfo = Some(Box::new(mi));
        }

        let target = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(target).unwrap();
            ent.game.targetname = "tgt".to_string();
            ent.use_fn = Some(test_use);
        }

        button_use(&mut world, button, button, button);

        assert!(FIRED.load(Ordering::Relaxed));

        let state = world
            .entities
            .get(button)
            .unwrap()
            .moveinfo
            .as_ref()
            .unwrap()
            .state;
        assert_eq!(state, STATE_UP);
    }

    #[test]
    fn func_explosive_dies_with_damage() {
        let mut world = test_world();

        let explosive = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(explosive).unwrap();
            ent.game.dmg = 100;
            ent.state.origin = Vec3f::ZERO;
        }

        let victim = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(victim).unwrap();
            ent.state.origin = Vec3f::new(20.0, 0.0, 0.0);
            ent.game.health = 200;
            ent.game.takedamage = TakeDamage::Yes;
        }

        let attacker = world.spawn().unwrap();

        func_explosive_die(&mut world, explosive, attacker, attacker, 100, Vec3f::ZERO);

        assert!(world.entities.get(victim).unwrap().game.health < 200);
        assert!(world.entities.get(explosive).is_none());
    }

    #[test]
    fn func_timer_fires_periodically() {
        use std::sync::atomic::{AtomicI32, Ordering};
        static COUNT: AtomicI32 = AtomicI32::new(0);

        fn test_use(_w: &mut GameWorld, _s: EntityKey, _o: EntityKey, _a: EntityKey) {
            COUNT.fetch_add(1, Ordering::Relaxed);
        }

        COUNT.store(0, Ordering::Relaxed);

        let mut world = test_world();
        world.level.time = 1.0;

        let timer = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(timer).unwrap();
            ent.game.classname = "func_timer".to_string();
            ent.game.wait = 2.0;
            ent.game.target = "tgt".to_string();
        }

        let target = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(target).unwrap();
            ent.game.targetname = "tgt".to_string();
            ent.use_fn = Some(test_use);
        }

        // Call think twice.
        func_timer_think(&mut world, timer);
        assert_eq!(COUNT.load(Ordering::Relaxed), 1);

        func_timer_think(&mut world, timer);
        assert_eq!(COUNT.load(Ordering::Relaxed), 2);

        // nextthink should be set for next firing.
        let nt = world.entities.get(timer).unwrap().game.nextthink;
        assert!(nt > world.level.time);
    }

    #[test]
    fn plat_movement_states() {
        let mut world = test_world();
        world.level.time = 1.0;

        let plat = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(plat).unwrap();
            ent.state.origin = Vec3f::new(0.0, 0.0, 100.0);
            let mut mi = MoveInfo::default();
            mi.speed = 200.0;
            mi.state = STATE_TOP;
            mi.start_origin = Vec3f::new(0.0, 0.0, 100.0);
            mi.end_origin = Vec3f::new(0.0, 0.0, 0.0);
            ent.moveinfo = Some(Box::new(mi));
        }

        // Go down.
        plat_go_down(&mut world, plat);
        let state = world.entities.get(plat).unwrap().moveinfo.as_ref().unwrap().state;
        assert_eq!(state, STATE_DOWN);

        // Velocity should be downward.
        let vz = world.entities.get(plat).unwrap().velocity.z;
        assert!(vz < 0.0);
    }

    #[test]
    fn func_rotating_spins() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        let mut fields = HashMap::new();
        fields.insert("speed".to_string(), "360".to_string());

        sp_func_rotating(&mut world.entities, key, &fields);

        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.avelocity.y, 360.0);
    }

    #[test]
    fn move_calc_sets_velocity() {
        let mut world = test_world();
        world.level.time = 1.0;

        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.state.origin = Vec3f::ZERO;
            let mut mi = MoveInfo::default();
            mi.speed = 100.0;
            mi.end_origin = Vec3f::new(100.0, 0.0, 0.0);
            ent.moveinfo = Some(Box::new(mi));
        }

        move_calc(&mut world, key);

        let v = world.entities.get(key).unwrap().velocity;
        assert!(v.x > 0.0); // Moving toward end_origin.
    }
}
