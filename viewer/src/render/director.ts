/** Auto-director v0 (PLAN §6.2): cuts to the action cluster; falls back to
 * a wide arena view. Plus 'global' and 'follow' modes. */

import { CamMode, Frame, KillMarker } from "../types.js";
import { CameraTarget } from "./stage.js";


const ARENA_CENTER = 1600;

/** Auto-mode stability: action events accumulate in a decaying ~2s window so
 * the "action cluster" is a place, not the last tick's flicker; the desired
 * shot is then low-pass filtered (τ ≈ 0.4s, log-zoom space), and brief lulls
 * hold the last shot before easing wide. */
const TAU_S = 0.4;
const EVENT_HALF_LIFE_S = 0.8;
const MIN_CLUSTER_WEIGHT = 0.15;
const HOLD_IDLE_MS = 1200;
const EASE_TO_WIDE_MS = 2500;

export class Director {
  mode: CamMode = "auto";
  lastGood: CameraTarget = { x: ARENA_CENTER, y: ARENA_CENTER, zoom: 0.35 };

  private smX = ARENA_CENTER;
  private smY = ARENA_CENTER;
  private smLogZoom = Math.log(0.35);
  private lastActionAt = 0;
  private lastCallAt = 0;
  private acc: { x: number; y: number; w: number }[] = [];

  /** Compute the desired camera for the frame being shown. */
  targetFor(frameA: Frame, frameB: Frame, botPos: { x: number; y: number } | null): CameraTarget {
    if (this.mode.startsWith("cam:")) {
      // Player-cam: ride just behind the bot's position.
      if (botPos) return { x: botPos.x, y: botPos.y, zoom: 1.15 };
      return this.lastGood;
    }
    if (this.mode.startsWith("follow:")) {
      if (botPos) return { x: botPos.x, y: botPos.y, zoom: 1.7 };
      return this.lastGood;
    }
    if (this.mode === "global") {
      const t = { x: ARENA_CENTER, y: ARENA_CENTER, zoom: fitZoom(3300) };
      this.lastGood = t;
      return t;
    }
    // auto: decay the recent-action window, fold in this tick's events
    // (deaths > hits > shots), and aim at the accumulated cluster.
    const now = performance.now();
    const dt = Math.min(0.1, Math.max(0.001, (now - this.lastCallAt) / 1000));
    this.lastCallAt = now;
    const evs = [...frameA.events, ...frameB.events];
    const decay = Math.exp(-dt / EVENT_HALF_LIFE_S);
    for (const p of this.acc) p.w *= decay;
    for (const e of evs) {
      const at = (e.at ?? e.from) as [number, number] | undefined;
      if (!at) continue;
      const w = e.type === "death" ? 4 : e.type === "hit" ? 2.4 : e.type === "shot" ? 1.2 : 0;
      if (w === 0) continue;
      this.acc.push({ x: at[0], y: at[1], w });
    }
    this.acc = this.acc.filter((p) => p.w > 0.04);

    let wx = 0, wy = 0, wsum = 0;
    for (const p of this.acc) { wsum += p.w; wx += p.x * p.w; wy += p.y * p.w; }

    let dx: number, dy: number, dzLog: number;
    if (wsum >= MIN_CLUSTER_WEIGHT) {
      this.lastActionAt = now;
      const cx = wx / wsum, cy = wy / wsum;
      // Spread is measured against the settled cluster mean.
      let maxSpread = 0;
      for (const p of this.acc) {
        maxSpread = Math.max(maxSpread, Math.hypot(p.x - cx, p.y - cy) * 2 + 500);
      }
      // Zoom floor of 0.55 keeps the tarsius readable at wide shots.
      dx = cx;
      dy = cy;
      dzLog = Math.log(Math.min(1.5, Math.max(0.55, fitZoom(maxSpread + 380))));
    } else {
      // Quiet window: hold the last shot for a beat, then ease back to the
      // wide zone view — never snap (that was the zoom pumping).
      const wide = this.wideTarget(frameB);
      const blend = Math.max(0, Math.min(1,
        (now - this.lastActionAt - HOLD_IDLE_MS) / EASE_TO_WIDE_MS));
      const curZoom = Math.exp(this.smLogZoom);
      dx = this.smX + (wide.x - this.smX) * blend;
      dy = this.smY + (wide.y - this.smY) * blend;
      dzLog = Math.log(curZoom) + (Math.log(wide.zoom) - Math.log(curZoom)) * blend;
    }

    // Low-pass the desired shot; log-zoom so 0.6→1.2 pulls as hard as
    // 1.2→2.4. The dt clamp bounds jumps after seeks/lag spikes.
    const k = 1 - Math.exp(-dt / TAU_S);
    this.smX += (dx - this.smX) * k;
    this.smY += (dy - this.smY) * k;
    this.smLogZoom += (dzLog - this.smLogZoom) * k;
    const t = { x: this.smX, y: this.smY, zoom: Math.exp(this.smLogZoom) };
    if (Number.isFinite(t.x) && Number.isFinite(t.y) && Number.isFinite(t.zoom)) {
      this.lastGood = t;
      return t;
    }
    return this.lastGood;
  }

  /** Wide establishing shot, biased toward the zone center. */
  private wideTarget(frameB: Frame): CameraTarget {
    const zx = frameB.zone[3] || frameB.zone[0];
    const zy = frameB.zone[4] || frameB.zone[1];
    const zr = frameB.zone[5] || frameB.zone[2];
    return { x: (zx + ARENA_CENTER) / 2, y: (zy + ARENA_CENTER) / 2, zoom: Math.min(0.55, fitZoom(zr * 2.4 + 700)) };
  }
}

/** Zoom that fits `span` world units into the viewport height-ish. */
export function fitZoom(span: number): number {
  const h = window.innerHeight;
  const w = window.innerWidth;
  return Math.min(w, h * 1.7) / span;
}

/** Kill markers for the timeline (from replay data). */
export function markerTitle(m: KillMarker, names: string[]): string {
  const killer = m.killer === null ? "the zone" : `${names[m.killer]} (bot ${m.killer})`;
  return `t${m.tick}: ${killer} eliminated ${names[m.victim]} (bot ${m.victim})`;
}
