//! Qwasm2-rs: Game logic — entity storage, entity-string spawning, and the
//! GameImport/GameExport traits
//!
//! # Unsafe Audit Status
//! - Total unsafe blocks: 0
//! - c2rust mechanical: 0
//! - FFI boundary: 0
//! - Performance: 0
//! - Inherent: 0

pub mod entity;
pub mod game;
pub mod spawn;
pub mod traits;

pub use game::GameLogic;
