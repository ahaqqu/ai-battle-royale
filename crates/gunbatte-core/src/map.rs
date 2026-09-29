//! Static map: public knowledge by design (PLAN §2.4). Fog covers only
//! dynamic things. Walls block vision + projectiles + movement; low cover
//! blocks projectiles + movement but not vision.

use crate::fixed::{self, Fix, ONE};
use crate::types::Vec2;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WallKind {
    /// Blocks vision, projectiles and movement.
    Wall,
    /// Low cover: blocks projectiles and movement; vision passes over.
    Cover,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Wall {
    pub min: Vec2,
    pub max: Vec2,
    pub kind: WallKind,
}

impl Wall {
    fn blocks_vision(&self) -> bool {
        self.kind == WallKind::Wall
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GameMap {
    pub id: String,
    pub size: Fix,
    pub walls: Vec<Wall>,
    pub spawns: Vec<Vec2>,
}

fn wall(min: (f64, f64), max: (f64, f64), kind: WallKind) -> Wall {
    Wall {
        min: Vec2::new(fixed::from_f64(min.0), fixed::from_f64(min.1)),
        max: Vec2::new(fixed::from_f64(max.0), fixed::from_f64(max.1)),
        kind,
    }
}

/// arena-1: 3200×3200. Center plaza with four pillar-bunkers, four long
/// mid-lane walls, loot-rich corner bunkers, low cover scattered on the
/// approach lanes. 180°-symmetric for fairness, asymmetric enough for a meta.
/// One push per wall so the layout mirrors the map sketch.
#[allow(clippy::vec_init_then_push)]
pub fn arena1() -> GameMap {
    let mut walls: Vec<Wall> = Vec::new();
    // Center plaza pillars.
    walls.push(wall((1350., 1350.), (1500., 1500.), WallKind::Wall));
    walls.push(wall((1700., 1350.), (1850., 1500.), WallKind::Wall));
    walls.push(wall((1350., 1700.), (1500., 1850.), WallKind::Wall));
    walls.push(wall((1700., 1700.), (1850., 1850.), WallKind::Wall));
    // Mid-lane long walls.
    walls.push(wall((700., 1300.), (900., 1900.), WallKind::Wall));
    walls.push(wall((2300., 1300.), (2500., 1900.), WallKind::Wall));
    walls.push(wall((1300., 700.), (1900., 900.), WallKind::Wall));
    walls.push(wall((1300., 2300.), (1900., 2500.), WallKind::Wall));
    // Corner bunkers (loot-rich), L-shaped.
    walls.push(wall((500., 500.), (750., 560.), WallKind::Wall));
    walls.push(wall((500., 500.), (560., 750.), WallKind::Wall));
    walls.push(wall((2450., 500.), (2700., 560.), WallKind::Wall));
    walls.push(wall((2640., 500.), (2700., 750.), WallKind::Wall));
    walls.push(wall((500., 2640.), (750., 2700.), WallKind::Wall));
    walls.push(wall((500., 2450.), (560., 2700.), WallKind::Wall));
    walls.push(wall((2450., 2640.), (2700., 2700.), WallKind::Wall));
    walls.push(wall((2640., 2450.), (2700., 2700.), WallKind::Wall));
    // Low cover on the four center approaches.
    walls.push(wall((1100., 1550.), (1350., 1650.), WallKind::Cover));
    walls.push(wall((1850., 1550.), (2100., 1650.), WallKind::Cover));
    walls.push(wall((1550., 1100.), (1650., 1350.), WallKind::Cover));
    walls.push(wall((1550., 1850.), (1650., 2100.), WallKind::Cover));
    // Mid-field low cover.
    walls.push(wall((1000., 1000.), (1250., 1060.), WallKind::Cover));
    walls.push(wall((1950., 1000.), (2200., 1060.), WallKind::Cover));
    walls.push(wall((1000., 2140.), (1250., 2200.), WallKind::Cover));
    walls.push(wall((1950., 2140.), (2200., 2200.), WallKind::Cover));

    let spawns = [
        (1600., 150.),
        (2325., 344.),
        (2856., 875.),
        (3050., 1600.),
        (2856., 2325.),
        (2325., 2856.),
        (1600., 3050.),
        (875., 2856.),
        (344., 2325.),
        (150., 1600.),
        (344., 875.),
        (875., 344.),
        (900., 900.),
        (2300., 900.),
        (900., 2300.),
        (2300., 2300.),
    ]
    .into_iter()
    .map(|(x, y)| Vec2::new(fixed::from_f64(x), fixed::from_f64(y)))
    .collect();

    GameMap {
        id: "arena-1".into(),
        size: 3200 * ONE,
        walls,
        spawns,
    }
}

impl GameMap {
    /// Wire format: f64 arrays for coordinates (game state stays Fix).
    pub fn to_wire(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "size": fixed::to_f64(self.size),
            "walls": self.walls.iter().map(|w| serde_json::json!({
                "min": [fixed::to_f64(w.min.x), fixed::to_f64(w.min.y)],
                "max": [fixed::to_f64(w.max.x), fixed::to_f64(w.max.y)],
                "kind": w.kind,
            })).collect::<Vec<_>>(),
            "spawns": self.spawns.iter().map(|p| [fixed::to_f64(p.x), fixed::to_f64(p.y)]).collect::<Vec<_>>(),
        })
    }
}

/// Load a map by id (embedded layouts for v1).
pub fn load_map(id: &str) -> Option<GameMap> {
    match id {
        "arena-1" => Some(arena1()),
        _ => None,
    }
}

/// Segment vs AABB (slab method). Returns Some(t) for the earliest entry
/// hit with t in [0, 1], plus the axis (0 = x, 1 = y) of the entry slab.
/// `a`,`b` are segment endpoints.
pub fn segment_aabb_axis(a: Vec2, b: Vec2, min: Vec2, max: Vec2) -> Option<(Fix, usize)> {
    let d = b.sub(a);
    let mut t_near = fixed::from_int(0);
    let mut t_far = ONE;
    let mut axis_hit: Option<usize> = None;
    for axis in 0..2 {
        let (p, dd, lo, hi) = match axis {
            0 => (a.x, d.x, min.x, max.x),
            _ => (a.y, d.y, min.y, max.y),
        };
        if dd == 0 {
            if p < lo || p > hi {
                return None;
            }
        } else {
            let inv = fixed::div(ONE, dd); // 1/d
            let mut t0 = fixed::mul(lo - p, inv);
            let mut t1 = fixed::mul(hi - p, inv);
            if t0 > t1 {
                core::mem::swap(&mut t0, &mut t1);
            }
            if t0 > t_near || axis_hit.is_none() {
                t_near = t_near.max(t0);
                if t0 == t_near {
                    axis_hit = Some(axis);
                }
            }
            t_far = t_far.min(t1);
            if t_near > t_far {
                return None;
            }
        }
    }
    if t_near <= ONE && t_far >= 0 {
        // Ray started inside the slab box on the winning axis: fall back to
        // the shallower direction so a reflect normal still exists.
        let axis = axis_hit.unwrap_or(if d.x != 0 { 0 } else { 1 });
        Some((t_near, axis))
    } else {
        None
    }
}

/// Segment vs AABB (slab method). Returns Some(t) for the earliest entry
/// hit with t in [0, 1], None otherwise. `a`,`b` are segment endpoints.
pub fn segment_aabb(a: Vec2, b: Vec2, min: Vec2, max: Vec2) -> Option<Fix> {
    segment_aabb_axis(a, b, min, max).map(|(t, _)| t)
}

/// Line of sight between two points: blocked by full walls only.
pub fn los_blocked(map: &GameMap, a: Vec2, b: Vec2) -> bool {
    for w in &map.walls {
        if w.blocks_vision() && segment_aabb(a, b, w.min, w.max).is_some() {
            return true;
        }
    }
    false
}

/// Earliest wall (any kind) hit along segment a→b, as t in [0,1].
pub fn projectile_wall_hit(map: &GameMap, a: Vec2, b: Vec2) -> Option<Fix> {
    let mut best: Option<Fix> = None;
    for w in &map.walls {
        if let Some(t) = segment_aabb(a, b, w.min, w.max) {
            best = Some(match best {
                Some(bt) if bt <= t => bt,
                _ => t,
            });
        }
    }
    best
}

/// Earliest wall hit along a→b, plus the hit surface normal (axis-aligned
/// unit, pointing back toward the ray origin) — used by ricocheting bullets.
pub fn projectile_wall_hit_n(map: &GameMap, a: Vec2, b: Vec2) -> Option<(Fix, Fix, Fix)> {
    let mut best: Option<(Fix, usize)> = None;
    for w in &map.walls {
        if let Some((t, axis)) = segment_aabb_axis(a, b, w.min, w.max) {
            if best.is_none() || t < best.unwrap().0 {
                best = Some((t, axis));
            }
        }
    }
    let (t, axis) = best?;
    let d = if axis == 0 { b.x - a.x } else { b.y - a.y };
    let sign: Fix = if d > 0 { -1 } else { 1 };
    Some(if axis == 0 { (t, sign, 0) } else { (t, 0, sign) })
}

/// True if a circle of `radius` at `p` overlaps any wall (for spawn/loot placement).
pub fn circle_blocked(map: &GameMap, p: Vec2, radius: Fix) -> bool {
    for w in &map.walls {
        let cx = p.x.clamp(w.min.x, w.max.x);
        let cy = p.y.clamp(w.min.y, w.max.y);
        let d2 = Vec2::new(cx, cy).dist2(p);
        if d2 < fixed::mul(radius, radius) {
            return true;
        }
    }
    false
}

/// Push a circle out of walls and clamp to arena bounds. Deterministic:
/// walls resolved in map order, two passes.
pub fn resolve_circle(map: &GameMap, pos: &mut Vec2, radius: Fix) {
    for _ in 0..2 {
        for w in &map.walls {
            let cx = pos.x.clamp(w.min.x, w.max.x);
            let cy = pos.y.clamp(w.min.y, w.max.y);
            let close = Vec2::new(cx, cy);
            let d2 = close.dist2(*pos);
            let r2 = fixed::mul(radius, radius);
            if d2 < r2 {
                if d2 == 0 {
                    // Center inside the wall: push out along the shallowest axis.
                    let left = pos.x - w.min.x;
                    let right = w.max.x - pos.x;
                    let up = pos.y - w.min.y;
                    let down = w.max.y - pos.y;
                    let m = left.min(right).min(up).min(down);
                    if m == up {
                        pos.y = w.min.y - radius;
                    } else if m == down {
                        pos.y = w.max.y + radius;
                    } else if m == left {
                        pos.x = w.min.x - radius;
                    } else {
                        pos.x = w.max.x + radius;
                    }
                } else {
                    let d = fixed::sqrt(d2);
                    let push = radius - d;
                    let n = close.sub(*pos);
                    pos.x -= fixed::mul(fixed::div(n.x, d), push);
                    pos.y -= fixed::mul(fixed::div(n.y, d), push);
                }
            }
        }
    }
    // Arena bounds.
    let r = radius;
    pos.x = pos.x.clamp(r, map.size - r);
    pos.y = pos.y.clamp(r, map.size - r);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed::from_int;

    #[test]
    fn los_blocked_by_center_pillar() {
        let m = arena1();
        // Straight line across the NW center pillar.
        let a = Vec2::new(from_int(1300), from_int(1300));
        let b = Vec2::new(from_int(1600), from_int(1600));
        assert!(los_blocked(&m, a, b));
        // Around it: not blocked.
        let c = Vec2::new(from_int(1000), from_int(1600));
        let d = Vec2::new(from_int(1200), from_int(1600));
        assert!(!los_blocked(&m, c, d));
    }

    #[test]
    fn cover_does_not_block_vision() {
        let m = arena1();
        // Through the west low cover strip.
        let a = Vec2::new(from_int(1050), from_int(1600));
        let b = Vec2::new(from_int(1400), from_int(1600));
        assert!(!los_blocked(&m, a, b));
        assert!(projectile_wall_hit(&m, a, b).is_some());
    }

    #[test]
    fn segment_aabb_earliest_hit() {
        let t = segment_aabb(
            Vec2::new(from_int(0), from_int(100)),
            Vec2::new(from_int(200), from_int(100)),
            Vec2::new(from_int(50), from_int(0)),
            Vec2::new(from_int(150), from_int(200)),
        );
        assert!(t.is_some());
        assert!((fixed::to_f64(t.unwrap()) - 0.25).abs() < 0.01);
    }

    #[test]
    fn spawns_are_clear() {
        let m = arena1();
        for s in &m.spawns {
            assert!(
                !circle_blocked(&m, *s, from_int(30)),
                "spawn blocked at {:?}",
                fixed::to_f64(s.x)
            );
        }
    }

    #[test]
    fn resolve_pushes_out() {
        let m = arena1();
        let mut p = Vec2::new(from_int(1420), from_int(1450)); // inside NW pillar
        resolve_circle(&m, &mut p, from_int(14));
        assert!(!circle_blocked(&m, p, from_int(13)));
    }
}
