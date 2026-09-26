/** Replay pre-simulation: drives the WASM sim once end-to-end, caching a
 * compact frame per tick so scrubbing is O(1) afterwards. Player-cam passes
 * re-simulate and cache strict-fog observations for one bot. */

import init, { ReplaySim } from "./wasm/abr_wasm.js";
import wasmUrl from "./wasm/abr_wasm_bg.wasm?url";
import {
  CamFrame, Frame, FrameEvent, KillMarker, MapData, PICKUP_STRIDE,
  PlayerCam, PROJ_STRIDE, ReplayData, UNIT_STRIDE, U, UF_ALIVE, UF_DASH, UF_SHIELD, UF_SPRINT, Z,
} from "./types.js";

let inited = false;

async function ensureWasm(onProgress?: (label: string) => void): Promise<void> {
  if (!inited) {
    const mark = (m: string) => { try { onProgress?.(m); } catch { /* noop */ } };
    mark('fetching simulation engine…');
    const res = await fetch(wasmUrl);
    const bytes = await res.arrayBuffer();
    mark('compiling simulation engine…');
    await init(bytes);
    mark('wasm ready');
    inited = true;
  }
}

function nextTick(): Promise<void> {
  return new Promise((r) => requestAnimationFrame(() => r()));
}

interface RawFrame {
  tick: number;
  units: { id: number; bot: number; kind: string; pos: [number, number]; vel: [number, number]; facing: number; hp: number; max_hp: number; alive: boolean; sprint: boolean; dashing: boolean; shielding: boolean }[];
  projectiles: { id: number; bot: number; pos: [number, number]; vel: [number, number] }[];
  pickups: { id: number; pos: [number, number]; kind: string }[];
  zone: { center: [number, number]; radius: number; next?: { center: [number, number]; radius: number; locks_at_tick: number } | null };
  events: FrameEvent[];
  alive: number;
  finished: boolean;
  winner: number | null;
  kill_feed: { tick: number; killer: number | null; victim: number }[];
  minds: Record<string, { intent?: string | null; belief?: number[] | null }>;
}

function kindIdx(kind: string): number {
  switch (kind) {
    case "hp_kit": return 0;
    case "energy": return 1;
    case "mod_cooldown": return 2;
    default: return 3;
  }
}

/** Rust Vec2 serializes as {x,y}; the renderer wants [x,y]. */
function normalizeEvents(events: FrameEvent[]): FrameEvent[] {
  const toXY = (v: unknown): [number, number] | undefined => {
    if (Array.isArray(v)) return v as [number, number];
    if (v && typeof v === 'object') {
      const o = v as { x?: number; y?: number };
      if (typeof o.x === 'number' && typeof o.y === 'number') return [o.x, o.y];
    }
    return undefined;
  };
  return events.map((e) => {
    const out: Record<string, unknown> = { ...e };
    for (const k of ['at', 'from', 'center']) {
      const v = toXY(out[k]);
      if (v) out[k] = v;
    }
    return out as FrameEvent;
  });
}

function buildFrame(raw: RawFrame): Frame {
  const n = raw.units.length;
  const units = new Float32Array(n * UNIT_STRIDE);
  for (let i = 0; i < n; i++) {
    const u = raw.units[i];
    const o = i * UNIT_STRIDE;
    units[o + U.ID] = u.id;
    units[o + U.BOT] = u.bot;
    units[o + U.KIND] = u.kind === "main" ? 0 : 1;
    units[o + U.X] = u.pos[0];
    units[o + U.Y] = u.pos[1];
    units[o + U.VX] = u.vel[0];
    units[o + U.VY] = u.vel[1];
    units[o + U.FACING] = u.facing;
    units[o + U.HP01] = u.max_hp > 0 ? Math.max(0, u.hp) / u.max_hp : 0;
    units[o + U.MAXHP] = u.max_hp;
    units[o + U.FLAGS] =
      (u.alive ? UF_ALIVE : 0) | (u.sprint ? UF_SPRINT : 0) | (u.dashing ? UF_DASH : 0) | (u.shielding ? UF_SHIELD : 0);
  }
  const m = raw.projectiles.length;
  const projs = new Float32Array(m * PROJ_STRIDE);
  for (let i = 0; i < m; i++) {
    const p = raw.projectiles[i];
    const o = i * PROJ_STRIDE;
    projs[o] = p.id;
    projs[o + 1] = p.bot;
    projs[o + 2] = p.pos[0];
    projs[o + 3] = p.pos[1];
    projs[o + 4] = p.vel[0];
    projs[o + 5] = p.vel[1];
  }
  const k = raw.pickups.length;
  const pickups = new Float32Array(k * PICKUP_STRIDE);
  for (let i = 0; i < k; i++) {
    const p = raw.pickups[i];
    const o = i * PICKUP_STRIDE;
    pickups[o] = p.id;
    pickups[o + 1] = kindIdx(p.kind);
    pickups[o + 2] = p.pos[0];
    pickups[o + 3] = p.pos[1];
  }
  const zone = new Float32Array(7);
  zone[Z.CX] = raw.zone.center[0];
  zone[Z.CY] = raw.zone.center[1];
  zone[Z.R] = raw.zone.radius;
  if (raw.zone.next) {
    zone[Z.NCX] = raw.zone.next.center[0];
    zone[Z.NCY] = raw.zone.next.center[1];
    zone[Z.NR] = raw.zone.next.radius;
  }
  zone[Z.ALIVE] = raw.alive;
  return {
    tick: raw.tick,
    units, unitCount: n,
    projs, projCount: m,
    pickups, pickupCount: k,
    zone,
    events: normalizeEvents(raw.events),
    finished: raw.finished,
    winner: raw.winner,
    minds: raw.minds ?? {},
  };
}

export interface LoadedReplay {
  sim: ReplaySim;
  data: ReplayData;
}

/** Fetch + pre-simulate a replay. onProgress(0..1). */
export async function loadReplay(json: string, onProgress: (p: number, label: string) => void): Promise<LoadedReplay> {
  await ensureWasm((label) => onProgress(0.02, label));
  onProgress(0.03, "parsing replay…");
  const sim = new ReplaySim(json);

  const map: MapData = JSON.parse(sim.map_json());
  const botNames: string[] = [];
  for (let b = 0; b < sim.bots(); b++) botNames.push(sim.bot_name(b) ?? `bot ${b}`);

  const total = sim.total_ticks();
  const frames: Frame[] = [];
  const killMarkers: KillMarker[] = [];
  let winner: number | null = null;
  let feedLen = 0;

  // Yield to the UI periodically while the sim runs.
  const budgetMs = 14;
  let sliceStart = performance.now();
  while (true) {
    const rawJson = sim.step();
    if (rawJson === null || rawJson === undefined) break;
    const raw: RawFrame = JSON.parse(rawJson);
    frames.push(buildFrame(raw));
    for (; feedLen < raw.kill_feed.length; feedLen++) {
      const k = raw.kill_feed[feedLen];
      killMarkers.push({
        tick: k.tick, killer: k.killer, victim: k.victim,
        at: total > 0 ? k.tick / total : 0,
      });
    }
    if (raw.winner !== null && raw.winner !== undefined) winner = raw.winner;
    if (performance.now() - sliceStart > budgetMs) {
      onProgress(0.05 + 0.9 * (frames.length / total), `simulating match… tick ${frames.length}/${total}`);
      await nextTick();
      sliceStart = performance.now();
    }
  }

  onProgress(1, "ready");
  const data: ReplayData = { map, botNames, mapId: sim.map_id(), seed: sim.seed(), totalTicks: total, frames, killMarkers, winner };
  return { sim, data };
}

/** Re-simulate from tick 0 collecting one bot's strict-fog observation per
 * tick ("watch how bot #7 deduced the ambush" — PLAN §6.2). */
export async function buildPlayerCam(
  sim: ReplaySim,
  bot: number,
  onProgress: (p: number) => void,
): Promise<PlayerCam> {
  sim.reset();
  const total = sim.total_ticks();
  const frames: (CamFrame | null)[] = new Array(total).fill(null);
  const budgetMs = 14;
  let sliceStart = performance.now();
  for (let i = 0; i < total; i++) {
    // Step first, then observe: frames[i] mirrors the global frame at the
    // same tick, so both cameras stay in sync during playback.
    const stepJson = sim.step();
    if (stepJson === null || stepJson === undefined) break;
    const obsJson = sim.observe_current(bot);
    if (obsJson) {
      const obs = JSON.parse(obsJson);
      frames[i] = {
        me: {
          main: { pos: obs.you.main.pos, alive: obs.you.main.alive },
          comp: { pos: obs.you.companion.pos, alive: obs.you.companion.alive },
        },
        seenPlayers: (obs.seen.players ?? []).map((p: any) => ({
          id: p.id, pos: p.pos, detail: p.detail, hp: p.hp, viaSonar: p.via_sonar,
        })),
        seenCompanions: (obs.seen.companions ?? []).map((c: any) => ({
          id: c.id, owner: c.owner, pos: c.pos, detail: c.detail,
        })),
        seenProjectiles: (obs.seen.projectiles ?? []).map((p: any) => ({
          id: p.id, pos: p.pos, vel: p.vel, owner: p.owner,
        })),
        seenPickups: (obs.seen.pickups ?? []).map((p: any) => ({ id: p.id, pos: p.pos, kind: p.kind })),
        heard: (obs.heard ?? []).map((h: any) => ({ kind: h.kind, bearing: h.bearing, band: h.band })),
        zone: { center: obs.global.zone.center, radius: obs.global.zone.radius, next: obs.global.zone.next },
      };
    }
    if (performance.now() - sliceStart > budgetMs) {
      onProgress((i + 1) / total);
      await nextTick();
      sliceStart = performance.now();
    }
  }
  return { bot, frames };
}
