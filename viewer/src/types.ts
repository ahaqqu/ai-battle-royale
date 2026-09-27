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

/** The 16-slot candy palette — Fall Guys-style vivid candy hues, tuned to
 * read on a bright pastel floor and survive stream compression (PLAN §7.3).
 * The first 8 are the standard lineup: maximally distinct hues. */
export const BOT_COLORS = [
  "#ff5f7e", // 0 watermelon
  "#35c1f0", // 1 sky
  "#ffd93b", // 2 banana
  "#8a5cff", // 3 grape
  "#43d66e", // 4 green apple
  "#ff9d3b", // 5 tangerine
  "#ff5fd0", // 6 bubblegum
  "#f2f6ff", // 7 cloud white
  "#a8e04c", // 8 lime
  "#35d6b5", // 9 mint
  "#4f7dff", // 10 blueberry
  "#c06bff", // 11 lavender
  "#c97a4a", // 12 cocoa
  "#e6455f", // 13 cherry
  "#ff8fb8", // 14 rose
  "#00e0c8", // 15 turquoise
];

export const KIND_COLORS = ["#43d66e", "#35c1f0", "#c06bff", "#ffa03c"];
export const KIND_LABELS = ["HP KIT", "ENERGY", "COOLDOWN MOD", "SPEED MOD"];

/** Chunky rounded display font used everywhere (HUD + in-canvas text). */
export const FONT = '"Baloo 2", "Fredoka", "Trebuchet MS", "Inter", sans-serif';

/** Dark grape outline "ink" every sprite is drawn with — the thick cartoon
 * outline that makes candy colors pop on the pastel floor. */
export const INK = 0x3a2c5a;
export const INK_HEX = "#3a2c5a";

/** Multiply a 0xRRGGBB color's channels by `f` (f<1 darkens, f>1 lightens). */
export function shade(col: number, f: number): number {
  const r = Math.min(255, Math.round(((col >> 16) & 0xff) * f));
  const g = Math.min(255, Math.round(((col >> 8) & 0xff) * f));
  const b = Math.min(255, Math.round((col & 0xff) * f));
  return (r << 16) | (g << 8) | b;
}

export function botColor(bot: number): string {
  return BOT_COLORS[bot % BOT_COLORS.length];
}

export function fmtTime(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

export type CamMode = "auto" | "global" | `follow:${number}` | `cam:${number}`;
