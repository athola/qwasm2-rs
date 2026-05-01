//! Concrete GameExport implementation — the game module's public entry point.
//!
//! Wires together EntityStorage, SpawnTable, and the GameImport callbacks
//! into the interface the server drives.

use crate::entity::{ClientData, EntityKey, EntityStorage};
use crate::spawn::{parse_entity_string, SpawnTable};
use crate::traits::{GameExport, GameImport};
use q2_shared::{
    constants::{CS_MAXCLIENTS, MAX_CLIENTS, MAX_EDICTS},
    types::UserCmd,
};

pub struct GameLogic {
    storage: EntityStorage,
    spawn_table: SpawnTable,
    framenum: i32,
    /// Maps server client-slot index (0-based) to entity key.
    player_slots: Vec<Option<EntityKey>>,
}

impl GameLogic {
    pub fn new() -> Self {
        Self {
            storage: EntityStorage::new(MAX_EDICTS),
            spawn_table: SpawnTable::new(),
            framenum: 0,
            player_slots: vec![None; MAX_CLIENTS],
        }
    }

    fn ensure_slot(&mut self, idx: usize) {
        debug_assert!(idx < MAX_CLIENTS, "player index {} out of range", idx);
    }
}

impl Default for GameLogic {
    fn default() -> Self {
        Self::new()
    }
}

impl GameExport for GameLogic {
    fn api_version(&self) -> i32 {
        3
    }

    fn init(&mut self, import: &dyn GameImport) {
        import.dprintf("GameLogic initialised\n");
        // Mirrors G_Init in g_main.c: advertise max client count via configstring.
        import.configstring(CS_MAXCLIENTS as i32, "8");
    }

    fn shutdown(&mut self) {
        self.storage = EntityStorage::new(MAX_EDICTS);
        self.player_slots.fill(None);
        self.framenum = 0;
    }

    fn spawn_entities(&mut self, mapname: &str, entstring: &str, _spawnpoint: &str) {
        let entities = parse_entity_string(entstring);
        for props in &entities {
            let key = match self.storage.spawn() {
                Some(k) => k,
                None => {
                    tracing::warn!(map = mapname, "entity pool full during spawn_entities");
                    break;
                }
            };
            if let Some(classname) = props.get("classname") {
                if let Some(spawn_fn) = self.spawn_table.get(classname) {
                    spawn_fn(&mut self.storage, key, props);
                }
            }
        }
        tracing::debug!(
            map = mapname,
            count = entities.len(),
            "spawn_entities complete"
        );
    }

    fn client_connect(&mut self, ent_idx: usize, _userinfo: &str) -> bool {
        self.ensure_slot(ent_idx);
        if let Some(old_key) = self.player_slots[ent_idx].take() {
            self.storage.free(old_key);
        }
        true
    }

    fn client_begin(&mut self, ent_idx: usize) {
        self.ensure_slot(ent_idx);
        if let Some(old_key) = self.player_slots[ent_idx].take() {
            self.storage.free(old_key);
        }
        if let Some(key) = self.storage.spawn() {
            if let Some(ent) = self.storage.get_mut(key) {
                ent.client = Some(ClientData::default());
                ent.game.classname = "player".to_string();
            }
            self.player_slots[ent_idx] = Some(key);
        } else {
            tracing::warn!(slot = ent_idx, "entity pool full — cannot spawn player");
        }
    }

    fn client_disconnect(&mut self, ent_idx: usize) {
        self.ensure_slot(ent_idx);
        if let Some(key) = self.player_slots[ent_idx].take() {
            self.storage.free(key);
        }
    }

    fn client_command(&mut self, _ent_idx: usize) {}

    fn client_think(&mut self, _ent_idx: usize, _cmd: &UserCmd) {}

    fn run_frame(&mut self) {
        self.framenum += 1;
        // Future: iterate entities with nextthink <= server time and call think fns
    }

    fn server_command(&mut self) {}
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::GameImport;
    use q2_shared::types::*;

    struct NoopImport;

    impl GameImport for NoopImport {
        fn bprintf(&self, _: i32, _: &str) {}
        fn dprintf(&self, _: &str) {}
        fn cprintf(&self, _: Option<usize>, _: i32, _: &str) {}
        fn centerprintf(&self, _: Option<usize>, _: &str) {}
        fn sound(&self, _: Option<usize>, _: i32, _: i32, _: f32, _: f32, _: f32) {}
        fn model_index(&self, _: &str) -> i32 {
            0
        }
        fn sound_index(&self, _: &str) -> i32 {
            0
        }
        fn image_index(&self, _: &str) -> i32 {
            0
        }
        fn set_model(&self, _: usize, _: &str) {}
        fn trace(&self, _: Vec3f, _: Vec3f, _: Vec3f, _: Vec3f, _: Option<usize>, _: i32) -> Trace {
            Trace::default()
        }
        fn point_contents(&self, _: Vec3f) -> i32 {
            0
        }
        fn in_pvs(&self, _: Vec3f, _: Vec3f) -> bool {
            false
        }
        fn in_phs(&self, _: Vec3f, _: Vec3f) -> bool {
            false
        }
        fn link_entity(&self, _: usize) {}
        fn unlink_entity(&self, _: usize) {}
        fn box_edicts(&self, _: Vec3f, _: Vec3f, _: usize, _: i32) -> Vec<usize> {
            Vec::new()
        }
        fn configstring(&self, _: i32, _: &str) {}
        fn write_byte(&self, _: i32) {}
        fn write_short(&self, _: i32) {}
        fn write_long(&self, _: i32) {}
        fn write_float(&self, _: f32) {}
        fn write_string(&self, _: &str) {}
        fn write_position(&self, _: Vec3f) {}
        fn write_dir(&self, _: Vec3f) {}
        fn write_angle(&self, _: f32) {}
        fn multicast(&self, _: Vec3f, _: Multicast) {}
        fn unicast(&self, _: usize, _: bool) {}
        fn argc(&self) -> i32 {
            0
        }
        fn argv(&self, _: i32) -> String {
            String::new()
        }
        fn args(&self) -> String {
            String::new()
        }
        fn add_command_string(&self, _: &str) {}
    }

    #[test]
    fn game_logic_init() {
        let mut game = GameLogic::new();
        game.init(&NoopImport);
        assert_eq!(game.framenum, 0);
        assert_eq!(game.storage.count(), 0);
    }

    #[test]
    fn spawn_entities_populates_storage() {
        let mut game = GameLogic::new();
        let entstring = r#"
{ "classname" "worldspawn" "message" "Test" }
{ "classname" "info_player_start" "origin" "0 0 0" }
"#;
        game.spawn_entities("test_map", entstring, "");
        assert_eq!(game.storage.count(), 2);
    }

    #[test]
    fn client_connect_and_begin() {
        let mut game = GameLogic::new();
        assert!(game.client_connect(0, r"name\test\skin\male/grunt"));
        game.client_begin(0);
        // Player entity should exist
        let slot_key = game.player_slots[0].expect("player slot should be filled");
        let ent = game.storage.get(slot_key).expect("entity should exist");
        assert_eq!(ent.game.classname, "player");
        assert!(ent.client.is_some());
    }

    #[test]
    fn client_disconnect_frees_entity() {
        let mut game = GameLogic::new();
        game.client_connect(0, "");
        game.client_begin(0);
        let pre_count = game.storage.count();
        game.client_disconnect(0);
        assert_eq!(game.storage.count(), pre_count - 1);
        assert!(game.player_slots[0].is_none());
    }

    #[test]
    fn run_frame_increments_framenum() {
        let mut game = GameLogic::new();
        for _ in 0..10 {
            game.run_frame();
        }
        assert_eq!(game.framenum, 10);
    }

    #[test]
    fn shutdown_resets_state() {
        let mut game = GameLogic::new();
        game.client_connect(0, "");
        game.client_begin(0);
        game.run_frame();
        game.shutdown();
        assert_eq!(game.framenum, 0);
        assert_eq!(game.storage.count(), 0);
        assert!(game.player_slots.iter().all(|s| s.is_none()));
    }
}
