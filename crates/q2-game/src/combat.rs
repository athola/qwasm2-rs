//! Combat and damage system — T_Damage, T_RadiusDamage, Killed.
//!
//! Faithful port of `g_combat.c` (762 lines) from the C source.
//! Handles damage application, armor absorption, knockback, kill tracking,
//! and radius (splash) damage.
//!
//! # C source reference
//! `~/Qwasm2/src/game/g_combat.c`

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
        let (targ_origin, targ_mins, targ_maxs, targ_movetype) = {
            let Some(targ) = self.entities.get(target) else {
                return false;
            };
            (
                targ.state.origin,
                targ.mins,
                targ.maxs,
                targ.game.movetype,
            )
        };
        let inf_origin = match self.entities.get(inflictor) {
            Some(e) => e.state.origin,
            None => return false,
        };

        // For pushable entities (doors), trace to bounding box center.
        if targ_movetype == MoveType::Push {
            let dest = (targ_mins + targ_maxs) * 0.5 + targ_origin;
            let trace =
                self.gi
                    .trace(inf_origin, Vec3f::ZERO, Vec3f::ZERO, dest, None, 1);
            return trace.fraction == 1.0;
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
            let trace =
                self.gi
                    .trace(inf_origin, Vec3f::ZERO, Vec3f::ZERO, dest, None, 1);
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
    fn apply_knockback(
        &mut self,
        target: EntityKey,
        dir: Vec3f,
        knockback: f32,
        scale: f32,
    ) {
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

    /// Calculate how much damage armor absorbs.
    /// Returns the amount of damage saved by armor.
    fn check_armor(
        &mut self,
        target: EntityKey,
        damage: i32,
        dflags: DamageFlags,
    ) -> i32 {
        if dflags.contains(DamageFlags::NO_ARMOR) {
            return 0;
        }

        let Some(ent) = self.entities.get(target) else {
            return 0;
        };
        let client = match &ent.client {
            Some(c) => c,
            None => return 0,
        };

        // Determine armor type from inventory.
        // Armor index 0 = none. For now, use a simplified lookup:
        // body armor = index 1, combat = 2, jacket = 3.
        // The actual armor type is determined by which armor item the
        // player has. This will be connected to the item system in TASK-006.
        let armor_count = client.pers.inventory[1]; // armor points
        if armor_count <= 0 {
            return 0;
        }

        // Use body armor protection as default (0.8 for both normal and energy).
        // TODO: Select protection based on actual equipped armor type.
        // Energy protection differs from normal on jacket/combat armor.
        let protection: f32 = 0.8;

        let mut armor_save = (protection * damage as f32).ceil() as i32;

        // Can't save more armor than we have.
        if armor_save > armor_count {
            armor_save = armor_count;
        }

        // Reduce armor count.
        if let Some(ent) = self.entities.get_mut(target) {
            if let Some(client) = &mut ent.client {
                client.pers.inventory[1] -= armor_save;
            }
        }

        armor_save
    }

    // -----------------------------------------------------------------------
    // SpawnDamage — visual effect
    // C ref: g_combat.c:169-177
    // -----------------------------------------------------------------------

    /// Send a damage visual effect to clients (sparks, blood, etc.).
    fn spawn_damage(&self, effect_type: i32, _origin: Vec3f, _normal: Vec3f) {
        // In full implementation, this writes a temp entity message:
        //   gi.write_byte(SVC_TEMP_ENTITY);
        //   gi.write_byte(effect_type);
        //   gi.write_position(origin);
        //   gi.write_dir(normal);
        //   gi.multicast(origin, Multicast::PVS);
        // For now, just log it.
        let _ = effect_type;
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
        // Set the dead entity's enemy to the attacker.
        if let Some(ent) = self.entities.get_mut(target) {
            ent.game.enemy = Some(attacker);
        }

        // Monster kill tracking.
        let is_monster = self
            .entities
            .get(target)
            .map(|e| e.svflags & SvFlags::MONSTER.bits() != 0)
            .unwrap_or(false);
        let is_dead_already = self
            .entities
            .get(target)
            .map(|e| e.game.deadflag == DeadFlag::Dead)
            .unwrap_or(true);

        if is_monster && !is_dead_already {
            self.level.killed_monsters += 1;
        }

        // Call die callback.
        let die_fn = self.entities.get(target).and_then(|e| e.die);
        if let Some(die) = die_fn {
            die(self, target, inflictor, attacker, damage, point);
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
        normal: Vec3f,
        damage: i32,
        knockback: i32,
        dflags: DamageFlags,
        _means_of_death: MeansOfDeath,
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

        if !dflags.contains(DamageFlags::RADIUS)
            && is_monster
            && attacker_is_client
            && !has_enemy
        {
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
        let mut save = 0;

        // Check god mode.
        let godmode = self
            .entities
            .get(target)
            .map(|e| e.game.flags.contains(EntityFlags::GODMODE))
            .unwrap_or(false);
        if godmode && !dflags.contains(DamageFlags::NO_PROTECTION) {
            take = 0;
            save = damage;
            self.spawn_damage(0, point, normal);
        }

        // Apply armor (if not god mode).
        if take > 0 {
            let armor_save = self.check_armor(target, take, dflags);
            take -= armor_save;
            save += armor_save;
        }

        // Apply damage to health.
        if take > 0 {
            // Spawn visual effect.
            let te_type = if is_monster
                || self
                    .entities
                    .get(target)
                    .map(|e| e.client.is_some())
                    .unwrap_or(false)
            {
                1 // TE_BLOOD
            } else {
                0 // TE_SPARKS
            };
            self.spawn_damage(te_type, point, normal);

            if let Some(ent) = self.entities.get_mut(target) {
                ent.game.health -= take;
            }
        }

        // Check death.
        let health = self
            .entities
            .get(target)
            .map(|e| e.game.health)
            .unwrap_or(0);

        if health <= 0 {
            // Disable knockback on dead entities.
            if is_monster
                || self
                    .entities
                    .get(target)
                    .map(|e| e.client.is_some())
                    .unwrap_or(false)
            {
                if let Some(ent) = self.entities.get_mut(target) {
                    ent.game.flags |= EntityFlags::NO_KNOCKBACK;
                }
            }

            self.killed(target, inflictor, attacker, take, point);
            return;
        }

        // Entity survived — call pain callback.
        let pain_fn = self.entities.get(target).and_then(|e| e.pain);
        if let Some(pain) = pain_fn {
            if take > 0 {
                pain(self, target, attacker, knockback, take);
            }
        }

        let _ = save; // tracked for HUD display in full implementation
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

            let (takes_damage, ent_origin, ent_mins, ent_maxs) = {
                let Some(ent) = self.entities.get(key) else {
                    continue;
                };
                if ent.game.takedamage == TakeDamage::No {
                    continue;
                }
                (true, ent.state.origin, ent.mins, ent.maxs)
            };
            let _ = takes_damage;

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
    fn can_damage_always_true_with_mock() {
        // MockGameImport traces return fraction=1.0 (no collision),
        // so can_damage should always return true.
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
            world.free_entity(k);
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
        let target = world.spawn().unwrap();

        // Give the entity client data with 100 armor.
        let mut client = crate::entity::ClientData::default();
        client.pers.inventory[1] = 100;
        world.entities.get_mut(target).unwrap().client = Some(client);

        let saved = world.check_armor(target, 50, DamageFlags::empty());
        // Body armor absorbs 80%: ceil(0.8 * 50) = 40
        assert_eq!(saved, 40);

        // Armor should be reduced.
        let armor = world.entities.get(target).unwrap().client.as_ref().unwrap().pers.inventory[1];
        assert_eq!(armor, 60); // 100 - 40
    }

    #[test]
    fn check_armor_capped_by_remaining() {
        let mut world = test_world();
        let target = world.spawn().unwrap();

        let mut client = crate::entity::ClientData::default();
        client.pers.inventory[1] = 10; // only 10 armor
        world.entities.get_mut(target).unwrap().client = Some(client);

        let saved = world.check_armor(target, 100, DamageFlags::empty());
        // Would save 80, but only 10 available.
        assert_eq!(saved, 10);
    }

    #[test]
    fn check_armor_no_armor_flag_bypasses() {
        let mut world = test_world();
        let target = world.spawn().unwrap();

        let mut client = crate::entity::ClientData::default();
        client.pers.inventory[1] = 100;
        world.entities.get_mut(target).unwrap().client = Some(client);

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
        use std::sync::atomic::{AtomicBool, Ordering};
        static DIED: AtomicBool = AtomicBool::new(false);

        fn test_die(
            _world: &mut GameWorld,
            _self_key: EntityKey,
            _inflictor: EntityKey,
            _attacker: EntityKey,
            _damage: i32,
            _point: Vec3f,
        ) {
            DIED.store(true, Ordering::Relaxed);
        }

        DIED.store(false, Ordering::Relaxed);

        let mut world = test_world();
        let target = world.spawn().unwrap();
        let attacker = world.spawn().unwrap();

        {
            let ent = world.entities.get_mut(target).unwrap();
            ent.game.health = 50;
            ent.game.takedamage = TakeDamage::Yes;
            ent.die = Some(test_die);
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

        assert!(DIED.load(Ordering::Relaxed));
        // Health should be negative.
        assert!(world.entities.get(target).unwrap().game.health <= 0);
    }

    #[test]
    fn t_damage_calls_pain_when_surviving() {
        use std::sync::atomic::{AtomicBool, Ordering};
        static PAINED: AtomicBool = AtomicBool::new(false);

        fn test_pain(
            _world: &mut GameWorld,
            _self_key: EntityKey,
            _attacker: EntityKey,
            _kick: f32,
            _damage: i32,
        ) {
            PAINED.store(true, Ordering::Relaxed);
        }

        PAINED.store(false, Ordering::Relaxed);

        let mut world = test_world();
        let target = world.spawn().unwrap();
        let attacker = world.spawn().unwrap();

        {
            let ent = world.entities.get_mut(target).unwrap();
            ent.game.health = 100;
            ent.game.takedamage = TakeDamage::Yes;
            ent.pain = Some(test_pain);
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

        assert!(PAINED.load(Ordering::Relaxed));
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
        let target = world.spawn().unwrap();
        let attacker = world.spawn().unwrap();

        {
            let ent = world.entities.get_mut(target).unwrap();
            ent.game.health = 100;
            ent.game.takedamage = TakeDamage::Yes;

            let mut client = crate::entity::ClientData::default();
            client.pers.inventory[1] = 100; // 100 armor
            ent.client = Some(client);
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
        assert_eq!(ent.client.as_ref().unwrap().pers.inventory[1], 60);
    }

    #[test]
    fn t_damage_surprise_bonus() {
        let mut world = test_world();
        let target = world.spawn().unwrap();
        let attacker = world.spawn().unwrap();

        {
            let ent = world.entities.get_mut(target).unwrap();
            ent.game.health = 200;
            ent.game.takedamage = TakeDamage::Yes;
            ent.svflags = SvFlags::MONSTER.bits(); // is a monster
            ent.game.enemy = None; // hasn't seen attacker
        }
        // Attacker is a "client" for surprise bonus.
        world.entities.get_mut(attacker).unwrap().client =
            Some(crate::entity::ClientData::default());

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
        }

        world.t_radius_damage(
            inflictor,
            inflictor,
            120.0,   // base damage
            None,     // no ignore
            200.0,    // radius
            MeansOfDeath::RocketSplash,
        );

        let health = world.entities.get(target).unwrap().game.health;
        // points = 120 - 0.5 * 50 = 95
        // health = 200 - 95 = 105
        assert!(health < 200);
        assert!(health > 0);
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
}
