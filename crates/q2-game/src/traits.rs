//! Game trait definitions — replaces C function-pointer tables
//! (`game_import_t` / `game_export_t` from `game.h`).
//!
//! Rust doesn't need `tag_malloc`/`tag_free`, so those are omitted.

use q2_shared::types::*;

/// Handle to a cvar (opaque index).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CVarHandle(pub u32);

// ---------------------------------------------------------------------------
// GameImport — server callbacks available to the game module
// ---------------------------------------------------------------------------

/// Server-to-game callbacks.  Mirrors `game_import_t` from `game.h`.
pub trait GameImport: Send + Sync {
    // -- printing ----------------------------------------------------------
    fn bprintf(&self, printlevel: i32, msg: &str);
    fn dprintf(&self, msg: &str);
    fn cprintf(&self, ent_idx: Option<usize>, printlevel: i32, msg: &str);
    fn centerprintf(&self, ent_idx: Option<usize>, msg: &str);

    // -- sound -------------------------------------------------------------
    fn sound(
        &self,
        ent_idx: Option<usize>,
        channel: i32,
        sound_index: i32,
        volume: f32,
        attenuation: f32,
        time_ofs: f32,
    );

    // -- asset indexing ----------------------------------------------------
    fn model_index(&self, name: &str) -> i32;
    fn sound_index(&self, name: &str) -> i32;
    fn image_index(&self, name: &str) -> i32;

    fn set_model(&self, ent_idx: usize, name: &str);

    // -- collision ---------------------------------------------------------
    fn trace(
        &self,
        start: Vec3f,
        mins: Vec3f,
        maxs: Vec3f,
        end: Vec3f,
        pass_ent: Option<usize>,
        content_mask: i32,
    ) -> Trace;
    fn point_contents(&self, point: Vec3f) -> i32;
    fn in_pvs(&self, p1: Vec3f, p2: Vec3f) -> bool;
    fn in_phs(&self, p1: Vec3f, p2: Vec3f) -> bool;

    // -- entity linking ----------------------------------------------------
    fn link_entity(&self, ent_idx: usize);
    fn unlink_entity(&self, ent_idx: usize);
    fn box_edicts(
        &self,
        mins: Vec3f,
        maxs: Vec3f,
        max_count: usize,
        area_type: i32,
    ) -> Vec<usize>;

    // -- configstrings -----------------------------------------------------
    fn configstring(&self, num: i32, string: &str);

    // -- network writing ---------------------------------------------------
    fn write_byte(&self, c: i32);
    fn write_short(&self, c: i32);
    fn write_long(&self, c: i32);
    fn write_float(&self, f: f32);
    fn write_string(&self, s: &str);
    fn write_position(&self, pos: Vec3f);
    fn write_dir(&self, dir: Vec3f);
    fn write_angle(&self, f: f32);

    // -- multicast / unicast -----------------------------------------------
    fn multicast(&self, origin: Vec3f, to: Multicast);
    fn unicast(&self, ent_idx: usize, reliable: bool);

    // -- command args ------------------------------------------------------
    fn argc(&self) -> i32;
    fn argv(&self, n: i32) -> String;
    fn args(&self) -> String;

    // -- misc --------------------------------------------------------------
    fn add_command_string(&self, text: &str);
}

// ---------------------------------------------------------------------------
// GameExport — functions the game module exposes to the server
// ---------------------------------------------------------------------------

/// Game-to-server interface.  Mirrors `game_export_t` from `game.h`.
pub trait GameExport: Send + Sync {
    /// API version (must be `GAME_API_VERSION == 3`).
    fn api_version(&self) -> i32 {
        3
    }

    fn init(&mut self, import: &dyn GameImport);
    fn shutdown(&mut self);

    /// Called when a new map is loaded.
    fn spawn_entities(&mut self, mapname: &str, entstring: &str, spawnpoint: &str);

    /// Returns `true` if the client is allowed to connect.
    fn client_connect(&mut self, ent_idx: usize, userinfo: &str) -> bool;
    fn client_begin(&mut self, ent_idx: usize);
    fn client_disconnect(&mut self, ent_idx: usize);
    fn client_command(&mut self, ent_idx: usize);
    fn client_think(&mut self, ent_idx: usize, cmd: &UserCmd);

    /// Run one game frame (10 Hz by default in Q2).
    fn run_frame(&mut self);

    /// Handle `sv <command>` on the server console.
    fn server_command(&mut self);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A no-op mock that proves `GameImport` is object-safe and
    /// implementable.
    struct MockGameImport;

    impl GameImport for MockGameImport {
        fn bprintf(&self, _printlevel: i32, _msg: &str) {}
        fn dprintf(&self, _msg: &str) {}
        fn cprintf(&self, _ent_idx: Option<usize>, _printlevel: i32, _msg: &str) {}
        fn centerprintf(&self, _ent_idx: Option<usize>, _msg: &str) {}
        fn sound(
            &self,
            _ent_idx: Option<usize>,
            _channel: i32,
            _sound_index: i32,
            _volume: f32,
            _attenuation: f32,
            _time_ofs: f32,
        ) {
        }
        fn model_index(&self, _name: &str) -> i32 { 0 }
        fn sound_index(&self, _name: &str) -> i32 { 0 }
        fn image_index(&self, _name: &str) -> i32 { 0 }
        fn set_model(&self, _ent_idx: usize, _name: &str) {}
        fn trace(
            &self,
            _start: Vec3f,
            _mins: Vec3f,
            _maxs: Vec3f,
            _end: Vec3f,
            _pass_ent: Option<usize>,
            _content_mask: i32,
        ) -> Trace {
            Trace::default()
        }
        fn point_contents(&self, _point: Vec3f) -> i32 { 0 }
        fn in_pvs(&self, _p1: Vec3f, _p2: Vec3f) -> bool { false }
        fn in_phs(&self, _p1: Vec3f, _p2: Vec3f) -> bool { false }
        fn link_entity(&self, _ent_idx: usize) {}
        fn unlink_entity(&self, _ent_idx: usize) {}
        fn box_edicts(
            &self,
            _mins: Vec3f,
            _maxs: Vec3f,
            _max_count: usize,
            _area_type: i32,
        ) -> Vec<usize> {
            Vec::new()
        }
        fn configstring(&self, _num: i32, _string: &str) {}
        fn write_byte(&self, _c: i32) {}
        fn write_short(&self, _c: i32) {}
        fn write_long(&self, _c: i32) {}
        fn write_float(&self, _f: f32) {}
        fn write_string(&self, _s: &str) {}
        fn write_position(&self, _pos: Vec3f) {}
        fn write_dir(&self, _dir: Vec3f) {}
        fn write_angle(&self, _f: f32) {}
        fn multicast(&self, _origin: Vec3f, _to: Multicast) {}
        fn unicast(&self, _ent_idx: usize, _reliable: bool) {}
        fn argc(&self) -> i32 { 0 }
        fn argv(&self, _n: i32) -> String { String::new() }
        fn args(&self) -> String { String::new() }
        fn add_command_string(&self, _text: &str) {}
    }

    #[test]
    fn mock_game_import() {
        let gi = MockGameImport;

        // Verify trait-object construction works (object safety).
        let gi_ref: &dyn GameImport = &gi;
        gi_ref.dprintf("hello");
        assert_eq!(gi_ref.argc(), 0);
        assert_eq!(gi_ref.model_index("models/test.md2"), 0);
    }
}
