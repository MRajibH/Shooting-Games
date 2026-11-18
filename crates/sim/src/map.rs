//! The map: static axis-aligned boxes shared by server collision and client rendering.
//! An industrial yard, 64 x 64 m, with buildings, shipping containers, a raised
//! steel walkway with stairs, concrete barriers, crates and a vaultable low wall.

use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapBox {
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub mat: u8,
}

pub mod mat {
    pub const CONCRETE: u8 = 0;
    pub const BRICK: u8 = 1;
    pub const PLASTER: u8 = 2;
    pub const WOOD: u8 = 3;
    pub const CONTAINER_RED: u8 = 4;
    pub const CONTAINER_BLUE: u8 = 5;
    pub const STEEL: u8 = 6;
}

/// Playable area is [-HALF, HALF] on x and z.
pub const HALF: f32 = 32.0;

/// Box from its centre on x/z, its base height and its size.
fn bx(x: f32, y: f32, z: f32, sx: f32, sy: f32, sz: f32, mat: u8) -> MapBox {
    MapBox {
        min: [x - sx / 2.0, y, z - sz / 2.0],
        max: [x + sx / 2.0, y + sy, z + sz / 2.0],
        mat,
    }
}

fn build() -> Vec<MapBox> {
    use mat::*;
    let mut v = Vec::new();
    let span = HALF * 2.0 + 2.0;
    // perimeter walls
    v.push(bx(0.0, 0.0, -HALF - 0.5, span, 5.0, 1.0, CONCRETE));
    v.push(bx(0.0, 0.0, HALF + 0.5, span, 5.0, 1.0, CONCRETE));
    v.push(bx(-HALF - 0.5, 0.0, 0.0, 1.0, 5.0, span, CONCRETE));
    v.push(bx(HALF + 0.5, 0.0, 0.0, 1.0, 5.0, span, CONCRETE));
    // buildings
    v.push(bx(-22.0, 0.0, -22.0, 14.0, 7.0, 14.0, BRICK));
    v.push(bx(22.0, 0.0, 22.0, 14.0, 9.0, 14.0, PLASTER));
    // raised walkway (top at 3.0 m) on pillars, railings on its long sides
    v.push(bx(0.0, 2.8, -14.0, 12.0, 0.2, 3.0, STEEL));
    for (x, z) in [(-5.6, -15.2), (5.6, -15.2), (-5.6, -12.8), (5.6, -12.8)] {
        v.push(bx(x, 0.0, z, 0.3, 2.8, 0.3, STEEL));
    }
    v.push(bx(0.0, 3.0, -15.46, 12.0, 1.0, 0.08, STEEL));
    v.push(bx(0.0, 3.0, -12.54, 12.0, 1.0, 0.08, STEEL));
    // two flights of concrete stairs, 15 steps of 0.2 m rise / 0.3 m run
    for k in 1..=15 {
        let top = 0.2 * k as f32;
        let cx = 6.0 + (15 - k) as f32 * 0.3 + 0.15;
        v.push(bx(cx, 0.0, -14.0, 0.3, top, 1.4, CONCRETE));
        v.push(bx(-cx, 0.0, -14.0, 0.3, top, 1.4, CONCRETE));
    }
    // 20 ft shipping containers (6.06 x 2.59 x 2.44 m), one stacked
    v.push(bx(12.0, 0.0, -2.0, 6.06, 2.59, 2.44, CONTAINER_RED));
    v.push(bx(12.0, 2.59, -2.0, 6.06, 2.59, 2.44, CONTAINER_BLUE));
    v.push(bx(-12.0, 0.0, 6.0, 2.44, 2.59, 6.06, CONTAINER_BLUE));
    v.push(bx(4.0, 0.0, 14.0, 6.06, 2.59, 2.44, CONTAINER_RED));
    v.push(bx(-20.0, 0.0, 14.0, 2.44, 2.59, 6.06, CONTAINER_RED));
    v.push(bx(24.0, 0.0, -8.0, 2.44, 2.59, 6.06, CONTAINER_BLUE));
    // concrete barriers (1 m, vaultable)
    v.push(bx(0.0, 0.0, 4.0, 3.0, 1.0, 0.6, CONCRETE));
    v.push(bx(-6.0, 0.0, -4.0, 0.6, 1.0, 3.0, CONCRETE));
    v.push(bx(8.0, 0.0, 8.0, 3.0, 1.0, 0.6, CONCRETE));
    v.push(bx(-16.0, 0.0, 22.0, 3.0, 1.0, 0.6, CONCRETE));
    v.push(bx(18.0, 0.0, -18.0, 0.6, 1.0, 3.0, CONCRETE));
    // wooden crates (1.2 m), one stacked
    for (x, y, z) in [
        (14.0, 0.0, 6.0),
        (15.3, 0.0, 6.0),
        (14.65, 1.2, 6.0),
        (-8.0, 0.0, -10.0),
        (-20.0, 0.0, 0.0),
        (20.0, 0.0, 4.0),
        (-2.0, 0.0, 22.0),
    ] {
        v.push(bx(x, y, z, 1.2, 1.2, 1.2, WOOD));
    }
    // low wall to vault
    v.push(bx(-14.0, 0.0, -8.0, 6.0, 0.9, 0.3, CONCRETE));
    v
}

pub fn boxes() -> &'static [MapBox] {
    static B: OnceLock<Vec<MapBox>> = OnceLock::new();
    B.get_or_init(build)
}

const SPAWNS: [(f32, f32); 8] = [
    (-26.0, 26.0),
    (26.0, -26.0),
    (0.0, 27.0),
    (0.0, -27.0),
    (27.0, 0.0),
    (-27.0, 0.0),
    (-8.0, 14.0),
    (8.0, -24.0),
];

/// Spawn point `i` (wraps): position and a yaw facing the centre of the map.
pub fn spawn(i: usize) -> (f32, f32, f32) {
    let (x, z) = SPAWNS[i % SPAWNS.len()];
    (x, z, libm::atan2f(x, z))
}

pub fn spawn_count() -> usize {
    SPAWNS.len()
}

/// Which team's half a spawn point is on (team 1 south-west, team 2 north-east).
pub fn spawn_side(i: usize) -> u8 {
    let (x, z) = SPAWNS[i % SPAWNS.len()];
    if x - z < 0.0 {
        1
    } else {
        2
    }
}

/// Domination capture points A, B, C: one near each team's half and one in the middle.
pub const FLAGS: [(f32, f32); 3] = [(-17.0, 12.0), (0.0, 0.0), (17.0, -12.0)];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_points_are_open_ground() {
        for (x, z) in FLAGS {
            for b in boxes() {
                let inside = x > b.min[0] - 1.0
                    && x < b.max[0] + 1.0
                    && z > b.min[2] - 1.0
                    && z < b.max[2] + 1.0;
                assert!(!inside || b.min[1] > 2.0, "flag at {x},{z} overlaps {b:?}");
            }
        }
    }

    #[test]
    fn each_team_has_four_spawns() {
        let ones = (0..spawn_count()).filter(|&i| spawn_side(i) == 1).count();
        assert_eq!(ones, 4);
    }
}
