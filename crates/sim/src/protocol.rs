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
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RosterEntry {
    pub id: u32,
    pub name: String,
    pub slaps: u32,
    pub kos: u32,
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
