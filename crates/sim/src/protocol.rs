//! Wire protocol (JSON for now). The TypeScript mirror is client/src/protocol.ts.

use serde::{Deserialize, Serialize};

pub mod button {
    pub const JUMP: u32 = 1 << 0;
    pub const SPRINT: u32 = 1 << 1;
    pub const FIRE: u32 = 1 << 2;
    pub const AIM: u32 = 1 << 3;
    pub const RELOAD: u32 = 1 << 4;
    pub const CROUCH: u32 = 1 << 5;
    pub const LEAN_LEFT: u32 = 1 << 6;
    pub const LEAN_RIGHT: u32 = 1 << 7;
    /// Toggle semi / full auto on automatic weapons.
    pub const FIRE_MODE: u32 = 1 << 8;
}

/// One tick of player intent. Clients never send positions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputFrame {
    pub seq: u32,
    pub move_x: f32,
    pub move_z: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub buttons: u32,
    /// Weapon slot the player wants in hand (0..4).
    #[serde(default)]
    pub weapon: u8,
    /// Server tick of the world the client was looking at when this frame was made
    /// (other players are drawn in the past); used to rewind hitboxes.
    #[serde(default)]
    pub view: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PlayerState {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub vx: f32,
    pub vy: f32,
    pub vz: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub grounded: bool,
    pub crouched: bool,
    /// 0..1
    pub stamina: f32,
    pub exhausted: bool,
    /// -1 (left) .. 1 (right)
    pub lean: f32,
    /// Seconds left staggered (no control).
    pub stun: f32,
    /// Seconds left knocked out.
    pub ko: f32,
    /// Seconds until the next slap.
    pub slap_cd: f32,
    /// 0..100; a knockout at 100.
    pub daze: f32,

    // ---- combat ----
    /// False in Slap mode (empty hands).
    pub armed: bool,
    pub hp: f32,
    /// Seconds until respawn while dead.
    pub dead: f32,
    pub regen_cd: f32,
    pub weapon: u8,
    pub mags: [u16; 4],
    pub reserves: [u16; 4],
    pub fire_cd: f32,
    /// Reload time left, and the full length of the current reload (for animation).
    pub reload: f32,
    pub reload_len: f32,
    /// Weapon swap time left.
    pub swap: f32,
    /// Weapon lowered after sprinting, time left until it can fire.
    pub lowered: f32,
    /// 0 (hip) .. 1 (fully aimed).
    pub ads: f32,
    pub bloom: f32,
    /// Shots fired (seeds the deterministic spread).
    pub shots: u32,
    /// Semi-auto selected on an automatic weapon.
    pub semi: bool,
    /// Trigger already used for this pull (semi-auto).
    pub trigger_spent: bool,
    /// Buttons held last tick (edge detection).
    pub held: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Combat,
    Slap,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "camelCase")]
pub enum GameEvent {
    /// `to` is None for a miss.
    Slap {
        from: u32,
        to: Option<u32>,
    },
    Ko {
        from: u32,
        to: u32,
    },
    Respawn {
        id: u32,
    },
    /// Weapon fired: origin and aim of the first round (for tracers, flash and sound).
    Fire {
        id: u32,
        w: u8,
        o: [f32; 3],
        d: [f32; 3],
    },
    /// Bullet struck the map.
    Impact {
        p: [f32; 3],
        n: [f32; 3],
        mat: u8,
    },
    /// Bullet struck a player.
    Hit {
        from: u32,
        to: u32,
        dmg: f32,
        zone: crate::ballistics::Zone,
        p: [f32; 3],
    },
    Kill {
        from: u32,
        to: u32,
        w: u8,
        head: bool,
        dist: f32,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RosterEntry {
    pub id: u32,
    pub name: String,
    pub slaps: u32,
    pub kos: u32,
    pub kills: u32,
    pub deaths: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "camelCase")]
pub enum ClientMessage {
    Join {
        name: String,
        room: Option<String>,
        #[serde(default)]
        mode: Option<Mode>,
    },
    Input {
        frames: Vec<InputFrame>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "camelCase")]
pub enum ServerMessage {
    #[serde(rename_all = "camelCase")]
    Welcome {
        id: u32,
        room: String,
        tick_rate: u32,
        mode: Mode,
    },
    Snapshot {
        tick: u64,
        ack: u32,
        players: Vec<PlayerState>,
        events: Vec<GameEvent>,
    },
    Roster {
        players: Vec<RosterEntry>,
    },
    Left {
        id: u32,
    },
}
