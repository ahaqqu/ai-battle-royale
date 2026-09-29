//! The authoritative full world state (PLAN §3.1). Fog lives only in
//! `observe()`; this struct is what replays regenerate and what spectators
//! see. Fixed-point + sorted iteration everywhere.

use crate::fixed::{self, Fix, ONE};
use crate::loot::Pickup;
use crate::params::SimParams;
use crate::rng::Rng;
use crate::types::{UnitKind, Vec2};
use crate::zone::{self, ZonePhase};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Unit {
    pub id: u32,
    pub bot: u32,
    pub kind: UnitKind,
    pub pos: Vec2,
    pub vel: Vec2,
    /// Whole degrees, 0 = north, clockwise.
    pub facing: u16,
    pub hp: Fix,
    pub energy: Fix,
    /// Seconds remaining (Fix).
    pub fire_cd: Fix,
    /// Ticks remaining on dash / shield.
    pub dashing: u64,
    pub shielding: u64,
    pub dash_dir: Vec2,
    pub sprint: bool,
    pub alive: bool,
    /// Companion only: tick at which it respawns.
    pub respawn_at: Option<u64>,
    pub kills: u32,
    pub placement: Option<u32>,
    /// Bot index of the last damager (kill credit).
    pub last_damager: Option<u32>,
    /// Tick of the last damage taken (kill-credit recency window).
    pub last_damager_tick: u64,
    /// Weapon mods (PLAN §2.4): negative fraction / positive fraction.
    pub mod_cooldown_pct: Fix,
    pub mod_speed_pct: Fix,
    /// The gun this main fires (swapped by weapon pickups).
    pub weapon: crate::weapons::WeaponKind,
}

impl Unit {
    pub fn radius(&self, p: &SimParams) -> Fix {
        match self.kind {
            UnitKind::Main => p.main_radius,
            UnitKind::Companion => p.comp_radius,
            UnitKind::Boss => p.boss_radius,
        }
    }
    pub fn max_hp(&self, p: &SimParams) -> Fix {
        match self.kind {
            UnitKind::Main => p.main_hp,
            UnitKind::Companion => p.comp_hp,
            UnitKind::Boss => p.boss_hp,
        }
    }
    pub fn max_energy(&self, p: &SimParams) -> Fix {
        match self.kind {
            UnitKind::Main => p.energy_max,
            UnitKind::Companion => p.comp_energy_max,
            UnitKind::Boss => p.boss_energy_max,
        }
    }
    /// Ground speed per tick — the boss is slower than the raiders (kiting
    /// is the counterplay).
    pub fn base_speed(&self, p: &SimParams) -> Fix {
        match self.kind {
            UnitKind::Main => p.main_speed,
            UnitKind::Companion => p.comp_speed,
            UnitKind::Boss => p.boss_speed,
        }
    }
    /// Mains fire, dash, shield and count for eliminations; the boss does
    /// everything a main does except pick up loot.
    pub fn is_main(&self) -> bool {
        self.kind == UnitKind::Main
    }
    pub fn is_boss(&self) -> bool {
        self.kind == UnitKind::Boss
    }
    /// A "combatant main": mains plus the boss (death = elimination).
    pub fn is_combatant(&self) -> bool {
        self.kind != UnitKind::Companion
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Projectile {
    pub id: u32,
    pub bot: u32,
    pub unit_id: u32,
    pub pos: Vec2,
    pub vel: Vec2,
    pub damage: Fix,
    /// Range remaining before it fizzles.
    pub remaining: Fix,
    /// Which gun fired it (drives bounce/pierce/splash + rendering).
    pub weapon: crate::weapons::WeaponKind,
    /// Ricochets left (Bouncer).
    pub bounces: u8,
    /// Units it may still punch through (Skewer).
    pub pierce_left: u8,
    /// Units already damaged on this flight (prevents double-pierce hits).
    pub hits: [u32; 4],
    pub hits_n: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundKind {
    Gunshot,
    Dash,
    Footstep,
}

/// An audio emission during the current tick; filtered per-bot by `observe`.
#[derive(Clone, Copy, Debug)]
pub struct SoundEvent {
    pub kind: SoundKind,
    pub pos: Vec2,
    /// Emitting bot (never reported back to the emitter itself).
    pub bot: u32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct KillEntry {
    pub tick: u64,
    /// None = killed by the zone.
    pub killer: Option<u32>,
    pub victim: u32,
}

#[derive(Clone, Debug)]
pub struct WorldState {
    pub tick: u64,
    pub bots: u32,
    pub units: Vec<Unit>,
    pub projectiles: Vec<Projectile>,
    pub pickups: Vec<Pickup>,
    pub zone_phases: Vec<ZonePhase>,
    pub kill_feed: Vec<KillEntry>,
    /// Sounds emitted during the current tick (cleared each step).
    pub sounds: Vec<SoundEvent>,
    pub next_projectile_id: u32,
    pub finished: bool,
    pub winner: Option<u32>,
    /// Final placement per bot index (index = rank-1 → bot).
    pub placements: Vec<u32>,
    pub rng: Rng,
    /// Total damage dealt per bot (secondary ladder metric).
    pub damage_dealt: Vec<u64>,
}

impl WorldState {
    /// Fresh match state: bots spawn at distinct seeded spawn points,
    /// mains + companions side by side, zone + loot schedules generated
    /// from the seed.
    pub fn new(p: &SimParams, map: &crate::map::GameMap, bots: u32, seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        // Seeded shuffle of spawn points, then take the first `bots`.
        let mut spawns: Vec<Vec2> = map.spawns.clone();
        for i in (1..spawns.len()).rev() {
            let j = rng.below((i + 1) as u64) as usize;
            spawns.swap(i, j);
        }
        let mut units = Vec::with_capacity((bots * 2) as usize);
        for b in 0..bots {
            let base = spawns[b as usize % spawns.len()];
            let main = Unit {
                id: crate::types::main_id(b),
                bot: b,
                kind: UnitKind::Main,
                pos: base,
                vel: Vec2::default(),
                facing: 0,
                hp: p.main_hp,
                energy: p.energy_max,
                fire_cd: 0.into(),
                dashing: 0,
                shielding: 0,
                dash_dir: Vec2::default(),
                sprint: false,
                alive: true,
                respawn_at: None,
                kills: 0,
                placement: None,
                last_damager: None,
                last_damager_tick: 0,
                mod_cooldown_pct: 0.into(),
                mod_speed_pct: 0.into(),
                weapon: crate::weapons::WeaponKind::Pea,
            };
            let companion = Unit {
                id: crate::types::companion_id(b),
                bot: b,
                kind: UnitKind::Companion,
                pos: base.add(Vec2::new(fixed::from_int(30), fixed::from_int(0))),
                vel: Vec2::default(),
                facing: 0,
                hp: p.comp_hp,
                energy: p.comp_energy_max,
                fire_cd: 0.into(),
                dashing: 0,
                shielding: 0,
                dash_dir: Vec2::default(),
                sprint: false,
                alive: true,
                respawn_at: None,
                kills: 0,
                placement: None,
                last_damager: None,
                last_damager_tick: 0,
                mod_cooldown_pct: 0.into(),
                mod_speed_pct: 0.into(),
                weapon: crate::weapons::WeaponKind::Pea,
            };
            units.push(main);
            units.push(companion);
        }
        let zone_phases = zone::generate_schedule(p, &mut rng);
        let pickups = crate::loot::generate(p, map, &mut rng);
        let mut state = WorldState {
            tick: 0,
            bots,
            units,
            projectiles: Vec::new(),
            pickups,
            zone_phases,
            kill_feed: Vec::new(),
            sounds: Vec::new(),
            next_projectile_id: 1,
            finished: false,
            winner: None,
            placements: Vec::new(),
            rng,
            damage_dealt: vec![0; bots as usize],
        };
        // Slain the Boss: the last entrant is the raid boss. Its main unit
        // becomes `UnitKind::Boss` — boss stats, boss cannon — while its
        // companion stays the respawning minion.
        if p.mode == crate::config::GameMode::Boss {
            let boss_bot = state.boss_bot();
            let boss = state.main_mut(boss_bot);
            boss.kind = UnitKind::Boss;
            boss.hp = p.boss_hp;
            boss.energy = p.boss_energy_max;
            boss.weapon = crate::weapons::WeaponKind::BossCannon;
        }
        state
    }

    /// The entrant index whose main is the raid boss (boss mode only).
    #[inline]
    pub fn boss_bot(&self) -> u32 {
        self.bots - 1
    }

    /// True when the two entrants are on the same side. Royale: never
    /// (everyone is hostile). Boss mode: every non-boss entrant raids
    /// together — teammates cannot hurt each other.
    #[inline]
    pub fn same_team(&self, a: u32, b: u32, mode: crate::config::GameMode) -> bool {
        if a == b {
            return true;
        }
        mode == crate::config::GameMode::Boss && a != self.boss_bot() && b != self.boss_bot()
    }

    #[inline]
    pub fn main(&self, bot: u32) -> &Unit {
        &self.units[(bot * 2) as usize]
    }
    #[inline]
    pub fn main_mut(&mut self, bot: u32) -> &mut Unit {
        &mut self.units[(bot * 2) as usize]
    }
    #[inline]
    pub fn companion(&self, bot: u32) -> &Unit {
        &self.units[(bot * 2 + 1) as usize]
    }
    #[inline]
    pub fn companion_mut(&mut self, bot: u32) -> &mut Unit {
        &mut self.units[(bot * 2 + 1) as usize]
    }

    pub fn alive_mains(&self) -> u32 {
        self.units.iter().filter(|u| u.is_main() && u.alive).count() as u32
    }

    pub fn unit_by_id(&self, id: u32) -> Option<&Unit> {
        if (1..=self.bots).contains(&id) {
            Some(&self.units[(id - 1) as usize])
        } else if (101..101 + self.bots).contains(&id) {
            Some(&self.units[(id - 101 + self.bots) as usize])
        } else {
            None
        }
    }

    /// Deterministic FNV-1a digest over the full state in canonical order —
    /// the byte-identical re-simulation check (PLAN §9 M1 accept).
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        fn put(h: &mut u64, bytes: &[u8]) {
            for &b in bytes {
                *h ^= b as u64;
                *h = h.wrapping_mul(0x100000001b3);
            }
        }
        fn put_i64(h: &mut u64, v: i64) {
            put(h, &v.to_le_bytes());
        }
        fn put_u64(h: &mut u64, v: u64) {
            put(h, &v.to_le_bytes());
        }
        fn put_vec2(h: &mut u64, v: Vec2) {
            put_i64(h, v.x);
            put_i64(h, v.y);
        }

        put_u64(&mut h, self.tick);
        put_u64(&mut h, self.bots as u64);
        put_u64(&mut h, self.next_projectile_id as u64);
        put(&mut h, &[self.finished as u8]);
        put_u64(&mut h, self.winner.map(|b| b as u64 + 1).unwrap_or(0));
        for b in &self.placements {
            put_u64(&mut h, *b as u64);
        }
        for u in &self.units {
            put_u64(&mut h, u.id as u64);
            put_i64(&mut h, u.pos.x);
            put_i64(&mut h, u.pos.y);
            put_i64(&mut h, u.vel.x);
            put_i64(&mut h, u.vel.y);
            put_u64(&mut h, u.facing as u64);
            put_i64(&mut h, u.hp);
            put_i64(&mut h, u.energy);
            put_i64(&mut h, u.fire_cd);
            put_u64(&mut h, u.dashing);
            put_u64(&mut h, u.shielding);
            put_vec2(&mut h, u.dash_dir);
            put(&mut h, &[u.sprint as u8, u.alive as u8]);
            put_u64(&mut h, u.respawn_at.unwrap_or(u64::MAX));
            put_u64(&mut h, u.kills as u64);
            put_u64(&mut h, u.placement.unwrap_or(0) as u64);
            put_u64(&mut h, u.last_damager.map(|b| b as u64 + 1).unwrap_or(0));
            put_i64(&mut h, u.mod_cooldown_pct);
            put_i64(&mut h, u.mod_speed_pct);
            put(&mut h, &[u.weapon.idx()]);
        }
        // Projectiles in id order.
        let mut projs: Vec<&Projectile> = self.projectiles.iter().collect();
        projs.sort_by_key(|p| p.id);
        put_u64(&mut h, projs.len() as u64);
        for pr in projs {
            put_u64(&mut h, pr.id as u64);
            put_vec2(&mut h, pr.pos);
            put_vec2(&mut h, pr.vel);
            put_i64(&mut h, pr.damage);
            put_i64(&mut h, pr.remaining);
            put(&mut h, &[pr.weapon.idx(), pr.bounces, pr.pierce_left, pr.hits_n]);
            for i in 0..pr.hits.len() {
                put_u64(&mut h, pr.hits[i] as u64);
            }
        }
        for pk in &self.pickups {
            put(&mut h, &[pk.kind.idx(), pk.taken as u8]);
        }
        for z in &self.zone_phases {
            put_vec2(&mut h, z.center);
            put_i64(&mut h, z.radius);
            put_u64(&mut h, z.lock_tick);
            put_u64(&mut h, z.shrink_start_tick);
            put_i64(&mut h, z.damage);
        }
        put_u64(&mut h, self.kill_feed.len() as u64);
        for k in &self.kill_feed {
            put_u64(&mut h, k.tick);
            put_u64(&mut h, k.killer.map(|b| b as u64 + 1).unwrap_or(0));
            put_u64(&mut h, k.victim as u64);
        }
        put_u64(&mut h, self.rng.s);
        for d in &self.damage_dealt {
            put_u64(&mut h, *d);
        }
        h
    }
}

pub type ProjectileRef<'a> = &'a Projectile;

/// Damage actually applied after shield reduction (helper shared by step).
#[inline]
pub fn apply_damage(target: &mut Unit, raw: Fix, shield_reduction: Fix) -> Fix {
    if target.shielding > 0 {
        let reduced = raw - fixed::mul(raw, shield_reduction);
        target.hp -= reduced;
        reduced
    } else {
        target.hp -= raw;
        raw
    }
}

/// Helper: seconds → Fix seconds.
pub fn secs(v: f64) -> Fix {
    fixed::from_f64(v)
}

/// Fix seconds → ticks.
pub fn secs_to_ticks(v: Fix, tick_rate: u64) -> u64 {
    fixed::mul(v, fixed::from_int(tick_rate as i64)) as u64
}

pub const ONE_FIX: Fix = ONE;
