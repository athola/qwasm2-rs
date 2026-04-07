//! Tag-based memory pool system.
//! Replaces C zone.c/szone.c with safe Rust Vec pools.
//! In Quake 2, zone memory is used for:
//! - Level data (freed on map change)
//! - Temporary allocations (freed per-frame)
//! - Persistent data (freed on shutdown)

use std::collections::HashMap;

/// Memory tags — identifies what owns the allocation so groups can be freed together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemTag {
    /// Freed every frame.
    Temp,
    /// Freed on map change.
    Level,
    /// Freed on game shutdown.
    Game,
    /// Persistent (freed only on full shutdown).
    Persistent,
}

/// A tagged memory pool. Stores byte buffers organized by tag.
///
/// When a tag is freed, all allocations with that tag are dropped at once —
/// matching the C `Z_FreeTags` / `Hunk_FreeToLowMark` semantics without any
/// raw pointer manipulation.
pub struct Zone {
    pools: HashMap<MemTag, Vec<Vec<u8>>>,
    /// Track total bytes per tag for diagnostics.
    tag_bytes: HashMap<MemTag, usize>,
    /// Maximum total bytes allowed (`0` = unlimited).
    max_bytes: usize,
    /// Current total bytes across all tags.
    total_bytes: usize,
}

impl Zone {
    /// Create a new zone.
    ///
    /// `max_bytes` is a soft cap on the total number of bytes that may be
    /// allocated at once.  Pass `0` for unlimited (useful in tests).
    pub fn new(max_bytes: usize) -> Self {
        Self {
            pools: HashMap::new(),
            tag_bytes: HashMap::new(),
            max_bytes,
            total_bytes: 0,
        }
    }

    /// Allocate `size` bytes with the given tag. Returns a mutable slice into
    /// the freshly-zeroed buffer.
    ///
    /// # Panics
    ///
    /// Panics when `max_bytes` is non-zero and the allocation would exceed it.
    /// This mirrors the original C `Sys_Error("Z_Malloc: failed on allocation")`.
    pub fn alloc(&mut self, tag: MemTag, size: usize) -> &mut [u8] {
        if self.max_bytes != 0 && self.total_bytes + size > self.max_bytes {
            tracing::error!(
                "Zone::alloc: out of memory — requested {size} bytes (have {}/{} used)",
                self.total_bytes, self.max_bytes
            );
            panic!(
                "Zone::alloc: out of memory — requested {size} bytes (have {}/{} used)",
                self.total_bytes, self.max_bytes
            );
        }

        // Grow accounting.
        self.total_bytes += size;
        *self.tag_bytes.entry(tag).or_insert(0) += size;

        // Push a zeroed buffer into the pool and return a mutable slice to it.
        let pool = self.pools.entry(tag).or_default();
        pool.push(vec![0u8; size]);
        pool.last_mut().expect("just pushed").as_mut_slice()
    }

    /// Free all allocations with the given tag.
    ///
    /// After this call `tag_allocated(tag) == 0`.  Allocations under other
    /// tags are unaffected.
    pub fn free_tag(&mut self, tag: MemTag) {
        if let Some(pool) = self.pools.remove(&tag) {
            let freed: usize = pool.iter().map(|v| v.len()).sum();
            self.total_bytes = self.total_bytes.saturating_sub(freed);
            self.tag_bytes.remove(&tag);
        }
    }

    /// Total bytes currently allocated across all tags.
    pub fn total_allocated(&self) -> usize {
        self.total_bytes
    }

    /// Bytes currently allocated under `tag`.
    pub fn tag_allocated(&self, tag: MemTag) -> usize {
        self.tag_bytes.get(&tag).copied().unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Allocate a buffer, write bytes into it, and read them back.
    #[test]
    fn alloc_and_read() {
        let mut zone = Zone::new(0);
        let buf = zone.alloc(MemTag::Temp, 4);
        buf[0] = 0xDE;
        buf[1] = 0xAD;
        buf[2] = 0xBE;
        buf[3] = 0xEF;
        assert_eq!(buf, &[0xDE, 0xAD, 0xBE, 0xEF]);
    }

    /// Buffers start zeroed.
    #[test]
    fn alloc_is_zeroed() {
        let mut zone = Zone::new(0);
        let buf = zone.alloc(MemTag::Level, 8);
        assert_eq!(buf, &[0u8; 8]);
    }

    /// After freeing a tag, `total_allocated` drops by exactly the freed bytes.
    #[test]
    fn free_tag_releases_memory() {
        let mut zone = Zone::new(0);
        zone.alloc(MemTag::Temp, 64);
        assert_eq!(zone.total_allocated(), 64);

        zone.free_tag(MemTag::Temp);
        assert_eq!(zone.total_allocated(), 0);
        assert_eq!(zone.tag_allocated(MemTag::Temp), 0);
    }

    /// Freeing one tag must not disturb allocations under another tag.
    #[test]
    fn free_tag_doesnt_affect_others() {
        let mut zone = Zone::new(0);
        zone.alloc(MemTag::Temp, 32);
        zone.alloc(MemTag::Level, 48);

        zone.free_tag(MemTag::Temp);

        assert_eq!(zone.tag_allocated(MemTag::Temp), 0);
        assert_eq!(zone.tag_allocated(MemTag::Level), 48);
        assert_eq!(zone.total_allocated(), 48);
    }

    /// `total_allocated` and `tag_allocated` must reflect every allocation
    /// made across several tags.
    #[test]
    fn total_tracking() {
        let mut zone = Zone::new(0);
        zone.alloc(MemTag::Game, 100);
        zone.alloc(MemTag::Game, 200);
        zone.alloc(MemTag::Persistent, 50);

        assert_eq!(zone.tag_allocated(MemTag::Game), 300);
        assert_eq!(zone.tag_allocated(MemTag::Persistent), 50);
        assert_eq!(zone.total_allocated(), 350);
    }

    /// Allocating up to the limit must succeed; one byte over must panic.
    #[test]
    fn max_bytes_enforcement() {
        let mut zone = Zone::new(128);

        // Filling up to the exact limit is fine.
        zone.alloc(MemTag::Temp, 64);
        zone.alloc(MemTag::Temp, 64);
        assert_eq!(zone.total_allocated(), 128);

        // Any further allocation must panic.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            zone.alloc(MemTag::Temp, 1);
        }));
        assert!(result.is_err(), "expected a panic when max_bytes is exceeded");
    }

    /// Freeing a tag that was never allocated must be a no-op (no panic).
    #[test]
    fn free_empty_tag_is_noop() {
        let mut zone = Zone::new(0);
        zone.free_tag(MemTag::Level); // must not panic
        assert_eq!(zone.total_allocated(), 0);
    }

    /// Multiple small allocations under the same tag all count toward
    /// `tag_allocated`.
    #[test]
    fn multiple_allocs_same_tag() {
        let mut zone = Zone::new(0);
        for _ in 0..10 {
            zone.alloc(MemTag::Level, 16);
        }
        assert_eq!(zone.tag_allocated(MemTag::Level), 160);
        assert_eq!(zone.total_allocated(), 160);
    }

    /// After freeing and re-allocating under the same tag, accounting must
    /// remain consistent.
    #[test]
    fn realloc_after_free() {
        let mut zone = Zone::new(0);
        zone.alloc(MemTag::Temp, 32);
        zone.free_tag(MemTag::Temp);

        zone.alloc(MemTag::Temp, 16);
        assert_eq!(zone.tag_allocated(MemTag::Temp), 16);
        assert_eq!(zone.total_allocated(), 16);
    }
}
