//! Qwasm2-rs: Game logic — entities, AI, combat, weapons, items, physics, save/load
//!
//! # Unsafe Audit Status
//! - Total unsafe blocks: 0
//! - c2rust mechanical: 0
//! - FFI boundary: 0
//! - Performance: 0
//! - Inherent: 0

pub mod constants;
pub mod traits;
pub mod entity;
pub mod world;
pub mod physics;
pub mod combat;
pub mod items;
pub mod weapons;
pub mod ai;
pub mod monster;
pub mod triggers;
pub mod targets;
pub mod func;
pub mod misc;
pub mod spawn;
