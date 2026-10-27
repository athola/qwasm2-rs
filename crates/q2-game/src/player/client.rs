//! Player client management — connect, begin, disconnect, think, respawn.
//!
//! C ref: `player/client.c` (2,501 lines)

use std::rc::Rc;

use q2_common::pmove::Pmove;
use q2_shared::constants::{MASK_DEADSOLID, MASK_PLAYERSOLID};
use q2_shared::types::*;

use crate::constants::*;
use crate::entity::{ClientData, ClientPersistent, EntityKey};
use crate::world::GameWorld;

/// C ANGLE2SHORT.
fn angle2short(degrees: f32) -> i16 {
    ((degrees * 65536.0 / 360.0) as i32 & 65535) as i16
}

/// C SHORT2ANGLE.
fn short2angle(s: i16) -> f32 {
    f32::from(s) * (360.0 / 65536.0)
}

/// Maximum health for a player.
const PLAYER_MAX_HEALTH: i32 = 100;

/// Default player bounding box.
const PLAYER_MINS: Vec3f = Vec3f::new(-16.0, -16.0, -24.0);
const PLAYER_MAXS: Vec3f = Vec3f::new(16.0, 16.0, 32.0);

impl GameWorld {
    /// Handle a new client connection. Allocates a player entity.
    /// C ref: `ClientConnect` (player/client.c).
    pub fn connect_client(&mut self, number: usize, userinfo: &str) -> Option<EntityKey> {
        let key = self.entities.spawn_numbered(number)?;

        let ent = self.entities.get_mut(key)?;
        ent.game.classname = "player".to_string();
        ent.mins = PLAYER_MINS;
        ent.maxs = PLAYER_MAXS;
        ent.solid = Solid::Bbox;
        ent.clipmask = q2_shared::constants::MASK_PLAYERSOLID;
        ent.game.movetype = MoveType::Walk;
        ent.game.mass = 200;
        ent.game.takedamage = TakeDamage::Aim;

        let client = ClientData {
            pers: ClientPersistent {
                connected: true,
                health: PLAYER_MAX_HEALTH,
                max_health: PLAYER_MAX_HEALTH,
                netname: extract_info_value(userinfo, "name")
                    .unwrap_or_else(|| "player".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        ent.client = Some(client);
        ent.game.health = PLAYER_MAX_HEALTH;
        ent.game.max_health = PLAYER_MAX_HEALTH;

        Some(key)
    }

    /// Place a connected client into the game world at a spawn point.
    /// C ref: `PutClientInServer` (player/client.c), single-player part.
    pub fn begin_client(&mut self, key: EntityKey) {
        let (spawn_origin, spawn_angles) = self
            .select_spawn_point()
            .and_then(|k| self.entities.get(k))
            .map(|e| (e.state.origin + Vec3f::new(0.0, 0.0, 9.0), e.state.angles))
            .unwrap_or_else(|| {
                tracing::error!(spawnpoint = %self.game.spawnpoint, "Couldn't find spawn point");
                (Vec3f::ZERO, Vec3f::ZERO)
            });

        if let Some(ent) = self.entities.get_mut(key) {
            ent.in_use = true;
            ent.game.takedamage = TakeDamage::Aim;
            ent.game.movetype = MoveType::Walk;
            ent.game.viewheight = 22.0;
            ent.game.classname = "player".to_string();
            ent.game.mass = 200;
            ent.solid = Solid::Bbox;
            ent.clipmask = MASK_PLAYERSOLID;
            ent.mins = PLAYER_MINS;
            ent.maxs = PLAYER_MAXS;
            ent.velocity = Vec3f::ZERO;
            // 255: "use the skin specified model"; anything else makes
            // ClientThink treat the player as gibs.
            ent.state.modelindex = 255;
            ent.state.origin = spawn_origin + Vec3f::new(0.0, 0.0, 1.0); // off the ground
            ent.state.angles = Vec3f::new(0.0, spawn_angles.y, 0.0);

            if let Some(ref mut client) = ent.client {
                client.ps = PlayerState {
                    fov: 90.0,
                    viewangles: ent.state.angles,
                    ..Default::default()
                };
                for i in 0..3 {
                    client.ps.pmove.origin[i] = (spawn_origin[i] * 8.0) as i32 as i16;
                    client.ps.pmove.delta_angles[i] =
                        angle2short(spawn_angles[i] - client.resp.cmd_angles[i]);
                }
                // Give starting weapon (blaster).
                client.pers.inventory[7] = 1; // blaster at item index 7
            }
        }

        self.link_entity(key);
    }

    /// C SelectSpawnPoint, single-player part. With no `game.spawnpoint`
    /// the first start without a targetname is used, else any start, else
    /// (not C) a deathmatch start; with one, only the start whose targetname
    /// matches (case-insensitively). C raises gi.error when nothing matches;
    /// here the caller logs it.
    fn select_spawn_point(&self) -> Option<EntityKey> {
        let wanted = self.game.spawnpoint.as_str();
        // By entity number, the order C's G_Find walks g_edicts.
        let of_class = |class: &'static str| {
            (0..self.game.maxentities.max(0) as usize)
                .filter_map(|n| self.entities.key_for_number(n))
                .filter_map(|k| self.entities.get(k).map(|e| (k, e)))
                .filter(move |(_, e)| e.in_use && e.game.classname == class)
        };
        if !wanted.is_empty() {
            return of_class("info_player_start")
                .find(|(_, e)| e.game.targetname.eq_ignore_ascii_case(wanted))
                .map(|(k, _)| k);
        }
        of_class("info_player_start")
            .find(|(_, e)| e.game.targetname.is_empty())
            .or_else(|| of_class("info_player_start").next())
            // Not C: single player has no deathmatch fallback, but the
            // browser runs deathmatch maps without a deathmatch mode.
            .or_else(|| of_class("info_player_deathmatch").next())
            .map(|(k, _)| k)
    }

    /// Handle client disconnection. Free the player entity.
    /// C ref: `ClientDisconnect` (player/client.c).
    pub fn disconnect_client(&mut self, key: EntityKey) {
        if let Some(ent) = self.entities.get_mut(key) {
            if let Some(ref mut client) = ent.client {
                client.pers.connected = false;
            }
            ent.in_use = false;
            ent.solid = Solid::Not;
            ent.state.modelindex = 0;
            ent.game.classname = "disconnected".to_string();
        }
        // Unlinks; a client slot is never freed (C G_FreeEdict).
        self.free_entity(key);
    }

    /// Move the player by one user command with pmove, tracing through the
    /// server. C ref: `ClientThink` (player/client.c).
    ///
    /// Not yet ported (#12): pmove's touch list and G_TouchTriggers, and the
    /// identity of the ground entity (the Rust pmove only reports whether
    /// there is ground, so it is recorded as the world).
    pub fn think_client(&mut self, key: EntityKey, cmd: &UserCmd) {
        let Some(number) = self.number_of(key) else {
            return;
        };
        let (state, snap_initial, mask) = {
            let Some(ent) = self.entities.get_mut(key) else {
                return;
            };
            let Some(client) = ent.client.as_mut() else {
                return;
            };
            client.ps.pmove.pm_type = if ent.game.movetype == MoveType::Noclip {
                PmType::Spectator
            } else if ent.state.modelindex != 255 {
                PmType::Gib
            } else if ent.game.deadflag != DeadFlag::No {
                PmType::Dead
            } else {
                PmType::Normal
            };
            client.ps.pmove.gravity = DEFAULT_GRAVITY as i16; // sv_gravity

            let mut state = client.ps.pmove;
            for i in 0..3 {
                // Through i32 so an overflowing short wraps as in C.
                state.origin[i] = (ent.state.origin[i] * 8.0) as i32 as i16;
                state.velocity[i] = (ent.velocity[i] * 8.0) as i32 as i16;
            }
            let mask = if ent.game.health > 0 {
                MASK_PLAYERSOLID
            } else {
                MASK_DEADSOLID
            };
            (state, state != client.old_pmove, mask)
        };

        let (gi_trace, gi_contents) = (Rc::clone(&self.gi), Rc::clone(&self.gi));
        let mut pm = Pmove {
            s: state,
            cmd: *cmd,
            snap_initial,
            viewangles: Vec3f::ZERO,
            viewheight: 0.0,
            mins: Vec3f::ZERO,
            maxs: Vec3f::ZERO,
            ground_entity: false,
            watertype: 0,
            waterlevel: 0,
            airaccelerate: 0.0,
            trace: Box::new(move |start, mins, maxs, end| {
                gi_trace.trace(start, mins, maxs, end, Some(number), mask)
            }),
            pointcontents: Box::new(move |point| gi_contents.point_contents(point)),
        };
        pm.run();

        let world = self.entities.key_for_number(0);
        let Some(ent) = self.entities.get_mut(key) else {
            return;
        };
        for i in 0..3 {
            ent.state.origin[i] = f32::from(pm.s.origin[i]) * 0.125;
            ent.velocity[i] = f32::from(pm.s.velocity[i]) * 0.125;
        }
        ent.mins = pm.mins;
        ent.maxs = pm.maxs;
        ent.game.viewheight = pm.viewheight;
        ent.game.waterlevel = pm.waterlevel;
        ent.game.watertype = pm.watertype;
        ent.game.ground_entity = if pm.ground_entity { world } else { None };
        let dead = ent.game.deadflag != DeadFlag::No;
        if let Some(client) = ent.client.as_mut() {
            client.ps.pmove = pm.s;
            client.old_pmove = pm.s;
            for i in 0..3 {
                client.resp.cmd_angles[i] = short2angle(cmd.angles[i]);
            }
            if dead {
                client.ps.viewangles.x = -15.0;
                client.ps.viewangles.z = 40.0;
            } else {
                client.ps.viewangles = pm.viewangles;
            }
        }

        self.link_entity(key);
    }

    /// Per-frame setup for a client at the start of the server frame.
    /// C ref: `ClientBeginServerFrame` (player/client.c).
    pub fn client_begin_server_frame(&mut self, _key: EntityKey) {
        // Reset per-frame state (damage counters, etc.).
        // Simplified — full impl resets damage tracking and weapon state.
    }

    /// Per-frame cleanup for all clients at the end of the server frame.
    /// C ref: `ClientEndServerFrames` (player/client.c).
    pub fn client_end_server_frames(&mut self) {
        let keys: Vec<EntityKey> = self
            .entities
            .iter()
            .filter(|(_, e)| e.client.is_some() && e.in_use)
            .map(|(k, _)| k)
            .collect();

        for key in keys {
            self.update_player_view(key);
            self.update_player_stats(key);
        }
    }
}

/// Extract a value from a Quake-style userinfo string ("\\key\\value\\key\\value").
fn extract_info_value(userinfo: &str, key: &str) -> Option<String> {
    let parts: Vec<&str> = userinfo.split('\\').filter(|s| !s.is_empty()).collect();
    for chunk in parts.chunks(2) {
        if chunk.len() == 2 && chunk[0] == key {
            return Some(chunk[1].to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::test_world;

    #[test]
    fn client_connect_creates_player() {
        let mut world = test_world();
        let key = world.connect_client(1, "\\name\\TestPlayer").unwrap();

        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.classname, "player");
        assert_eq!(ent.game.health, PLAYER_MAX_HEALTH);
        assert!(ent.client.is_some());
        assert_eq!(ent.client.as_ref().unwrap().pers.netname, "TestPlayer");
        assert!(ent.client.as_ref().unwrap().pers.connected);
    }

    #[test]
    fn client_begin_places_at_spawn() {
        let mut world = test_world();

        // Create a spawn point.
        let spawn = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(spawn).unwrap();
            ent.game.classname = "info_player_start".to_string();
            ent.state.origin = Vec3f::new(100.0, 200.0, 0.0);
        }

        let key = world.connect_client(1, "\\name\\P1").unwrap();
        world.begin_client(key);

        let pos = world.entities.get(key).unwrap().state.origin;
        assert!((pos.x - 100.0).abs() < 0.01);
        assert!((pos.y - 200.0).abs() < 0.01);
    }

    #[test]
    fn client_begin_gives_blaster() {
        let mut world = test_world();
        let key = world.connect_client(1, "\\name\\P1").unwrap();
        world.begin_client(key);

        let inv = world
            .entities
            .get(key)
            .unwrap()
            .client
            .as_ref()
            .unwrap()
            .pers
            .inventory;
        assert_eq!(inv[7], 1); // blaster
    }

    #[test]
    fn client_disconnect_frees_entity() {
        let mut world = test_world();
        let key = world.connect_client(1, "\\name\\P1").unwrap();

        assert_eq!(world.entities.count(), 1);
        world.disconnect_client(key);
        // C keeps client edicts allocated; the slot is just no longer in use.
        assert!(!world.entities.get(key).unwrap().in_use);
    }

    #[test]
    fn begin_client_uses_the_untargeted_start_like_select_spawn_point() {
        // demo1 lists a "base2" arrival start first; with no spawnpoint C
        // skips starts that have a targetname.
        let mut world = test_world();
        world.init_game();
        world.spawn_entities(
            "demo1",
            concat!(
                "{ \"classname\" \"worldspawn\" }\n",
                "{ \"classname\" \"info_player_start\" \"targetname\" \"base2\" \"origin\" \"-1768 1536 128\" }\n",
                "{ \"classname\" \"info_player_start\" \"angle\" \"135\" \"origin\" \"128 -320 32\" }",
            ),
            "",
        );
        let key = world.connect_client(1, "").unwrap();
        world.begin_client(key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.state.origin, Vec3f::new(128.0, -320.0, 42.0));
        assert_eq!(ent.state.angles.y, 135.0);

        // With a spawnpoint, only the start it names is used.
        world.game.spawnpoint = "BASE2".to_string(); // compared case-insensitively
        world.disconnect_client(key);
        let key = world.connect_client(1, "").unwrap();
        world.begin_client(key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.state.origin, Vec3f::new(-1768.0, 1536.0, 138.0));
    }

    #[test]
    fn begin_client_falls_back_to_a_deathmatch_start_on_maps_without_player_starts() {
        // Deviation kept from the old browser spawn: C single player raises
        // gi.error here, but the browser has no deathmatch mode to switch to.
        let mut world = test_world();
        world.init_game();
        world.spawn_entities(
            "q2dm1",
            concat!(
                "{ \"classname\" \"worldspawn\" }\n",
                "{ \"classname\" \"info_player_deathmatch\" \"origin\" \"64 64 16\" }",
            ),
            "",
        );
        let key = world.connect_client(1, "").unwrap();
        world.begin_client(key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.state.origin, Vec3f::new(64.0, 64.0, 26.0));
    }

    #[test]
    fn think_client_runs_pmove_with_c_gravity_and_records_the_result() {
        // The mock's traces never hit, so the player is airborne: pmove must
        // apply sv_gravity (800) and the entity follows pm.s back out.
        let mut world = test_world();
        let key = world.connect_client(1, "\\name\\P1").unwrap();
        world.begin_client(key);
        let cmd = UserCmd {
            msec: 100,
            ..Default::default()
        };

        world.think_client(key, &cmd);

        let ent = world.entities.get(key).unwrap();
        let client = ent.client.as_ref().unwrap();
        assert_eq!(client.ps.pmove.pm_type, PmType::Normal);
        assert_eq!(client.ps.pmove.gravity, 800);
        assert_eq!(client.old_pmove, client.ps.pmove);
        assert!(ent.velocity.z < 0.0, "falling: {:?}", ent.velocity);
        assert_eq!(
            ent.state.origin.z,
            f32::from(client.ps.pmove.origin[2]) * 0.125
        );
    }

    #[test]
    fn think_client_treats_a_player_without_model_255_as_gibs_like_c() {
        let mut world = test_world();
        let key = world.connect_client(1, "\\name\\P1").unwrap();
        world.think_client(key, &UserCmd::default());
        let client = world.entities.get(key).unwrap().client.clone().unwrap();
        assert_eq!(client.ps.pmove.pm_type, PmType::Gib);
    }

    #[test]
    fn extract_info_value_works() {
        assert_eq!(
            extract_info_value("\\name\\Player1\\skin\\male/grunt", "name"),
            Some("Player1".to_string())
        );
        assert_eq!(
            extract_info_value("\\name\\Player1\\skin\\male/grunt", "skin"),
            Some("male/grunt".to_string())
        );
        assert_eq!(extract_info_value("\\name\\Player1", "missing"), None);
    }
}
