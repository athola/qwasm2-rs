//! Game trait definitions — replaces C function-pointer tables
//! (`game_import_t` / `game_export_t` from `game.h`).
//!
//! Rust doesn't need `tag_malloc`/`tag_free`, so those are omitted.

use q2_shared::types::*;

// Re-export from canonical location so game code can use `traits::CVarHandle`.
pub use q2_shared::CVarHandle;

// ---------------------------------------------------------------------------
// GameImport — server callbacks available to the game module
// ---------------------------------------------------------------------------

/// Server-to-game callbacks.  Mirrors `game_import_t` from `game.h`.
///
/// # Network message pattern
///
/// The `write_*` methods append data to an implicit shared message buffer.
/// After writing the message payload, the caller **must** call either
/// [`multicast`](Self::multicast) or [`unicast`](Self::unicast) to flush and
/// deliver the buffer. Forgetting to call a send method silently discards the
/// written data.
///
/// ```text
/// gi.write_byte(SvcOp::TempEntity as i32);
/// gi.write_position(origin);
/// gi.multicast(origin, Multicast::PVS);  // ← flush
/// ```
pub trait GameImport: Send + Sync {
    // -- printing ----------------------------------------------------------

    /// Broadcast a print message to all connected clients.
    /// `printlevel`: 0 = low, 1 = medium, 2 = high, 3 = chat.
    fn bprintf(&self, printlevel: i32, msg: &str);

    /// Print a debug message to the server console only.
    fn dprintf(&self, msg: &str);

    /// Print to a specific client (or server console if `ent_idx` is `None`).
    /// `printlevel`: same as [`bprintf`](Self::bprintf).
    fn cprintf(&self, ent_idx: Option<usize>, printlevel: i32, msg: &str);

    /// Display a centered message on a client's screen.
    fn centerprintf(&self, ent_idx: Option<usize>, msg: &str);

    // -- sound -------------------------------------------------------------

    /// Play a sound effect.
    ///
    /// * `ent_idx` — entity emitting the sound (`None` for world).
    /// * `channel` — `CHAN_VOICE`, `CHAN_WEAPON`, `CHAN_ITEM`, etc. (0-7).
    /// * `sound_index` — precached sound index from [`sound_index`](Self::sound_index).
    /// * `volume` — 0.0 to 1.0.
    /// * `attenuation` — `ATTN_NONE`(0), `ATTN_NORM`(1), `ATTN_IDLE`(2), `ATTN_STATIC`(3).
    /// * `time_ofs` — delay in seconds before playing.
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

    /// Precache a model and return its configstring index.
    fn model_index(&self, name: &str) -> i32;

    /// Precache a sound and return its configstring index.
    fn sound_index(&self, name: &str) -> i32;

    /// Precache an image and return its configstring index.
    fn image_index(&self, name: &str) -> i32;

    /// Set the model on an entity (updates `configstrings[CS_MODELS + index]`).
    fn set_model(&self, ent_idx: usize, name: &str);

    // -- collision ---------------------------------------------------------

    /// Trace a bounding box through the world and entities.
    ///
    /// * `pass_ent` — entity to skip during the trace (typically self).
    /// * `content_mask` — bitfield of `CONTENTS_*` flags to collide with.
    fn trace(
        &self,
        start: Vec3f,
        mins: Vec3f,
        maxs: Vec3f,
        end: Vec3f,
        pass_ent: Option<usize>,
        content_mask: i32,
    ) -> Trace;

    /// Return the `CONTENTS_*` flags at a world point.
    fn point_contents(&self, point: Vec3f) -> i32;

    /// Check if two points are in the same Potentially Visible Set.
    fn in_pvs(&self, p1: Vec3f, p2: Vec3f) -> bool;

    /// Check if two points are in the same Potentially Hearable Set.
    fn in_phs(&self, p1: Vec3f, p2: Vec3f) -> bool;

    // -- entity linking ----------------------------------------------------

    /// Link an entity into the world spatial structure (call after changing
    /// origin, mins, maxs, or solid type).
    fn link_entity(&self, ent_idx: usize);

    /// Remove an entity from the world spatial structure.
    fn unlink_entity(&self, ent_idx: usize);

    /// Find all entity indices whose bounding boxes intersect the given region.
    ///
    /// * `area_type` — `AREA_SOLID`(1) or `AREA_TRIGGERS`(2).
    fn box_edicts(
        &self,
        mins: Vec3f,
        maxs: Vec3f,
        max_count: usize,
        area_type: i32,
    ) -> Vec<usize>;

    // -- configstrings -----------------------------------------------------

    /// Set a configstring by index (broadcast to all clients).
    fn configstring(&self, num: i32, string: &str);

    // -- network writing ---------------------------------------------------
    // These methods append to an implicit message buffer.
    // Call `multicast()` or `unicast()` after writing to send.

    /// Write a single byte to the message buffer.
    fn write_byte(&self, c: i32);

    /// Write a 16-bit integer (little-endian) to the message buffer.
    fn write_short(&self, c: i32);

    /// Write a 32-bit integer (little-endian) to the message buffer.
    fn write_long(&self, c: i32);

    /// Write a 32-bit float (little-endian IEEE 754) to the message buffer.
    fn write_float(&self, f: f32);

    /// Write a null-terminated string to the message buffer.
    fn write_string(&self, s: &str);

    /// Write a quantised 3D position (3 × 16-bit coords) to the message buffer.
    fn write_position(&self, pos: Vec3f);

    /// Write a compressed direction (1-byte index into BYTEDIRS) to the message buffer.
    fn write_dir(&self, dir: Vec3f);

    /// Write a quantised angle (1 byte, 256 steps per revolution) to the message buffer.
    fn write_angle(&self, f: f32);

    // -- multicast / unicast -----------------------------------------------

    /// Flush the message buffer to clients within a multicast region.
    fn multicast(&self, origin: Vec3f, to: Multicast);

    /// Flush the message buffer to a single client.
    /// `reliable`: if true, message is queued for reliable delivery.
    fn unicast(&self, ent_idx: usize, reliable: bool);

    // -- command args ------------------------------------------------------

    /// Number of arguments in the current client command.
    fn argc(&self) -> i32;

    /// Get argument `n` of the current client command.
    fn argv(&self, n: i32) -> String;

    /// Get all arguments (excluding argv\[0\]) as a single string.
    fn args(&self) -> String;

    // -- misc --------------------------------------------------------------

    /// Insert a command string into the server's command buffer.
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
    fn game_import_is_object_safe() {
        // Prove the trait is object-safe by constructing a trait object.
        let gi: Box<dyn GameImport> = Box::new(MockGameImport);
        // Call methods through the vtable — this tests dispatch, not return values.
        gi.bprintf(0, "broadcast");
        gi.dprintf("debug");
        gi.cprintf(Some(1), 0, "client msg");
        gi.centerprintf(Some(1), "center");
        gi.sound(Some(1), 0, 0, 1.0, 1.0, 0.0);
        gi.set_model(1, "models/test.md2");
        gi.configstring(0, "test");
        gi.link_entity(1);
        gi.unlink_entity(1);
        gi.multicast(Vec3f::ZERO, Multicast::All);
        gi.unicast(1, true);
        gi.add_command_string("test");
    }

    #[test]
    fn game_import_trace_returns_valid_default() {
        let gi: &dyn GameImport = &MockGameImport;
        let tr = gi.trace(
            Vec3f::ZERO,
            Vec3f::ZERO,
            Vec3f::ZERO,
            Vec3f::new(100.0, 0.0, 0.0),
            None,
            0,
        );
        // Trace::default() has fraction=1.0, meaning no collision
        assert_eq!(tr.fraction, 1.0);
        assert!(!tr.allsolid);
    }

    #[test]
    fn game_import_box_edicts_returns_empty() {
        let gi: &dyn GameImport = &MockGameImport;
        let edicts = gi.box_edicts(Vec3f::ZERO, Vec3f::new(100.0, 100.0, 100.0), 32, 0);
        assert!(edicts.is_empty());
    }
}
