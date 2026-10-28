//! Deathmatch rules: ending the match on the frag or time limit and
//! picking the next map, and which players are on a team. C ref:
//! `CheckDMRules`, `EndDMLevel` (g_main.c), `CreateTargetChangeLevel`
//! (g_utils.c), `ClientTeam`, `OnSameTeam` (g_cmds.c). The scoreboard
//! layout is not ported.

use q2_shared::constants::PRINT_HIGH;

use crate::constants::{DF_MODELTEAMS, DF_SAME_LEVEL, DF_SKINTEAMS};
use crate::entity::EntityKey;
use crate::world::GameWorld;

impl GameWorld {
    /// End a deathmatch on the time limit (minutes) or when a player
    /// reaches the frag limit. C ref: `CheckDMRules` (g_main.c).
    pub fn check_dm_rules(&mut self) {
        if self.level.intermission_time != 0.0 || !self.game.deathmatch {
            return;
        }
        if self.game.timelimit != 0.0 && self.level.time >= self.game.timelimit * 60.0 {
            self.gi.bprintf(PRINT_HIGH, "Timelimit hit.\n");
            self.end_dm_level();
            return;
        }
        if self.game.fraglimit != 0 {
            let reached = (1..=self.game.maxclients.max(0) as usize)
                .filter_map(|n| self.entities.key_for_number(n))
                .filter(|&k| self.entities.get(k).is_some_and(|e| e.in_use))
                .any(|k| {
                    self.client(k)
                        .is_some_and(|c| c.resp.score >= self.game.fraglimit)
                });
            if reached {
                self.gi.bprintf(PRINT_HIGH, "Fraglimit hit.\n");
                self.end_dm_level();
            }
        }
    }

    /// Go to the next deathmatch map: this one again with `DF_SAME_LEVEL`,
    /// the one after it in `maplist` (wrapping round), the map's
    /// `nextmap`, its exit, or this one again. C ref: `EndDMLevel`.
    pub fn end_dm_level(&mut self) {
        let mapname = self.level.mapname.clone();
        let next = if self.game.dmflags & DF_SAME_LEVEL != 0 {
            Some(mapname.clone())
        } else {
            let list: Vec<&str> = self
                .game
                .maplist
                .split([' ', ',', '\n', '\r'])
                .filter(|t| !t.is_empty())
                .collect();
            list.iter()
                .position(|t| t.eq_ignore_ascii_case(&mapname))
                .map(|i| list.get(i + 1).unwrap_or(&list[0]).to_string())
        };
        let next =
            next.or_else(|| (!self.level.nextmap.is_empty()).then(|| self.level.nextmap.clone()));
        let exit = match next {
            Some(map) => self.create_target_changelevel(&map),
            None => self
                .find_by_classname(None, "target_changelevel")
                // The map designer didn't include a changelevel.
                .or_else(|| self.create_target_changelevel(&mapname)),
        };
        if let Some(exit) = exit {
            self.begin_intermission(exit);
        }
    }

    /// A changelevel to `map` made on the fly. C ref:
    /// `CreateTargetChangeLevel` (g_utils.c).
    pub fn create_target_changelevel(&mut self, map: &str) -> Option<EntityKey> {
        let key = self.spawn()?;
        let ent = self.entities.get_mut(key)?;
        ent.game.classname = "target_changelevel".into();
        ent.game.map = map.to_string();
        Some(key)
    }

    /// A player's team: the skin after the model with `DF_SKINTEAMS`,
    /// the model with `DF_MODELTEAMS`. C ref: `ClientTeam` (g_cmds.c).
    fn client_team(&self, key: EntityKey) -> String {
        let Some(client) = self.client(key) else {
            return String::new();
        };
        let skin = crate::player::client::extract_info_value(&client.pers.userinfo, "skin")
            .unwrap_or_default();
        match skin.split_once('/') {
            None => skin,
            Some((model, _)) if self.game.dmflags & DF_MODELTEAMS != 0 => model.to_string(),
            Some((_, skin)) => skin.to_string(),
        }
    }

    /// Whether two players are teammates under the team dmflags. C ref:
    /// `OnSameTeam` (g_cmds.c).
    pub fn on_same_team(&self, a: EntityKey, b: EntityKey) -> bool {
        if self.game.dmflags & (DF_MODELTEAMS | DF_SKINTEAMS) == 0 {
            return false;
        }
        let team = self.client_team(a);
        !team.is_empty() && team == self.client_team(b)
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use q2_shared::types::Vec3f;

    use crate::constants::*;
    use crate::entity::EntityKey;
    use crate::world::{test_world_recording, GameWorld, MockGameImport};

    fn match_world() -> (GameWorld, EntityKey, Rc<MockGameImport>) {
        let (mut world, gi) = test_world_recording();
        world.init_items();
        world.set_max_clients(2);
        world.game.deathmatch = true;
        world.level.mapname = "q2dm1".into();
        let start = world.spawn().unwrap();
        world.entities.get_mut(start).unwrap().game.classname = "info_player_deathmatch".into();
        let key = world
            .connect_client(1, "\\name\\P1\\skin\\male/grunt")
            .unwrap();
        world.begin_client(key);
        (world, key, gi)
    }

    fn printed(gi: &MockGameImport) -> Vec<String> {
        gi.bprints.borrow().iter().map(|(_, m)| m.clone()).collect()
    }

    #[test]
    fn the_time_limit_ends_the_match() {
        let (mut world, _, gi) = match_world();
        world.game.timelimit = 10.0;
        world.level.time = 599.0;
        world.check_dm_rules();
        assert_eq!(world.level.intermission_time, 0.0);
        world.level.time = 600.0;
        world.check_dm_rules();
        assert_eq!(printed(&gi), ["Timelimit hit.\n"]);
        assert_eq!(world.level.intermission_time, 600.0);
        assert_eq!(world.level.changemap, "q2dm1", "no exit or list: same map");
    }

    #[test]
    fn the_frag_limit_ends_the_match() {
        let (mut world, key, gi) = match_world();
        world.game.fraglimit = 5;
        world.client_mut(key).unwrap().resp.score = 4;
        world.check_dm_rules();
        assert!(printed(&gi).is_empty());
        world.client_mut(key).unwrap().resp.score = 5;
        world.check_dm_rules();
        assert_eq!(printed(&gi), ["Fraglimit hit.\n"]);
    }

    #[test]
    fn rules_wait_out_an_intermission_and_skip_single_player() {
        let (mut world, key, gi) = match_world();
        world.game.fraglimit = 1;
        world.client_mut(key).unwrap().resp.score = 3;
        world.level.intermission_time = 1.0;
        world.check_dm_rules();
        world.level.intermission_time = 0.0;
        world.game.deathmatch = false;
        world.check_dm_rules();
        assert!(printed(&gi).is_empty());
    }

    fn end_level_map(world: &mut GameWorld) -> String {
        world.end_dm_level();
        world.level.changemap.clone()
    }

    #[test]
    fn the_map_list_goes_to_the_next_map_and_wraps() {
        let (mut world, _, _) = match_world();
        world.game.maplist = "q2dm1 q2dm2,q2dm3".into();
        assert_eq!(end_level_map(&mut world), "q2dm2");

        let (mut world, _, _) = match_world();
        world.level.mapname = "q2dm3".into();
        world.game.maplist = "q2dm1 q2dm2 q2dm3".into();
        assert_eq!(end_level_map(&mut world), "q2dm1", "wraps to the first");

        let (mut world, _, _) = match_world();
        world.game.maplist = "q2dm7 q2dm8".into();
        world.level.nextmap = "base1".into();
        assert_eq!(
            end_level_map(&mut world),
            "base1",
            "not in the list: nextmap"
        );
    }

    #[test]
    fn same_level_dmflag_and_a_map_exit_choose_the_next_map() {
        let (mut world, _, _) = match_world();
        world.game.dmflags = DF_SAME_LEVEL;
        world.game.maplist = "q2dm1 q2dm2".into();
        assert_eq!(end_level_map(&mut world), "q2dm1");

        let (mut world, _, _) = match_world();
        let exit = world.spawn().unwrap();
        let e = world.entities.get_mut(exit).unwrap();
        e.game.classname = "target_changelevel".into();
        e.game.map = "q2dm4".into();
        assert_eq!(end_level_map(&mut world), "q2dm4");
    }

    fn rival(world: &mut GameWorld, skin: &str) -> EntityKey {
        let key = world
            .connect_client(2, &format!("\\name\\P2\\skin\\{skin}"))
            .unwrap();
        world.begin_client(key);
        key
    }

    #[test]
    fn teams_go_by_skin_or_model_with_the_dmflag() {
        let (mut world, me, _) = match_world();
        let other = rival(&mut world, "female/grunt");
        assert!(!world.on_same_team(me, other), "no team flag");
        world.game.dmflags = DF_SKINTEAMS;
        assert!(world.on_same_team(me, other), "same skin");
        world.game.dmflags = DF_MODELTEAMS;
        assert!(!world.on_same_team(me, other), "different model");
    }

    #[test]
    fn no_friendly_fire_spares_teammates_but_not_yourself() {
        let (mut world, me, _) = match_world();
        let other = rival(&mut world, "male/grunt");
        world.game.dmflags = DF_MODELTEAMS | DF_NO_FRIENDLY_FIRE;
        let hit = |world: &mut GameWorld, target, attacker| {
            world.t_damage(
                target,
                attacker,
                attacker,
                Vec3f::X,
                Vec3f::ZERO,
                Vec3f::ZERO,
                20,
                0,
                DamageFlags::empty(),
                MeansOfDeath::Shotgun,
            );
            world.entities.get(target).unwrap().game.health
        };
        assert_eq!(hit(&mut world, other, me), 100);
        assert_eq!(hit(&mut world, me, me), 80, "self damage stands");
        world.game.dmflags = DF_MODELTEAMS;
        assert_eq!(hit(&mut world, other, me), 80, "friendly fire allowed");
    }
}
