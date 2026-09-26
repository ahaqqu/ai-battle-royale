//! Match configuration — every number in PLAN §11 as a serde-serializable
//! knob. Stored as f64 on the wire, converted to Fix once at engine
//! construction (deterministic conversion, see `fixed::from_f64`).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct MatchConfig {
    pub apiversion: u32,
    pub map_id: String,
    pub tick_rate_hz: u32,
    /// Bot reply deadline in ms (PLAN §4.2).
    pub deadline_ms: u64,
    /// Hard cap on match length in seconds (safety net; the zone ends matches).
    pub match_max_s: u64,
    pub main: MainConfig,
    pub companion: CompanionConfig,
    pub audio: AudioConfig,
    pub zone: ZoneConfig,
    pub loot: LootConfig,
    /// Bots that never send companion commands get this server-side AI.
    pub auto_heel: bool,
    /// Timeout ladder (PLAN §4.3).
    pub timeouts: TimeoutConfig,
}

impl MatchConfig {
    /// The PLAN §2/§11 starting values.
    pub fn standard() -> Self {
        MatchConfig {
            apiversion: 1,
            map_id: "arena-1".into(),
            tick_rate_hz: 10,
            deadline_ms: 50,
            match_max_s: 600,
            main: MainConfig::default(),
            companion: CompanionConfig::default(),
            audio: AudioConfig::default(),
            zone: ZoneConfig::default(),
            loot: LootConfig::default(),
            auto_heel: true,
            timeouts: TimeoutConfig::default(),
        }
    }
}

impl Default for MatchConfig {
    fn default() -> Self {
        Self::standard()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct MainConfig {
    pub radius: f64,
    pub hp: f64,
    pub speed: f64,
    pub fire_cooldown_s: f64,
    pub projectile_speed: f64,
    pub projectile_damage: f64,
    pub projectile_range: f64,
    pub vision: f64,
    /// Inside this distance from the sensing unit: full detail (PLAN §3.2).
    pub full_detail_range: f64,
    pub dash: DashConfig,
    pub shield: ShieldConfig,
    pub sprint: SprintConfig,
    pub energy_max: f64,
    pub energy_regen: f64,
    /// Weapon-mod caps for stacking.
    pub mod_cooldown_pct_max: f64,
    pub mod_speed_pct_max: f64,
}

impl Default for MainConfig {
    fn default() -> Self {
        MainConfig {
            radius: 14.0,
            hp: 100.0,
            speed: 140.0,
            fire_cooldown_s: 0.5,
            projectile_speed: 420.0,
            projectile_damage: 12.0,
            projectile_range: 1000.0,
            vision: 450.0,
            full_detail_range: 300.0,
            dash: DashConfig::default(),
            shield: ShieldConfig::default(),
            sprint: SprintConfig::default(),
            energy_max: 100.0,
            energy_regen: 10.0,
            mod_cooldown_pct_max: 40.0,
            mod_speed_pct_max: 30.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DashConfig {
    pub cost: f64,
    pub duration_s: f64,
    pub speed_mult: f64,
}

impl Default for DashConfig {
    fn default() -> Self {
        DashConfig {
            cost: 20.0,
            duration_s: 0.3,
            speed_mult: 2.2,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ShieldConfig {
    pub cost: f64,
    pub duration_s: f64,
    /// Fraction of damage blocked, e.g. 0.7 = 70% reduction.
    pub reduction: f64,
    pub speed_mult: f64,
}

impl Default for ShieldConfig {
    fn default() -> Self {
        ShieldConfig {
            cost: 15.0,
            duration_s: 1.0,
            reduction: 0.7,
            speed_mult: 0.7,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SprintConfig {
    pub speed_mult: f64,
    pub footstep_radius: f64,
}

impl Default for SprintConfig {
    fn default() -> Self {
        SprintConfig {
            speed_mult: 1.4,
            footstep_radius: 200.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CompanionConfig {
    pub radius: f64,
    pub hp: f64,
    pub speed: f64,
    pub vision: f64,
    pub leash: f64,
    pub energy_max: f64,
    pub respawn_s: f64,
    pub sonar: SonarConfig,
}

impl Default for CompanionConfig {
    fn default() -> Self {
        CompanionConfig {
            radius: 10.0,
            hp: 30.0,
            speed: 170.0,
            vision: 250.0,
            leash: 350.0,
            energy_max: 50.0,
            respawn_s: 20.0,
            sonar: SonarConfig::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SonarConfig {
    pub cost: f64,
    pub cooldown_s: f64,
    pub reveal_radius: f64,
    pub duration_s: f64,
    pub audio_radius: f64,
}

impl Default for SonarConfig {
    fn default() -> Self {
        SonarConfig {
            cost: 25.0,
            cooldown_s: 15.0,
            reveal_radius: 600.0,
            duration_s: 3.0,
            audio_radius: 900.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AudioConfig {
    pub gunshot: f64,
    pub dash: f64,
    pub footstep: f64,
    pub sonar: f64,
    /// Bearing quantization in degrees (PLAN §3.2).
    pub bearing_quantization: u32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        AudioConfig {
            gunshot: 900.0,
            dash: 500.0,
            footstep: 200.0,
            sonar: 900.0,
            bearing_quantization: 15,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ZoneConfig {
    /// Radii per phase; last entry is the endgame collapse.
    pub radii: Vec<f64>,
    pub hold_s_min: f64,
    pub hold_s_max: f64,
    pub shrink_s: f64,
    /// Damage per second outside the circle, per phase.
    pub damage_per_phase: Vec<f64>,
}

impl Default for ZoneConfig {
    fn default() -> Self {
        ZoneConfig {
            radii: vec![1600.0, 1200.0, 850.0, 550.0, 300.0, 0.0],
            hold_s_min: 45.0,
            hold_s_max: 60.0,
            shrink_s: 10.0,
            damage_per_phase: vec![2.0, 2.0, 4.0, 4.0, 6.0, 8.0],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct LootConfig {
    pub count: u32,
    /// Pickups spawn over the first N seconds.
    pub spawn_window_s: f64,
    pub weight_hp: u32,
    pub weight_energy: u32,
    pub weight_mod: u32,
    pub hp_kit_amount: f64,
    pub energy_pack_amount: f64,
    pub mod_cooldown_pct: f64,
    pub mod_speed_pct: f64,
    pub pickup_radius: f64,
}

impl Default for LootConfig {
    fn default() -> Self {
        LootConfig {
            count: 28,
            spawn_window_s: 180.0,
            weight_hp: 40,
            weight_energy: 40,
            weight_mod: 20,
            hp_kit_amount: 35.0,
            energy_pack_amount: 40.0,
            mod_cooldown_pct: 20.0,
            mod_speed_pct: 15.0,
            pickup_radius: 25.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TimeoutConfig {
    /// A reply slower than this (ms) counts against the chronic-slow budget.
    pub slow_ms: u64,
    /// A reply slower than this (ms) forfeits immediately.
    pub fatal_ms: u64,
    /// Chronic-slow budget: this many slow replies forfeit.
    pub max_slow_count: u64,
    /// Missing this fraction of deadlines (percent) forfeits.
    pub max_missed_pct: u64,
    /// Ticks a bot may stay disconnected (momentum) before forfeit: 10s.
    pub disconnect_grace_ticks: u64,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        TimeoutConfig {
            slow_ms: 200,
            fatal_ms: 1000,
            max_slow_count: 30,
            max_missed_pct: 20,
            disconnect_grace_ticks: 100,
        }
    }
}
