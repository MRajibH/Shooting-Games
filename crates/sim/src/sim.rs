//! Player movement and collision. Uses `libm` (pure Rust maths) instead of platform
//! maths so the native server and the WebAssembly client produce bit-identical results.

use crate::constants::{body::*, movement::*, slap, stamina as st};
use crate::map::{self, MapBox, HALF};
use crate::protocol::{button, InputFrame, PlayerState};

const EPS: f32 = 1e-4;

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct StepResult {
    /// A slap was thrown this tick (hit resolution is the server's job).
    pub slap: bool,
}

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

pub fn height(p: &PlayerState) -> f32 {
    if p.crouched {
        CROUCH_HEIGHT
    } else {
        STAND_HEIGHT
    }
}

fn over_xz(b: &MapBox, x: f32, z: f32) -> bool {
    x + RADIUS > b.min[0] + EPS
        && x - RADIUS < b.max[0] - EPS
        && z + RADIUS > b.min[2] + EPS
        && z - RADIUS < b.max[2] - EPS
}

fn overlaps(b: &MapBox, x: f32, y: f32, z: f32, h: f32) -> bool {
    over_xz(b, x, z) && y + h > b.min[1] + EPS && y < b.max[1] - EPS
}

/// First map box a body at (x, y, z) of height `h` would intersect.
pub fn blocked(x: f32, y: f32, z: f32, h: f32) -> Option<&'static MapBox> {
    map::boxes().iter().find(|b| overlaps(b, x, y, z, h))
}

/// Highest floor (ground or box top) under (x, z) whose top lies in [lo, hi].
fn support(x: f32, z: f32, lo: f32, hi: f32) -> Option<f32> {
    let mut best = if lo <= 0.0 && hi >= 0.0 {
        Some(0.0)
    } else {
        None
    };
    for b in map::boxes() {
        let t = b.max[1];
        if t >= lo && t <= hi + EPS && over_xz(b, x, z) && best.is_none_or(|v| t > v) {
            best = Some(t);
        }
    }
    best
}

pub fn spawn_state(id: u32, spawn: usize) -> PlayerState {
    let (x, z, yaw) = map::spawn(spawn);
    PlayerState {
        id,
        x,
        z,
        yaw,
        grounded: true,
        stamina: 1.0,
        ..Default::default()
    }
}

/// Height of a vaultable obstacle just ahead, if any.
fn vault_height(p: &PlayerState, fx: f32, fz: f32) -> Option<f32> {
    let (px, pz) = (p.x + fx * 0.55, p.z + fz * 0.55);
    let mut best: Option<f32> = None;
    for b in map::boxes() {
        let h = b.max[1] - p.y;
        if h > VAULT_MIN
            && h <= VAULT_MAX
            && b.min[1] <= p.y + VAULT_MAX
            && over_xz(b, px, pz)
            && blocked(px, b.max[1], pz, CROUCH_HEIGHT).is_none()
        {
            best = Some(best.map_or(h, |v: f32| v.max(h)));
        }
    }
    best
}

fn move_axis(p: &mut PlayerState, axis: usize, d: f32, h: f32) {
    if d == 0.0 {
        return;
    }
    let (mut nx, mut nz) = (p.x, p.z);
    if axis == 0 {
        nx += d
    } else {
        nz += d
    }
    let Some(b) = blocked(nx, p.y, nz, h) else {
        p.x = nx;
        p.z = nz;
        return;
    };
    // walk up steps and kerbs
    let rise = b.max[1] - p.y;
    if p.grounded && rise > 0.0 && rise <= STEP_HEIGHT && blocked(nx, b.max[1], nz, h).is_none() {
        p.x = nx;
        p.z = nz;
        p.y = b.max[1];
        return;
    }
    // slide flush against the face
    let face = if d > 0.0 {
        b.min[axis] - RADIUS - EPS * 2.0
    } else {
        b.max[axis] + RADIUS + EPS * 2.0
    };
    if axis == 0 {
        nx = face
    } else {
        nz = face
    }
    if blocked(nx, p.y, nz, h).is_none() {
        p.x = nx;
        p.z = nz;
    }
    if axis == 0 {
        p.vx = 0.0
    } else {
        p.vz = 0.0
    }
}

/// Advance one player by one tick. Pure apart from mutating `p`: the server runs it
/// for authority, the browser runs the same code (as WASM) for prediction.
pub fn step_player(p: &mut PlayerState, input: &InputFrame, dt: f32) -> StepResult {
    let mut res = StepResult::default();
    p.slap_cd = (p.slap_cd - dt).max(0.0);
    p.stun = (p.stun - dt).max(0.0);
    p.ko = (p.ko - dt).max(0.0);
    p.daze = (p.daze - slap::DAZE_DECAY * dt).max(0.0);
    let disabled = p.stun > 0.0 || p.ko > 0.0;
    let buttons = if disabled { 0 } else { input.buttons };

    let (mut mx, mut mz) = if disabled {
        (0.0, 0.0)
    } else {
        (
            clamp(input.move_x, -1.0, 1.0),
            clamp(input.move_z, -1.0, 1.0),
        )
    };
    let len = libm::hypotf(mx, mz);
    if len > 1.0 {
        mx /= len;
        mz /= len;
    }

    if p.ko <= 0.0 && input.yaw.is_finite() {
        p.yaw = input.yaw;
    }
    p.pitch = clamp(input.pitch, -MAX_PITCH, MAX_PITCH);

    // crouch; standing up needs headroom
    if buttons & button::CROUCH != 0 {
        p.crouched = true;
    } else if p.crouched && blocked(p.x, p.y, p.z, STAND_HEIGHT).is_none() {
        p.crouched = false;
    }

    // stamina: sprint drains it, running dry locks sprint until it recovers
    if p.exhausted && p.stamina >= st::RECOVER {
        p.exhausted = false;
    }
    let sprint = buttons & button::SPRINT != 0 && mz > 0.5 && !p.crouched && !p.exhausted;
    if sprint {
        p.stamina -= st::SPRINT_DRAIN * dt;
        if p.stamina <= 0.0 {
            p.stamina = 0.0;
            p.exhausted = true;
        }
    } else {
        p.stamina = (p.stamina + st::REGEN * dt).min(1.0);
    }

    let lean_target = if sprint {
        0.0
    } else {
        (if buttons & button::LEAN_RIGHT != 0 {
            1.0
        } else {
            0.0
        }) - (if buttons & button::LEAN_LEFT != 0 {
            1.0
        } else {
            0.0
        })
    };
    p.lean = approach(p.lean, lean_target, LEAN_SPEED * dt);

    let speed = if p.crouched {
        CROUCH_SPEED
    } else if sprint {
        SPRINT_SPEED
    } else {
        WALK_SPEED
    };
    // forward is -Z at yaw 0 (three.js convention)
    let (sin, cos) = (libm::sinf(p.yaw), libm::cosf(p.yaw));
    let (fx, fz) = (-sin, -cos);
    let tx = (mx * cos - mz * sin) * speed;
    let tz = (-mx * sin - mz * cos) * speed;
    let accel = if disabled {
        GROUND_ACCEL * 0.25
    } else if p.grounded {
        GROUND_ACCEL
    } else {
        AIR_ACCEL
    } * dt;
    p.vx = approach(p.vx, tx, accel);
    p.vz = approach(p.vz, tz, accel);

    // jump, or vault when facing a low obstacle
    if p.grounded && !p.crouched && buttons & button::JUMP != 0 && p.stamina >= st::JUMP_COST {
        p.stamina -= st::JUMP_COST;
        if let Some(h) = vault_height(p, fx, fz) {
            p.vy = libm::sqrtf(2.0 * GRAVITY * (h + 0.15));
            let fwd = (p.vx * fx + p.vz * fz).max(2.5);
            p.vx = fx * fwd;
            p.vz = fz * fwd;
        } else {
            p.vy = JUMP_SPEED;
        }
        p.grounded = false;
    }

    if buttons & button::FIRE != 0 && p.slap_cd <= 0.0 {
        p.slap_cd = slap::COOLDOWN;
        res.slap = true;
    }

    let h = height(p);
    move_axis(p, 0, p.vx * dt, h);
    move_axis(p, 2, p.vz * dt, h);
    let lim = HALF - RADIUS;
    p.x = clamp(p.x, -lim, lim);
    p.z = clamp(p.z, -lim, lim);

    // vertical: land on the highest floor crossed, bump heads on ceilings
    let was_grounded = p.grounded;
    p.vy -= GRAVITY * dt;
    let ny = p.y + p.vy * dt;
    p.grounded = false;
    if p.vy <= 0.0 {
        if let Some(top) = support(p.x, p.z, ny, p.y) {
            p.y = top;
            p.vy = 0.0;
            p.grounded = true;
        } else if let (true, Some(top)) = (was_grounded, support(p.x, p.z, p.y - STEP_HEIGHT, p.y))
        {
            // stick to stairs going down instead of hopping off each step
            p.y = top;
            p.vy = 0.0;
            p.grounded = true;
        } else {
            p.y = ny;
        }
    } else if let Some(b) = blocked(p.x, ny, p.z, h) {
        p.y = (b.min[1] - h).max(p.y);
        p.vy = 0.0;
    } else {
        p.y = ny;
    }
    res
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

    fn at(x: f32, z: f32, yaw: f32) -> PlayerState {
        PlayerState {
            id: 1,
            x,
            z,
            yaw,
            grounded: true,
            stamina: 1.0,
            ..Default::default()
        }
    }

    fn run(p: &mut PlayerState, ticks: usize, f: InputFrame) {
        for _ in 0..ticks {
            step_player(p, &f, TICK_DT);
        }
    }

    #[test]
    fn deterministic() {
        let (mut a, mut b) = (spawn_state(1, 0), spawn_state(1, 0));
        for i in 0..600 {
            let f = frame(|f| {
                f.move_z = 1.0;
                f.move_x = 0.5;
                f.yaw = i as f32 * 0.03;
                f.buttons = if i % 50 == 20 {
                    button::JUMP
                } else {
                    button::SPRINT | button::LEAN_LEFT
                };
            });
            step_player(&mut a, &f, TICK_DT);
            step_player(&mut b, &f, TICK_DT);
        }
        assert_eq!(a, b);
    }

    #[test]
    fn spawns_are_clear() {
        for i in 0..map::spawn_count() {
            let p = spawn_state(1, i);
            assert!(blocked(p.x, p.y, p.z, STAND_HEIGHT).is_none(), "spawn {i}");
        }
    }

    #[test]
    fn reaches_walk_speed() {
        let mut p = at(0.0, 10.0, 0.0);
        run(&mut p, 60, frame(|f| f.move_z = 1.0));
        assert!((libm::hypotf(p.vx, p.vz) - WALK_SPEED).abs() < 1e-3);
    }

    #[test]
    fn jump_is_human_height() {
        let mut p = at(0.0, 10.0, 0.0);
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
    fn walls_block() {
        // container at x 8.97..15.03, z -3.22..-0.78; walk +x into it from x = 6
        let mut p = at(6.0, -2.0, -std::f32::consts::FRAC_PI_2);
        run(
            &mut p,
            180,
            frame(|f| {
                f.move_z = 1.0;
                f.yaw = -std::f32::consts::FRAC_PI_2
            }),
        );
        assert!(p.x <= 8.97 - RADIUS + 1e-3, "x {}", p.x);
    }

    #[test]
    fn climbs_stairs_to_walkway() {
        // stairs rise toward -x, from x = 10.5 down to the walkway at x = 6 (top 3.0 m)
        let mut p = at(11.0, -14.0, std::f32::consts::FRAC_PI_2);
        run(
            &mut p,
            150,
            frame(|f| {
                f.move_z = 1.0;
                f.yaw = std::f32::consts::FRAC_PI_2
            }),
        );
        assert!((p.y - 3.0).abs() < 1e-3 && p.x < 6.0, "y {} x {}", p.y, p.x);
        // and walks back down without leaving the ground
        p.yaw = -std::f32::consts::FRAC_PI_2;
        let mut airborne = 0;
        for _ in 0..240 {
            step_player(
                &mut p,
                &frame(|f| {
                    f.move_z = 1.0;
                    f.yaw = -std::f32::consts::FRAC_PI_2
                }),
                TICK_DT,
            );
            if !p.grounded {
                airborne += 1;
            }
        }
        assert!(p.y == 0.0 && airborne == 0, "y {} airborne {airborne}", p.y);
    }

    #[test]
    fn vaults_low_wall_but_not_container() {
        // low wall at z -8.15..-7.85, 0.9 m; approach from z = -6 facing -z (yaw 0)
        let mut p = at(-14.0, -6.0, 0.0);
        for i in 0..120 {
            let b = if i == 25 { button::JUMP } else { 0 };
            step_player(
                &mut p,
                &frame(|f| {
                    f.move_z = 1.0;
                    f.buttons = b
                }),
                TICK_DT,
            );
        }
        assert!(p.z < -8.5, "did not vault, z {}", p.z);

        let mut q = at(6.0, -2.0, -std::f32::consts::FRAC_PI_2);
        for i in 0..120 {
            let b = if i == 50 { button::JUMP } else { 0 };
            step_player(
                &mut q,
                &frame(|f| {
                    f.move_z = 1.0;
                    f.yaw = -std::f32::consts::FRAC_PI_2;
                    f.buttons = b
                }),
                TICK_DT,
            );
        }
        assert!(q.x < 9.0, "went over a container, x {}", q.x);
    }

    #[test]
    fn sprint_drains_and_locks() {
        let mut p = at(0.0, 30.0, std::f32::consts::FRAC_PI_2);
        let f = frame(|f| {
            f.move_z = 1.0;
            f.yaw = std::f32::consts::FRAC_PI_2;
            f.buttons = button::SPRINT
        });
        // long straight run along -x; walls stop us but stamina still drains
        run(&mut p, 60 * 6, f);
        assert!(p.exhausted);
        run(&mut p, 30, f);
        assert!(libm::hypotf(p.vx, p.vz) <= WALK_SPEED + 1e-3);
    }

    #[test]
    fn no_sprint_backwards() {
        let mut p = at(0.0, 10.0, 0.0);
        run(
            &mut p,
            60,
            frame(|f| {
                f.move_z = -1.0;
                f.buttons = button::SPRINT
            }),
        );
        assert!((libm::hypotf(p.vx, p.vz) - WALK_SPEED).abs() < 1e-3);
    }

    #[test]
    fn cannot_stand_under_walkway_edge() {
        // under the walkway (bottom at 2.8 m) there is room to stand; under a stacked crate gap there is not.
        let mut p = at(-14.0, -10.0, 0.0);
        p.crouched = true;
        step_player(&mut p, &frame(|_| {}), TICK_DT);
        assert!(!p.crouched);
    }

    #[test]
    fn stun_blocks_control() {
        let mut p = at(0.0, 10.0, 0.0);
        p.stun = 0.5;
        run(
            &mut p,
            10,
            frame(|f| {
                f.move_z = 1.0;
                f.buttons = button::FIRE
            }),
        );
        assert!(p.vz.abs() < 1e-3 && p.slap_cd == 0.0);
    }

    #[test]
    fn protocol_json_shape() {
        use crate::protocol::{ClientMessage, Mode, ServerMessage};
        let m: ClientMessage =
            serde_json::from_str(r#"{"t":"join","name":"a","mode":"slap"}"#).unwrap();
        assert!(matches!(
            m,
            ClientMessage::Join {
                mode: Some(Mode::Slap),
                ..
            }
        ));
        let w = serde_json::to_string(&ServerMessage::Welcome {
            id: 1,
            room: "R1".into(),
            tick_rate: 60,
            mode: Mode::Slap,
        })
        .unwrap();
        assert_eq!(
            w,
            r#"{"t":"welcome","id":1,"room":"R1","tickRate":60,"mode":"slap"}"#
        );
    }
}
