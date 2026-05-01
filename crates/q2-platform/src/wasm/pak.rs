//! WASM PAK backend: JS-heap `Uint8Array` as a lazy `PakReader`.
//!
//! The full PAK bytes stay in the JS heap after fetch.  Only the directory
//! index (a few hundred KB at most) is parsed into Rust memory at open time.
//! Individual asset reads slice the `Uint8Array` on demand, copying only the
//! requested bytes into WASM linear memory.  Between level transitions the
//! caller drops loaded asset buffers; WASM memory returns to baseline.

use js_sys::Uint8Array;
use q2_common::{
    error::{Q2Error, Q2Result},
    filesystem::PakReader,
};

/// PAK reader backed by a JS-heap `Uint8Array`.
///
/// The array is kept alive for the lifetime of the `Pack` that owns this
/// reader.  It is never fully copied into Rust memory.
pub struct JsPakReader {
    array: Uint8Array,
}

impl JsPakReader {
    pub fn new(array: Uint8Array) -> Self {
        Self { array }
    }
}

// SAFETY: WASM targets compile to a single-threaded environment; there are no
// OS threads, so Send + Sync cannot be violated.  js_sys types lack these
// impls only because the Rust/JS interop crate is conservative by default.
unsafe impl Send for JsPakReader {}
unsafe impl Sync for JsPakReader {}

impl PakReader for JsPakReader {
    fn read_slice(&self, offset: u32, len: u32) -> Q2Result<Vec<u8>> {
        let end = offset
            .checked_add(len)
            .ok_or_else(|| Q2Error::Drop("JsPakReader: offset overflow".into()))?;
        if end > self.array.length() {
            return Err(Q2Error::Drop(format!(
                "JsPakReader: read [{offset}, {end}) out of bounds (len={})",
                self.array.length()
            )));
        }
        // Uint8Array::slice copies [offset, end) from the JS buffer into a new
        // typed array, then to_vec() moves those bytes into WASM linear memory.
        Ok(self.array.slice(offset, end).to_vec())
    }
}
