//! Browser bindings (built with `--features wasm`). The client predicts its own
//! player with exactly the code the server uses, and builds the map from the same data.

use wasm_bindgen::prelude::*;

use crate::constants::{self, combat, slap};
use crate::map;
use crate::protocol::{InputFrame, PlayerState};
use crate::sim::{self, spawn_state, step_player, StepResult};
use crate::weapons;

#[wasm_bindgen]
pub struct Predictor {
    s: PlayerState,
    last: StepResult,
}

#[wasm_bindgen]
impl Predictor {
    #[wasm_bindgen(constructor)]
    pub fn new(id: u32, armed: bool) -> Predictor {
        Predictor {
            s: spawn_state(id, 0, armed),
            last: StepResult::default(),
        }
    }

    /// Overwrite with the server's authoritative state (JSON PlayerState).
    #[wasm_bindgen(js_name = setJson)]
    pub fn set_json(&mut self, json: &str) {
        if let Ok(s) = serde_json::from_str::<PlayerState>(json) {
            self.s = s;
        }
    }

    /// Returns flags: 1 slap thrown, 2 shot fired, 4 dry trigger on an empty magazine.
    #[allow(clippy::too_many_arguments)]
    pub fn step(
        &mut self,
        seq: u32,
        move_x: f32,
        move_z: f32,
        yaw: f32,
        pitch: f32,
        buttons: u32,
        weapon: u8,
    ) -> u32 {
        let f = InputFrame {
            seq,
            move_x,
            move_z,
            yaw,
            pitch,
            buttons,
            weapon,
            view: 0.0,
        };
        self.last = step_player(&mut self.s, &f, constants::TICK_DT);
        u32::from(self.last.slap)
            | (u32::from(!self.last.shots.is_empty()) << 1)
            | (u32::from(self.last.dry) << 2)
    }

    /// Rounds from the last step: 6 floats each (origin xyz, direction xyz).
    pub fn shots(&self) -> Vec<f32> {
        self.last
            .shots
            .iter()
            .flat_map(|s| {
                [
                    s.origin[0],
                    s.origin[1],
                    s.origin[2],
                    s.dir[0],
                    s.dir[1],
                    s.dir[2],
                ]
            })
            .collect()
    }
    #[wasm_bindgen(getter, js_name = kickPitch)]
    pub fn kick_pitch(&self) -> f32 {
        self.last.kick.0
    }
    #[wasm_bindgen(getter, js_name = kickYaw)]
    pub fn kick_yaw(&self) -> f32 {
        self.last.kick.1
    }
    /// Current shot cone half-angle (radians), for the crosshair.
    #[wasm_bindgen(getter)]
    pub fn spread(&self) -> f32 {
        sim::spread(&self.s)
    }
    #[wasm_bindgen(getter)]
    pub fn team(&self) -> u8 {
        self.s.team
    }
    #[wasm_bindgen(getter)]
    pub fn armed(&self) -> bool {
        self.s.armed
    }
    #[wasm_bindgen(getter)]
    pub fn armor(&self) -> f32 {
        self.s.armor
    }
    #[wasm_bindgen(getter)]
    pub fn hp(&self) -> f32 {
        self.s.hp
    }
    #[wasm_bindgen(getter)]
    pub fn dead(&self) -> f32 {
        self.s.dead
    }
    #[wasm_bindgen(getter)]
    pub fn weapon(&self) -> u8 {
        self.s.weapon
    }
    #[wasm_bindgen(getter)]
    pub fn mag(&self) -> u16 {
        self.s.mags[self.s.weapon as usize]
    }
    #[wasm_bindgen(getter)]
    pub fn reserve(&self) -> u16 {
        self.s.reserves[self.s.weapon as usize]
    }
    #[wasm_bindgen(getter)]
    pub fn reload(&self) -> f32 {
        self.s.reload
    }
    #[wasm_bindgen(getter, js_name = reloadLen)]
    pub fn reload_len(&self) -> f32 {
        self.s.reload_len
    }
    #[wasm_bindgen(getter)]
    pub fn swap(&self) -> f32 {
        self.s.swap
    }
    #[wasm_bindgen(getter)]
    pub fn ads(&self) -> f32 {
        self.s.ads
    }
    #[wasm_bindgen(getter)]
    pub fn semi(&self) -> bool {
        self.s.semi
    }
    #[wasm_bindgen(getter)]
    pub fn lowered(&self) -> f32 {
        self.s.lowered
    }
    #[wasm_bindgen(getter)]
    pub fn pitch(&self) -> f32 {
        self.s.pitch
    }

    #[wasm_bindgen(getter)]
    pub fn x(&self) -> f32 {
        self.s.x
    }
    #[wasm_bindgen(getter)]
    pub fn y(&self) -> f32 {
        self.s.y
    }
    #[wasm_bindgen(getter)]
    pub fn z(&self) -> f32 {
        self.s.z
    }
    #[wasm_bindgen(getter)]
    pub fn vx(&self) -> f32 {
        self.s.vx
    }
    #[wasm_bindgen(getter)]
    pub fn vz(&self) -> f32 {
        self.s.vz
    }
    #[wasm_bindgen(getter)]
    pub fn yaw(&self) -> f32 {
        self.s.yaw
    }
    #[wasm_bindgen(getter)]
    pub fn grounded(&self) -> bool {
        self.s.grounded
    }
    #[wasm_bindgen(getter)]
    pub fn crouched(&self) -> bool {
        self.s.crouched
    }
    #[wasm_bindgen(getter)]
    pub fn stamina(&self) -> f32 {
        self.s.stamina
    }
    #[wasm_bindgen(getter)]
    pub fn exhausted(&self) -> bool {
        self.s.exhausted
    }
    #[wasm_bindgen(getter)]
    pub fn lean(&self) -> f32 {
        self.s.lean
    }
    #[wasm_bindgen(getter)]
    pub fn stun(&self) -> f32 {
        self.s.stun
    }
    #[wasm_bindgen(getter)]
    pub fn ko(&self) -> f32 {
        self.s.ko
    }
    #[wasm_bindgen(getter)]
    pub fn daze(&self) -> f32 {
        self.s.daze
    }
}

/// The weapon table as JSON (array of WeaponSpec, camelCase).
#[wasm_bindgen(js_name = weaponsJson)]
pub fn weapons_json() -> String {
    serde_json::to_string(&weapons::WEAPONS).unwrap_or_default()
}
#[wasm_bindgen(js_name = shoulderOffset)]
pub fn shoulder_offset() -> f32 {
    combat::SHOULDER
}
#[wasm_bindgen(js_name = leanShift)]
pub fn lean_shift() -> f32 {
    combat::LEAN_SHIFT
}
#[wasm_bindgen(js_name = respawnTime)]
pub fn respawn_time() -> f32 {
    combat::RESPAWN_TIME
}
#[wasm_bindgen(js_name = gravity)]
pub fn gravity() -> f32 {
    constants::movement::GRAVITY
}

#[wasm_bindgen(js_name = tickDt)]
pub fn tick_dt() -> f32 {
    constants::TICK_DT
}
#[wasm_bindgen(js_name = interpDelayMs)]
pub fn interp_delay_ms() -> f64 {
    constants::INTERP_DELAY_MS
}
#[wasm_bindgen(js_name = eyeHeight)]
pub fn eye_height(crouched: bool) -> f32 {
    if crouched {
        constants::movement::CROUCH_EYE_HEIGHT
    } else {
        constants::movement::EYE_HEIGHT
    }
}
#[wasm_bindgen(js_name = slapCooldown)]
pub fn slap_cooldown() -> f32 {
    slap::COOLDOWN
}
#[wasm_bindgen(js_name = koTime)]
pub fn ko_time() -> f32 {
    slap::KO_TIME
}

/// Map boxes, 7 floats each: minX minY minZ maxX maxY maxZ material.
#[wasm_bindgen(js_name = mapBoxes)]
pub fn map_boxes() -> Vec<f32> {
    map::boxes()
        .iter()
        .flat_map(|b| {
            [
                b.min[0],
                b.min[1],
                b.min[2],
                b.max[0],
                b.max[1],
                b.max[2],
                b.mat as f32,
            ]
        })
        .collect()
}

#[wasm_bindgen(js_name = mapHalf)]
pub fn map_half() -> f32 {
    map::HALF
}
