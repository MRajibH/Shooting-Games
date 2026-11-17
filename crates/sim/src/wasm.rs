//! Browser bindings (built with `--features wasm`). The client predicts its own
//! player with exactly the code the server uses.

use wasm_bindgen::prelude::*;

use crate::constants;
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
        Predictor { s: spawn_state(id) }
    }

    /// Overwrite with the server's authoritative state (reconciliation).
    #[allow(clippy::too_many_arguments)]
    pub fn set(
        &mut self,
        x: f32,
        y: f32,
        z: f32,
        vx: f32,
        vy: f32,
        vz: f32,
        yaw: f32,
        pitch: f32,
        grounded: bool,
        crouched: bool,
    ) {
        let id = self.s.id;
        self.s = PlayerState {
            id,
            x,
            y,
            z,
            vx,
            vy,
            vz,
            yaw,
            pitch,
            grounded,
            crouched,
        };
    }

    pub fn step(&mut self, seq: u32, move_x: f32, move_z: f32, yaw: f32, pitch: f32, buttons: u32) {
        let f = InputFrame {
            seq,
            move_x,
            move_z,
            yaw,
            pitch,
            buttons,
        };
        step_player(&mut self.s, &f, constants::TICK_DT);
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
    pub fn crouched(&self) -> bool {
        self.s.crouched
    }
    #[wasm_bindgen(getter)]
    pub fn speed(&self) -> f32 {
        libm::hypotf(self.s.vx, self.s.vz)
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
