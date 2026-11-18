//! Ground navigation for bots: a walkable grid built from the shared map, A* paths,
//! and straight-line shortcuts so paths do not zig-zag.

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::OnceLock;

use riftline_sim::constants::body::STAND_HEIGHT;
use riftline_sim::map::HALF;
use riftline_sim::sim::blocked;

const STEP: f32 = 1.0;
/// Extra clearance from walls, so bots do not scrape along them.
const MARGIN: f32 = 0.25;

pub struct Nav {
    n: usize,
    walk: Vec<bool>,
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

impl Nav {
    fn build() -> Nav {
        let n = ((2.0 * (HALF - 1.0)) / STEP) as usize + 1;
        let mut nav = Nav {
            n,
            walk: vec![false; n * n],
        };
        for j in 0..n {
            for i in 0..n {
                let (x, z) = nav.pos(i, j);
                nav.walk[j * n + i] = open_at(x, z);
            }
        }
        nav
    }

    fn pos(&self, i: usize, j: usize) -> (f32, f32) {
        (-HALF + 1.0 + i as f32 * STEP, -HALF + 1.0 + j as f32 * STEP)
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

    /// Nearest walkable cell to a point.
    fn nearest(&self, x: f32, z: f32) -> Option<(usize, usize)> {
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

    /// Can a body walk straight from a to b on the ground?
    pub fn clear(&self, a: (f32, f32), b: (f32, f32)) -> bool {
        let d = libm::hypotf(b.0 - a.0, b.1 - a.1);
        let steps = (d / 0.3).ceil().max(1.0) as usize;
        (0..=steps).all(|k| {
            let t = k as f32 / steps as f32;
            open_at(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
        })
    }

    /// Waypoints from `from` to `to` (excluding the start), or empty if unreachable.
    pub fn path(&self, from: (f32, f32), to: (f32, f32)) -> Vec<(f32, f32)> {
        let (Some(s), Some(g)) = (self.nearest(from.0, from.1), self.nearest(to.0, to.1)) else {
            return Vec::new();
        };
        let n = self.n;
        let idx = |c: (usize, usize)| c.1 * n + c.0;
        let h = |c: (usize, usize)| {
            let (dx, dz) = (
                (c.0 as f32 - g.0 as f32).abs(),
                (c.1 as f32 - g.1 as f32).abs(),
            );
            dx.max(dz) + (std::f32::consts::SQRT_2 - 1.0) * dx.min(dz)
        };
        let mut cost = vec![f32::MAX; n * n];
        let mut prev = vec![usize::MAX; n * n];
        let mut open = BinaryHeap::new();
        cost[idx(s)] = 0.0;
        open.push(Node { f: h(s), c: idx(s) });
        while let Some(Node { c, .. }) = open.pop() {
            if c == idx(g) {
                break;
            }
            let (ci, cj) = ((c % n) as isize, (c / n) as isize);
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
                if !self.ok(i, j)
                    || (di != 0 && dj != 0 && !(self.ok(ci + di, cj) && self.ok(ci, cj + dj)))
                {
                    continue;
                }
                let nc = j as usize * n + i as usize;
                let step = if di != 0 && dj != 0 {
                    std::f32::consts::SQRT_2
                } else {
                    1.0
                };
                let nc_cost = cost[c] + step;
                if nc_cost < cost[nc] {
                    cost[nc] = nc_cost;
                    prev[nc] = c;
                    open.push(Node {
                        f: nc_cost + h((i as usize, j as usize)),
                        c: nc,
                    });
                }
            }
        }
        if cost[idx(g)] == f32::MAX {
            return Vec::new();
        }
        let mut cells = vec![idx(g)];
        while let Some(&c) = cells.last() {
            if c == idx(s) {
                break;
            }
            cells.push(prev[c]);
        }
        cells.reverse();
        let pts: Vec<(f32, f32)> = cells.iter().map(|&c| self.pos(c % n, c / n)).collect();
        // shortcut: from each kept point, jump to the farthest point still in a straight line
        let mut out = Vec::new();
        let mut at = from;
        let mut k = 0;
        while k < pts.len() {
            let mut far = k;
            for m in (k..pts.len().min(k + 40)).rev() {
                if self.clear(at, pts[m]) {
                    far = m;
                    break;
                }
            }
            out.push(pts[far]);
            at = pts[far];
            k = far + 1;
        }
        out.push(to);
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
    use riftline_sim::map;

    #[test]
    fn every_spawn_reaches_every_capture_point() {
        for i in 0..map::spawn_count() {
            let (x, z, _) = map::spawn(i);
            for f in map::FLAGS {
                let p = nav().path((x, z), f);
                assert!(!p.is_empty(), "spawn {i} cannot reach {f:?}");
                // every leg is walkable in a straight line
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
        // straight through the brick building is blocked
        let (a, b) = ((-30.0, -22.0), (-14.0, -22.0));
        assert!(!nav().clear(a, b));
        let p = nav().path(a, b);
        assert!(p.len() >= 2, "{p:?}");
    }
}
