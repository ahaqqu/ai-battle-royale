//! Reference bots: in-process scripted brains used by the local runner and
//! ladder smoke tests (PLAN §9 M1, §10 "reference bot tiers"). They run
//! against strict-fog observations only — same information diet as remote
//! bots. Bot code may use floats freely; determinism is the sim's contract,
//! not theirs (their actions are logged into the replay anyway).

use crate::fixed::{self, ONE};
use crate::map::GameMap;
use crate::observe::Observation;
use crate::rng::Rng;
use crate::types::{BotInput, MoveInput, UnitAction, UnitInput};

pub trait RefBot: Send {
    fn name(&self) -> &'static str;
    /// Does this brain issue companion commands? (Bots that don't get the
    /// server-side auto-heel AI, PLAN §2.3.)
    fn uses_companion(&self) -> bool {
        false
    }
    fn act(&mut self, obs: &Observation, map: &GameMap) -> BotInput;
}

/// North-clockwise degrees from a→b, in f64 (bot-side convenience).
fn bearing(ax: f64, ay: f64, bx: f64, by: f64) -> u16 {
    let d = (bx - ax).atan2(by - ay).to_degrees();
    (d.rem_euclid(360.0)) as u16
}

fn move_to(ax: f64, ay: f64, bx: f64, by: f64, throttle: f64) -> MoveInput {
    MoveInput {
        dir: bearing(ax, ay, bx, by),
        throttle: fixed::from_f64(throttle.clamp(0.0, 1.0)),
    }
}

fn dist(ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    ((bx - ax).powi(2) + (by - ay).powi(2)).sqrt()
}

fn fire_at(tx: f64, ty: f64) -> UnitAction {
    UnitAction::Fire {
        target: crate::types::Vec2::new(fixed::from_f64(tx), fixed::from_f64(ty)),
    }
}

fn nearest_player(obs: &Observation) -> Option<(f64, f64, f64)> {
    obs.seen
        .players
        .iter()
        .map(|p| (p.pos[0], p.pos[1], p.range))
        .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap())
}

fn lead_target(obs: &Observation, me: (f64, f64)) -> Option<(f64, f64)> {
    obs.seen
        .players
        .iter()
        .filter(|p| p.detail == "full")
        .min_by(|a, b| a.range.partial_cmp(&b.range).unwrap())
        .map(|p| {
            // Lead the shot by ~0.6s of target velocity (projectiles are 420u/s,
            // typical engage range 300-450u).
            let t = (p.range / 420.0).clamp(0.0, 1.2);
            let vx = p.vel.map(|v| v[0]).unwrap_or(0.0);
            let vy = p.vel.map(|v| v[1]).unwrap_or(0.0);
            (p.pos[0] + vx * t, p.pos[1] + vy * t)
        })
        .filter(|(tx, ty)| dist(me.0, me.1, *tx, *ty) < 450.0)
}

fn zone_target(obs: &Observation) -> (f64, f64) {
    let z = &obs.global.zone;
    // Rotate early to the next circle once it's within ~25s of locking.
    let ticks_to_lock = z
        .next
        .as_ref()
        .map(|n| n.locks_at_tick.saturating_sub(obs.tick));
    if let (Some(next), Some(remaining)) = (z.next.as_ref(), ticks_to_lock) {
        if remaining < 250 {
            return (next.center[0], next.center[1]);
        }
    }
    (z.center[0], z.center[1])
}

fn base_input(m: MoveInput, action: Option<UnitAction>) -> BotInput {
    BotInput {
        main: UnitInput { r#move: m, action },
        companion: UnitInput::default(),
        intent: None,
        belief: None,
    }
}

// ---------------------------------------------------------------------------
// Wanderer — the "dumb" tier: drifts around, shoots at what it fully sees.
// ---------------------------------------------------------------------------

pub struct Wanderer {
    rng: Rng,
    dir: u16,
    until: u64,
}

impl Wanderer {
    pub fn new(bot: u32) -> Self {
        Wanderer {
            rng: Rng::new(1000 + bot as u64),
            dir: 0,
            until: 0,
        }
    }
}

impl RefBot for Wanderer {
    fn name(&self) -> &'static str {
        "wanderer"
    }
    fn act(&mut self, obs: &Observation, _map: &GameMap) -> BotInput {
        if obs.tick >= self.until {
            self.dir = self.rng.below(360) as u16;
            self.until = obs.tick + 20 + self.rng.below(30);
        }
        let (mx, my) = (obs.you.main.pos[0], obs.you.main.pos[1]);
        // Dumb, not suicidal: drift to the zone when it gets dangerous.
        let (zx, zy) = (obs.global.zone.center[0], obs.global.zone.center[1]);
        if dist(mx, my, zx, zy) > obs.global.zone.radius * 0.75 {
            self.dir = bearing(mx, my, zx, zy);
            self.until = obs.tick + 10;
        }
        let mut action = lead_target(obs, (mx, my)).map(|(tx, ty)| fire_at(tx, ty));
        let m = MoveInput {
            dir: self.dir,
            throttle: ONE,
        };
        if self.rng.below(100) == 0 {
            action = Some(UnitAction::Dash);
        }
        base_input(m, action)
    }
}

// ---------------------------------------------------------------------------
// Camper — holds its spawn corner, shoots anything that walks in.
// ---------------------------------------------------------------------------

pub struct Camper {
    home: Option<(f64, f64)>,
}

impl Camper {
    pub fn new(_bot: u32) -> Self {
        Camper { home: None }
    }
}

impl RefBot for Camper {
    fn name(&self) -> &'static str {
        "camper"
    }
    fn act(&mut self, obs: &Observation, _map: &GameMap) -> BotInput {
        let (mx, my) = (obs.you.main.pos[0], obs.you.main.pos[1]);
        if self.home.is_none() {
            self.home = Some((mx, my));
        }
        let (hx, hy) = self.home.unwrap();

        // Drift back toward the zone if it left us behind.
        let (zx, zy) = zone_target(obs);
        let want = if dist(mx, my, zx, zy) > obs.global.zone.radius * 0.8 {
            Some((zx, zy))
        } else {
            None
        };
        let target_point = want.unwrap_or((hx, hy));
        let d_home = dist(mx, my, target_point.0, target_point.1);
        let m = if d_home > 60.0 {
            move_to(mx, my, target_point.0, target_point.1, 1.0)
        } else {
            MoveInput::stop()
        };

        let mut action = None;
        if let Some((tx, ty)) = lead_target(obs, (mx, my)) {
            action = Some(UnitAction::Fire {
                target: crate::types::Vec2::new(fixed::from_f64(tx), fixed::from_f64(ty)),
            });
        } else if obs.you.main.hp < 35.0 && obs.you.main.energy > 40.0 {
            action = Some(UnitAction::Shield);
        }

        let mut inp = base_input(m, action);
        inp.companion.action = Some(UnitAction::Sonar);
        inp.intent = Some("holding position".into());
        inp
    }
}

// ---------------------------------------------------------------------------
// Hunter — rotates with the zone, engages anything it sees, scouts with sonar.
// ---------------------------------------------------------------------------

pub struct Hunter;

impl Hunter {
    pub fn new(_bot: u32) -> Self {
        Hunter
    }
}

impl RefBot for Hunter {
    fn name(&self) -> &'static str {
        "hunter"
    }
    fn uses_companion(&self) -> bool {
        true
    }
    fn act(&mut self, obs: &Observation, _map: &GameMap) -> BotInput {
        let (mx, my) = (obs.you.main.pos[0], obs.you.main.pos[1]);
        let (zx, zy) = zone_target(obs);
        let mut m = move_to(mx, my, zx, zy, 1.0);
        let mut action = None;

        if let Some((tx, ty)) = lead_target(obs, (mx, my)) {
            let d = dist(mx, my, tx, ty);
            // Keep a mid-range stand-off.
            if d < 220.0 {
                m = move_to(mx, my, mx + (mx - tx) * 3.0, my + (my - ty) * 3.0, 1.0);
            } else if d > 420.0 {
                m = move_to(mx, my, tx, ty, 1.0);
            } else {
                m = MoveInput::stop();
            }
            action = Some(UnitAction::Fire {
                target: crate::types::Vec2::new(fixed::from_f64(tx), fixed::from_f64(ty)),
            });
            if obs.you.main.hp < 25.0 {
                m = move_to(mx, my, mx + (mx - tx) * 4.0, my + (my - ty) * 4.0, 1.0);
                action = Some(UnitAction::Sprint { on: true });
            }
        }

        // Companion scouts toward zone center.
        let (cx, cy) = (obs.you.companion.pos[0], obs.you.companion.pos[1]);
        let mut inp = base_input(m, action);
        inp.companion.r#move = move_to(cx, cy, zx, zy, 1.0);
        if obs.you.companion.cooldown.sonar.unwrap_or(99.0) <= 0.0 {
            inp.companion.action = Some(UnitAction::Sonar);
        }
        inp.intent = Some("rotating, hunting".into());
        inp
    }
}

// ---------------------------------------------------------------------------
// Looter — grabs pickups, avoids fights, sonars ahead.
// ---------------------------------------------------------------------------

pub struct Looter;

impl Looter {
    pub fn new(_bot: u32) -> Self {
        Looter
    }
}

impl RefBot for Looter {
    fn name(&self) -> &'static str {
        "looter"
    }
    fn uses_companion(&self) -> bool {
        true
    }
    fn act(&mut self, obs: &Observation, _map: &GameMap) -> BotInput {
        let (mx, my) = (obs.you.main.pos[0], obs.you.main.pos[1]);
        let mut goal = zone_target(obs);

        if let Some(pk) = obs.seen.pickups.iter().min_by(|a, b| {
            let da = dist(mx, my, a.pos[0], a.pos[1]);
            let db = dist(mx, my, b.pos[0], b.pos[1]);
            da.partial_cmp(&db).unwrap()
        }) {
            goal = (pk.pos[0], pk.pos[1]);
        }

        // Flee from full-detail enemies inside 300u.
        let mut m = move_to(mx, my, goal.0, goal.1, 1.0);
        if let Some((ex, ey, er)) = nearest_player(obs) {
            if er < 320.0 {
                m = move_to(mx, my, mx + (mx - ex) * 3.0, my + (my - ey) * 3.0, 1.0);
            }
        }

        let action = lead_target(obs, (mx, my)).map(|(tx, ty)| fire_at(tx, ty));
        let (cx, cy) = (obs.you.companion.pos[0], obs.you.companion.pos[1]);
        let mut inp = base_input(m, action);
        if obs.you.companion.cooldown.sonar.unwrap_or(99.0) <= 0.0 {
            inp.companion.action = Some(UnitAction::Sonar);
        } else {
            inp.companion.r#move = move_to(cx, cy, goal.0, goal.1, 1.0);
        }
        inp.intent = Some("looting".into());
        inp
    }
}

// ---------------------------------------------------------------------------
// Survivor — zone discipline first, distance keeping, defensive shield.
// ---------------------------------------------------------------------------

pub struct Survivor {
    last_hp: f64,
}

impl Survivor {
    pub fn new(_bot: u32) -> Self {
        Survivor { last_hp: 100.0 }
    }
}

impl RefBot for Survivor {
    fn name(&self) -> &'static str {
        "survivor"
    }
    fn uses_companion(&self) -> bool {
        true
    }
    fn act(&mut self, obs: &Observation, _map: &GameMap) -> BotInput {
        let (mx, my) = (obs.you.main.pos[0], obs.you.main.pos[1]);
        let (zx, zy) = zone_target(obs);
        let mut m = move_to(mx, my, zx, zy, 1.0);
        let mut action = None;

        // Sprint home when outside the circle.
        let zd = dist(mx, my, obs.global.zone.center[0], obs.global.zone.center[1]);
        let outside = zd > obs.global.zone.radius;
        if outside {
            m = move_to(mx, my, zx, zy, 1.0);
            action = Some(UnitAction::Sprint { on: true });
        }

        // Keep enemies at 450+; shoot only point-blank.
        if let Some((ex, ey, er)) = nearest_player(obs) {
            if er < 450.0 {
                m = move_to(mx, my, mx + (mx - ex) * 4.0, my + (my - ey) * 4.0, 1.0);
            }
            if er < 240.0 {
                if let Some((tx, ty)) = lead_target(obs, (mx, my)) {
                    action = Some(UnitAction::Fire {
                        target: crate::types::Vec2::new(fixed::from_f64(tx), fixed::from_f64(ty)),
                    });
                }
            }
        }
        // Shield on damage taken.
        let hp = obs.you.main.hp;
        if self.last_hp - hp > 1.0 && obs.you.main.energy > 40.0 {
            action = Some(UnitAction::Shield);
        }
        self.last_hp = hp;

        let (cx, cy) = (obs.you.companion.pos[0], obs.you.companion.pos[1]);
        let mut inp = base_input(m, action);
        if obs.you.companion.cooldown.sonar.unwrap_or(99.0) <= 0.0 {
            inp.companion.action = Some(UnitAction::Sonar);
        } else {
            inp.companion.r#move = move_to(cx, cy, zx, zy, 1.0);
        }
        inp.intent = Some(
            if outside {
                "sprinting to zone!"
            } else {
                "rotating safe"
            }
            .into(),
        );
        inp
    }
}

// ---------------------------------------------------------------------------
// Berserker — sprints at faces. Dumb aggression, fun to watch, dies young.
// ---------------------------------------------------------------------------

pub struct Berserker {
    sprinting: bool,
}

impl Berserker {
    pub fn new(_bot: u32) -> Self {
        Berserker { sprinting: true }
    }
}

impl RefBot for Berserker {
    fn name(&self) -> &'static str {
        "berserker"
    }
    fn act(&mut self, obs: &Observation, _map: &GameMap) -> BotInput {
        let (mx, my) = (obs.you.main.pos[0], obs.you.main.pos[1]);
        let target = nearest_player(obs)
            .map(|(x, y, _)| (x, y))
            .unwrap_or_else(|| zone_target(obs));
        let mut m = move_to(mx, my, target.0, target.1, 1.0);
        let d = dist(mx, my, target.0, target.1);
        let mut action = None;

        if let Some((tx, ty)) = lead_target(obs, (mx, my)) {
            action = Some(UnitAction::Fire {
                target: crate::types::Vec2::new(fixed::from_f64(tx), fixed::from_f64(ty)),
            });
            if d < 150.0 {
                // Dash through them.
                m = move_to(mx, my, tx, ty, 1.0);
                if obs.you.main.energy > 30.0 {
                    action = Some(UnitAction::Dash);
                }
            }
        }
        if obs.you.main.hp < 20.0 {
            self.sprinting = false;
        }
        if self.sprinting != obs.you.main.status.contains(&"sprint") {
            action = Some(UnitAction::Sprint { on: self.sprinting });
        }
        let mut inp = base_input(m, action);
        inp.intent = Some("BLOOD FOR THE LADDER".into());
        inp
    }
}

/// Registry: name → constructor. Repeated names get distinct instances.
pub fn create(name: &str, bot: u32) -> Option<Box<dyn RefBot>> {
    match name {
        "wanderer" => Some(Box::new(Wanderer::new(bot))),
        "camper" => Some(Box::new(Camper::new(bot))),
        "hunter" => Some(Box::new(Hunter::new(bot))),
        "looter" => Some(Box::new(Looter::new(bot))),
        "survivor" => Some(Box::new(Survivor::new(bot))),
        "berserker" => Some(Box::new(Berserker::new(bot))),
        _ => None,
    }
}

pub const BOT_NAMES: &[&str] = &[
    "wanderer",
    "camper",
    "hunter",
    "looter",
    "survivor",
    "berserker",
];

/// default16: the 16-entrant ladder smoke lineup.
pub fn default16() -> Vec<String> {
    let mut v = Vec::new();
    for i in 0..16 {
        v.push(BOT_NAMES[i % BOT_NAMES.len()].to_string());
    }
    v
}
