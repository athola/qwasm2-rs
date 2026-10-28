//! Combat and damage system — T_Damage, T_RadiusDamage, Killed.
//!
//! Port of `g_combat.c`: damage application, armor absorption, knockback,
//! kill tracking, radius (splash) damage, power armor and
//! `M_ReactToDamage`. Not ported: the per-frame client damage blends, the skill
//! and deathmatch rules for players, and temp-entity effects.
//!
//! # C source reference
//! `~/Qwasm2/src/game/g_combat.c`

use q2_shared::constants::MASK_SOLID;
use q2_shared::types::*;

use crate::constants::*;
use crate::entity::EntityKey;
use crate::world::GameWorld;

// ---------------------------------------------------------------------------
// CanDamage — line-of-sight check
// C ref: g_combat.c:34-119
// ---------------------------------------------------------------------------

impl GameWorld {
    /// Check if `inflictor` has line-of-sight to `target` for damage purposes.
    /// Tries 5 trace offsets: center, and 4 corners (+/-15 on X/Y).
    pub fn can_damage(&self, target: EntityKey, inflictor: EntityKey) -> bool {
        let (targ_origin, targ_absmin, targ_absmax, targ_movetype) = {
            let Some(targ) = self.entities.get(target) else {
                return false;
            };
            (
                targ.state.origin,
                targ.absmin,
                targ.absmax,
                targ.game.movetype,
            )
        };
        let pass = self.number_of(inflictor);
        let inf_origin = match self.entities.get(inflictor) {
            Some(e) => e.state.origin,
            None => return false,
        };

        // Bmodels have their origin at 0,0,0: trace to the box centre,
        // and hitting the bmodel itself counts.
        if targ_movetype == MoveType::Push {
            let dest = (targ_absmin + targ_absmax) * 0.5;
            let trace = self
                .gi
                .trace(inf_origin, Vec3f::ZERO, Vec3f::ZERO, dest, pass, MASK_SOLID);
            return trace.fraction == 1.0
                || (trace.ent_index.is_some() && trace.ent_index == self.number_of(target));
        }

        // Try 5 offsets: center, then 4 corners.
        let offsets = [
            Vec3f::ZERO,
            Vec3f::new(15.0, 15.0, 0.0),
            Vec3f::new(15.0, -15.0, 0.0),
            Vec3f::new(-15.0, 15.0, 0.0),
            Vec3f::new(-15.0, -15.0, 0.0),
        ];

        for offset in &offsets {
            let dest = targ_origin + *offset;
            let trace = self
                .gi
                .trace(inf_origin, Vec3f::ZERO, Vec3f::ZERO, dest, pass, MASK_SOLID);
            if trace.fraction == 1.0 {
                return true;
            }
        }

        false
    }

    // -----------------------------------------------------------------------
    // Knockback
    // C ref: g_combat.c:504-520
    // -----------------------------------------------------------------------

    /// Apply knockback to an entity — adds velocity in `dir` direction.
    ///
    /// `scale`: 1600.0 for self-damage, 500.0 for other damage.
    /// Minimum effective mass is 50.
    fn apply_knockback(&mut self, target: EntityKey, dir: Vec3f, knockback: f32, scale: f32) {
        if knockback == 0.0 {
            return;
        }

        let Some(ent) = self.entities.get_mut(target) else {
            return;
        };

        let mass = (ent.game.mass as f32).max(50.0);
        let dir_norm = if dir.length_squared() > 0.0 {
            dir.normalize()
        } else {
            Vec3f::ZERO
        };
        let kvel = dir_norm * (scale * knockback / mass);
        ent.velocity += kvel;
    }

    // -----------------------------------------------------------------------
    // CheckArmor — armor absorption
    // C ref: g_combat.c:320-383
    // -----------------------------------------------------------------------

    /// Calculate how much damage the worn armor absorbs, and use it up.
    /// Returns the amount of damage saved by armor.
    fn check_armor(&mut self, target: EntityKey, damage: i32, dflags: DamageFlags) -> i32 {
        if damage == 0 || dflags.contains(DamageFlags::NO_ARMOR) {
            return 0;
        }
        let Some(index) = self.armor_index(target) else {
            return 0; // no armor, or not a client
        };
        let Some(info) = self.items.get(index).and_then(|i| i.armor_info) else {
            return 0;
        };
        let protection = if dflags.contains(DamageFlags::ENERGY) {
            info.energy_protection
        } else {
            info.normal_protection
        };
        let Some(client) = self.client_mut(target) else {
            return 0;
        };
        let save = ((protection * damage as f32).ceil() as i32).min(client.pers.inventory[index]);
        client.pers.inventory[index] -= save;
        save
    }

    /// The power armor a player has switched on: the shield if carried,
    /// else the screen. C ref: `PowerArmorType` (g_items.c).
    fn power_armor_type(&self, key: EntityKey) -> i32 {
        let switched_on = self
            .entities
            .get(key)
            .is_some_and(|e| e.game.flags.contains(EntityFlags::POWER_ARMOR));
        let Some(client) = self.client(key).filter(|_| switched_on) else {
            return POWER_ARMOR_NONE;
        };
        let carries = |name| {
            self.find_item_by_name(name)
                .is_some_and(|index| client.pers.inventory[index] > 0)
        };
        if carries("Power Shield") {
            POWER_ARMOR_SHIELD
        } else if carries("Power Screen") {
            POWER_ARMOR_SCREEN
        } else {
            POWER_ARMOR_NONE
        }
    }

    /// How much of `damage` power armor absorbs, using up cells (a
    /// player's) or `power_armor_power` (a monster's). The screen covers
    /// only hits in front and saves a third at a point per cell; the shield
    /// saves two thirds at two points per cell. The C `SpawnDamage` sparks
    /// are not sent (temp entities are not ported here). C ref:
    /// `CheckPowerArmor` (g_combat.c).
    pub(crate) fn check_power_armor(
        &mut self,
        target: EntityKey,
        point: Vec3f,
        damage: i32,
        dflags: DamageFlags,
    ) -> i32 {
        if damage == 0 || dflags.contains(DamageFlags::NO_ARMOR) {
            return 0;
        }
        let Some(ent) = self.entities.get(target) else {
            return 0;
        };
        let (origin, angles) = (ent.state.origin, ent.state.angles);
        let cells = self.find_item_by_name("Cells");
        let (kind, power) = if let Some(client) = self.client(target) {
            let kind = self.power_armor_type(target);
            let power = match (kind, cells) {
                (POWER_ARMOR_NONE, _) | (_, None) => 0,
                (_, Some(index)) => client.pers.inventory[index],
            };
            (kind, power)
        } else if ent.svflags & SvFlags::MONSTER.bits() != 0 {
            let mi = ent.monsterinfo.as_ref();
            (
                mi.map_or(POWER_ARMOR_NONE, |mi| mi.power_armor_type),
                mi.map_or(0, |mi| mi.power_armor_power),
            )
        } else {
            return 0;
        };
        if kind == POWER_ARMOR_NONE || power == 0 {
            return 0;
        }

        let (damage_per_cell, damage) = if kind == POWER_ARMOR_SCREEN {
            let (forward, _, _) = crate::physics::angle_vectors(angles);
            let dir = (point - origin).normalize_or_zero();
            if dir.dot(forward) <= 0.3 {
                return 0;
            }
            (1, damage / 3)
        } else {
            (2, (2 * damage) / 3)
        };
        let save = (power * damage_per_cell).min(damage);
        if save == 0 {
            return 0;
        }

        let time = self.level.time;
        let power_used = save / damage_per_cell;
        let Some(ent) = self.entities.get_mut(target) else {
            return 0;
        };
        ent.game.powerarmor_time = time + 0.2;
        if ent.client.is_none() {
            if let Some(mi) = ent.monsterinfo.as_mut() {
                mi.power_armor_power -= power_used;
            }
        } else if let (Some(client), Some(index)) = (self.client_mut(target), cells) {
            client.pers.inventory[index] -= power_used;
        }
        save
    }

    // -----------------------------------------------------------------------
    // Killed
    // C ref: g_combat.c:121-167
    // -----------------------------------------------------------------------

    /// Handle entity death — update kill stats, call die callback.
    pub fn killed(
        &mut self,
        target: EntityKey,
        inflictor: EntityKey,
        attacker: EntityKey,
        damage: i32,
        point: Vec3f,
    ) {
        let Some(ent) = self.entities.get_mut(target) else {
            return;
        };
        ent.game.enemy = Some(attacker);

        let dying_monster =
            ent.svflags & SvFlags::MONSTER.bits() != 0 && ent.game.deadflag != DeadFlag::Dead;
        let good_guy = ent
            .monsterinfo
            .as_ref()
            .is_some_and(|mi| mi.aiflags.contains(AiFlags::GOOD_GUY));
        if dying_monster && !good_guy {
            self.level.killed_monsters += 1;
        }

        let Some(ent) = self.entities.get_mut(target) else {
            return;
        };
        let pusher = matches!(
            ent.game.movetype,
            MoveType::Push | MoveType::Stop | MoveType::None
        );
        if dying_monster && !pusher {
            ent.touch = None;
            self.monster_death_use(target);
        }

        // Call die callback.
        let die_fn = self.entities.get(target).and_then(|e| e.die);
        if let Some(die) = die_fn {
            die(self, target, inflictor, attacker, damage, point);
        }
    }

    /// A dying monster drops what it carried and fires its targets (its
    /// `deathtarget`, when set) at its killer. C ref: `monster_death_use`
    /// (g_monster.c).
    fn monster_death_use(&mut self, key: EntityKey) {
        let Some(ent) = self.entities.get_mut(key) else {
            return;
        };
        ent.game.flags.remove(EntityFlags::FLY | EntityFlags::SWIM);
        if let Some(mi) = ent.monsterinfo.as_mut() {
            mi.aiflags &= AiFlags::GOOD_GUY;
        }
        if let Some(item) = ent.game.item.take() {
            self.drop_item(key, item);
        }
        let Some(ent) = self.entities.get_mut(key) else {
            return;
        };
        if !ent.game.deathtarget.is_empty() {
            ent.game.target = ent.game.deathtarget.clone();
        }
        if ent.game.target.is_empty() {
            return;
        }
        if let Some(enemy) = ent.game.enemy {
            self.use_targets(key, enemy);
        }
    }

    // -----------------------------------------------------------------------
    // T_Damage — core damage function
    // C ref: g_combat.c:522-713
    // -----------------------------------------------------------------------

    /// Apply damage to an entity. This is THE core damage function.
    ///
    /// Sequence: check god mode → armor absorption → knockback → reduce health
    /// → if health <= 0: Killed() → else: pain callback.
    ///
    /// # Parameters
    /// - `target`: entity taking damage
    /// - `inflictor`: entity that caused the damage (rocket entity, etc.)
    /// - `attacker`: entity responsible (player who fired the rocket)
    /// - `dir`: damage direction (for knockback)
    /// - `point`: impact point
    /// - `normal`: surface normal at impact
    /// - `damage`: base damage amount
    /// - `knockback`: knockback force
    /// - `dflags`: damage modifier flags
    /// - `means_of_death`: what killed the entity (for obituary)
    #[allow(clippy::too_many_arguments)]
    pub fn t_damage(
        &mut self,
        target: EntityKey,
        inflictor: EntityKey,
        attacker: EntityKey,
        dir: Vec3f,
        point: Vec3f,
        _normal: Vec3f,
        damage: i32,
        knockback: i32,
        dflags: DamageFlags,
        means_of_death: MeansOfDeath,
    ) {
        // Check if target can take damage.
        let takes_damage = self
            .entities
            .get(target)
            .map(|e| e.game.takedamage != TakeDamage::No)
            .unwrap_or(false);
        if !takes_damage {
            return;
        }
        // C sets it after friendly fire avoidance, which can only add
        // MOD_FRIENDLY_FIRE (teams are not ported).
        self.means_of_death = means_of_death;

        let mut damage = damage;
        let mut knockback = knockback as f32;

        // Surprise bonus: 2x damage if monster hasn't seen the attacker.
        let is_monster = self
            .entities
            .get(target)
            .map(|e| e.svflags & SvFlags::MONSTER.bits() != 0)
            .unwrap_or(false);
        let has_enemy = self
            .entities
            .get(target)
            .and_then(|e| e.game.enemy)
            .is_some();
        let attacker_is_client = self
            .entities
            .get(attacker)
            .map(|e| e.client.is_some())
            .unwrap_or(false);

        if !dflags.contains(DamageFlags::RADIUS) && is_monster && attacker_is_client && !has_enemy {
            let health = self
                .entities
                .get(target)
                .map(|e| e.game.health)
                .unwrap_or(0);
            if health > 0 {
                damage *= 2;
            }
        }

        // Check FL_NO_KNOCKBACK flag.
        let no_knockback = self
            .entities
            .get(target)
            .map(|e| e.game.flags.contains(EntityFlags::NO_KNOCKBACK))
            .unwrap_or(false);
        if no_knockback {
            knockback = 0.0;
        }

        // Apply knockback (unless suppressed by damage flags or movetype).
        if !dflags.contains(DamageFlags::NO_KNOCKBACK) && knockback > 0.0 {
            let movetype = self
                .entities
                .get(target)
                .map(|e| e.game.movetype)
                .unwrap_or_default();

            if movetype != MoveType::None
                && movetype != MoveType::Bounce
                && movetype != MoveType::Push
                && movetype != MoveType::Stop
            {
                let is_self_damage = target == attacker;
                let scale = if is_self_damage { 1600.0 } else { 500.0 };
                self.apply_knockback(target, dir, knockback, scale);
            }
        }

        let mut take = damage;

        // Check god mode.
        let godmode = self
            .entities
            .get(target)
            .map(|e| e.game.flags.contains(EntityFlags::GODMODE))
            .unwrap_or(false);
        if godmode && !dflags.contains(DamageFlags::NO_PROTECTION) {
            take = 0;
        }

        take -= self.check_power_armor(target, point, take, dflags);
        // Apply armor (if not god mode).
        if take > 0 {
            take -= self.check_armor(target, take, dflags);
        }

        let is_client = self
            .entities
            .get(target)
            .is_some_and(|e| e.client.is_some());

        // Apply damage to health; only damage that is taken can kill.
        if take > 0 {
            let Some(ent) = self.entities.get_mut(target) else {
                return;
            };
            ent.game.health -= take;
            if ent.game.health <= 0 {
                // Disable knockback on dead entities.
                if is_monster || is_client {
                    ent.game.flags |= EntityFlags::NO_KNOCKBACK;
                }
                self.killed(target, inflictor, attacker, take, point);
                return;
            }
        }

        // Entity survived — a monster gets angry, then pain. C: not for a
        // ducked monster or a client in god mode.
        if is_monster {
            self.m_react_to_damage(target, attacker);
        }
        let Some(ent) = self.entities.get(target) else {
            return;
        };
        let feels_pain = if is_monster {
            !ent.monsterinfo
                .as_ref()
                .is_some_and(|mi| mi.aiflags.contains(AiFlags::DUCKED))
        } else if is_client {
            !godmode
        } else {
            true
        };
        if !(feels_pain && take > 0) {
            return;
        }
        if let Some(pain) = ent.pain {
            pain(self, target, attacker, knockback, take);
        }
        // Nightmare mode monsters don't go into pain frames often.
        if is_monster && self.game.skill == SKILL_HARDPLUS {
            let time = self.level.time;
            if let Some(ent) = self.entities.get_mut(target) {
                ent.game.pain_debounce_time = time + 5.0;
            }
        }
    }

    /// A hurt monster turns on the player who hurt it, on a monster of
    /// another kind that moves like it (not a tank, they spray too much),
    /// on one that meant to hit it, or else on its attacker's enemy; a
    /// good guy forgives players and other good guys.
    /// C ref: `M_ReactToDamage` (g_combat.c).
    pub fn m_react_to_damage(&mut self, targ: EntityKey, attacker: EntityKey) {
        let (Some(t), Some(a)) = (self.entities.get(targ), self.entities.get(attacker)) else {
            return;
        };
        if t.game.health <= 0 {
            return;
        }
        let attacker_is_client = a.client.is_some();
        if !attacker_is_client && a.svflags & SvFlags::MONSTER.bits() == 0 {
            return;
        }
        if attacker == targ || Some(attacker) == t.game.enemy {
            return;
        }
        let good_guy = |e: &crate::entity::Entity| {
            e.monsterinfo
                .as_ref()
                .is_some_and(|mi| mi.aiflags.contains(AiFlags::GOOD_GUY))
        };
        // A good guy monster does not get mad at players or good guys.
        if good_guy(t) && (attacker_is_client || good_guy(a)) {
            return;
        }
        let client_enemy = t
            .game
            .enemy
            .filter(|&e| self.entities.get(e).is_some_and(|e| e.client.is_some()));

        // If attacker is a client, get mad at them because he's good and
        // we're not.
        if attacker_is_client {
            if let Some(mi) = self
                .entities
                .get_mut(targ)
                .and_then(|e| e.monsterinfo.as_mut())
            {
                mi.aiflags.remove(AiFlags::SOUND_TARGET);
            }
            // Only in coop (both enemies are clients): switch only if the
            // current enemy is out of sight.
            if let Some(current) = client_enemy {
                let keep = crate::ai::visible(self, targ, current);
                if let Some(ent) = self.entities.get_mut(targ) {
                    ent.game.oldenemy = Some(if keep { attacker } else { current });
                }
                if keep {
                    return;
                }
            }
            self.get_mad_at(targ, attacker, None);
            return;
        }

        let fly_swim = EntityFlags::FLY | EntityFlags::SWIM;
        let same_base = t.game.flags & fly_swim == a.game.flags & fly_swim;
        let sprays = matches!(
            a.game.classname.as_str(),
            "monster_tank" | "monster_supertank" | "monster_makron" | "monster_jorg"
        );
        let new_enemy = if same_base && t.game.classname != a.game.classname && !sprays {
            Some(attacker)
        } else if a.game.enemy == Some(targ) {
            // If they *meant* to shoot us, then shoot back.
            Some(attacker)
        } else {
            // Otherwise get mad at whoever they are mad at (help our
            // buddy).
            a.game.enemy
        };
        if let Some(new_enemy) = new_enemy {
            self.get_mad_at(targ, new_enemy, client_enemy);
        }
    }

    /// Take `enemy` as the enemy, keeping a player it replaces as
    /// `oldenemy`, and go after it unless ducked. The shared tail of the
    /// C `M_ReactToDamage` branches.
    fn get_mad_at(&mut self, targ: EntityKey, enemy: EntityKey, old: Option<EntityKey>) {
        let Some(ent) = self.entities.get_mut(targ) else {
            return;
        };
        if old.is_some() {
            ent.game.oldenemy = old;
        }
        ent.game.enemy = Some(enemy);
        let ducked = ent
            .monsterinfo
            .as_ref()
            .is_some_and(|mi| mi.aiflags.contains(AiFlags::DUCKED));
        if !ducked {
            crate::ai::found_target(self, targ);
        }
    }

    // -----------------------------------------------------------------------
    // T_RadiusDamage — splash/area damage
    // C ref: g_combat.c:715-762
    // -----------------------------------------------------------------------

    /// Apply damage to all entities within `radius` of `inflictor`.
    /// Damage scales linearly with distance: `points = damage - 0.5 * dist`.
    /// Self-damage (attacker == target) is halved.
    pub fn t_radius_damage(
        &mut self,
        inflictor: EntityKey,
        attacker: EntityKey,
        damage: f32,
        ignore: Option<EntityKey>,
        radius: f32,
        means_of_death: MeansOfDeath,
    ) {
        let inf_origin = match self.entities.get(inflictor) {
            Some(e) => e.state.origin,
            None => return,
        };

        // Collect all entity keys to avoid borrow issues during iteration.
        let all_keys: Vec<EntityKey> = self.entities.iter().map(|(k, _)| k).collect();

        for key in all_keys {
            if Some(key) == ignore {
                continue;
            }

            let (ent_origin, ent_mins, ent_maxs) = {
                let Some(ent) = self.entities.get(key) else {
                    continue;
                };
                // C findradius skips free and SOLID_NOT edicts.
                if !ent.in_use || ent.solid == Solid::Not || ent.game.takedamage == TakeDamage::No {
                    continue;
                }
                (ent.state.origin, ent.mins, ent.maxs)
            };

            // Calculate distance to entity's bounding box center.
            let center = ent_origin + (ent_mins + ent_maxs) * 0.5;
            let diff = inf_origin - center;
            let dist = diff.length();

            // Check if within radius.
            if dist > radius {
                continue;
            }

            // Linear falloff: damage - 0.5 * distance.
            let mut points = damage - 0.5 * dist;

            // Self-damage halved.
            if key == attacker {
                points *= 0.5;
            }

            if points <= 0.0 {
                continue;
            }

            // Line-of-sight check.
            if !self.can_damage(key, inflictor) {
                continue;
            }

            let dir = ent_origin - inf_origin;
            self.t_damage(
                key,
                inflictor,
                attacker,
                dir,
                inf_origin,
                Vec3f::ZERO,
                points as i32,
                points as i32,
                DamageFlags::RADIUS,
                means_of_death,
            );
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

    // -- can_damage tests --

    #[test]
    fn can_damage_with_a_clear_line_of_sight() {
        // MockGameImport traces return fraction=1.0 (no collision).
        let mut world = test_world();
        let target = world.spawn().unwrap();
        let inflictor = world.spawn().unwrap();

        assert!(world.can_damage(target, inflictor));
    }

    #[test]
    fn can_damage_returns_false_for_missing_entity() {
        let mut world = test_world();
        let inflictor = world.spawn().unwrap();
        let fake_key = {
            let k = world.spawn().unwrap();
            world.entities.free(k); // gone entirely, not just freed
            k
        };

        assert!(!world.can_damage(fake_key, inflictor));
    }

    // -- apply_knockback tests --

    #[test]
    fn knockback_applies_velocity() {
        let mut world = test_world();
        let target = world.spawn().unwrap();
        world.entities.get_mut(target).unwrap().game.mass = 100;
        world.entities.get_mut(target).unwrap().velocity = Vec3f::ZERO;

        world.apply_knockback(target, Vec3f::new(1.0, 0.0, 0.0), 100.0, 500.0);

        let v = world.entities.get(target).unwrap().velocity;
        // v = dir_norm * (500 * 100 / 100) = (500, 0, 0)
        assert!((v.x - 500.0).abs() < 0.01);
    }

    #[test]
    fn knockback_minimum_mass() {
        let mut world = test_world();
        let target = world.spawn().unwrap();
        world.entities.get_mut(target).unwrap().game.mass = 10; // below 50 minimum
        world.entities.get_mut(target).unwrap().velocity = Vec3f::ZERO;

        world.apply_knockback(target, Vec3f::new(1.0, 0.0, 0.0), 100.0, 500.0);

        let v = world.entities.get(target).unwrap().velocity;
        // mass clamped to 50: v = 500 * 100 / 50 = 1000
        assert!((v.x - 1000.0).abs() < 0.01);
    }

    #[test]
    fn knockback_zero_does_nothing() {
        let mut world = test_world();
        let target = world.spawn().unwrap();
        world.entities.get_mut(target).unwrap().velocity = Vec3f::ZERO;

        world.apply_knockback(target, Vec3f::new(1.0, 0.0, 0.0), 0.0, 500.0);

        let v = world.entities.get(target).unwrap().velocity;
        assert_eq!(v, Vec3f::ZERO);
    }

    #[test]
    fn knockback_self_damage_uses_higher_scale() {
        let mut world = test_world();
        let target = world.spawn().unwrap();
        world.entities.get_mut(target).unwrap().game.mass = 100;
        world.entities.get_mut(target).unwrap().velocity = Vec3f::ZERO;

        // Self-damage scale is 1600.
        world.apply_knockback(target, Vec3f::new(0.0, 0.0, 1.0), 50.0, 1600.0);

        let v = world.entities.get(target).unwrap().velocity;
        // v.z = 1600 * 50 / 100 = 800
        assert!((v.z - 800.0).abs() < 0.01);
    }

    // -- check_armor tests --

    #[test]
    fn check_armor_absorbs_damage() {
        let mut world = test_world();
        world.init_items(); // inventory[1] is Body Armor
                            // Give the entity client data with 100 armor.
        let mut client = crate::entity::ClientData::default();
        client.pers.inventory[1] = 100;
        let target = world.spawn_test_client(client);

        let saved = world.check_armor(target, 50, DamageFlags::empty());
        // Body armor absorbs 80%: ceil(0.8 * 50) = 40
        assert_eq!(saved, 40);

        // Armor should be reduced.
        let armor = world.client(target).unwrap().pers.inventory[1];
        assert_eq!(armor, 60); // 100 - 40
    }

    #[test]
    fn check_armor_capped_by_remaining() {
        let mut world = test_world();
        world.init_items(); // inventory[1] is Body Armor
        let mut client = crate::entity::ClientData::default();
        client.pers.inventory[1] = 10; // only 10 armor
        let target = world.spawn_test_client(client);

        let saved = world.check_armor(target, 100, DamageFlags::empty());
        // Would save 80, but only 10 available.
        assert_eq!(saved, 10);
    }

    #[test]
    fn check_armor_no_armor_flag_bypasses() {
        let mut world = test_world();
        world.init_items(); // inventory[1] is Body Armor
        let mut client = crate::entity::ClientData::default();
        client.pers.inventory[1] = 100;
        let target = world.spawn_test_client(client);

        let saved = world.check_armor(target, 50, DamageFlags::NO_ARMOR);
        assert_eq!(saved, 0);
    }

    #[test]
    fn check_armor_returns_zero_for_non_clients() {
        let mut world = test_world();
        let target = world.spawn().unwrap();
        // No client data.

        let saved = world.check_armor(target, 50, DamageFlags::empty());
        assert_eq!(saved, 0);
    }

    // -- check_power_armor tests --

    /// A monster at the origin facing +x with `kind` power armor.
    fn armored_monster(world: &mut GameWorld, kind: i32, power: i32) -> EntityKey {
        let key = world.spawn().unwrap();
        let ent = world.entities.get_mut(key).unwrap();
        ent.svflags |= SvFlags::MONSTER.bits();
        ent.game.health = 100;
        ent.game.takedamage = TakeDamage::Yes;
        ent.monsterinfo = Some(Box::new(crate::entity::MonsterInfo {
            power_armor_type: kind,
            power_armor_power: power,
            ..Default::default()
        }));
        key
    }

    fn power(world: &GameWorld, key: EntityKey) -> i32 {
        let mi = world.entities.get(key).unwrap().monsterinfo.as_ref();
        mi.unwrap().power_armor_power
    }

    #[test]
    fn power_screen_saves_a_third_of_a_hit_from_the_front_at_a_cell_a_point() {
        let mut world = test_world();
        world.level.time = 3.0;
        let key = armored_monster(&mut world, POWER_ARMOR_SCREEN, 100);
        let front = Vec3f::new(100.0, 10.0, 0.0);
        let saved = world.check_power_armor(key, front, 30, DamageFlags::empty());
        assert_eq!(saved, 10);
        assert_eq!(power(&world, key), 90);
        let ent = world.entities.get(key).unwrap();
        assert!((ent.game.powerarmor_time - 3.2).abs() < 1e-5);
    }

    #[test]
    fn power_screen_does_not_cover_hits_from_the_side_or_behind() {
        // C: the screen works only when dot(point dir, forward) > 0.3.
        let mut world = test_world();
        let key = armored_monster(&mut world, POWER_ARMOR_SCREEN, 100);
        for point in [Vec3f::new(-100.0, 0.0, 0.0), Vec3f::new(0.0, 100.0, 0.0)] {
            assert_eq!(
                world.check_power_armor(key, point, 30, DamageFlags::empty()),
                0
            );
        }
        assert_eq!(power(&world, key), 100);
        assert_eq!(world.entities.get(key).unwrap().game.powerarmor_time, 0.0);
    }

    #[test]
    fn power_shield_saves_two_thirds_at_two_points_a_cell_up_to_its_power() {
        let mut world = test_world();
        let key = armored_monster(&mut world, POWER_ARMOR_SHIELD, 100);
        let behind = Vec3f::new(-100.0, 0.0, 0.0);
        assert_eq!(
            world.check_power_armor(key, behind, 30, DamageFlags::empty()),
            20
        );
        assert_eq!(power(&world, key), 90);
        // Only 5 cells left: 10 points saved, all cells used.
        world
            .entities
            .get_mut(key)
            .unwrap()
            .monsterinfo
            .as_mut()
            .unwrap()
            .power_armor_power = 5;
        assert_eq!(
            world.check_power_armor(key, behind, 30, DamageFlags::empty()),
            10
        );
        assert_eq!(power(&world, key), 0);
    }

    #[test]
    fn power_armor_saves_nothing_without_power_damage_or_armor_or_when_bypassed() {
        let mut world = test_world();
        let front = Vec3f::new(100.0, 0.0, 0.0);
        let empty = armored_monster(&mut world, POWER_ARMOR_SHIELD, 0);
        assert_eq!(
            world.check_power_armor(empty, front, 30, DamageFlags::empty()),
            0
        );
        let none = armored_monster(&mut world, POWER_ARMOR_NONE, 100);
        assert_eq!(
            world.check_power_armor(none, front, 30, DamageFlags::empty()),
            0
        );
        let shield = armored_monster(&mut world, POWER_ARMOR_SHIELD, 100);
        assert_eq!(
            world.check_power_armor(shield, front, 0, DamageFlags::empty()),
            0
        );
        assert_eq!(
            world.check_power_armor(shield, front, 30, DamageFlags::NO_ARMOR),
            0
        );
        // Neither a client nor a monster.
        let wall = world.spawn().unwrap();
        assert_eq!(
            world.check_power_armor(wall, front, 30, DamageFlags::empty()),
            0
        );
    }

    fn armored_client(world: &mut GameWorld, item: &str, flag: bool) -> EntityKey {
        world.init_items();
        let mut client = crate::entity::ClientData::default();
        let cells = world.find_item_by_name("Cells").unwrap();
        client.pers.inventory[cells] = 50;
        if let Some(index) = world.find_item_by_name(item) {
            client.pers.inventory[index] = 1;
        }
        let key = world.spawn_test_client(client);
        if flag {
            world.entities.get_mut(key).unwrap().game.flags |= EntityFlags::POWER_ARMOR;
        }
        key
    }

    fn cells(world: &GameWorld, key: EntityKey) -> i32 {
        let index = world.find_item_by_name("Cells").unwrap();
        world.client(key).unwrap().pers.inventory[index]
    }

    #[test]
    fn switched_on_power_shield_of_a_player_burns_its_cells() {
        // C PowerArmorType: the shield wins over the screen; FL_POWER_ARMOR
        // is the on switch.
        let mut world = test_world();
        let key = armored_client(&mut world, "Power Shield", true);
        let screen = world.find_item_by_name("Power Screen").unwrap();
        world.client_mut(key).unwrap().pers.inventory[screen] = 1;
        let behind = Vec3f::new(-100.0, 0.0, 0.0);
        assert_eq!(
            world.check_power_armor(key, behind, 30, DamageFlags::empty()),
            20
        );
        assert_eq!(cells(&world, key), 40);
    }

    #[test]
    fn players_power_screen_needs_the_switch_and_the_item() {
        let mut world = test_world();
        let front = Vec3f::new(100.0, 0.0, 0.0);
        let on = armored_client(&mut world, "Power Screen", true);
        assert_eq!(
            world.check_power_armor(on, front, 30, DamageFlags::empty()),
            10
        );
        assert_eq!(cells(&world, on), 40);
        let mut world = test_world();
        let off = armored_client(&mut world, "Power Screen", false);
        assert_eq!(
            world.check_power_armor(off, front, 30, DamageFlags::empty()),
            0
        );
        let mut world = test_world();
        let no_item = armored_client(&mut world, "", true);
        assert_eq!(
            world.check_power_armor(no_item, front, 30, DamageFlags::empty()),
            0
        );
        assert_eq!(cells(&world, no_item), 50);
    }

    #[test]
    fn t_damage_takes_power_armor_savings_off_the_hit() {
        let mut world = test_world();
        let key = armored_monster(&mut world, POWER_ARMOR_SCREEN, 100);
        let attacker = world.spawn().unwrap();
        world.t_damage(
            key,
            attacker,
            attacker,
            Vec3f::new(-1.0, 0.0, 0.0),
            Vec3f::new(100.0, 0.0, 0.0),
            Vec3f::ZERO,
            30,
            0,
            DamageFlags::empty(),
            MeansOfDeath::Blaster,
        );
        assert_eq!(world.entities.get(key).unwrap().game.health, 80);
        assert_eq!(power(&world, key), 90);
    }

    // -- t_damage tests --

    #[test]
    fn t_damage_reduces_health() {
        let mut world = test_world();
        let target = world.spawn().unwrap();
        let attacker = world.spawn().unwrap();

        {
            let ent = world.entities.get_mut(target).unwrap();
            ent.game.health = 100;
            ent.game.takedamage = TakeDamage::Yes;
        }

        world.t_damage(
            target,
            attacker,
            attacker,
            Vec3f::new(1.0, 0.0, 0.0),
            Vec3f::ZERO,
            Vec3f::ZERO,
            30,
            0,
            DamageFlags::empty(),
            MeansOfDeath::Blaster,
        );

        assert_eq!(world.entities.get(target).unwrap().game.health, 70);
    }

    #[test]
    fn t_damage_skips_no_takedamage() {
        let mut world = test_world();
        let target = world.spawn().unwrap();
        let attacker = world.spawn().unwrap();

        {
            let ent = world.entities.get_mut(target).unwrap();
            ent.game.health = 100;
            ent.game.takedamage = TakeDamage::No;
        }

        world.t_damage(
            target,
            attacker,
            attacker,
            Vec3f::ZERO,
            Vec3f::ZERO,
            Vec3f::ZERO,
            50,
            0,
            DamageFlags::empty(),
            MeansOfDeath::Blaster,
        );

        assert_eq!(world.entities.get(target).unwrap().game.health, 100);
    }

    #[test]
    fn t_damage_godmode_prevents_damage() {
        let mut world = test_world();
        let target = world.spawn().unwrap();
        let attacker = world.spawn().unwrap();

        {
            let ent = world.entities.get_mut(target).unwrap();
            ent.game.health = 100;
            ent.game.takedamage = TakeDamage::Yes;
            ent.game.flags = EntityFlags::GODMODE;
        }

        world.t_damage(
            target,
            attacker,
            attacker,
            Vec3f::ZERO,
            Vec3f::ZERO,
            Vec3f::ZERO,
            50,
            0,
            DamageFlags::empty(),
            MeansOfDeath::Blaster,
        );

        assert_eq!(world.entities.get(target).unwrap().game.health, 100);
    }

    #[test]
    fn t_damage_no_protection_bypasses_godmode() {
        let mut world = test_world();
        let target = world.spawn().unwrap();
        let attacker = world.spawn().unwrap();

        {
            let ent = world.entities.get_mut(target).unwrap();
            ent.game.health = 100;
            ent.game.takedamage = TakeDamage::Yes;
            ent.game.flags = EntityFlags::GODMODE;
        }

        world.t_damage(
            target,
            attacker,
            attacker,
            Vec3f::ZERO,
            Vec3f::ZERO,
            Vec3f::ZERO,
            50,
            0,
            DamageFlags::NO_PROTECTION,
            MeansOfDeath::TriggerHurt,
        );

        assert_eq!(world.entities.get(target).unwrap().game.health, 50);
    }

    #[test]
    fn t_damage_lethal_calls_killed() {
        let mut world = test_world();
        let target = world.spawn().unwrap();
        let attacker = world.spawn().unwrap();

        {
            let ent = world.entities.get_mut(target).unwrap();
            ent.game.health = 50;
            ent.game.takedamage = TakeDamage::Yes;
            ent.die = Some(counting_die);
        }

        world.t_damage(
            target,
            attacker,
            attacker,
            Vec3f::ZERO,
            Vec3f::ZERO,
            Vec3f::ZERO,
            100,
            0,
            DamageFlags::empty(),
            MeansOfDeath::Rocket,
        );

        assert_eq!(world.entities.get(target).unwrap().game.count, 1); // died
                                                                       // Health should be negative.
        assert!(world.entities.get(target).unwrap().game.health <= 0);
    }

    #[test]
    fn t_damage_calls_pain_when_surviving() {
        let mut world = test_world();
        let target = world.spawn().unwrap();
        let attacker = world.spawn().unwrap();

        {
            let ent = world.entities.get_mut(target).unwrap();
            ent.game.health = 100;
            ent.game.takedamage = TakeDamage::Yes;
            ent.pain = Some(counting_pain);
        }

        world.t_damage(
            target,
            attacker,
            attacker,
            Vec3f::ZERO,
            Vec3f::ZERO,
            Vec3f::ZERO,
            30,
            10,
            DamageFlags::empty(),
            MeansOfDeath::Blaster,
        );

        assert_eq!(world.entities.get(target).unwrap().game.style, 1); // pained
    }

    #[test]
    fn t_damage_no_knockback_flag() {
        let mut world = test_world();
        let target = world.spawn().unwrap();
        let attacker = world.spawn().unwrap();

        {
            let ent = world.entities.get_mut(target).unwrap();
            ent.game.health = 100;
            ent.game.takedamage = TakeDamage::Yes;
            ent.game.mass = 100;
            ent.game.movetype = MoveType::Step;
            ent.velocity = Vec3f::ZERO;
        }

        world.t_damage(
            target,
            attacker,
            attacker,
            Vec3f::new(1.0, 0.0, 0.0),
            Vec3f::ZERO,
            Vec3f::ZERO,
            30,
            50,
            DamageFlags::NO_KNOCKBACK,
            MeansOfDeath::Blaster,
        );

        // Velocity should be unchanged — knockback was suppressed.
        let v = world.entities.get(target).unwrap().velocity;
        assert_eq!(v, Vec3f::ZERO);
    }

    #[test]
    fn t_damage_applies_knockback() {
        let mut world = test_world();
        let target = world.spawn().unwrap();
        let attacker = world.spawn().unwrap();

        {
            let ent = world.entities.get_mut(target).unwrap();
            ent.game.health = 100;
            ent.game.takedamage = TakeDamage::Yes;
            ent.game.mass = 100;
            ent.game.movetype = MoveType::Step;
            ent.velocity = Vec3f::ZERO;
        }

        world.t_damage(
            target,
            attacker,
            attacker,
            Vec3f::new(1.0, 0.0, 0.0),
            Vec3f::ZERO,
            Vec3f::ZERO,
            30,
            50,
            DamageFlags::empty(),
            MeansOfDeath::Shotgun,
        );

        // Velocity should have changed from knockback.
        let v = world.entities.get(target).unwrap().velocity;
        assert!(v.x > 0.0);
    }

    #[test]
    fn t_damage_with_armor() {
        let mut world = test_world();
        world.init_items(); // inventory[1] is Body Armor
        let mut client = crate::entity::ClientData::default();
        client.pers.inventory[1] = 100; // 100 armor
        let target = world.spawn_test_client(client);
        let attacker = world.spawn().unwrap();

        {
            let ent = world.entities.get_mut(target).unwrap();
            ent.game.health = 100;
            ent.game.takedamage = TakeDamage::Yes;
        }

        world.t_damage(
            target,
            attacker,
            attacker,
            Vec3f::ZERO,
            Vec3f::ZERO,
            Vec3f::ZERO,
            50,
            0,
            DamageFlags::empty(),
            MeansOfDeath::Machinegun,
        );

        let ent = world.entities.get(target).unwrap();
        // Armor absorbs 80%: ceil(0.8 * 50) = 40 saved
        // Health: 100 - (50 - 40) = 90
        assert_eq!(ent.game.health, 90);
        // Armor: 100 - 40 = 60
        assert_eq!(world.client(target).unwrap().pers.inventory[1], 60);
    }

    #[test]
    fn t_damage_surprise_bonus() {
        let mut world = test_world();
        let target = world.spawn().unwrap();
        // Attacker is a "client" for surprise bonus.
        let attacker = world.spawn_test_client(crate::entity::ClientData::default());

        {
            let ent = world.entities.get_mut(target).unwrap();
            ent.game.health = 200;
            ent.game.takedamage = TakeDamage::Yes;
            ent.svflags = SvFlags::MONSTER.bits(); // is a monster
            ent.game.enemy = None; // hasn't seen attacker
        }

        world.t_damage(
            target,
            attacker,
            attacker,
            Vec3f::ZERO,
            Vec3f::ZERO,
            Vec3f::ZERO,
            30, // base damage
            0,
            DamageFlags::empty(),
            MeansOfDeath::Railgun,
        );

        // Surprise: 2x damage = 60 → health = 200 - 60 = 140
        assert_eq!(world.entities.get(target).unwrap().game.health, 140);
    }

    // -- t_radius_damage tests --

    #[test]
    fn radius_damage_hits_nearby() {
        let mut world = test_world();
        let inflictor = world.spawn().unwrap();
        let target = world.spawn().unwrap();

        // Place inflictor at origin, target at (50, 0, 0).
        world.entities.get_mut(inflictor).unwrap().state.origin = Vec3f::ZERO;
        {
            let ent = world.entities.get_mut(target).unwrap();
            ent.state.origin = Vec3f::new(50.0, 0.0, 0.0);
            ent.game.health = 200;
            ent.game.takedamage = TakeDamage::Yes;
            ent.solid = Solid::Bbox;
        }

        world.t_radius_damage(
            inflictor,
            inflictor,
            120.0, // base damage
            None,  // no ignore
            200.0, // radius
            MeansOfDeath::RocketSplash,
        );

        let health = world.entities.get(target).unwrap().game.health;
        // points = 120 - 0.5 * 50 = 95
        // health = 200 - 95 = 105
        assert_eq!(health, 105);
    }

    #[test]
    fn radius_damage_skips_non_solid_entities_like_c_findradius() {
        let mut world = test_world();
        let inflictor = world.spawn().unwrap();
        let target = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(target).unwrap();
            ent.state.origin = Vec3f::new(50.0, 0.0, 0.0);
            ent.game.health = 200;
            ent.game.takedamage = TakeDamage::Yes;
            ent.solid = Solid::Not;
        }
        world.t_radius_damage(
            inflictor,
            inflictor,
            120.0,
            None,
            200.0,
            MeansOfDeath::RocketSplash,
        );
        assert_eq!(world.entities.get(target).unwrap().game.health, 200);
    }

    #[test]
    fn radius_damage_ignores_specified_entity() {
        let mut world = test_world();
        let inflictor = world.spawn().unwrap();
        let target = world.spawn().unwrap();

        world.entities.get_mut(inflictor).unwrap().state.origin = Vec3f::ZERO;
        {
            let ent = world.entities.get_mut(target).unwrap();
            ent.state.origin = Vec3f::new(10.0, 0.0, 0.0);
            ent.game.health = 100;
            ent.game.takedamage = TakeDamage::Yes;
            ent.solid = Solid::Bbox;
        }

        world.t_radius_damage(
            inflictor,
            inflictor,
            120.0,
            Some(target), // ignore this entity
            200.0,
            MeansOfDeath::GrenadeSplash,
        );

        // Target should be untouched.
        assert_eq!(world.entities.get(target).unwrap().game.health, 100);
    }

    #[test]
    fn radius_damage_out_of_range() {
        let mut world = test_world();
        let inflictor = world.spawn().unwrap();
        let target = world.spawn().unwrap();

        world.entities.get_mut(inflictor).unwrap().state.origin = Vec3f::ZERO;
        {
            let ent = world.entities.get_mut(target).unwrap();
            ent.state.origin = Vec3f::new(500.0, 0.0, 0.0);
            ent.game.health = 100;
            ent.game.takedamage = TakeDamage::Yes;
            ent.solid = Solid::Bbox;
        }

        world.t_radius_damage(
            inflictor,
            inflictor,
            120.0,
            None,
            100.0, // radius 100, target at 500
            MeansOfDeath::RocketSplash,
        );

        // Target should be untouched — out of range.
        assert_eq!(world.entities.get(target).unwrap().game.health, 100);
    }

    #[test]
    fn radius_damage_self_damage_halved() {
        let mut world = test_world();
        let player = world.spawn().unwrap();

        {
            let ent = world.entities.get_mut(player).unwrap();
            ent.state.origin = Vec3f::ZERO;
            ent.game.health = 200;
            ent.game.takedamage = TakeDamage::Yes;
            ent.solid = Solid::Bbox;
        }

        // Player is both inflictor and attacker — self-damage.
        world.t_radius_damage(
            player,
            player,
            120.0,
            None,
            200.0,
            MeansOfDeath::RocketSplash,
        );

        let health = world.entities.get(player).unwrap().game.health;
        // At distance 0: points = 120 - 0 = 120, halved for self = 60
        // health = 200 - 60 = 140
        assert_eq!(health, 140);
    }

    // -- killed tests --

    #[test]
    fn killed_increments_monster_counter() {
        let mut world = test_world();
        let target = world.spawn().unwrap();
        let attacker = world.spawn().unwrap();

        {
            let ent = world.entities.get_mut(target).unwrap();
            ent.svflags = SvFlags::MONSTER.bits();
            ent.game.deadflag = DeadFlag::No;
        }

        assert_eq!(world.level.killed_monsters, 0);
        world.killed(target, attacker, attacker, 100, Vec3f::ZERO);
        assert_eq!(world.level.killed_monsters, 1);
    }

    #[test]
    fn killed_sets_enemy_to_attacker() {
        let mut world = test_world();
        let target = world.spawn().unwrap();
        let attacker = world.spawn().unwrap();

        world.killed(target, attacker, attacker, 100, Vec3f::ZERO);

        assert_eq!(
            world.entities.get(target).unwrap().game.enemy,
            Some(attacker)
        );
    }

    // -- C parity tests --

    fn counting_die(
        world: &mut GameWorld,
        key: EntityKey,
        _: EntityKey,
        _: EntityKey,
        _: i32,
        _: Vec3f,
    ) {
        world.entities.get_mut(key).unwrap().game.count += 1;
    }

    fn counting_pain(world: &mut GameWorld, key: EntityKey, _: EntityKey, _: f32, _: i32) {
        world.entities.get_mut(key).unwrap().game.style += 1;
    }

    fn damageable(world: &mut GameWorld, origin: Vec3f, health: i32) -> EntityKey {
        let key = world.spawn().unwrap();
        make_damageable(world, key, origin, health);
        key
    }

    fn make_damageable(world: &mut GameWorld, key: EntityKey, origin: Vec3f, health: i32) {
        let ent = world.entities.get_mut(key).unwrap();
        ent.state.origin = origin;
        ent.game.health = health;
        ent.game.takedamage = TakeDamage::Yes;
        ent.solid = Solid::Bbox;
        ent.die = Some(counting_die);
        ent.pain = Some(counting_pain);
    }

    /// A fake player wearing `count` of `armor`.
    fn wearing(world: &mut GameWorld, armor: &str, count: i32) -> EntityKey {
        world.init_items();
        let index = world.find_item(armor).unwrap();
        let mut client = crate::entity::ClientData::default();
        client.pers.inventory[index] = count;
        world.spawn_test_client(client)
    }

    fn worn(world: &GameWorld, key: EntityKey, armor: &str) -> i32 {
        let index = world.find_item(armor).unwrap();
        world.client(key).unwrap().pers.inventory[index]
    }

    #[test]
    fn can_damage_traces_from_the_inflictor_and_skips_it() {
        let (mut world, gi) = crate::world::test_world_recording();
        let target = world.spawn().unwrap();
        let inflictor = world.spawn().unwrap();

        world.can_damage(target, inflictor);

        assert_eq!(gi.trace_pass_ents.borrow()[0], world.number_of(inflictor));
    }

    #[test]
    fn can_damage_reaches_a_bmodel_whose_own_brush_stops_the_trace() {
        let (mut world, gi) = crate::world::test_world_recording();
        let target = world.spawn().unwrap();
        let inflictor = world.spawn().unwrap();
        world.entities.get_mut(target).unwrap().game.movetype = MoveType::Push;
        *gi.next_trace.borrow_mut() = Some(Trace {
            fraction: 0.5,
            ent_index: world.number_of(target),
            ..Trace::default()
        });

        assert!(world.can_damage(target, inflictor));
    }

    #[test]
    fn can_damage_fails_for_a_bmodel_behind_another_solid() {
        let (mut world, gi) = crate::world::test_world_recording();
        let target = world.spawn().unwrap();
        let inflictor = world.spawn().unwrap();
        world.entities.get_mut(target).unwrap().game.movetype = MoveType::Push;
        *gi.next_trace.borrow_mut() = Some(Trace {
            fraction: 0.5,
            ent_index: Some(0),
            ..Trace::default()
        });

        assert!(!world.can_damage(target, inflictor));
    }

    #[test]
    fn can_damage_tries_the_corners_when_the_centre_is_blocked() {
        let (mut world, gi) = crate::world::test_world_recording();
        let target = world.spawn().unwrap();
        let inflictor = world.spawn().unwrap();
        *gi.next_trace.borrow_mut() = Some(Trace {
            fraction: 0.5,
            ..Trace::default()
        });

        assert!(world.can_damage(target, inflictor));
        assert_eq!(gi.trace_pass_ents.borrow().len(), 2);
    }

    #[test]
    fn t_damage_that_takes_no_health_does_not_kill_again() {
        let mut world = test_world();
        let target = damageable(&mut world, Vec3f::ZERO, 0);
        let attacker = world.spawn().unwrap();

        world.t_damage(
            target,
            attacker,
            attacker,
            Vec3f::X,
            Vec3f::ZERO,
            Vec3f::ZERO,
            0,
            0,
            DamageFlags::empty(),
            MeansOfDeath::Unknown,
        );

        assert_eq!(world.entities.get(target).unwrap().game.count, 0);
    }

    #[test]
    fn t_damage_godmode_client_feels_no_pain_from_unprotected_damage() {
        let mut world = test_world();
        let target = world.spawn_test_client(crate::entity::ClientData::default());
        make_damageable(&mut world, target, Vec3f::ZERO, 100);
        let attacker = world.spawn().unwrap();
        world.entities.get_mut(target).unwrap().game.flags |= EntityFlags::GODMODE;

        world.t_damage(
            target,
            attacker,
            attacker,
            Vec3f::X,
            Vec3f::ZERO,
            Vec3f::ZERO,
            10,
            0,
            DamageFlags::NO_PROTECTION,
            MeansOfDeath::Unknown,
        );

        let ent = world.entities.get(target).unwrap();
        assert_eq!(ent.game.health, 90);
        assert_eq!(ent.game.style, 0);
    }

    #[test]
    fn radius_damage_skips_freed_entities() {
        let mut world = test_world();
        let inflictor = world.spawn().unwrap();
        let freed = damageable(&mut world, Vec3f::new(10.0, 0.0, 0.0), 100);
        world.free_entity(freed);
        {
            // Stale fields a soft-freed edict might still carry.
            let ent = world.entities.get_mut(freed).unwrap();
            ent.game.takedamage = TakeDamage::Yes;
            ent.game.health = 100;
        }

        world.t_radius_damage(
            inflictor,
            inflictor,
            120.0,
            None,
            200.0,
            MeansOfDeath::RocketSplash,
        );

        assert_eq!(world.entities.get(freed).unwrap().game.health, 100);
    }

    #[test]
    fn check_armor_uses_the_worn_armor_protection() {
        let mut world = test_world();
        let target = wearing(&mut world, "item_armor_jacket", 50);

        // Jacket armor: ceil(0.3f * 50) = 16, as in C (0.3f is a hair
        // over 0.3).
        assert_eq!(world.check_armor(target, 50, DamageFlags::empty()), 16);
        assert_eq!(worn(&world, target, "item_armor_jacket"), 34);
    }

    #[test]
    fn check_armor_uses_energy_protection_for_energy_damage() {
        let mut world = test_world();
        world.set_max_clients(2);
        let jacket = wearing(&mut world, "item_armor_jacket", 50);
        let combat = wearing(&mut world, "item_armor_combat", 50);

        assert_eq!(world.check_armor(jacket, 50, DamageFlags::ENERGY), 0);
        // Combat armor: ceil(0.3f * 50) = 16.
        assert_eq!(world.check_armor(combat, 50, DamageFlags::ENERGY), 16);
    }

    #[test]
    fn killed_monster_fires_its_targets_and_stops_touching() {
        fn note_use(world: &mut GameWorld, key: EntityKey, _: EntityKey, _: EntityKey) {
            world.entities.get_mut(key).unwrap().game.count += 1;
        }
        fn touch(
            _: &mut GameWorld,
            _: EntityKey,
            _: EntityKey,
            _: Option<&Plane>,
            _: Option<&Surface>,
        ) {
        }

        let mut world = test_world();
        let monster = damageable(&mut world, Vec3f::ZERO, 0);
        let attacker = world.spawn().unwrap();
        let relay = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(monster).unwrap();
            ent.svflags |= SvFlags::MONSTER.bits();
            ent.game.movetype = MoveType::Step;
            ent.game.target = "door".to_string();
            ent.touch = Some(touch);
            ent.monsterinfo = Some(Box::default());
        }
        {
            let ent = world.entities.get_mut(relay).unwrap();
            ent.game.targetname = "door".to_string();
            ent.use_fn = Some(note_use);
        }

        world.killed(monster, attacker, attacker, 10, Vec3f::ZERO);

        assert_eq!(world.entities.get(relay).unwrap().game.count, 1);
        let ent = world.entities.get(monster).unwrap();
        assert!(ent.touch.is_none());
        assert_eq!(ent.game.count, 1); // die ran
    }

    #[test]
    fn killed_monster_fires_its_deathtarget_instead_of_its_target() {
        fn note_use(world: &mut GameWorld, key: EntityKey, _: EntityKey, _: EntityKey) {
            world.entities.get_mut(key).unwrap().game.count += 1;
        }
        let mut world = test_world();
        let monster = damageable(&mut world, Vec3f::ZERO, 0);
        let attacker = world.spawn().unwrap();
        let fields: std::collections::HashMap<String, String> = [
            ("target".to_string(), "door".to_string()),
            ("deathtarget".to_string(), "dt".to_string()),
        ]
        .into();
        {
            let ent = world.entities.get_mut(monster).unwrap();
            crate::spawn::apply_spawn_fields(ent, &fields);
            ent.svflags |= SvFlags::MONSTER.bits();
            ent.game.movetype = MoveType::Step;
            ent.monsterinfo = Some(Box::default());
        }
        let mut relay = |name: &str| {
            let key = world.spawn().unwrap();
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.targetname = name.to_string();
            ent.use_fn = Some(note_use);
            key
        };
        let door = relay("door");
        let dt = relay("dt");

        world.killed(monster, attacker, attacker, 10, Vec3f::ZERO);

        assert_eq!(world.entities.get(dt).unwrap().game.count, 1);
        assert_eq!(world.entities.get(door).unwrap().game.count, 0);
    }

    fn dying_monster(world: &mut GameWorld, item: Option<usize>) -> EntityKey {
        let monster = damageable(world, Vec3f::new(1.0, 2.0, 3.0), 0);
        let ent = world.entities.get_mut(monster).unwrap();
        ent.svflags |= SvFlags::MONSTER.bits();
        ent.game.movetype = MoveType::Step;
        ent.game.flags |= EntityFlags::FLY | EntityFlags::SWIM;
        ent.game.item = item;
        ent.monsterinfo = Some(Box::default());
        monster
    }

    fn drops(world: &GameWorld) -> Vec<EntityKey> {
        world
            .entities
            .iter()
            .filter(|(_, e)| e.in_use && e.game.spawnflags & crate::items::DROPPED_ITEM != 0)
            .map(|(k, _)| k)
            .collect()
    }

    #[test]
    fn killed_monster_drops_its_item_and_forgets_it() {
        let mut world = test_world();
        world.init_items();
        let shotgun = world.find_item("weapon_shotgun").unwrap();
        let monster = dying_monster(&mut world, Some(shotgun));
        let attacker = world.spawn().unwrap();

        world.killed(monster, attacker, attacker, 10, Vec3f::ZERO);

        let dropped = drops(&world);
        assert_eq!(dropped.len(), 1);
        let ent = world.entities.get(dropped[0]).unwrap();
        assert_eq!(ent.game.item, Some(shotgun));
        assert_eq!(ent.owner, Some(monster));
        assert_eq!(ent.state.origin, Vec3f::new(1.0, 2.0, 3.0));
        assert_eq!(world.entities.get(monster).unwrap().game.item, None);
    }

    #[test]
    fn killed_monster_without_an_item_drops_nothing() {
        let mut world = test_world();
        world.init_items();
        let monster = dying_monster(&mut world, None);
        let attacker = world.spawn().unwrap();

        world.killed(monster, attacker, attacker, 10, Vec3f::ZERO);

        assert!(drops(&world).is_empty());
    }

    #[test]
    fn killed_monster_stops_flying_and_swimming() {
        let mut world = test_world();
        let monster = dying_monster(&mut world, None);
        let attacker = world.spawn().unwrap();

        world.killed(monster, attacker, attacker, 10, Vec3f::ZERO);

        let flags = world.entities.get(monster).unwrap().game.flags;
        assert!(!flags.intersects(EntityFlags::FLY | EntityFlags::SWIM));
    }

    #[test]
    fn killed_good_guy_monster_is_not_counted() {
        let mut world = test_world();
        let monster = damageable(&mut world, Vec3f::ZERO, 0);
        let attacker = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(monster).unwrap();
            ent.svflags |= SvFlags::MONSTER.bits();
            ent.monsterinfo = Some(Box::new(crate::entity::MonsterInfo {
                aiflags: AiFlags::GOOD_GUY,
                ..Default::default()
            }));
        }

        world.killed(monster, attacker, attacker, 10, Vec3f::ZERO);

        assert_eq!(world.level.killed_monsters, 0);
    }

    // -- M_ReactToDamage and the monster branch of T_Damage --

    /// A live monster of `classname` at the origin, counting pain in
    /// `game.style`.
    fn live_monster(world: &mut GameWorld, classname: &str) -> EntityKey {
        let key = damageable(world, Vec3f::ZERO, 100);
        let ent = world.entities.get_mut(key).unwrap();
        ent.svflags |= SvFlags::MONSTER.bits();
        ent.game.classname = classname.to_string();
        ent.monsterinfo = Some(Box::default());
        key
    }

    /// A live player at (x, 0, 0); the second and later ones only carry a
    /// client slot, as coop players would.
    fn live_player(world: &mut GameWorld, x: f32) -> EntityKey {
        let key = if world.entities.key_for_number(1).is_none() {
            world.spawn_test_client(crate::entity::ClientData::default())
        } else {
            let key = world.spawn().unwrap();
            world.entities.get_mut(key).unwrap().client = Some(0);
            key
        };
        make_damageable(world, key, Vec3f::new(x, 0.0, 0.0), 100);
        key
    }

    fn hurt(world: &mut GameWorld, target: EntityKey, attacker: EntityKey) {
        world.t_damage(
            target,
            attacker,
            attacker,
            Vec3f::ZERO,
            Vec3f::ZERO,
            Vec3f::ZERO,
            5,
            0,
            DamageFlags::empty(),
            MeansOfDeath::Blaster,
        );
    }

    fn enemy(world: &GameWorld, key: EntityKey) -> Option<EntityKey> {
        world.entities.get(key).unwrap().game.enemy
    }

    fn oldenemy(world: &GameWorld, key: EntityKey) -> Option<EntityKey> {
        world.entities.get(key).unwrap().game.oldenemy
    }

    fn aiflags_of(world: &mut GameWorld, key: EntityKey) -> &mut AiFlags {
        &mut world
            .entities
            .get_mut(key)
            .unwrap()
            .monsterinfo
            .as_mut()
            .unwrap()
            .aiflags
    }

    #[test]
    fn t_damage_makes_a_monster_hunt_the_player_who_hurt_it_and_feel_pain() {
        let mut world = test_world();
        let monster = live_monster(&mut world, "monster_soldier");
        let player = live_player(&mut world, 100.0);
        *aiflags_of(&mut world, monster) = AiFlags::SOUND_TARGET;

        hurt(&mut world, monster, player);

        let ent = world.entities.get(monster).unwrap();
        assert_eq!(ent.game.enemy, Some(player));
        assert_eq!(ent.game.goalentity, Some(player), "FoundTarget");
        assert_eq!(ent.game.style, 1, "pain");
        assert!(!ent
            .monsterinfo
            .as_ref()
            .unwrap()
            .aiflags
            .contains(AiFlags::SOUND_TARGET));
    }

    #[test]
    fn m_react_to_damage_ignores_attackers_that_are_neither_players_nor_monsters() {
        let mut world = test_world();
        let monster = live_monster(&mut world, "monster_soldier");
        let barrel = damageable(&mut world, Vec3f::ZERO, 10);

        hurt(&mut world, monster, barrel);

        assert_eq!(enemy(&world, monster), None);
    }

    #[test]
    fn m_react_to_damage_ignores_itself_its_enemy_and_a_dead_monster() {
        let mut world = test_world();
        let monster = live_monster(&mut world, "monster_soldier");
        let player = live_player(&mut world, 100.0);
        world.m_react_to_damage(monster, monster);
        assert_eq!(enemy(&world, monster), None);

        world.entities.get_mut(monster).unwrap().game.health = 0;
        world.m_react_to_damage(monster, player);
        assert_eq!(enemy(&world, monster), None);
    }

    #[test]
    fn m_react_to_damage_good_guy_forgives_players_and_other_good_guys() {
        let mut world = test_world();
        let good = live_monster(&mut world, "misc_insane");
        *aiflags_of(&mut world, good) = AiFlags::GOOD_GUY;
        let player = live_player(&mut world, 100.0);
        let other_good = live_monster(&mut world, "misc_insane2");
        *aiflags_of(&mut world, other_good) = AiFlags::GOOD_GUY;

        world.m_react_to_damage(good, player);
        world.m_react_to_damage(good, other_good);

        assert_eq!(enemy(&world, good), None);
    }

    #[test]
    fn m_react_to_damage_keeps_a_visible_player_enemy_and_remembers_the_new_one() {
        let mut world = test_world();
        let monster = live_monster(&mut world, "monster_soldier");
        let first = live_player(&mut world, 100.0);
        let second = live_player(&mut world, -100.0);
        world.entities.get_mut(monster).unwrap().game.enemy = Some(first);

        world.m_react_to_damage(monster, second);

        assert_eq!(enemy(&world, monster), Some(first));
        assert_eq!(oldenemy(&world, monster), Some(second));
    }

    #[test]
    fn m_react_to_damage_switches_from_a_player_it_cannot_see() {
        let (mut world, gi) = crate::world::test_world_recording();
        let monster = live_monster(&mut world, "monster_soldier");
        let first = live_player(&mut world, 100.0);
        let second = live_player(&mut world, -100.0);
        world.entities.get_mut(monster).unwrap().game.enemy = Some(first);
        *gi.next_trace.borrow_mut() = Some(Trace {
            fraction: 0.5,
            ..Trace::default()
        });

        world.m_react_to_damage(monster, second);

        assert_eq!(enemy(&world, monster), Some(second));
        assert_eq!(oldenemy(&world, monster), Some(first));
    }

    #[test]
    fn m_react_to_damage_ducked_monster_changes_enemy_without_hunting_it() {
        let mut world = test_world();
        let monster = live_monster(&mut world, "monster_soldier");
        let player = live_player(&mut world, 100.0);
        *aiflags_of(&mut world, monster) = AiFlags::DUCKED;

        hurt(&mut world, monster, player);

        let ent = world.entities.get(monster).unwrap();
        assert_eq!(ent.game.enemy, Some(player));
        assert_eq!(ent.game.goalentity, None);
        assert_eq!(ent.game.style, 0, "no pain while ducked");
    }

    #[test]
    fn m_react_to_damage_turns_on_a_monster_of_another_kind_that_walks_too() {
        let mut world = test_world();
        let monster = live_monster(&mut world, "monster_soldier");
        let player = live_player(&mut world, 100.0);
        world.entities.get_mut(monster).unwrap().game.enemy = Some(player);
        let gunner = live_monster(&mut world, "monster_gunner");

        world.m_react_to_damage(monster, gunner);

        assert_eq!(enemy(&world, monster), Some(gunner));
        assert_eq!(oldenemy(&world, monster), Some(player));
    }

    #[test]
    fn m_react_to_damage_forgives_a_tank_unless_it_meant_it() {
        for tank in [
            "monster_tank",
            "monster_supertank",
            "monster_makron",
            "monster_jorg",
        ] {
            let mut world = test_world();
            let monster = live_monster(&mut world, "monster_soldier");
            let attacker = live_monster(&mut world, tank);

            world.m_react_to_damage(monster, attacker);
            assert_eq!(enemy(&world, monster), None, "{tank}");

            world.entities.get_mut(attacker).unwrap().game.enemy = Some(monster);
            world.m_react_to_damage(monster, attacker);
            assert_eq!(enemy(&world, monster), Some(attacker), "{tank}");
        }
    }

    #[test]
    fn m_react_to_damage_from_its_own_kind_helps_it_against_their_enemy() {
        let mut world = test_world();
        let monster = live_monster(&mut world, "monster_soldier");
        let buddy = live_monster(&mut world, "monster_soldier");
        let player = live_player(&mut world, 100.0);
        world.entities.get_mut(buddy).unwrap().game.enemy = Some(player);

        world.m_react_to_damage(monster, buddy);

        assert_eq!(enemy(&world, monster), Some(player));
    }

    #[test]
    fn m_react_to_damage_from_a_flyer_counts_only_if_it_meant_it() {
        let mut world = test_world();
        let monster = live_monster(&mut world, "monster_soldier");
        let flyer = live_monster(&mut world, "monster_flyer");
        world.entities.get_mut(flyer).unwrap().game.flags |= EntityFlags::FLY;

        world.m_react_to_damage(monster, flyer);

        assert_eq!(enemy(&world, monster), None);
    }

    #[test]
    fn t_damage_on_nightmare_keeps_a_monster_out_of_pain_for_5_seconds() {
        let mut world = test_world();
        world.game.skill = 3;
        world.level.time = 2.0;
        let monster = live_monster(&mut world, "monster_soldier");
        let player = live_player(&mut world, 100.0);

        hurt(&mut world, monster, player);

        assert_eq!(
            world.entities.get(monster).unwrap().game.pain_debounce_time,
            7.0
        );
    }

    #[test]
    fn t_damage_below_nightmare_leaves_the_pain_debounce_to_the_monster() {
        let mut world = test_world();
        world.level.time = 2.0;
        let monster = live_monster(&mut world, "monster_soldier");
        let player = live_player(&mut world, 100.0);

        hurt(&mut world, monster, player);

        assert_eq!(
            world.entities.get(monster).unwrap().game.pain_debounce_time,
            0.0
        );
    }

    #[test]
    fn t_damage_to_a_monster_that_takes_none_still_makes_it_angry() {
        let mut world = test_world();
        let monster = live_monster(&mut world, "monster_soldier");
        world.entities.get_mut(monster).unwrap().game.flags |= EntityFlags::GODMODE;
        let player = live_player(&mut world, 100.0);

        hurt(&mut world, monster, player);

        let ent = world.entities.get(monster).unwrap();
        assert_eq!(ent.game.enemy, Some(player));
        assert_eq!(ent.game.style, 0, "no pain without damage");
    }
}
