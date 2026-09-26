/** Projectiles + pickups + zone rings, redrawn per frame. */

import { Container, Graphics, Sprite, Text } from "pixi.js";
import { botColor, KIND_COLORS, K, P, PICKUP_STRIDE, PROJ_STRIDE, Z } from "../types.js";
import { Stage } from "./stage.js";

export class ProjectileLayer {
  private container: Container;
  private sprites = new Map<number, { line: Graphics; glow: Sprite; bot: number }>();

  constructor(stage: Stage) {
    this.container = stage.projLayer;
    this.glowTex = stage.glowTex;
  }
  private glowTex: Sprite["texture"];

  /** Match by id across A→B and interpolate; render-only. */
  update(
    a: Float32Array, aCount: number,
    b: Float32Array, bCount: number,
    t: number,
    onTrail: (x: number, y: number, color: number, intense: boolean) => void,
  ): void {
    const seen = new Set<number>();
    const mapB = new Map<number, number>();
    for (let i = 0; i < bCount; i++) mapB.set(b[i * PROJ_STRIDE + P.ID], i);
    for (let i = 0; i < aCount; i++) {
      const id = a[i * PROJ_STRIDE + P.ID];
      const bot = a[i * PROJ_STRIDE + P.BOT];
      const ax = a[i * PROJ_STRIDE + P.X], ay = a[i * PROJ_STRIDE + P.Y];
      const avx = a[i * PROJ_STRIDE + P.VX], avy = a[i * PROJ_STRIDE + P.VY];
      const bi = mapB.get(id);
      let x = ax, y = ay;
      if (bi !== undefined) {
        x = ax + (b[bi * PROJ_STRIDE + P.X] - ax) * t;
        y = ay + (b[bi * PROJ_STRIDE + P.Y] - ay) * t;
      }
      seen.add(id);
      let s = this.sprites.get(id);
      if (!s) {
        const line = new Graphics();
        const glow = new Sprite(this.glowTex);
        glow.anchor.set(0.5);
        glow.blendMode = "add";
        this.container.addChild(line, glow);
        s = { line, glow, bot: -1 };
        this.sprites.set(id, s);
      }
      if (s.bot !== bot) {
        const col = parseInt(botColor(bot).slice(1), 16);
        s.line.clear();
        s.line.moveTo(0, 0).lineTo(16, 0).stroke({ width: 2.6, color: col });
        s.glow.tint = col;
        s.bot = bot;
      }
      s.line.position.set(x, y);
      s.line.rotation = Math.atan2(avy, avx);
      s.glow.position.set(x, y);
      s.glow.scale.set(0.22);
      s.line.visible = true;
      s.glow.visible = true;
      onTrail(x, y, parseInt(botColor(bot).slice(1), 16), true);
    }
    // Hide vanished, show current.
    for (const [id, s] of this.sprites) {
      const vis = seen.has(id);
      s.line.visible = vis;
      s.glow.visible = vis;
    }
  }
}

/** Pickups + zone rings. */
export class PickupLayer {
  private container: Container;
  private sprites = new Map<number, { root: Container; kind: number }>();

  constructor(private stage: Stage) {
    this.container = stage.pickupLayer;
  }

  update(pickups: Float32Array, count: number, tick: number): void {
    const seen = new Set<number>();
    const pulse = 1 + Math.sin(tick / 5) * 0.12;
    for (let i = 0; i < count; i++) {
      const id = pickups[i * PICKUP_STRIDE + K.ID];
      const kind = pickups[i * PICKUP_STRIDE + K.KIND];
      const x = pickups[i * PICKUP_STRIDE + K.X];
      const y = pickups[i * PICKUP_STRIDE + K.Y];
      seen.add(id);
      let s = this.sprites.get(id);
      if (!s || s.kind !== kind) {
        if (s) this.container.removeChild(s.root);
        const root = new Container();
        const col = parseInt(KIND_COLORS[kind].slice(1), 16);
        const glow = new Sprite(this.stage.glowTex);
        glow.anchor.set(0.5);
        glow.tint = col;
        glow.alpha = 0.4;
        glow.blendMode = "add";
        glow.scale.set(0.5);
        const g = new Graphics();
        g.moveTo(0, -8).lineTo(8, 0).lineTo(0, 8).lineTo(-8, 0).closePath()
          .fill({ color: 0x0c1220, alpha: 0.9 })
          .stroke({ width: 1.8, color: col });
        root.addChild(glow, g);
        this.container.addChild(root);
        s = { root, kind };
        this.sprites.set(id, s);
      }
      s.root.position.set(x, y);
      s.root.scale.set(pulse);
      s.root.visible = true;
    }
    for (const [id, s] of this.sprites) {
      if (!seen.has(id)) s.root.visible = false;
      void id;
    }
  }
}

/** Zone: dark red outside-overlay + glowing current ring + dashed next ring. */
export class ZoneLayerView {
  private overlay = new Graphics();
  private holes = new Graphics();
  private ring = new Graphics();
  private nextRing = new Graphics();
  private label: Text;

  constructor(stage: Stage) {
    this.overlay.blendMode = "normal";
    this.holes.blendMode = "erase";
    stage.zoneLayer.addChild(this.overlay, this.holes, this.ring, this.nextRing);
    this.label = new Text({
      text: "",
      style: { fontFamily: "Inter, sans-serif", fontSize: 15, fontWeight: "800", fill: "#ff8fa3", letterSpacing: 3 },
    });
    this.label.anchor.set(0.5);
    stage.zoneLayer.addChild(this.label);
  }

  update(zone: Float32Array, shrinking: boolean, nextVisible: boolean): void {
    const cx = zone[Z.CX], cy = zone[Z.CY], r = zone[Z.R];
    // Outside-darkening via erase hole (works in WebGL/WebGPU).
    this.overlay.clear();
    this.overlay.rect(-2000, -2000, 7200, 7200).fill({ color: 0x30091a, alpha: shrinking ? 0.4 : 0.26 });
    this.holes.clear();
    this.holes.circle(cx, cy, r).fill({ color: 0xffffff });

    this.ring.clear();
    this.ring.circle(cx, cy, r).stroke({ width: 3, color: 0xff5d7d, alpha: 0.85 });
    this.ring.circle(cx, cy, Math.max(1, r - 7)).stroke({ width: 1, color: 0xff8fa3, alpha: 0.35 });

    this.nextRing.clear();
    if (nextVisible) {
      const nx = zone[Z.NCX], ny = zone[Z.NCY], nr = zone[Z.NR];
      // Dashed next-zone ring.
      const segs = 72;
      for (let i = 0; i < segs; i += 2) {
        const a0 = (i / segs) * Math.PI * 2;
        const a1 = ((i + 1) / segs) * Math.PI * 2;
        this.nextRing.moveTo(nx + Math.cos(a0) * nr, ny + Math.sin(a0) * nr)
          .lineTo(nx + Math.cos(a1) * nr, ny + Math.sin(a1) * nr)
          .stroke({ width: 1.6, color: 0x9fd8ff, alpha: 0.6 });
      }
    }

    if (shrinking) {
      this.label.position.set(cx, cy - r - 18);
      this.label.text = "⚠ ZONE SHRINKING";
      this.label.visible = true;
      this.label.alpha = 0.6 + 0.4 * Math.sin(performance.now() / 180);
    } else {
      this.label.visible = false;
    }
  }
}
