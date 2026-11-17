//! Browser bindings (built with `--features wasm`). The client predicts its own
//! player with exactly the code the server uses, and builds the map from the same data.

use wasm_bindgen::prelude::*;

use crate::constants::{self, slap};
use crate::map;
use crate::protocol::{InputFrame, PlayerState};
use crate::sim::{spawn_state, step_player};

#[wasm_bindgen]
pub struct Predictor {
    s: PlayerState,
}

#[wasm_bindgen]
impl Predictor {
    #[wasm_bindgen(constructor)]
    pub fn new(id: u32) -> Predictor {
        Predictor {
            s: spawn_state(id, 0),
        }
    }

    /// Overwrite with the server's authoritative state (JSON PlayerState).
    #[wasm_bindgen(js_name = setJson)]
    pub fn set_json(&mut self, json: &str) {
        if let Ok(s) = serde_json::from_str::<PlayerState>(json) {
            self.s = s;
        }
    }

    /// Returns true when a slap was thrown this tick.
    pub fn step(
        &mut self,
        seq: u32,
        move_x: f32,
        move_z: f32,
        yaw: f32,
        pitch: f32,
        buttons: u32,
    ) -> bool {
        let f = InputFrame {
            seq,
            move_x,
            move_z,
            yaw,
            pitch,
            buttons,
        };
        step_player(&mut self.s, &f, constants::TICK_DT).slap
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
