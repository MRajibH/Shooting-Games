//! Server-side AI players. A bot produces the same InputFrames a person would and
//! runs through the same simulation, so it plays by the same rules (speed, stamina,
//! recoil, ammo, reloads). It sees only what has line of sight, reacts with a human
//! delay, and aims with an error that shrinks the longer it tracks a target.

use riftline_sim::ballistics::raycast_map;
use riftline_sim::constants::{combat, rules, slap};
use riftline_sim::map::FLAGS;
use riftline_sim::protocol::{button, FlagState, InputFrame, Mode, PlayerState};
use riftline_sim::sim::eye;
use riftline_sim::weapons;

use crate::nav::nav;

pub const NAMES: [&str; 8] = [
    "Kestrel",
    "Basalt",
    "Wren",
    "Juniper",
    "Halyard",
    "Tamarack",
    "Cormorant",
    "Sable",
];

/// What a bot can see of the world this tick.
pub struct View<'a> {
    pub players: &'a [PlayerState],
    pub mode: Mode,
    pub flags: &'a [FlagState],
    pub dt: f32,
}

pub struct Brain {
    seq: u32,
    rng: u32,
    weapon: u8,
    yaw: f32,
    pitch: f32,
    path: Vec<(f32, f32)>,
    repath: f32,
    target: Option<u32>,
    seen_for: f32,
    reaction: f32,
    strafe: f32,
    strafe_t: f32,
    burst_t: f32,
    stuck_t: f32,
    last_pos: (f32, f32),
    time: f32,
    phase: f32,
}

fn wrap(a: f32) -> f32 {
    libm::atan2f(libm::sinf(a), libm::cosf(a))
}

impl Brain {
    pub fn new(id: u32) -> Brain {
        let mut b = Brain {
            seq: 0,
            rng: id.wrapping_mul(2_654_435_761) | 1,
            weapon: 0,
            yaw: 0.0,
            pitch: 0.0,
            path: Vec::new(),
            repath: 0.0,
            target: None,
            seen_for: 0.0,
            reaction: 0.45,
            strafe: 1.0,
            strafe_t: 0.0,
            burst_t: 0.0,
            stuck_t: 0.0,
            last_pos: (0.0, 0.0),
            time: 0.0,
            phase: 0.0,
        };
        b.weapon = if b.rand() < 0.65 {
            weapons::RIFLE
        } else {
            weapons::SMG
        };
        b.phase = b.rand() * 10.0;
        b
    }

    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng >> 8) as f32 / 16_777_216.0
    }

    fn turn_to(&mut self, yaw: f32, pitch: f32, rate: f32, dt: f32) {
        let dy = wrap(yaw - self.yaw);
        let max = rate * dt;
        self.yaw = wrap(self.yaw + dy.clamp(-max, max));
        self.pitch += (pitch - self.pitch).clamp(-max, max);
    }

    fn enemy(me: &PlayerState, p: &PlayerState) -> bool {
        p.id != me.id && p.dead <= 0.0 && p.ko <= 0.0 && (me.team == 0 || p.team != me.team)
    }

    /// Line of sight from my eye to a point on the target.
    fn sees(me: &PlayerState, p: &PlayerState) -> Option<f32> {
        let o = [me.x, me.y + eye(me), me.z];
        let t = [p.x, p.y + if p.crouched { 0.8 } else { 1.2 }, p.z];
        let d = [t[0] - o[0], t[1] - o[1], t[2] - o[2]];
        let len = libm::sqrtf(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
        if !(1e-3..=80.0).contains(&len) {
            return None;
        }
        let dir = [d[0] / len, d[1] / len, d[2] / len];
        raycast_map(o, dir, len).is_none().then_some(len)
    }

    /// Where to head when no enemy is in sight.
    fn goal(&self, me: &PlayerState, v: &View) -> Option<(f32, f32)> {
        if v.mode == Mode::Domination {
            // nearest point my team does not hold (or is losing)
            let mine = |f: &FlagState| {
                (f.owner == me.team)
                    && (if me.team == 1 {
                        f.progress <= -0.9
                    } else {
                        f.progress >= 0.9
                    })
            };
            let best = v
                .flags
                .iter()
                .zip(FLAGS)
                .filter(|(f, _)| !mine(f))
                .map(|(_, p)| p)
                .min_by(|a, b| {
                    let d = |p: &(f32, f32)| libm::hypotf(p.0 - me.x, p.1 - me.z);
                    d(a).total_cmp(&d(b))
                });
            if best.is_some() {
                return best;
            }
        }
        v.players
            .iter()
            .filter(|p| Self::enemy(me, p))
            .min_by(|a, b| {
                let d = |p: &PlayerState| libm::hypotf(p.x - me.x, p.z - me.z);
                d(a).total_cmp(&d(b))
            })
            .map(|p| (p.x, p.z))
    }

    pub fn think(&mut self, me: &PlayerState, v: &View) -> InputFrame {
        let dt = v.dt;
        self.seq += 1;
        self.time += dt;
        let mut f = InputFrame {
            seq: self.seq,
            weapon: self.weapon,
            ..Default::default()
        };
        if me.dead > 0.0 || me.ko > 0.0 {
            self.yaw = me.yaw;
            self.pitch = 0.0;
            self.path.clear();
            self.target = None;
            f.yaw = self.yaw;
            return f;
        }

        // nearest enemy in sight
        let seen = v
            .players
            .iter()
            .filter(|p| Self::enemy(me, p))
            .filter_map(|p| Self::sees(me, p).map(|d| (p, d)))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        match seen {
            Some((p, _)) if self.target == Some(p.id) => self.seen_for += dt,
            Some((p, _)) => {
                self.target = Some(p.id);
                self.seen_for = 0.0;
                self.reaction = 0.3 + self.rand() * 0.35;
            }
            None => {
                self.target = None;
                self.seen_for = 0.0;
            }
        }

        let mut buttons = 0;
        let (mut mx, mut mz) = (0.0, 0.0);
        if let Some((t, dist)) = seen {
            let armed = v.mode.armed();
            // aim at the chest from where the shots leave (beside the head)
            let aim_h = if t.crouched { 0.8 } else { 1.15 };
            let mut yaw = libm::atan2f(-(t.x - me.x), -(t.z - me.z));
            for _ in 0..2 {
                let ox = me.x + libm::cosf(yaw) * combat::SHOULDER;
                let oz = me.z - libm::sinf(yaw) * combat::SHOULDER;
                yaw = libm::atan2f(-(t.x - ox), -(t.z - oz));
            }
            let flat = libm::hypotf(t.x - me.x, t.z - me.z).max(0.1);
            let pitch = libm::atan2f(t.y + aim_h - (me.y + eye(me)), flat);
            // tracking error: large on first sight, settles as the bot follows the target
            let err = 0.07 / (1.0 + self.seen_for * 2.5) + 0.01;
            let t2 = self.time + self.phase;
            let ey = err * (libm::sinf(t2 * 1.9) * 0.7 + libm::sinf(t2 * 3.7) * 0.3);
            let ep = err * 0.6 * libm::sinf(t2 * 2.3 + 1.0);
            self.turn_to(yaw + ey, pitch + ep, 5.5, dt);
            let off = libm::fabsf(wrap(yaw - self.yaw)) + libm::fabsf(pitch - self.pitch);

            if armed {
                if dist > 10.0 {
                    buttons |= button::AIM;
                }
                let mag = me.mags[(me.weapon as usize).min(3)];
                if mag == 0 {
                    buttons |= button::RELOAD;
                } else if self.seen_for > self.reaction && off < 0.06 + 0.6 / dist {
                    // short bursts: 0.3 s on, 0.25 s off
                    self.burst_t = (self.burst_t + dt) % 0.55;
                    if self.burst_t < 0.3 {
                        buttons |= button::FIRE;
                    }
                }
                // keep moving while fighting: strafe, and close in or back off
                self.strafe_t -= dt;
                if self.strafe_t <= 0.0 {
                    self.strafe = if self.rand() < 0.5 { -1.0 } else { 1.0 };
                    self.strafe_t = 0.6 + self.rand() * 0.9;
                }
                mx = self.strafe;
                mz = if dist > 28.0 {
                    1.0
                } else if dist < 6.0 {
                    -0.6
                } else {
                    0.0
                };
            } else {
                // slap: run in and swing when in reach
                mz = 1.0;
                if dist > 5.0 {
                    buttons |= button::SPRINT;
                }
                if dist < slap::RANGE * 0.9 && off < 0.5 && self.seen_for > self.reaction {
                    buttons |= button::FIRE;
                }
            }
            self.path.clear();
        } else {
            // travel toward an objective along a ground path
            self.repath -= dt;
            if self.repath <= 0.0 || self.path.is_empty() {
                self.repath = 1.5;
                self.path = self
                    .goal(me, v)
                    .map(|g| nav().path((me.x, me.z), g))
                    .unwrap_or_default();
            }
            while let Some(&(wx, wz)) = self.path.first() {
                if libm::hypotf(wx - me.x, wz - me.z) < 0.9 && self.path.len() > 1 {
                    self.path.remove(0);
                } else {
                    break;
                }
            }
            let holding = v.mode == Mode::Domination
                && FLAGS
                    .iter()
                    .any(|&(fx, fz)| libm::hypotf(fx - me.x, fz - me.z) < rules::FLAG_RADIUS * 0.6)
                && self.path.len() <= 1;
            if let (Some(&(wx, wz)), false) = (self.path.first(), holding) {
                let yaw = libm::atan2f(-(wx - me.x), -(wz - me.z));
                self.turn_to(yaw, 0.0, 6.0, dt);
                let facing = libm::fabsf(wrap(yaw - self.yaw)) < 0.6;
                mz = if facing { 1.0 } else { 0.3 };
                let far = libm::hypotf(wx - me.x, wz - me.z) > 6.0 || self.path.len() > 1;
                if facing && far && me.stamina > 0.3 {
                    buttons |= button::SPRINT;
                }
            } else {
                // on the point: scan around
                let y = self.yaw + 0.8 * dt;
                self.turn_to(y, 0.0, 6.0, dt);
            }
            let w = (me.weapon as usize).min(3);
            if v.mode.armed() && me.mags[w] < weapons::WEAPONS[w].mag / 3 {
                buttons |= button::RELOAD;
            }
        }

        // unstick: if trying to move but not getting anywhere, hop and re-plan
        if mx != 0.0 || mz != 0.0 {
            self.stuck_t += dt;
            if self.stuck_t > 1.0 {
                if libm::hypotf(me.x - self.last_pos.0, me.z - self.last_pos.1) < 0.5 {
                    buttons |= button::JUMP;
                    self.path.clear();
                    self.strafe = -self.strafe;
                }
                self.stuck_t = 0.0;
                self.last_pos = (me.x, me.z);
            }
        }

        f.yaw = self.yaw;
        f.pitch = self.pitch;
        f.move_x = mx;
        f.move_z = mz;
        f.buttons = buttons;
        f
    }
}
