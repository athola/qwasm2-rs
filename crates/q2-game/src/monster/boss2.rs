//! Boss2, the Hornet: a flying gunship in biggun and the inner hangar.
//! C ref: `~/Qwasm2/src/game/monster/boss2/boss2.c`
//!
//! C's `boss2_move_fidget` is not ported: nothing uses it, and its frame
//! range runs backwards (stand1 = 21 to stand30 = 0).

use q2_shared::constants::{
    CONTENTS_LAVA, CONTENTS_MONSTER, CONTENTS_SLIME, CONTENTS_SOLID, MZ2_BOSS2_MACHINEGUN_L1,
    MZ2_BOSS2_MACHINEGUN_R1, MZ2_BOSS2_ROCKET_1, MZ2_BOSS2_ROCKET_2, MZ2_BOSS2_ROCKET_3,
    MZ2_BOSS2_ROCKET_4,
};
use q2_shared::types::*;
use std::collections::HashMap;

use crate::ai;
use crate::constants::*;
use crate::entity::{
    AnimFrameFn, EntityKey, EntityStorage, MonsterFrame, MonsterInfo, MonsterMove, ThinkFn,
};
use crate::monster::fire::{monster_fire_bullet, monster_fire_rocket};
use crate::monster::flash::monster_flash_source;
use crate::monster::movement::vectoyaw;
use crate::monster::supertank::boss_explode;
use crate::world::GameWorld;

/// C boss2.h `MODEL_SCALE`.
const MODEL_SCALE: f32 = 1.0;

const fn frame(aifunc: AnimFrameFn, dist: f32, thinkfunc: Option<ThinkFn>) -> MonsterFrame {
    MonsterFrame {
        aifunc: Some(aifunc),
        dist,
        thinkfunc,
    }
}

// Moves. C ref: boss2.c `boss2_frames_*` / `boss2_move_*`.
const FRAME_STAND30: i32 = 0;
const FRAME_STAND50: i32 = 20;
const FRAME_WALK1: i32 = 50;
const FRAME_WALK20: i32 = 69;
const FRAME_ATTACK1: i32 = 70;
const FRAME_ATTACK9: i32 = 78;
const FRAME_ATTACK10: i32 = 79;
const FRAME_ATTACK15: i32 = 84;
const FRAME_ATTACK16: i32 = 85;
const FRAME_ATTACK19: i32 = 88;
const FRAME_ATTACK20: i32 = 89;
const FRAME_ATTACK40: i32 = 109;
const FRAME_PAIN2: i32 = 110;
const FRAME_PAIN19: i32 = 127;
const FRAME_PAIN20: i32 = 128;
const FRAME_PAIN23: i32 = 131;
const FRAME_DEATH2: i32 = 132;
const FRAME_DEATH50: i32 = 180;

static BOSS2_FRAMES_STAND: [MonsterFrame; 21] = [frame(ai::ai_stand, 0.0, None); 21];
static BOSS2_MOVE_STAND: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND30,
    lastframe: FRAME_STAND50,
    frames: &BOSS2_FRAMES_STAND,
    endfunc: None,
};

static BOSS2_FRAMES_WALK: [MonsterFrame; 20] = [frame(ai::ai_walk, 8.0, None); 20];
static BOSS2_MOVE_WALK: MonsterMove = MonsterMove {
    firstframe: FRAME_WALK1,
    lastframe: FRAME_WALK20,
    frames: &BOSS2_FRAMES_WALK,
    endfunc: None,
};

static BOSS2_FRAMES_RUN: [MonsterFrame; 20] = [frame(ai::ai_run, 8.0, None); 20];
static BOSS2_MOVE_RUN: MonsterMove = MonsterMove {
    firstframe: FRAME_WALK1,
    lastframe: FRAME_WALK20,
    frames: &BOSS2_FRAMES_RUN,
    endfunc: None,
};

static BOSS2_FRAMES_ATTACK_PRE_MG: [MonsterFrame; 9] = [
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 1.0, Some(boss2_attack_mg)),
];
static BOSS2_MOVE_ATTACK_PRE_MG: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTACK1,
    lastframe: FRAME_ATTACK9,
    frames: &BOSS2_FRAMES_ATTACK_PRE_MG,
    endfunc: None,
};

static BOSS2_FRAMES_ATTACK_MG: [MonsterFrame; 6] = [
    frame(ai::ai_charge, 1.0, Some(boss2_machine_gun)),
    frame(ai::ai_charge, 1.0, Some(boss2_machine_gun)),
    frame(ai::ai_charge, 1.0, Some(boss2_machine_gun)),
    frame(ai::ai_charge, 1.0, Some(boss2_machine_gun)),
    frame(ai::ai_charge, 1.0, Some(boss2_machine_gun)),
    frame(ai::ai_charge, 1.0, Some(boss2_reattack_mg)),
];
static BOSS2_MOVE_ATTACK_MG: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTACK10,
    lastframe: FRAME_ATTACK15,
    frames: &BOSS2_FRAMES_ATTACK_MG,
    endfunc: None,
};

static BOSS2_FRAMES_ATTACK_POST_MG: [MonsterFrame; 4] = [frame(ai::ai_charge, 1.0, None); 4];
static BOSS2_MOVE_ATTACK_POST_MG: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTACK16,
    lastframe: FRAME_ATTACK19,
    frames: &BOSS2_FRAMES_ATTACK_POST_MG,
    endfunc: Some(boss2_run),
};

static BOSS2_FRAMES_ATTACK_ROCKET: [MonsterFrame; 21] = [
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_move, -20.0, Some(boss2_rocket)),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 1.0, None),
];
static BOSS2_MOVE_ATTACK_ROCKET: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTACK20,
    lastframe: FRAME_ATTACK40,
    frames: &BOSS2_FRAMES_ATTACK_ROCKET,
    endfunc: Some(boss2_run),
};

static BOSS2_FRAMES_PAIN_HEAVY: [MonsterFrame; 18] = [frame(ai::ai_move, 0.0, None); 18];
static BOSS2_MOVE_PAIN_HEAVY: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN2,
    lastframe: FRAME_PAIN19,
    frames: &BOSS2_FRAMES_PAIN_HEAVY,
    endfunc: Some(boss2_run),
};

static BOSS2_FRAMES_PAIN_LIGHT: [MonsterFrame; 4] = [frame(ai::ai_move, 0.0, None); 4];
static BOSS2_MOVE_PAIN_LIGHT: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN20,
    lastframe: FRAME_PAIN23,
    frames: &BOSS2_FRAMES_PAIN_LIGHT,
    endfunc: Some(boss2_run),
};

static BOSS2_FRAMES_DEATH: [MonsterFrame; 49] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, Some(boss_explode)),
];
static BOSS2_MOVE_DEATH: MonsterMove = MonsterMove {
    firstframe: FRAME_DEATH2,
    lastframe: FRAME_DEATH50,
    frames: &BOSS2_FRAMES_DEATH,
    endfunc: Some(boss2_dead),
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn set_move(world: &mut GameWorld, key: EntityKey, mv: &MonsterMove) {
    if let Some(mi) = world
        .entities
        .get_mut(key)
        .and_then(|e| e.monsterinfo.as_mut())
    {
        mi.currentmove = Some(mv.clone());
    }
}

/// C `gi.sound(self, CHAN_VOICE, gi.soundindex(name), 1, ATTN_NONE, 0)`.
fn voice(world: &GameWorld, key: EntityKey, name: &str) {
    let index = world.gi.sound_index(name);
    world
        .gi
        .sound(world.number_of(key), CHAN_VOICE, index, 1.0, ATTN_NONE, 0.0);
}

fn enemy_of(world: &GameWorld, key: EntityKey) -> Option<EntityKey> {
    world
        .entities
        .get(key)?
        .game
        .enemy
        .filter(|&e| world.entities.get(e).is_some())
}

/// The enemy's origin `lead` seconds along its velocity, raised to its
/// eye.
fn enemy_aim(world: &GameWorld, key: EntityKey, lead: f32) -> Option<Vec3f> {
    let enemy = world.entities.get(enemy_of(world, key)?)?;
    Some(enemy.state.origin + enemy.velocity * lead + Vec3f::new(0.0, 0.0, enemy.game.viewheight))
}

// ---------------------------------------------------------------------------
// Attacks
// ---------------------------------------------------------------------------

/// All four rocket pods at the enemy's eye. C ref: `Boss2Rocket`.
pub fn boss2_rocket(world: &mut GameWorld, key: EntityKey) {
    for flash in [
        MZ2_BOSS2_ROCKET_1,
        MZ2_BOSS2_ROCKET_2,
        MZ2_BOSS2_ROCKET_3,
        MZ2_BOSS2_ROCKET_4,
    ] {
        let (Some((start, _, _)), Some(eye)) = (
            monster_flash_source(world, key, flash),
            enemy_aim(world, key, 0.0),
        ) else {
            return;
        };
        let dir = (eye - start).normalize_or_zero();
        monster_fire_rocket(world, key, start, dir, 50, 500, flash);
    }
}

/// One gun at where the enemy was a fifth of a second ago.
/// C ref: `boss2_firebullet_right` / `boss2_firebullet_left`.
fn boss2_firebullet(world: &mut GameWorld, key: EntityKey, flash: i32) {
    let (Some((start, _, _)), Some(target)) = (
        monster_flash_source(world, key, flash),
        enemy_aim(world, key, -0.2),
    ) else {
        return;
    };
    let dir = (target - start).normalize_or_zero();
    monster_fire_bullet(
        world,
        key,
        start,
        dir,
        6,
        4,
        DEFAULT_BULLET_HSPREAD,
        DEFAULT_BULLET_VSPREAD,
        flash,
    );
}

/// C ref: `Boss2MachineGun`: left gun, then right.
pub fn boss2_machine_gun(world: &mut GameWorld, key: EntityKey) {
    boss2_firebullet(world, key, MZ2_BOSS2_MACHINEGUN_L1);
    boss2_firebullet(world, key, MZ2_BOSS2_MACHINEGUN_R1);
}

// ---------------------------------------------------------------------------
// States
// ---------------------------------------------------------------------------

/// C ref: `boss2_search`.
pub fn boss2_search(world: &mut GameWorld, key: EntityKey) {
    if world.random() < 0.5 {
        voice(world, key, "bosshovr/bhvunqv1.wav");
    }
}

/// C ref: `boss2_stand`.
pub fn boss2_stand(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &BOSS2_MOVE_STAND);
}

/// C ref: `boss2_run`.
pub fn boss2_run(world: &mut GameWorld, key: EntityKey) {
    let stand_ground = world
        .entities
        .get(key)
        .and_then(|e| e.monsterinfo.as_ref())
        .is_some_and(|mi| mi.aiflags.contains(AiFlags::STAND_GROUND));
    let next = if stand_ground {
        &BOSS2_MOVE_STAND
    } else {
        &BOSS2_MOVE_RUN
    };
    set_move(world, key, next);
}

/// C ref: `boss2_walk`.
pub fn boss2_walk(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &BOSS2_MOVE_WALK);
}

/// Guns within 125 units, else guns six times in ten and rockets the
/// rest. C ref: `boss2_attack`.
pub fn boss2_attack(world: &mut GameWorld, key: EntityKey) {
    let (Some(ent), Some(enemy)) = (world.entities.get(key), enemy_of(world, key)) else {
        return;
    };
    let origin = ent.state.origin;
    let range = world
        .entities
        .get(enemy)
        .map_or(0.0, |e| (e.state.origin - origin).length());
    let next = if range <= 125.0 || world.random() <= 0.6 {
        &BOSS2_MOVE_ATTACK_PRE_MG
    } else {
        &BOSS2_MOVE_ATTACK_ROCKET
    };
    set_move(world, key, next);
}

/// C ref: `boss2_attack_mg`.
pub fn boss2_attack_mg(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &BOSS2_MOVE_ATTACK_MG);
}

/// Keep the guns going seven times in ten while the enemy is in front.
/// C ref: `boss2_reattack_mg`.
pub fn boss2_reattack_mg(world: &mut GameWorld, key: EntityKey) {
    let in_front = enemy_of(world, key).is_some_and(|e| ai::infront(world, key, e));
    let next = if in_front && world.random() <= 0.7 {
        &BOSS2_MOVE_ATTACK_MG
    } else {
        &BOSS2_MOVE_ATTACK_POST_MG
    };
    set_move(world, key, next);
}

/// C ref: `boss2_pain`.
pub fn boss2_pain(
    world: &mut GameWorld,
    key: EntityKey,
    _other: EntityKey,
    _kick: f32,
    damage: i32,
) {
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    if ent.game.health < ent.game.max_health / 2 {
        ent.state.skinnum = 1;
    }
    if time < ent.game.pain_debounce_time {
        return;
    }
    ent.game.pain_debounce_time = time + 3.0;
    let (name, next) = if damage < 10 {
        ("bosshovr/bhvpain3.wav", &BOSS2_MOVE_PAIN_LIGHT)
    } else if damage < 30 {
        ("bosshovr/bhvpain1.wav", &BOSS2_MOVE_PAIN_LIGHT)
    } else {
        ("bosshovr/bhvpain2.wav", &BOSS2_MOVE_PAIN_HEAVY)
    };
    voice(world, key, name);
    set_move(world, key, next);
}

/// The corpse box, then stop. Never reached: `boss_explode` takes over
/// the think on the death move's last frame. C ref: `boss2_dead`.
pub fn boss2_dead(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.mins = Vec3f::new(-56.0, -56.0, 0.0);
        ent.maxs = Vec3f::new(56.0, 56.0, 80.0);
    }
    ai::monster_dead(world, key);
}

/// C ref: `boss2_die`.
pub fn boss2_die(
    world: &mut GameWorld,
    key: EntityKey,
    _inflictor: EntityKey,
    _attacker: EntityKey,
    _damage: i32,
    _point: Vec3f,
) {
    voice(world, key, "bosshovr/bhvdeth1.wav");
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.deadflag = DeadFlag::Dead;
        ent.game.takedamage = TakeDamage::No;
        ent.game.count = 0;
    }
    set_move(world, key, &BOSS2_MOVE_DEATH);
}

/// `M_CheckAttack` with boss2's own odds: no skill scaling, and a
/// missile chance of 0.8 at near and mid range. C ref: `Boss2_CheckAttack`.
pub fn boss2_checkattack(world: &mut GameWorld, key: EntityKey) -> bool {
    let (Some(me), Some(enemy)) = (world.entities.get(key), enemy_of(world, key)) else {
        return false;
    };
    let Some(them) = world.entities.get(enemy) else {
        return false;
    };
    if them.game.health > 0 {
        // See if any entities are in the way of the shot.
        let spot1 = me.state.origin + Vec3f::new(0.0, 0.0, me.game.viewheight);
        let spot2 = them.state.origin + Vec3f::new(0.0, 0.0, them.game.viewheight);
        let tr = world.gi.trace(
            spot1,
            Vec3f::ZERO,
            Vec3f::ZERO,
            spot2,
            world.number_of(key),
            CONTENTS_SOLID | CONTENTS_MONSTER | CONTENTS_SLIME | CONTENTS_LAVA,
        );
        if tr.ent_index != world.number_of(enemy) {
            return false;
        }
    }

    let enemy_range = ai::range(world, key, enemy);
    let yaw = vectoyaw(them.state.origin - me.state.origin);
    let time = world.level.time;
    let flies = me.game.flags.contains(EntityFlags::FLY);
    let Some(ent) = world.entities.get_mut(key) else {
        return false;
    };
    ent.game.ideal_yaw = yaw;
    let Some(mi) = ent.monsterinfo.as_mut() else {
        return false;
    };

    if enemy_range == Range::Melee {
        mi.attack_state = if mi.melee.is_some() {
            AttackState::Melee
        } else {
            AttackState::Missile
        };
        return true;
    }

    // Missile attack.
    if mi.attack.is_none() || time < mi.attack_finished || enemy_range == Range::Far {
        return false;
    }
    let chance = if mi.aiflags.contains(AiFlags::STAND_GROUND) {
        0.4
    } else if enemy_range == Range::Near || enemy_range == Range::Mid {
        0.8
    } else {
        return false;
    };

    if world.random() < chance {
        let wait = 2.0 * world.random();
        if let Some(mi) = world
            .entities
            .get_mut(key)
            .and_then(|e| e.monsterinfo.as_mut())
        {
            mi.attack_state = AttackState::Missile;
            mi.attack_finished = time + wait;
        }
        return true;
    }

    if flies {
        let state = if world.random() < 0.3 {
            AttackState::Sliding
        } else {
            AttackState::Straight
        };
        if let Some(mi) = world
            .entities
            .get_mut(key)
            .and_then(|e| e.monsterinfo.as_mut())
        {
            mi.attack_state = state;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Spawning
// ---------------------------------------------------------------------------

/// C ref: `SP_monster_boss2`. C's gib_health of -200 is never read: the
/// Hornet always explodes.
pub fn sp_monster_boss2(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    ent.game.classname = fields
        .get("classname")
        .cloned()
        .unwrap_or_else(|| "monster_boss2".to_string());
    ent.game.movetype = MoveType::Step;
    ent.solid = Solid::Bbox;
    ent.mins = Vec3f::new(-56.0, -56.0, 0.0);
    ent.maxs = Vec3f::new(56.0, 56.0, 80.0);
    ent.game.health = 2000;
    ent.game.gib_health = -200;
    ent.game.mass = 1000;
    ent.game.flags |= EntityFlags::IMMUNE_LASER;
    ent.pain = Some(boss2_pain);
    ent.die = Some(boss2_die);
    ent.monsterinfo = Some(Box::new(MonsterInfo {
        stand: Some(boss2_stand),
        walk: Some(boss2_walk),
        run: Some(boss2_run),
        attack: Some(boss2_attack),
        search: Some(boss2_search),
        checkattack: Some(boss2_checkattack),
        currentmove: Some(BOSS2_MOVE_STAND.clone()),
        scale: MODEL_SCALE,
        ..Default::default()
    }));
    ai::flymonster_start(ent);
}

/// The sounds `SP_monster_boss2` indexes, and its engine hum.
pub fn boss2_precache(world: &mut GameWorld, key: EntityKey, _fields: &HashMap<String, String>) {
    for name in [
        "bosshovr/bhvpain1.wav",
        "bosshovr/bhvpain2.wav",
        "bosshovr/bhvpain3.wav",
        "bosshovr/bhvdeth1.wav",
        "bosshovr/bhvunqv1.wav",
    ] {
        world.gi.sound_index(name);
    }
    let engine = world.gi.sound_index("bosshovr/bhvengn1.wav");
    if let Some(ent) = world.entities.get_mut(key) {
        ent.state.sound = engine;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::misc::wire::WireImport;
    use crate::monster::test_support::*;
    use std::rc::Rc;

    fn boss2() -> (GameWorld, Rc<WireImport>, EntityKey) {
        monster_world(sp_monster_boss2)
    }

    fn mi(world: &GameWorld, key: EntityKey) -> &MonsterInfo {
        world
            .entities
            .get(key)
            .unwrap()
            .monsterinfo
            .as_ref()
            .unwrap()
    }

    #[test]
    fn boss2_moves_match_c_boss2_c() {
        let charge = Some(ai::ai_charge as AnimFrameFn);
        let mv = Some(ai::ai_move as AnimFrameFn);
        let mg: ThinkFn = boss2_machine_gun;
        let mut rocket = vec![1.0; 21];
        rocket[12] = -20.0;
        let moves: Vec<(&str, &MonsterMove, CMove)> = vec![
            (
                "stand",
                &BOSS2_MOVE_STAND,
                CMove {
                    bounds: (0, 20),
                    ai: vec![(21, Some(ai::ai_stand as AnimFrameFn))],
                    dists: zeros(21),
                    thinks: vec![],
                    end: None,
                },
            ),
            (
                "walk",
                &BOSS2_MOVE_WALK,
                CMove {
                    bounds: (50, 69),
                    ai: vec![(20, Some(ai::ai_walk as AnimFrameFn))],
                    dists: vec![8.0; 20],
                    thinks: vec![],
                    end: None,
                },
            ),
            (
                "run",
                &BOSS2_MOVE_RUN,
                CMove {
                    bounds: (50, 69),
                    ai: vec![(20, Some(ai::ai_run as AnimFrameFn))],
                    dists: vec![8.0; 20],
                    thinks: vec![],
                    end: None,
                },
            ),
            (
                "attack_pre_mg",
                &BOSS2_MOVE_ATTACK_PRE_MG,
                CMove {
                    bounds: (70, 78),
                    ai: vec![(9, charge)],
                    dists: vec![1.0; 9],
                    thinks: vec![(8, boss2_attack_mg)],
                    end: None,
                },
            ),
            (
                "attack_mg",
                &BOSS2_MOVE_ATTACK_MG,
                CMove {
                    bounds: (79, 84),
                    ai: vec![(6, charge)],
                    dists: vec![1.0; 6],
                    thinks: vec![
                        (0, mg),
                        (1, mg),
                        (2, mg),
                        (3, mg),
                        (4, mg),
                        (5, boss2_reattack_mg),
                    ],
                    end: None,
                },
            ),
            (
                "attack_post_mg",
                &BOSS2_MOVE_ATTACK_POST_MG,
                CMove {
                    bounds: (85, 88),
                    ai: vec![(4, charge)],
                    dists: vec![1.0; 4],
                    thinks: vec![],
                    end: Some(boss2_run),
                },
            ),
            (
                "attack_rocket",
                &BOSS2_MOVE_ATTACK_ROCKET,
                CMove {
                    bounds: (89, 109),
                    ai: vec![(12, charge), (1, mv), (8, charge)],
                    dists: rocket,
                    thinks: vec![(12, boss2_rocket)],
                    end: Some(boss2_run),
                },
            ),
            (
                "pain_heavy",
                &BOSS2_MOVE_PAIN_HEAVY,
                CMove {
                    bounds: (110, 127),
                    ai: vec![(18, mv)],
                    dists: zeros(18),
                    thinks: vec![],
                    end: Some(boss2_run),
                },
            ),
            (
                "pain_light",
                &BOSS2_MOVE_PAIN_LIGHT,
                CMove {
                    bounds: (128, 131),
                    ai: vec![(4, mv)],
                    dists: zeros(4),
                    thinks: vec![],
                    end: Some(boss2_run),
                },
            ),
            (
                "death",
                &BOSS2_MOVE_DEATH,
                CMove {
                    bounds: (132, 180),
                    ai: vec![(49, mv)],
                    dists: zeros(49),
                    thinks: vec![(48, boss_explode)],
                    end: Some(boss2_dead),
                },
            ),
        ];
        for (name, mv, c) in moves {
            assert_move(name, mv, c);
        }
    }

    #[test]
    fn boss2_spawns_as_a_laser_immune_flyer_with_its_own_checkattack() {
        let (world, _, key) = boss2();
        let ent = world.entities.get(key).unwrap();
        assert_eq!((ent.game.health, ent.game.mass), (2000, 1000));
        assert_eq!(ent.game.max_health, 0, "C leaves it to monster_start");
        assert_eq!(
            (ent.mins, ent.maxs),
            (Vec3f::new(-56.0, -56.0, 0.0), Vec3f::new(56.0, 56.0, 80.0))
        );
        assert!(ent
            .game
            .flags
            .contains(EntityFlags::IMMUNE_LASER | EntityFlags::FLY));
        assert!(ent
            .think
            .is_some_and(|f| std::ptr::fn_addr_eq(f, ai::flymonster_start_go as ThinkFn)));
        let mi = mi(&world, key);
        assert!(mi.checkattack.is_some_and(|f| std::ptr::fn_addr_eq(
            f,
            boss2_checkattack as crate::entity::MonsterCheckAttackFn
        )));
        assert!(mi.search.is_some() && mi.attack.is_some() && mi.walk.is_some());
        assert!(mi.sight.is_none() && mi.melee.is_none());
        assert!(playing(&world, key, &BOSS2_MOVE_STAND));
    }

    #[test]
    fn boss2_precache_indexes_its_sounds_and_hums_its_engine() {
        let (mut world, gi, key) = boss2();
        boss2_precache(&mut world, key, &HashMap::new());
        let names = gi.inner.sounds.borrow().clone();
        assert_eq!(names.len(), 6);
        let hum = world.entities.get(key).unwrap().state.sound;
        assert_eq!(names[hum as usize - 1], "bosshovr/bhvengn1.wav");
    }

    #[test]
    fn boss2_search_calls_out_half_the_time_at_no_attenuation() {
        let (mut world, gi, key) = boss2();
        seed_random(&mut world, |r| r >= 0.5);
        boss2_search(&mut world, key);
        assert!(gi.sounds.borrow().is_empty());
        seed_random(&mut world, |r| r < 0.5);
        boss2_search(&mut world, key);
        assert_eq!(
            wire_sounds(&gi),
            [(CHAN_VOICE, "bosshovr/bhvunqv1.wav".to_string(), ATTN_NONE)]
        );
    }

    #[test]
    fn boss2_walk_run_stand_and_attack_mg_pick_their_moves() {
        let (mut world, _, key) = boss2();
        boss2_walk(&mut world, key);
        assert!(playing(&world, key, &BOSS2_MOVE_WALK));
        boss2_run(&mut world, key);
        assert!(playing(&world, key, &BOSS2_MOVE_RUN));
        boss2_attack_mg(&mut world, key);
        assert!(playing(&world, key, &BOSS2_MOVE_ATTACK_MG));
        world
            .entities
            .get_mut(key)
            .unwrap()
            .monsterinfo
            .as_mut()
            .unwrap()
            .aiflags = AiFlags::STAND_GROUND;
        boss2_run(&mut world, key);
        assert!(playing(&world, key, &BOSS2_MOVE_STAND));
        boss2_walk(&mut world, key);
        boss2_stand(&mut world, key);
        assert!(playing(&world, key, &BOSS2_MOVE_STAND));
    }

    #[test]
    fn boss2_attack_guns_close_and_rockets_four_times_in_ten_beyond_125() {
        for (range, roll, mv) in [
            (125.0, 0.99, &BOSS2_MOVE_ATTACK_PRE_MG),
            (126.0, 0.6, &BOSS2_MOVE_ATTACK_PRE_MG),
            (126.0, 0.61, &BOSS2_MOVE_ATTACK_ROCKET),
        ] {
            let (mut world, _, key) = boss2();
            give_enemy(&mut world, key, Vec3f::new(range, 0.0, 0.0));
            seed_random(&mut world, |r| (r - roll).abs() < 0.003);
            boss2_attack(&mut world, key);
            assert!(playing(&world, key, mv), "{range} {roll}");
        }
    }

    #[test]
    fn boss2_reattack_mg_keeps_firing_seven_times_in_ten_while_facing_the_enemy() {
        let (mut world, _, key) = boss2();
        let enemy = give_enemy(&mut world, key, Vec3f::new(300.0, 0.0, 0.0));
        let mut again = 0;
        for _ in 0..1000 {
            boss2_reattack_mg(&mut world, key);
            again += playing(&world, key, &BOSS2_MOVE_ATTACK_MG) as usize;
        }
        assert!((650..750).contains(&again), "{again}");
        world.entities.get_mut(enemy).unwrap().state.origin = Vec3f::new(-300.0, 0.0, 0.0);
        seed_random(&mut world, |r| r < 0.1);
        boss2_reattack_mg(&mut world, key);
        assert!(playing(&world, key, &BOSS2_MOVE_ATTACK_POST_MG));
    }

    #[test]
    fn boss2_pain_picks_sound_and_move_by_damage_and_debounces() {
        for (damage, name, mv) in [
            (9, "bosshovr/bhvpain3.wav", &BOSS2_MOVE_PAIN_LIGHT),
            (10, "bosshovr/bhvpain1.wav", &BOSS2_MOVE_PAIN_LIGHT),
            (29, "bosshovr/bhvpain1.wav", &BOSS2_MOVE_PAIN_LIGHT),
            (30, "bosshovr/bhvpain2.wav", &BOSS2_MOVE_PAIN_HEAVY),
        ] {
            let (mut world, gi, key) = boss2();
            boss2_pain(&mut world, key, key, 0.0, damage);
            assert!(playing(&world, key, mv), "{damage}");
            assert_eq!(
                wire_sounds(&gi),
                [(CHAN_VOICE, name.to_string(), ATTN_NONE)]
            );
            boss2_pain(&mut world, key, key, 0.0, damage);
            assert_eq!(gi.sounds.borrow().len(), 1, "debounced");
            assert_eq!(
                world.entities.get(key).unwrap().game.pain_debounce_time,
                4.0
            );
        }
    }

    #[test]
    fn boss2_pain_sets_the_damaged_skin_below_half_health() {
        let (mut world, _, key) = boss2();
        // C monster_start fills max_health from health; the test world
        // never runs it.
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.max_health = 2000;
        ent.game.health = 1000;
        boss2_pain(&mut world, key, key, 0.0, 1);
        assert_eq!(world.entities.get(key).unwrap().state.skinnum, 0);
        world.entities.get_mut(key).unwrap().game.health = 999;
        boss2_pain(&mut world, key, key, 0.0, 1);
        assert_eq!(world.entities.get(key).unwrap().state.skinnum, 1);
    }

    #[test]
    fn boss2_rocket_fires_all_four_pods_at_the_eye() {
        let (mut world, gi, key) = boss2();
        give_enemy(&mut world, key, Vec3f::new(600.0, 0.0, 0.0));
        boss2_rocket(&mut world, key);
        assert_eq!(all_of(&world, "rocket").len(), 4);
        assert_eq!(
            muzzle_flashes(&gi),
            [
                MZ2_BOSS2_ROCKET_1,
                MZ2_BOSS2_ROCKET_2,
                MZ2_BOSS2_ROCKET_3,
                MZ2_BOSS2_ROCKET_4
            ]
        );
        let rocket = world.entities.get(all_of(&world, "rocket")[0]).unwrap();
        assert_eq!((rocket.game.dmg, rocket.game.radius_dmg), (50, 50));
        let (start, _, _) = monster_flash_source(&world, key, MZ2_BOSS2_ROCKET_1).unwrap();
        let want = (Vec3f::new(600.0, 0.0, 22.0) - start).normalize() * 500.0;
        assert!((rocket.velocity - want).length() < 0.01);
    }

    #[test]
    fn boss2_machine_gun_fires_left_then_right_behind_a_moving_enemy() {
        let (mut world, gi, key) = boss2();
        let enemy = give_enemy(&mut world, key, Vec3f::new(600.0, 0.0, 0.0));
        world.entities.get_mut(enemy).unwrap().velocity = Vec3f::new(0.0, 100.0, 0.0);
        boss2_machine_gun(&mut world, key);
        assert_eq!(
            muzzle_flashes(&gi),
            [MZ2_BOSS2_MACHINEGUN_L1, MZ2_BOSS2_MACHINEGUN_R1]
        );
        assert_eq!(
            enemy_aim(&world, key, -0.2),
            Some(Vec3f::new(600.0, -20.0, 22.0))
        );
    }

    /// A pain callback that keeps the kick it was handed in `game.count`.
    fn keep_kick(world: &mut GameWorld, key: EntityKey, _: EntityKey, kick: f32, _: i32) {
        world.entities.get_mut(key).unwrap().game.count = kick as i32;
    }

    #[test]
    fn boss2_machine_gun_bullets_hit_the_enemy_for_6_with_kick_4() {
        // Close enough that the default spread stays inside the player.
        let (mut world, _, player) = player_arena(100.0);
        let key = world.spawn().unwrap();
        sp_monster_boss2(&mut world.entities, key, &std::collections::HashMap::new());
        world.entities.get_mut(key).unwrap().game.enemy = Some(player);
        world.entities.get_mut(player).unwrap().pain = Some(keep_kick);
        boss2_machine_gun(&mut world, key);
        let ent = world.entities.get(player).unwrap();
        assert_eq!(ent.game.health, 100 - 2 * 6);
        assert_eq!(ent.game.count, 4, "kick");
    }

    #[test]
    fn boss2_die_stops_damage_and_starts_the_death_move() {
        let (mut world, gi, key) = boss2();
        world.entities.get_mut(key).unwrap().game.count = 3;
        boss2_die(&mut world, key, key, key, 100, Vec3f::ZERO);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(
            (ent.game.deadflag, ent.game.takedamage, ent.game.count),
            (DeadFlag::Dead, TakeDamage::No, 0)
        );
        assert!(playing(&world, key, &BOSS2_MOVE_DEATH));
        assert_eq!(
            wire_sounds(&gi),
            [(CHAN_VOICE, "bosshovr/bhvdeth1.wav".to_string(), ATTN_NONE)]
        );
    }

    #[test]
    fn boss2_dead_sets_the_c_corpse_box() {
        let (mut world, _, key) = boss2();
        boss2_dead(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(
            (ent.mins, ent.maxs),
            (Vec3f::new(-56.0, -56.0, 0.0), Vec3f::new(56.0, 56.0, 80.0))
        );
        assert!(ent.svflags & SvFlags::DEADMONSTER.bits() != 0);
    }

    /// A boss2 with an enemy at `x` whose clear-shot trace hits it.
    fn checker(x: f32) -> (GameWorld, Rc<WireImport>, EntityKey) {
        let (mut world, gi, key) = boss2();
        let enemy = give_enemy(&mut world, key, Vec3f::new(x, 0.0, 0.0));
        clear_shots_at(&gi, &world, enemy, Vec3f::ZERO, 1);
        (world, gi, key)
    }

    #[test]
    fn boss2_checkattack_needs_a_clear_shot_through_its_own_mask() {
        let (mut world, gi, key) = boss2();
        give_enemy(&mut world, key, Vec3f::new(300.0, 0.0, 0.0));
        assert!(!boss2_checkattack(&mut world, key));
        assert_eq!(
            gi.inner.trace_masks.borrow().as_slice(),
            [CONTENTS_SOLID | CONTENTS_MONSTER | CONTENTS_SLIME | CONTENTS_LAVA]
        );
    }

    #[test]
    fn boss2_checkattack_at_melee_range_attacks_with_its_missile_and_faces_the_enemy() {
        let (mut world, _, key) = checker(50.0);
        world.entities.get_mut(key).unwrap().game.ideal_yaw = 90.0;
        assert!(boss2_checkattack(&mut world, key));
        assert_eq!(mi(&world, key).attack_state, AttackState::Missile);
        assert_eq!(world.entities.get(key).unwrap().game.ideal_yaw, 0.0);
    }

    #[test]
    fn boss2_checkattack_fires_eight_times_in_ten_near_or_mid_whatever_the_skill() {
        for (x, skill) in [(300.0, 0), (300.0, 3), (700.0, 1)] {
            let (mut world, _, key) = checker(x);
            world.game.skill = skill;
            seed_random(&mut world, |r| (0.79..0.8).contains(&r));
            assert!(boss2_checkattack(&mut world, key), "{x} {skill}");
            let mi = mi(&world, key);
            assert_eq!(mi.attack_state, AttackState::Missile);
            assert!((1.0..=3.0).contains(&mi.attack_finished));
            let (mut world, _, key) = checker(x);
            seed_random(&mut world, |r| (0.8..0.81).contains(&r));
            assert!(!boss2_checkattack(&mut world, key));
        }
    }

    #[test]
    fn boss2_checkattack_holding_ground_fires_four_times_in_ten() {
        let (mut world, _, key) = checker(300.0);
        world
            .entities
            .get_mut(key)
            .unwrap()
            .monsterinfo
            .as_mut()
            .unwrap()
            .aiflags = AiFlags::STAND_GROUND;
        seed_random(&mut world, |r| (0.4..0.41).contains(&r));
        assert!(!boss2_checkattack(&mut world, key));
    }

    #[test]
    fn boss2_checkattack_never_fires_too_soon_or_too_far() {
        // Far: no shot even holding ground, where any nearer range has 0.4.
        let (mut world, _, key) = checker(1200.0);
        world
            .entities
            .get_mut(key)
            .unwrap()
            .monsterinfo
            .as_mut()
            .unwrap()
            .aiflags = AiFlags::STAND_GROUND;
        seed_random(&mut world, |r| r < 0.01);
        assert!(!boss2_checkattack(&mut world, key));
        let (mut world, _, key) = checker(300.0);
        world
            .entities
            .get_mut(key)
            .unwrap()
            .monsterinfo
            .as_mut()
            .unwrap()
            .attack_finished = 2.0;
        assert!(!boss2_checkattack(&mut world, key));
        let (mut world, _, key) = checker(300.0);
        world
            .entities
            .get_mut(key)
            .unwrap()
            .monsterinfo
            .as_mut()
            .unwrap()
            .attack = None;
        assert!(!boss2_checkattack(&mut world, key));
    }

    #[test]
    fn boss2_checkattack_holding_fire_slides_three_times_in_ten() {
        let mut slides = 0;
        let (mut world, gi, key) = checker(300.0);
        for _ in 0..1000 {
            let enemy = world.entities.get(key).unwrap().game.enemy.unwrap();
            clear_shots_at(&gi, &world, enemy, Vec3f::ZERO, 1);
            world
                .entities
                .get_mut(key)
                .unwrap()
                .monsterinfo
                .as_mut()
                .unwrap()
                .attack_finished = 0.0;
            if !boss2_checkattack(&mut world, key) {
                slides += (mi(&world, key).attack_state == AttackState::Sliding) as usize;
            }
        }
        // 0.2 hold fire * 0.3 slide.
        assert!((35..90).contains(&slides), "{slides}");
    }

    #[test]
    fn boss2_checkattack_skips_the_shot_check_for_a_dead_enemy() {
        let (mut world, gi, key) = boss2();
        let enemy = give_enemy(&mut world, key, Vec3f::new(50.0, 0.0, 0.0));
        world.entities.get_mut(enemy).unwrap().game.health = 0;
        assert!(boss2_checkattack(&mut world, key));
        assert!(gi.inner.trace_masks.borrow().is_empty());
    }

    #[test]
    fn boss2_sights_a_player_and_fires_at_it() {
        let (mut world, gi, key) = boss2();
        world.entities.get_mut(key).unwrap().state.origin = Vec3f::new(0.0, 0.0, 200.0);
        let player = player_at(&mut world, Vec3f::new(400.0, 0.0, 24.0));
        let at = world.entities.get(key).unwrap().state.origin;
        clear_shots_at(&gi, &world, player, at, 100_000);
        let fired = run_monster(&mut world, key, 600, |w| !all_of(w, "rocket").is_empty());
        assert!(fired, "no rocket");
        assert_eq!(world.entities.get(key).unwrap().game.enemy, Some(player));
        assert!(muzzle_flashes(&gi).contains(&MZ2_BOSS2_ROCKET_1));
    }
}
