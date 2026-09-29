//! Per-tick spectator events. Purely descriptive — the renderer derives all
//! juice (particles, shake, hitstop) from these; none of it can desync a
//! match (PLAN §7.3).

use crate::fixed;
use crate::loot::PickupKind;
use crate::types::Vec2;
use serde::{Deserialize, Serialize};

/// Wire format for positions: f64 arrays (game state stays Fix internally).
pub fn pa(v: Vec2) -> [f64; 2] {
    [fixed::to_f64(v.x), fixed::to_f64(v.y)]
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Shot {
        bot: u32,
        unit_id: u32,
        from: [f64; 2],
        dir: u16,
        /// Which gun fired (renderer picks the muzzle flash).
        weapon: crate::weapons::WeaponKind,
    },
    /// A Bouncer bullet ricocheted off a wall.
    Bounce {
        id: u32,
        at: [f64; 2],
    },
    /// A Popper bullet detonated; splash damage already applied via Hit events.
    Explosion {
        bot: u32,
        at: [f64; 2],
        radius: f32,
    },
    /// Projectile impact on a unit.
    Hit {
        unit_id: u32,
        bot: u32,
        at: [f64; 2],
        damage: f32,
        hp_after: f32,
    },
    /// Projectile died on a wall / range fizzle.
    ProjectileEnd {
        id: u32,
        at: [f64; 2],
        wall: bool,
    },
    Death {
        bot: u32,
        unit_id: u32,
        at: [f64; 2],
        killer: Option<u32>,
    },
    CompanionDown {
        bot: u32,
        unit_id: u32,
        at: [f64; 2],
    },
    CompanionBack {
        bot: u32,
        unit_id: u32,
        at: [f64; 2],
    },
    Pickup {
        bot: u32,
        pickup_id: u32,
        kind: PickupKind,
        at: [f64; 2],
    },
    Dash {
        bot: u32,
        unit_id: u32,
        at: [f64; 2],
        dir: u16,
    },
    Shield {
        bot: u32,
        unit_id: u32,
        at: [f64; 2],
    },
    /// A zone shrink started (the 10s warning).
    ZoneShrinkStarted {
        phase: usize,
        center: [f64; 2],
        radius: f32,
    },
    ZoneLocked {
        phase: usize,
    },
    MatchEnded {
        winner: Option<u32>,
    },
}
