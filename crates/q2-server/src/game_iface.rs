//! Server-side GameImport implementation.
//!
//! `ServerGameImport` implements `q2_game::traits::GameImport` so the game
//! module can call back into the server during a game frame.
//!
//! `GameImport` methods take `&self`, so the message buffer and configstring
//! mirror live in a `RefCell`. The engine is single-threaded, so the trait
//! carries no `Send + Sync` bound and no lock is needed.

use std::cell::RefCell;

use q2_common::net_msg::NetMsg;
use q2_game::traits::{EntityLink, GameImport};
use q2_shared::{constants::MAX_CONFIGSTRINGS, types::*};

// ---------------------------------------------------------------------------
// Interior-mutable state
// ---------------------------------------------------------------------------

struct GiInner {
    /// Outbound message buffer accumulating write_* calls; discarded (not sent)
    /// when multicast/unicast is called until proper routing is implemented.
    /// Uses `NetMsg` so game-written temp entities encode byte-for-byte like
    /// every other server message (MSG_Write* in C).
    msg_buf: NetMsg,
    /// Game-written configstrings; `Server::frame` copies dirty slots into
    /// `Server::configstrings` after each game frame.
    configstrings: Vec<String>,
    /// Tracks which configstring slots were written since the last drain.
    dirty: Vec<bool>,
}

impl GiInner {
    fn new() -> Self {
        Self {
            msg_buf: NetMsg::new(),
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
    inner: RefCell<GiInner>,
}

impl std::fmt::Debug for ServerGameImport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerGameImport").finish_non_exhaustive()
    }
}

impl ServerGameImport {
    pub fn new() -> Self {
        Self {
            inner: RefCell::new(GiInner::new()),
        }
    }

    /// Drain configstring updates written since the last call.
    ///
    /// Returns `(index, value)` pairs for every slot that was written via
    /// `configstring()`, including slots cleared to an empty string (valid Q2
    /// protocol to erase a slot).  Clears the dirty flags so subsequent calls
    /// return only new writes.
    pub fn drain_configstring_updates(&self) -> Vec<(usize, String)> {
        let mut inner = self.inner.borrow_mut();
        let updates: Vec<(usize, String)> = inner
            .dirty
            .iter()
            .enumerate()
            .filter_map(|(i, &d)| {
                if d {
                    Some((i, inner.configstrings[i].clone()))
                } else {
                    None
                }
            })
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
        end: Vec3f,
        _pass_ent: Option<usize>,
        _content_mask: i32,
    ) -> Trace {
        // CP-3: no BSP loaded — return an unobstructed trace. A trace that
        // hits nothing reaches `end` (fraction 1.0), as in C SV_Trace.
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

    // -- entity linking ------------------------------------------------------

    fn link_entity(&self, _ent: &EntityLink) {
        // TODO(#31): store the link in the area tree for trace clipping.
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
        let mut inner = self.inner.borrow_mut();
        if idx < inner.configstrings.len() {
            inner.configstrings[idx] = string.to_string();
            inner.dirty[idx] = true;
        }
    }

    // -- network writing (buffered; flushed on multicast/unicast) -----------

    fn write_byte(&self, c: i32) {
        self.inner.borrow_mut().msg_buf.write_byte(c);
    }

    fn write_short(&self, c: i32) {
        self.inner.borrow_mut().msg_buf.write_short(c);
    }

    fn write_long(&self, c: i32) {
        self.inner.borrow_mut().msg_buf.write_long(c);
    }

    fn write_float(&self, f: f32) {
        self.inner.borrow_mut().msg_buf.write_float(f);
    }

    fn write_string(&self, s: &str) {
        self.inner.borrow_mut().msg_buf.write_string(s);
    }

    fn write_position(&self, pos: Vec3f) {
        self.inner.borrow_mut().msg_buf.write_position(pos);
    }

    fn write_dir(&self, dir: Vec3f) {
        self.inner.borrow_mut().msg_buf.write_dir(dir);
    }

    fn write_angle(&self, f: f32) {
        self.inner.borrow_mut().msg_buf.write_angle(f);
    }

    // -- multicast / unicast (flush message buffer) -------------------------

    fn multicast(&self, _origin: Vec3f, _to: Multicast) {
        self.inner.borrow_mut().msg_buf.clear();
    }

    fn unicast(&self, _ent_idx: usize, _reliable: bool) {
        self.inner.borrow_mut().msg_buf.clear();
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
    fn unobstructed_trace_ends_at_requested_end_point() {
        let gi = ServerGameImport::new();
        let end = Vec3f::new(100.0, -20.0, 8.0);
        let tr = gi.trace(Vec3f::ZERO, Vec3f::ZERO, Vec3f::ZERO, end, None, 0);
        assert_eq!(tr.fraction, 1.0);
        assert_eq!(tr.endpos, end);
        assert!(!tr.allsolid && !tr.startsolid);
    }

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
            let inner = gi.inner.borrow();
            assert_eq!(inner.msg_buf.len(), 2);
        }
        gi.unicast(0, true);
        {
            let inner = gi.inner.borrow();
            assert!(inner.msg_buf.is_empty());
        }
    }

    #[test]
    fn write_string_null_terminates() {
        let gi = ServerGameImport::new();
        gi.write_string("hello");
        let inner = gi.inner.borrow();
        assert_eq!(inner.msg_buf.data().last(), Some(&0u8));
    }

    #[test]
    fn configstring_still_writable_after_panic_during_buffer_access() {
        let gi = ServerGameImport::new();
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = gi.inner.borrow_mut();
            panic!("game callback panicked while the buffer was borrowed");
        }));
        assert!(caught.is_err());

        gi.configstring(2, "after_panic");
        let updates = gi.drain_configstring_updates();
        assert_eq!(updates, vec![(2, "after_panic".to_string())]);
    }

    #[test]
    fn write_short_encodes_little_endian() {
        let gi = ServerGameImport::new();
        gi.write_short(0x1234);
        let inner = gi.inner.borrow();
        assert_eq!(inner.msg_buf.data(), &0x1234i16.to_le_bytes());
    }

    #[test]
    fn write_long_encodes_little_endian() {
        let gi = ServerGameImport::new();
        gi.write_long(0x0A0B_0C0D);
        let inner = gi.inner.borrow();
        assert_eq!(inner.msg_buf.data(), &0x0A0B_0C0Di32.to_le_bytes());
    }

    #[test]
    fn write_float_encodes_little_endian() {
        let gi = ServerGameImport::new();
        gi.write_float(1.5);
        let inner = gi.inner.borrow();
        assert_eq!(inner.msg_buf.data(), &1.5f32.to_le_bytes());
    }

    #[test]
    fn write_position_scales_by_8() {
        let gi = ServerGameImport::new();
        gi.write_position(q2_shared::types::Vec3f::new(16.0, -8.0, 0.5));
        let inner = gi.inner.borrow();
        // coord wire format: (int)(f * 8) written as a short, three components
        let x: i16 = i16::from_le_bytes([inner.msg_buf.data()[0], inner.msg_buf.data()[1]]);
        let y: i16 = i16::from_le_bytes([inner.msg_buf.data()[2], inner.msg_buf.data()[3]]);
        let z: i16 = i16::from_le_bytes([inner.msg_buf.data()[4], inner.msg_buf.data()[5]]);
        assert_eq!(x, (16.0f32 * 8.0) as i16);
        assert_eq!(y, (-8.0f32 * 8.0) as i16);
        assert_eq!(z, (0.5f32 * 8.0) as i16);
    }

    fn written(gi: &ServerGameImport) -> Vec<u8> {
        gi.inner.borrow().msg_buf.data().to_vec()
    }

    fn netmsg_bytes(write: impl FnOnce(&mut q2_common::net_msg::NetMsg)) -> Vec<u8> {
        let mut msg = q2_common::net_msg::NetMsg::new();
        write(&mut msg);
        msg.data().to_vec()
    }

    #[test]
    fn write_dir_up_encodes_bytedirs_index_5() {
        let gi = ServerGameImport::new();
        gi.write_dir(Vec3f::new(0.0, 0.0, 1.0));
        assert_eq!(&written(&gi)[..], &[5u8]);
    }

    #[test]
    fn write_dir_matches_netmsg_encoding() {
        let dirs = [
            Vec3f::new(1.0, 0.0, 0.0),
            Vec3f::new(0.0, -1.0, 0.0),
            Vec3f::new(0.0, 0.0, -1.0),
            Vec3f::new(0.6, 0.0, 0.8),
            Vec3f::new(-0.48, 0.36, 0.8),
        ];
        for dir in dirs {
            let gi = ServerGameImport::new();
            gi.write_dir(dir);
            let expected = netmsg_bytes(|m| m.write_dir(dir));
            assert_eq!(&written(&gi)[..], &expected[..], "dir {dir:?}");
        }
    }

    #[test]
    fn write_position_beyond_4096_wraps_like_netmsg() {
        let pos = Vec3f::new(5000.0, -5000.0, 4095.875);
        let gi = ServerGameImport::new();
        gi.write_position(pos);
        let expected = netmsg_bytes(|m| m.write_position(pos));
        assert_eq!(&written(&gi)[..], &expected[..]);
    }

    #[test]
    fn write_angle_quantizes_correctly() {
        let gi = ServerGameImport::new();
        // 90 degrees → (90 * 256 / 360) & 255 = 64
        gi.write_angle(90.0);
        let inner = gi.inner.borrow();
        let expected = ((90.0f32 * 256.0 / 360.0) as i32 & 255) as u8;
        assert_eq!(inner.msg_buf.data()[0], expected);
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
        let inner = gi.inner.borrow();
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
