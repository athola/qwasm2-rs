//! The `g_misc.c` brush and display entities: func_wall, func_object,
//! func_clock and the target_string / target_character digit display it
//! drives.
//!
//! # C source reference
//! `~/Qwasm2/src/game/g_misc.c`

use std::collections::HashMap;

use q2_shared::constants::{EF_ANIM_ALL, EF_ANIM_ALLFAST, SVF_NOCLIENT};
use q2_shared::types::*;

use crate::constants::*;
use crate::entity::{EntityKey, EntityStorage};
use crate::world::GameWorld;

// ---------------------------------------------------------------------------
// func_wall
// ---------------------------------------------------------------------------

/// func_wall spawnflags (C g_misc.c).
const WALL_TRIGGER_SPAWN: i32 = 1;
const WALL_TOGGLE: i32 = 2;
const WALL_START_ON: i32 = 4;
const WALL_ANIMATED: i32 = 8;
const WALL_ANIMATED_FAST: i32 = 16;

/// Blink the wall in (telefragging what is inside it) or out; only a
/// TOGGLE wall can be used again. C ref: `func_wall_use`.
pub fn func_wall_use(world: &mut GameWorld, key: EntityKey, _: EntityKey, _: EntityKey) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    if ent.solid == Solid::Not {
        ent.solid = Solid::Bsp;
        ent.svflags &= !SVF_NOCLIENT;
        world.kill_box(key);
    } else {
        ent.solid = Solid::Not;
        ent.svflags |= SVF_NOCLIENT;
    }
    world.link_entity(key);
    if let Some(ent) = world.entities.get_mut(key) {
        if ent.game.spawnflags & WALL_TOGGLE == 0 {
            ent.use_fn = None;
        }
    }
}

/// A solid wall, or a TRIGGER_SPAWN one that appears (or, with TOGGLE,
/// switches) when used. C ref: `SP_func_wall`.
pub fn sp_func_wall(storage: &mut EntityStorage, key: EntityKey, _: &HashMap<String, String>) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    ent.game.movetype = MoveType::Push;
    let sf = ent.game.spawnflags;
    if sf & WALL_ANIMATED != 0 {
        ent.state.effects |= EF_ANIM_ALL;
    }
    if sf & WALL_ANIMATED_FAST != 0 {
        ent.state.effects |= EF_ANIM_ALLFAST;
    }

    // Just a wall.
    if sf & (WALL_TRIGGER_SPAWN | WALL_TOGGLE | WALL_START_ON) == 0 {
        ent.solid = Solid::Bsp;
        return;
    }

    // It must be TRIGGER_SPAWN, and START_ON implies TOGGLE.
    ent.game.spawnflags |= WALL_TRIGGER_SPAWN;
    if sf & WALL_START_ON != 0 && sf & WALL_TOGGLE == 0 {
        tracing::debug!("func_wall START_ON without TOGGLE");
        ent.game.spawnflags |= WALL_TOGGLE;
    }
    ent.use_fn = Some(func_wall_use);
    if sf & WALL_START_ON != 0 {
        ent.solid = Solid::Bsp;
    } else {
        ent.solid = Solid::Not;
        ent.svflags |= SVF_NOCLIENT;
    }
}

// ---------------------------------------------------------------------------
// func_object
// ---------------------------------------------------------------------------

/// Crush what the falling object lands squarely on.
/// C ref: `func_object_touch`.
pub fn func_object_touch(
    world: &mut GameWorld,
    key: EntityKey,
    other: EntityKey,
    plane: Option<&Plane>,
    _surf: Option<&Surface>,
) {
    // Only squash things we fall on top of.
    if plane.is_none_or(|p| p.normal.z < 1.0) {
        return;
    }
    if world
        .entities
        .get(other)
        .is_none_or(|o| o.game.takedamage == TakeDamage::No)
    {
        return;
    }
    let Some((origin, dmg)) = world
        .entities
        .get(key)
        .map(|e| (e.state.origin, e.game.dmg))
    else {
        return;
    };
    world.t_damage(
        other,
        key,
        key,
        Vec3f::ZERO,
        origin,
        Vec3f::ZERO,
        dmg,
        1,
        DamageFlags::empty(),
        MeansOfDeath::Crush,
    );
}

/// Let the object fall. C ref: `func_object_release`.
pub fn func_object_release(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.movetype = MoveType::Toss;
        ent.touch = Some(func_object_touch);
    }
}

/// Appear (telefragging what is inside) and fall. C ref: `func_object_use`.
pub fn func_object_use(world: &mut GameWorld, key: EntityKey, _: EntityKey, _: EntityKey) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.solid = Solid::Bsp;
    ent.svflags &= !SVF_NOCLIENT;
    ent.use_fn = None;
    world.kill_box(key);
    func_object_release(world, key);
}

/// A brush that falls once its support is gone: in place from the start,
/// or (any spawnflag) hidden until used. C ref: `SP_func_object`.
pub fn sp_func_object(storage: &mut EntityStorage, key: EntityKey, _: &HashMap<String, String>) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    ent.mins += Vec3f::splat(1.0);
    ent.maxs -= Vec3f::splat(1.0);
    if ent.game.dmg == 0 {
        ent.game.dmg = 100;
    }
    ent.game.movetype = MoveType::Push;
    if ent.game.spawnflags == 0 {
        ent.solid = Solid::Bsp;
        ent.think = Some(func_object_release);
    } else {
        ent.solid = Solid::Not;
        ent.use_fn = Some(func_object_use);
        ent.svflags |= SVF_NOCLIENT;
    }
    // TRIGGER_SPAWN is 1; ANIMATED and ANIMATED_FAST follow it.
    if ent.game.spawnflags & 2 != 0 {
        ent.state.effects |= EF_ANIM_ALL;
    }
    if ent.game.spawnflags & 4 != 0 {
        ent.state.effects |= EF_ANIM_ALLFAST;
    }
    ent.clipmask = MASK_MONSTERSOLID;
}

/// An object in place from the start is released two frames in.
/// C ref: `SP_func_object`.
pub fn finish_func_object(world: &mut GameWorld, key: EntityKey, _: &HashMap<String, String>) {
    let time = world.level.time;
    if let Some(ent) = world.entities.get_mut(key) {
        if ent.game.spawnflags == 0 {
            ent.game.nextthink = time + 2.0 * FRAMETIME;
        }
    }
}

// ---------------------------------------------------------------------------
// target_character, target_string
// ---------------------------------------------------------------------------

/// The blank frame of a target_character (0-9 are digits, 10 `-`, 11 `:`).
const CHARACTER_BLANK: i32 = 12;

/// One digit of a target_string display. C ref: `SP_target_character`.
pub fn sp_target_character(
    storage: &mut EntityStorage,
    key: EntityKey,
    _: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.movetype = MoveType::Push;
        ent.solid = Solid::Bsp;
        ent.state.frame = CHARACTER_BLANK;
    }
}

/// Show `message` on the team's characters, each at its `count`
/// (1-based) position. C ref: `target_string_use`.
pub fn target_string_use(world: &mut GameWorld, key: EntityKey, _: EntityKey, _: EntityKey) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let message = ent.game.message.clone().into_bytes();
    let mut member = ent.game.teammaster;
    while let Some(e) = member.and_then(|k| world.entities.get_mut(k)) {
        member = e.game.teamchain;
        if e.game.count == 0 {
            continue;
        }
        // Past the end (C reads the terminating NUL at `count - 1 == len`).
        let c = usize::try_from(e.game.count - 1)
            .ok()
            .and_then(|n| message.get(n));
        e.state.frame = match c {
            Some(&c @ b'0'..=b'9') => i32::from(c - b'0'),
            Some(b'-') => 10,
            Some(b':') => 11,
            _ => CHARACTER_BLANK,
        };
    }
}

/// C ref: `SP_target_string` (an absent message is already empty).
pub fn sp_target_string(storage: &mut EntityStorage, key: EntityKey, _: &HashMap<String, String>) {
    if let Some(ent) = storage.get_mut(key) {
        ent.use_fn = Some(target_string_use);
    }
}

// ---------------------------------------------------------------------------
// func_clock
// ---------------------------------------------------------------------------

/// func_clock spawnflags (C g_misc.c).
const CLOCK_TIMER_UP: i32 = 1;
const CLOCK_TIMER_DOWN: i32 = 2;
const CLOCK_START_OFF: i32 = 4;
const CLOCK_MULTI_USE: i32 = 8;
/// C `CLOCK_MESSAGE_SIZE`: the buffer holds 15 characters and a NUL.
const CLOCK_MESSAGE_SIZE: usize = 16;

/// C `Com_sprintf` into the clock buffer, then the `' '` -> `'0'` fix-ups
/// C applies at fixed byte offsets (wherever wider numbers put them).
fn clock_text(mut text: String, zero_at: &[usize]) -> String {
    text.truncate(CLOCK_MESSAGE_SIZE - 1);
    let mut bytes = text.into_bytes();
    for &i in zero_at {
        if bytes.get(i) == Some(&b' ') {
            bytes[i] = b'0';
        }
    }
    String::from_utf8(bytes).unwrap_or_default()
}

/// The countdown text for `health` seconds in `style` 0 (`xx`),
/// 1 (`xx:xx`) or 2 (`xx:xx:xx`); `None` (the message is left as it was)
/// for other styles. C ref: `func_clock_format_countdown`.
pub fn func_clock_format_countdown(style: i32, health: i32) -> Option<String> {
    match style {
        0 => Some(clock_text(format!("{health:2}"), &[])),
        1 => Some(clock_text(
            format!("{:2}:{:2}", health / 60, health % 60),
            &[3],
        )),
        2 => Some(clock_text(
            format!(
                "{:2}:{:2}:{:2}",
                health / 3600,
                (health - (health / 3600) * 3600) / 60,
                health % 60
            ),
            &[3, 6],
        )),
        _ => None,
    }
}

/// Rewind a timer clock: up from 0 to `count`, or down from `count` to 0.
/// C ref: `func_clock_reset`.
fn func_clock_reset(ent: &mut crate::entity::Entity) {
    ent.game.activator = None;
    let g = &mut ent.game;
    if g.spawnflags & CLOCK_TIMER_UP != 0 {
        g.health = 0;
        g.wait = g.count as f32;
    } else if g.spawnflags & CLOCK_TIMER_DOWN != 0 {
        g.health = g.count;
        g.wait = 0.0;
    }
}

/// Once a second: show the time on the target_string, and when a timer
/// runs out fire `pathtarget`, then stop (removed), wait to be used again
/// (MULTI_USE + START_OFF) or start over (MULTI_USE).
/// C ref: `func_clock_think`.
pub fn func_clock_think(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let display = match ent.game.enemy {
        Some(display) => display,
        None => {
            let target = ent.game.target.clone();
            let Some(display) = world.find_by_targetname(None, &target) else {
                return;
            };
            if let Some(ent) = world.entities.get_mut(key) {
                ent.game.enemy = Some(display);
            }
            display
        }
    };

    // C time + localtime; without an engine clock the text stays.
    let wall_clock = world.gi.local_time();
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    let sf = ent.game.spawnflags;
    if sf & (CLOCK_TIMER_UP | CLOCK_TIMER_DOWN) != 0 {
        if let Some(text) = func_clock_format_countdown(ent.game.style, ent.game.health) {
            ent.game.message = text;
        }
        ent.game.health += if sf & CLOCK_TIMER_UP != 0 { 1 } else { -1 };
    } else if let Some((h, m, s)) = wall_clock {
        ent.game.message = clock_text(format!("{h:2}:{m:2}:{s:2}"), &[3, 6]);
    }
    let message = ent.game.message.clone();
    let (health, wait) = (ent.game.health as f32, ent.game.wait);

    // C points the display's message at the clock's buffer.
    let display_use = world.entities.get_mut(display).and_then(|d| {
        d.game.message = message;
        d.use_fn
    });
    if let Some(display_use) = display_use {
        display_use(world, display, key, key);
    }

    let time = world.level.time;
    let ran_out = (sf & CLOCK_TIMER_UP != 0 && health > wait)
        || (sf & CLOCK_TIMER_DOWN != 0 && health < wait);
    if ran_out {
        let Some(ent) = world.entities.get_mut(key) else {
            return;
        };
        if !ent.game.pathtarget.is_empty() {
            // Fire pathtarget without the message (no centerprint).
            let pathtarget = ent.game.pathtarget.clone();
            let savetarget = std::mem::replace(&mut ent.game.target, pathtarget);
            let savemessage = std::mem::take(&mut ent.game.message);
            // C passes a NULL activator for a clock nobody used.
            let activator = ent.game.activator.unwrap_or(key);
            world.use_targets(key, activator);
            let Some(ent) = world.entities.get_mut(key) else {
                return;
            };
            ent.game.target = savetarget;
            ent.game.message = savemessage;
        }
        let Some(ent) = world.entities.get_mut(key) else {
            return;
        };
        if sf & CLOCK_MULTI_USE == 0 {
            ent.think = Some(crate::misc::free_think);
            ent.game.nextthink = time + 1.0;
            return;
        }
        func_clock_reset(ent);
        if sf & CLOCK_START_OFF != 0 {
            return;
        }
    }
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.nextthink = time + 1.0;
    }
}

/// Start a START_OFF clock (once, unless MULTI_USE).
/// C ref: `func_clock_use`.
pub fn func_clock_use(world: &mut GameWorld, key: EntityKey, _: EntityKey, activator: EntityKey) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    if ent.game.spawnflags & CLOCK_MULTI_USE == 0 {
        ent.use_fn = None;
    }
    if ent.game.activator.is_some() {
        return;
    }
    ent.game.activator = Some(activator);
    if let Some(think) = ent.think {
        think(world, key);
    }
}

/// A time-of-day clock, or a TIMER_UP / TIMER_DOWN timer of `count`
/// seconds, shown on the target_string it targets.
/// C ref: `SP_func_clock`; `finish_func_clock` starts it.
pub fn sp_func_clock(storage: &mut EntityStorage, key: EntityKey, _: &HashMap<String, String>) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    let sf = ent.game.spawnflags;
    if ent.game.target.is_empty() {
        tracing::debug!("func_clock with no target at {:?}", ent.state.origin);
        storage.soft_free(key);
        return;
    }
    if sf & CLOCK_TIMER_DOWN != 0 && ent.game.count == 0 {
        tracing::debug!("func_clock with no count at {:?}", ent.state.origin);
        storage.soft_free(key);
        return;
    }
    if sf & CLOCK_TIMER_UP != 0 && ent.game.count == 0 {
        ent.game.count = 60 * 60;
    }
    func_clock_reset(ent);
    // C replaces any message with a zeroed buffer.
    ent.game.message.clear();
    ent.think = Some(func_clock_think);
    if sf & CLOCK_START_OFF != 0 {
        ent.use_fn = Some(func_clock_use);
    }
}

/// A clock that is not START_OFF starts a second in. C ref: `SP_func_clock`.
pub fn finish_func_clock(world: &mut GameWorld, key: EntityKey, _: &HashMap<String, String>) {
    let time = world.level.time;
    if let Some(ent) = world.entities.get_mut(key) {
        if ent.game.spawnflags & CLOCK_START_OFF == 0 {
            ent.game.nextthink = time + 1.0;
        }
    }
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{ThinkFn, TouchFn, UseFn};
    use crate::world::{test_world_recording, MockGameImport};
    use std::rc::Rc;

    const BRUSH_MINS: Vec3f = Vec3f::new(0.0, 0.0, 0.0);
    const BRUSH_MAXS: Vec3f = Vec3f::new(64.0, 64.0, 32.0);

    /// A world spawned from a worldspawn plus `entities`, where `*1` is a
    /// 64x64x32 brush.
    fn spawned(entities: &str) -> (GameWorld, Rc<MockGameImport>) {
        let (mut world, gi) = test_world_recording();
        let bounds = (BRUSH_MINS, BRUSH_MAXS);
        gi.inline_bounds.borrow_mut().insert("*1".into(), bounds);
        world.init_game();
        world.spawn_entities(
            "base1",
            &format!("{{ \"classname\" \"worldspawn\" }}\n{entities}"),
            "",
        );
        (world, gi)
    }

    fn one(world: &GameWorld, classname: &str) -> EntityKey {
        world.find_by_classname(None, classname).unwrap()
    }

    fn named(world: &GameWorld, targetname: &str) -> EntityKey {
        world.find_by_targetname(None, targetname).unwrap()
    }

    fn use_is(world: &GameWorld, key: EntityKey, f: UseFn) -> bool {
        world
            .entities
            .get(key)
            .and_then(|e| e.use_fn)
            .is_some_and(|u| std::ptr::fn_addr_eq(u, f))
    }

    fn think_is(world: &GameWorld, key: EntityKey, f: ThinkFn) -> bool {
        world
            .entities
            .get(key)
            .and_then(|e| e.think)
            .is_some_and(|t| std::ptr::fn_addr_eq(t, f))
    }

    fn thinks_after(world: &GameWorld, key: EntityKey, delay: f32) -> bool {
        let nextthink = world.entities.get(key).unwrap().game.nextthink;
        (nextthink - (world.level.time + delay)).abs() < 1e-5
    }

    fn wall(flags: i32) -> (GameWorld, Rc<MockGameImport>, EntityKey) {
        let (world, gi) = spawned(&format!(
            r#"{{ "classname" "func_wall" "model" "*1" "spawnflags" "{flags}" }}"#
        ));
        let wall = one(&world, "func_wall");
        (world, gi, wall)
    }

    fn shown(world: &GameWorld, key: EntityKey) -> (Solid, bool) {
        let e = world.entities.get(key).unwrap();
        (e.solid, e.svflags & SVF_NOCLIENT == 0)
    }

    /// Make the next `trace` (KillBox's) hit a live, shootable `victim`.
    fn victim_in_the_way(world: &mut GameWorld, gi: &MockGameImport) -> EntityKey {
        let victim = world.spawn().unwrap();
        let v = world.entities.get_mut(victim).unwrap();
        v.game.health = 100;
        v.game.takedamage = TakeDamage::Yes;
        v.solid = Solid::Bbox;
        *gi.next_trace.borrow_mut() = Some(Trace {
            fraction: 0.0,
            endpos: Vec3f::ZERO,
            allsolid: true,
            startsolid: true,
            plane: Plane::default(),
            surface: None,
            contents: 0,
            ent_index: world.number_of(victim),
        });
        victim
    }

    #[test]
    fn plain_func_wall_is_a_solid_pusher_brush() {
        let (world, _, wall) = wall(0);
        let e = world.entities.get(wall).unwrap();
        assert_eq!((e.solid, e.game.movetype), (Solid::Bsp, MoveType::Push));
        assert_eq!((e.mins, e.maxs), (BRUSH_MINS, BRUSH_MAXS));
        assert!(e.use_fn.is_none());
        assert_eq!(e.state.effects, 0);
    }

    #[test]
    fn animated_func_walls_set_the_animation_effects() {
        let (world, _, w) = wall(8);
        assert_eq!(world.entities.get(w).unwrap().state.effects, EF_ANIM_ALL);
        let (world, _, w) = wall(16);
        let effects = world.entities.get(w).unwrap().state.effects;
        assert_eq!(effects, EF_ANIM_ALLFAST);
        assert_eq!(shown(&world, w), (Solid::Bsp, true));
    }

    #[test]
    fn trigger_spawn_wall_appears_once_and_telefrags_what_is_inside() {
        let (mut world, gi, w) = wall(1);
        assert_eq!(shown(&world, w), (Solid::Not, false));
        assert!(use_is(&world, w, func_wall_use));
        let victim = victim_in_the_way(&mut world, &gi);
        func_wall_use(&mut world, w, w, w);
        assert_eq!(shown(&world, w), (Solid::Bsp, true));
        assert!(world.entities.get(victim).unwrap().game.health <= 0);
        assert!(world.entities.get(w).unwrap().use_fn.is_none(), "no TOGGLE");
    }

    #[test]
    fn toggle_wall_switches_on_and_off_each_use() {
        let (mut world, _, w) = wall(3);
        func_wall_use(&mut world, w, w, w);
        assert_eq!(shown(&world, w), (Solid::Bsp, true));
        func_wall_use(&mut world, w, w, w);
        assert_eq!(shown(&world, w), (Solid::Not, false));
        assert!(use_is(&world, w, func_wall_use));
    }

    #[test]
    fn start_on_wall_is_a_trigger_spawn_toggle_that_starts_solid() {
        for flags in [4, 2, 6] {
            let (mut world, _, w) = wall(flags);
            let expect_on = flags & 4 != 0;
            let sf = world.entities.get(w).unwrap().game.spawnflags;
            assert_eq!(sf & 1, 1, "spawnflags {flags}: forced TRIGGER_SPAWN");
            if expect_on {
                assert_eq!(sf & 2, 2, "START_ON implies TOGGLE");
                assert_eq!(shown(&world, w), (Solid::Bsp, true));
                func_wall_use(&mut world, w, w, w);
                assert_eq!(shown(&world, w), (Solid::Not, false));
            } else {
                assert_eq!(shown(&world, w), (Solid::Not, false));
            }
        }
    }

    #[test]
    fn func_object_starts_solid_and_is_released_two_frames_in() {
        let (mut world, _) =
            spawned(r#"{ "classname" "func_object" "model" "*1" "spawnflags" "0" }"#);
        let obj = one(&world, "func_object");
        let e = world.entities.get(obj).unwrap();
        assert_eq!((e.solid, e.game.movetype), (Solid::Bsp, MoveType::Push));
        assert_eq!(
            (e.mins, e.maxs),
            (
                BRUSH_MINS + Vec3f::splat(1.0),
                BRUSH_MAXS - Vec3f::splat(1.0)
            )
        );
        assert_eq!((e.game.dmg, e.clipmask), (100, MASK_MONSTERSOLID));
        assert!(think_is(&world, obj, func_object_release));
        assert!(thinks_after(&world, obj, 2.0 * FRAMETIME));
        func_object_release(&mut world, obj);
        let e = world.entities.get(obj).unwrap();
        assert_eq!(e.game.movetype, MoveType::Toss);
        assert!(e
            .touch
            .is_some_and(|t| std::ptr::fn_addr_eq(t, func_object_touch as TouchFn)));
    }

    #[test]
    fn trigger_spawned_func_object_appears_and_falls_when_used() {
        let (mut world, gi) =
            spawned(r#"{ "classname" "func_object" "model" "*1" "spawnflags" "7" "dmg" "30" }"#);
        let obj = one(&world, "func_object");
        let e = world.entities.get(obj).unwrap();
        assert_eq!(shown(&world, obj), (Solid::Not, false));
        assert_eq!(e.state.effects, EF_ANIM_ALL | EF_ANIM_ALLFAST);
        assert_eq!((e.game.dmg, e.game.nextthink), (30, 0.0));
        assert!(use_is(&world, obj, func_object_use));
        let victim = victim_in_the_way(&mut world, &gi);
        func_object_use(&mut world, obj, obj, obj);
        assert_eq!(shown(&world, obj), (Solid::Bsp, true));
        let e = world.entities.get(obj).unwrap();
        assert_eq!(e.game.movetype, MoveType::Toss);
        assert!(e.use_fn.is_none() && e.touch.is_some());
        assert!(world.entities.get(victim).unwrap().game.health <= 0);
    }

    #[test]
    fn falling_func_object_crushes_only_what_it_lands_squarely_on() {
        let (mut world, _) = spawned(r#"{ "classname" "func_object" "model" "*1" "dmg" "30" }"#);
        let obj = one(&world, "func_object");
        let other = world.spawn().unwrap();
        let o = world.entities.get_mut(other).unwrap();
        o.game.health = 100;
        o.game.takedamage = TakeDamage::Yes;
        let health = |world: &GameWorld| world.entities.get(other).unwrap().game.health;
        let floor = |z: f32| Plane {
            normal: Vec3f::new(0.0, (1.0 - z * z).sqrt(), z),
            ..Plane::default()
        };
        func_object_touch(&mut world, obj, other, None, None);
        func_object_touch(&mut world, obj, other, Some(&floor(0.99)), None);
        assert_eq!(health(&world), 100);
        func_object_touch(&mut world, obj, other, Some(&floor(1.0)), None);
        assert_eq!(health(&world), 70);
        world.entities.get_mut(other).unwrap().game.takedamage = TakeDamage::No;
        func_object_touch(&mut world, obj, other, Some(&floor(1.0)), None);
        assert_eq!(health(&world), 70);
    }

    const DISPLAY: &str = r#"
        { "classname" "target_string" "targetname" "display" "team" "digits" }
        { "classname" "target_character" "team" "digits" "count" "1" }
        { "classname" "target_character" "team" "digits" "count" "2" }
        { "classname" "target_character" "team" "digits" "count" "3" }
        { "classname" "target_character" "team" "digits" "count" "4" }
        { "classname" "target_character" "team" "digits" "count" "5" }
        { "classname" "target_character" "team" "digits" "count" "6" }
        { "classname" "target_character" "team" "digits" "count" "0" }
    "#;

    /// The frames of the display's characters, in spawn order.
    fn digits(world: &GameWorld) -> Vec<i32> {
        let mut frames = Vec::new();
        let mut at = None;
        while let Some(c) = world.find_by_classname(at, "target_character") {
            frames.push(world.entities.get(c).unwrap().state.frame);
            at = Some(c);
        }
        frames
    }

    #[test]
    fn target_character_is_a_blank_solid_brush() {
        let (world, _) = spawned(r#"{ "classname" "target_character" "model" "*1" }"#);
        let e = world.entities.get(one(&world, "target_character")).unwrap();
        assert_eq!((e.solid, e.game.movetype), (Solid::Bsp, MoveType::Push));
        assert_eq!((e.state.frame, e.maxs), (12, BRUSH_MAXS));
    }

    #[test]
    fn target_string_shows_digits_dashes_and_colons_on_its_team() {
        let (mut world, _) = spawned(DISPLAY);
        let display = named(&world, "display");
        assert!(use_is(&world, display, target_string_use));
        world.entities.get_mut(display).unwrap().game.message = "1:-x5".into();
        target_string_use(&mut world, display, display, display);
        // Position 6 is past the end; count 0 is never drawn.
        assert_eq!(digits(&world), [1, 11, 10, 12, 5, 12, 12]);
    }

    #[test]
    fn countdown_text_matches_c_sprintf_and_zero_fill() {
        let text = |style, health| func_clock_format_countdown(style, health);
        assert_eq!(text(0, 5).as_deref(), Some(" 5"));
        assert_eq!(text(0, -1).as_deref(), Some("-1"));
        assert_eq!(text(1, 65).as_deref(), Some(" 1:05"));
        assert_eq!(text(1, 600).as_deref(), Some("10:00"));
        // C fixes byte 3 whatever lands there.
        assert_eq!(text(1, 6000).as_deref(), Some("100: 0"));
        assert_eq!(text(2, 3725).as_deref(), Some(" 1:02:05"));
        assert_eq!(text(3, 5), None);
        assert_eq!(
            clock_text(format!("{:2}:{:2}:{:2}", 9, 5, 7), &[3, 6]),
            " 9:05:07"
        );
        assert_eq!(clock_text("x".repeat(20), &[]).len(), 15);
    }

    #[test]
    fn func_clock_needs_a_target_and_a_timer_down_needs_a_count() {
        let (world, _) = spawned(
            r#"{ "classname" "func_clock" }
               { "classname" "func_clock" "target" "display" "spawnflags" "2" }"#,
        );
        assert!(world.find_by_classname(None, "func_clock").is_none());
    }

    #[test]
    fn func_clock_spawn_rewinds_its_timer_and_starts_a_second_in() {
        let (world, _) = spawned(
            r#"{ "classname" "func_clock" "targetname" "up" "target" "d" "spawnflags" "1" "message" "hi" }
               { "classname" "func_clock" "targetname" "down" "target" "d" "spawnflags" "6" "count" "10" }"#,
        );
        let up = named(&world, "up");
        let e = world.entities.get(up).unwrap();
        assert_eq!(
            (e.game.count, e.game.health, e.game.wait),
            (3600, 0, 3600.0)
        );
        assert_eq!(e.game.message, "");
        assert!(think_is(&world, up, func_clock_think));
        assert!(thinks_after(&world, up, 1.0));
        assert!(e.use_fn.is_none());
        let down = named(&world, "down");
        let e = world.entities.get(down).unwrap();
        assert_eq!((e.game.health, e.game.wait), (10, 0.0));
        assert_eq!(e.game.nextthink, 0.0, "START_OFF");
        assert!(use_is(&world, down, func_clock_use));
    }

    #[test]
    fn timer_down_clock_counts_on_the_display_then_fires_pathtarget_and_goes() {
        let (mut world, gi) = spawned(&format!(
            r#"{DISPLAY}
               {{ "classname" "func_clock" "target" "display" "spawnflags" "2" "count" "1" "pathtarget" "dish" }}
               {{ "classname" "misc_satellite_dish" "targetname" "dish" }}"#
        ));
        let clock = one(&world, "func_clock");
        let dish = named(&world, "dish");
        func_clock_think(&mut world, clock);
        assert_eq!(digits(&world), [12, 1, 12, 12, 12, 12, 12]);
        assert_eq!(world.entities.get(clock).unwrap().game.health, 0);
        assert!(thinks_after(&world, clock, 1.0));
        assert!(world.entities.get(dish).unwrap().think.is_none());

        func_clock_think(&mut world, clock);
        assert_eq!(digits(&world), [12, 0, 12, 12, 12, 12, 12]);
        assert!(
            world.entities.get(dish).unwrap().think.is_some(),
            "pathtarget"
        );
        assert!(
            gi.centerprints.borrow().is_empty(),
            "fired without the message"
        );
        let e = world.entities.get(clock).unwrap();
        assert_eq!(
            (e.game.target.as_str(), e.game.message.as_str()),
            ("display", " 0")
        );
        assert!(think_is(&world, clock, crate::misc::free_think));
        assert!(thinks_after(&world, clock, 1.0));
    }

    #[test]
    fn a_time_of_day_clock_shows_the_engines_local_time() {
        let (mut world, gi) = spawned(&format!(
            r#"{DISPLAY}
               {{ "classname" "func_clock" "target" "display" }}"#
        ));
        let clock = one(&world, "func_clock");
        // Without a clock the display keeps its text.
        func_clock_think(&mut world, clock);
        let before = world.entities.get(clock).unwrap().game.message.clone();
        assert!(!before.contains(':'), "{before:?}");
        // C localtime, formatted "%2i:%2i:%2i" with the tens zero-filled.
        gi.local_time.set(Some((9, 5, 7)));
        func_clock_think(&mut world, clock);
        assert_eq!(world.entities.get(clock).unwrap().game.message, " 9:05:07");
    }

    #[test]
    fn func_clock_without_its_display_stops_thinking() {
        let (mut world, _) =
            spawned(r#"{ "classname" "func_clock" "target" "nowhere" "spawnflags" "1" }"#);
        let clock = one(&world, "func_clock");
        world.entities.get_mut(clock).unwrap().game.nextthink = 0.0;
        func_clock_think(&mut world, clock);
        assert_eq!(world.entities.get(clock).unwrap().game.nextthink, 0.0);
    }

    #[test]
    fn multi_use_start_off_timer_rewinds_and_waits_to_be_used_again() {
        let (mut world, _) = spawned(&format!(
            r#"{DISPLAY}
               {{ "classname" "func_clock" "target" "display" "spawnflags" "13" "count" "1" "style" "1" }}"#
        ));
        let clock = one(&world, "func_clock");
        let player = world.spawn().unwrap();
        func_clock_use(&mut world, clock, player, player);
        assert_eq!(
            world.entities.get(clock).unwrap().game.activator,
            Some(player)
        );
        assert_eq!(digits(&world)[..5], [12, 0, 11, 0, 0], " 0:00");
        assert!(
            use_is(&world, clock, func_clock_use),
            "MULTI_USE stays usable"
        );
        world.entities.get_mut(clock).unwrap().game.nextthink = -1.0;
        func_clock_think(&mut world, clock);
        assert_eq!(digits(&world)[..5], [12, 0, 11, 0, 1], " 0:01");
        let e = world.entities.get(clock).unwrap();
        assert_eq!((e.game.health, e.game.activator), (0, None), "rewound");
        assert!(think_is(&world, clock, func_clock_think));
        assert_eq!(e.game.nextthink, -1.0, "START_OFF: not rescheduled");
        func_clock_use(&mut world, clock, player, player);
        assert_eq!(world.entities.get(clock).unwrap().game.health, 1);
    }

    #[test]
    fn using_a_running_clock_again_does_nothing_and_single_use_drops_use() {
        let (mut world, _) = spawned(&format!(
            r#"{DISPLAY}
               {{ "classname" "func_clock" "target" "display" "spawnflags" "5" "count" "5" }}"#
        ));
        let clock = one(&world, "func_clock");
        let (a, b) = (world.spawn().unwrap(), world.spawn().unwrap());
        func_clock_use(&mut world, clock, a, a);
        assert!(world.entities.get(clock).unwrap().use_fn.is_none());
        assert_eq!(world.entities.get(clock).unwrap().game.health, 1);
        func_clock_use(&mut world, clock, b, b);
        let e = world.entities.get(clock).unwrap();
        assert_eq!((e.game.health, e.game.activator), (1, Some(a)));
    }
}
