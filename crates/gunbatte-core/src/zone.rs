//! Zone schedule: shrinking circle phases, next zone published one phase
//! ahead (PLAN §2.5). All randomness comes from the match seed.

use crate::fixed::{self, Fix};
use crate::params::SimParams;
use crate::rng::Rng;
use crate::types::Vec2;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ZonePhase {
    pub center: Vec2,
    pub radius: Fix,
    pub lock_tick: u64,
    pub shrink_start_tick: u64,
    /// HP/s outside the circle while this phase is current.
    pub damage: Fix,
}

/// Generate the phase list. Phase 0 is the initial circle (active from tick
/// 0). Phase k>0 shrinks from phase k-1's circle starting at
/// `shrink_start_tick` and locks at `lock_tick`.
pub fn generate_schedule(p: &SimParams, rng: &mut Rng) -> Vec<ZonePhase> {
    let n = p.zone_radii.len();
    let mut phases: Vec<ZonePhase> = Vec::with_capacity(n);
    let center0 = Vec2::splat(p.arena / 2);
    phases.push(ZonePhase {
        center: center0,
        radius: p.zone_radii[0],
        lock_tick: 0,
        shrink_start_tick: 0,
        damage: phase_damage(p, 0),
    });
    for k in 1..n {
        let hold = p.zone_hold_min + rng.below((p.zone_hold_max - p.zone_hold_min).max(1) + 1);
        let prev = phases[k - 1];
        let lock_tick = prev.lock_tick + hold;
        // Phase 1 locks after the initial hold; shrink begins `shrink_ticks`
        // before the lock (that window IS the 10s warning).
        let shrink_start_tick = lock_tick.saturating_sub(p.zone_shrink_ticks);
        let max_off = fixed::mul(prev.radius - p.zone_radii[k], fixed::from_f64(0.9));
        let ang = (rng.below(360)) as i32;
        let mag = fixed::mul(fixed::sqrt(rng.unit()), max_off);
        let off = Vec2::dir(ang).scale(mag);
        phases.push(ZonePhase {
            center: prev.center.add(off),
            radius: p.zone_radii[k],
            lock_tick,
            shrink_start_tick,
            damage: phase_damage(p, k),
        });
    }
    phases
}

fn phase_damage(p: &SimParams, k: usize) -> Fix {
    p.zone_damage
        .get(k)
        .copied()
        .unwrap_or_else(|| *p.zone_damage.last().unwrap_or(&fixed::from_int(2)))
}

/// The current circle at `tick`: phase k where lock_tick(k) <= tick < lock(k+1);
/// while inside [shrink_start(k+1), lock(k+1)) interpolate k → k+1.
#[derive(Clone, Copy, Debug)]
pub struct ZoneNow {
    pub center: Vec2,
    pub radius: Fix,
    pub phase: usize,
    pub damage: Fix, // per second, phase of the *target* during shrink
}

pub fn zone_at(phases: &[ZonePhase], tick: u64) -> ZoneNow {
    // Find current phase: last phase with lock_tick <= tick.
    let mut k = 0;
    for (i, ph) in phases.iter().enumerate() {
        if ph.lock_tick <= tick {
            k = i;
        }
    }
    if k + 1 >= phases.len() {
        let ph = phases[k];
        return ZoneNow {
            center: ph.center,
            radius: ph.radius,
            phase: k,
            damage: ph.damage,
        };
    }
    let nxt = phases[k + 1];
    if tick >= nxt.shrink_start_tick {
        let span = (nxt.lock_tick - nxt.shrink_start_tick).max(1);
        let t = fixed::div(
            fixed::from_int((tick - nxt.shrink_start_tick) as i64),
            fixed::from_int(span as i64),
        );
        let cur = phases[k];
        ZoneNow {
            center: lerp_center(cur.center, nxt.center, t),
            radius: cur.radius + fixed::mul(nxt.radius - cur.radius, t),
            phase: k,
            damage: nxt.damage,
        }
    } else {
        let ph = phases[k];
        ZoneNow {
            center: ph.center,
            radius: ph.radius,
            phase: k,
            damage: ph.damage,
        }
    }
}

fn lerp_center(a: Vec2, b: Vec2, t: Fix) -> Vec2 {
    Vec2::new(
        a.x + fixed::mul(b.x - a.x, t),
        a.y + fixed::mul(b.y - a.y, t),
    )
}

/// The next phase after the one currently locking/locked, if any.
pub fn next_phase(phases: &[ZonePhase], tick: u64) -> Option<&ZonePhase> {
    let now = zone_at(phases, tick);
    phases.get(now.phase + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MatchConfig;
    use crate::fixed::to_f64;

    #[test]
    fn schedule_is_well_formed() {
        let p = SimParams::from_config(&MatchConfig::default());
        let mut rng = Rng::new(1);
        let z = generate_schedule(&p, &mut rng);
        assert_eq!(z.len(), 6);
        for k in 1..z.len() {
            // Next circle fits inside previous circle.
            let off = z[k].center.dist(z[k - 1].center);
            assert!(off + z[k].radius <= z[k - 1].radius + fixed::from_int(2));
            // Shrink starts before lock, after previous lock.
            assert!(z[k].shrink_start_tick < z[k].lock_tick);
            assert!(z[k].shrink_start_tick >= z[k - 1].lock_tick);
        }
        // Locks are strictly increasing.
        for w in z.windows(2) {
            assert!(w[0].lock_tick < w[1].lock_tick);
        }
    }

    #[test]
    fn interpolation_monotonic() {
        let p = SimParams::from_config(&MatchConfig::default());
        let mut rng = Rng::new(3);
        let z = generate_schedule(&p, &mut rng);
        let lock1 = z[1].lock_tick;
        let r_before = zone_at(&z, lock1 - 20).radius;
        let r_mid = zone_at(&z, lock1 - 5).radius;
        let r_lock = zone_at(&z, lock1).radius;
        assert!(to_f64(r_before) > to_f64(r_mid));
        assert!((to_f64(r_lock) - to_f64(p.zone_radii[1])).abs() < 0.01);
    }

    #[test]
    fn zone_starts_full() {
        let p = SimParams::from_config(&MatchConfig::default());
        let mut rng = Rng::new(9);
        let z = generate_schedule(&p, &mut rng);
        let now = zone_at(&z, 0);
        assert_eq!(now.radius, p.zone_radii[0]);
        assert_eq!(now.center, Vec2::splat(p.arena / 2));
    }
}
