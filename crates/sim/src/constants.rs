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

/// Player body (collision is an axis-aligned box of this footprint).
pub mod body {
    pub const RADIUS: f32 = 0.3;
    pub const STAND_HEIGHT: f32 = 1.8;
    pub const CROUCH_HEIGHT: f32 = 1.2;
    /// Highest ledge walked up without jumping (stairs are 0.2 m).
    pub const STEP_HEIGHT: f32 = 0.35;
    /// Obstacles between these heights can be vaulted with Jump.
    pub const VAULT_MIN: f32 = 0.45;
    pub const VAULT_MAX: f32 = 1.25;
    pub const LEAN_SPEED: f32 = 6.0;
}

/// Stamina is 0..1: about 5.5 s of sprinting from full.
pub mod stamina {
    pub const SPRINT_DRAIN: f32 = 0.18;
    pub const REGEN: f32 = 0.22;
    pub const JUMP_COST: f32 = 0.08;
    /// After running dry, sprint unlocks again at this level.
    pub const RECOVER: f32 = 0.35;
}

/// Slap mode: open-hand melee with knockback, stagger and knockouts.
pub mod slap {
    pub const RANGE: f32 = 1.8;
    /// cos(55°): target must be inside this cone in front of the slapper.
    pub const CONE_COS: f32 = 0.574;
    pub const COOLDOWN: f32 = 0.65;
    pub const KNOCKBACK: f32 = 6.5;
    pub const LIFT: f32 = 2.2;
    pub const STUN: f32 = 0.45;
    pub const DAZE_PER_HIT: f32 = 34.0;
    pub const DAZE_DECAY: f32 = 8.0;
    pub const KO_TIME: f32 = 3.5;
}

/// Combat mode: health, death and where shots leave from.
pub mod combat {
    pub const MAX_HP: f32 = 100.0;
    /// Health starts coming back after this long without taking damage.
    pub const REGEN_DELAY: f32 = 5.0;
    pub const REGEN_RATE: f32 = 15.0;
    pub const RESPAWN_TIME: f32 = 4.0;
    /// Shots leave from a point beside the head (over the right shoulder), which is
    /// also where the third-person camera's aim ray passes, so the crosshair is exact.
    pub const SHOULDER: f32 = 0.5;
    pub const LEAN_SHIFT: f32 = 0.45;
    /// Bullets are dropped after this long in flight.
    pub const BULLET_LIFE: f32 = 2.5;
}

/// Match rules per mode.
pub mod rules {
    pub const FFA_KILLS: u32 = 25;
    pub const TDM_KILLS: u32 = 50;
    pub const DOM_POINTS: u32 = 200;
    pub const SLAP_KOS: u32 = 10;
    /// Match length in seconds.
    pub const MATCH_TIME: f32 = 600.0;
    /// Results screen before the next match.
    pub const RESULTS_TIME: f32 = 12.0;
    /// Capture points: radius, seconds to take one alone, points per second per point held.
    pub const FLAG_RADIUS: f32 = 4.0;
    pub const CAPTURE_TIME: f32 = 6.0;
    pub const POINTS_EVERY: f32 = 1.0;
    /// Bots keep a lone player company; they leave when a second person joins.
    pub const BOTS_FOR_LONE_PLAYER: usize = 2;
}
