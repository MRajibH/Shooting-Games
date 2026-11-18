//! Player movement and collision. Uses `libm` (pure Rust maths) instead of platform
//! maths so the native server and the WebAssembly client produce bit-identical results.

use crate::ballistics::{raycast_map, V3};
use crate::constants::{body::*, combat, movement::*, slap, stamina as st};
use crate::map::{self, MapBox, HALF};
use crate::protocol::{button, InputFrame, PlayerState};
use crate::weapons::{self, spec};

const EPS: f32 = 1e-4;

/// A round leaving the gun.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shot {
    pub origin: V3,
    pub dir: V3,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct StepResult {
    /// A slap was thrown this tick (hit resolution is the server's job).
    pub slap: bool,
    /// The trigger released a shot (one entry per round or pellet).
    pub shots: Vec<Shot>,
    /// Trigger pulled on an empty magazine.
    pub dry: bool,
    /// View kick from recoil to add to the player's aim: (pitch up, yaw).
    pub kick: (f32, f32),
}

/// Deterministic hash -> [0, 1), so spread matches on server and client.
fn rand01(a: u32, b: u32, c: u32) -> f32 {
    let mut x = a
        .wrapping_mul(0x9E37_79B9)
        .wrapping_add(b.wrapping_mul(0x85EB_CA6B))
        .wrapping_add(c.wrapping_mul(0xC2B2_AE35))
        .wrapping_add(0x27D4_EB2F);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846C_A68B);
    x ^= x >> 16;
    (x >> 8) as f32 / 16_777_216.0
}

pub fn eye(p: &PlayerState) -> f32 {
    if p.crouched {
        CROUCH_EYE_HEIGHT
    } else {
        EYE_HEIGHT
    }
}

/// Where shots leave from: beside the head over the right shoulder, on the camera's
/// aim ray. Falls back to the eye if that point is inside a wall.
pub fn muzzle_origin(p: &PlayerState) -> V3 {
    let e = [p.x, p.y + eye(p), p.z];
    let off = combat::SHOULDER + p.lean * combat::LEAN_SHIFT;
    let r = [libm::cosf(p.yaw), 0.0, -libm::sinf(p.yaw)];
    let reach = off.abs();
    if reach < 1e-3 {
        return e;
    }
    let s = off.signum();
    let d = [r[0] * s, 0.0, r[2] * s];
    match raycast_map(e, d, reach + 0.05) {
        Some(_) => e,
        None => [e[0] + d[0] * reach, e[1], e[2] + d[2] * reach],
    }
}

pub fn aim_dir(yaw: f32, pitch: f32) -> V3 {
    let cp = libm::cosf(pitch);
    [
        -libm::sinf(yaw) * cp,
        libm::sinf(pitch),
        -libm::cosf(yaw) * cp,
    ]
}

/// Current cone half-angle of a shot.
pub fn spread(p: &PlayerState) -> f32 {
    let s = spec(p.weapon);
    let speed = libm::hypotf(p.vx, p.vz);
    let mut a =
        s.hip_spread + (s.ads_spread - s.hip_spread) * p.ads + speed * s.move_spread + p.bloom;
    if !p.grounded {
        a += 0.05;
    } else if p.crouched {
        a *= 0.75;
    }
    a
}

fn rounds_needed(p: &PlayerState) -> u16 {
    let s = spec(p.weapon);
    let w = p.weapon as usize;
    let cap = s.mag + u16::from(s.chamber && p.mags[w] > 0);
    cap.saturating_sub(p.mags[w])
}

fn start_reload(p: &mut PlayerState) -> bool {
    let w = p.weapon as usize;
    if p.reload > 0.0 || rounds_needed(p) == 0 || p.reserves[w] == 0 {
        return false;
    }
    let s = spec(p.weapon);
    p.reload_len = if s.reload_per_shell || p.mags[w] > 0 {
        s.reload_tactical
    } else {
        s.reload_empty
    };
    p.reload = p.reload_len;
    true
}

fn finish_reload(p: &mut PlayerState) {
    let s = spec(p.weapon);
    let w = p.weapon as usize;
    let take = if s.reload_per_shell {
        1
    } else {
        rounds_needed(p)
    }
    .min(p.reserves[w]);
    p.mags[w] += take;
    p.reserves[w] -= take;
}

/// Weapon handling for one tick: swap, fire mode, aim, reload, trigger.
fn step_weapon(
    p: &mut PlayerState,
    input: &InputFrame,
    buttons: u32,
    sprint: bool,
    dt: f32,
    res: &mut StepResult,
) {
    p.fire_cd = (p.fire_cd - dt).max(-dt);
    p.swap = (p.swap - dt).max(0.0);
    p.lowered = if sprint {
        weapons::SPRINT_OUT
    } else {
        (p.lowered - dt).max(0.0)
    };
    p.bloom = (p.bloom - weapons::BLOOM_DECAY * dt).max(0.0);
    let pressed = |b: u32| buttons & b != 0 && p.held & b == 0;

    if (input.weapon as usize) < weapons::COUNT && input.weapon != p.weapon {
        p.weapon = input.weapon;
        p.swap = weapons::SWAP_TIME;
        p.reload = 0.0;
        p.semi = false;
        p.trigger_spent = true;
    }
    let s = spec(p.weapon);
    if pressed(button::FIRE_MODE) && s.auto {
        p.semi = !p.semi;
    }

    let aiming = buttons & button::AIM != 0 && !sprint && p.swap <= 0.0 && p.reload <= 0.0;
    let rate = dt / s.ads_time;
    p.ads = if aiming {
        (p.ads + rate).min(1.0)
    } else {
        (p.ads - rate * 1.5).max(0.0)
    };

    let trigger = buttons & button::FIRE != 0;
    if !trigger {
        p.trigger_spent = false;
    }

    // reloading: shells go in one at a time and firing interrupts; magazines just finish
    if p.reload > 0.0 {
        if s.reload_per_shell && trigger && p.mags[p.weapon as usize] > 0 {
            p.reload = 0.0;
        } else {
            p.reload -= dt;
            if p.reload <= 0.0 {
                p.reload = 0.0;
                finish_reload(p);
                if s.reload_per_shell {
                    start_reload(p);
                }
            }
        }
    }
    if buttons & button::RELOAD != 0 && p.swap <= 0.0 {
        start_reload(p);
    }

    let wants = if s.auto && !p.semi {
        trigger
    } else {
        trigger && !p.trigger_spent
    };
    let ready = p.swap <= 0.0 && p.reload <= 0.0 && p.lowered <= 0.0 && p.fire_cd <= 0.0 && !sprint;
    if !(wants && ready) {
        return;
    }
    p.trigger_spent = true;
    let w = p.weapon as usize;
    if p.mags[w] == 0 {
        res.dry = true;
        start_reload(p);
        return;
    }
    p.mags[w] -= 1;
    p.fire_cd = p.fire_cd.max(-dt) + s.interval;
    let cone = spread(p);
    let origin = muzzle_origin(p);
    let (sy, cy) = (libm::sinf(p.yaw), libm::cosf(p.yaw));
    let (sp, cp) = (libm::sinf(p.pitch), libm::cosf(p.pitch));
    let fwd = [-sy * cp, sp, -cy * cp];
    let right = [cy, 0.0, -sy];
    let up = [sy * sp, cp, cy * sp];
    for k in 0..s.pellets as u32 {
        // uniform over the cone's disc
        let r = cone * libm::sqrtf(rand01(p.id, p.shots, k * 2));
        let a = std::f32::consts::TAU * rand01(p.id, p.shots, k * 2 + 1);
        let (ox, oy) = (r * libm::cosf(a), r * libm::sinf(a));
        let d = [
            fwd[0] + right[0] * ox + up[0] * oy,
            fwd[1] + right[1] * ox + up[1] * oy,
            fwd[2] + right[2] * ox + up[2] * oy,
        ];
        let l = crate::ballistics::len(d);
        res.shots.push(Shot {
            origin,
            dir: [d[0] / l, d[1] / l, d[2] / l],
        });
    }
    let mult = (1.0 - 0.3 * p.ads) * if p.crouched { 0.8 } else { 1.0 };
    let ku = s.kick_up * (0.85 + 0.3 * rand01(p.id, p.shots, 101)) * mult;
    let ks = s.kick_side * (rand01(p.id, p.shots, 102) * 2.0 - 0.7) * mult;
    res.kick = (ku, ks);
    p.bloom = (p.bloom + s.bloom).min(weapons::BLOOM_MAX);
    p.shots = p.shots.wrapping_add(1);
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

pub fn spawn_state(id: u32, spawn: usize, armed: bool) -> PlayerState {
    let (x, z, yaw) = map::spawn(spawn);
    PlayerState {
        id,
        x,
        z,
        yaw,
        grounded: true,
        stamina: 1.0,
        armed,
        hp: combat::MAX_HP,
        armor: if armed { combat::ARMOR } else { 0.0 },
        mags: weapons::WEAPONS.map(|w| w.mag),
        reserves: weapons::WEAPONS.map(|w| w.reserve),
        trigger_spent: true,
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
    p.dead = (p.dead - dt).max(0.0);
    let alive = p.dead <= 0.0;
    if alive && p.armed && p.hp < combat::MAX_HP {
        p.regen_cd = (p.regen_cd - dt).max(0.0);
        if p.regen_cd <= 0.0 {
            p.hp = (p.hp + combat::REGEN_RATE * dt).min(combat::MAX_HP);
        }
    }
    let disabled = p.stun > 0.0 || p.ko > 0.0 || !alive;
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

    if p.ko <= 0.0 && alive && input.yaw.is_finite() {
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
    // armed: aiming or pulling the trigger stops a sprint
    let busy = p.armed && buttons & (button::AIM | button::FIRE) != 0;
    let sprint = buttons & button::SPRINT != 0 && mz > 0.5 && !p.crouched && !p.exhausted && !busy;
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
    } * (1.0 - (1.0 - weapons::ADS_MOVE_MULT) * p.ads);
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

    if p.armed {
        if alive {
            step_weapon(p, input, buttons, sprint, dt, &mut res);
        }
    } else if buttons & button::FIRE != 0 && p.slap_cd <= 0.0 {
        p.slap_cd = slap::COOLDOWN;
        res.slap = true;
    }
    p.held = buttons;

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
        let (mut a, mut b) = (spawn_state(1, 0, false), spawn_state(1, 0, false));
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
            let p = spawn_state(1, i, false);
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
            difficulty: crate::protocol::Difficulty::Hard,
        })
        .unwrap();
        assert_eq!(
            w,
            r#"{"t":"welcome","id":1,"room":"R1","tickRate":60,"mode":"slap","difficulty":"hard"}"#
        );
    }

    // ---- weapons ----

    fn armed() -> PlayerState {
        let mut p = spawn_state(7, 0, true);
        p.trigger_spent = false;
        p
    }

    fn fire_for(p: &mut PlayerState, ticks: usize, buttons: u32, weapon: u8) -> (usize, bool) {
        let mut shots = 0;
        let mut dry = false;
        for i in 0..ticks {
            let r = step_player(
                p,
                &frame(|f| {
                    f.seq = i as u32;
                    f.buttons = buttons;
                    f.weapon = weapon;
                    f.yaw = p.yaw;
                }),
                TICK_DT,
            );
            shots += usize::from(!r.shots.is_empty());
            dry |= r.dry;
        }
        (shots, dry)
    }

    #[test]
    fn carbine_fires_at_its_rate_and_empties() {
        let mut p = armed();
        // one second of full auto at 750 rpm is 12.5 rounds
        let (n, _) = fire_for(&mut p, 60, button::FIRE, weapons::RIFLE);
        assert!((12..=13).contains(&n), "{n} shots");
        assert_eq!(p.mags[0] as usize, 30 - n);
        // keep holding: empties, then the dry trigger starts a reload by itself
        let (_, dry) = fire_for(&mut p, 120, button::FIRE, weapons::RIFLE);
        assert!(dry && p.reload > 0.0);
        fire_for(&mut p, (3.0 / TICK_DT) as usize, 0, weapons::RIFLE);
        assert_eq!(p.mags[0], 30);
        assert_eq!(p.reserves[0], 90);
    }

    #[test]
    fn tactical_reload_keeps_one_in_the_chamber() {
        let mut p = armed();
        fire_for(&mut p, 30, button::FIRE, weapons::RIFLE);
        let before = p.mags[0] + p.reserves[0];
        fire_for(&mut p, 1, button::RELOAD, weapons::RIFLE);
        assert!(
            (p.reload - 2.1).abs() < 0.05,
            "tactical reload time {}",
            p.reload
        );
        fire_for(&mut p, (2.2 / TICK_DT) as usize, 0, weapons::RIFLE);
        assert_eq!(p.mags[0], 31);
        assert_eq!(
            p.mags[0] + p.reserves[0],
            before,
            "no rounds lost or created"
        );
    }

    #[test]
    fn semi_auto_needs_trigger_release() {
        let mut p = armed();
        let (n, _) = fire_for(&mut p, 120, button::FIRE, weapons::SNIPER);
        // switching weapons takes 0.6 s, and the bolt rifle fires once per pull
        assert_eq!(n, 0, "swap time, trigger still held from before the swap");
        fire_for(&mut p, 1, 0, weapons::SNIPER);
        let (n, _) = fire_for(&mut p, 200, button::FIRE, weapons::SNIPER);
        assert_eq!(n, 1);
        // fire-mode toggle puts the carbine on semi
        let mut c = armed();
        fire_for(&mut c, 1, button::FIRE_MODE, weapons::RIFLE);
        assert!(c.semi);
        let (n, _) = fire_for(&mut c, 60, button::FIRE, weapons::RIFLE);
        assert_eq!(n, 1);
    }

    #[test]
    fn shotgun_fires_pellets_and_loads_shells_one_by_one() {
        let mut p = armed();
        fire_for(&mut p, 40, 0, weapons::SHOTGUN);
        let r = step_player(
            &mut p,
            &frame(|f| {
                f.buttons = button::FIRE;
                f.weapon = weapons::SHOTGUN;
            }),
            TICK_DT,
        );
        assert_eq!(r.shots.len(), 9);
        assert!(r.kick.0 > 0.04);
        fire_for(&mut p, 60, 0, weapons::SHOTGUN);
        p.mags[2] = 4;
        fire_for(&mut p, 1, button::RELOAD, weapons::SHOTGUN);
        fire_for(&mut p, (0.55 / TICK_DT) as usize, 0, weapons::SHOTGUN);
        assert_eq!(p.mags[2], 5, "one shell loaded");
        assert!(p.reload > 0.0, "and it keeps going while there is room");
    }

    #[test]
    fn spread_is_deterministic_and_aiming_tightens_it() {
        let shoot = |aim: bool| {
            let mut p = armed();
            p.ads = if aim { 1.0 } else { 0.0 };
            let b = if aim {
                button::FIRE | button::AIM
            } else {
                button::FIRE
            };
            step_player(&mut p, &frame(|f| f.buttons = b), TICK_DT).shots
        };
        assert_eq!(shoot(false), shoot(false));
        let hip = shoot(false);
        let ads = shoot(true);
        let a_hip = libm::acosf(-hip[0].dir[2].clamp(-1.0, 1.0));
        let a_ads = libm::acosf(-ads[0].dir[2].clamp(-1.0, 1.0));
        assert!(a_hip <= 0.041 && a_ads <= 0.0036, "{a_hip} {a_ads}");
    }

    #[test]
    fn sprinting_lowers_the_weapon() {
        let mut p = armed();
        let run = |p: &mut PlayerState, b: u32, n: usize| {
            let mut fired = 0;
            for _ in 0..n {
                let r = step_player(
                    p,
                    &frame(|f| {
                        f.move_z = 1.0;
                        f.buttons = b;
                    }),
                    TICK_DT,
                );
                fired += r.shots.len();
            }
            fired
        };
        run(&mut p, button::SPRINT, 60);
        assert!(p.lowered > 0.0);
        // pulling the trigger ends the sprint; the gun comes up after SPRINT_OUT
        let n = run(&mut p, button::SPRINT | button::FIRE, 6);
        assert_eq!(n, 0);
        let n = run(&mut p, button::SPRINT | button::FIRE, 30);
        assert!(n > 0);
    }

    #[test]
    fn health_regenerates_after_a_delay_and_the_dead_cannot_act() {
        let mut p = armed();
        p.hp = 40.0;
        p.regen_cd = combat::REGEN_DELAY;
        run(
            &mut p,
            (combat::REGEN_DELAY / TICK_DT) as usize - 5,
            InputFrame::default(),
        );
        assert!(p.hp < 41.0);
        run(&mut p, 120, InputFrame::default());
        assert!(p.hp > 60.0);
        p.hp = 0.0;
        p.dead = 2.0;
        let (n, _) = fire_for(&mut p, 30, button::FIRE, weapons::RIFLE);
        assert_eq!(n, 0);
        let x = p.x;
        run(&mut p, 30, frame(|f| f.move_z = 1.0));
        assert_eq!(p.x, x);
    }
}
