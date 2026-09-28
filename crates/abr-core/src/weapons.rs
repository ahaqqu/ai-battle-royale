//! Weapon types (PLAN §2.4b): loot-carryable guns that swap the bullet a
//! main fires. The starter "pea" gun keeps every stat config-driven; the
//! pickup guns are fixed tables tuned for play feel (Gungeon-style: each
//! gun has a distinct speed/rhythm the eye can read). All stats flow
//! through `WeaponSpec` so cooldown/speed mods apply uniformly.

use crate::fixed::{self, Fix};
use crate::params::SimParams;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeaponKind {
    /// Starter gun (stats from MatchConfig).
    Pea,
    /// SMG: tiny fast bullets, lots of them, wobbly aim.
    Sprinkler,
    /// Shotgun: a fan of pellets, brutal up close, useless far.
    Scatter,
    /// Sniper: one huge slow-cadence hitscan-feel bolt.
    Lance,
    /// Ricochet gumball: bounces off walls up to N times.
    Bouncer,
    /// Needle: punches through units, keeps flying.
    Skewer,
    /// Pop rock: explodes on impact, splashing nearby enemies.
    Popper,
}

pub const PICKUP_WEAPONS: [WeaponKind; 6] = [
    WeaponKind::Sprinkler,
    WeaponKind::Scatter,
    WeaponKind::Lance,
    WeaponKind::Bouncer,
    WeaponKind::Skewer,
    WeaponKind::Popper,
];

impl WeaponKind {
    /// Stable small integer for compact frames / digests.
    pub fn idx(self) -> u8 {
        match self {
            WeaponKind::Pea => 0,
            WeaponKind::Sprinkler => 1,
            WeaponKind::Scatter => 2,
            WeaponKind::Lance => 3,
            WeaponKind::Bouncer => 4,
            WeaponKind::Skewer => 5,
            WeaponKind::Popper => 6,
        }
    }

    pub fn from_idx(i: u8) -> Self {
        match i {
            1 => WeaponKind::Sprinkler,
            2 => WeaponKind::Scatter,
            3 => WeaponKind::Lance,
            4 => WeaponKind::Bouncer,
            5 => WeaponKind::Skewer,
            6 => WeaponKind::Popper,
            _ => WeaponKind::Pea,
        }
    }
}

/// Fully resolved stats for one shot. `Fix` everywhere; the renderer gets
/// the weapon name via the wire and picks its own visuals.
#[derive(Clone, Copy, Debug)]
pub struct WeaponSpec {
    pub damage: Fix,
    pub speed: Fix,
    pub range: Fix,
    pub cooldown: Fix,
    /// Projectiles per trigger pull.
    pub pellets: u32,
    /// Total fan angle across the pellets (degrees).
    pub spread_deg: i32,
    /// Per-shot random wobble either side (degrees, seeded RNG).
    pub jitter_deg: i32,
    /// Wall ricochets before it dies.
    pub bounces: u8,
    /// Units it can punch through before stopping.
    pub pierce: u8,
    /// Splash radius on impact (0 = no explosion).
    pub splash_radius: Fix,
    /// Splash damage to every enemy caught in the radius.
    pub splash_damage: Fix,
}

#[allow(clippy::too_many_arguments)]
fn s(
    damage: f64,
    speed: f64,
    range: f64,
    cooldown: f64,
    pellets: u32,
    spread_deg: i32,
    jitter_deg: i32,
    bounces: u8,
    pierce: u8,
    splash_radius: f64,
    splash_damage: f64,
) -> WeaponSpec {
    WeaponSpec {
        damage: fixed::from_f64(damage),
        speed: fixed::from_f64(speed),
        range: fixed::from_f64(range),
        cooldown: fixed::from_f64(cooldown),
        pellets,
        spread_deg,
        jitter_deg,
        bounces,
        pierce,
        splash_radius: fixed::from_f64(splash_radius),
        splash_damage: fixed::from_f64(splash_damage),
    }
}

/// Resolve the stats for one unit's weapon. The pea gun reads its stats
/// from `SimParams` (config stays authoritative for the default); pickup
/// guns use the tuned table.
pub fn spec(p: &SimParams, kind: WeaponKind) -> WeaponSpec {
    match kind {
        WeaponKind::Pea => WeaponSpec {
            damage: p.proj_damage,
            speed: p.proj_speed,
            range: p.proj_range,
            cooldown: p.fire_cooldown,
            pellets: 1,
            spread_deg: 0,
            jitter_deg: 0,
            bounces: 0,
            pierce: 0,
            splash_radius: 0.into(),
            splash_damage: 0.into(),
        },
        WeaponKind::Sprinkler => s(5.0, 470.0, 620.0, 0.16, 1, 0, 10, 0, 0, 0.0, 0.0),
        WeaponKind::Scatter => s(7.0, 390.0, 430.0, 0.95, 6, 30, 4, 0, 0, 0.0, 0.0),
        WeaponKind::Lance => s(42.0, 950.0, 1700.0, 1.7, 1, 0, 0, 0, 0, 0.0, 0.0),
        WeaponKind::Bouncer => s(13.0, 430.0, 1100.0, 0.55, 1, 0, 2, 3, 0, 0.0, 0.0),
        WeaponKind::Skewer => s(11.0, 640.0, 1100.0, 0.6, 1, 0, 0, 0, 3, 0.0, 0.0),
        WeaponKind::Popper => s(16.0, 330.0, 850.0, 0.95, 1, 0, 3, 0, 0, 90.0, 10.0),
    }
}
