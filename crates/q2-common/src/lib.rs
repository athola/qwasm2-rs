//! Qwasm2-rs: Engine common layer — error handling, cvars, commands, filesystem, network messages
//!
//! # Unsafe Audit Status
//! - Total unsafe blocks: 0
//! - c2rust mechanical: 0
//! - FFI boundary: 0
//! - Performance: 0
//! - Inherent: 0

pub mod binary;
pub mod cmd;
pub mod collision;
pub mod cvar;
pub mod error;
pub mod filesystem;
pub mod netchan;
pub mod net_msg;
pub mod player_ctrl;
pub mod pmove;
pub mod zone;
pub use error::{Q2Error, Q2Result};
pub use q2_shared::CVarHandle;
