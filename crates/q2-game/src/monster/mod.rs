//! Monster spawn functions for the 20 C monster types.
//!
//! Each file ports one C monster (or monster family): its frame tables,
//! callbacks and `SP_*` spawn function. `fire` and `flash` hold the shared
//! C `monster_fire_*` and `monster_flash_offset`, `movement` the C `m_move.c`
//! steps, and `test_support` the fixtures the monster tests share.

pub mod berserker;
pub mod boss2;
pub mod boss3;
pub mod brain;
pub mod chick;
pub mod fire;
pub mod flash;
pub mod flipper;
pub mod floater;
pub mod flyer;
pub mod gladiator;
pub mod gunner;
pub mod hover;
pub mod infantry;
pub mod insane;
pub mod medic;
pub mod movement;
pub mod mutant;
pub mod parasite;
pub mod soldier;
pub mod supertank;
pub mod tank;
#[cfg(test)]
pub(crate) mod test_support;
pub mod turret;

use q2_shared::types::Vec3f;

use crate::constants::{DeadFlag, ATTN_NORM, CHAN_BODY};
use crate::entity::{AnimFrameFn, EntityKey, MonsterFrame, MonsterMove, ThinkFn};
use crate::misc::{throw_gib, throw_head, GibType};
use crate::world::GameWorld;

/// One C `mframe_t` row; `const` so the move tables can be statics.
pub(crate) const fn frame(
    aifunc: AnimFrameFn,
    dist: f32,
    thinkfunc: Option<ThinkFn>,
) -> MonsterFrame {
    MonsterFrame {
        aifunc: Some(aifunc),
        dist,
        thinkfunc,
    }
}

/// C `self->monsterinfo.currentmove = &move`.
pub(crate) fn set_move(world: &mut GameWorld, key: EntityKey, mv: &MonsterMove) {
    if let Some(mi) = world
        .entities
        .get_mut(key)
        .and_then(|e| e.monsterinfo.as_mut())
    {
        mi.currentmove = Some(mv.clone());
    }
}

/// C `gi.sound(self, channel, gi.soundindex(name), 1, attenuation, 0)`;
/// the server hands back the index the spawn precached.
pub(crate) fn sound(world: &GameWorld, key: EntityKey, channel: i32, name: &str, attenuation: f32) {
    let index = world.gi.sound_index(name);
    world
        .gi
        .sound(world.number_of(key), channel, index, 1.0, attenuation, 0.0);
}

/// C cvar `g_monsterfootsteps`, which defaults to "0". `GameImport` has no
/// cvar reader, so monster footsteps stay off as in a default C game.
pub(crate) const MONSTER_FOOTSTEPS: bool = false;

/// The body of the C `<monster>_footstep`s: nothing (and no `randk()`)
/// unless `enabled`, else `steps[randk() % steps.len()]` on `CHAN_BODY`
/// at `ATTN_NORM`. Callers pass `MONSTER_FOOTSTEPS`; tests pass `true`.
pub(crate) fn footstep(
    world: &mut GameWorld,
    key: EntityKey,
    enabled: bool,
    steps: &[&str],
    volume: f32,
) {
    if !enabled {
        return;
    }
    let step = steps[(world.random_u32() % steps.len() as u32) as usize];
    let index = world.gi.sound_index(step);
    world.gi.sound(
        world.number_of(key),
        CHAN_BODY,
        index,
        volume,
        ATTN_NORM,
        0.0,
    );
}

/// The organic gib-out shared by the C `*_die` functions: two bones, four
/// meat chunks, the `head2` head, then dead. Callers play
/// `misc/udeath.wav` first, on their own channel and attenuation.
pub(crate) fn throw_organic_gibs(world: &mut GameWorld, key: EntityKey, damage: i32) {
    for _ in 0..2 {
        throw_gib(
            world,
            key,
            "models/objects/gibs/bone/tris.md2",
            damage,
            GibType::Organic,
        );
    }
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
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.deadflag = DeadFlag::Dead;
    }
}

/// The enemy's eye: C `enemy->s.origin` plus `viewheight` on z.
pub(crate) fn enemy_eye(world: &GameWorld, key: EntityKey) -> Option<Vec3f> {
    let enemy = world.entities.get(key)?.game.enemy?;
    let enemy = world.entities.get(enemy)?;
    Some(enemy.state.origin + Vec3f::new(0.0, 0.0, enemy.game.viewheight))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{test_world, test_world_recording};

    /// Each C `<monster>_footstep`, the steps its body picks from, its
    /// volume, and the sound names C loads for those steps.
    #[rustfmt::skip]
    fn c_footsteps() -> Vec<(ThinkFn, &'static [&'static str], f32, [&'static str; 4])> {
        let pair = |m: &'static str| -> [&'static str; 4] {
            match m {
                "berserk" => ["berserk/step1.wav", "berserk/step2.wav", "", ""],
                "brain" => ["brain/step1.wav", "brain/step2.wav", "", ""],
                "bitch" => ["bitch/step1.wav", "bitch/step2.wav", "", ""],
                "gladiator" => ["gladiator/step1.wav", "gladiator/step2.wav", "", ""],
                "gunner" => ["gunner/step1.wav", "gunner/step2.wav", "", ""],
                "infantry" => ["infantry/step1.wav", "infantry/step2.wav", "", ""],
                "medic" => ["medic/step1.wav", "medic/step2.wav", "", ""],
                _ => ["player/step1.wav", "player/step2.wav", "player/step3.wav", "player/step4.wav"],
            }
        };
        vec![
            (berserker::berserk_footstep, &berserker::SOUND_STEPS, 1.0, pair("berserk")),
            (brain::brain_footstep, &brain::SOUND_STEPS, 1.0, pair("brain")),
            (chick::chick_footstep, &chick::SOUND_STEPS, 1.0, pair("bitch")),
            (gladiator::gladiator_footstep, &gladiator::SOUND_STEPS, 1.0, pair("gladiator")),
            (gunner::gunner_footstep, &gunner::SOUND_STEPS, 1.0, pair("gunner")),
            (infantry::infantry_footstep, &infantry::SOUND_STEPS, 1.0, pair("infantry")),
            (insane::insane_footstep, &insane::SOUND_STEPS, 0.7, pair("player")),
            (medic::medic_footstep, &medic::SOUND_STEPS, 1.0, pair("medic")),
            (soldier::soldier_footstep, &soldier::SOUND_STEPS, 1.0, pair("player")),
        ]
    }

    #[test]
    fn monster_footsteps_are_silent_and_draw_no_randk_with_the_default_cvar() {
        let (mut world, gi) = test_world_recording();
        let key = world.spawn().unwrap();
        let fns = c_footsteps().into_iter().map(|(f, ..)| f);
        for step in fns.chain([parasite::parasite_footstep as ThinkFn]) {
            let rand = world.level.rand_state;
            step(&mut world, key);
            assert_eq!(world.level.rand_state, rand, "no randk() drawn");
        }
        assert!(gi.sound_calls.borrow().is_empty());
    }

    #[test]
    fn monster_footsteps_play_the_c_step_randk_picks_when_the_cvar_is_on() {
        for (_, steps, volume, c_names) in c_footsteps() {
            let c_names: Vec<&str> = c_names.into_iter().filter(|n| !n.is_empty()).collect();
            let (mut world, gi) = test_world_recording();
            let key = world.spawn().unwrap();
            let number = world.number_of(key);
            let mut played = Vec::new();
            for _ in 0..32 {
                let before = world.level.rand_state;
                let roll = world.random_u32();
                let after = world.level.rand_state;
                world.level.rand_state = before;
                footstep(&mut world, key, true, steps, volume);
                assert_eq!(world.level.rand_state, after, "one randk() per step");
                let call = *gi.sound_calls.borrow().last().unwrap();
                let name = gi.sounds.borrow()[call.2 as usize - 1].clone();
                let want = c_names[(roll % c_names.len() as u32) as usize];
                assert_eq!(name, want);
                assert_eq!(
                    (call.0, call.1, call.3, call.4, call.5),
                    (number, CHAN_BODY, volume, ATTN_NORM, 0.0),
                    "{want}"
                );
                played.push(name);
            }
            for name in &c_names {
                assert!(played.iter().any(|p| p == name), "{name} never picked");
            }
        }
    }

    #[test]
    fn register_monster_spawns() {
        let world = test_world();
        assert!(world.spawn_table.get("monster_soldier").is_some());
        assert!(world.spawn_table.get("monster_soldier_light").is_some());
        assert!(world.spawn_table.get("monster_infantry").is_some());
        assert!(world.spawn_table.get("monster_tank").is_some());
        assert!(world.spawn_table.get("monster_boss2").is_some());
    }

    /// Classname, health, mins, maxs, mass.
    type MonsterStats = (&'static str, i32, [f32; 3], [f32; 3], i32);

    /// Health, box and mass from each C `SP_monster_*`.
    #[test]
    fn monsters_spawn_with_the_c_health_box_and_mass() {
        #[rustfmt::skip]
        let table: &[MonsterStats] = &[
            ("monster_berserk", 240, [-16.0, -16.0, -24.0], [16.0, 16.0, 32.0], 250),
            ("monster_boss2", 2000, [-56.0, -56.0, 0.0], [56.0, 56.0, 80.0], 1000),
            ("monster_jorg", 3000, [-80.0, -80.0, 0.0], [80.0, 80.0, 140.0], 1000),
            ("monster_makron", 3000, [-30.0, -30.0, 0.0], [30.0, 30.0, 90.0], 500),
            ("monster_brain", 300, [-16.0, -16.0, -24.0], [16.0, 16.0, 32.0], 400),
            ("monster_chick", 175, [-16.0, -16.0, 0.0], [16.0, 16.0, 56.0], 200),
            ("monster_flipper", 50, [-16.0, -16.0, 0.0], [16.0, 16.0, 32.0], 100),
            ("monster_floater", 200, [-24.0, -24.0, -24.0], [24.0, 24.0, 32.0], 300),
            ("monster_flyer", 50, [-16.0, -16.0, -24.0], [16.0, 16.0, 32.0], 50),
            ("monster_gladiator", 400, [-32.0, -32.0, -24.0], [32.0, 32.0, 64.0], 400),
            ("monster_gunner", 175, [-16.0, -16.0, -24.0], [16.0, 16.0, 32.0], 200),
            ("monster_hover", 240, [-24.0, -24.0, -24.0], [24.0, 24.0, 32.0], 150),
            ("monster_infantry", 100, [-16.0, -16.0, -24.0], [16.0, 16.0, 32.0], 200),
            ("misc_insane", 100, [-16.0, -16.0, -24.0], [16.0, 16.0, 32.0], 300),
            ("monster_medic", 300, [-24.0, -24.0, -24.0], [24.0, 24.0, 32.0], 400),
            ("monster_mutant", 300, [-32.0, -32.0, -24.0], [32.0, 32.0, 48.0], 300),
            ("monster_parasite", 175, [-16.0, -16.0, -24.0], [16.0, 16.0, 24.0], 250),
            ("monster_soldier_light", 20, [-16.0, -16.0, -24.0], [16.0, 16.0, 32.0], 100),
            ("monster_soldier", 30, [-16.0, -16.0, -24.0], [16.0, 16.0, 32.0], 100),
            ("monster_soldier_ss", 40, [-16.0, -16.0, -24.0], [16.0, 16.0, 32.0], 100),
            ("monster_supertank", 1500, [-64.0, -64.0, 0.0], [64.0, 64.0, 112.0], 800),
            ("monster_tank", 750, [-32.0, -32.0, -16.0], [32.0, 32.0, 72.0], 500),
            ("monster_tank_commander", 1000, [-32.0, -32.0, -16.0], [32.0, 32.0, 72.0], 500),
            ("turret_driver", 100, [-16.0, -16.0, -24.0], [16.0, 16.0, 32.0], 200),
        ];
        let mut world = test_world();
        for &(class, health, mins, maxs, mass) in table {
            let spawn = *world
                .spawn_table
                .get(class)
                .unwrap_or_else(|| panic!("{class}"));
            let key = world.spawn().unwrap();
            spawn(&mut world.entities, key, &std::collections::HashMap::new());
            let ent = world.entities.get(key).unwrap();
            assert_eq!(ent.game.health, health, "{class}");
            assert_eq!(ent.mins, Vec3f::from(mins), "{class}");
            assert_eq!(ent.maxs, Vec3f::from(maxs), "{class}");
            assert_eq!(ent.game.mass, mass, "{class}");
        }
    }

    /// `gib_health` from each C `SP_*` (0 for the flyer, which C leaves
    /// unset, and the turret driver, which C sets to 0).
    #[test]
    fn monsters_spawn_with_the_c_gib_health() {
        let table: &[(&str, i32)] = &[
            ("monster_berserk", -60),
            ("monster_boss2", -200),
            ("monster_jorg", -2000),
            ("monster_makron", -2000),
            ("monster_brain", -150),
            ("monster_chick", -70),
            ("monster_flipper", -30),
            ("monster_floater", -80),
            ("monster_flyer", 0),
            ("monster_gladiator", -175),
            ("monster_gunner", -70),
            ("monster_hover", -100),
            ("monster_infantry", -40),
            ("misc_insane", -50),
            ("monster_medic", -130),
            ("monster_mutant", -120),
            ("monster_parasite", -50),
            ("monster_soldier_light", -30),
            ("monster_soldier", -30),
            ("monster_soldier_ss", -30),
            ("monster_supertank", -500),
            ("monster_tank", -200),
            ("monster_tank_commander", -225),
            ("turret_driver", 0),
        ];
        let mut world = test_world();
        for &(class, gib_health) in table {
            let spawn = *world.spawn_table.get(class).unwrap();
            let key = world.spawn().unwrap();
            let fields =
                std::collections::HashMap::from([("classname".to_string(), class.to_string())]);
            spawn(&mut world.entities, key, &fields);
            let ent = world.entities.get(key).unwrap();
            assert_eq!(ent.game.gib_health, gib_health, "{class}");
        }
    }

    /// Each die gibs (C `misc/udeath.wav`) by the entity's own
    /// `gib_health`, as C `self->health <= self->gib_health` does.
    #[test]
    fn monster_die_gibs_at_the_entitys_own_gib_health() {
        let classes = [
            "monster_berserk",
            "monster_makron",
            "monster_brain",
            "monster_chick",
            "monster_flipper",
            "monster_gladiator",
            "monster_gunner",
            "monster_hover",
            "monster_infantry",
            "misc_insane",
            "monster_medic",
            "monster_mutant",
            "monster_parasite",
            "monster_soldier",
            "monster_tank",
        ];
        for class in classes {
            let (mut world, gi) = crate::world::test_world_recording();
            world.entities.spawn_numbered(0).unwrap();
            let spawn = *world.spawn_table.get(class).unwrap();
            let key = world.spawn().unwrap();
            let fields =
                std::collections::HashMap::from([("classname".to_string(), class.to_string())]);
            spawn(&mut world.entities, key, &fields);
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.gib_health = 10;
            ent.game.health = 5;
            let die = ent.die.unwrap();
            die(&mut world, key, key, key, 20, Vec3f::ZERO);
            let udeath = gi.sounds.borrow().iter().any(|s| s == "misc/udeath.wav");
            assert!(udeath, "{class}");
        }
    }

    #[test]
    fn brush_turrets_and_the_jorg_rider_are_not_monsters() {
        let mut world = test_world();
        let mut breach = None;
        for class in ["turret_breach", "turret_base", "monster_boss3_stand"] {
            let spawn = *world.spawn_table.get(class).unwrap();
            let key = world.spawn().unwrap();
            spawn(&mut world.entities, key, &std::collections::HashMap::new());
            assert!(
                world.entities.get(key).unwrap().monsterinfo.is_none(),
                "{class}"
            );
            breach = breach.or(Some(key));
        }
        let ent = world.entities.get(breach.unwrap()).unwrap();
        assert_eq!(ent.solid, q2_shared::types::Solid::Bsp);
        assert_eq!(ent.game.movetype, crate::constants::MoveType::Push);
    }

    #[test]
    fn map_classnames_berserk_and_misc_insane_spawn_monsters() {
        let mut world = test_world();
        for class in ["monster_berserk", "misc_insane"] {
            let spawn = *world.spawn_table.get(class).unwrap();
            let key = world.spawn().unwrap();
            spawn(&mut world.entities, key, &std::collections::HashMap::new());
            assert!(
                world.entities.get(key).unwrap().monsterinfo.is_some(),
                "{class}"
            );
        }
        assert!(world.spawn_table.get("monster_berserker").is_none());
        assert!(world.spawn_table.get("monster_insane").is_none());
    }
}
