/** Projectiles + pickups + zone rings, redrawn per frame. Fall Guys styling:
 * candy-pellet projectiles, candy-box pickups, bubblegum slime zone. */

import { Container, Graphics, Sprite, Text } from "pixi.js";
import { botColor, FONT, INK, KIND_COLORS, K, P, PICKUP_STRIDE, PROJ_STRIDE, Z } from "../types.js";
import { drawOutsideOverlay, Stage } from "./stage.js";

export class ProjectileLayer {
  private container: Container;
  private sprites = new Map<number, { ball: Graphics; glow: Sprite; bot: number }>();

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
      const bi = mapB.get(id);
      let x = ax, y = ay;
      if (bi !== undefined) {
        x = ax + (b[bi * PROJ_STRIDE + P.X] - ax) * t;
        y = ay + (b[bi * PROJ_STRIDE + P.Y] - ay) * t;
      }
      seen.add(id);
      let s = this.sprites.get(id);
      if (!s) {
        const ball = new Graphics();
        const glow = new Sprite(this.glowTex);
        glow.anchor.set(0.5);
        glow.blendMode = "add";
        glow.alpha = 0.35;
        this.container.addChild(glow, ball);
        s = { ball, glow, bot: -1 };
        this.sprites.set(id, s);
      }
      if (s.bot !== bot) {
        const col = parseInt(botColor(bot).slice(1), 16);
        s.ball.clear();
        // Candy pellet: colored core, white glaze ring, thin ink edge.
        s.ball.circle(0, 0, 5.4).fill({ color: 0xffffff });
        s.ball.circle(0, 0, 4).fill({ color: col }).stroke({ width: 1.4, color: INK, alpha: 0.7 });
        s.ball.circle(-1.4, -1.4, 1.3).fill({ color: 0xffffff, alpha: 0.9 });
        s.glow.tint = col;
        s.glow.scale.set(0.16);
        s.bot = bot;
      }
      s.ball.position.set(x, y);
      s.glow.position.set(x, y);
      s.ball.visible = true;
      s.glow.visible = true;
      onTrail(x, y, parseInt(botColor(bot).slice(1), 16), true);
    }
    // Hide vanished, show current.
    for (const [id, s] of this.sprites) {
      const vis = seen.has(id);
      s.ball.visible = vis;
      s.glow.visible = vis;
    }
  }
}

/** Pickups: bobbing candy boxes with a glaze shine. */
export class PickupLayer {
  private container: Container;
  private sprites = new Map<number, { root: Container; kind: number }>();

  constructor(private stage: Stage) {
    this.container = stage.pickupLayer;
  }

  update(pickups: Float32Array, count: number, tick: number): void {
    const seen = new Set<number>();
    const pulse = 1 + Math.sin(tick / 5) * 0.1;
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
        glow.alpha = 0.3;
        glow.blendMode = "add";
        glow.scale.set(0.5);
        const g = new Graphics();
        // Rounded candy tin + ink edge + glaze shine.
        g.roundRect(-9, -9, 18, 18, 7).fill({ color: col }).stroke({ width: 2.2, color: INK, alpha: 0.85 });
        g.roundRect(-5.5, -6, 11, 5, 2.5).fill({ color: 0xffffff, alpha: 0.6 });
        root.addChild(glow, g);
        this.container.addChild(root);
        s = { root, kind };
        this.sprites.set(id, s);
      }
      const bob = Math.sin(tick / 4 + id * 1.3) * 3;
      s.root.position.set(x, y + bob);
      s.root.scale.set(pulse);
      s.root.visible = true;
    }
    for (const [id, s] of this.sprites) {
      if (!seen.has(id)) s.root.visible = false;
      void id;
    }
  }
}

/** Zone: bubblegum-slime outside-overlay + chunky white current ring +
 * dashed next ring. */
export class ZoneLayerView {
  private overlay = new Graphics();
  private ring = new Graphics();
  private nextRing = new Graphics();
  private label: Text;

  constructor(stage: Stage) {
    stage.zoneLayer.addChild(this.overlay, this.ring, this.nextRing);
    this.label = new Text({
      text: "",
      style: {
        fontFamily: FONT, fontSize: 17, fontWeight: "800",
        fill: 0xffffff, letterSpacing: 2,
        stroke: { color: "#e0457f", width: 5, join: "round" },
      },
    });
    this.label.anchor.set(0.5);
    stage.zoneLayer.addChild(this.label);
  }

  update(zone: Float32Array, shrinking: boolean, nextVisible: boolean): void {
    const cx = zone[Z.CX], cy = zone[Z.CY], r = zone[Z.R];
    // Pink slime tide closing in — fan mesh around the safe circle (no
    // blend tricks: they punch through to black on an opaque canvas).
    this.overlay.clear();
    drawOutsideOverlay(this.overlay, -2000, -2000, 7200, 7200, cx, cy, () => r, 0xff5fae, shrinking ? 0.5 : 0.3);

    this.ring.clear();
    this.ring.circle(cx, cy, r).stroke({ width: 6, color: 0xffffff, alpha: 0.95 });
    this.ring.circle(cx, cy, Math.max(1, r - 9)).stroke({ width: 2, color: 0xff5fae, alpha: 0.8 });

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
          .stroke({ width: 2.2, color: 0xffffff, alpha: 0.75 });
      }
    }

    if (shrinking) {
      this.label.position.set(cx, cy - r - 20);
      this.label.text = "⚠ SLIME RISING!";
      this.label.visible = true;
      this.label.alpha = 0.6 + 0.4 * Math.sin(performance.now() / 180);
    } else {
      this.label.visible = false;
    }
  }
}

