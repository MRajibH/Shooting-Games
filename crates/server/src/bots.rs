//! Server-side AI players. A bot produces the same InputFrames a person would and
//! runs through the same simulation, so it plays by the same rules (speed, stamina,
//! recoil, ammo, reloads). It sees only what has line of sight, hears gunfire, remembers
//! where it last saw or heard someone, reacts with a human delay, aims with an error
//! that settles while it tracks, and (above Easy) falls back to cover to reload or
//! recover before peeking out again.

use riftline_sim::ballistics::{raycast_map, V3};
use riftline_sim::constants::{combat, rules, slap};
use riftline_sim::map::FLAGS;
use riftline_sim::protocol::{button, Difficulty, FlagState, InputFrame, Mode, PlayerState};
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

/// How good a bot is.
#[derive(Clone, Copy, Debug)]
pub struct Skill {
    /// Seconds from first sight to first shot (random in this range).
    pub reaction: (f32, f32),
    /// Aim error on first sight and after settling (radians), and how fast it settles.
    pub err_first: f32,
    pub err_settled: f32,
    pub settle: f32,
    /// Fastest turn (radians per second).
    pub turn: f32,
    /// Trigger held / released during bursts (seconds).
    pub burst_on: f32,
    pub burst_off: f32,
    /// Falls back to cover below this health (0 = never uses cover).
    pub retreat_hp: f32,
    /// Aims down sights beyond this distance.
    pub ads_beyond: f32,
    /// Crouches to steady long shots.
    pub crouch_long: bool,
    /// Hearing range for gunfire (metres).
    pub hearing: f32,
}

pub fn skill(d: Difficulty) -> Skill {
    match d {
        Difficulty::Easy => Skill {
            reaction: (0.65, 1.1),
            err_first: 0.13,
            err_settled: 0.03,
            settle: 1.2,
            turn: 3.2,
            burst_on: 0.2,
            burst_off: 0.5,
            retreat_hp: 0.0,
            ads_beyond: 18.0,
            crouch_long: false,
            hearing: 25.0,
        },
        Difficulty::Normal => Skill {
            reaction: (0.3, 0.65),
            err_first: 0.07,
            err_settled: 0.012,
            settle: 2.5,
            turn: 5.5,
            burst_on: 0.3,
            burst_off: 0.25,
            retreat_hp: 40.0,
            ads_beyond: 10.0,
            crouch_long: false,
            hearing: 45.0,
        },
        Difficulty::Hard => Skill {
            reaction: (0.17, 0.32),
            err_first: 0.045,
            err_settled: 0.006,
            settle: 4.0,
            turn: 8.0,
            burst_on: 0.4,
            burst_off: 0.16,
            retreat_hp: 55.0,
            ads_beyond: 7.0,
            crouch_long: true,
            hearing: 65.0,
        },
    }
}

/// A gunshot the bots can hear: where, and who fired.
#[derive(Clone, Copy, Debug)]
pub struct Noise {
    pub at: V3,
    pub by: u32,
}

/// What a bot can perceive this tick.
pub struct View<'a> {
    pub players: &'a [PlayerState],
    pub mode: Mode,
    pub flags: &'a [FlagState],
    pub noises: &'a [Noise],
    pub dt: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Plan {
    /// Head for the objective, the last known enemy position, or wander.
    Roam,
    /// Get to a spot the enemy cannot see, reload / recover there.
    Cover {
        spot: (f32, f32),
        until: f32,
        crouch: bool,
    },
}

pub struct Brain {
    pub skill: Skill,
    seq: u32,
    rng: u32,
    weapon: u8,
    yaw: f32,
    pitch: f32,
    path: Vec<(f32, f32)>,
    path_goal: Option<V3>,
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
    plan: Plan,
    /// Last place an enemy was seen or heard, and how long ago.
    lead: Option<(V3, f32)>,
    wander: Option<(f32, f32)>,
    /// Time since any contact; after a while the bot is told roughly where people are.
    quiet: f32,
    /// Times this bot took cover (for tests and tuning).
    pub covers: u32,
}

fn wrap(a: f32) -> f32 {
    libm::atan2f(libm::sinf(a), libm::cosf(a))
}

fn flat(a: V3, x: f32, z: f32) -> f32 {
    libm::hypotf(a[0] - x, a[2] - z)
}

impl Brain {
    pub fn new(id: u32, difficulty: Difficulty) -> Brain {
        let mut b = Brain {
            skill: skill(difficulty),
            seq: 0,
            rng: id.wrapping_mul(2_654_435_761) | 1,
            weapon: 0,
            yaw: 0.0,
            pitch: 0.0,
            path: Vec::new(),
            path_goal: None,
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
            plan: Plan::Roam,
            lead: None,
            wander: None,
            quiet: 0.0,
            covers: 0,
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

    /// Unblocked line between two points.
    fn line(o: V3, t: V3) -> Option<f32> {
        let d = [t[0] - o[0], t[1] - o[1], t[2] - o[2]];
        let len = libm::sqrtf(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
        if !(1e-3..=80.0).contains(&len) {
            return None;
        }
        let dir = [d[0] / len, d[1] / len, d[2] / len];
        raycast_map(o, dir, len).is_none().then_some(len)
    }

    fn eye_of(p: &PlayerState) -> V3 {
        [p.x, p.y + eye(p), p.z]
    }

    /// Line of sight from my eye to the target's chest.
    fn sees(me: &PlayerState, p: &PlayerState) -> Option<f32> {
        Self::line(
            Self::eye_of(me),
            [p.x, p.y + if p.crouched { 0.8 } else { 1.2 }, p.z],
        )
    }

    /// A nearby ground spot the threat cannot see: fully hidden if possible, else
    /// hidden when crouched. Prefers spots close to me and away from the threat.
    pub fn find_cover(me: &PlayerState, threat: &PlayerState) -> Option<((f32, f32), bool)> {
        if me.y > 1.2 {
            return None;
        }
        let te = Self::eye_of(threat);
        let mut best: Option<((f32, f32), bool, f32)> = None;
        for (x, z) in nav().cells_near(me.x, me.z, 9.0) {
            if Self::line(te, [x, 0.9, z]).is_some() {
                continue;
            }
            let full = Self::line(te, [x, 1.6, z]).is_none();
            let d = libm::hypotf(x - me.x, z - me.z);
            let closer = libm::hypotf(threat.x - me.x, threat.z - me.z)
                - libm::hypotf(threat.x - x, threat.z - z);
            let score = d + closer.max(0.0) * 1.5 + if full { 0.0 } else { 2.5 };
            if best.is_none_or(|b| score < b.2) {
                best = Some(((x, z), !full, score));
            }
        }
        best.map(|b| (b.0, b.1))
    }

    /// Where to head when no enemy is in sight.
    fn goal(&mut self, me: &PlayerState, v: &View) -> Option<V3> {
        if v.mode == Mode::Domination {
            let held = |f: &FlagState| {
                f.owner == me.team
                    && if me.team == 1 {
                        f.progress <= -0.9
                    } else {
                        f.progress >= 0.9
                    }
            };
            let best = v
                .flags
                .iter()
                .zip(FLAGS)
                .filter(|(f, _)| !held(f))
                .map(|(_, p)| p)
                .min_by(|a, b| {
                    libm::hypotf(a.0 - me.x, a.1 - me.z)
                        .total_cmp(&libm::hypotf(b.0 - me.x, b.1 - me.z))
                });
            if let Some((x, z)) = best {
                return Some([x, 0.0, z]);
            }
        }
        if let Some((at, _)) = self.lead {
            return Some(at);
        }
        // after a long quiet spell, head roughly toward the nearest enemy (keeps lone players busy)
        if self.quiet > 8.0 {
            if let Some(p) = v
                .players
                .iter()
                .filter(|p| Self::enemy(me, p))
                .min_by(|a, b| {
                    libm::hypotf(a.x - me.x, a.z - me.z)
                        .total_cmp(&libm::hypotf(b.x - me.x, b.z - me.z))
                })
            {
                return Some([p.x, p.y, p.z]);
            }
        }
        let w = match self.wander {
            Some(w) if libm::hypotf(w.0 - me.x, w.1 - me.z) > 2.0 => w,
            _ => {
                let (u, v2) = (self.rand(), self.rand());
                let w = nav().random_spot(u, v2);
                self.wander = Some(w);
                w
            }
        };
        Some([w.0, 0.0, w.1])
    }

    /// Walk the path toward `goal`. Returns forward input.
    fn follow(&mut self, me: &PlayerState, goal: V3, dt: f32, buttons: &mut u32) -> f32 {
        self.repath -= dt;
        let moved_goal = self
            .path_goal
            .is_none_or(|g| flat(g, goal[0], goal[2]) > 2.0);
        if self.repath <= 0.0 || self.path.is_empty() || moved_goal {
            self.repath = 1.5;
            self.path = nav().path([me.x, me.y, me.z], goal);
            self.path_goal = Some(goal);
        }
        while let Some(&(wx, wz)) = self.path.first() {
            if libm::hypotf(wx - me.x, wz - me.z) < 0.9 && self.path.len() > 1 {
                self.path.remove(0);
            } else {
                break;
            }
        }
        let Some(&(wx, wz)) = self.path.first() else {
            return 0.0;
        };
        let yaw = libm::atan2f(-(wx - me.x), -(wz - me.z));
        self.turn_to(yaw, 0.0, self.skill.turn.max(6.0), dt);
        let facing = libm::fabsf(wrap(yaw - self.yaw)) < 0.6;
        let far = libm::hypotf(wx - me.x, wz - me.z) > 6.0 || self.path.len() > 1;
        if facing && far && me.stamina > 0.3 {
            *buttons |= button::SPRINT;
        }
        if facing {
            1.0
        } else {
            0.3
        }
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
            self.plan = Plan::Roam;
            self.lead = None;
            f.yaw = self.yaw;
            return f;
        }
        self.quiet += dt;
        if let Some((_, age)) = self.lead.as_mut() {
            *age += dt;
            if *age > 12.0 {
                self.lead = None;
            }
        }
        // gunfire within hearing gives away where someone is
        for n in v.noises {
            if n.by == me.id {
                continue;
            }
            let enemy = v.players.iter().any(|p| p.id == n.by && Self::enemy(me, p));
            if enemy && flat(n.at, me.x, me.z) < self.skill.hearing {
                self.lead = Some((n.at, 0.0));
                self.quiet = 0.0;
            }
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
                let (a, b) = self.skill.reaction;
                self.reaction = a + self.rand() * (b - a);
            }
            None => {
                self.target = None;
                self.seen_for = 0.0;
            }
        }
        if let Some((p, _)) = seen {
            self.lead = Some(([p.x, p.y, p.z], 0.0));
            self.quiet = 0.0;
        }

        let armed = v.mode.armed();
        let w = (me.weapon as usize).min(3);
        let mag = me.mags[w];
        let low_ammo = mag == 0 || (mag < weapons::WEAPONS[w].mag / 4 && me.reserves[w] > 0);

        // decide whether to fall back to cover
        if armed && self.skill.retreat_hp > 0.0 && self.plan == Plan::Roam {
            if let Some((t, _)) = seen {
                if me.hp < self.skill.retreat_hp || low_ammo {
                    if let Some((spot, crouch)) = Self::find_cover(me, t) {
                        self.plan = Plan::Cover {
                            spot,
                            until: self.time + 6.0,
                            crouch,
                        };
                        self.covers += 1;
                        self.path.clear();
                    }
                }
            }
        }

        let mut buttons = 0;
        let (mut mx, mut mz) = (0.0, 0.0);

        if let Plan::Cover {
            spot,
            until,
            crouch,
        } = self.plan
        {
            // in cover: reload and recover, fight back only if found there
            let there = libm::hypotf(spot.0 - me.x, spot.1 - me.z) < 0.7;
            let ready = me.reload <= 0.0 && !low_ammo && me.hp >= 80.0;
            if ready || self.time > until {
                self.plan = Plan::Roam;
            }
            if !there {
                mz = self.follow(me, [spot.0, 0.0, spot.1], dt, &mut buttons);
            } else if crouch {
                buttons |= button::CROUCH;
            }
            if mag < weapons::WEAPONS[w].mag {
                buttons |= button::RELOAD;
            }
            if let (Some((t, dist)), true) = (seen, there) {
                buttons |= self.engage(me, t, dist, armed, dt) & !button::RELOAD;
            }
        } else if let Some((t, dist)) = seen {
            buttons |= self.engage(me, t, dist, armed, dt);
            if armed {
                self.strafe_t -= dt;
                if self.strafe_t <= 0.0 {
                    self.strafe = if self.rand() < 0.5 { -1.0 } else { 1.0 };
                    self.strafe_t = 0.6 + self.rand() * 0.9;
                }
                if self.skill.crouch_long && dist > 15.0 && self.seen_for > self.reaction {
                    buttons |= button::CROUCH;
                } else {
                    mx = self.strafe;
                }
                mz = if dist > 28.0 {
                    1.0
                } else if dist < 6.0 {
                    -0.6
                } else {
                    0.0
                };
            } else {
                mz = 1.0;
                if dist > 5.0 {
                    buttons |= button::SPRINT;
                }
            }
            self.path.clear();
        } else {
            // nobody in sight: objective, last known position, or wander
            let goal = self.goal(me, v);
            let holding = v.mode == Mode::Domination
                && goal.is_some_and(|g| flat(g, me.x, me.z) < rules::FLAG_RADIUS * 0.6);
            if holding {
                let y = self.yaw + 0.8 * dt;
                self.turn_to(y, 0.0, 6.0, dt);
            } else if let Some(g) = goal {
                // reached the last known position and nobody is there: forget it
                if self.lead.is_some_and(|(at, _)| flat(at, me.x, me.z) < 2.0) {
                    self.lead = None;
                }
                mz = self.follow(me, g, dt, &mut buttons);
            }
            if armed && mag < weapons::WEAPONS[w].mag / 3 {
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
                    self.wander = None;
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

    /// Track and shoot (or slap) a visible target. Returns buttons to press.
    fn engage(
        &mut self,
        me: &PlayerState,
        t: &PlayerState,
        dist: f32,
        armed: bool,
        dt: f32,
    ) -> u32 {
        let aim_h = if t.crouched { 0.8 } else { 1.15 };
        let mut yaw = libm::atan2f(-(t.x - me.x), -(t.z - me.z));
        for _ in 0..2 {
            let ox = me.x + libm::cosf(yaw) * combat::SHOULDER;
            let oz = me.z - libm::sinf(yaw) * combat::SHOULDER;
            yaw = libm::atan2f(-(t.x - ox), -(t.z - oz));
        }
        let flat_d = libm::hypotf(t.x - me.x, t.z - me.z).max(0.1);
        let pitch = libm::atan2f(t.y + aim_h - (me.y + eye(me)), flat_d);
        let s = self.skill;
        let err = (s.err_first - s.err_settled) / (1.0 + self.seen_for * s.settle) + s.err_settled;
        let t2 = self.time + self.phase;
        let ey = err * (libm::sinf(t2 * 1.9) * 0.7 + libm::sinf(t2 * 3.7) * 0.3);
        let ep = err * 0.6 * libm::sinf(t2 * 2.3 + 1.0);
        self.turn_to(yaw + ey, pitch + ep, s.turn, dt);
        let off = libm::fabsf(wrap(yaw - self.yaw)) + libm::fabsf(pitch - self.pitch);
        let mut b = 0;
        if armed {
            if dist > s.ads_beyond {
                b |= button::AIM;
            }
            let mag = me.mags[(me.weapon as usize).min(3)];
            if mag == 0 {
                b |= button::RELOAD;
            } else if self.seen_for > self.reaction && off < 0.06 + 0.6 / dist {
                let period = s.burst_on + s.burst_off;
                self.burst_t = (self.burst_t + dt) % period;
                if self.burst_t < s.burst_on {
                    b |= button::FIRE;
                }
            }
        } else if dist < slap::RANGE * 0.9 && off < 0.5 && self.seen_for > self.reaction {
            b |= button::FIRE;
        }
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riftline_sim::sim::spawn_state;

    fn at(id: u32, x: f32, z: f32) -> PlayerState {
        let mut p = spawn_state(id, 0, true);
        (p.x, p.y, p.z) = (x, 0.0, z);
        p
    }

    #[test]
    fn finds_cover_out_of_the_threats_sight() {
        // open ground near the 1 m barrier at (0, 4); the threat stands to the north
        let me = at(1, 1.0, 2.0);
        let threat = at(2, 0.0, -12.0);
        let ((x, z), crouch) = Brain::find_cover(&me, &threat).expect("cover nearby");
        let te = Brain::eye_of(&threat);
        assert!(
            Brain::line(te, [x, 0.9, z]).is_none(),
            "spot {x},{z} is visible"
        );
        assert!(z > 4.0, "behind the barrier, not in front: {x},{z}");
        assert!(libm::hypotf(x - me.x, z - me.z) < 9.0);
        let _ = crouch;
    }

    #[test]
    fn harder_bots_react_faster_and_aim_tighter() {
        let (e, h) = (skill(Difficulty::Easy), skill(Difficulty::Hard));
        assert!(h.reaction.1 < e.reaction.0);
        assert!(h.err_settled < e.err_settled && h.turn > e.turn);
        assert!(e.retreat_hp == 0.0 && h.retreat_hp > 0.0);
    }
}
