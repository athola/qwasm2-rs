//! Player client management — connect, begin, disconnect, think, respawn.
//!
//! C ref: `player/client.c` (2,501 lines)

use std::rc::Rc;

use q2_common::pmove::Pmove;
use q2_shared::constants::{BUTTON_ATTACK, MASK_DEADSOLID, MASK_PLAYERSOLID};
use q2_shared::types::*;

use crate::constants::*;
use crate::entity::{ClientData, ClientPersistent, ClientRespawn, EntityKey};
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

/// C `MZ_LOGIN` (shared.h): the muzzle flash of a player entering the game.
const MZ_LOGIN: i32 = 9;
/// C `DF_SPAWN_FARTHEST` (shared.h) dmflags bit.
const DF_SPAWN_FARTHEST: i32 = 0x0000_0200;

impl GameWorld {
    /// Handle a new client connection. Allocates a player entity.
    /// C ref: `ClientConnect` (player/client.c).
    pub fn connect_client(&mut self, number: usize, userinfo: &str) -> Option<EntityKey> {
        let slot = number.checked_sub(1).filter(|&i| i < self.clients.len())?;
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

        // The slot is not in use (a fresh body), so C clears the respawn
        // data and, with no autosave to keep, the persistent data.
        let mut client = ClientData::default();
        self.init_client_resp(&mut client);
        self.init_client_persistant(&mut client.pers);
        client_userinfo_changed(&mut client.pers, userinfo);
        if self.game.maxclients() > 1 {
            self.gi
                .dprintf(&format!("{} connected\n", client.pers.netname));
        }
        self.clients[slot] = client;
        let ent = self.entities.get_mut(key)?;
        ent.client = Some(slot);
        ent.die = Some(player_die);
        ent.game.health = PLAYER_MAX_HEALTH;
        ent.game.max_health = PLAYER_MAX_HEALTH;
        self.player_skin_configstring(key);

        Some(key)
    }

    /// Place a connected client into the game world at a spawn point.
    /// C ref: `PutClientInServer` (player/client.c). Not ported: the
    /// help-computer counts coop carries across a respawn, and
    /// `air_finished` and `pain`, which only player drowning (P_WorldEffects)
    /// and player_pain read, neither of which is ported.
    pub fn begin_client(&mut self, key: EntityKey) {
        // Before health is set back up, so farthest ranging does not count
        // this client.
        let (spawn_origin, spawn_angles) = self
            .select_spawn_point(key)
            .and_then(|k| self.entities.get(k))
            .map(|e| (e.state.origin + Vec3f::new(0.0, 0.0, 9.0), e.state.angles))
            .unwrap_or_else(|| {
                tracing::error!(spawnpoint = %self.game.spawnpoint, "Couldn't find spawn point");
                (Vec3f::ZERO, Vec3f::ZERO)
            });

        // Clear everything but the persistent data, which starts over only
        // for a client that died (or never had any). Deathmatch restarts it
        // every spawn and coop goes back to what the level started with;
        // both keep the respawn data (the score), single player clears it.
        if let Some(slot) = self.entities.get(key).and_then(|e| e.client) {
            let mut pers = std::mem::take(&mut self.clients[slot].pers);
            let kept = std::mem::take(&mut self.clients[slot].resp);
            let resp = if self.game.deathmatch {
                let userinfo = pers.userinfo.clone();
                self.init_client_persistant(&mut pers);
                client_userinfo_changed(&mut pers, &userinfo);
                kept
            } else if self.game.coop {
                let userinfo = std::mem::take(&mut pers.userinfo);
                pers = kept.coop_respawn.clone();
                client_userinfo_changed(&mut pers, &userinfo);
                pers.score = pers.score.max(kept.score);
                kept
            } else {
                ClientRespawn::default()
            };
            let userinfo = pers.userinfo.clone();
            client_userinfo_changed(&mut pers, &userinfo);
            if pers.health <= 0 {
                self.init_client_persistant(&mut pers);
            }
            self.clients[slot] = ClientData {
                pers,
                resp,
                ..Default::default()
            };
        }
        self.fetch_client_ent_data(key);

        let number = self.number_of(key).unwrap_or(1);
        if let Some(ent) = self.entities.get_mut(key) {
            ent.game.ground_entity = None;
            ent.in_use = true;
            ent.game.takedamage = TakeDamage::Aim;
            ent.game.deadflag = DeadFlag::No;
            ent.game.waterlevel = 0;
            ent.game.watertype = 0;
            ent.game.flags.remove(EntityFlags::NO_KNOCKBACK);
            ent.svflags = 0;
            ent.die = Some(player_die);
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
            ent.state.effects = 0;
            ent.state.modelindex = 255;
            ent.state.modelindex2 = 255; // custom gun model
                                         // The skin is the player number; the weapon is added later.
            ent.state.skinnum = number as i32 - 1;
            ent.state.frame = 0;
            ent.state.origin = spawn_origin + Vec3f::new(0.0, 0.0, 1.0); // off the ground
            ent.state.old_origin = ent.state.origin;
            ent.state.angles = Vec3f::new(0.0, spawn_angles.y, 0.0);

            if let Some(client) = ent.client.and_then(|slot| self.clients.get_mut(slot)) {
                client.ps = PlayerState {
                    fov: 90.0,
                    viewangles: ent.state.angles,
                    ..Default::default()
                };
                client.v_angle = ent.state.angles;
                client.air_finished = self.level.time + 12.0;
                for i in 0..3 {
                    client.ps.pmove.origin[i] = (spawn_origin[i] * 8.0) as i32 as i16;
                    client.ps.pmove.delta_angles[i] =
                        angle2short(spawn_angles[i] - client.resp.cmd_angles[i]);
                }
            }
        }

        // C ignores whether something survived.
        self.kill_box(key);
        self.link_entity(key);

        self.player_skin_configstring(key);

        // Force the current weapon up.
        if let Some(client) = self.client_mut(key) {
            client.newweapon = client.pers.weapon;
        }
        self.change_weapon(key);
    }

    /// Starting persistent data: the Blaster, 100 health and the default
    /// ammo caps. C ref: `InitClientPersistant` (player/client.c).
    pub fn init_client_persistant(&self, pers: &mut ClientPersistent) {
        *pers = ClientPersistent::default();
        let blaster = self.find_item_by_name("Blaster");
        if let Some(blaster) = blaster {
            pers.selected_item = blaster as i32;
            pers.inventory[blaster] = 1;
        }
        pers.weapon = blaster;
        pers.health = PLAYER_MAX_HEALTH;
        pers.max_health = PLAYER_MAX_HEALTH;
        pers.ammo_max = crate::items::AMMO_MAX_DEFAULT;
        pers.connected = true;
    }

    /// C ref: `InitClientResp` (player/client.c).
    pub fn init_client_resp(&self, client: &mut ClientData) {
        client.resp = ClientRespawn {
            enter_frame: self.level.framenum,
            coop_respawn: client.pers.clone(),
            ..Default::default()
        };
    }

    /// Copy what the next level keeps from each in-use client edict into its
    /// persistent data; in coop that includes the score.
    /// C ref: `SaveClientData` (player/client.c).
    pub fn save_client_data(&mut self) {
        let coop = self.game.coop;
        for (i, client) in self.clients.iter_mut().enumerate() {
            let Some(ent) = self
                .entities
                .key_for_number(i + 1)
                .and_then(|k| self.entities.get(k))
                .filter(|e| e.in_use)
            else {
                continue;
            };
            client.pers.health = ent.game.health;
            client.pers.max_health = ent.game.max_health;
            client.pers.saved_flags = ent.game.flags & SAVED_FLAGS;
            if coop {
                client.pers.score = client.resp.score;
            }
        }
    }

    /// The reverse of `save_client_data`, onto a client's new body.
    /// C ref: `FetchClientEntData` (player/client.c).
    fn fetch_client_ent_data(&mut self, key: EntityKey) {
        let coop = self.game.coop;
        let Some((ent, client)) = self.entity_and_client_mut(key) else {
            return;
        };
        ent.game.health = client.pers.health;
        ent.game.max_health = client.pers.max_health;
        ent.game.flags |= client.pers.saved_flags;
        if coop {
            client.resp.score = client.pers.score;
        }
    }

    /// Every in-use entity of `class`, in the order C's G_Find walks them.
    fn spots(&self, class: &str) -> Vec<EntityKey> {
        std::iter::successors(self.find_by_classname(None, class), |&k| {
            self.find_by_classname(Some(k), class)
        })
        .collect()
    }

    fn spot_origin(&self, key: EntityKey) -> Vec3f {
        self.entities
            .get(key)
            .map_or(Vec3f::ZERO, |e| e.state.origin)
    }

    /// Spawn point for the client `key`: a deathmatch or coop start in those
    /// modes, else the single-player start. With no `game.spawnpoint` the
    /// first start without a targetname is used, else any start, else (not
    /// C) a deathmatch start; with one, only the start whose targetname
    /// matches (case-insensitively). C raises gi.error when nothing
    /// matches; here the caller logs it.
    /// C ref: `SelectSpawnPoint` (player/client.c).
    fn select_spawn_point(&mut self, key: EntityKey) -> Option<EntityKey> {
        let index = self.entities.get(key).and_then(|e| e.client);
        let spot = if self.game.deathmatch {
            self.select_deathmatch_spawn_point()
        } else if self.game.coop {
            index.and_then(|i| self.select_coop_spawn_point(i))
        } else {
            None
        };
        let mut spot = spot.or_else(|| self.select_player_start())?;

        // Coop on a map whose coop starts don't match the spawnpoint (map
        // bugs, or loaded from the console): take one within 550 units.
        let index = index.unwrap_or(0);
        if self.game.coop
            && index != 0
            && self
                .entities
                .get(spot)
                .is_some_and(|e| e.game.classname.eq_ignore_ascii_case("info_player_start"))
        {
            let origin = self.spot_origin(spot);
            let mut counter = 0;
            for coopspot in self.spots("info_player_coop") {
                if counter >= 3 {
                    break;
                }
                if (self.spot_origin(coopspot) - origin).length() < 550.0 {
                    if index == counter {
                        spot = coopspot;
                        break;
                    }
                    counter += 1;
                }
            }
        }
        Some(spot)
    }

    /// The single-player part of C SelectSpawnPoint.
    fn select_player_start(&self) -> Option<EntityKey> {
        let wanted = self.game.spawnpoint.as_str();
        let starts = self.spots("info_player_start");
        let start = |k: &EntityKey| self.entities.get(*k).map(|e| &e.game);
        if !wanted.is_empty() {
            return starts
                .into_iter()
                .find(|k| start(k).is_some_and(|g| g.targetname.eq_ignore_ascii_case(wanted)));
        }
        starts
            .iter()
            .copied()
            .find(|k| start(k).is_some_and(|g| g.targetname.is_empty()))
            .or_else(|| starts.first().copied())
            // Not C: single player has no deathmatch fallback, but the
            // browser runs deathmatch maps without a deathmatch mode.
            .or_else(|| self.spots("info_player_deathmatch").first().copied())
    }

    /// Distance from `spot` to the nearest live player.
    /// C ref: `PlayersRangeFromSpot` (player/client.c).
    fn players_range_from_spot(&self, spot: EntityKey) -> f32 {
        let origin = self.spot_origin(spot);
        (1..=self.game.maxclients().max(0) as usize)
            .filter_map(|n| self.entities.key_for_number(n))
            .filter_map(|k| self.entities.get(k))
            .filter(|p| p.in_use && p.game.health > 0)
            .map(|p| (origin - p.state.origin).length())
            .fold(9_999_999.0, f32::min)
    }

    /// C ref: `SelectDeathmatchSpawnPoint` (player/client.c).
    fn select_deathmatch_spawn_point(&mut self) -> Option<EntityKey> {
        if self.game.dmflags & DF_SPAWN_FARTHEST != 0 {
            self.select_farthest_deathmatch_spawn_point()
        } else {
            self.select_random_deathmatch_spawn_point()
        }
    }

    /// A random deathmatch start, but not the two nearest other players.
    /// C ref: `SelectRandomDeathmatchSpawnPoint` (player/client.c).
    fn select_random_deathmatch_spawn_point(&mut self) -> Option<EntityKey> {
        let spots = self.spots("info_player_deathmatch");
        let (mut range1, mut range2) = (99_999.0, 99_999.0);
        let (mut spot1, mut spot2) = (None, None);
        for &spot in &spots {
            let range = self.players_range_from_spot(spot);
            if range < range1 {
                range1 = range;
                spot1 = Some(spot);
            } else if range < range2 {
                range2 = range;
                spot2 = Some(spot);
            }
        }
        if spots.is_empty() {
            return None;
        }
        if spots.len() <= 2 {
            (spot1, spot2) = (None, None);
        }
        let count = spots.len() - usize::from(spot1.is_some()) - usize::from(spot2.is_some());
        let selection = self.random_u32() as usize % count;
        spots
            .into_iter()
            .filter(|&s| Some(s) != spot1 && Some(s) != spot2)
            .nth(selection)
    }

    /// The deathmatch start farthest from any live player.
    /// C ref: `SelectFarthestDeathmatchSpawnPoint` (player/client.c).
    fn select_farthest_deathmatch_spawn_point(&self) -> Option<EntityKey> {
        let spots = self.spots("info_player_deathmatch");
        let mut best = None;
        let mut best_distance = 0.0;
        for &spot in &spots {
            let distance = self.players_range_from_spot(spot);
            if distance > best_distance {
                best = Some(spot);
                best_distance = distance;
            }
        }
        // A player on every start: one of them gets telefragged.
        best.or_else(|| spots.first().copied())
    }

    /// The coop start for client slot `index` at `game.spawnpoint`; slot 0
    /// uses the normal player start. C assumes four coop starts per spawn.
    /// C ref: `SelectCoopSpawnPoint` (player/client.c).
    fn select_coop_spawn_point(&self, index: usize) -> Option<EntityKey> {
        if index == 0 {
            return None;
        }
        self.spots("info_player_coop")
            .into_iter()
            .filter(|&k| {
                self.entities.get(k).is_some_and(|e| {
                    e.game
                        .targetname
                        .eq_ignore_ascii_case(&self.game.spawnpoint)
                })
            })
            .nth(index - 1)
    }

    /// Reserve the body queue edicts right after the clients in coop and
    /// deathmatch. C ref: `InitBodyQue` (player/client.c).
    pub(crate) fn init_body_que(&mut self) {
        if !(self.game.deathmatch || self.game.coop) {
            return;
        }
        self.level.body_que = 0;
        for _ in 0..BODY_QUEUE_SIZE {
            if let Some(ent) = self.spawn().and_then(|k| self.entities.get_mut(k)) {
                ent.game.classname = "bodyque".to_string();
            }
        }
    }

    /// Leave a copy of the player's corpse in the next body queue slot.
    /// C ref: `CopyToBodyQue` (player/client.c).
    fn copy_to_body_que(&mut self, key: EntityKey) {
        let number = self.game.maxclients() + self.level.body_que + 1;
        self.level.body_que = (self.level.body_que + 1) % BODY_QUEUE_SIZE as i32;
        let Some(body) = usize::try_from(number)
            .ok()
            .and_then(|n| self.entities.key_for_number(n))
        else {
            return;
        };
        let Some(ent) = self.entities.get(key).cloned() else {
            return;
        };
        if let Some(n) = self.number_of(key) {
            self.gi.unlink_entity(n);
        }
        self.gi.unlink_entity(number as usize);
        let Some(b) = self.entities.get_mut(body) else {
            return;
        };
        b.state = ent.state;
        b.state.number = number;
        b.svflags = ent.svflags;
        b.mins = ent.mins;
        b.maxs = ent.maxs;
        b.absmin = ent.absmin;
        b.absmax = ent.absmax;
        b.size = ent.size;
        b.solid = ent.solid;
        b.clipmask = ent.clipmask;
        b.owner = ent.owner;
        b.game.movetype = ent.game.movetype;
        b.die = Some(body_die);
        b.game.takedamage = TakeDamage::Yes;
        self.link_entity(body);
    }

    /// Put a dead player back in: deathmatch and coop leave the corpse in
    /// the body queue and respawn at a start with a teleport effect; single
    /// player asks for the load menu. C ref: `respawn` (player/client.c).
    pub fn respawn(&mut self, key: EntityKey) {
        if !(self.game.deathmatch || self.game.coop) {
            // Restart the entire server.
            self.gi.add_command_string("menu_loadgame\n");
            return;
        }
        // Spectators don't leave bodies.
        let noclip = self
            .entities
            .get(key)
            .is_some_and(|e| e.game.movetype == MoveType::Noclip);
        if !noclip {
            self.copy_to_body_que(key);
        }
        if let Some(ent) = self.entities.get_mut(key) {
            ent.svflags &= !q2_shared::constants::SVF_NOCLIENT;
        }
        self.begin_client(key);
        let time = self.level.time;
        if let Some((ent, client)) = self.entity_and_client_mut(key) {
            // Teleport effect, held in place briefly.
            ent.state.event = q2_shared::constants::EV_PLAYER_TELEPORT;
            client.ps.pmove.pm_flags = q2_common::pmove::PMF_TIME_TELEPORT;
            client.ps.pmove.pm_time = 14;
            client.respawn_time = time;
        }
    }

    /// The muzzle flash a player shows entering the game.
    /// C ref: `ClientBeginDeathmatch` and `ClientBegin` (player/client.c).
    pub(crate) fn send_login_effect(&self, key: EntityKey) {
        let (Some(number), Some(ent)) = (self.number_of(key), self.entities.get(key)) else {
            return;
        };
        self.gi
            .write_byte(q2_shared::protocol::SvcOp::MuzzleFlash as i32);
        self.gi.write_short(number as i32);
        self.gi.write_byte(MZ_LOGIN);
        self.gi.multicast(ent.state.origin, Multicast::PVS);
    }

    /// C ref: `ClientBeginDeathmatch` and `ClientBegin` (player/client.c).
    pub(crate) fn print_entered_the_game(&self, key: EntityKey) {
        if let Some(client) = self.client(key) {
            self.gi.bprintf(
                q2_shared::constants::PRINT_HIGH,
                &format!("{} entered the game\n", client.pers.netname),
            );
        }
    }

    /// Handle client disconnection. Free the player entity.
    /// C ref: `ClientDisconnect` (player/client.c).
    pub fn disconnect_client(&mut self, key: EntityKey) {
        if let Some(client) = self.client_mut(key) {
            client.pers.connected = false;
        }
        if let Some(ent) = self.entities.get_mut(key) {
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
    pub fn think_client(&mut self, key: EntityKey, cmd: &UserCmd) {
        let Some(number) = self.number_of(key) else {
            return;
        };
        if self.level.intermission_time != 0.0 {
            let can_exit = self.level.time > self.level.intermission_time + 5.0;
            if let Some(client) = self.client_mut(key) {
                client.ps.pmove.pm_type = PmType::Freeze;
            }
            // Can exit intermission after five seconds.
            if can_exit && cmd.buttons & q2_shared::constants::BUTTON_ANY != 0 {
                self.level.exitintermission = true;
            }
            return;
        }
        let (state, snap_initial, mask) = {
            let Some((ent, client)) = self.entity_and_client_mut(key) else {
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
            ground_entity: None,
            touchents: Vec::new(),
            watertype: 0,
            waterlevel: 0,
            airaccelerate: 0.0,
            trace: Box::new(move |start, mins, maxs, end| {
                gi_trace.trace(start, mins, maxs, end, Some(number), mask)
            }),
            pointcontents: Box::new(move |point| gi_contents.point_contents(point)),
        };
        pm.run();

        let ground = pm
            .ground_entity
            .and_then(|n| self.entities.key_for_number(n));
        let ground_linkcount = ground
            .and_then(|g| self.entities.get(g))
            .map(|g| g.linkcount);
        let had_ground = self
            .entities
            .get(key)
            .is_some_and(|e| e.game.ground_entity.is_some());
        let jumped =
            had_ground && pm.ground_entity.is_none() && pm.cmd.upmove >= 10 && pm.waterlevel == 0;
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
        ent.game.ground_entity = ground;
        if let Some(linkcount) = ground_linkcount {
            ent.game.groundentity_linkcount = linkcount;
        }
        let noclip = ent.game.movetype == MoveType::Noclip;
        let dead = ent.game.deadflag != DeadFlag::No;
        if let Some(client) = ent.client.and_then(|slot| self.clients.get_mut(slot)) {
            client.ps.pmove = pm.s;
            client.old_pmove = pm.s;
            for i in 0..3 {
                client.resp.cmd_angles[i] = short2angle(cmd.angles[i]);
            }
            if dead {
                client.ps.viewangles.x = -15.0;
                client.ps.viewangles.z = 40.0;
            } else {
                client.v_angle = pm.viewangles;
                client.ps.viewangles = pm.viewangles;
            }
        }

        if jumped {
            let jump = self.gi.sound_index("*jump1.wav");
            self.gi
                .sound(Some(number), CHAN_VOICE, jump, 1.0, ATTN_NORM, 0.0);
            let origin = self
                .entities
                .get(key)
                .map_or(Vec3f::ZERO, |e| e.state.origin);
            self.player_noise(key, origin, crate::player::weapon::PlayerNoise::SelfNoise);
        }

        self.link_entity(key);

        if !noclip {
            self.touch_triggers(key);
        }

        // Touch other objects, each once.
        for (i, &number) in pm.touchents.iter().enumerate() {
            if pm.touchents[..i].contains(&number) {
                continue; // duplicated
            }
            let Some(other) = self.entities.key_for_number(number) else {
                continue;
            };
            let touch = self.entities.get(other).and_then(|e| e.touch);
            if let Some(touch) = touch {
                touch(self, other, key, None, None);
            }
        }

        let Some((ent, client)) = self.entity_and_client_mut(key) else {
            return;
        };
        client.oldbuttons = client.buttons;
        client.buttons = cmd.buttons;
        client.latched_buttons |= client.buttons & !client.oldbuttons;
        // Save the light level the player is standing on for monster
        // sighting AI.
        ent.game.light_level = i32::from(cmd.lightlevel);

        // Fire from the final position. A spectator's press (chase cam)
        // waits on spectator support.
        let fire = client.latched_buttons & BUTTON_ATTACK != 0
            && !client.resp.spectator
            && !client.weapon_thunk;
        if fire {
            client.weapon_thunk = true;
            self.think_weapon(key);
        }
    }

    /// Per-frame setup for a client at the start of the server frame:
    /// run the weapon unless a command already did, respawn a dead player
    /// on a press, then the player trail. C ref: `ClientBeginServerFrame`
    /// (player/client.c); the deathmatch spectator switch is not ported.
    pub fn client_begin_server_frame(&mut self, key: EntityKey) {
        if self.level.intermission_time != 0.0 {
            return;
        }
        if let Some(client) = self.client_mut(key) {
            if !client.weapon_thunk && !client.resp.spectator {
                self.think_weapon(key);
            } else {
                client.weapon_thunk = false;
            }
        }
        let dead = self
            .entities
            .get(key)
            .is_none_or(|e| e.game.deadflag != crate::constants::DeadFlag::No);
        if dead {
            // Wait for any button just going down; in deathmatch only
            // attack, or none with forced respawn.
            let deathmatch = self.game.deathmatch;
            let force = deathmatch && self.game.dmflags & crate::constants::DF_FORCE_RESPAWN != 0;
            let time = self.level.time;
            let pressed = self.client(key).is_some_and(|c| {
                let mask = if deathmatch { BUTTON_ATTACK } else { u8::MAX };
                time > c.respawn_time && (c.latched_buttons & mask != 0 || force)
            });
            if pressed {
                self.respawn(key);
                if let Some(client) = self.client_mut(key) {
                    client.latched_buttons = 0;
                }
            }
            return;
        }
        self.player_trail_follow(key);
        if let Some(client) = self.client_mut(key) {
            client.latched_buttons = 0;
        }
    }

    /// The player's looping sound: frying in lava or slime, else the
    /// weapon's hum. C ref: `G_SetClientSound` (player/view.c), without
    /// the help-computer beep.
    fn set_client_sound(&mut self, key: EntityKey) {
        // New help: flag it, and beep (no more than three times).
        let (game_helpchanged, framenum) = (self.game.helpchanged, self.level.framenum);
        let beep = self.client_mut(key).is_some_and(|c| {
            if c.pers.game_helpchanged != game_helpchanged {
                c.pers.game_helpchanged = game_helpchanged;
                c.pers.helpchanged = 1;
            }
            let due = c.pers.helpchanged != 0 && c.pers.helpchanged <= 3 && framenum & 63 == 0;
            if due {
                c.pers.helpchanged += 1;
            }
            due
        });
        if beep {
            let pc_up = self.gi.sound_index("misc/pc_up.wav");
            self.gi.sound(
                self.number_of(key),
                CHAN_VOICE,
                pc_up,
                1.0,
                ATTN_STATIC,
                0.0,
            );
        }
        let fry = self.gi.sound_index("player/fry.wav");
        let Some((ent, client)) = self.entity_and_client_mut(key) else {
            return;
        };
        let burning = ent.game.waterlevel != 0
            && ent.game.watertype
                & (q2_shared::constants::CONTENTS_LAVA | q2_shared::constants::CONTENTS_SLIME)
                != 0;
        ent.state.sound = if burning { fry } else { client.weapon_sound };
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
            self.client_end_server_frame(key);
        }
        self.level.gibs_this_frame = 0;
        self.level.debris_this_frame = 0;
    }

    /// Build one client's view, effects, sound and status bar for this
    /// frame. C ref: `ClientEndServerFrame` (player/view.c).
    pub fn client_end_server_frame(&mut self, key: EntityKey) {
        self.world_effects(key);
        // C works out the bob before P_DamageFeedback; the feedback does
        // not touch the bob, so the view offset computes it here.
        self.damage_feedback(key);
        let bob = self.update_player_view(key);
        self.calc_gun_offset(key, &bob);
        // After the view offset, so eye contents are right.
        self.calc_blend(key);
        self.update_player_stats(key);
        self.set_client_effects(key);
        self.set_client_sound(key);
        // If the scoreboard is up, update it.
        let showscores = self.client(key).is_some_and(|c| c.showscores);
        if showscores && self.level.framenum & 31 == 0 {
            let enemy = self.entities.get(key).and_then(|e| e.game.enemy);
            self.deathmatch_scoreboard_message(key, enemy);
            if let Some(number) = self.number_of(key) {
                self.gi.unicast(number, false);
            }
        }
        if let Some(client) = self.client_mut(key) {
            client.oldviewangles = client.ps.viewangles;
            client.kick_origin = Vec3f::ZERO;
            client.kick_angles = Vec3f::ZERO;
        }
    }
}

/// A queued corpse shot below -40 health bursts into gibs and a head.
/// C ref: `body_die` (player/client.c).
pub fn body_die(
    world: &mut GameWorld,
    key: EntityKey,
    _inflictor: EntityKey,
    _attacker: EntityKey,
    damage: i32,
    _point: Vec3f,
) {
    if world.entities.get(key).is_none_or(|e| e.game.health >= -40) {
        return;
    }
    let number = world.number_of(key);
    let sound = world.gi.sound_index("misc/udeath.wav");
    world
        .gi
        .sound(number, CHAN_BODY, sound, 1.0, ATTN_NORM, 0.0);
    for _ in 0..4 {
        crate::misc::throw_gib(
            world,
            key,
            "models/objects/gibs/sm_meat/tris.md2",
            damage,
            crate::misc::GibType::Organic,
        );
    }
    if let Some(ent) = world.entities.get_mut(key) {
        ent.state.origin.z -= 48.0;
    }
    crate::misc::throw_client_head(world, key, damage);
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.takedamage = TakeDamage::No;
    }
}

/// Flags a client keeps across levels (C SaveClientData's mask).
const SAVED_FLAGS: EntityFlags = EntityFlags::GODMODE
    .union(EntityFlags::NOTARGET)
    .union(EntityFlags::POWER_ARMOR);

/// Keep the userinfo and take the player's name and handedness from it.
/// The skin and fov parts are not ported. C ref: `ClientUserinfoChanged`.
fn client_userinfo_changed(pers: &mut ClientPersistent, userinfo: &str) {
    pers.netname = extract_info_value(userinfo, "name").unwrap_or_else(|| "player".to_string());
    if let Some(hand) = extract_info_value(userinfo, "hand").filter(|h| !h.is_empty()) {
        pers.hand = crate::spawn::strtol(&hand);
    }
    pers.userinfo = userinfo.to_string();
}

/// Extract a value from a Quake-style userinfo string ("\\key\\value\\key\\value").
/// Keys compare case-sensitively and a value may be empty, as in C
/// `Info_ValueForKey`.
pub(crate) fn extract_info_value(userinfo: &str, key: &str) -> Option<String> {
    let body = userinfo.strip_prefix('\\').unwrap_or(userinfo);
    let mut parts = body.split('\\');
    while let (Some(k), Some(value)) = (parts.next(), parts.next()) {
        if k == key {
            return Some(value.to_string());
        }
    }
    None
}

/// The player's die callback. C ref: `player_die` (player/client.c),
/// without the obituary, weapon toss, gibs, death animation and sounds.
/// Single player has no respawn: C reloads the last save on a button
/// press, and there is no save/load yet (#15).
pub fn player_die(
    world: &mut GameWorld,
    key: EntityKey,
    inflictor: EntityKey,
    attacker: EntityKey,
    damage: i32,
    _point: Vec3f,
) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.avelocity = Vec3f::ZERO;
    ent.game.takedamage = TakeDamage::Yes;
    ent.game.movetype = MoveType::Toss;
    ent.state.modelindex2 = 0; // remove linked weapon model
    ent.state.angles.x = 0.0;
    ent.state.angles.z = 0.0;
    ent.state.sound = 0;
    ent.maxs.z = -8.0;
    ent.svflags |= SvFlags::DEADMONSTER.bits();
    let first_death = ent.game.deadflag == DeadFlag::No;
    let time = world.level.time;
    if first_death {
        world.client_obituary(key, inflictor, attacker);
        world.toss_client_weapon(key);
        if world.game.deathmatch {
            world.cmd_help(key); // show scores
        }
    }
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    if let Some(client) = ent
        .client
        .and_then(|slot| world.clients.get_mut(slot))
        .filter(|_| first_death)
    {
        client.respawn_time = time + 1.0;
        client.ps.pmove.pm_type = PmType::Dead;
        // Clear the inventory, keeping keys for the coop respawn.
        for (n, item) in world.items.iter().enumerate() {
            if world.game.coop && item.flags.contains(crate::constants::ItemFlags::KEY) {
                client.resp.coop_respawn.inventory[n] = client.pers.inventory[n];
            }
        }
        client.pers.inventory.fill(0);
    }
    // Remove powerups.
    if let Some(client) = ent.client.and_then(|slot| world.clients.get_mut(slot)) {
        client.quad_framenum = 0;
        client.invincible_framenum = 0;
        client.breather_framenum = 0;
        client.enviro_framenum = 0;
    }
    ent.game.flags.remove(EntityFlags::POWER_ARMOR);
    let gibbed = ent.game.health < -40;
    let was_dead = ent.game.deadflag != DeadFlag::No;
    if gibbed {
        // The sound is played at the end of the server frame.
        let udeath = world.gi.sound_index("misc/udeath.wav");
        if let Some(client) = world.client_mut(key) {
            client.death_sound = udeath;
        }
        for _ in 0..4 {
            crate::misc::throw_gib(
                world,
                key,
                "models/objects/gibs/sm_meat/tris.md2",
                damage,
                crate::misc::GibType::Organic,
            );
        }
        crate::misc::throw_client_head(world, key, damage);
        if let Some(ent) = world.entities.get_mut(key) {
            ent.game.takedamage = TakeDamage::No;
        }
    } else if !was_dead && world.client(key).is_some_and(|c| c.death_sound == 0) {
        // Normal death; the animation is #32. The sound is played at the
        // end of the server frame.
        let n = world.random_u32() % 4 + 1;
        let scream = world.gi.sound_index(&format!("*death{n}.wav"));
        if let Some(client) = world.client_mut(key) {
            client.death_sound = scream;
        }
    }
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.game.deadflag = DeadFlag::Dead;
    world.link_entity(key);
}

/// What a self-kill message calls the player (C `IsFemale`, `IsNeutral`
/// on the userinfo `gender`).
fn pronoun(userinfo: &str) -> (&'static str, &'static str) {
    let gender = extract_info_value(userinfo, "gender").unwrap_or_default();
    match gender.chars().next() {
        Some('f' | 'F') => ("her", "herself"),
        Some('m' | 'M') => ("his", "himself"),
        _ => ("its", "itself"),
    }
}

impl GameWorld {
    /// Say how a player died and keep score: a world death or suicide
    /// costs a deathmatch frag, a kill earns the killer one (or costs one
    /// for a teammate). Single player only says "died". C ref:
    /// `ClientObituary` (player/client.c).
    pub fn client_obituary(&mut self, key: EntityKey, _inflictor: EntityKey, attacker: EntityKey) {
        use crate::constants::MeansOfDeath as M;
        use q2_shared::constants::PRINT_MEDIUM;
        let (deathmatch, coop) = (self.game.deathmatch, self.game.coop);
        let attacker_client = self.client(attacker).map(|c| c.pers.netname.clone());
        if coop && attacker_client.is_some() {
            self.friendly_fire = true;
        }
        let Some(client) = self.client(key) else {
            return;
        };
        let name = client.pers.netname.clone();
        let (his, himself) = pronoun(&client.pers.userinfo);
        if deathmatch || coop {
            let (ff, mod_) = (self.friendly_fire, self.means_of_death);
            let mut message: Option<String> = match mod_ {
                M::Suicide => Some("suicides".into()),
                M::Falling => Some("cratered".into()),
                M::Crush => Some("was squished".into()),
                M::Water => Some("sank like a rock".into()),
                M::Slime => Some("melted".into()),
                M::Lava => Some("does a back flip into the lava".into()),
                M::Explosive | M::Barrel => Some("blew up".into()),
                M::Exit => Some("found a way out".into()),
                M::TargetLaser => Some("saw the light".into()),
                M::TargetBlaster => Some("got blasted".into()),
                M::Bomb | M::Splash | M::TriggerHurt => Some("was in the wrong place".into()),
                _ => None,
            };
            if attacker == key {
                message = Some(match mod_ {
                    M::HeldGrenade => "tried to put the pin back in".into(),
                    M::HandgrenadeSplash | M::GrenadeSplash => {
                        format!("tripped on {his} own grenade")
                    }
                    M::RocketSplash => format!("blew {himself} up"),
                    M::BfgBlast => "should have used a smaller gun".into(),
                    _ => format!("killed {himself}"),
                });
            }
            if let Some(message) = message {
                self.gi
                    .bprintf(PRINT_MEDIUM, &format!("{name} {message}.\n"));
                if deathmatch {
                    if let Some(client) = self.client_mut(key) {
                        client.resp.score -= 1;
                    }
                }
                if let Some(ent) = self.entities.get_mut(key) {
                    ent.game.enemy = None;
                }
                return;
            }
            if let Some(ent) = self.entities.get_mut(key) {
                ent.game.enemy = Some(attacker);
            }
            if let Some(killer) = attacker_client {
                let words = match mod_ {
                    M::Blaster => Some(("was blasted by", "")),
                    M::Shotgun => Some(("was gunned down by", "")),
                    M::SShotgun => Some(("was blown away by", "'s super shotgun")),
                    M::Machinegun => Some(("was machinegunned by", "")),
                    M::Chaingun => Some(("was cut in half by", "'s chaingun")),
                    M::Grenade => Some(("was popped by", "'s grenade")),
                    M::GrenadeSplash => Some(("was shredded by", "'s shrapnel")),
                    M::Rocket => Some(("ate", "'s rocket")),
                    M::RocketSplash => Some(("almost dodged", "'s rocket")),
                    M::Hyperblaster => Some(("was melted by", "'s hyperblaster")),
                    M::Railgun => Some(("was railed by", "")),
                    M::BfgLaser => Some(("saw the pretty lights from", "'s BFG")),
                    M::BfgBlast => Some(("was disintegrated by", "'s BFG blast")),
                    M::BfgEffect => Some(("couldn't hide from", "'s BFG")),
                    M::Handgrenade => Some(("caught", "'s handgrenade")),
                    M::HandgrenadeSplash => Some(("didn't see", "'s handgrenade")),
                    M::HeldGrenade => Some(("feels", "'s pain")),
                    M::Telefrag => Some(("tried to invade", "'s personal space")),
                    _ => None,
                };
                if let Some((message, message2)) = words {
                    self.gi.bprintf(
                        PRINT_MEDIUM,
                        &format!("{name} {message} {killer}{message2}\n"),
                    );
                    if deathmatch {
                        if let Some(client) = self.client_mut(attacker) {
                            client.resp.score += if ff { -1 } else { 1 };
                        }
                    }
                    return;
                }
            }
        }
        self.gi.bprintf(PRINT_MEDIUM, &format!("{name} died.\n"));
        if deathmatch {
            if let Some(client) = self.client_mut(key) {
                client.resp.score -= 1;
            }
        }
    }
}

impl GameWorld {
    /// A deathmatch death drops the weapon in hand if it has ammo (never
    /// the blaster) and, with `DF_QUAD_DROP`, a quad with more than a
    /// second left, which vanishes when its time would have run out; the
    /// two go 22.5 degrees either side of the aim. C ref:
    /// `TossClientWeapon` (player/client.c).
    pub fn toss_client_weapon(&mut self, key: EntityKey) {
        if !self.game.deathmatch {
            return;
        }
        let framenum = self.level.framenum;
        let quad_drop = self.game.dmflags & crate::constants::DF_QUAD_DROP != 0;
        let blaster = self.find_item_by_name("Blaster");
        let Some(client) = self.client(key) else {
            return;
        };
        let item = client
            .pers
            .weapon
            .filter(|_| client.pers.inventory[client.ammo_index] != 0)
            .filter(|&w| Some(w) != blaster);
        let quad_left = client.quad_framenum - framenum;
        let quad = quad_drop && client.quad_framenum > framenum + 10;
        let spread = if item.is_some() && quad { 22.5 } else { 0.0 };
        let turn = |world: &mut GameWorld, degrees: f32| {
            if let Some(client) = world.client_mut(key) {
                client.v_angle.y += degrees;
            }
        };
        if let Some(item) = item {
            turn(self, -spread);
            let drop = self.drop_item(key, item);
            turn(self, spread);
            if let Some(e) = drop.and_then(|d| self.entities.get_mut(d)) {
                e.game.spawnflags = crate::items::DROPPED_PLAYER_ITEM;
            }
        }
        if quad {
            let Some(quad_item) = self.find_item("item_quad") else {
                return;
            };
            turn(self, spread);
            let drop = self.drop_item(key, quad_item);
            turn(self, -spread);
            let time = self.level.time;
            if let Some(e) = drop.and_then(|d| self.entities.get_mut(d)) {
                e.game.spawnflags |= crate::items::DROPPED_PLAYER_ITEM;
                e.touch = Some(crate::items::touch_item);
                e.game.nextthink = time + quad_left as f32 * FRAMETIME;
                e.think = Some(crate::items::free_drop);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::GameExport;
    use crate::world::test_world;
    use q2_shared::constants::EF_GIB;

    #[test]
    fn the_userinfo_sets_the_players_hand_and_an_absent_one_keeps_it() {
        let mut world = test_world();
        let key = world.connect_client(1, "\\name\\Ann\\hand\\1").unwrap();
        assert_eq!(world.client(key).unwrap().pers.hand, LEFT_HANDED);
        let pers = &mut world.client_mut(key).unwrap().pers;
        client_userinfo_changed(pers, "\\name\\Ann");
        assert_eq!(pers.hand, LEFT_HANDED, "C only sets it when present");
        client_userinfo_changed(pers, "\\hand\\2");
        assert_eq!(pers.hand, CENTER_HANDED);
    }

    #[test]
    fn client_connect_creates_player() {
        let mut world = test_world();
        let key = world.connect_client(1, "\\name\\TestPlayer").unwrap();

        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.classname, "player");
        assert_eq!(ent.game.health, PLAYER_MAX_HEALTH);
        assert!(ent.client.is_some());
        assert_eq!(world.client(key).unwrap().pers.netname, "TestPlayer");
        assert!(world.client(key).unwrap().pers.connected);
    }

    #[test]
    fn client_entity_numbers_map_to_game_clients_slots() {
        // C ClientConnect: ent->client = game.clients + (ent - g_edicts - 1).
        let mut world = test_world();
        world.set_max_clients(3);
        for number in 1..=3 {
            let key = world.connect_client(number, "").unwrap();
            assert_eq!(world.entities.get(key).unwrap().client, Some(number - 1));
            world.client_mut(key).unwrap().resp.score = number as i32 * 10;
        }
        let scores: Vec<i32> = world.clients.iter().map(|c| c.resp.score).collect();
        assert_eq!(scores, [10, 20, 30]);
    }

    #[test]
    fn client_record_outlives_its_entity_being_freed_and_respawned() {
        // C G_FreeEdict never touches game.clients: a new body for the same
        // number attaches the same record.
        let mut world = test_world();
        world.init_items();
        let key = world.connect_client(1, "\\name\\P1").unwrap();
        world.begin_client(key);
        let shells = world.find_item("ammo_shells").unwrap();
        world.client_mut(key).unwrap().pers.inventory[shells] = 25;

        world.entities.free(key);
        assert!(world.client(key).is_none());
        assert_eq!(world.clients[0].pers.inventory[shells], 25);

        crate::traits::GameExport::client_begin(&mut world, 1);
        let body = world.entities.key_for_number(1).unwrap();
        assert_ne!(body, key);
        assert_eq!(world.client(body).unwrap().pers.inventory[shells], 25);
        assert_eq!(world.client(body).unwrap().pers.netname, "P1");
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
        world.init_items();
        let key = world.connect_client(1, "\\name\\P1").unwrap();
        world.begin_client(key);

        let inv = world.client(key).unwrap().pers.inventory;
        assert_eq!(inv[world.find_item("weapon_blaster").unwrap()], 1);
    }

    #[test]
    fn client_begin_starts_the_lerp_at_the_spawn_point() {
        let mut world = test_world();
        let key = world.connect_client(1, "\\name\\P1").unwrap();
        world.entities.get_mut(key).unwrap().state.old_origin = Vec3f::new(-500.0, 0.0, 0.0);
        world.begin_client(key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.state.old_origin, ent.state.origin);
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
    fn begin_client_clears_what_a_gibbed_body_left_on_the_edict() {
        // C PutClientInServer: effects 0, skin = client index, frame 0,
        // modelindex2 255, svflags 0, no FL_NO_KNOCKBACK, no ground, dry.
        let mut world = test_world();
        world.init_game();
        let key = world.connect_client(1, "").unwrap();
        {
            let e = world.entities.get_mut(key).unwrap();
            e.state.effects = q2_shared::constants::EF_GIB;
            e.state.skinnum = 7;
            e.state.frame = 5;
            e.svflags = SvFlags::DEADMONSTER.bits() | SvFlags::NOCLIENT.bits();
            e.game.flags |= EntityFlags::NO_KNOCKBACK;
            e.game.ground_entity = Some(key);
            e.game.waterlevel = 2;
            e.game.watertype = q2_shared::constants::CONTENTS_WATER;
        }
        world.begin_client(key);
        let e = world.entities.get(key).unwrap();
        // ChangeWeapon then raises the blaster: C ShowGun puts its weapmodel
        // (1) in the skin's high byte and plays FRAME_pain301 (62).
        assert_eq!(
            (e.state.effects, e.state.skinnum, e.state.frame),
            (0, 1 << 8, 62)
        );
        assert_eq!((e.state.modelindex, e.state.modelindex2), (255, 255));
        assert_eq!(e.svflags, 0);
        assert!(!e.game.flags.contains(EntityFlags::NO_KNOCKBACK));
        assert_eq!(e.game.ground_entity, None);
        assert_eq!((e.game.waterlevel, e.game.watertype), (0, 0));
    }

    #[test]
    fn begin_client_telefrags_whatever_stands_on_the_spawn_point() {
        let (mut world, gi) = crate::world::test_world_recording();
        world.init_game();
        let key = world.connect_client(1, "").unwrap();
        let camper = world.spawn().unwrap();
        {
            let e = world.entities.get_mut(camper).unwrap();
            e.game.health = 100;
            e.game.takedamage = TakeDamage::Yes;
            e.solid = Solid::Bbox;
        }
        // The first trace KillBox makes hits the camper.
        gi.queued_traces.borrow_mut().push_back(Trace {
            startsolid: true,
            ent_index: world.number_of(camper),
            ..Default::default()
        });
        world.begin_client(key);
        assert!(world.entities.get(camper).unwrap().game.health <= 0);
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

    /// A spot entity of `class` at `origin`.
    fn spot(world: &mut GameWorld, class: &str, origin: Vec3f, targetname: &str) -> EntityKey {
        let key = world.spawn().unwrap();
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.classname = class.to_string();
        ent.game.targetname = targetname.to_string();
        ent.state.origin = origin;
        key
    }

    /// A live fake player standing at `origin`.
    fn standing(world: &mut GameWorld, origin: Vec3f) -> EntityKey {
        let key = world.spawn_test_client(ClientData::default());
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.health = 100;
        ent.state.origin = origin;
        key
    }

    #[test]
    fn deathmatch_random_spawn_skips_the_two_spots_nearest_a_player() {
        let mut world = test_world();
        world.set_max_clients(2);
        world.game.deathmatch = true;
        let me = world.spawn_test_client(ClientData::default()); // health 0
        standing(&mut world, Vec3f::ZERO);
        spot(
            &mut world,
            "info_player_deathmatch",
            Vec3f::new(10.0, 0.0, 0.0),
            "",
        );
        spot(
            &mut world,
            "info_player_deathmatch",
            Vec3f::new(20.0, 0.0, 0.0),
            "",
        );
        let far = spot(
            &mut world,
            "info_player_deathmatch",
            Vec3f::new(900.0, 0.0, 0.0),
            "",
        );
        // Three spots, two excluded: randk() % 1 always picks the third.
        for _ in 0..4 {
            assert_eq!(world.select_spawn_point(me), Some(far));
        }
    }

    #[test]
    fn deathmatch_random_spawn_excludes_nothing_with_two_spots_or_fewer() {
        let mut world = test_world();
        world.set_max_clients(2);
        world.game.deathmatch = true;
        let me = world.spawn_test_client(ClientData::default());
        standing(&mut world, Vec3f::ZERO);
        let only = spot(&mut world, "info_player_deathmatch", Vec3f::ZERO, "");
        assert_eq!(world.select_spawn_point(me), Some(only));
    }

    #[test]
    fn deathmatch_spawn_farthest_takes_the_spot_farthest_from_players() {
        let mut world = test_world();
        world.set_max_clients(2);
        world.game.deathmatch = true;
        world.game.dmflags = DF_SPAWN_FARTHEST;
        let me = world.spawn_test_client(ClientData::default());
        standing(&mut world, Vec3f::ZERO);
        spot(
            &mut world,
            "info_player_deathmatch",
            Vec3f::new(10.0, 0.0, 0.0),
            "",
        );
        let far = spot(
            &mut world,
            "info_player_deathmatch",
            Vec3f::new(900.0, 0.0, 0.0),
            "",
        );
        spot(
            &mut world,
            "info_player_deathmatch",
            Vec3f::new(20.0, 0.0, 0.0),
            "",
        );
        assert_eq!(world.select_spawn_point(me), Some(far));
    }

    #[test]
    fn deathmatch_spawn_farthest_with_a_player_on_every_spot_uses_the_first() {
        let mut world = test_world();
        world.set_max_clients(3);
        world.game.deathmatch = true;
        world.game.dmflags = DF_SPAWN_FARTHEST;
        let me = world.spawn_test_client(ClientData::default());
        standing(&mut world, Vec3f::ZERO);
        standing(&mut world, Vec3f::new(50.0, 0.0, 0.0));
        let first = spot(&mut world, "info_player_deathmatch", Vec3f::ZERO, "");
        spot(
            &mut world,
            "info_player_deathmatch",
            Vec3f::new(50.0, 0.0, 0.0),
            "",
        );
        assert_eq!(world.select_spawn_point(me), Some(first));
    }

    #[test]
    fn deathmatch_without_deathmatch_spots_uses_a_player_start() {
        let mut world = test_world();
        world.game.deathmatch = true;
        let me = world.spawn_test_client(ClientData::default());
        let start = spot(&mut world, "info_player_start", Vec3f::ZERO, "");
        assert_eq!(world.select_spawn_point(me), Some(start));
    }

    #[test]
    fn coop_clients_after_the_first_take_the_coop_spots_for_the_spawnpoint() {
        let mut world = test_world();
        world.set_max_clients(3);
        world.game.coop = true;
        let first = world.spawn_test_client(ClientData::default());
        let second = world.spawn_test_client(ClientData::default());
        let third = world.spawn_test_client(ClientData::default());
        let start = spot(&mut world, "info_player_start", Vec3f::ZERO, "");
        spot(
            &mut world,
            "info_player_coop",
            Vec3f::new(5.0, 0.0, 0.0),
            "base2",
        );
        let a = spot(
            &mut world,
            "info_player_coop",
            Vec3f::new(6.0, 0.0, 0.0),
            "",
        );
        let b = spot(
            &mut world,
            "info_player_coop",
            Vec3f::new(7.0, 0.0, 0.0),
            "",
        );
        assert_eq!(world.select_spawn_point(first), Some(start), "slot 0");
        assert_eq!(world.select_spawn_point(second), Some(a));
        assert_eq!(world.select_spawn_point(third), Some(b));
    }

    #[test]
    fn coop_without_matching_coop_spots_takes_one_within_550_of_the_start() {
        let mut world = test_world();
        world.set_max_clients(2);
        world.game.coop = true;
        world.game.spawnpoint = "base2".to_string();
        let first = world.spawn_test_client(ClientData::default());
        let second = world.spawn_test_client(ClientData::default());
        let start = spot(&mut world, "info_player_start", Vec3f::ZERO, "BASE2");
        spot(
            &mut world,
            "info_player_coop",
            Vec3f::new(100.0, 0.0, 0.0),
            "other",
        );
        spot(
            &mut world,
            "info_player_coop",
            Vec3f::new(2000.0, 0.0, 0.0),
            "other",
        );
        let near = spot(
            &mut world,
            "info_player_coop",
            Vec3f::new(200.0, 0.0, 0.0),
            "other",
        );
        assert_eq!(world.select_spawn_point(first), Some(start));
        // C counts near spots from 0 and takes the one matching the slot.
        assert_eq!(world.select_spawn_point(second), Some(near));
    }

    fn attack(buttons: u8) -> UserCmd {
        UserCmd {
            msec: 10,
            buttons,
            ..Default::default()
        }
    }

    #[test]
    fn think_client_latches_a_new_press_and_not_a_held_button() {
        use q2_shared::constants::BUTTON_ATTACK;
        let mut world = test_world();
        let key = world.connect_client(1, "\\name\\P1").unwrap();
        world.begin_client(key);
        world.client_mut(key).unwrap().pers.weapon = None;

        world.think_client(key, &attack(BUTTON_ATTACK));
        let client = world.client(key).unwrap();
        assert_eq!(client.buttons, BUTTON_ATTACK);
        assert_eq!(client.latched_buttons & BUTTON_ATTACK, BUTTON_ATTACK);

        world.client_mut(key).unwrap().latched_buttons = 0;
        world.think_client(key, &attack(BUTTON_ATTACK));
        let client = world.client(key).unwrap();
        assert_eq!(client.oldbuttons, BUTTON_ATTACK);
        assert_eq!(client.latched_buttons, 0, "held, not pressed again");

        world.think_client(key, &attack(0));
        world.think_client(key, &attack(BUTTON_ATTACK));
        assert_eq!(world.client(key).unwrap().latched_buttons, BUTTON_ATTACK);
    }

    #[test]
    fn an_attack_press_thinks_the_weapon_once_for_the_frame_like_c() {
        use q2_shared::constants::BUTTON_ATTACK;
        let mut world = test_world();
        let key = world.connect_client(1, "\\name\\P1").unwrap();
        world.begin_client(key);

        world.think_client(key, &attack(BUTTON_ATTACK));
        assert!(world.client(key).unwrap().weapon_thunk);
        // ClientBeginServerFrame skips the think the command already ran
        // and clears the flag and the latched buttons for the next frame.
        world.client_begin_server_frame(key);
        let client = world.client(key).unwrap();
        assert!(!client.weapon_thunk);
        assert_eq!(client.latched_buttons, 0);
    }

    #[test]
    fn think_client_keeps_the_view_angles_for_aiming_like_c_v_angle() {
        let mut world = test_world();
        let key = world.connect_client(1, "\\name\\P1").unwrap();
        world.begin_client(key);
        let mut cmd = attack(0);
        cmd.angles = [angle2short(10.0), angle2short(90.0), 0];
        world.think_client(key, &cmd);
        let client = world.client(key).unwrap();
        assert_eq!(client.v_angle, client.ps.viewangles);
        assert!(
            (client.v_angle.y - 90.0).abs() < 0.1,
            "{:?}",
            client.v_angle
        );
    }

    #[test]
    fn the_weapon_hum_plays_on_the_player_and_lava_drowns_it_out() {
        let mut world = test_world();
        let key = world.connect_client(1, "\\name\\P1").unwrap();
        world.begin_client(key);
        let hum = world.gi.sound_index("weapons/hyprbl1a.wav");
        world.client_mut(key).unwrap().weapon_sound = hum;
        world.client_end_server_frames();
        assert_eq!(world.entities.get(key).unwrap().state.sound, hum);

        let ent = world.entities.get_mut(key).unwrap();
        ent.game.waterlevel = 1;
        ent.game.watertype = q2_shared::constants::CONTENTS_LAVA;
        world.client_end_server_frames();
        let fry = world.gi.sound_index("player/fry.wav");
        assert_eq!(world.entities.get(key).unwrap().state.sound, fry);

        let ent = world.entities.get_mut(key).unwrap();
        ent.game.waterlevel = 0;
        world.client_mut(key).unwrap().weapon_sound = 0;
        world.client_end_server_frames();
        assert_eq!(world.entities.get(key).unwrap().state.sound, 0);
    }

    fn dead_player(
        deathmatch: bool,
    ) -> (
        GameWorld,
        EntityKey,
        std::rc::Rc<crate::world::MockGameImport>,
    ) {
        let (mut world, gi) = crate::world::test_world_recording();
        world.init_items();
        world.game.deathmatch = deathmatch;
        let start = world.spawn().unwrap();
        world.entities.get_mut(start).unwrap().game.classname = if deathmatch {
            "info_player_deathmatch"
        } else {
            "info_player_start"
        }
        .into();
        let key = world.connect_client(1, "\\name\\P1").unwrap();
        world.begin_client(key);
        world.level.time = 10.0;
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.health = 0;
        ent.game.deadflag = DeadFlag::Dead;
        world.client_mut(key).unwrap().respawn_time = 11.0;
        (world, key, gi)
    }

    #[test]
    fn a_dead_deathmatch_player_respawns_on_attack_after_a_second() {
        use q2_shared::constants::{BUTTON_ATTACK, BUTTON_USE};
        let (mut world, key, _) = dead_player(true);
        world.client_mut(key).unwrap().latched_buttons = BUTTON_ATTACK;
        world.client_begin_server_frame(key);
        assert_eq!(
            world.entities.get(key).unwrap().game.deadflag,
            DeadFlag::Dead,
            "too soon"
        );
        world.level.time = 11.5;
        world.client_mut(key).unwrap().latched_buttons = BUTTON_USE;
        world.client_begin_server_frame(key);
        assert_eq!(
            world.entities.get(key).unwrap().game.deadflag,
            DeadFlag::Dead,
            "attack only"
        );
        world.client_mut(key).unwrap().latched_buttons = BUTTON_ATTACK;
        world.client_begin_server_frame(key);
        assert_eq!(world.entities.get(key).unwrap().game.deadflag, DeadFlag::No);
        let client = world.client(key).unwrap();
        assert_eq!((client.latched_buttons, client.respawn_time), (0, 11.5));
    }

    #[test]
    fn force_respawn_brings_a_deathmatch_player_back_without_a_press() {
        let (mut world, key, _) = dead_player(true);
        world.game.dmflags = crate::constants::DF_FORCE_RESPAWN;
        world.level.time = 11.5;
        world.client_begin_server_frame(key);
        assert_eq!(world.entities.get(key).unwrap().game.deadflag, DeadFlag::No);
    }

    #[test]
    fn a_dead_single_player_asks_for_the_load_menu_on_any_press() {
        use q2_shared::constants::BUTTON_USE;
        let (mut world, key, gi) = dead_player(false);
        world.level.time = 11.5;
        world.client_mut(key).unwrap().latched_buttons = BUTTON_USE;
        world.client_begin_server_frame(key);
        assert_eq!(*gi.commands.borrow(), ["menu_loadgame\n"]);
    }

    /// A deathmatch player holding `weapon` with `ammo` rounds, about to die.
    fn armed_for_death(weapon: &str, ammo: (&str, i32)) -> (GameWorld, EntityKey) {
        let (mut world, _) = crate::world::test_world_recording();
        world.init_items();
        world.game.deathmatch = true;
        world.level.framenum = 100;
        world.level.time = 10.0;
        let key = world.connect_client(1, "\\name\\P1").unwrap();
        world.begin_client(key);
        let weapon = world.find_item_by_name(weapon).unwrap();
        let ammo_item = world.find_item_by_name(ammo.0).unwrap();
        let client = world.client_mut(key).unwrap();
        client.pers.weapon = Some(weapon);
        client.pers.inventory[weapon] = 1;
        client.ammo_index = ammo_item;
        client.pers.inventory[ammo_item] = ammo.1;
        (world, key)
    }

    fn dropped(world: &GameWorld, classname: &str) -> Option<EntityKey> {
        world.find_by_classname(None, classname)
    }

    #[test]
    fn a_deathmatch_death_drops_the_loaded_weapon() {
        let (mut world, key) = armed_for_death("Shotgun", ("Shells", 5));
        player_die(&mut world, key, key, key, 50, Vec3f::ZERO);
        let gun = dropped(&world, "weapon_shotgun").expect("dropped");
        assert_eq!(
            world.entities.get(gun).unwrap().game.spawnflags,
            crate::items::DROPPED_PLAYER_ITEM
        );
    }

    #[test]
    fn no_drop_for_the_blaster_an_empty_gun_or_single_player() {
        let (mut world, key) = armed_for_death("Shotgun", ("Shells", 0));
        player_die(&mut world, key, key, key, 50, Vec3f::ZERO);
        assert!(dropped(&world, "weapon_shotgun").is_none(), "no ammo");

        let (mut world, key) = armed_for_death("Blaster", ("Shells", 5));
        player_die(&mut world, key, key, key, 50, Vec3f::ZERO);
        assert!(dropped(&world, "weapon_blaster").is_none());

        let (mut world, key) = armed_for_death("Shotgun", ("Shells", 5));
        world.game.deathmatch = false;
        player_die(&mut world, key, key, key, 50, Vec3f::ZERO);
        assert!(dropped(&world, "weapon_shotgun").is_none());
    }

    #[test]
    fn quad_drop_throws_the_quad_with_its_time_left_beside_the_gun() {
        let (mut world, key) = armed_for_death("Shotgun", ("Shells", 5));
        world.game.dmflags = crate::constants::DF_QUAD_DROP;
        world.client_mut(key).unwrap().quad_framenum = 150;
        player_die(&mut world, key, key, key, 50, Vec3f::ZERO);
        let quad = dropped(&world, "item_quad").expect("quad dropped");
        let e = world.entities.get(quad).unwrap();
        assert_eq!(
            e.game.spawnflags,
            crate::items::DROPPED_ITEM | crate::items::DROPPED_PLAYER_ITEM
        );
        assert_eq!(e.game.nextthink, 15.0, "50 frames left");
        assert!(std::ptr::fn_addr_eq(
            e.touch.unwrap(),
            crate::items::touch_item as crate::entity::TouchFn
        ));
        // The gun and the quad go 22.5 degrees either side.
        let gun = world
            .entities
            .get(dropped(&world, "weapon_shotgun").unwrap())
            .unwrap();
        assert!((gun.velocity.normalize() - e.velocity.normalize()).length() > 0.1);
        assert_eq!(world.client(key).unwrap().v_angle.y, 0.0, "aim restored");
    }

    #[test]
    fn a_nearly_spent_quad_is_not_dropped() {
        let (mut world, key) = armed_for_death("Shotgun", ("Shells", 5));
        world.game.dmflags = crate::constants::DF_QUAD_DROP;
        world.client_mut(key).unwrap().quad_framenum = 110;
        player_die(&mut world, key, key, key, 50, Vec3f::ZERO);
        assert!(dropped(&world, "item_quad").is_none(), "10 frames or less");
    }

    #[test]
    fn a_dying_player_screams_one_of_four_death_sounds() {
        let (mut world, _) = crate::world::test_world_recording();
        world.init_items();
        let key = world.connect_client(1, "\\name\\P1").unwrap();
        world.begin_client(key);
        world.entities.get_mut(key).unwrap().game.health = -10;
        player_die(&mut world, key, key, key, 50, Vec3f::ZERO);
        let sound = world.client(key).unwrap().death_sound;
        let names: Vec<i32> = (1..=4)
            .map(|n| world.gi.sound_index(&format!("*death{n}.wav")))
            .collect();
        assert!(names.contains(&sound), "{sound}");
    }

    #[test]
    fn a_gibbed_player_bursts_into_gibs_and_a_head() {
        let (mut world, gi) = crate::world::test_world_recording();
        world.init_items();
        let key = world.connect_client(1, "\\name\\P1").unwrap();
        world.begin_client(key);
        world.entities.get_mut(key).unwrap().game.health = -50;
        player_die(&mut world, key, key, key, 80, Vec3f::ZERO);
        assert_eq!(
            world.client(key).unwrap().death_sound,
            world.gi.sound_index("misc/udeath.wav")
        );
        let gibs = world
            .entities
            .iter()
            .filter(|&(k, e)| e.in_use && k != key && e.state.effects & EF_GIB != 0)
            .count();
        assert_eq!(gibs, 4);
        let head = world.entities.get(key).unwrap();
        let model = gi.models.borrow()[head.state.modelindex as usize - 1].clone();
        assert!(
            model.contains("head2") || model.contains("skull"),
            "{model}"
        );
        assert_eq!(head.game.takedamage, TakeDamage::No);
    }

    #[test]
    fn dying_schedules_the_respawn_a_second_later() {
        let (mut world, gi) = crate::world::test_world_recording();
        world.init_items();
        let key = world.connect_client(1, "\\name\\P1").unwrap();
        world.begin_client(key);
        world.level.time = 7.0;
        let _ = gi;
        world.client_mut(key).unwrap().quad_framenum = 500;
        player_die(&mut world, key, key, key, 50, Vec3f::ZERO);
        assert_eq!(world.client(key).unwrap().respawn_time, 8.0);
        assert_eq!(world.client(key).unwrap().quad_framenum, 0, "powerups go");
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
        let client = world.client(key).unwrap();
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
    fn leaving_the_ground_with_upmove_plays_the_jump_sound_and_makes_noise() {
        // C ClientThink: had ground, pmove lost it, upmove >= 10, dry.
        for (upmove, jumps) in [(200, true), (9, false)] {
            let (mut world, gi) = crate::world::test_world_recording();
            let key = world.connect_client(1, "\\name\\P1").unwrap();
            world.begin_client(key);
            let floor = world.spawn().unwrap();
            world.entities.get_mut(key).unwrap().game.ground_entity = Some(floor);
            world.level.framenum = 10;
            let cmd = UserCmd {
                msec: 100,
                upmove,
                ..Default::default()
            };

            world.think_client(key, &cmd);

            let jump = gi.sounds.borrow().iter().position(|s| s == "*jump1.wav");
            let voiced = jump.is_some_and(|i| {
                gi.sound_calls
                    .borrow()
                    .iter()
                    .any(|c| c.0 == Some(1) && c.1 == CHAN_VOICE && c.2 == i as i32 + 1)
            });
            assert_eq!(voiced, jumps, "upmove {upmove}");
            assert_eq!(world.level.sound_entity.is_some(), jumps, "upmove {upmove}");
        }
    }

    #[test]
    fn think_client_keeps_the_light_level_the_player_stands_in_for_monsters() {
        let mut world = test_world();
        let key = world.connect_client(1, "\\name\\P1").unwrap();
        world.begin_client(key);
        let cmd = UserCmd {
            msec: 100,
            lightlevel: 77,
            ..Default::default()
        };
        world.think_client(key, &cmd);
        assert_eq!(world.entities.get(key).unwrap().game.light_level, 77);
    }

    #[test]
    fn think_client_treats_a_player_without_model_255_as_gibs_like_c() {
        let mut world = test_world();
        let key = world.connect_client(1, "\\name\\P1").unwrap();
        world.think_client(key, &UserCmd::default());
        let client = world.client(key).unwrap().clone();
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

    #[test]
    fn extract_info_value_keeps_pairs_aligned_across_an_empty_value() {
        let info = "\\name\\\\skin\\male/grunt";
        assert_eq!(extract_info_value(info, "name"), Some(String::new()));
        assert_eq!(
            extract_info_value(info, "skin"),
            Some("male/grunt".to_string())
        );
        assert_eq!(extract_info_value(info, "male/grunt"), None);
    }

    #[test]
    fn a_coop_player_dying_keeps_its_keys_for_the_respawn() {
        // C player_die copies IT_KEY items to resp.coop_respawn in coop.
        for coop in [true, false] {
            let mut world = test_world();
            world.game.coop = coop;
            world.init_items();
            let key = world.connect_client(1, "\\name\\P1").unwrap();
            world.begin_client(key);
            let pass = world.find_item("key_pass").unwrap();
            let shells = world.find_item("ammo_shells").unwrap();
            {
                let inv = &mut world.client_mut(key).unwrap().pers.inventory;
                inv[pass] = 1;
                inv[shells] = 10;
            }
            let attacker = world.spawn().unwrap();
            player_die(&mut world, key, attacker, attacker, 100, Vec3f::ZERO);
            let client = world.client(key).unwrap();
            assert!(client.pers.inventory.iter().all(|&n| n == 0));
            let kept = &client.resp.coop_respawn.inventory;
            assert_eq!(
                (kept[pass], kept[shells]),
                (i32::from(coop), 0),
                "coop {coop}"
            );
        }
    }

    #[test]
    fn player_dies_when_damage_takes_its_health_to_zero() {
        let mut world = test_world();
        world.init_items();
        let key = world.connect_client(1, "\\name\\P1").unwrap();
        world.begin_client(key);
        let attacker = world.spawn().unwrap();

        world.t_damage(
            key,
            attacker,
            attacker,
            Vec3f::X,
            Vec3f::ZERO,
            Vec3f::ZERO,
            120,
            0,
            DamageFlags::empty(),
            MeansOfDeath::Unknown,
        );

        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.deadflag, DeadFlag::Dead);
        assert_eq!(ent.game.movetype, MoveType::Toss);
        assert_eq!(ent.maxs.z, -8.0);
        assert!(ent.svflags & SvFlags::DEADMONSTER.bits() != 0);
        let client = world.client(key).unwrap();
        assert_eq!(client.ps.pmove.pm_type, PmType::Dead);
        assert!(client.pers.inventory.iter().all(|&n| n == 0));
    }

    /// A deathmatch world with one client in game, its body queue reserved.
    fn deathmatch_world() -> (GameWorld, EntityKey) {
        let mut world = test_world();
        world.set_max_clients(1);
        world.game.deathmatch = true;
        world.init();
        world.spawn_entities(
            "q2dm1",
            "{ \"classname\" \"worldspawn\" }\n\
             { \"classname\" \"info_player_deathmatch\" \"origin\" \"64 0 24\" }",
            "",
        );
        assert!(world.client_connect(1, r"\name\Tester"));
        world.client_begin(1);
        let player = world.entities.key_for_number(1).unwrap();
        (world, player)
    }

    #[test]
    fn deathmatch_respawn_leaves_the_corpse_in_the_next_body_queue_slot() {
        // C CopyToBodyQue: body = g_edicts[maxclients + body_que + 1].
        let (mut world, player) = deathmatch_world();
        {
            let ent = world.entities.get_mut(player).unwrap();
            ent.state.origin = Vec3f::new(300.0, 20.0, 8.0);
            ent.state.frame = 197;
            ent.game.movetype = MoveType::Toss;
        }
        world.respawn(player);
        let body = world.entities.key_for_number(2).unwrap();
        let b = world.entities.get(body).unwrap();
        assert_eq!(b.state.number, 2);
        assert_eq!(b.state.origin, Vec3f::new(300.0, 20.0, 8.0));
        assert_eq!(b.state.frame, 197);
        assert_eq!(b.game.movetype, MoveType::Toss);
        assert_eq!(b.maxs, PLAYER_MAXS);
        assert_eq!(b.game.takedamage, TakeDamage::Yes);
        assert!(b.die.is_some());
        assert_eq!(b.game.classname, "bodyque");
        assert_eq!(world.level.body_que, 1);
        // The queue wraps after eight bodies.
        for _ in 0..7 {
            world.respawn(player);
        }
        assert_eq!(world.level.body_que, 0);
    }

    #[test]
    fn spectator_respawn_leaves_no_body() {
        let (mut world, player) = deathmatch_world();
        world.entities.get_mut(player).unwrap().game.movetype = MoveType::Noclip;
        world.respawn(player);
        assert_eq!(world.level.body_que, 0);
        let body = world.entities.key_for_number(2).unwrap();
        assert!(world.entities.get(body).unwrap().die.is_none());
    }

    #[test]
    fn body_die_gibs_a_corpse_only_below_minus_forty_health() {
        let (mut world, player) = deathmatch_world();
        world.respawn(player);
        let body = world.entities.key_for_number(2).unwrap();
        let gibs = |world: &GameWorld| {
            world
                .entities
                .iter()
                .filter(|(k, e)| e.in_use && *k != body && e.state.effects & EF_GIB != 0)
                .count()
        };
        world.entities.get_mut(body).unwrap().game.health = -40;
        body_die(&mut world, body, body, body, 50, Vec3f::ZERO);
        assert_eq!(gibs(&world), 0);
        assert_eq!(
            world.entities.get(body).unwrap().game.takedamage,
            TakeDamage::Yes
        );

        let z = world.entities.get(body).unwrap().state.origin.z;
        world.entities.get_mut(body).unwrap().game.health = -41;
        body_die(&mut world, body, body, body, 50, Vec3f::ZERO);
        assert_eq!(gibs(&world), 4);
        let b = world.entities.get(body).unwrap();
        assert_eq!(b.game.takedamage, TakeDamage::No);
        // Dropped 48, then ThrowClientHead raises the head 32.
        assert_eq!(b.state.origin.z, z - 48.0 + 32.0);
        assert_eq!(b.state.effects, EF_GIB);
    }
}
