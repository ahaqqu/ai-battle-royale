//! MatchConfig converted to Fix, precomputed once per match. Everything the
//! deterministic sim touches goes through here.

use crate::config::MatchConfig;
use crate::fixed::{self, Fix, ONE};
use crate::types::Vec2;

#[derive(Clone, Debug)]
pub struct SimParams {
    pub tick_rate: u64,
    pub dt: Fix,
    pub arena: Fix,
    pub deadline_ms: u64,
    pub match_max_ticks: u64,

    pub main_radius: Fix,
    pub main_hp: Fix,
    pub main_speed: Fix,
    pub fire_cooldown: Fix,
    pub proj_speed: Fix,
    pub proj_damage: Fix,
    pub proj_range: Fix,
    pub main_vision: Fix,
    pub full_detail_range: Fix,
    pub dash_cost: Fix,
    pub dash_ticks: u64,
    pub dash_mult: Fix,
    pub shield_cost: Fix,
    pub shield_ticks: u64,
    pub shield_reduction: Fix,
    pub shield_speed: Fix,
    pub sprint_mult: Fix,
    pub footstep_radius: Fix,
    pub energy_max: Fix,
    pub energy_regen: Fix, // per second
    pub mod_cooldown_pct_max: Fix,
    pub mod_speed_pct_max: Fix,

    pub comp_radius: Fix,
    pub comp_hp: Fix,
    pub comp_speed: Fix,
    pub comp_vision: Fix,
    pub leash: Fix,
    pub comp_energy_max: Fix,
    pub respawn_ticks: u64,
    pub sonar_cost: Fix,
    pub sonar_cooldown: Fix,
    pub sonar_reveal: Fix,
    pub sonar_ticks: u64,
    pub sonar_audio: Fix,

    pub audio_gunshot: Fix,
    pub audio_dash: Fix,
    pub audio_footstep: Fix,
    pub bearing_q: u32,

    pub zone_radii: Vec<Fix>,
    pub zone_hold_min: u64, // ticks
    pub zone_hold_max: u64,
    pub zone_shrink_ticks: u64,
    pub zone_damage: Vec<Fix>, // per second

    pub loot_count: u32,
    pub loot_window_ticks: u64,
    pub loot_weight_hp: u32,
    pub loot_weight_energy: u32,
    pub loot_weight_mod: u32,
    pub hp_kit: Fix,
    pub energy_pack: Fix,
    pub mod_cooldown_pct: Fix,
    pub mod_speed_pct: Fix,
    pub pickup_radius: Fix,

    pub timeouts: crate::config::TimeoutConfig,
    pub auto_heel: bool,
}

impl SimParams {
    pub fn from_config(cfg: &MatchConfig) -> Self {
        let f = fixed::from_f64;
        let tick_rate = cfg.tick_rate_hz.max(1) as u64;
        SimParams {
            tick_rate,
            dt: fixed::div(ONE, f(tick_rate as f64)),
            arena: f(3200.0),
            deadline_ms: cfg.deadline_ms,
            match_max_ticks: cfg.match_max_s * tick_rate,
            main_radius: f(cfg.main.radius),
            main_hp: f(cfg.main.hp),
            main_speed: f(cfg.main.speed),
            fire_cooldown: f(cfg.main.fire_cooldown_s),
            proj_speed: f(cfg.main.projectile_speed),
            proj_damage: f(cfg.main.projectile_damage),
            proj_range: f(cfg.main.projectile_range),
            main_vision: f(cfg.main.vision),
            full_detail_range: f(cfg.main.full_detail_range),
            dash_cost: f(cfg.main.dash.cost),
            dash_ticks: (cfg.main.dash.duration_s * tick_rate as f64).round() as u64,
            dash_mult: f(cfg.main.dash.speed_mult),
            shield_cost: f(cfg.main.shield.cost),
            shield_ticks: (cfg.main.shield.duration_s * tick_rate as f64).round() as u64,
            shield_reduction: f(cfg.main.shield.reduction),
            shield_speed: f(cfg.main.shield.speed_mult),
            sprint_mult: f(cfg.main.sprint.speed_mult),
            footstep_radius: f(cfg.main.sprint.footstep_radius),
            energy_max: f(cfg.main.energy_max),
            energy_regen: f(cfg.main.energy_regen),
            mod_cooldown_pct_max: f(cfg.main.mod_cooldown_pct_max),
            mod_speed_pct_max: f(cfg.main.mod_speed_pct_max),
            comp_radius: f(cfg.companion.radius),
            comp_hp: f(cfg.companion.hp),
            comp_speed: f(cfg.companion.speed),
            comp_vision: f(cfg.companion.vision),
            leash: f(cfg.companion.leash),
            comp_energy_max: f(cfg.companion.energy_max),
            respawn_ticks: (cfg.companion.respawn_s * tick_rate as f64).round() as u64,
            sonar_cost: f(cfg.companion.sonar.cost),
            sonar_cooldown: f(cfg.companion.sonar.cooldown_s),
            sonar_reveal: f(cfg.companion.sonar.reveal_radius),
            sonar_ticks: (cfg.companion.sonar.duration_s * tick_rate as f64).round() as u64,
            sonar_audio: f(cfg.companion.sonar.audio_radius),
            audio_gunshot: f(cfg.audio.gunshot),
            audio_dash: f(cfg.audio.dash),
            audio_footstep: f(cfg.audio.footstep),
            bearing_q: cfg.audio.bearing_quantization,
            zone_radii: cfg.zone.radii.iter().map(|r| f(*r)).collect(),
            zone_hold_min: (cfg.zone.hold_s_min * tick_rate as f64).round() as u64,
            zone_hold_max: (cfg.zone.hold_s_max * tick_rate as f64).round() as u64,
            zone_shrink_ticks: (cfg.zone.shrink_s * tick_rate as f64).round() as u64,
            zone_damage: cfg.zone.damage_per_phase.iter().map(|d| f(*d)).collect(),
            loot_count: cfg.loot.count,
            loot_window_ticks: (cfg.loot.spawn_window_s * tick_rate as f64).round() as u64,
            loot_weight_hp: cfg.loot.weight_hp,
            loot_weight_energy: cfg.loot.weight_energy,
            loot_weight_mod: cfg.loot.weight_mod,
            hp_kit: f(cfg.loot.hp_kit_amount),
            energy_pack: f(cfg.loot.energy_pack_amount),
            mod_cooldown_pct: f(cfg.loot.mod_cooldown_pct / 100.0),
            mod_speed_pct: f(cfg.loot.mod_speed_pct / 100.0),
            pickup_radius: f(cfg.loot.pickup_radius),
            timeouts: cfg.timeouts.clone(),
            auto_heel: cfg.auto_heel,
        }
    }
}

/// Point on segment a→b at parameter t.
#[inline]
pub fn lerp_point(a: Vec2, b: Vec2, t: Fix) -> Vec2 {
    Vec2::new(
        a.x + fixed::mul(b.x - a.x, t),
        a.y + fixed::mul(b.y - a.y, t),
    )
}
