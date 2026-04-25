//! Player client management — connect, begin, disconnect, think, respawn.
//!
//! C ref: `player/client.c` (2,501 lines)

use q2_shared::types::*;

use crate::constants::*;
use crate::entity::{ClientData, ClientPersistent, EntityKey};
use crate::world::GameWorld;

/// Maximum health for a player.
const PLAYER_MAX_HEALTH: i32 = 100;

/// Default player bounding box.
const PLAYER_MINS: Vec3f = Vec3f::new(-16.0, -16.0, -24.0);
const PLAYER_MAXS: Vec3f = Vec3f::new(16.0, 16.0, 32.0);

impl GameWorld {
    /// Handle a new client connection. Allocates a player entity.
    /// C ref: `ClientConnect` (player/client.c).
    pub fn client_connect(&mut self, userinfo: &str) -> Option<EntityKey> {
        let key = self.spawn()?;

        let ent = self.entities.get_mut(key)?;
        ent.game.classname = "player".to_string();
        ent.mins = PLAYER_MINS;
        ent.maxs = PLAYER_MAXS;
        ent.solid = Solid::Bbox;
        ent.clipmask = 0xFFFF; // MASK_PLAYERSOLID
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
    /// C ref: `PutClientInServer` / `ClientBegin` (player/client.c).
    pub fn client_begin(&mut self, key: EntityKey) {
        // Find a spawn point by searching live entities.
        let spawn_pos = self
            .find_by_classname(None, "info_player_start")
            .or_else(|| self.find_by_classname(None, "info_player_deathmatch"))
            .and_then(|k| self.entities.get(k))
            .map(|e| e.state.origin)
            .unwrap_or(Vec3f::ZERO);

        if let Some(ent) = self.entities.get_mut(key) {
            ent.state.origin = spawn_pos;
            ent.in_use = true;

            // Give starting weapon (blaster).
            if let Some(ref mut client) = ent.client {
                client.pers.inventory[7] = 1; // blaster at item index 7
            }
        }

        self.gi.link_entity(0);
    }

    /// Handle client disconnection. Free the player entity.
    /// C ref: `ClientDisconnect` (player/client.c).
    pub fn client_disconnect(&mut self, key: EntityKey) {
        if let Some(ent) = self.entities.get_mut(key) {
            if let Some(ref mut client) = ent.client {
                client.pers.connected = false;
            }
            ent.in_use = false;
        }
        self.free_entity(key);
    }

    /// Process a client's input command for one frame.
    /// C ref: `ClientThink` (player/client.c).
    pub fn client_think(&mut self, key: EntityKey, cmd: &UserCmd) {
        let Some(ent) = self.entities.get_mut(key) else {
            return;
        };

        // Apply movement from UserCmd to velocity (simplified).
        // Full implementation uses pmove for prediction-compatible movement.
        let yaw = ent.state.angles.y.to_radians();
        let forward = Vec3f::new(yaw.cos(), yaw.sin(), 0.0);
        let right = Vec3f::new(-yaw.sin(), yaw.cos(), 0.0);

        let fmove = cmd.forwardmove as f32;
        let smove = cmd.sidemove as f32;
        let umove = cmd.upmove as f32;

        ent.velocity = forward * fmove + right * smove + Vec3f::new(0.0, 0.0, umove);

        // Apply view angles from command.
        ent.state.angles = Vec3f::new(
            cmd.angles[0] as f32 * (360.0 / 65536.0),
            cmd.angles[1] as f32 * (360.0 / 65536.0),
            cmd.angles[2] as f32 * (360.0 / 65536.0),
        );

        // Handle buttons (attack, use).
        if cmd.buttons & 1 != 0 {
            // Attack button — would trigger weapon fire.
            // Connected to weapon system in full implementation.
        }
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
        let key = world.client_connect("\\name\\TestPlayer").unwrap();

        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.classname, "player");
        assert_eq!(ent.game.health, PLAYER_MAX_HEALTH);
        assert!(ent.client.is_some());
        assert_eq!(
            ent.client.as_ref().unwrap().pers.netname,
            "TestPlayer"
        );
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

        let key = world.client_connect("\\name\\P1").unwrap();
        world.client_begin(key);

        let pos = world.entities.get(key).unwrap().state.origin;
        assert!((pos.x - 100.0).abs() < 0.01);
        assert!((pos.y - 200.0).abs() < 0.01);
    }

    #[test]
    fn client_begin_gives_blaster() {
        let mut world = test_world();
        let key = world.client_connect("\\name\\P1").unwrap();
        world.client_begin(key);

        let inv = world.entities.get(key).unwrap()
            .client.as_ref().unwrap().pers.inventory;
        assert_eq!(inv[7], 1); // blaster
    }

    #[test]
    fn client_disconnect_frees_entity() {
        let mut world = test_world();
        let key = world.client_connect("\\name\\P1").unwrap();

        assert_eq!(world.entities.count(), 1);
        world.client_disconnect(key);
        assert_eq!(world.entities.count(), 0);
    }

    #[test]
    fn client_think_applies_movement() {
        let mut world = test_world();
        let key = world.client_connect("\\name\\P1").unwrap();
        world.entities.get_mut(key).unwrap().state.angles.y = 0.0;

        let cmd = UserCmd {
            forwardmove: 200,
            sidemove: 0,
            upmove: 0,
            msec: 16,
            buttons: 0,
            angles: [0, 0, 0],
            impulse: 0,
            lightlevel: 0,
        };

        world.client_think(key, &cmd);

        let v = world.entities.get(key).unwrap().velocity;
        assert!(v.x > 0.0); // Moving forward
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
        assert_eq!(
            extract_info_value("\\name\\Player1", "missing"),
            None
        );
    }
}
