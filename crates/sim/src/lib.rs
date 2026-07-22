//! The deterministic core of the moe transit arcade.
//!
//! Everything in this crate is a pure function of its inputs: no rendering,
//! no audio, no wall clock, no I/O, no randomness that isn't passed in.
//! This is the rollback-netcode contract (design doc §5) — peers exchange
//! inputs and re-derive identical state, so anything nondeterministic in
//! here desyncs the game. CI runs the determinism test on every PR.

pub mod arena;
pub mod judgment;
pub mod movement;

pub use judgment::{judge, BeatGrid, Judgment};
pub use movement::{step, NetInput, PlayerState};

/// Fixed simulation rate. All gameplay time is ticks of this.
pub const TICK_HZ: u32 = 60;

/// Seconds → ticks (for bridging the presentation clock into sim space).
pub fn secs_to_ticks(secs: f64) -> i64 {
    (secs * TICK_HZ as f64).round() as i64
}
