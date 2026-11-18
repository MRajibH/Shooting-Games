//! Navigation for bots: a walkable ground grid built from the shared map, plus an
//! upper layer (the steel walkway) joined to the ground by the two staircases.
//! A* over both, with straight-line shortcuts on the ground so paths do not zig-zag.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::sync::OnceLock;

use riftline_sim::constants::body::STAND_HEIGHT;
use riftline_sim::map::HALF;
use riftline_sim::sim::blocked;

const STEP: f32 = 1.0;
/// Extra clearance from walls, so bots do not scrape along them.
const MARGIN: f32 = 0.25;
/// Walkway deck height and centre line (see map.rs).
const DECK_Y: f32 = 3.0;
const DECK_Z: f32 = -14.0;

pub type P3 = [f32; 3];

pub struct Nav {
    n: usize,
    walk: Vec<bool>,
    /// Upper-layer and stair nodes (ids n*n ..).
    extra: Vec<P3>,
    links: HashMap<usize, Vec<usize>>,
}

fn open_at(x: f32, z: f32) -> bool {
    [
        (0.0, 0.0),
        (MARGIN, 0.0),
        (-MARGIN, 0.0),
        (0.0, MARGIN),
        (0.0, -MARGIN),
    ]
    .iter()
    .all(|(dx, dz)| blocked(x + dx, 0.0, z + dz, STAND_HEIGHT).is_none())
}

fn dist(a: P3, b: P3) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    libm::sqrtf(d[0] * d[0] + d[1] * d[1] + d[2] * d[2])
}

impl Nav {
    fn build() -> Nav {
        let n = ((2.0 * (HALF - 1.0)) / STEP) as usize + 1;
        let mut nav = Nav {
            n,
            walk: vec![false; n * n],
            extra: Vec::new(),
            links: HashMap::new(),
        };
        for j in 0..n {
            for i in 0..n {
                let (x, z) = nav.pos(i, j);
                nav.walk[j * n + i] = open_at(x, z);
            }
        }
        // walkway deck: a chain of nodes along its centre line
        let deck: Vec<usize> = (0..12)
            .map(|k| nav.add([-5.5 + k as f32, DECK_Y, DECK_Z]))
            .collect();
        for w in deck.windows(2) {
            nav.link(w[0], w[1]);
        }
        // each staircase: deck end - top step - bottom step - ground in front of it
        for s in [1.0f32, -1.0] {
            let end = if s > 0.0 { deck[11] } else { deck[0] };
            let top = nav.add([s * 6.15, DECK_Y, DECK_Z]);
            let bottom = nav.add([s * 10.35, 0.2, DECK_Z]);
            nav.link(end, top);
            nav.link(top, bottom);
            let (gi, gj) = nav
                .nearest_cell(s * 11.5, DECK_Z)
                .expect("ground by the stairs");
            nav.link(bottom, gj * n + gi);
        }
        nav
    }

    fn add(&mut self, p: P3) -> usize {
        self.extra.push(p);
        self.n * self.n + self.extra.len() - 1
    }

    fn link(&mut self, a: usize, b: usize) {
        self.links.entry(a).or_default().push(b);
        self.links.entry(b).or_default().push(a);
    }

    fn pos(&self, i: usize, j: usize) -> (f32, f32) {
        (-HALF + 1.0 + i as f32 * STEP, -HALF + 1.0 + j as f32 * STEP)
    }

    fn at(&self, id: usize) -> P3 {
        let g = self.n * self.n;
        if id < g {
            let (x, z) = self.pos(id % self.n, id / self.n);
            [x, 0.0, z]
        } else {
            self.extra[id - g]
        }
    }

    fn cell(&self, x: f32, z: f32) -> (usize, usize) {
        let f = |v: f32| (((v + HALF - 1.0) / STEP).round().max(0.0) as usize).min(self.n - 1);
        (f(x), f(z))
    }

    fn ok(&self, i: isize, j: isize) -> bool {
        i >= 0
            && j >= 0
            && (i as usize) < self.n
            && (j as usize) < self.n
            && self.walk[j as usize * self.n + i as usize]
    }

    /// Nearest walkable ground cell to a point.
    fn nearest_cell(&self, x: f32, z: f32) -> Option<(usize, usize)> {
        let (ci, cj) = self.cell(x, z);
        for r in 0..6isize {
            let mut best: Option<((usize, usize), f32)> = None;
            for dj in -r..=r {
                for di in -r..=r {
                    let (i, j) = (ci as isize + di, cj as isize + dj);
                    if (di.abs() == r || dj.abs() == r) && self.ok(i, j) {
                        let (px, pz) = self.pos(i as usize, j as usize);
                        let d = libm::hypotf(px - x, pz - z);
                        if best.is_none_or(|b| d < b.1) {
                            best = Some(((i as usize, j as usize), d));
                        }
                    }
                }
            }
            if let Some((c, _)) = best {
                return Some(c);
            }
        }
        None
    }

    /// Nearest node: an upper-layer one when the point is up on the walkway or stairs.
    fn nearest(&self, p: P3) -> Option<usize> {
        if p[1] > 1.2 {
            let g = self.n * self.n;
            let up = self
                .extra
                .iter()
                .enumerate()
                .filter(|(_, e)| libm::fabsf(e[1] - p[1]) < 1.8)
                .min_by(|a, b| {
                    let d = |e: &P3| libm::hypotf(e[0] - p[0], e[2] - p[2]);
                    d(a.1).total_cmp(&d(b.1))
                });
            if let Some((k, _)) = up {
                return Some(g + k);
            }
        }
        self.nearest_cell(p[0], p[2]).map(|(i, j)| j * self.n + i)
    }

    /// Can a body walk straight from a to b on the ground?
    pub fn clear(&self, a: (f32, f32), b: (f32, f32)) -> bool {
        let d = libm::hypotf(b.0 - a.0, b.1 - a.1);
        let steps = (d / 0.3).ceil().max(1.0) as usize;
        (0..=steps).all(|k| {
            let t = k as f32 / steps as f32;
            open_at(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
        })
    }

    /// Ground positions within `r` of a point (candidates for cover).
    pub fn cells_near(&self, x: f32, z: f32, r: f32) -> Vec<(f32, f32)> {
        let (ci, cj) = self.cell(x, z);
        let k = (r / STEP).ceil() as isize;
        let mut out = Vec::new();
        for dj in -k..=k {
            for di in -k..=k {
                let (i, j) = (ci as isize + di, cj as isize + dj);
                if self.ok(i, j) {
                    let (px, pz) = self.pos(i as usize, j as usize);
                    if libm::hypotf(px - x, pz - z) <= r {
                        out.push((px, pz));
                    }
                }
            }
        }
        out
    }

    /// A random walkable spot (for wandering), from a seed in [0, 1).
    pub fn random_spot(&self, u: f32, v: f32) -> (f32, f32) {
        let x = -HALF + 2.0 + u * (2.0 * HALF - 4.0);
        let z = -HALF + 2.0 + v * (2.0 * HALF - 4.0);
        self.nearest_cell(x, z)
            .map_or((0.0, 0.0), |(i, j)| self.pos(i, j))
    }

    fn neighbours(&self, id: usize, out: &mut Vec<usize>) {
        out.clear();
        let n = self.n;
        if id < n * n {
            let (ci, cj) = ((id % n) as isize, (id / n) as isize);
            for (di, dj) in [
                (1, 0),
                (-1, 0),
                (0, 1),
                (0, -1),
                (1, 1),
                (1, -1),
                (-1, 1),
                (-1, -1),
            ] {
                let (i, j) = (ci + di, cj + dj);
                if self.ok(i, j)
                    && (di == 0 || dj == 0 || (self.ok(ci + di, cj) && self.ok(ci, cj + dj)))
                {
                    out.push(j as usize * n + i as usize);
                }
            }
        }
        if let Some(l) = self.links.get(&id) {
            out.extend_from_slice(l);
        }
    }

    /// Waypoints (x, z) from `from` to `to`, excluding the start; empty if unreachable.
    pub fn path(&self, from: P3, to: P3) -> Vec<(f32, f32)> {
        let (Some(s), Some(g)) = (self.nearest(from), self.nearest(to)) else {
            return Vec::new();
        };
        let total = self.n * self.n + self.extra.len();
        let goal = self.at(g);
        let mut cost = vec![f32::MAX; total];
        let mut prev = vec![usize::MAX; total];
        let mut open = BinaryHeap::new();
        let mut nb = Vec::with_capacity(10);
        cost[s] = 0.0;
        open.push(Node {
            f: dist(self.at(s), goal),
            c: s,
        });
        while let Some(Node { c, .. }) = open.pop() {
            if c == g {
                break;
            }
            self.neighbours(c, &mut nb);
            let pc = self.at(c);
            for &m in &nb {
                let pm = self.at(m);
                let nc = cost[c] + dist(pc, pm);
                if nc < cost[m] {
                    cost[m] = nc;
                    prev[m] = c;
                    open.push(Node {
                        f: nc + dist(pm, goal),
                        c: m,
                    });
                }
            }
        }
        if cost[g] == f32::MAX {
            return Vec::new();
        }
        let mut ids = vec![g];
        while let Some(&c) = ids.last() {
            if c == s {
                break;
            }
            ids.push(prev[c]);
        }
        ids.reverse();
        let ground = |id: usize| id < self.n * self.n;
        // shortcut runs of ground cells: jump to the farthest point still in a straight line
        let mut out = Vec::new();
        let mut at = (from[0], from[2]);
        let mut at_ground = from[1] < 1.2;
        let mut k = 0;
        while k < ids.len() {
            let mut far = k;
            if at_ground && ground(ids[k]) {
                let mut end = k;
                while end + 1 < ids.len() && ground(ids[end + 1]) && end + 1 < k + 40 {
                    end += 1;
                }
                for m in (k..=end).rev() {
                    let p = self.at(ids[m]);
                    if self.clear(at, (p[0], p[2])) {
                        far = m;
                        break;
                    }
                }
            }
            let p = self.at(ids[far]);
            out.push((p[0], p[2]));
            at = (p[0], p[2]);
            at_ground = ground(ids[far]);
            k = far + 1;
        }
        out.push((to[0], to[2]));
        out
    }
}

#[derive(PartialEq)]
struct Node {
    f: f32,
    c: usize,
}
impl Eq for Node {}
impl Ord for Node {
    fn cmp(&self, o: &Self) -> Ordering {
        o.f.total_cmp(&self.f)
    }
}
impl PartialOrd for Node {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

pub fn nav() -> &'static Nav {
    static N: OnceLock<Nav> = OnceLock::new();
    N.get_or_init(Nav::build)
}

#[cfg(test)]
mod tests {
    use super::*;
    use riftline_sim::constants::TICK_DT;
    use riftline_sim::map;
    use riftline_sim::protocol::InputFrame;
    use riftline_sim::sim::{spawn_state, step_player};

    #[test]
    fn every_spawn_reaches_every_capture_point() {
        for i in 0..map::spawn_count() {
            let (x, z, _) = map::spawn(i);
            for (fx, fz) in map::FLAGS {
                let p = nav().path([x, 0.0, z], [fx, 0.0, fz]);
                assert!(!p.is_empty(), "spawn {i} cannot reach {fx},{fz}");
                let mut at = (x, z);
                for &w in &p[..p.len() - 1] {
                    assert!(nav().clear(at, w), "leg {at:?} -> {w:?}");
                    at = w;
                }
            }
        }
    }

    #[test]
    fn paths_go_around_buildings() {
        let (a, b) = ((-30.0, -22.0), (-14.0, -22.0));
        assert!(!nav().clear(a, b));
        let p = nav().path([a.0, 0.0, a.1], [b.0, 0.0, b.1]);
        assert!(p.len() >= 2, "{p:?}");
    }

    /// Follow a path with the real movement code, like a bot does.
    fn walk(from: P3, to: P3, secs: f32) -> riftline_sim::protocol::PlayerState {
        let mut p = spawn_state(1, 0, true);
        (p.x, p.y, p.z) = (from[0], from[1], from[2]);
        let mut path = nav().path(from, to);
        for _ in 0..(secs / TICK_DT) as u32 {
            while path.len() > 1 && libm::hypotf(path[0].0 - p.x, path[0].1 - p.z) < 0.9 {
                path.remove(0);
            }
            let Some(&(wx, wz)) = path.first() else { break };
            let f = InputFrame {
                move_z: 1.0,
                yaw: libm::atan2f(-(wx - p.x), -(wz - p.z)),
                ..Default::default()
            };
            step_player(&mut p, &f, TICK_DT);
        }
        p
    }

    #[test]
    fn bots_can_climb_to_the_walkway_and_come_back_down() {
        let up = walk([20.0, 0.0, -20.0], [0.0, DECK_Y, DECK_Z], 25.0);
        assert!(
            (up.y - DECK_Y).abs() < 0.05 && up.x.abs() < 1.5,
            "ended at {:?}",
            (up.x, up.y, up.z)
        );
        // down the other staircase to open ground west of it
        let down = walk([up.x, up.y, up.z], [-24.0, 0.0, -10.0], 25.0);
        assert!(
            down.y == 0.0 && libm::hypotf(down.x + 24.0, down.z + 10.0) < 1.5,
            "{:?}",
            (down.x, down.y, down.z)
        );
    }
}
