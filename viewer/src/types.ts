/** Shared viewer types. Frames are compact typed arrays built from the
 * WASM spectator frames — cheap enough to cache every tick of a match. */

export const ARENA = 3200;
export const UNIT_STRIDE = 11;
export const PROJ_STRIDE = 8;
export const PICKUP_STRIDE = 4;

/** Unit float layout (UNIT_STRIDE per unit):
 * [id, bot, kind(0 main | 1 companion), x, y, vx, vy, facingDeg, hp01, flags, maxHp] */
export const enum U {
  ID = 0, BOT = 1, KIND = 2, X = 3, Y = 4, VX = 5, VY = 6, FACING = 7, HP01 = 8, FLAGS = 9, MAXHP = 10,
}
export const UF_ALIVE = 1, UF_SPRINT = 2, UF_DASH = 4, UF_SHIELD = 8;

/** Projectile layout: [id, bot, x, y, vx, vy, damage, weapon] */
export const enum P { ID = 0, BOT = 1, X = 2, Y = 3, VX = 4, VY = 5, DMG = 6, WEAPON = 7 }

/** Pickup layout: [id, kindIdx, x, y] — kindIdx 0-3 are the candy tins,
 * 4-9 are the gun pickups (index into WEAPONS). */
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
  seenPlayers: { id: number; pos: [number, number]; detail: string; hp?: number; weapon?: string; viaSonar?: boolean }[];
  seenCompanions: { id: number; owner: number; pos: [number, number]; detail: string }[];
  seenProjectiles: { id: number; pos: [number, number]; vel: [number, number]; owner: number; weapon?: string }[];
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

/** CamFrame seen-player / projectile weapon entries carry the wire name. */
export type WeaponName =
  | "pea"
  | "sprinkler"
  | "scatter"
  | "lance"
  | "bouncer"
  | "skewer"
  | "popper"
  | "boss_cannon";

/** The gun roster, wire name order. Index 0 = starter pea gun. */
export const WEAPONS: {
  name: WeaponName; label: string; color: string; blurb: string; cd: number;
}[] = [
  { name: "pea", label: "PEA POPPER", color: "#8d82b5", blurb: "starter bubblegun", cd: 0.5 },
  { name: "sprinkler", label: "SPRINKLER", color: "#ffd93b", blurb: "hyper SMG — wobbly but wild", cd: 0.16 },
  { name: "scatter", label: "SCATTERSHOT", color: "#ff5f7e", blurb: "6-pellet boom cone", cd: 0.95 },
  { name: "lance", label: "SUGAR LANCE", color: "#7df0ff", blurb: "slow bolt, huge damage", cd: 1.7 },
  { name: "bouncer", label: "GUM BOUNCER", color: "#35d6b5", blurb: "ricochets off walls ×3", cd: 0.55 },
  { name: "skewer", label: "LIQUORICE SKEWER", color: "#c06bff", blurb: "pierces up to 3 units", cd: 0.6 },
  { name: "popper", label: "POP ROCK", color: "#ff6a00", blurb: "explodes on impact", cd: 0.95 },
  // Boss-only gun (Slain the Boss): never in the loot pool.
  { name: "boss_cannon", label: "BOSS CANNON", color: "#ff2e4d", blurb: "the boss's heavy splash shell", cd: 1.1 },
];

/** Wire name → roster index (unknown = pea). */
export function weaponIdx(name?: string | null): number {
  if (!name) return 0;
  const i = WEAPONS.findIndex((w) => w.name === name);
  return i < 0 ? 0 : i;
}

/** Pickup kind indices: 0-3 candy tins, 4-9 = weapon_<name> wire kinds. */
export function pickupKindIdx(kind: string): number {
  switch (kind) {
    case "hp_kit": return 0;
    case "energy": return 1;
    case "mod_cooldown": return 2;
    case "mod_speed": return 3;
    default: {
      if (kind.startsWith("weapon_")) return 4 + weaponIdx(kind.slice(7)) - 1;
      return 3;
    }
  }
}

export const KIND_COLORS = [
  "#43d66e", "#35c1f0", "#c06bff", "#ffa03c",
  ...WEAPONS.slice(1).map((w) => w.color),
];
export const KIND_LABELS = [
  "HP KIT", "ENERGY", "COOLDOWN MOD", "SPEED MOD",
  ...WEAPONS.slice(1).map((w) => w.label),
];

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
