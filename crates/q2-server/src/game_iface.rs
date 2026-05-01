//! Server-side GameImport implementation.
//!
//! `ServerGameImport` implements `q2_game::traits::GameImport` so the game
//! module can call back into the server during a game frame.
//!
//! Interior-mutable state (message buffer, configstring updates) is guarded by
//! a `Mutex` to satisfy the `Send + Sync` bound on `GameImport`.  In the
//! single-threaded WASM target the lock is always uncontended.

use std::sync::Mutex;

use q2_game::traits::GameImport;
use q2_shared::{constants::MAX_CONFIGSTRINGS, types::*};

// ---------------------------------------------------------------------------
// Interior-mutable state
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct GiInner {
    /// Outbound message buffer accumulating write_* calls; discarded (not sent)
    /// when multicast/unicast is called until proper routing is implemented.
    msg_buf: Vec<u8>,
    /// Mirror of Server::configstrings, updated by game calls to configstring().
    configstrings: Vec<String>,
    /// Tracks which configstring slots were written since the last drain.
    dirty: Vec<bool>,
}

impl GiInner {
    fn new() -> Self {
        Self {
            msg_buf: Vec::new(),
            configstrings: vec![String::new(); MAX_CONFIGSTRINGS],
            dirty: vec![false; MAX_CONFIGSTRINGS],
        }
    }
}

// ---------------------------------------------------------------------------
// ServerGameImport
// ---------------------------------------------------------------------------

/// Server implementation of the `GameImport` trait.
pub struct ServerGameImport {
    inner: Mutex<GiInner>,
}

impl std::fmt::Debug for ServerGameImport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerGameImport").finish_non_exhaustive()
    }
}

impl ServerGameImport {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(GiInner::new()),
        }
    }

    /// Drain configstring updates written since the last call.
    ///
    /// Returns `(index, value)` pairs for every slot that was written via
    /// `configstring()`, including slots cleared to an empty string (valid Q2
    /// protocol to erase a slot).  Clears the dirty flags so subsequent calls
    /// return only new writes.
    pub fn drain_configstring_updates(&self) -> Vec<(usize, String)> {
        let mut inner = self.inner.lock().unwrap();
        let updates: Vec<(usize, String)> = inner
            .dirty
            .iter()
            .enumerate()
            .filter_map(|(i, &d)| if d { Some((i, inner.configstrings[i].clone())) } else { None })
            .collect();
        for (i, _) in &updates {
            inner.dirty[*i] = false;
        }
        updates
    }
}

impl Default for ServerGameImport {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// GameImport impl
// ---------------------------------------------------------------------------

impl GameImport for ServerGameImport {
    // -- printing ------------------------------------------------------------

    fn bprintf(&self, _printlevel: i32, msg: &str) {
        tracing::info!("[bprintf] {}", msg.trim_end());
    }

    fn dprintf(&self, msg: &str) {
        tracing::debug!("[dprintf] {}", msg.trim_end());
    }

    fn cprintf(&self, ent_idx: Option<usize>, _printlevel: i32, msg: &str) {
        tracing::debug!(ent = ?ent_idx, "[cprintf] {}", msg.trim_end());
    }

    fn centerprintf(&self, ent_idx: Option<usize>, msg: &str) {
        tracing::debug!(ent = ?ent_idx, "[centerprintf] {}", msg.trim_end());
    }

    // -- sound ---------------------------------------------------------------

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

    // -- asset indexing ------------------------------------------------------

    fn model_index(&self, _name: &str) -> i32 {
        0
    }

    fn sound_index(&self, _name: &str) -> i32 {
        0
    }

    fn image_index(&self, _name: &str) -> i32 {
        0
    }

    fn set_model(&self, _ent_idx: usize, _name: &str) {}

    // -- collision -----------------------------------------------------------

    fn trace(
        &self,
        _start: Vec3f,
        _mins: Vec3f,
        _maxs: Vec3f,
        _end: Vec3f,
        _pass_ent: Option<usize>,
        _content_mask: i32,
    ) -> Trace {
        // CP-3: no BSP loaded — return an unobstructed trace (fraction = 1.0).
        Trace::default()
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

    // -- entity linking ------------------------------------------------------

    fn link_entity(&self, _ent_idx: usize) {
        // TODO: implement once EntityStorage is accessible here (tracked in #31).
    }

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

    // -- configstrings -------------------------------------------------------

    fn configstring(&self, num: i32, string: &str) {
        if num < 0 {
            return;
        }
        let idx = num as usize;
        let mut inner = self.inner.lock().unwrap();
        if idx < inner.configstrings.len() {
            inner.configstrings[idx] = string.to_string();
            inner.dirty[idx] = true;
        }
    }

    // -- network writing (buffered; flushed on multicast/unicast) -----------

    fn write_byte(&self, c: i32) {
        self.inner.lock().unwrap().msg_buf.push(c as u8);
    }

    fn write_short(&self, c: i32) {
        let mut inner = self.inner.lock().unwrap();
        let bytes = (c as i16).to_le_bytes();
        inner.msg_buf.extend_from_slice(&bytes);
    }

    fn write_long(&self, c: i32) {
        let mut inner = self.inner.lock().unwrap();
        inner.msg_buf.extend_from_slice(&c.to_le_bytes());
    }

    fn write_float(&self, f: f32) {
        let mut inner = self.inner.lock().unwrap();
        inner.msg_buf.extend_from_slice(&f.to_le_bytes());
    }

    fn write_string(&self, s: &str) {
        let mut inner = self.inner.lock().unwrap();
        inner.msg_buf.extend_from_slice(s.as_bytes());
        inner.msg_buf.push(0); // null-terminate
    }

    fn write_position(&self, pos: Vec3f) {
        let mut inner = self.inner.lock().unwrap();
        let encode = |f: f32| -> i16 { (f * 8.0) as i16 };
        inner
            .msg_buf
            .extend_from_slice(&encode(pos.x).to_le_bytes());
        inner
            .msg_buf
            .extend_from_slice(&encode(pos.y).to_le_bytes());
        inner
            .msg_buf
            .extend_from_slice(&encode(pos.z).to_le_bytes());
    }

    fn write_dir(&self, dir: Vec3f) {
        // Stub: should quantise to the nearest BYTEDIRS entry (162 directions).
        // Writing byte 0 keeps the stream aligned until quantization is wired up.
        let _ = dir;
        self.inner.lock().unwrap().msg_buf.push(0);
    }

    fn write_angle(&self, f: f32) {
        let byte = ((f * 256.0 / 360.0) as i32 & 255) as u8;
        self.inner.lock().unwrap().msg_buf.push(byte);
    }

    // -- multicast / unicast (flush message buffer) -------------------------

    fn multicast(&self, _origin: Vec3f, _to: Multicast) {
        self.inner.lock().unwrap().msg_buf.clear();
    }

    fn unicast(&self, _ent_idx: usize, _reliable: bool) {
        self.inner.lock().unwrap().msg_buf.clear();
    }

    // -- command args --------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configstring_stored() {
        let gi = ServerGameImport::new();
        gi.configstring(0, "base1");
        gi.configstring(1, "models/world.bsp");

        let updates = gi.drain_configstring_updates();
        let map: std::collections::HashMap<_, _> = updates.into_iter().collect();
        assert_eq!(map[&0], "base1");
        assert_eq!(map[&1], "models/world.bsp");
    }

    #[test]
    fn write_byte_then_unicast_clears_buf() {
        let gi = ServerGameImport::new();
        gi.write_byte(42);
        gi.write_byte(99);
        {
            let inner = gi.inner.lock().unwrap();
            assert_eq!(inner.msg_buf.len(), 2);
        }
        gi.unicast(0, true);
        {
            let inner = gi.inner.lock().unwrap();
            assert!(inner.msg_buf.is_empty());
        }
    }

    #[test]
    fn write_string_null_terminates() {
        let gi = ServerGameImport::new();
        gi.write_string("hello");
        let inner = gi.inner.lock().unwrap();
        assert_eq!(inner.msg_buf.last(), Some(&0u8));
    }

    #[test]
    fn send_sync_bound() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ServerGameImport>();
    }

    #[test]
    fn write_short_encodes_little_endian() {
        let gi = ServerGameImport::new();
        gi.write_short(0x1234);
        let inner = gi.inner.lock().unwrap();
        assert_eq!(&inner.msg_buf[..], &0x1234i16.to_le_bytes());
    }

    #[test]
    fn write_long_encodes_little_endian() {
        let gi = ServerGameImport::new();
        gi.write_long(0x0A0B_0C0D);
        let inner = gi.inner.lock().unwrap();
        assert_eq!(&inner.msg_buf[..], &0x0A0B_0C0Di32.to_le_bytes());
    }

    #[test]
    fn write_float_encodes_little_endian() {
        let gi = ServerGameImport::new();
        gi.write_float(1.5);
        let inner = gi.inner.lock().unwrap();
        assert_eq!(&inner.msg_buf[..], &1.5f32.to_le_bytes());
    }

    #[test]
    fn write_position_scales_by_8() {
        let gi = ServerGameImport::new();
        gi.write_position(q2_shared::types::Vec3f::new(16.0, -8.0, 0.5));
        let inner = gi.inner.lock().unwrap();
        // coord wire format: (f * 8.0) as i16, three components
        let x: i16 = i16::from_le_bytes([inner.msg_buf[0], inner.msg_buf[1]]);
        let y: i16 = i16::from_le_bytes([inner.msg_buf[2], inner.msg_buf[3]]);
        let z: i16 = i16::from_le_bytes([inner.msg_buf[4], inner.msg_buf[5]]);
        assert_eq!(x, (16.0f32 * 8.0) as i16);
        assert_eq!(y, (-8.0f32 * 8.0) as i16);
        assert_eq!(z, (0.5f32 * 8.0) as i16);
    }

    #[test]
    fn write_angle_quantizes_correctly() {
        let gi = ServerGameImport::new();
        // 90 degrees → (90 * 256 / 360) & 255 = 64
        gi.write_angle(90.0);
        let inner = gi.inner.lock().unwrap();
        let expected = ((90.0f32 * 256.0 / 360.0) as i32 & 255) as u8;
        assert_eq!(inner.msg_buf[0], expected);
    }

    #[test]
    fn multicast_clears_buf() {
        let gi = ServerGameImport::new();
        gi.write_byte(1);
        gi.write_byte(2);
        gi.multicast(
            q2_shared::types::Vec3f::new(0.0, 0.0, 0.0),
            q2_shared::types::Multicast::All,
        );
        let inner = gi.inner.lock().unwrap();
        assert!(inner.msg_buf.is_empty());
    }

    #[test]
    fn configstring_negative_index_ignored() {
        let gi = ServerGameImport::new();
        gi.configstring(-1, "should_not_store");
        let updates = gi.drain_configstring_updates();
        assert!(updates.is_empty());
    }

    #[test]
    fn configstring_oob_index_ignored() {
        let gi = ServerGameImport::new();
        gi.configstring(MAX_CONFIGSTRINGS as i32 + 10, "oob");
        let updates = gi.drain_configstring_updates();
        assert!(updates.is_empty());
    }

    #[test]
    fn configstring_overwrite_returns_latest() {
        let gi = ServerGameImport::new();
        gi.configstring(3, "first");
        gi.configstring(3, "second");
        let updates = gi.drain_configstring_updates();
        let found = updates
            .iter()
            .find(|(i, _)| *i == 3)
            .map(|(_, v)| v.as_str());
        assert_eq!(found, Some("second"));
    }
}
