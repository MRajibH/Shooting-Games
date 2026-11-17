//! Player movement. Uses `libm` (pure Rust maths) instead of platform maths so the
//! native server and the WebAssembly client produce bit-identical results.

use crate::constants::movement::*;
use crate::protocol::{button, InputFrame, PlayerState};

fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    if v.is_nan() {
        0.0
    } else {
        v.max(lo).min(hi)
    }
}

fn approach(cur: f32, target: f32, max_delta: f32) -> f32 {
    let d = target - cur;
    if d.abs() <= max_delta {
        target
    } else {
        cur + max_delta * d.signum()
    }
}

pub fn spawn_state(id: u32) -> PlayerState {
    PlayerState {
        id,
        grounded: true,
        ..Default::default()
    }
}

/// Advance one player by one tick. Pure apart from mutating `p`: the server runs it
/// for authority, the browser runs the same code (as WASM) for prediction.
/// Flat ground for now; map collision arrives in milestone 2.
pub fn step_player(p: &mut PlayerState, input: &InputFrame, dt: f32) {
    let mx = clamp(input.move_x, -1.0, 1.0);
    let mz = clamp(input.move_z, -1.0, 1.0);
    let len = libm::hypotf(mx, mz);
    let (mx, mz) = if len > 1.0 {
        (mx / len, mz / len)
    } else {
        (mx, mz)
    };

    p.yaw = if input.yaw.is_finite() {
        input.yaw
    } else {
        p.yaw
    };
    p.pitch = clamp(input.pitch, -MAX_PITCH, MAX_PITCH);
    p.crouched = p.grounded && input.buttons & button::CROUCH != 0;

    // Sprinting only works moving forward, as in reality.
    let sprint = input.buttons & button::SPRINT != 0 && mz > 0.5 && !p.crouched;
    let speed = if p.crouched {
        CROUCH_SPEED
    } else if sprint {
        SPRINT_SPEED
    } else {
        WALK_SPEED
    };

    // forward is -Z at yaw 0 (three.js convention)
    let (sin, cos) = (libm::sinf(p.yaw), libm::cosf(p.yaw));
    let tx = (mx * cos - mz * sin) * speed;
    let tz = (-mx * sin - mz * cos) * speed;
    let accel = if p.grounded { GROUND_ACCEL } else { AIR_ACCEL } * dt;
    p.vx = approach(p.vx, tx, accel);
    p.vz = approach(p.vz, tz, accel);

    if p.grounded && !p.crouched && input.buttons & button::JUMP != 0 {
        p.vy = JUMP_SPEED;
        p.grounded = false;
    }
    p.vy -= GRAVITY * dt;

    p.x += p.vx * dt;
    p.y += p.vy * dt;
    p.z += p.vz * dt;
    if p.y <= 0.0 {
        p.y = 0.0;
        p.vy = 0.0;
        p.grounded = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::TICK_DT;

    fn frame(o: impl FnOnce(&mut InputFrame)) -> InputFrame {
        let mut f = InputFrame::default();
        o(&mut f);
        f
    }

    #[test]
    fn deterministic() {
        let (mut a, mut b) = (spawn_state(1), spawn_state(1));
        for i in 0..240 {
            let f = frame(|f| {
                f.move_z = 1.0;
                f.move_x = 0.5;
                f.yaw = i as f32 * 0.03;
                f.buttons = if i == 20 {
                    button::JUMP
                } else {
                    button::SPRINT
                };
            });
            step_player(&mut a, &f, TICK_DT);
            step_player(&mut b, &f, TICK_DT);
        }
        assert_eq!(a, b);
    }

    #[test]
    fn forward_is_negative_z_and_reaches_walk_speed() {
        let mut p = spawn_state(1);
        for _ in 0..60 {
            step_player(&mut p, &frame(|f| f.move_z = 1.0), TICK_DT);
        }
        assert!(p.z < 0.0 && p.x.abs() < 1e-4);
        assert!((libm::hypotf(p.vx, p.vz) - WALK_SPEED).abs() < 1e-3);
    }

    #[test]
    fn jump_is_human_height() {
        let mut p = spawn_state(1);
        step_player(&mut p, &frame(|f| f.buttons = button::JUMP), TICK_DT);
        let mut peak: f32 = 0.0;
        for _ in 0..120 {
            step_player(&mut p, &frame(|_| {}), TICK_DT);
            peak = peak.max(p.y);
        }
        assert!(p.grounded && p.y == 0.0);
        assert!((0.40..0.50).contains(&peak), "peak {peak}");
    }

    #[test]
    fn no_sprint_backwards() {
        let mut p = spawn_state(1);
        for _ in 0..60 {
            step_player(
                &mut p,
                &frame(|f| {
                    f.move_z = -1.0;
                    f.buttons = button::SPRINT;
                }),
                TICK_DT,
            );
        }
        assert!((libm::hypotf(p.vx, p.vz) - WALK_SPEED).abs() < 1e-3);
    }

    #[test]
    fn protocol_json_shape() {
        use crate::protocol::{ClientMessage, ServerMessage};
        let m: ClientMessage = serde_json::from_str(
            r#"{"t":"input","frames":[{"seq":1,"moveX":0,"moveZ":1,"yaw":0,"pitch":0,"buttons":0}]}"#,
        ).unwrap();
        assert!(matches!(m, ClientMessage::Input { ref frames } if frames.len() == 1));
        let w = serde_json::to_string(&ServerMessage::Welcome {
            id: 1,
            room: "R1".into(),
            tick_rate: 60,
        })
        .unwrap();
        assert_eq!(w, r#"{"t":"welcome","id":1,"room":"R1","tickRate":60}"#);
    }
}
