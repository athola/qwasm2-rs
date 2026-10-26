//! Qwasm2-rs: Game logic — entities, spawning, world/physics, combat, items,
//! weapons, AI, monsters, player, triggers, and the GameImport/GameExport traits
//!
//! # Unsafe Audit Status
//! - Total unsafe blocks: 0
//! - c2rust mechanical: 0
//! - FFI boundary: 0
//! - Performance: 0
//! - Inherent: 0

pub mod ai;
pub mod combat;
pub mod constants;
pub mod entity;
pub mod func;
pub mod game;
pub mod items;
pub mod misc;
pub mod monster;
pub mod physics;
pub mod player;
pub mod spawn;
pub mod targets;
pub mod traits;
pub mod triggers;
pub mod weapons;
pub mod world;

pub use game::GameLogic;
