/** Shared viewer types. Frames are compact typed arrays built from the
 * WASM spectator frames — cheap enough to cache every tick of a match. */

export const ARENA = 3200;
export const UNIT_STRIDE = 11;
export const PROJ_STRIDE = 7;
export const PICKUP_STRIDE = 4;

/** Unit float layout (UNIT_STRIDE per unit):
 * [id, bot, kind(0 main | 1 companion), x, y, vx, vy, facingDeg, hp01, flags, maxHp] */
export const enum U {
  ID = 0, BOT = 1, KIND = 2, X = 3, Y = 4, VX = 5, VY = 6, FACING = 7, HP01 = 8, FLAGS = 9, MAXHP = 10,
}
export const UF_ALIVE = 1, UF_SPRINT = 2, UF_DASH = 4, UF_SHIELD = 8;

/** Projectile layout: [id, bot, x, y, vx, vy, damage] */
export const enum P { ID = 0, BOT = 1, X = 2, Y = 3, VX = 4, VY = 5, DMG = 6 }

/** Pickup layout: [id, kindIdx(0 hp,1 energy,2 cd mod,3 spd mod), x, y] */
export const enum K { ID = 0, KIND = 1, X = 2, Y = 3 }

/** Zone layout: [cx, cy, r, nextCx, nextCy, nextR, aliveCount] */
export const enum Z { CX = 0, CY = 1, R = 2, NCX = 3, NCY = 4, NR = 5, ALIVE = 6 }

export interface Frame {
  tick: number;
  units: Float32Array;   // 32 slots
  unitCount: number;
  projs: Float32Array;
  projCount: number;
  pickups: Float32Array;
  pickupCount: number;
  zone: Float32Array;
  events: FrameEvent[];
  finished: boolean;
  winner: number | null;
  /** Mind-cam debug channel per bot (PLAN §6.3). */
  minds: Record<string, { intent?: string | null; belief?: number[] | null }>;
}

export interface FrameEvent {
  type: string;
  [k: string]: unknown;
}

export interface KillMarker {
  tick: number;
  killer: number | null; // null = zone
  victim: number;
  at: number; // 0..1 position on the timeline
}

export interface MapData {
  id: string;
  size: number;
  walls: { min: [number, number]; max: [number, number]; kind: "wall" | "cover" }[];
  spawns: [number, number][];
}

export interface ReplayData {
  map: MapData;
  botNames: string[];
  mapId: string;
  seed: number;
  totalTicks: number;
  frames: Frame[];
  killMarkers: KillMarker[];
  winner: number | null;
}

/** Player-cam data: what one bot actually saw, per tick (strict fog). */
export interface CamFrame {
  me: { main: { pos: [number, number]; alive: boolean }; comp: { pos: [number, number] | null; alive: boolean } };
  seenPlayers: { id: number; pos: [number, number]; detail: string; hp?: number; viaSonar?: boolean }[];
  seenCompanions: { id: number; owner: number; pos: [number, number]; detail: string }[];
  seenProjectiles: { id: number; pos: [number, number]; vel: [number, number]; owner: number }[];
  seenPickups: { id: number; pos: [number, number]; kind: string }[];
  heard: { kind: string; bearing: number; band: string }[];
  zone: { center: [number, number]; radius: number; next?: { center: [number, number]; radius: number } | null };
}

export interface PlayerCam {
  bot: number;
  frames: (CamFrame | null)[];
}

/** The 16-bot neon palette. Distinct hues, tuned to read on dark bg and
 * survive stream compression (PLAN §7.3). */
export const BOT_COLORS = [
  "#00e5ff", "#ff4fd8", "#7cff4f", "#ffd54f", "#ff6b3d", "#4f7cff",
  "#b44fff", "#4fffb0", "#ff4f4f", "#e8ff4f", "#4fd8ff", "#ff9ff3",
  "#9dff4f", "#ffb84f", "#4fff6b", "#c04fff",
];

export const KIND_COLORS = ["#58ff9b", "#59c2ff", "#e59bff", "#e59bff"];
export const KIND_LABELS = ["HP KIT", "ENERGY", "COOLDOWN MOD", "SPEED MOD"];

export function botColor(bot: number): string {
  return BOT_COLORS[bot % BOT_COLORS.length];
}

export function fmtTime(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

export type CamMode = "auto" | "global" | `follow:${number}` | `cam:${number}`;
