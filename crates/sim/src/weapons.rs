//! Weapon table. Values are based on real weapon classes (muzzle velocity, rate of
//! fire, magazine size, reload times), with damage tuned for 100 hp soldiers.
//! Angles are radians, distances metres, times seconds.

use serde::Serialize;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeaponSpec {
    pub name: &'static str,
    /// Rounds per magazine (shells for the shotgun).
    pub mag: u16,
    /// Spare rounds carried at spawn.
    pub reserve: u16,
    /// Seconds between shots (60 / rpm). For the bolt rifle this includes cycling the bolt.
    pub interval: f32,
    /// Holding the trigger keeps firing (can be switched to semi with the fire-mode key).
    pub auto: bool,
    pub pellets: u8,
    pub muzzle_velocity: f32,
    /// Exponential velocity loss per second (air drag): v = v0 * e^(-drag * t).
    pub drag: f32,
    /// Damage per bullet/pellet to the torso at close range.
    pub damage: f32,
    /// Full damage up to `falloff_start`, then linearly down to `falloff_min` x at `falloff_end`.
    pub falloff_start: f32,
    pub falloff_end: f32,
    pub falloff_min: f32,
    pub head_mult: f32,
    pub limb_mult: f32,
    /// Cone half-angle of the shot from the hip and fully aimed.
    pub hip_spread: f32,
    pub ads_spread: f32,
    /// Extra spread per m/s of movement.
    pub move_spread: f32,
    /// Spread added per shot (bloom), recovering at `BLOOM_DECAY` per second.
    pub bloom: f32,
    /// View kick per shot.
    pub kick_up: f32,
    pub kick_side: f32,
    /// Magazine reload with a round still chambered / from empty.
    pub reload_tactical: f32,
    pub reload_empty: f32,
    /// Shotguns load one shell at a time; this is the time per shell.
    pub reload_per_shell: bool,
    /// Closed-bolt guns keep one round in the chamber on a tactical reload (30 + 1).
    pub chamber: bool,
    /// Time to raise the sights.
    pub ads_time: f32,
    /// Camera field of view when fully aimed (degrees, client only).
    pub ads_fov: f32,
    /// Sway amplitude when aimed (client only).
    pub sway: f32,
}

pub const RIFLE: u8 = 0;
pub const SMG: u8 = 1;
pub const SHOTGUN: u8 = 2;
pub const SNIPER: u8 = 3;
pub const COUNT: usize = 4;

pub const BLOOM_DECAY: f32 = 0.1;
pub const BLOOM_MAX: f32 = 0.06;
pub const SWAP_TIME: f32 = 0.6;
/// Sprinting lowers the weapon; firing needs it raised again.
pub const SPRINT_OUT: f32 = 0.2;
pub const ADS_MOVE_MULT: f32 = 0.6;

pub const WEAPONS: [WeaponSpec; COUNT] = [
    // 5.56 mm carbine
    WeaponSpec {
        name: "Carbine",
        mag: 30,
        reserve: 120,
        interval: 60.0 / 750.0,
        auto: true,
        pellets: 1,
        muzzle_velocity: 880.0,
        drag: 0.9,
        damage: 31.0,
        falloff_start: 45.0,
        falloff_end: 110.0,
        falloff_min: 0.7,
        head_mult: 3.5,
        limb_mult: 0.75,
        hip_spread: 0.040,
        ads_spread: 0.0035,
        move_spread: 0.006,
        bloom: 0.006,
        kick_up: 0.011,
        kick_side: 0.005,
        reload_tactical: 2.1,
        reload_empty: 2.8,
        reload_per_shell: false,
        chamber: true,
        ads_time: 0.25,
        ads_fov: 50.0,
        sway: 0.004,
    },
    // 9 mm submachine gun
    WeaponSpec {
        name: "SMG",
        mag: 30,
        reserve: 150,
        interval: 60.0 / 900.0,
        auto: true,
        pellets: 1,
        muzzle_velocity: 380.0,
        drag: 1.3,
        damage: 23.0,
        falloff_start: 15.0,
        falloff_end: 45.0,
        falloff_min: 0.55,
        head_mult: 2.5,
        limb_mult: 0.8,
        hip_spread: 0.032,
        ads_spread: 0.007,
        move_spread: 0.003,
        bloom: 0.004,
        kick_up: 0.0065,
        kick_side: 0.0055,
        reload_tactical: 1.8,
        reload_empty: 2.4,
        reload_per_shell: false,
        chamber: false,
        ads_time: 0.2,
        ads_fov: 56.0,
        sway: 0.0035,
    },
    // 12 gauge pump, 00 buckshot (9 pellets)
    WeaponSpec {
        name: "Pump Shotgun",
        mag: 7,
        reserve: 28,
        interval: 60.0 / 70.0,
        auto: false,
        pellets: 9,
        muzzle_velocity: 400.0,
        drag: 2.6,
        damage: 15.0,
        falloff_start: 7.0,
        falloff_end: 28.0,
        falloff_min: 0.15,
        head_mult: 1.6,
        limb_mult: 0.8,
        hip_spread: 0.055,
        ads_spread: 0.040,
        move_spread: 0.002,
        bloom: 0.0,
        kick_up: 0.06,
        kick_side: 0.012,
        reload_tactical: 0.5,
        reload_empty: 0.5,
        reload_per_shell: true,
        chamber: false,
        ads_time: 0.3,
        ads_fov: 62.0,
        sway: 0.005,
    },
    // .338 bolt-action rifle with a scope
    WeaponSpec {
        name: "Bolt Rifle",
        mag: 5,
        reserve: 25,
        interval: 1.35,
        auto: false,
        pellets: 1,
        muzzle_velocity: 850.0,
        drag: 0.45,
        damage: 105.0,
        falloff_start: 300.0,
        falloff_end: 600.0,
        falloff_min: 0.85,
        head_mult: 2.5,
        limb_mult: 0.7,
        hip_spread: 0.09,
        ads_spread: 0.0006,
        move_spread: 0.02,
        bloom: 0.0,
        kick_up: 0.07,
        kick_side: 0.01,
        reload_tactical: 3.0,
        reload_empty: 3.6,
        reload_per_shell: false,
        chamber: false,
        ads_time: 0.4,
        ads_fov: 12.0,
        sway: 0.009,
    },
];

pub fn spec(w: u8) -> &'static WeaponSpec {
    &WEAPONS[(w as usize).min(COUNT - 1)]
}

/// Damage multiplier for distance travelled.
pub fn falloff(s: &WeaponSpec, dist: f32) -> f32 {
    if dist <= s.falloff_start {
        1.0
    } else if dist >= s.falloff_end {
        s.falloff_min
    } else {
        let k = (dist - s.falloff_start) / (s.falloff_end - s.falloff_start);
        1.0 + (s.falloff_min - 1.0) * k
    }
}
