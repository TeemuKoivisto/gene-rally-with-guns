//! Coarse navigation grid + A* for cop pathfinding (design §9).
//!
//! The arena is small and its buildings are static, so a 2 m cell grid built
//! once at startup is plenty. Cells inside an inflated building AABB are
//! blocked; A* runs on 8-connected cells (no corner cutting), and callers
//! smooth the path with line-of-sight checks.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use bevy::prelude::*;

use crate::arena::{ARENA_HALF_X, ARENA_HALF_Z, BLOCKS};

const CELL: f32 = 2.0;
/// Obstacle inflation: keeps car-sized bodies clear of building corners.
const INFLATE: f32 = 1.3;
/// Line-of-sight sampling step, world units.
const LOS_STEP: f32 = 0.5;

pub struct NavPlugin;

impl Plugin for NavPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(NavGrid::build());
    }
}

#[derive(Resource)]
pub struct NavGrid {
    nx: i32,
    nz: i32,
    blocked: Vec<bool>,
}

impl NavGrid {
    fn build() -> Self {
        let nx = (ARENA_HALF_X * 2.0 / CELL).round() as i32;
        let nz = (ARENA_HALF_Z * 2.0 / CELL).round() as i32;
        let mut grid = Self {
            nx,
            nz,
            blocked: vec![false; (nx * nz) as usize],
        };
        for iz in 0..nz {
            for ix in 0..nx {
                let p = grid.pos_of(ix, iz);
                let blocked = BLOCKS.iter().any(|(center, dim)| {
                    (p.x - center.x).abs() <= dim.x / 2.0 + INFLATE
                        && (p.z - center.z).abs() <= dim.z / 2.0 + INFLATE
                });
                let i = grid.idx(ix, iz);
                grid.blocked[i] = blocked;
            }
        }
        grid
    }

    fn idx(&self, ix: i32, iz: i32) -> usize {
        (iz * self.nx + ix) as usize
    }

    fn in_bounds(&self, ix: i32, iz: i32) -> bool {
        ix >= 0 && ix < self.nx && iz >= 0 && iz < self.nz
    }

    fn is_free(&self, ix: i32, iz: i32) -> bool {
        self.in_bounds(ix, iz) && !self.blocked[self.idx(ix, iz)]
    }

    fn cell_of(&self, pos: Vec3) -> (i32, i32) {
        let ix = ((pos.x + ARENA_HALF_X) / CELL) as i32;
        let iz = ((pos.z + ARENA_HALF_Z) / CELL) as i32;
        (ix.clamp(0, self.nx - 1), iz.clamp(0, self.nz - 1))
    }

    fn pos_of(&self, ix: i32, iz: i32) -> Vec3 {
        Vec3::new(
            -ARENA_HALF_X + (ix as f32 + 0.5) * CELL,
            0.0,
            -ARENA_HALF_Z + (iz as f32 + 0.5) * CELL,
        )
    }

    /// Nearest free cell to `cell`, spiraling outward (targets can hug walls).
    fn nearest_free(&self, (cx, cz): (i32, i32)) -> Option<(i32, i32)> {
        if self.is_free(cx, cz) {
            return Some((cx, cz));
        }
        for radius in 1i32..=4 {
            for dz in -radius..=radius {
                for dx in -radius..=radius {
                    if dx.abs() != radius && dz.abs() != radius {
                        continue; // ring only
                    }
                    if self.is_free(cx + dx, cz + dz) {
                        return Some((cx + dx, cz + dz));
                    }
                }
            }
        }
        None
    }

    /// True if the straight segment stays on free cells (both endpoints included).
    pub fn line_of_sight(&self, from: Vec3, to: Vec3) -> bool {
        let delta = (to - from) * Vec3::new(1.0, 0.0, 1.0);
        let len = delta.length();
        let steps = (len / LOS_STEP).ceil() as i32;
        for step in 0..=steps {
            let p = from + delta * (step as f32 / steps.max(1) as f32);
            let (ix, iz) = self.cell_of(p);
            if !self.is_free(ix, iz) {
                return false;
            }
        }
        true
    }

    /// A* from `from` to `to`; returns world-space waypoints (goal cell last).
    /// 8-connected, diagonals only when both adjacent cardinals are free.
    pub fn find_path(&self, from: Vec3, to: Vec3) -> Option<Vec<Vec3>> {
        let start = self.nearest_free(self.cell_of(from))?;
        let goal = self.nearest_free(self.cell_of(to))?;
        if start == goal {
            return Some(vec![self.pos_of(goal.0, goal.1)]);
        }

        let n = (self.nx * self.nz) as usize;
        let mut g_cost = vec![i32::MAX; n];
        let mut parent = vec![usize::MAX; n];
        let mut open = BinaryHeap::new();

        let heuristic = |ix: i32, iz: i32| {
            let dx = (ix - goal.0).abs();
            let dz = (iz - goal.1).abs();
            // Octile distance in tenths.
            14 * dx.min(dz) + 10 * (dx - dz).abs()
        };

        let start_idx = self.idx(start.0, start.1);
        g_cost[start_idx] = 0;
        open.push(Reverse((heuristic(start.0, start.1), start_idx)));

        const DIRS: [(i32, i32, i32); 8] = [
            (1, 0, 10),
            (-1, 0, 10),
            (0, 1, 10),
            (0, -1, 10),
            (1, 1, 14),
            (1, -1, 14),
            (-1, 1, 14),
            (-1, -1, 14),
        ];

        let goal_idx = self.idx(goal.0, goal.1);
        while let Some(Reverse((_, current))) = open.pop() {
            if current == goal_idx {
                // Reconstruct: goal back to (excluded) start.
                let mut cells = Vec::new();
                let mut walk = current;
                while walk != start_idx {
                    cells.push(walk);
                    walk = parent[walk];
                }
                return Some(
                    cells
                        .iter()
                        .rev()
                        .map(|&i| self.pos_of(i as i32 % self.nx, i as i32 / self.nx))
                        .collect(),
                );
            }
            let (cx, cz) = (current as i32 % self.nx, current as i32 / self.nx);
            for (dx, dz, step_cost) in DIRS {
                let (nx_, nz_) = (cx + dx, cz + dz);
                if !self.is_free(nx_, nz_) {
                    continue;
                }
                // No cutting corners diagonally past a blocked cell.
                if dx != 0 && dz != 0 && !(self.is_free(cx + dx, cz) && self.is_free(cx, cz + dz)) {
                    continue;
                }
                let neighbor = self.idx(nx_, nz_);
                let tentative = g_cost[current].saturating_add(step_cost);
                if tentative < g_cost[neighbor] {
                    g_cost[neighbor] = tentative;
                    parent[neighbor] = current;
                    open.push(Reverse((tentative + heuristic(nx_, nz_), neighbor)));
                }
            }
        }
        None
    }
}
