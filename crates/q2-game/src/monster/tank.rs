//! Tank and Tank Commander monsters. C ref: ~/Qwasm2/src/game/monster/tank/
use q2_shared::types::*;
use crate::ai;
use crate::constants::*;
use crate::entity::{EntityKey, EntityStorage, MonsterInfo, MonsterMove};
use crate::world::GameWorld;
use std::collections::HashMap;

const TANK_HEALTH: i32 = 750;
const TANK_COMMANDER_HEALTH: i32 = 1000;

fn stand(world: &mut GameWorld, key: EntityKey) {
    let cm = MonsterMove { firstframe: 0, lastframe: 21, frame_fn: Some(ai::ai_stand), dist: 0.0, endfunc: Some(stand) };
    if let Some(ent) = world.entities.get_mut(key) { if let Some(ref mut mi) = ent.monsterinfo { mi.currentmove = Some(cm); } }
}

fn die(world: &mut GameWorld, key: EntityKey, _inf: EntityKey, _atk: EntityKey, _dmg: i32, _pt: Vec3f) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.deadflag = DeadFlag::Dead;
        ent.svflags |= SvFlags::DEADMONSTER.bits();
        ent.solid = Solid::Not;
        ent.game.takedamage = TakeDamage::No;
    }
}

fn spawn_tank(storage: &mut EntityStorage, key: EntityKey, fields: &HashMap<String, String>, health: i32, classname: &str) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = fields.get("classname").cloned().unwrap_or_else(|| classname.to_string());
        ent.game.health = health;
        ent.game.max_health = health;
        ent.mins = Vec3f::new(-16.0, -16.0, -24.0);
        ent.maxs = Vec3f::new(16.0, 16.0, 32.0);
        ent.monsterinfo = Some(Box::new(MonsterInfo { stand: Some(stand), currentmove: Some(MonsterMove { firstframe: 0, lastframe: 21, frame_fn: Some(ai::ai_stand), dist: 0.0, endfunc: Some(stand) }), ..Default::default() }));
        ent.die = Some(die);
    }
}

pub fn sp_monster_tank(storage: &mut EntityStorage, key: EntityKey, fields: &HashMap<String, String>) {
    spawn_tank(storage, key, fields, TANK_HEALTH, "monster_tank");
}

pub fn sp_monster_tank_commander(storage: &mut EntityStorage, key: EntityKey, fields: &HashMap<String, String>) {
    spawn_tank(storage, key, fields, TANK_COMMANDER_HEALTH, "monster_tank_commander");
}
