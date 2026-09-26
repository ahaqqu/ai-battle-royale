//! Loot schedule derived from the hidden seed (PLAN §2.4) — never sent to
//! bots; scouting must have value (PLAN §3.3).

use crate::fixed::{self};
use crate::map::GameMap;
use crate::params::SimParams;
use crate::rng::Rng;
use crate::types::Vec2;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PickupKind {
    HpKit,
    Energy,
    ModCooldown,
    ModSpeed,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Pickup {
    pub id: u32,
    pub kind: PickupKind,
    pub pos: Vec2,
    pub spawn_tick: u64,
    pub taken: bool,
}

pub fn generate(p: &SimParams, map: &GameMap, rng: &mut Rng) -> Vec<Pickup> {
    let total_w = (p.loot_weight_hp + p.loot_weight_energy + p.loot_weight_mod) as u64;
    let mut out = Vec::new();
    for id in 1..=p.loot_count {
        let kind = if total_w == 0 {
            PickupKind::HpKit
        } else {
            let roll = rng.below(total_w);
            if roll < p.loot_weight_hp as u64 {
                PickupKind::HpKit
            } else if roll < (p.loot_weight_hp + p.loot_weight_energy) as u64 {
                PickupKind::Energy
            } else if rng.below(2) == 0 {
                PickupKind::ModCooldown
            } else {
                PickupKind::ModSpeed
            }
        };
        // Deterministic placement with bounded retries. Sample in a margin
        // band inside the arena, reject wall overlaps.
        let margin = fixed::from_int(80);
        let span = p.arena - margin * 2;
        let mut pos = Vec2::splat(p.arena / 2);
        for _ in 0..100 {
            let cand = Vec2::new(
                margin + fixed::mul(rng.unit(), span),
                margin + fixed::mul(rng.unit(), span),
            );
            pos = cand;
            if !crate::map::circle_blocked(map, cand, fixed::from_int(30)) {
                break;
            }
        }
        let spawn_tick = rng.below(p.loot_window_ticks.max(1));
        out.push(Pickup {
            id,
            kind,
            pos,
            spawn_tick,
            taken: false,
        });
    }
    // Sort by spawn_tick for cache-friendly activation; stable order = deterministic.
    out.sort_by_key(|pk| (pk.spawn_tick, pk.id));
    out
}
