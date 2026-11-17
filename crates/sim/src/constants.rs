//! Rates and real-world movement values. Distances in metres, time in seconds.

pub const TICK_RATE: u32 = 60;
pub const TICK_DT: f32 = 1.0 / TICK_RATE as f32;
pub const SNAPSHOT_RATE: u32 = 30;
pub const INTERP_DELAY_MS: f64 = 100.0;
pub const MAX_REWIND_MS: f64 = 250.0;
pub const MAX_PLAYERS: usize = 10;
pub const DEFAULT_PORT: u16 = 8080;

/// Human-scale movement (an armed, equipped soldier, not an arcade hero).
pub mod movement {
    pub const GRAVITY: f32 = 9.81;
    pub const WALK_SPEED: f32 = 3.0;
    pub const SPRINT_SPEED: f32 = 6.0;
    pub const CROUCH_SPEED: f32 = 1.5;
    /// Takeoff speed for a ~0.45 m jump: sqrt(2 * g * h).
    pub const JUMP_SPEED: f32 = 2.97;
    /// Ground acceleration / braking, so starts and stops have weight.
    pub const GROUND_ACCEL: f32 = 30.0;
    pub const AIR_ACCEL: f32 = 3.0;
    pub const EYE_HEIGHT: f32 = 1.65;
    pub const CROUCH_EYE_HEIGHT: f32 = 1.05;
    pub const MAX_PITCH: f32 = 1.48;
}
