/** Auto-director v0 (PLAN §6.2): cuts to the action cluster; falls back to
 * a wide arena view. Plus 'global' and 'follow' modes. */

import { CamMode, Frame, KillMarker } from "../types.js";
import { CameraTarget } from "./stage.js";


const ARENA_CENTER = 1600;

export class Director {
  mode: CamMode = "auto";
  lastGood: CameraTarget = { x: ARENA_CENTER, y: ARENA_CENTER, zoom: 0.35 };

  /** Compute the desired camera for the frame being shown. */
  targetFor(frameA: Frame, frameB: Frame, botPos: { x: number; y: number } | null): CameraTarget {
    if (this.mode.startsWith("cam:")) {
      // Player-cam: ride just behind the bot's position.
      if (botPos) return { x: botPos.x, y: botPos.y, zoom: 1.15 };
      return this.lastGood;
    }
    if (this.mode.startsWith("follow:")) {
      if (botPos) return { x: botPos.x, y: botPos.y, zoom: 1.5 };
      return this.lastGood;
    }
    if (this.mode === "global") {
      const t = { x: ARENA_CENTER, y: ARENA_CENTER, zoom: fitZoom(3300) };
      this.lastGood = t;
      return t;
    }
    // auto: weighted cluster of recent action (last ~2s of frames).
    const evs = [...frameA.events, ...frameB.events];
    let wx = 0, wy = 0, wsum = 0, maxSpread = 0;
    let any = false;
    for (const e of evs) {
      const at = (e.at ?? e.from) as [number, number] | undefined;
      if (!at) continue;
      const w = e.type === "death" ? 4 : e.type === "hit" ? 2.4 : e.type === "shot" ? 1.2 : 0;
      if (w === 0) continue;
      any = true;
      wx += at[0] * w; wy += at[1] * w; wsum += w;
      maxSpread = Math.max(maxSpread, Math.hypot(at[0] - (wx / wsum), at[1] - (wy / wsum)) * 2 + 500);
    }
    if (any && wsum > 0) {
      const cx = wx / wsum, cy = wy / wsum;
      const zoom = Math.min(1.4, Math.max(0.42, fitZoom(maxSpread + 380)));
      const t = { x: cx, y: cy, zoom };
      if (Number.isFinite(t.x) && Number.isFinite(t.y) && Number.isFinite(t.zoom)) {
        this.lastGood = t;
        return t;
      }
      return this.lastGood;
    }
    // No action: ease back to a wide shot, biased toward the zone center.
    const zx = frameB.zone[3] || frameB.zone[0];
    const zy = frameB.zone[4] || frameB.zone[1];
    const zr = frameB.zone[5] || frameB.zone[2];
    const t = { x: (zx + ARENA_CENTER) / 2, y: (zy + ARENA_CENTER) / 2, zoom: Math.min(0.5, fitZoom(zr * 2.4 + 700)) };
    this.lastGood = t;
    return t;
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
