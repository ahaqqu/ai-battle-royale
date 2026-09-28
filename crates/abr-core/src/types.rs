//! Core value types: 2D vectors in Fix, entity ids, wire actions.

use crate::fixed::{self, Fix, ONE};
use serde::{Deserialize, Serialize};

pub const ARENA: Fix = 3200 * ONE;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vec2 {
    pub x: Fix,
    pub y: Fix,
}

#[allow(clippy::should_implement_trait, clippy::len_without_is_empty)]
impl Vec2 {
    #[inline]
    pub fn new(x: Fix, y: Fix) -> Self {
        Vec2 { x, y }
    }

    #[inline]
    pub fn splat(v: Fix) -> Self {
        Vec2 { x: v, y: v }
    }

    #[inline]
    pub fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }

    #[inline]
    pub fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }

    #[inline]
    pub fn scale(self, s: Fix) -> Vec2 {
        Vec2::new(fixed::mul(self.x, s), fixed::mul(self.y, s))
    }

    #[inline]
    pub fn dot(self, o: Vec2) -> Fix {
        fixed::mul(self.x, o.x) + fixed::mul(self.y, o.y)
    }

    #[inline]
    pub fn len2(self) -> Fix {
        fixed::mul(self.x, self.x) + fixed::mul(self.y, self.y)
    }

    #[inline]
    pub fn len(self) -> Fix {
        fixed::sqrt(self.len2())
    }

    #[inline]
    pub fn dist2(self, o: Vec2) -> Fix {
        self.sub(o).len2()
    }

    #[inline]
    pub fn dist(self, o: Vec2) -> Fix {
        self.sub(o).len()
    }

    /// Bearing from `self` to `o` in whole degrees (0 = north/+Y, clockwise).
    #[inline]
    pub fn bearing_to(self, o: Vec2) -> i32 {
        let d = o.sub(self);
        fixed::atan2_deg(d.y, d.x)
    }

    /// Unit vector pointing along `deg` degrees (0 = north, clockwise).
    #[inline]
    pub fn dir(deg: i32) -> Vec2 {
        Vec2::new(fixed::sin_deg(deg), fixed::cos_deg(deg))
    }

    #[inline]
    pub fn clamp_to(self, lo: Fix, hi: Fix) -> Vec2 {
        Vec2::new(self.x.clamp(lo, hi), self.y.clamp(lo, hi))
    }
}

/// Bot indices are 0..N. Entity ids are stable per match:
/// main of bot *b* has id `1 + b`, its companion `101 + b`.
pub fn main_id(bot: u32) -> u32 {
    1 + bot
}
pub fn companion_id(bot: u32) -> u32 {
    101 + bot
}
pub fn owner_of_main(id: u32) -> u32 {
    id - 1
}
pub fn owner_of_companion(id: u32) -> u32 {
    id - 101
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnitKind {
    Main,
    Companion,
    /// Slain-the-Boss raid boss: the main unit of the last entrant. Fires,
    /// dashes and shields like a main but with boss stats and its cannon;
    /// it never picks up loot and ignores the zone.
    Boss,
}

/// What a bot sends for one unit for one tick (PLAN §4.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MoveInput {
    /// degrees, 0 = north, clockwise
    pub dir: u16,
    /// 0..=1 of max speed
    pub throttle: Fix,
}

impl MoveInput {
    pub fn stop() -> Self {
        MoveInput {
            dir: 0,
            throttle: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UnitAction {
    Fire { target: Vec2 },
    Dash,
    Shield,
    Sprint { on: bool },
    Sonar,
    Heel,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UnitInput {
    #[serde(rename = "move")]
    pub r#move: MoveInput,
    /// None = no action this tick (movement alone is always legal).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<UnitAction>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BotInput {
    #[serde(default)]
    pub main: UnitInput,
    #[serde(default)]
    pub companion: UnitInput,
    /// Spectator shout (≤64 chars), never parsed by the engine (PLAN §6.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    /// Mind-cam debug channel: 64×64 belief heat map, write-only to viewers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub belief: Option<Vec<u8>>,
}
