//! Game trait definitions — replaces C function-pointer tables
//! (`game_import_t` / `game_export_t` from `game.h`).
//!
//! Rust doesn't need `tag_malloc`/`tag_free`, so those are omitted.

use q2_shared::types::*;

// ---------------------------------------------------------------------------
// EntityLink — the edict fields SV_LinkEdict reads
// ---------------------------------------------------------------------------

/// What the server needs to place an entity in its spatial tree and clip
/// traces against it. In C the server reads these fields straight out of the
/// shared `edict_t`; here the game copies them out when it links.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EntityLink {
    /// Entity number (index into C `g_edicts`).
    pub number: usize,
    pub origin: Vec3f,
    pub angles: Vec3f,
    pub mins: Vec3f,
    pub maxs: Vec3f,
    /// World-space bounds, already expanded by one unit as SV_LinkEdict does.
    pub absmin: Vec3f,
    pub absmax: Vec3f,
    pub solid: Solid,
    /// Inline model index for `Solid::Bsp` entities (`*n` -> `n + 1`).
    pub modelindex: i32,
    /// Number of the owning entity; traces from the owner pass through.
    pub owner: Option<usize>,
    /// `SVF_*` flags (`SVF_DEADMONSTER` changes how traces clip).
    pub svflags: u32,
}

/// What the server reads of one in-use edict when it builds a client's
/// frame. C `SV_BuildClientFrame` reads `ent->s`, `svflags`, `owner` and
/// the link state straight out of `ge->edicts`; here the game lends them
/// by entity number.
#[derive(Debug, Clone, Copy)]
pub struct EdictView<'a> {
    pub state: &'a EntityState,
    pub svflags: u32,
    /// Edict number of its owner (C `ent->owner`).
    pub owner: Option<usize>,
    /// Whether it has been linked since it was spawned. C `G_FreeEdict`
    /// zeroes `num_clusters`, so an edict that is freed and spawned again
    /// is in no PVS until it is linked; the server's cluster list for the
    /// number still holds the previous occupant's clusters.
    pub linked: bool,
}

/// What `GameImport::set_model` reports back.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ModelInfo {
    /// Configstring index under `CS_MODELS` (0 when the table is full).
    pub index: i32,
    /// `(mins, maxs)` of an inline model; `None` for file models.
    pub bounds: Option<(Vec3f, Vec3f)>,
}

/// `GameImport::box_edicts` area types (C `AREA_SOLID`/`AREA_TRIGGERS`).
/// Defined once in q2-shared so both sides of the boundary agree.
pub use q2_shared::constants::{AREA_SOLID, AREA_TRIGGERS};

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
pub trait GameImport {
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

    /// Play a sound at a fixed `origin` on `ent_idx`'s channel (C
    /// `gi.positioned_sound`, `SV_StartSound` with an origin): for
    /// entities clients are not sent, and world splashes.
    #[allow(clippy::too_many_arguments)]
    fn positioned_sound(
        &self,
        origin: Vec3f,
        ent_idx: usize,
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

    /// C PF_setmodel's server half: register `name` and, for an inline
    /// model (`*n`), report its bounds. The game owns its entities, so
    /// `GameWorld::set_model` writes them and links.
    fn set_model(&self, ent_idx: usize, name: &str) -> ModelInfo;

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

    // -- area portals ------------------------------------------------------

    /// Open or close area portal `portal_num` (a func_areaportal's
    /// `style`). C ref: `gi.SetAreaPortalState`. The default does nothing,
    /// for test doubles without a map; the server implements it.
    fn set_area_portal_state(&self, _portal_num: i32, _open: bool) {}

    /// Whether two areas are joined through open portals. C ref:
    /// `gi.AreasConnected`. The default, for test doubles without a map,
    /// joins every area (C's `map_noareas`).
    fn areas_connected(&self, _area1: i32, _area2: i32) -> bool {
        true
    }

    /// The areas entity `number` touched at its last link, 0 for none: the
    /// `areanum`/`areanum2` C SV_LinkEdict writes into the shared edict,
    /// which the game reads back after linking. The default, for test
    /// doubles without a map, puts everything in no area.
    fn entity_areas(&self, _number: usize) -> (i32, i32) {
        (0, 0)
    }

    // -- entity linking ----------------------------------------------------

    /// Link an entity into the world spatial structure (call after changing
    /// origin, mins, maxs, or solid type). Replaces any earlier link for the
    /// same number.
    fn link_entity(&self, ent: &EntityLink);

    /// Remove entity `number` from the world spatial structure.
    fn unlink_entity(&self, number: usize);

    /// Find all entity indices whose bounding boxes intersect the given region.
    ///
    /// * `area_type` — `AREA_SOLID`(1) or `AREA_TRIGGERS`(2).
    /// * `max_count` — Implementations **must** return at most this many results
    ///   (truncate the result vec to `max_count`).
    fn box_edicts(&self, mins: Vec3f, maxs: Vec3f, max_count: usize, area_type: i32) -> Vec<usize>;

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
pub trait GameExport {
    /// API version (must be `GAME_API_VERSION == 3`).
    fn api_version(&self) -> i32 {
        3
    }

    /// C `InitGame`. The import is supplied when the game is constructed,
    /// since the game keeps calling it every frame.
    fn init(&mut self);
    fn shutdown(&mut self);

    /// Called when a new map is loaded.
    fn spawn_entities(&mut self, mapname: &str, entstring: &str, spawnpoint: &str);

    // Client calls take the client's entity number, `slot + 1`, as C passes
    // `EDICT_NUM(slot + 1)`; entity 0 is the world.

    /// Returns `true` if the client is allowed to connect.
    fn client_connect(&mut self, number: usize, userinfo: &str) -> bool;
    fn client_begin(&mut self, number: usize);
    fn client_disconnect(&mut self, number: usize);
    fn client_command(&mut self, number: usize);
    fn client_think(&mut self, number: usize, cmd: &UserCmd);

    /// Run one game frame (10 Hz by default in Q2).
    fn run_frame(&mut self);

    /// Handle `sv <command>` on the server console.
    fn server_command(&mut self);

    // The `ge->edicts` read surface: what C's server reads of the game's
    // entities directly.

    /// One past the highest entity number the game has used this map (C
    /// `ge->num_edicts`, which freeing an edict never lowers).
    fn num_edicts(&self) -> usize;

    /// Edict `number` if it is in use. A freed edict reads as `None`: C
    /// `G_FreeEdict` zeroes it, so the server sends nothing for it.
    fn edict(&self, number: usize) -> Option<EdictView<'_>>;

    /// The player state of the client at entity `number` (C
    /// `ent->client->ps`), `None` if that edict has no client.
    fn client_player_state(&self, number: usize) -> Option<&PlayerState>;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A no-op mock that proves `GameImport` is object-safe and
    /// implementable. The `Rc` marker makes it `!Send + !Sync`, so the tests
    /// also prove the trait does not demand thread-safety the single-threaded
    /// engine never uses.
    #[derive(Default)]
    struct NoopImport {
        _not_send: std::marker::PhantomData<std::rc::Rc<()>>,
    }

    impl GameImport for NoopImport {
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
        fn positioned_sound(&self, _: Vec3f, _: usize, _: i32, _: i32, _: f32, _: f32, _: f32) {}
        fn model_index(&self, _name: &str) -> i32 {
            0
        }
        fn sound_index(&self, _name: &str) -> i32 {
            0
        }
        fn image_index(&self, _name: &str) -> i32 {
            0
        }
        fn set_model(&self, _ent_idx: usize, _name: &str) -> ModelInfo {
            ModelInfo::default()
        }
        fn trace(
            &self,
            _start: Vec3f,
            _mins: Vec3f,
            _maxs: Vec3f,
            end: Vec3f,
            _pass_ent: Option<usize>,
            _content_mask: i32,
        ) -> Trace {
            Trace {
                endpos: end,
                ..Trace::default()
            }
        }
        fn point_contents(&self, _point: Vec3f) -> i32 {
            0
        }
        fn in_pvs(&self, _p1: Vec3f, _p2: Vec3f) -> bool {
            false
        }
        fn in_phs(&self, _p1: Vec3f, _p2: Vec3f) -> bool {
            false
        }
        fn link_entity(&self, _ent: &EntityLink) {}
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
        fn argc(&self) -> i32 {
            0
        }
        fn argv(&self, _n: i32) -> String {
            String::new()
        }
        fn args(&self) -> String {
            String::new()
        }
        fn add_command_string(&self, _text: &str) {}
    }

    #[test]
    fn game_import_is_object_safe() {
        // Prove the trait is object-safe by constructing a trait object.
        let gi: Box<dyn GameImport> = Box::new(NoopImport::default());
        // Call methods through the vtable — this tests dispatch, not return values.
        gi.bprintf(0, "broadcast");
        gi.dprintf("debug");
        gi.cprintf(Some(1), 0, "client msg");
        gi.centerprintf(Some(1), "center");
        gi.sound(Some(1), 0, 0, 1.0, 1.0, 0.0);
        gi.set_model(1, "models/test.md2");
        gi.configstring(0, "test");
        gi.link_entity(&EntityLink::default());
        gi.unlink_entity(1);
        gi.multicast(Vec3f::ZERO, Multicast::All);
        gi.unicast(1, true);
        gi.add_command_string("test");
    }
}
