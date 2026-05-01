//! Network message buffer — replaces C's `sizebuf_t` + all `MSG_Write*`/`MSG_Read*` functions.
//!
//! This is the binary wire format for the Quake 2 network protocol.
//! A cursor-based `Vec<u8>` buffer that supports both writing and reading.
//!
//! # Design note
//!
//! `NetMsg` combines writing and reading into a single type, mirroring the
//! original C `sizebuf_t`. In practice a given `NetMsg` instance is used
//! for either writing (server building a packet) or reading (client parsing
//! a received packet), never both simultaneously. A future refactor could
//! split this into `MsgWriter` / `MsgReader` for compile-time enforcement.

use q2_shared::constants::MAX_STATS;
use q2_shared::protocol::{PlayerStateFlags, SvcOp, UpdateFlags};
use q2_shared::types::{EntityState, PlayerState, UserCmd, Vec3f};

// ---------------------------------------------------------------------------
// BYTEDIRS — 162 pre-computed vertex normals for direction compression
// Taken from the Quake 2 source (anorms.h / NUMVERTEXNORMALS).
// ---------------------------------------------------------------------------

const NUM_VERTEX_NORMALS: usize = 162;

#[rustfmt::skip]
const BYTEDIRS: [[f32; 3]; NUM_VERTEX_NORMALS] = [
    [-0.525731, 0.000000, 0.850651], [-0.442863, 0.238856, 0.864188],
    [-0.295242, 0.000000, 0.955423], [-0.309017, 0.500000, 0.809017],
    [-0.162460, 0.262866, 0.951056], [0.000000, 0.000000, 1.000000],
    [0.000000, 0.850651, 0.525731], [-0.147621, 0.716567, 0.681718],
    [0.147621, 0.716567, 0.681718], [0.000000, 0.525731, 0.850651],
    [0.309017, 0.500000, 0.809017], [0.525731, 0.000000, 0.850651],
    [0.295242, 0.000000, 0.955423], [0.442863, 0.238856, 0.864188],
    [0.162460, 0.262866, 0.951056], [-0.681718, 0.147621, 0.716567],
    [-0.809017, 0.309017, 0.500000], [-0.587785, 0.425325, 0.688191],
    [-0.850651, 0.525731, 0.000000], [-0.864188, 0.442863, 0.238856],
    [-0.716567, 0.681718, 0.147621], [-0.688191, 0.587785, 0.425325],
    [-0.500000, 0.809017, 0.309017], [-0.238856, 0.864188, 0.442863],
    [-0.425325, 0.688191, 0.587785], [-0.716567, 0.681718, -0.147621],
    [-0.500000, 0.809017, -0.309017], [-0.525731, 0.850651, 0.000000],
    [0.000000, 0.850651, -0.525731], [-0.238856, 0.864188, -0.442863],
    [0.000000, 0.955423, -0.295242], [-0.262866, 0.951056, -0.162460],
    [0.000000, 1.000000, 0.000000], [0.000000, 0.955423, 0.295242],
    [-0.262866, 0.951056, 0.162460], [0.238856, 0.864188, 0.442863],
    [0.262866, 0.951056, 0.162460], [0.500000, 0.809017, 0.309017],
    [0.238856, 0.864188, -0.442863], [0.262866, 0.951056, -0.162460],
    [0.500000, 0.809017, -0.309017], [0.850651, 0.525731, 0.000000],
    [0.716567, 0.681718, 0.147621], [0.716567, 0.681718, -0.147621],
    [0.525731, 0.850651, 0.000000], [0.425325, 0.688191, 0.587785],
    [0.864188, 0.442863, 0.238856], [0.688191, 0.587785, 0.425325],
    [0.809017, 0.309017, 0.500000], [0.681718, 0.147621, 0.716567],
    [0.587785, 0.425325, 0.688191], [0.955423, 0.295242, 0.000000],
    [1.000000, 0.000000, 0.000000], [0.951056, 0.162460, 0.262866],
    [0.850651, -0.525731, 0.000000], [0.955423, -0.295242, 0.000000],
    [0.864188, -0.442863, 0.238856], [0.951056, -0.162460, 0.262866],
    [0.809017, -0.309017, 0.500000], [0.681718, -0.147621, 0.716567],
    [0.850651, 0.000000, 0.525731], [0.864188, 0.442863, -0.238856],
    [0.809017, 0.309017, -0.500000], [0.951056, 0.162460, -0.262866],
    [0.525731, 0.000000, -0.850651], [0.681718, 0.147621, -0.716567],
    [0.681718, -0.147621, -0.716567], [0.850651, 0.000000, -0.525731],
    [0.809017, -0.309017, -0.500000], [0.864188, -0.442863, -0.238856],
    [0.951056, -0.162460, -0.262866], [0.147621, 0.716567, -0.681718],
    [0.309017, 0.500000, -0.809017], [0.425325, 0.688191, -0.587785],
    [0.442863, 0.238856, -0.864188], [0.587785, 0.425325, -0.688191],
    [0.688191, 0.587785, -0.425325], [-0.147621, 0.716567, -0.681718],
    [-0.309017, 0.500000, -0.809017], [0.000000, 0.525731, -0.850651],
    [-0.525731, 0.000000, -0.850651], [-0.442863, 0.238856, -0.864188],
    [-0.295242, 0.000000, -0.955423], [-0.162460, 0.262866, -0.951056],
    [0.000000, 0.000000, -1.000000], [0.295242, 0.000000, -0.955423],
    [0.162460, 0.262866, -0.951056], [-0.442863, -0.238856, -0.864188],
    [-0.309017, -0.500000, -0.809017], [-0.162460, -0.262866, -0.951056],
    [0.000000, -0.850651, -0.525731], [-0.147621, -0.716567, -0.681718],
    [0.147621, -0.716567, -0.681718], [0.000000, -0.525731, -0.850651],
    [0.309017, -0.500000, -0.809017], [0.442863, -0.238856, -0.864188],
    [0.162460, -0.262866, -0.951056], [0.238856, -0.864188, -0.442863],
    [0.500000, -0.809017, -0.309017], [0.425325, -0.688191, -0.587785],
    [0.716567, -0.681718, -0.147621], [0.688191, -0.587785, -0.425325],
    [0.587785, -0.425325, -0.688191], [0.000000, -0.955423, -0.295242],
    [0.000000, -1.000000, 0.000000], [0.262866, -0.951056, -0.162460],
    [0.000000, -0.850651, 0.525731], [0.000000, -0.955423, 0.295242],
    [0.238856, -0.864188, 0.442863], [0.262866, -0.951056, 0.162460],
    [0.500000, -0.809017, 0.309017], [0.716567, -0.681718, 0.147621],
    [0.525731, -0.850651, 0.000000], [-0.238856, -0.864188, -0.442863],
    [-0.500000, -0.809017, -0.309017], [-0.262866, -0.951056, -0.162460],
    [-0.850651, -0.525731, 0.000000], [-0.716567, -0.681718, -0.147621],
    [-0.716567, -0.681718, 0.147621], [-0.525731, -0.850651, 0.000000],
    [-0.500000, -0.809017, 0.309017], [-0.238856, -0.864188, 0.442863],
    [-0.262866, -0.951056, 0.162460], [-0.864188, -0.442863, 0.238856],
    [-0.809017, -0.309017, 0.500000], [-0.688191, -0.587785, 0.425325],
    [-0.681718, -0.147621, 0.716567], [-0.442863, -0.238856, 0.864188],
    [-0.587785, -0.425325, 0.688191], [-0.309017, -0.500000, 0.809017],
    [-0.147621, -0.716567, 0.681718], [-0.425325, -0.688191, 0.587785],
    [-0.162460, -0.262866, 0.951056], [0.442863, -0.238856, 0.864188],
    [0.162460, -0.262866, 0.951056], [0.309017, -0.500000, 0.809017],
    [0.147621, -0.716567, 0.681718], [0.000000, -0.525731, 0.850651],
    [0.425325, -0.688191, 0.587785], [0.587785, -0.425325, 0.688191],
    [0.688191, -0.587785, 0.425325], [-0.955423, 0.295242, 0.000000],
    [-0.951056, 0.162460, 0.262866], [-1.000000, 0.000000, 0.000000],
    [-0.850651, 0.000000, 0.525731], [-0.955423, -0.295242, 0.000000],
    [-0.951056, -0.162460, 0.262866], [-0.864188, 0.442863, -0.238856],
    [-0.951056, 0.162460, -0.262866], [-0.809017, 0.309017, -0.500000],
    [-0.864188, -0.442863, -0.238856], [-0.951056, -0.162460, -0.262866],
    [-0.809017, -0.309017, -0.500000], [-0.681718, 0.147621, -0.716567],
    [-0.681718, -0.147621, -0.716567], [-0.850651, 0.000000, -0.525731],
    [-0.688191, 0.587785, -0.425325], [-0.587785, 0.425325, -0.688191],
    [-0.425325, 0.688191, -0.587785], [-0.425325, -0.688191, -0.587785],
    [-0.587785, -0.425325, -0.688191], [-0.688191, -0.587785, -0.425325],
];

// ---------------------------------------------------------------------------
// UserCmd delta bit flags
// ---------------------------------------------------------------------------

const CM_ANGLE1: u8 = 1 << 0;
const CM_ANGLE2: u8 = 1 << 1;
const CM_ANGLE3: u8 = 1 << 2;
const CM_FORWARD: u8 = 1 << 3;
const CM_SIDE: u8 = 1 << 4;
const CM_UP: u8 = 1 << 5;
const CM_BUTTONS: u8 = 1 << 6;
const CM_IMPULSE: u8 = 1 << 7;

// ---------------------------------------------------------------------------
// NetMsg — the network message buffer
// ---------------------------------------------------------------------------

/// Network message buffer -- replaces `sizebuf_t`.
/// Supports sequential write and read operations.
pub struct NetMsg {
    data: Vec<u8>,
    read_pos: usize,
    /// If true, overflows are allowed (data is silently truncated).
    #[allow(dead_code)]
    allow_overflow: bool,
    overflowed: bool,
}

impl Default for NetMsg {
    fn default() -> Self {
        Self::new()
    }
}

impl NetMsg {
    // ----- Construction -----

    /// Create an empty buffer.
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            read_pos: 0,
            allow_overflow: false,
            overflowed: false,
        }
    }

    /// Create a buffer with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            data: Vec::with_capacity(capacity),
            read_pos: 0,
            allow_overflow: false,
            overflowed: false,
        }
    }

    /// Create a buffer from received bytes (for reading).
    pub fn from_bytes(data: &[u8]) -> Self {
        Self {
            data: data.to_vec(),
            read_pos: 0,
            allow_overflow: false,
            overflowed: false,
        }
    }

    /// Clear the buffer, resetting both data and read position.
    pub fn clear(&mut self) {
        self.data.clear();
        self.read_pos = 0;
        self.overflowed = false;
    }

    /// Number of bytes currently in the buffer.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Raw bytes for sending.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Current read cursor offset in bytes.
    pub fn read_cursor(&self) -> usize {
        self.read_pos
    }

    /// Whether a read operation has tried to read past the end of the buffer.
    pub fn is_overflowed(&self) -> bool {
        self.overflowed
    }

    /// Return a slice of all data from the current read position to the end.
    pub fn remaining_data(&self) -> &[u8] {
        if self.read_pos >= self.data.len() {
            &[]
        } else {
            &self.data[self.read_pos..]
        }
    }

    // ----- Write operations (append to buffer) -----

    /// Write a signed byte (1 byte, sign-extended on read via [`read_char`](Self::read_char)).
    ///
    /// Use for values in the range `[-128, 127]`. For unsigned bytes `[0, 255]`,
    /// use [`write_byte`](Self::write_byte) / [`read_byte`](Self::read_byte).
    pub fn write_char(&mut self, c: i32) {
        self.data.push(c as u8);
    }

    /// Write an unsigned byte (1 byte).
    pub fn write_byte(&mut self, c: i32) {
        self.data.push(c as u8);
    }

    /// Write a 16-bit integer, little-endian.
    pub fn write_short(&mut self, c: i32) {
        let v = c as i16;
        self.data.extend_from_slice(&v.to_le_bytes());
    }

    /// Write a 32-bit integer, little-endian.
    pub fn write_long(&mut self, c: i32) {
        self.data.extend_from_slice(&c.to_le_bytes());
    }

    /// Write a 32-bit float, little-endian IEEE 754.
    pub fn write_float(&mut self, f: f32) {
        self.data.extend_from_slice(&f.to_le_bytes());
    }

    /// Write a null-terminated string.
    pub fn write_string(&mut self, s: &str) {
        self.data.extend_from_slice(s.as_bytes());
        self.data.push(0);
    }

    /// Write a coordinate: multiply by 8, write as short.
    pub fn write_coord(&mut self, f: f32) {
        self.write_short((f * 8.0) as i32);
    }

    /// Write 3 coordinates (a quantised 3D position).
    pub fn write_position(&mut self, pos: Vec3f) {
        self.write_coord(pos.x);
        self.write_coord(pos.y);
        self.write_coord(pos.z);
    }

    /// Write an angle as a single byte: `(f * 256.0 / 360.0) as byte`.
    pub fn write_angle(&mut self, f: f32) {
        self.write_byte((f * 256.0 / 360.0) as i32 & 0xFF);
    }

    /// Write an angle as a 16-bit value: `(f * 65536.0 / 360.0) as short`.
    pub fn write_angle16(&mut self, f: f32) {
        self.write_short((f * 65536.0 / 360.0) as i32);
    }

    /// Write raw bytes.
    pub fn write_data(&mut self, data: &[u8]) {
        self.data.extend_from_slice(data);
    }

    /// Compress a direction vector to a single byte index into the BYTEDIRS table.
    pub fn write_dir(&mut self, dir: Vec3f) {
        let mut best_idx: usize = 0;
        let mut best_dot: f32 = -1.0;

        for (i, entry) in BYTEDIRS.iter().enumerate() {
            let dot = dir.x * entry[0] + dir.y * entry[1] + dir.z * entry[2];
            if dot > best_dot {
                best_dot = dot;
                best_idx = i;
            }
        }

        self.write_byte(best_idx as i32);
    }

    // ----- Read operations (advance read cursor) -----

    /// Reset read position to the beginning.
    pub fn begin_reading(&mut self) {
        self.read_pos = 0;
        self.overflowed = false;
    }

    /// Read a signed byte (1 byte, sign-extended to `i32`). Returns -1 on read past end.
    ///
    /// Counterpart to [`write_char`](Self::write_char). For unsigned reads,
    /// use [`read_byte`](Self::read_byte).
    pub fn read_char(&mut self) -> i32 {
        if self.read_pos >= self.data.len() {
            self.overflowed = true;
            return -1;
        }
        let val = self.data[self.read_pos] as i8;
        self.read_pos += 1;
        i32::from(val)
    }

    /// Read an unsigned byte (1 byte). Returns -1 on read past end.
    pub fn read_byte(&mut self) -> i32 {
        if self.read_pos >= self.data.len() {
            self.overflowed = true;
            return -1;
        }
        let val = self.data[self.read_pos];
        self.read_pos += 1;
        i32::from(val)
    }

    /// Read a 16-bit signed integer, little-endian. Returns -1 on read past end.
    pub fn read_short(&mut self) -> i32 {
        if self.read_pos + 2 > self.data.len() {
            self.overflowed = true;
            return -1;
        }
        let bytes: [u8; 2] = [self.data[self.read_pos], self.data[self.read_pos + 1]];
        self.read_pos += 2;
        i32::from(i16::from_le_bytes(bytes))
    }

    /// Read a 32-bit integer, little-endian. Returns -1 on read past end.
    pub fn read_long(&mut self) -> i32 {
        if self.read_pos + 4 > self.data.len() {
            self.overflowed = true;
            return -1;
        }
        let bytes: [u8; 4] = [
            self.data[self.read_pos],
            self.data[self.read_pos + 1],
            self.data[self.read_pos + 2],
            self.data[self.read_pos + 3],
        ];
        self.read_pos += 4;
        i32::from_le_bytes(bytes)
    }

    /// Read a 32-bit float, little-endian. Returns 0.0 on read past end.
    pub fn read_float(&mut self) -> f32 {
        if self.read_pos + 4 > self.data.len() {
            self.overflowed = true;
            return 0.0;
        }
        let bytes: [u8; 4] = [
            self.data[self.read_pos],
            self.data[self.read_pos + 1],
            self.data[self.read_pos + 2],
            self.data[self.read_pos + 3],
        ];
        self.read_pos += 4;
        f32::from_le_bytes(bytes)
    }

    /// Read a null-terminated string.
    pub fn read_string(&mut self) -> String {
        let mut result = Vec::new();
        loop {
            if self.read_pos >= self.data.len() {
                self.overflowed = true;
                break;
            }
            let b = self.data[self.read_pos];
            self.read_pos += 1;
            if b == 0 {
                break;
            }
            result.push(b);
        }
        String::from_utf8_lossy(&result).into_owned()
    }

    /// Read a coordinate: read short, divide by 8.0.
    pub fn read_coord(&mut self) -> f32 {
        self.read_short() as f32 * (1.0 / 8.0)
    }

    /// Read a quantised 3D position (3 coordinates) into a Vec3f.
    pub fn read_position(&mut self, pos: &mut Vec3f) {
        pos.x = self.read_coord();
        pos.y = self.read_coord();
        pos.z = self.read_coord();
    }

    /// Read a byte angle: read byte, convert to degrees.
    pub fn read_angle(&mut self) -> f32 {
        self.read_byte() as f32 * (360.0 / 256.0)
    }

    /// Read a 16-bit angle: read short, convert to degrees.
    pub fn read_angle16(&mut self) -> f32 {
        self.read_short() as f32 * (360.0 / 65536.0)
    }

    /// Read raw bytes into a buffer.
    pub fn read_data(&mut self, buf: &mut [u8]) {
        for byte in buf.iter_mut() {
            if self.read_pos >= self.data.len() {
                self.overflowed = true;
                *byte = 0;
            } else {
                *byte = self.data[self.read_pos];
                self.read_pos += 1;
            }
        }
    }

    /// Read a compressed direction: read byte index, look up in BYTEDIRS table.
    pub fn read_dir(&mut self) -> Vec3f {
        let idx = self.read_byte() as usize;
        if idx >= NUM_VERTEX_NORMALS {
            return Vec3f::ZERO;
        }
        let entry = &BYTEDIRS[idx];
        Vec3f::new(entry[0], entry[1], entry[2])
    }

    // ----- Delta compression -----

    /// Write only the fields that changed between `from` and `to`.
    /// Uses bitflags from `UpdateFlags`.
    ///
    /// If `force` is false and nothing changed, writes nothing (returns early).
    /// If `newentity` is true, the receiving side creates a new entity slot.
    pub fn write_delta_entity(
        &mut self,
        from: &EntityState,
        to: &EntityState,
        force: bool,
        newentity: bool,
    ) {
        // Build the flags for which fields have changed.
        let mut flags = UpdateFlags::empty();

        if to.origin.x != from.origin.x {
            flags |= UpdateFlags::ORIGIN1;
        }
        if to.origin.y != from.origin.y {
            flags |= UpdateFlags::ORIGIN2;
        }
        if to.origin.z != from.origin.z {
            flags |= UpdateFlags::ORIGIN3;
        }

        if to.angles.x != from.angles.x {
            flags |= UpdateFlags::ANGLE1;
        }
        if to.angles.y != from.angles.y {
            flags |= UpdateFlags::ANGLE2;
        }
        if to.angles.z != from.angles.z {
            flags |= UpdateFlags::ANGLE3;
        }

        if to.skinnum != from.skinnum {
            if to.skinnum < 256 {
                flags |= UpdateFlags::SKIN8;
            } else {
                flags |= UpdateFlags::SKIN8 | UpdateFlags::SKIN16;
            }
        }

        if to.frame != from.frame {
            if to.frame < 256 {
                flags |= UpdateFlags::FRAME8;
            } else {
                flags |= UpdateFlags::FRAME8 | UpdateFlags::FRAME16;
            }
        }

        if to.effects != from.effects {
            if to.effects < 256 {
                flags |= UpdateFlags::EFFECTS8;
            } else {
                flags |= UpdateFlags::EFFECTS8 | UpdateFlags::EFFECTS16;
            }
        }

        if to.renderfx != from.renderfx {
            if to.renderfx < 256 {
                flags |= UpdateFlags::RENDERFX8;
            } else {
                flags |= UpdateFlags::RENDERFX8 | UpdateFlags::RENDERFX16;
            }
        }

        if to.solid != from.solid {
            flags |= UpdateFlags::SOLID;
        }

        if to.event != from.event {
            flags |= UpdateFlags::EVENT;
        }

        if to.modelindex != from.modelindex {
            flags |= UpdateFlags::MODEL;
        }
        if to.modelindex2 != from.modelindex2 {
            flags |= UpdateFlags::MODEL2;
        }
        if to.modelindex3 != from.modelindex3 {
            flags |= UpdateFlags::MODEL3;
        }
        if to.modelindex4 != from.modelindex4 {
            flags |= UpdateFlags::MODEL4;
        }

        if to.sound != from.sound {
            flags |= UpdateFlags::SOUND;
        }

        if to.old_origin != from.old_origin || newentity {
            flags |= UpdateFlags::OLDORIGIN;
        }

        // If nothing changed and we are not forced, write nothing.
        if flags.is_empty() && !force {
            return;
        }

        // Entity number > 255 needs 16-bit encoding.
        if to.number >= 256 {
            flags |= UpdateFlags::NUMBER16;
        }

        // Determine extension bytes in descending order so each added MOREBITS
        // flag can propagate into the byte below it.  E.g. MOREBITS3 ends up
        // in byte 2; the subsequent check must see it and set MOREBITS2.
        if flags.bits() & 0xFF00_0000 != 0 {
            flags |= UpdateFlags::MOREBITS3;
        }
        if flags.bits() & 0x00FF_0000 != 0 {
            flags |= UpdateFlags::MOREBITS2;
        }
        if flags.bits() & 0x0000_FF00 != 0 {
            flags |= UpdateFlags::MOREBITS1;
        }

        let bits = flags.bits();

        // Write flag bytes.
        self.write_byte(bits as i32 & 0xFF);

        if flags.contains(UpdateFlags::MOREBITS1) {
            self.write_byte((bits >> 8) as i32 & 0xFF);
        }
        if flags.contains(UpdateFlags::MOREBITS2) {
            self.write_byte((bits >> 16) as i32 & 0xFF);
        }
        if flags.contains(UpdateFlags::MOREBITS3) {
            self.write_byte((bits >> 24) as i32 & 0xFF);
        }

        // Write entity number.
        if flags.contains(UpdateFlags::NUMBER16) {
            self.write_short(to.number);
        } else {
            self.write_byte(to.number);
        }

        // Write changed fields.
        if flags.contains(UpdateFlags::MODEL) {
            self.write_byte(to.modelindex);
        }
        if flags.contains(UpdateFlags::MODEL2) {
            self.write_byte(to.modelindex2);
        }
        if flags.contains(UpdateFlags::MODEL3) {
            self.write_byte(to.modelindex3);
        }
        if flags.contains(UpdateFlags::MODEL4) {
            self.write_byte(to.modelindex4);
        }

        if flags.contains(UpdateFlags::FRAME8) {
            if flags.contains(UpdateFlags::FRAME16) {
                self.write_short(to.frame);
            } else {
                self.write_byte(to.frame);
            }
        }

        if flags.contains(UpdateFlags::SKIN8) {
            if flags.contains(UpdateFlags::SKIN16) {
                self.write_long(to.skinnum);
            } else {
                self.write_byte(to.skinnum);
            }
        }

        if flags.contains(UpdateFlags::EFFECTS8) {
            if flags.contains(UpdateFlags::EFFECTS16) {
                self.write_long(to.effects as i32);
            } else {
                self.write_byte(to.effects as i32);
            }
        }

        if flags.contains(UpdateFlags::RENDERFX8) {
            if flags.contains(UpdateFlags::RENDERFX16) {
                self.write_long(to.renderfx);
            } else {
                self.write_byte(to.renderfx);
            }
        }

        if flags.contains(UpdateFlags::ORIGIN1) {
            self.write_coord(to.origin.x);
        }
        if flags.contains(UpdateFlags::ORIGIN2) {
            self.write_coord(to.origin.y);
        }
        if flags.contains(UpdateFlags::ORIGIN3) {
            self.write_coord(to.origin.z);
        }

        if flags.contains(UpdateFlags::ANGLE1) {
            self.write_angle16(to.angles.x);
        }
        if flags.contains(UpdateFlags::ANGLE2) {
            self.write_angle16(to.angles.y);
        }
        if flags.contains(UpdateFlags::ANGLE3) {
            self.write_angle16(to.angles.z);
        }

        if flags.contains(UpdateFlags::OLDORIGIN) {
            self.write_coord(to.old_origin.x);
            self.write_coord(to.old_origin.y);
            self.write_coord(to.old_origin.z);
        }

        if flags.contains(UpdateFlags::SOUND) {
            self.write_byte(to.sound);
        }

        if flags.contains(UpdateFlags::EVENT) {
            self.write_byte(to.event);
        }

        if flags.contains(UpdateFlags::SOLID) {
            self.write_short(to.solid);
        }
    }

    /// Write a delta-compressed user command.
    pub fn write_delta_usercmd(&mut self, from: &UserCmd, to: &UserCmd) {
        let mut bits: u8 = 0;

        if to.angles[0] != from.angles[0] {
            bits |= CM_ANGLE1;
        }
        if to.angles[1] != from.angles[1] {
            bits |= CM_ANGLE2;
        }
        if to.angles[2] != from.angles[2] {
            bits |= CM_ANGLE3;
        }
        if to.forwardmove != from.forwardmove {
            bits |= CM_FORWARD;
        }
        if to.sidemove != from.sidemove {
            bits |= CM_SIDE;
        }
        if to.upmove != from.upmove {
            bits |= CM_UP;
        }
        if to.buttons != from.buttons {
            bits |= CM_BUTTONS;
        }
        if to.impulse != from.impulse {
            bits |= CM_IMPULSE;
        }

        self.write_byte(i32::from(bits));

        // msec and lightlevel are always written (they change virtually every frame).
        self.write_byte(i32::from(to.msec));
        self.write_byte(i32::from(to.lightlevel));

        if bits & CM_ANGLE1 != 0 {
            self.write_short(i32::from(to.angles[0]));
        }
        if bits & CM_ANGLE2 != 0 {
            self.write_short(i32::from(to.angles[1]));
        }
        if bits & CM_ANGLE3 != 0 {
            self.write_short(i32::from(to.angles[2]));
        }
        if bits & CM_FORWARD != 0 {
            self.write_short(i32::from(to.forwardmove));
        }
        if bits & CM_SIDE != 0 {
            self.write_short(i32::from(to.sidemove));
        }
        if bits & CM_UP != 0 {
            self.write_short(i32::from(to.upmove));
        }
        if bits & CM_BUTTONS != 0 {
            self.write_byte(i32::from(to.buttons));
        }
        if bits & CM_IMPULSE != 0 {
            self.write_byte(i32::from(to.impulse));
        }
    }

    /// Read a delta-compressed user command.
    pub fn read_delta_usercmd(&mut self, from: &UserCmd) -> UserCmd {
        let mut cmd = *from;
        let bits = self.read_byte() as u8;

        cmd.msec = self.read_byte() as u8;
        cmd.lightlevel = self.read_byte() as u8;

        if bits & CM_ANGLE1 != 0 {
            cmd.angles[0] = self.read_short() as i16;
        }
        if bits & CM_ANGLE2 != 0 {
            cmd.angles[1] = self.read_short() as i16;
        }
        if bits & CM_ANGLE3 != 0 {
            cmd.angles[2] = self.read_short() as i16;
        }
        if bits & CM_FORWARD != 0 {
            cmd.forwardmove = self.read_short() as i16;
        }
        if bits & CM_SIDE != 0 {
            cmd.sidemove = self.read_short() as i16;
        }
        if bits & CM_UP != 0 {
            cmd.upmove = self.read_short() as i16;
        }
        if bits & CM_BUTTONS != 0 {
            cmd.buttons = self.read_byte() as u8;
        }
        if bits & CM_IMPULSE != 0 {
            cmd.impulse = self.read_byte() as u8;
        }

        cmd
    }

    // -------------------------------------------------------------------------
    // Player state delta encoding
    // -------------------------------------------------------------------------

    /// Write a delta-compressed player state to the message.
    ///
    /// Writes the `svc_playerinfo` opcode byte followed by `PlayerStateFlags`
    /// and only the fields that differ between `old` and `new`. The statbits
    /// mask uses u32 to avoid i32 shift-overflow on bit 31.
    pub fn write_player_state(&mut self, old: &PlayerState, new: &PlayerState) {
        let mut flags = PlayerStateFlags::empty();

        if new.pmove.pm_type != old.pmove.pm_type {
            flags |= PlayerStateFlags::M_TYPE;
        }
        if new.pmove.origin != old.pmove.origin {
            flags |= PlayerStateFlags::M_ORIGIN;
        }
        if new.pmove.velocity != old.pmove.velocity {
            flags |= PlayerStateFlags::M_VELOCITY;
        }
        if new.pmove.pm_time != old.pmove.pm_time {
            flags |= PlayerStateFlags::M_TIME;
        }
        if new.pmove.pm_flags != old.pmove.pm_flags {
            flags |= PlayerStateFlags::M_FLAGS;
        }
        if new.pmove.gravity != old.pmove.gravity {
            flags |= PlayerStateFlags::M_GRAVITY;
        }
        if new.pmove.delta_angles != old.pmove.delta_angles {
            flags |= PlayerStateFlags::M_DELTA_ANGLES;
        }
        if new.viewoffset != old.viewoffset {
            flags |= PlayerStateFlags::VIEWOFFSET;
        }
        if new.viewangles != old.viewangles {
            flags |= PlayerStateFlags::VIEWANGLES;
        }
        if new.kick_angles != old.kick_angles {
            flags |= PlayerStateFlags::KICKANGLES;
        }
        if new.blend != old.blend {
            flags |= PlayerStateFlags::BLEND;
        }
        if new.fov != old.fov {
            flags |= PlayerStateFlags::FOV;
        }
        if new.rdflags != old.rdflags {
            flags |= PlayerStateFlags::RDFLAGS;
        }
        // gunindex always sent (mirrors C server behaviour)
        flags |= PlayerStateFlags::WEAPONINDEX;
        if new.gunframe != old.gunframe
            || new.gunoffset != old.gunoffset
            || new.gunangles != old.gunangles
        {
            flags |= PlayerStateFlags::WEAPONFRAME;
        }

        self.write_byte(SvcOp::PlayerInfo as i32);
        self.write_short(flags.bits() as i32);

        if flags.contains(PlayerStateFlags::M_TYPE) {
            self.write_byte(new.pmove.pm_type as i32);
        }
        if flags.contains(PlayerStateFlags::M_ORIGIN) {
            self.write_short(new.pmove.origin[0] as i32);
            self.write_short(new.pmove.origin[1] as i32);
            self.write_short(new.pmove.origin[2] as i32);
        }
        if flags.contains(PlayerStateFlags::M_VELOCITY) {
            self.write_short(new.pmove.velocity[0] as i32);
            self.write_short(new.pmove.velocity[1] as i32);
            self.write_short(new.pmove.velocity[2] as i32);
        }
        if flags.contains(PlayerStateFlags::M_TIME) {
            self.write_byte(new.pmove.pm_time as i32);
        }
        if flags.contains(PlayerStateFlags::M_FLAGS) {
            self.write_byte(new.pmove.pm_flags as i32);
        }
        if flags.contains(PlayerStateFlags::M_GRAVITY) {
            self.write_short(new.pmove.gravity as i32);
        }
        if flags.contains(PlayerStateFlags::M_DELTA_ANGLES) {
            self.write_short(new.pmove.delta_angles[0] as i32);
            self.write_short(new.pmove.delta_angles[1] as i32);
            self.write_short(new.pmove.delta_angles[2] as i32);
        }
        if flags.contains(PlayerStateFlags::VIEWOFFSET) {
            self.write_char((new.viewoffset.x * 4.0) as i32);
            self.write_char((new.viewoffset.y * 4.0) as i32);
            self.write_char((new.viewoffset.z * 4.0) as i32);
        }
        if flags.contains(PlayerStateFlags::VIEWANGLES) {
            self.write_angle16(new.viewangles.x);
            self.write_angle16(new.viewangles.y);
            self.write_angle16(new.viewangles.z);
        }
        if flags.contains(PlayerStateFlags::KICKANGLES) {
            self.write_char((new.kick_angles.x * 4.0) as i32);
            self.write_char((new.kick_angles.y * 4.0) as i32);
            self.write_char((new.kick_angles.z * 4.0) as i32);
        }
        if flags.contains(PlayerStateFlags::WEAPONINDEX) {
            self.write_byte(new.gunindex);
        }
        if flags.contains(PlayerStateFlags::WEAPONFRAME) {
            self.write_byte(new.gunframe);
            self.write_char((new.gunoffset.x * 4.0) as i32);
            self.write_char((new.gunoffset.y * 4.0) as i32);
            self.write_char((new.gunoffset.z * 4.0) as i32);
            self.write_char((new.gunangles.x * 4.0) as i32);
            self.write_char((new.gunangles.y * 4.0) as i32);
            self.write_char((new.gunangles.z * 4.0) as i32);
        }
        if flags.contains(PlayerStateFlags::BLEND) {
            self.write_byte((new.blend[0] * 255.0) as i32);
            self.write_byte((new.blend[1] * 255.0) as i32);
            self.write_byte((new.blend[2] * 255.0) as i32);
            self.write_byte((new.blend[3] * 255.0) as i32);
        }
        if flags.contains(PlayerStateFlags::FOV) {
            self.write_byte(new.fov as i32);
        }
        if flags.contains(PlayerStateFlags::RDFLAGS) {
            self.write_byte(new.rdflags);
        }

        // Stats: always write the statbits mask, then only changed slots.
        let mut statbits: u32 = 0;
        for i in 0..MAX_STATS {
            if new.stats[i] != old.stats[i] {
                statbits |= 1u32 << i;
            }
        }
        self.write_long(statbits as i32);
        for i in 0..MAX_STATS {
            if statbits & (1u32 << i) != 0 {
                self.write_short(new.stats[i] as i32);
            }
        }
    }

    // -------------------------------------------------------------------------
    // Packet entity list encoding
    // -------------------------------------------------------------------------

    /// Write a delta-compressed entity list followed by the `svc_packetentities`
    /// opcode and terminated by entity number 0.
    ///
    /// Both `old_entities` and `new_entities` **must** be sorted by entity number
    /// (ascending). Entities in `old_entities` but absent from `new_entities` are
    /// written with the REMOVE flag; entities that appear in both with identical
    /// state are omitted (the reader copies them forward from the old frame).
    pub fn write_packet_entities_list(
        &mut self,
        old_entities: &[EntityState],
        new_entities: &[EntityState],
    ) {
        self.write_byte(SvcOp::PacketEntities as i32);

        let mut old_idx = 0usize;

        for new_ent in new_entities {
            // Entities in old that come before this new entity number are REMOVED.
            while old_idx < old_entities.len() && old_entities[old_idx].number < new_ent.number {
                self.write_entity_remove(old_entities[old_idx].number);
                old_idx += 1;
            }

            // Determine delta base: old entity with same number (if any) or zero.
            let (from, is_new) =
                if old_idx < old_entities.len() && old_entities[old_idx].number == new_ent.number {
                    let from = old_entities[old_idx].clone();
                    old_idx += 1;
                    (from, false)
                } else {
                    (EntityState::default(), true)
                };

            self.write_delta_entity(&from, new_ent, is_new, is_new);
        }

        // Any remaining old entities are removed.
        while old_idx < old_entities.len() {
            self.write_entity_remove(old_entities[old_idx].number);
            old_idx += 1;
        }

        // Terminator: entity number 0 with no flags.
        self.write_byte(0); // flags byte = 0
        self.write_byte(0); // entity number = 0
    }

    /// Write entity bits encoding a REMOVE for the given entity number.
    fn write_entity_remove(&mut self, number: i32) {
        let mut flags = UpdateFlags::REMOVE;
        if number >= 256 {
            flags |= UpdateFlags::NUMBER16 | UpdateFlags::MOREBITS1;
        }
        let bits = flags.bits();
        self.write_byte(bits as i32 & 0xFF);
        if flags.contains(UpdateFlags::MOREBITS1) {
            self.write_byte((bits >> 8) as i32 & 0xFF);
        }
        if flags.contains(UpdateFlags::NUMBER16) {
            self.write_short(number);
        } else {
            self.write_byte(number);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_read_byte() {
        let mut buf = NetMsg::new();
        buf.write_byte(42);
        buf.begin_reading();
        assert_eq!(buf.read_byte(), 42);
    }

    #[test]
    fn write_read_char_negative() {
        let mut buf = NetMsg::new();
        buf.write_char(-1);
        buf.begin_reading();
        assert_eq!(buf.read_char(), -1);
    }

    #[test]
    fn write_read_short() {
        let mut buf = NetMsg::new();
        buf.write_short(1234);
        buf.write_short(-5678);
        buf.begin_reading();
        assert_eq!(buf.read_short(), 1234);
        assert_eq!(buf.read_short(), -5678);
    }

    #[test]
    fn write_read_long() {
        let mut buf = NetMsg::new();
        buf.write_long(0x12345678);
        buf.write_long(-1);
        buf.begin_reading();
        assert_eq!(buf.read_long(), 0x12345678);
        assert_eq!(buf.read_long(), -1);
    }

    #[test]
    fn write_read_float() {
        let mut buf = NetMsg::new();
        buf.write_float(3.14);
        buf.begin_reading();
        let val = buf.read_float();
        assert!((val - 3.14).abs() < 0.001);
    }

    #[test]
    fn write_read_string() {
        let mut buf = NetMsg::new();
        buf.write_string("hello world");
        buf.begin_reading();
        assert_eq!(buf.read_string(), "hello world");
    }

    #[test]
    fn write_read_coord() {
        let mut buf = NetMsg::new();
        buf.write_coord(123.5);
        buf.begin_reading();
        let val = buf.read_coord();
        // Coordinate precision is 1/8 unit
        assert!((val - 123.5).abs() < 0.125);
    }

    #[test]
    fn write_read_position() {
        let mut buf = NetMsg::new();
        let pos = Vec3f::new(100.0, 200.0, 50.0);
        buf.write_position(pos);
        buf.begin_reading();
        let mut result = Vec3f::ZERO;
        buf.read_position(&mut result);
        assert!((result.x - 100.0).abs() < 0.125);
        assert!((result.y - 200.0).abs() < 0.125);
        assert!((result.z - 50.0).abs() < 0.125);
    }

    #[test]
    fn write_read_angle() {
        let mut buf = NetMsg::new();
        buf.write_angle(90.0);
        buf.begin_reading();
        let val = buf.read_angle();
        // Byte angle precision: 360/256 ~ 1.4 degrees
        assert!((val - 90.0).abs() < 1.5);
    }

    #[test]
    fn write_read_angle16() {
        let mut buf = NetMsg::new();
        buf.write_angle16(90.0);
        buf.begin_reading();
        let val = buf.read_angle16();
        // Short angle precision: 360/65536 ~ 0.005 degrees
        assert!((val - 90.0).abs() < 0.01);
    }

    #[test]
    fn write_read_data() {
        let mut buf = NetMsg::new();
        buf.write_data(&[1, 2, 3, 4, 5]);
        buf.begin_reading();
        let mut out = [0u8; 5];
        buf.read_data(&mut out);
        assert_eq!(out, [1, 2, 3, 4, 5]);
    }

    #[test]
    fn from_bytes() {
        let data = [42u8, 0x39, 0x05]; // byte 42, then short 1337
        let mut buf = NetMsg::from_bytes(&data);
        assert_eq!(buf.read_byte(), 42);
        assert_eq!(buf.read_short(), 1337);
    }

    #[test]
    fn full_roundtrip() {
        let mut buf = NetMsg::with_capacity(1400);
        buf.write_byte(42);
        buf.write_short(1234);
        buf.write_long(-1);
        buf.write_float(3.14);
        buf.write_string("hello");
        buf.write_coord(100.5);
        buf.write_angle(180.0);

        buf.begin_reading();
        assert_eq!(buf.read_byte(), 42);
        assert_eq!(buf.read_short(), 1234);
        assert_eq!(buf.read_long(), -1);
        assert!((buf.read_float() - 3.14).abs() < 0.001);
        assert_eq!(buf.read_string(), "hello");
        assert!((buf.read_coord() - 100.5).abs() < 0.125);
        assert!((buf.read_angle() - 180.0).abs() < 1.5);
    }

    #[test]
    fn delta_usercmd_roundtrip() {
        let from = UserCmd::default();
        let to = UserCmd {
            msec: 16,
            buttons: 1,
            angles: [100, 200, 0],
            forwardmove: 400,
            sidemove: -200,
            upmove: 0,
            impulse: 0,
            lightlevel: 128,
        };

        let mut buf = NetMsg::new();
        buf.write_delta_usercmd(&from, &to);
        buf.begin_reading();
        let result = buf.read_delta_usercmd(&from);
        assert_eq!(result.msec, 16);
        assert_eq!(result.buttons, 1);
        assert_eq!(result.angles, [100, 200, 0]);
        assert_eq!(result.forwardmove, 400);
        assert_eq!(result.sidemove, -200);
    }

    #[test]
    fn delta_entity_unchanged_is_compact() {
        let state = EntityState::default();
        let mut buf = NetMsg::new();
        buf.write_delta_entity(&state, &state, false, false);
        // Unchanged entity with force=false should write nothing (or just a number)
        assert!(buf.len() < 10);
    }

    #[test]
    fn delta_entity_changed_fields() {
        let from = EntityState::default();
        let mut to = EntityState::default();
        to.number = 1;
        to.origin = Vec3f::new(100.0, 200.0, 0.0);
        to.modelindex = 5;

        let mut buf = NetMsg::new();
        buf.write_delta_entity(&from, &to, true, true);

        // Must encode at minimum: flag bytes + entity number + origin x/y + modelindex
        // 1 flag byte + 1 entity number + 2*2 coords + 1 model = at least 8 bytes
        assert!(
            buf.len() >= 8,
            "delta encoding too small: {} bytes",
            buf.len()
        );

        // Verify the encoded data is decodable: read the flag byte(s) and
        // check that ORIGIN1, ORIGIN2, and MODEL bits are set.
        buf.begin_reading();
        let first_byte = buf.read_byte() as u32;
        let flags = if first_byte & 0x80 != 0 {
            // MOREBITS1 set — read second flag byte
            let second_byte = buf.read_byte() as u32;
            first_byte | (second_byte << 8)
        } else {
            first_byte
        };
        let origin1_bit = UpdateFlags::ORIGIN1.bits();
        let origin2_bit = UpdateFlags::ORIGIN2.bits();
        let model_bit = UpdateFlags::MODEL.bits();
        assert_ne!(flags & origin1_bit, 0, "ORIGIN1 flag should be set");
        assert_ne!(flags & origin2_bit, 0, "ORIGIN2 flag should be set");
        assert_ne!(flags & model_bit, 0, "MODEL flag should be set");
    }

    #[test]
    fn write_read_dir_roundtrip() {
        let mut buf = NetMsg::new();
        let dir = Vec3f::new(0.0, 0.0, 1.0);
        buf.write_dir(dir);
        buf.begin_reading();
        let result = buf.read_dir();
        // Should be close to the original direction
        let dot = result.x * dir.x + result.y * dir.y + result.z * dir.z;
        assert!(dot > 0.9, "Direction roundtrip lost too much precision");
    }

    #[test]
    fn clear_resets_buffer() {
        let mut buf = NetMsg::new();
        buf.write_byte(1);
        buf.write_byte(2);
        assert_eq!(buf.len(), 2);
        buf.clear();
        assert_eq!(buf.len(), 0);
        assert!(buf.is_empty());
    }

    #[test]
    fn read_past_end_returns_sentinel() {
        let mut buf = NetMsg::new();
        buf.write_byte(42);
        buf.begin_reading();
        assert_eq!(buf.read_byte(), 42);
        // Reading past end should return -1
        assert_eq!(buf.read_byte(), -1);
        assert_eq!(buf.read_short(), -1);
        assert_eq!(buf.read_long(), -1);
    }

    #[test]
    fn default_creates_empty() {
        let buf = NetMsg::default();
        assert!(buf.is_empty());
        assert_eq!(buf.len(), 0);
    }
}
