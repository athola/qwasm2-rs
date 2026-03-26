//! Qwasm2-rs: Engine common layer — error handling, cvars, commands, filesystem, network messages
//!
//! # Unsafe Audit Status
//! - Total unsafe blocks: 0
//! - c2rust mechanical: 0
//! - FFI boundary: 0
//! - Performance: 0
//! - Inherent: 0

pub mod cmd;
pub mod cvar;
pub mod error;
pub mod net_msg;
pub use error::{Q2Error, Q2Result};
