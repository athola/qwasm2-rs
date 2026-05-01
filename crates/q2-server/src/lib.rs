//! Qwasm2-rs: Dedicated server — state management, client handling, game interface
//!
//! # Unsafe Audit Status
//! - Total unsafe blocks: 0
//! - c2rust mechanical: 0
//! - FFI boundary: 0
//! - Performance: 0
//! - Inherent: 0

pub mod frame;
pub mod game_iface;
pub mod init;
pub mod state;
pub mod world;

pub use game_iface::ServerGameImport;
