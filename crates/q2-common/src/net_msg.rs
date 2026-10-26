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

use crate::error::{Q2Error, Q2Result};
use q2_shared::constants::{MAX_EDICTS, MAX_MSGLEN, MAX_STATS};
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

/// `RF_BEAM` from shared.h: beams always carry their end point in old_origin.
const RF_BEAM: i32 = 128;

// ---------------------------------------------------------------------------
// NetMsg — the network message buffer
// ---------------------------------------------------------------------------

/// Network message buffer -- replaces `sizebuf_t`.
/// Supports sequential write and read operations.
pub struct NetMsg {
    data: Vec<u8>,
    read_pos: usize,
    /// Write limit, like `sizebuf_t.maxsize`. `usize::MAX` means unbounded.
    max_size: usize,
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
            max_size: usize::MAX,
            overflowed: false,
        }
    }

    /// Create a buffer with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            data: Vec::with_capacity(capacity),
            read_pos: 0,
            max_size: usize::MAX,
            overflowed: false,
        }
    }

    /// Create a write buffer limited to `max_size` bytes (C `SZ_Init` with
    /// `allowoverflow` set). A write that would exceed the limit clears the
    /// buffer and sets [`is_overflowed`](Self::is_overflowed), so the owner can
    /// detect the loss instead of sending a truncated message.
    pub fn with_max_size(max_size: usize) -> Self {
        Self {
            data: Vec::new(),
            read_pos: 0,
            max_size,
            overflowed: false,
        }
    }

    /// Create a buffer from received bytes (for reading).
    pub fn from_bytes(data: &[u8]) -> Self {
        Self {
            data: data.to_vec(),
            read_pos: 0,
            max_size: usize::MAX,
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

    /// Whether a read went past the end of the buffer, or a write went past
    /// the maximum size.
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

    /// C `SZ_GetSpace`: returns `false` (after clearing the buffer and setting
    /// `overflowed`) when `len` more bytes would pass `max_size`. C then
    /// writes the item at offset 0, but every owner discards an overflowed
    /// buffer, so the write is skipped here.
    fn get_space(&mut self, len: usize) -> bool {
        if self.data.len().saturating_add(len) > self.max_size {
            self.data.clear();
            self.overflowed = true;
            return false;
        }
        true
    }

    fn put(&mut self, bytes: &[u8]) {
        if self.get_space(bytes.len()) {
            self.data.extend_from_slice(bytes);
        }
    }

    /// Write a signed byte (1 byte, sign-extended on read via [`read_char`](Self::read_char)).
    ///
    /// Use for values in the range `[-128, 127]`. For unsigned bytes `[0, 255]`,
    /// use [`write_byte`](Self::write_byte) / [`read_byte`](Self::read_byte).
    pub fn write_char(&mut self, c: i32) {
        self.put(&[c as u8]);
    }

    /// Write an unsigned byte (1 byte).
    pub fn write_byte(&mut self, c: i32) {
        self.put(&[c as u8]);
    }

    /// Write a 16-bit integer, little-endian.
    pub fn write_short(&mut self, c: i32) {
        let v = c as i16;
        self.put(&v.to_le_bytes());
    }

    /// Write a 32-bit integer, little-endian.
    pub fn write_long(&mut self, c: i32) {
        self.put(&c.to_le_bytes());
    }

    /// Write a 32-bit float, little-endian IEEE 754.
    pub fn write_float(&mut self, f: f32) {
        self.put(&f.to_le_bytes());
    }

    /// Write a null-terminated string.
    ///
    /// Q2 strings are bytes, not UTF-8: each char U+0000..=U+00FF becomes the
    /// byte with that value (the inverse of [`read_string`](Self::read_string)),
    /// so high-bit console colour bytes survive a read/write cycle. Chars that
    /// do not fit in a byte become `?`. An interior NUL ends the string, as
    /// with C `strlen`.
    pub fn write_string(&mut self, s: &str) {
        let mut bytes: Vec<u8> = s
            .chars()
            .take_while(|&c| c != '\0')
            .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
            .collect();
        bytes.push(0);
        self.put(&bytes);
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
        self.put(data);
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
    ///
    /// Each byte maps to the char with the same code point (Latin-1), so no
    /// byte is lost. Like C `MSG_ReadString`, at most 2047 bytes are read;
    /// the remainder of a longer string stays in the stream.
    pub fn read_string(&mut self) -> String {
        const MAX_STRING_CHARS: usize = 2047;
        let mut result = String::new();
        for _ in 0..MAX_STRING_CHARS {
            let c = self.read_byte();
            if c <= 0 {
                break; // terminator, or end of message (-1)
            }
            result.push(char::from(c as u8));
        }
        result
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

    /// Read a byte angle in degrees, in `[-180, 178.6]`. The byte is signed,
    /// as in C `MSG_ReadAngle`.
    pub fn read_angle(&mut self) -> f32 {
        self.read_char() as f32 * (360.0 / 256.0)
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
    ///
    /// An index outside the table (or a read past the end) is an error, as in
    /// C `MSG_ReadDir` (ERR_DROP): a made-up normal would be silently wrong.
    pub fn read_dir(&mut self) -> Q2Result<Vec3f> {
        let idx = self.read_byte();
        let entry = usize::try_from(idx)
            .ok()
            .and_then(|i| BYTEDIRS.get(i))
            .ok_or_else(|| Q2Error::Drop(format!("read_dir: index {idx} out of range")))?;
        Ok(Vec3f::new(entry[0], entry[1], entry[2]))
    }

    // ----- Delta compression -----

    /// Write only the fields that changed between `from` and `to`, in the
    /// protocol 34 layout of C `MSG_WriteDeltaEntity`.
    ///
    /// If `force` is false and nothing changed, writes nothing (returns early).
    /// `newentity` forces old_origin to be sent so the client does not lerp
    /// from a stale position.
    ///
    /// Errors (C `ERR_FATAL`) when `to.number` is 0 (the list terminator) or
    /// not below `MAX_EDICTS`; nothing is written then.
    pub fn write_delta_entity(
        &mut self,
        from: &EntityState,
        to: &EntityState,
        force: bool,
        newentity: bool,
    ) -> Q2Result<()> {
        if to.number <= 0 {
            return Err(Q2Error::Fatal(format!(
                "write_delta_entity: unset entity number {}",
                to.number
            )));
        }
        if to.number >= MAX_EDICTS as i32 {
            return Err(Q2Error::Fatal(format!(
                "write_delta_entity: entity number {} >= MAX_EDICTS",
                to.number
            )));
        }

        // Build the flags for which fields have changed.
        let mut flags = UpdateFlags::empty();

        // Set before the "nothing to send" check, as in C: an entity >= 256
        // is always written, even when unchanged.
        if to.number >= 256 {
            flags |= UpdateFlags::NUMBER16;
        }

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

        // Field widths follow C: 8 bit, 16 bit alone, or both bits for 32.
        // C compares skinnum as unsigned (laser colours are negative as int);
        // frame and renderfx are signed compares, effects is unsigned.
        if to.skinnum != from.skinnum {
            let skin = to.skinnum as u32;
            if skin < 256 {
                flags |= UpdateFlags::SKIN8;
            } else if skin < 0x10000 {
                flags |= UpdateFlags::SKIN16;
            } else {
                flags |= UpdateFlags::SKIN8 | UpdateFlags::SKIN16;
            }
        }

        if to.frame != from.frame {
            if to.frame < 256 {
                flags |= UpdateFlags::FRAME8;
            } else {
                flags |= UpdateFlags::FRAME16;
            }
        }

        if to.effects != from.effects {
            if to.effects < 256 {
                flags |= UpdateFlags::EFFECTS8;
            } else if to.effects < 0x8000 {
                flags |= UpdateFlags::EFFECTS16;
            } else {
                flags |= UpdateFlags::EFFECTS8 | UpdateFlags::EFFECTS16;
            }
        }

        if to.renderfx != from.renderfx {
            if to.renderfx < 256 {
                flags |= UpdateFlags::RENDERFX8;
            } else if to.renderfx < 0x8000 {
                flags |= UpdateFlags::RENDERFX16;
            } else {
                flags |= UpdateFlags::RENDERFX8 | UpdateFlags::RENDERFX16;
            }
        }

        if to.solid != from.solid {
            flags |= UpdateFlags::SOLID;
        }

        // Events are not delta compressed, just zero compressed: the client
        // clears the event whenever EVENT is absent.
        if to.event != 0 {
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

        if newentity || (to.renderfx & RF_BEAM) != 0 {
            flags |= UpdateFlags::OLDORIGIN;
        }

        // If nothing changed and we are not forced, write nothing.
        if flags.is_empty() && !force {
            return Ok(());
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
            self.write_byte(to.frame);
        }
        if flags.contains(UpdateFlags::FRAME16) {
            self.write_short(to.frame);
        }

        let skin_bits = flags & (UpdateFlags::SKIN8 | UpdateFlags::SKIN16);
        if skin_bits == UpdateFlags::SKIN8 | UpdateFlags::SKIN16 {
            self.write_long(to.skinnum);
        } else if skin_bits == UpdateFlags::SKIN8 {
            self.write_byte(to.skinnum);
        } else if skin_bits == UpdateFlags::SKIN16 {
            self.write_short(to.skinnum);
        }

        let effects_bits = flags & (UpdateFlags::EFFECTS8 | UpdateFlags::EFFECTS16);
        if effects_bits == UpdateFlags::EFFECTS8 | UpdateFlags::EFFECTS16 {
            self.write_long(to.effects as i32);
        } else if effects_bits == UpdateFlags::EFFECTS8 {
            self.write_byte(to.effects as i32);
        } else if effects_bits == UpdateFlags::EFFECTS16 {
            self.write_short(to.effects as i32);
        }

        let renderfx_bits = flags & (UpdateFlags::RENDERFX8 | UpdateFlags::RENDERFX16);
        if renderfx_bits == UpdateFlags::RENDERFX8 | UpdateFlags::RENDERFX16 {
            self.write_long(to.renderfx);
        } else if renderfx_bits == UpdateFlags::RENDERFX8 {
            self.write_byte(to.renderfx);
        } else if renderfx_bits == UpdateFlags::RENDERFX16 {
            self.write_short(to.renderfx);
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
            self.write_angle(to.angles.x);
        }
        if flags.contains(UpdateFlags::ANGLE2) {
            self.write_angle(to.angles.y);
        }
        if flags.contains(UpdateFlags::ANGLE3) {
            self.write_angle(to.angles.z);
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

        Ok(())
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

        // msec and lightlevel are always written, after the flagged fields
        // (protocol 34 order, movemsg.c MSG_WriteDeltaUsercmd).
        self.write_byte(i32::from(to.msec));
        self.write_byte(i32::from(to.lightlevel));
    }

    /// Read a delta-compressed user command.
    pub fn read_delta_usercmd(&mut self, from: &UserCmd) -> UserCmd {
        let mut cmd = *from;
        let bits = self.read_byte() as u8;

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

        cmd.msec = self.read_byte() as u8;
        cmd.lightlevel = self.read_byte() as u8;

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
    /// opcode and terminated by entity number 0. Mirrors C
    /// `SV_EmitPacketEntities`.
    ///
    /// Both `old_entities` and `new_entities` **must** be sorted by entity number
    /// (ascending). Entities in `old_entities` but absent from `new_entities` are
    /// written with the REMOVE flag; entities that appear in both with identical
    /// state are omitted (the reader copies them forward from the old frame).
    /// A new entity is delta'd from `baselines[number]` (C `sv.baselines`;
    /// missing entries count as all-zero), which is what the client decodes it
    /// against. Entities numbered `<= max_clients` are players and always
    /// refresh old_origin, so they never lerp from a stale position.
    ///
    /// Like C, no more entities are written once the message passes
    /// `MAX_MSGLEN - 150` bytes; the rest go out in a later frame.
    ///
    /// Errors if an entity number is invalid (see
    /// [`write_delta_entity`](Self::write_delta_entity)).
    pub fn write_packet_entities_list(
        &mut self,
        old_entities: &[EntityState],
        new_entities: &[EntityState],
        baselines: &[EntityState],
        max_clients: i32,
    ) -> Q2Result<()> {
        self.write_byte(SvcOp::PacketEntities as i32);

        let zero_baseline = EntityState::default();
        let mut old_idx = 0usize;
        let mut new_idx = 0usize;

        while self.len() <= MAX_MSGLEN - 150 {
            let new_ent = new_entities.get(new_idx);
            match (new_ent, old_entities.get(old_idx)) {
                (Some(new_ent), Some(old_ent)) if new_ent.number == old_ent.number => {
                    // Unforced: unchanged entities emit nothing.
                    let is_player = new_ent.number <= max_clients;
                    self.write_delta_entity(old_ent, new_ent, false, is_player)?;
                    old_idx += 1;
                    new_idx += 1;
                }
                // The old entity is not in the new frame.
                (_, Some(old_ent)) if new_ent.is_none_or(|n| n.number > old_ent.number) => {
                    self.write_entity_remove(old_ent.number);
                    old_idx += 1;
                }
                // A new entity: send it from its baseline.
                (Some(new_ent), _) => {
                    let baseline = usize::try_from(new_ent.number)
                        .ok()
                        .and_then(|n| baselines.get(n))
                        .unwrap_or(&zero_baseline);
                    self.write_delta_entity(baseline, new_ent, true, true)?;
                    new_idx += 1;
                }
                (None, _) => break,
            }
        }

        // Terminator: entity number 0 with no flags.
        self.write_byte(0); // flags byte = 0
        self.write_byte(0); // entity number = 0
        Ok(())
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
        buf.write_float(2.5);
        buf.begin_reading();
        let val = buf.read_float();
        assert!((val - 2.5).abs() < 0.001);
    }

    #[test]
    fn write_read_string() {
        let mut buf = NetMsg::new();
        buf.write_string("hello world");
        buf.begin_reading();
        assert_eq!(buf.read_string(), "hello world");
    }

    // Q2 strings are raw bytes (high-bit bytes are coloured console text).
    // Each byte maps to the char with the same code point (Latin-1).
    #[test]
    fn read_string_keeps_high_bit_bytes() {
        let mut buf = NetMsg::from_bytes(&[0xA0, 0x41, 0xFF, 0]);
        assert_eq!(buf.read_string(), "\u{A0}A\u{FF}");
    }

    #[test]
    fn write_string_roundtrips_high_bit_bytes() {
        let mut rd = NetMsg::from_bytes(&[0xA0, 0x41, 0x80, 0]);
        let text = rd.read_string();
        let mut wr = NetMsg::new();
        wr.write_string(&text);
        assert_eq!(wr.data(), &[0xA0, 0x41, 0x80, 0]);
    }

    #[test]
    fn write_string_replaces_chars_outside_one_byte_range() {
        let mut buf = NetMsg::new();
        buf.write_string("a\u{2014}b");
        assert_eq!(buf.data(), b"a?b\0");
    }

    // C MSG_WriteString uses strlen, so an interior NUL ends the string.
    #[test]
    fn write_string_stops_at_interior_nul() {
        let mut buf = NetMsg::new();
        buf.write_string("ab\0cd");
        assert_eq!(buf.data(), b"ab\0");
    }

    // C MSG_ReadString (movemsg.c:896-915) stops after 2047 chars and leaves
    // the rest of the string in the stream.
    #[test]
    fn read_string_caps_at_2047_bytes_like_c() {
        let mut bytes = vec![b'a'; 3000];
        bytes.push(0);
        let mut buf = NetMsg::from_bytes(&bytes);
        assert_eq!(buf.read_string().len(), 2047);
        assert_eq!(buf.read_cursor(), 2047);
    }

    #[test]
    fn read_string_without_terminator_sets_overflow() {
        let mut buf = NetMsg::from_bytes(b"abc");
        assert_eq!(buf.read_string(), "abc");
        assert!(buf.is_overflowed());
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
        buf.write_float(2.5);
        buf.write_string("hello");
        buf.write_coord(100.5);
        buf.write_angle(180.0);

        buf.begin_reading();
        assert_eq!(buf.read_byte(), 42);
        assert_eq!(buf.read_short(), 1234);
        assert_eq!(buf.read_long(), -1);
        assert!((buf.read_float() - 2.5).abs() < 0.001);
        assert_eq!(buf.read_string(), "hello");
        assert!((buf.read_coord() - 100.5).abs() < 0.125);
        // Byte 128 reads back as -180 (C MSG_ReadAngle is signed): the same
        // direction as 180.
        assert!((buf.read_angle().rem_euclid(360.0) - 180.0).abs() < 1.5);
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

    // C MSG_WriteDeltaUsercmd (movemsg.c:290-381): bits byte, then the
    // flagged fields in angle/move/buttons/impulse order, then msec and
    // lightlevel LAST. bits = CM_BUTTONS (1 << 6 = 64).
    #[test]
    fn delta_usercmd_bytes_match_c_field_order() {
        let to = UserCmd {
            msec: 16,
            lightlevel: 99,
            buttons: 1,
            ..Default::default()
        };
        let mut buf = NetMsg::new();
        buf.write_delta_usercmd(&UserCmd::default(), &to);
        assert_eq!(buf.data(), &[64, 1, 16, 99]);
    }

    // All flags: bits 0xFF, angles/moves as LE shorts, buttons, impulse,
    // then msec, lightlevel.
    #[test]
    fn delta_usercmd_all_fields_bytes_match_c() {
        let to = UserCmd {
            msec: 8,
            buttons: 3,
            angles: [0x0102, -2, 3],
            forwardmove: 400,
            sidemove: -200,
            upmove: 200,
            impulse: 7,
            lightlevel: 42,
        };
        let mut buf = NetMsg::new();
        buf.write_delta_usercmd(&UserCmd::default(), &to);
        assert_eq!(
            buf.data(),
            &[
                0xFF, // bits
                0x02, 0x01, // angles[0]
                0xFE, 0xFF, // angles[1] = -2
                0x03, 0x00, // angles[2]
                0x90, 0x01, // forwardmove = 400
                0x38, 0xFF, // sidemove = -200
                0xC8, 0x00, // upmove = 200
                3,    // buttons
                7,    // impulse
                8,    // msec
                42,   // lightlevel
            ]
        );
        let mut rd = NetMsg::from_bytes(buf.data());
        let decoded = rd.read_delta_usercmd(&UserCmd::default());
        assert_eq!(decoded, to);
    }

    #[test]
    fn delta_entity_unchanged_writes_nothing() {
        let state = EntityState {
            number: 1,
            ..Default::default()
        };
        let mut buf = NetMsg::new();
        buf.write_delta_entity(&state, &state, false, false)
            .expect("valid number");
        assert!(buf.is_empty());
    }

    /// Encode `to` against `from` and return the bytes.
    fn delta_bytes(from: &EntityState, to: &EntityState, force: bool, newentity: bool) -> Vec<u8> {
        let mut buf = NetMsg::new();
        buf.write_delta_entity(from, to, force, newentity)
            .expect("valid number");
        buf.data().to_vec()
    }

    // Golden vectors below are hand-derived from C MSG_WriteDeltaEntity
    // (movemsg.c:432-774). Bit values are the U_* defines (common.h):
    // ORIGIN1 1<<0, ANGLE2 1<<2, ANGLE3 1<<3, FRAME8 1<<4, EVENT 1<<5,
    // MOREBITS1 1<<7, NUMBER16 1<<8, ANGLE1 1<<10, EFFECTS8 1<<14,
    // MOREBITS2 1<<15, SKIN8 1<<16, FRAME16 1<<17, RENDERFX16 1<<18,
    // EFFECTS16 1<<19, MOREBITS3 1<<23, OLDORIGIN 1<<24, SKIN16 1<<25.

    // frame >= 256 sets FRAME16 alone (short); angles are MSG_WriteAngle
    // bytes: (int)(10*256/360)=7, 20 -> 14, 30 -> 21.
    // bits = ANGLE1|ANGLE2|ANGLE3|FRAME16 | MOREBITS2|MOREBITS1 = 0x2848C.
    #[test]
    fn delta_entity_frame16_and_byte_angles_match_c() {
        let to = EntityState {
            number: 1,
            frame: 300,
            angles: Vec3f::new(10.0, 20.0, 30.0),
            ..Default::default()
        };
        assert_eq!(
            delta_bytes(&EntityState::default(), &to, true, false),
            vec![0x8C, 0x84, 0x02, 1, 0x2C, 0x01, 7, 14, 21]
        );
    }

    // 256 <= skinnum < 0x10000: SKIN16 alone, short.
    // bits = SKIN16 | MOREBITS3|MOREBITS2|MOREBITS1 = 0x2808080.
    #[test]
    fn delta_entity_skin16_is_short_like_c() {
        let to = EntityState {
            number: 2,
            skinnum: 300,
            ..Default::default()
        };
        assert_eq!(
            delta_bytes(&EntityState::default(), &to, false, false),
            vec![0x80, 0x80, 0x80, 0x02, 2, 0x2C, 0x01]
        );
    }

    // Laser colours: C compares (unsigned)skinnum, so a negative i32 is
    // >= 0x10000 and goes out as SKIN8|SKIN16 + long.
    // bits = SKIN8|SKIN16 | MOREBITS3|2|1 = 0x2818080.
    #[test]
    fn delta_entity_negative_skinnum_is_long_like_c() {
        let to = EntityState {
            number: 2,
            skinnum: 0xd0d1_d2d3_u32 as i32,
            ..Default::default()
        };
        assert_eq!(
            delta_bytes(&EntityState::default(), &to, false, false),
            vec![0x80, 0x80, 0x81, 0x02, 2, 0xD3, 0xD2, 0xD1, 0xD0]
        );
    }

    // 256 <= effects < 0x8000: EFFECTS16 alone, short.
    // bits = EFFECTS16 | MOREBITS2|MOREBITS1 = 0x88080.
    #[test]
    fn delta_entity_effects16_is_short_like_c() {
        let to = EntityState {
            number: 3,
            effects: 300,
            ..Default::default()
        };
        assert_eq!(
            delta_bytes(&EntityState::default(), &to, false, false),
            vec![0x80, 0x80, 0x08, 3, 0x2C, 0x01]
        );
    }

    // effects >= 0x8000: EFFECTS8|EFFECTS16, long.
    // bits = EFFECTS8|EFFECTS16 | MOREBITS2|MOREBITS1 = 0x8C080.
    #[test]
    fn delta_entity_effects_0x8000_is_long_like_c() {
        let to = EntityState {
            number: 3,
            effects: 0x8000,
            ..Default::default()
        };
        assert_eq!(
            delta_bytes(&EntityState::default(), &to, false, false),
            vec![0x80, 0xC0, 0x08, 3, 0x00, 0x80, 0x00, 0x00]
        );
    }

    // 256 <= renderfx < 0x8000: RENDERFX16 alone, short.
    // bits = RENDERFX16 | MOREBITS2|MOREBITS1 = 0x48080.
    #[test]
    fn delta_entity_renderfx16_is_short_like_c() {
        let to = EntityState {
            number: 3,
            renderfx: 300,
            ..Default::default()
        };
        assert_eq!(
            delta_bytes(&EntityState::default(), &to, false, false),
            vec![0x80, 0x80, 0x04, 3, 0x2C, 0x01]
        );
    }

    // "event is not delta compressed, just 0 compressed": a repeated event
    // is sent again. bits = EVENT = 0x20.
    #[test]
    fn delta_entity_resends_unchanged_nonzero_event() {
        let from = EntityState {
            number: 4,
            event: 3,
            ..Default::default()
        };
        assert_eq!(delta_bytes(&from, &from, false, false), vec![0x20, 4, 3]);
    }

    // OLDORIGIN only for new entities or RF_BEAM; a moving entity whose
    // old_origin changed sends ORIGIN1 only (bits 0x01, coord 8*8 = 64).
    #[test]
    fn delta_entity_moving_non_beam_omits_oldorigin() {
        let to = EntityState {
            number: 5,
            origin: Vec3f::new(8.0, 0.0, 0.0),
            old_origin: Vec3f::new(8.0, 0.0, 0.0),
            ..Default::default()
        };
        assert_eq!(
            delta_bytes(&EntityState::default(), &to, false, false),
            vec![0x01, 5, 0x40, 0x00]
        );
    }

    // RF_BEAM (128) forces OLDORIGIN even when nothing changed.
    // bits = OLDORIGIN | MOREBITS3|2|1 = 0x1808080; coords 1,2,3 -> 8,16,24.
    #[test]
    fn delta_entity_beam_always_sends_oldorigin() {
        let beam = EntityState {
            number: 6,
            renderfx: 128,
            old_origin: Vec3f::new(1.0, 2.0, 3.0),
            ..Default::default()
        };
        assert_eq!(
            delta_bytes(&beam, &beam, false, false),
            vec![0x80, 0x80, 0x80, 0x01, 6, 8, 0, 16, 0, 24, 0]
        );
    }

    // C sets U_NUMBER16 before the "nothing to send" check, so an unchanged
    // entity >= 256 is still written: bits = NUMBER16|MOREBITS1 = 0x180.
    #[test]
    fn delta_entity_unchanged_number16_is_still_written_like_c() {
        let ent = EntityState {
            number: 300,
            ..Default::default()
        };
        assert_eq!(
            delta_bytes(&ent, &ent, false, false),
            vec![0x80, 0x01, 0x2C, 0x01]
        );
    }

    // C SV_EmitPacketEntities: a new entity is sent with force and
    // newentity from sv.baselines[n]. Baseline modelindex 5 equals the
    // state, so only OLDORIGIN is set: bits 0x1808080, number 5, 3 coords,
    // then the 2-byte terminator.
    #[test]
    fn packet_entities_new_entity_deltas_from_baseline() {
        let mut baselines = vec![EntityState::default(); 8];
        baselines[5] = EntityState {
            number: 5,
            modelindex: 5,
            ..Default::default()
        };
        let ent = baselines[5].clone();
        let mut buf = NetMsg::new();
        buf.write_packet_entities_list(&[], &[ent], &baselines, 0)
            .expect("valid entities");
        assert_eq!(
            buf.data(),
            &[18, 0x80, 0x80, 0x80, 0x01, 5, 0, 0, 0, 0, 0, 0, 0, 0]
        );
    }

    // "players are always 'newentities'": an unchanged entity with number
    // <= maxclients still sends OLDORIGIN; other unchanged entities send
    // nothing.
    #[test]
    fn packet_entities_unchanged_player_still_sends_oldorigin() {
        let player = EntityState {
            number: 1,
            old_origin: Vec3f::new(1.0, 0.0, 0.0),
            ..Default::default()
        };
        let mut buf = NetMsg::new();
        let list = std::slice::from_ref(&player);
        buf.write_packet_entities_list(list, list, &[], 4)
            .expect("valid entities");
        assert_eq!(
            buf.data(),
            &[18, 0x80, 0x80, 0x80, 0x01, 1, 8, 0, 0, 0, 0, 0, 0, 0]
        );

        let monster = EntityState {
            number: 5,
            ..player
        };
        let mut buf = NetMsg::new();
        let list = std::slice::from_ref(&monster);
        buf.write_packet_entities_list(list, list, &[], 4)
            .expect("valid entities");
        assert_eq!(buf.data(), &[18, 0, 0]);
    }

    // C stops emitting entities once the message passes MAX_MSGLEN - 150;
    // the rest are sent in a later frame.
    #[test]
    fn packet_entities_stop_near_max_msglen() {
        let ents: Vec<EntityState> = (1..1024)
            .map(|number| EntityState {
                number,
                modelindex: 1,
                ..Default::default()
            })
            .collect();
        let mut buf = NetMsg::new();
        buf.write_packet_entities_list(&[], &ents, &[], 0)
            .expect("valid entities");
        let max_msglen = q2_shared::constants::MAX_MSGLEN;
        assert!(buf.len() <= max_msglen - 150 + 32, "len {}", buf.len());
        assert_eq!(&buf.data()[buf.len() - 2..], &[0, 0]);
    }

    // C: Com_Error(ERR_FATAL) for "Unset entity number" and
    // "Entity number >= MAX_EDICTS". Number 0 would also look like the
    // packet-entities terminator.
    #[test]
    fn delta_entity_rejects_number_0_and_max_edicts() {
        for number in [0, 1024, -1] {
            let to = EntityState {
                number,
                modelindex: 1,
                ..Default::default()
            };
            let mut buf = NetMsg::new();
            assert!(
                buf.write_delta_entity(&EntityState::default(), &to, true, true)
                    .is_err(),
                "number {number} must be rejected"
            );
            assert!(buf.is_empty());
        }
    }

    #[test]
    fn delta_entity_changed_fields() {
        let from = EntityState::default();
        let to = EntityState {
            number: 1,
            origin: Vec3f::new(100.0, 200.0, 0.0),
            modelindex: 5,
            ..Default::default()
        };

        let mut buf = NetMsg::new();
        buf.write_delta_entity(&from, &to, true, true)
            .expect("valid number");

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
        let result = buf.read_dir().expect("index from write_dir is valid");
        // Should be close to the original direction
        let dot = result.x * dir.x + result.y * dir.y + result.z * dir.z;
        assert!(dot > 0.9, "Direction roundtrip lost too much precision");
    }

    // C MSG_ReadAngle: MSG_ReadChar * 1.40625, so 0xC0 = -64 -> -90 degrees.
    #[test]
    fn read_angle_0xc0_is_minus_90() {
        let mut buf = NetMsg::from_bytes(&[0xC0]);
        assert_eq!(buf.read_angle(), -90.0);
    }

    // C MSG_ReadDir raises ERR_DROP for an index >= NUMVERTEXNORMALS (162).
    #[test]
    fn read_dir_index_162_is_error() {
        let mut buf = NetMsg::from_bytes(&[162]);
        assert!(matches!(buf.read_dir(), Err(Q2Error::Drop(_))));
    }

    #[test]
    fn read_dir_past_end_is_error() {
        let mut buf = NetMsg::from_bytes(&[]);
        assert!(buf.read_dir().is_err());
    }

    #[test]
    fn read_dir_index_161_is_last_bytedir() {
        let mut buf = NetMsg::from_bytes(&[161]);
        let dir = buf.read_dir().expect("161 is in range");
        assert_eq!(dir, Vec3f::new(-0.688191, -0.587785, -0.425325));
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

    // C SZ_GetSpace (szone.c:45-71) with allowoverflow: a write that would
    // pass maxsize clears the buffer and sets `overflowed`.
    #[test]
    fn write_past_max_size_sets_overflowed_and_clears() {
        let mut buf = NetMsg::with_max_size(4);
        buf.write_long(7);
        assert_eq!(buf.len(), 4);
        assert!(!buf.is_overflowed());

        buf.write_byte(1);
        assert!(buf.is_overflowed());
        assert!(buf.is_empty());
    }

    #[test]
    fn write_data_past_max_size_sets_overflowed() {
        let mut buf = NetMsg::with_max_size(3);
        buf.write_data(&[1, 2, 3, 4]);
        assert!(buf.is_overflowed());
        assert!(buf.is_empty());
    }

    #[test]
    fn clear_resets_write_overflow() {
        let mut buf = NetMsg::with_max_size(1);
        buf.write_short(1);
        assert!(buf.is_overflowed());
        buf.clear();
        assert!(!buf.is_overflowed());
        buf.write_byte(9);
        assert_eq!(buf.data(), &[9]);
    }

    #[test]
    fn default_creates_empty() {
        let buf = NetMsg::default();
        assert!(buf.is_empty());
        assert_eq!(buf.len(), 0);
    }
}
