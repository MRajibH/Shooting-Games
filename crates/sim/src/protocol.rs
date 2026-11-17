//! Wire protocol (JSON for now). The TypeScript mirror is client/src/protocol.ts.

use serde::{Deserialize, Serialize};

pub mod button {
    pub const JUMP: u32 = 1 << 0;
    pub const SPRINT: u32 = 1 << 1;
    pub const FIRE: u32 = 1 << 2;
    pub const AIM: u32 = 1 << 3;
    pub const RELOAD: u32 = 1 << 4;
    pub const CROUCH: u32 = 1 << 5;
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
#[serde(rename_all = "camelCase")]
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
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "camelCase")]
pub enum ClientMessage {
    Join { name: String, room: Option<String> },
    Input { frames: Vec<InputFrame> },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "camelCase")]
pub enum ServerMessage {
    #[serde(rename_all = "camelCase")]
    Welcome {
        id: u32,
        room: String,
        tick_rate: u32,
    },
    Snapshot {
        tick: u64,
        ack: u32,
        players: Vec<PlayerState>,
    },
    Left {
        id: u32,
    },
}
