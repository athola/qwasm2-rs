//! Binary read helpers for little-endian data (BSP files, PAK files, etc.).
//!
//! The `read_*` functions panic on out-of-bounds access (suitable for
//! already-validated data). The `try_read_*` variants return `Q2Result`
//! and should be used when parsing untrusted or potentially malformed data.

use crate::{Q2Error, Q2Result};

#[inline]
pub fn read_u16(data: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([data[off], data[off + 1]])
}

#[inline]
pub fn read_i16(data: &[u8], off: usize) -> i16 {
    i16::from_le_bytes([data[off], data[off + 1]])
}

#[inline]
pub fn read_u32(data: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

#[inline]
pub fn read_i32(data: &[u8], off: usize) -> i32 {
    i32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

#[inline]
pub fn read_f32(data: &[u8], off: usize) -> f32 {
    f32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

// ---------------------------------------------------------------------------
// Bounds-checked variants — return Q2Result instead of panicking
// ---------------------------------------------------------------------------

#[inline]
pub fn try_read_u16(data: &[u8], off: usize) -> Q2Result<u16> {
    let b = data.get(off..off + 2).ok_or(Q2Error::Drop("u16 read out of bounds".into()))?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}

#[inline]
pub fn try_read_i16(data: &[u8], off: usize) -> Q2Result<i16> {
    let b = data.get(off..off + 2).ok_or(Q2Error::Drop("i16 read out of bounds".into()))?;
    Ok(i16::from_le_bytes([b[0], b[1]]))
}

#[inline]
pub fn try_read_u32(data: &[u8], off: usize) -> Q2Result<u32> {
    let b = data.get(off..off + 4).ok_or(Q2Error::Drop("u32 read out of bounds".into()))?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

#[inline]
pub fn try_read_i32(data: &[u8], off: usize) -> Q2Result<i32> {
    let b = data.get(off..off + 4).ok_or(Q2Error::Drop("i32 read out of bounds".into()))?;
    Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

#[inline]
pub fn try_read_f32(data: &[u8], off: usize) -> Q2Result<f32> {
    let b = data.get(off..off + 4).ok_or(Q2Error::Drop("f32 read out of bounds".into()))?;
    Ok(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_roundtrips() {
        let mut buf = [0u8; 8];
        buf[0..2].copy_from_slice(&42u16.to_le_bytes());
        buf[2..4].copy_from_slice(&(-7i16).to_le_bytes());
        buf[4..8].copy_from_slice(&3.14f32.to_le_bytes());
        assert_eq!(read_u16(&buf, 0), 42);
        assert_eq!(read_i16(&buf, 2), -7);
        assert!((read_f32(&buf, 4) - 3.14).abs() < 0.001);
    }

    #[test]
    fn try_read_roundtrips() {
        let mut buf = [0u8; 8];
        buf[0..2].copy_from_slice(&42u16.to_le_bytes());
        buf[2..4].copy_from_slice(&(-7i16).to_le_bytes());
        buf[4..8].copy_from_slice(&3.14f32.to_le_bytes());
        assert_eq!(try_read_u16(&buf, 0).unwrap(), 42);
        assert_eq!(try_read_i16(&buf, 2).unwrap(), -7);
        assert!((try_read_f32(&buf, 4).unwrap() - 3.14).abs() < 0.001);
    }

    #[test]
    fn try_read_oob_returns_error() {
        assert!(try_read_u16(&[0u8; 1], 0).is_err());
        assert!(try_read_i16(&[0u8; 1], 0).is_err());
        assert!(try_read_u32(&[0u8; 3], 0).is_err());
        assert!(try_read_i32(&[0u8; 3], 0).is_err());
        assert!(try_read_f32(&[0u8; 3], 0).is_err());
    }

    #[test]
    fn try_read_empty_returns_error() {
        assert!(try_read_u16(&[], 0).is_err());
        assert!(try_read_u32(&[], 0).is_err());
    }

    #[test]
    #[should_panic]
    fn read_u16_oob_panics() {
        read_u16(&[0u8; 1], 0);
    }

    #[test]
    #[should_panic]
    fn read_i16_oob_panics() {
        read_i16(&[0u8; 1], 0);
    }

    #[test]
    #[should_panic]
    fn read_u32_oob_panics() {
        read_u32(&[0u8; 3], 0);
    }

    #[test]
    #[should_panic]
    fn read_i32_oob_panics() {
        read_i32(&[0u8; 3], 0);
    }

    #[test]
    #[should_panic]
    fn read_f32_oob_panics() {
        read_f32(&[0u8; 3], 0);
    }

    #[test]
    #[should_panic]
    fn read_u16_empty_panics() {
        read_u16(&[], 0);
    }

    #[test]
    #[should_panic]
    fn read_u32_offset_oob_panics() {
        read_u32(&[0u8; 8], 6);
    }
}
