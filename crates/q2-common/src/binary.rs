//! Bounds-checked little-endian read helpers for untrusted data (BSP files,
//! PAK files, model formats). Out-of-range reads return `Q2Error::Drop`.

use crate::{Q2Error, Q2Result};

/// The error for a `what` read at `off`, built only on failure.
fn out_of_bounds(what: &str, off: usize) -> Q2Error {
    Q2Error::Drop(format!("{what} read out of bounds at offset {off}"))
}

#[inline]
pub fn try_read_u16(data: &[u8], off: usize) -> Q2Result<u16> {
    let b = off
        .checked_add(2)
        .and_then(|end| data.get(off..end))
        .ok_or_else(|| out_of_bounds("u16", off))?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}

#[inline]
pub fn try_read_i16(data: &[u8], off: usize) -> Q2Result<i16> {
    let b = off
        .checked_add(2)
        .and_then(|end| data.get(off..end))
        .ok_or_else(|| out_of_bounds("i16", off))?;
    Ok(i16::from_le_bytes([b[0], b[1]]))
}

#[inline]
pub fn try_read_u32(data: &[u8], off: usize) -> Q2Result<u32> {
    let b = off
        .checked_add(4)
        .and_then(|end| data.get(off..end))
        .ok_or_else(|| out_of_bounds("u32", off))?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

#[inline]
pub fn try_read_i32(data: &[u8], off: usize) -> Q2Result<i32> {
    let b = off
        .checked_add(4)
        .and_then(|end| data.get(off..end))
        .ok_or_else(|| out_of_bounds("i32", off))?;
    Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

#[inline]
pub fn try_read_f32(data: &[u8], off: usize) -> Q2Result<f32> {
    let b = off
        .checked_add(4)
        .and_then(|end| data.get(off..end))
        .ok_or_else(|| out_of_bounds("f32", off))?;
    Ok(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_read_roundtrips() {
        let mut buf = [0u8; 8];
        buf[0..2].copy_from_slice(&42u16.to_le_bytes());
        buf[2..4].copy_from_slice(&(-7i16).to_le_bytes());
        buf[4..8].copy_from_slice(&2.5f32.to_le_bytes());
        assert_eq!(try_read_u16(&buf, 0).unwrap(), 42);
        assert_eq!(try_read_i16(&buf, 2).unwrap(), -7);
        assert!((try_read_f32(&buf, 4).unwrap() - 2.5).abs() < 0.001);
    }

    #[test]
    fn try_read_error_names_the_offset() {
        let err = try_read_i32(&[0u8; 6], 4).unwrap_err().to_string();
        assert!(err.contains("i32 read out of bounds at offset 4"), "{err}");
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
    fn try_read_offset_near_usize_max_returns_error() {
        let buf = [0u8; 8];
        assert!(try_read_u16(&buf, usize::MAX).is_err());
        assert!(try_read_i16(&buf, usize::MAX - 1).is_err());
        assert!(try_read_u32(&buf, usize::MAX - 2).is_err());
        assert!(try_read_i32(&buf, usize::MAX).is_err());
        assert!(try_read_f32(&buf, usize::MAX).is_err());
    }

    #[test]
    fn try_read_empty_returns_error() {
        assert!(try_read_u16(&[], 0).is_err());
        assert!(try_read_u32(&[], 0).is_err());
    }
}
