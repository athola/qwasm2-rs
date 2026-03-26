//! Binary read helpers for little-endian data (BSP files, PAK files, etc.).

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
}
