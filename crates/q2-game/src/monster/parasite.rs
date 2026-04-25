//! Parasite monster. C ref: ~/Qwasm2/src/game/monster/parasite/
use q2_shared::types::*;
use crate::ai;
use crate::constants::*;
use crate::entity::{EntityKey, EntityStorage, MonsterInfo, MonsterMove};
use crate::world::GameWorld;
use std::collections::HashMap;

const HEALTH: i32 = 250;

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

pub fn sp_monster_parasite(storage: &mut EntityStorage, key: EntityKey, fields: &HashMap<String, String>) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = fields.get("classname").cloned().unwrap_or_else(|| "monster_parasite".to_string());
        ent.game.health = HEALTH;
        ent.game.max_health = HEALTH;
        ent.mins = Vec3f::new(-16.0, -16.0, -24.0);
        ent.maxs = Vec3f::new(16.0, 16.0, 32.0);
        ent.monsterinfo = Some(Box::new(MonsterInfo { stand: Some(stand), currentmove: Some(MonsterMove { firstframe: 0, lastframe: 21, frame_fn: Some(ai::ai_stand), dist: 0.0, endfunc: Some(stand) }), ..Default::default() }));
        ent.die = Some(die);
    }
}
