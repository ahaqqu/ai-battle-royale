//! Per-tick spectator events. Purely descriptive — the renderer derives all
//! juice (particles, shake, hitstop) from these; none of it can desync a
//! match (PLAN §7.3).

use crate::loot::PickupKind;
use crate::types::Vec2;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Shot {
        bot: u32,
        unit_id: u32,
        from: Vec2,
        dir: u16,
    },
    /// Projectile impact on a unit.
    Hit {
        unit_id: u32,
        bot: u32,
        at: Vec2,
        damage: f32,
        hp_after: f32,
    },
    /// Projectile died on a wall / range fizzle.
    ProjectileEnd {
        id: u32,
        at: Vec2,
        wall: bool,
    },
    Death {
        bot: u32,
        unit_id: u32,
        at: Vec2,
        killer: Option<u32>,
    },
    CompanionDown {
        bot: u32,
        unit_id: u32,
        at: Vec2,
    },
    CompanionBack {
        bot: u32,
        unit_id: u32,
        at: Vec2,
    },
    Pickup {
        bot: u32,
        pickup_id: u32,
        kind: PickupKind,
        at: Vec2,
    },
    Sonar {
        bot: u32,
        unit_id: u32,
        at: Vec2,
    },
    Dash {
        bot: u32,
        unit_id: u32,
        at: Vec2,
        dir: u16,
    },
    Shield {
        bot: u32,
        unit_id: u32,
        at: Vec2,
    },
    /// A zone shrink started (the 10s warning).
    ZoneShrinkStarted {
        phase: usize,
        center: Vec2,
        radius: f32,
    },
    ZoneLocked {
        phase: usize,
    },
    MatchEnded {
        winner: Option<u32>,
    },
}
