//! Bullets as projectiles (muzzle velocity, air drag, gravity drop, travel time),
//! raycasts against the map, and per-body-part hitboxes.

use serde::{Deserialize, Serialize};

use crate::constants::{combat, movement::GRAVITY};
use crate::map::{self, HALF};
use crate::protocol::PlayerState;
use crate::weapons;

pub type V3 = [f32; 3];

/// Material id the ground reports (map boxes use 0..=6).
pub const GROUND_MAT: u8 = 7;

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add_scaled(a: V3, d: V3, t: f32) -> V3 {
    [a[0] + d[0] * t, a[1] + d[1] * t, a[2] + d[2] * t]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn len(a: V3) -> f32 {
    libm::sqrtf(dot(a, a))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapHit {
    pub t: f32,
    pub normal: V3,
    pub mat: u8,
}

/// Nearest map surface along `o + d * t`, t in [0, max]. `d` must be unit length.
pub fn raycast_map(o: V3, d: V3, max: f32) -> Option<MapHit> {
    let mut best: Option<MapHit> = None;
    if d[1] < -1e-6 && o[1] >= 0.0 {
        let t = -o[1] / d[1];
        if t <= max {
            best = Some(MapHit {
                t,
                normal: [0.0, 1.0, 0.0],
                mat: GROUND_MAT,
            });
        }
    }
    for b in map::boxes() {
        let (mut t0, mut t1) = (0.0f32, best.map_or(max, |h| h.t));
        let mut axis = usize::MAX;
        let mut sign = 0.0;
        let mut ok = true;
        for a in 0..3 {
            if d[a].abs() < 1e-9 {
                if o[a] < b.min[a] || o[a] > b.max[a] {
                    ok = false;
                    break;
                }
                continue;
            }
            let inv = 1.0 / d[a];
            let (mut n, mut f) = ((b.min[a] - o[a]) * inv, (b.max[a] - o[a]) * inv);
            let mut s = -1.0;
            if n > f {
                std::mem::swap(&mut n, &mut f);
                s = 1.0;
            }
            if n > t0 {
                t0 = n;
                axis = a;
                sign = s;
            }
            t1 = t1.min(f);
            if t0 > t1 {
                ok = false;
                break;
            }
        }
        if ok && axis != usize::MAX {
            let mut normal = [0.0; 3];
            normal[axis] = sign;
            best = Some(MapHit {
                t: t0,
                normal,
                mat: b.mat,
            });
        }
    }
    best
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Zone {
    Head,
    Torso,
    Limb,
}

/// One hitbox: a capsule from `a` to `b` (a sphere when a == b).
#[derive(Clone, Copy, Debug)]
pub struct Hitbox {
    pub a: V3,
    pub b: V3,
    pub r: f32,
    pub zone: Zone,
}

/// Head, torso and legs for a player, following stance and lean.
pub fn hitboxes(p: &PlayerState) -> [Hitbox; 3] {
    let (rx, rz) = (libm::cosf(p.yaw), -libm::sinf(p.yaw));
    let at = |h: f32, shift: f32| [p.x + rx * shift, p.y + h, p.z + rz * shift];
    let lean = p.lean;
    let (head, torso, legs) = if p.crouched {
        (1.08, (0.6, 0.8), (0.12, 0.55))
    } else {
        (1.62, (1.0, 1.3), (0.12, 0.92))
    };
    [
        Hitbox {
            a: at(head, lean * 0.38),
            b: at(head, lean * 0.38),
            r: 0.13,
            zone: Zone::Head,
        },
        Hitbox {
            a: at(torso.0, lean * 0.12),
            b: at(torso.1, lean * 0.25),
            r: 0.23,
            zone: Zone::Torso,
        },
        Hitbox {
            a: at(legs.0, 0.0),
            b: at(legs.1, 0.0),
            r: 0.2,
            zone: Zone::Limb,
        },
    ]
}

fn ray_sphere(o: V3, d: V3, c: V3, r: f32) -> Option<f32> {
    let oc = sub(o, c);
    let b = dot(oc, d);
    let h = b * b - (dot(oc, oc) - r * r);
    if h < 0.0 {
        return None;
    }
    let t = -b - libm::sqrtf(h);
    (t >= 0.0).then_some(t)
}

/// Entry distance of a unit ray into a capsule.
fn ray_capsule(o: V3, d: V3, hb: &Hitbox) -> Option<f32> {
    let mut best = ray_sphere(o, d, hb.a, hb.r);
    if let Some(t) = ray_sphere(o, d, hb.b, hb.r) {
        best = Some(best.map_or(t, |v| v.min(t)));
    }
    let ba = sub(hb.b, hb.a);
    let baba = dot(ba, ba);
    if baba > 1e-8 {
        let oa = sub(o, hb.a);
        let bard = dot(ba, d);
        let baoa = dot(ba, oa);
        let k2 = baba - bard * bard;
        if k2 > 1e-8 {
            let k1 = baba * dot(oa, d) - baoa * bard;
            let k0 = baba * dot(oa, oa) - baoa * baoa - hb.r * hb.r * baba;
            let h = k1 * k1 - k2 * k0;
            if h >= 0.0 {
                let t = (-k1 - libm::sqrtf(h)) / k2;
                let y = baoa + t * bard;
                if t >= 0.0 && y > 0.0 && y < baba {
                    best = Some(best.map_or(t, |v| v.min(t)));
                }
            }
        }
    }
    best
}

/// Nearest hitbox of `p` along the ray within `max`.
pub fn ray_player(o: V3, d: V3, max: f32, p: &PlayerState) -> Option<(f32, Zone)> {
    if p.dead > 0.0 {
        return None;
    }
    // cheap reject: distance from the body's centre line
    let c = [p.x, p.y + 0.9, p.z];
    let oc = sub(c, o);
    let along = dot(oc, d).clamp(0.0, max);
    if len(sub(add_scaled(o, d, along), c)) > 1.6 {
        return None;
    }
    let mut best: Option<(f32, Zone)> = None;
    for hb in hitboxes(p) {
        if let Some(t) = ray_capsule(o, d, &hb) {
            if t <= max && best.is_none_or(|b| t < b.0) {
                best = Some((t, hb.zone));
            }
        }
    }
    best
}

/// A bullet in flight (server side; the client runs the same maths for tracers).
#[derive(Clone, Debug)]
pub struct Bullet {
    pub owner: u32,
    pub weapon: u8,
    pub pos: V3,
    pub vel: V3,
    pub travelled: f32,
    pub age: f32,
    /// Ticks to rewind other players by when testing hits (lag compensation).
    pub rewind: u32,
}

impl Bullet {
    pub fn new(owner: u32, weapon: u8, origin: V3, dir: V3, rewind: u32) -> Self {
        let v = weapons::spec(weapon).muzzle_velocity;
        Bullet {
            owner,
            weapon,
            pos: origin,
            vel: [dir[0] * v, dir[1] * v, dir[2] * v],
            travelled: 0.0,
            age: 0.0,
            rewind,
        }
    }

    /// Move one step. Returns the segment covered as (start, unit dir, length).
    pub fn advance(&mut self, dt: f32) -> (V3, V3, f32) {
        let k = libm::expf(-weapons::spec(self.weapon).drag * dt);
        self.vel = [
            self.vel[0] * k,
            self.vel[1] * k - GRAVITY * dt,
            self.vel[2] * k,
        ];
        let start = self.pos;
        self.pos = add_scaled(self.pos, self.vel, dt);
        self.age += dt;
        let seg = sub(self.pos, start);
        let l = len(seg);
        let d = if l > 0.0 {
            [seg[0] / l, seg[1] / l, seg[2] / l]
        } else {
            [0.0, -1.0, 0.0]
        };
        (start, d, l)
    }

    pub fn expired(&self) -> bool {
        let lim = HALF + 60.0;
        self.age > combat::BULLET_LIFE
            || self.pos[1] < -1.0
            || self.pos[0].abs() > lim
            || self.pos[2].abs() > lim
    }

    /// Damage this bullet does to a zone at its current travelled distance.
    pub fn damage(&self, zone: Zone) -> f32 {
        let s = weapons::spec(self.weapon);
        let m = match zone {
            Zone::Head => s.head_mult,
            Zone::Torso => 1.0,
            Zone::Limb => s.limb_mult,
        };
        s.damage * weapons::falloff(s, self.travelled) * m
    }
}

pub fn point(o: V3, d: V3, t: f32) -> V3 {
    add_scaled(o, d, t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::TICK_DT;
    use crate::sim::spawn_state;

    #[test]
    fn raycast_hits_wall_with_normal() {
        // perimeter wall inner face is at z = -32
        let h = raycast_map([0.0, 1.5, 0.0], [0.0, 0.0, -1.0], 100.0).unwrap();
        assert!(
            (h.t - 32.0).abs() < 0.01 && h.normal == [0.0, 0.0, 1.0],
            "{h:?}"
        );
        let g = raycast_map([0.0, 1.0, 0.0], [0.0, -1.0, 0.0], 5.0).unwrap();
        assert!((g.t - 1.0).abs() < 1e-4 && g.mat == GROUND_MAT);
        assert!(raycast_map([0.0, 1.5, 0.0], [0.0, 0.0, -1.0], 10.0).is_none());
    }

    #[test]
    fn hitbox_zones() {
        let mut p = spawn_state(1, 0, true);
        p.x = 0.0;
        p.z = 0.0;
        p.y = 0.0;
        let shoot = |h: f32, p: &PlayerState| ray_player([0.0, h, 10.0], [0.0, 0.0, -1.0], 50.0, p);
        assert_eq!(shoot(1.62, &p).unwrap().1, Zone::Head);
        assert_eq!(shoot(1.2, &p).unwrap().1, Zone::Torso);
        assert_eq!(shoot(0.5, &p).unwrap().1, Zone::Limb);
        assert!(shoot(1.9, &p).is_none());
        // crouching drops the head below a standing-head shot
        p.crouched = true;
        assert!(shoot(1.62, &p).is_none());
        assert_eq!(shoot(1.08, &p).unwrap().1, Zone::Head);
        p.dead = 1.0;
        assert!(shoot(0.8, &p).is_none());
    }

    #[test]
    fn bullets_drop_and_slow() {
        // carbine level at 1.5 m: ~100 m away it has dropped a few cm and lost speed
        let mut b = Bullet::new(1, weapons::RIFLE, [0.0, 1.5, 0.0], [1.0, 0.0, 0.0], 0);
        let mut x = 0.0;
        let mut t = 0.0;
        while x < 100.0 {
            let (_, _, l) = b.advance(TICK_DT);
            x = b.pos[0];
            b.travelled += l;
            t += TICK_DT;
        }
        let drop = 1.5 - b.pos[1];
        let speed = len(b.vel);
        assert!(drop > 0.02 && drop < 0.15, "drop {drop}");
        assert!(speed < 880.0 && speed > 700.0, "speed {speed}");
        assert!(t > 0.1 && t < 0.15, "time {t}");
        // shotgun pellets lose most of their damage by 30 m
        let mut s = Bullet::new(1, weapons::SHOTGUN, [0.0; 3], [1.0, 0.0, 0.0], 0);
        s.travelled = 30.0;
        assert!(s.damage(Zone::Torso) < 3.0);
    }
}
