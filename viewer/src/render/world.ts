/** Projectiles + pickups + zone rings, redrawn per frame. Candy-arcade styling:
 * candy-pellet projectiles, candy-box pickups, bubblegum slime zone. */

import { Container, Graphics, Sprite, Text } from "pixi.js";
import { botColor, FONT, INK, KIND_COLORS, K, P, PICKUP_STRIDE, PROJ_STRIDE, shade, WEAPONS, Z } from "../types.js";
import { drawOutsideOverlay, Stage } from "./stage.js";

/** Gungeon-style bullet sprites: each gun reads differently at a glance —
 * shape first, color second. All point +X; the layer rotates by velocity. */
function drawBullet(g: Graphics, w: number, col: number): void {
  switch (w) {
    case 1: // sprinkler — tiny gold pill
      g.roundRect(-4.5, -2.4, 9, 4.8, 2.4).fill({ color: col }).stroke({ width: 1.2, color: INK, alpha: 0.7 });
      g.circle(3, 0, 1.7).fill({ color: 0xffffff, alpha: 0.95 });
      break;
    case 2: // scatter — chunky watermelon pellet
      g.circle(0, 0, 4.4).fill({ color: col }).stroke({ width: 1.4, color: INK, alpha: 0.7 });
      g.circle(-1.3, -1.3, 1.4).fill({ color: 0xffffff, alpha: 0.9 });
      break;
    case 3: // lance — long white-hot bolt with a bright tip
      g.roundRect(-12, -2, 24, 4, 2).fill({ color: shade(col, 0.55) });
      g.moveTo(11, -2.6).lineTo(18, 0).lineTo(11, 2.6).closePath().fill({ color: 0xffffff });
      g.roundRect(-12, -0.9, 23, 1.8, 0.9).fill({ color: 0xffffff });
      break;
    case 4: // bouncer — glossy gumball with a shine band
      g.circle(0, 0, 5.6).fill({ color: col }).stroke({ width: 1.4, color: INK, alpha: 0.75 });
      g.arc(0, 0, 3.1, 2.3, 4.1).stroke({ width: 1.7, color: 0xffffff, alpha: 0.6 });
      g.circle(-1.9, -1.9, 1.9).fill({ color: 0xffffff, alpha: 0.95 });
      break;
    case 5: // skewer — slim liquorice needle
      g.moveTo(-10, 0).lineTo(5, -1.9).lineTo(11, 0).lineTo(5, 1.9).closePath()
        .fill({ color: col }).stroke({ width: 1.1, color: INK, alpha: 0.7 });
      g.circle(-4.5, 0, 1.5).fill({ color: 0xffffff, alpha: 0.85 });
      break;
    case 6: // popper — fat charged orb with a fuse spark
      g.circle(0, 0, 6.2).fill({ color: col }).stroke({ width: 1.6, color: INK, alpha: 0.8 });
      g.circle(-2, -2, 2.1).fill({ color: 0xffffff, alpha: 0.9 });
      g.circle(3.6, 2.6, 1.5).fill({ color: 0xffd93b });
      break;
    default: // pea — the starter candy pellet (owner-colored)
      g.circle(0, 0, 5.4).fill({ color: 0xffffff });
      g.circle(0, 0, 4).fill({ color: col }).stroke({ width: 1.4, color: INK, alpha: 0.7 });
      g.circle(-1.4, -1.4, 1.3).fill({ color: 0xffffff, alpha: 0.9 });
  }
}

export class ProjectileLayer {
  private container: Container;
  private sprites = new Map<number, { ball: Graphics; glow: Sprite; key: number }>();
  private glowTex: Sprite["texture"];

  /** `enlarge` scales bullet art (not positions): live play runs zoomed-in
   * where bullets must read at a glance, spectator keeps stock sizes. */
  constructor(stage: Stage, private enlarge = 1) {
    this.container = stage.projLayer;
    this.glowTex = stage.glowTex;
  }

  /** Match by id across A→B and interpolate; render-only. Bullets present
   * only in B (freshly spawned) render at their B position immediately. */
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
      const weapon = a[i * PROJ_STRIDE + P.WEAPON] ?? 0;
      const vx = a[i * PROJ_STRIDE + P.VX], vy = a[i * PROJ_STRIDE + P.VY];
      const ax = a[i * PROJ_STRIDE + P.X], ay = a[i * PROJ_STRIDE + P.Y];
      const bi = mapB.get(id);
      let x = ax, y = ay;
      if (bi !== undefined) {
        x = ax + (b[bi * PROJ_STRIDE + P.X] - ax) * t;
        y = ay + (b[bi * PROJ_STRIDE + P.Y] - ay) * t;
      }
      seen.add(id);
      this.place(id, bot, weapon, x, y, vx, vy, onTrail);
    }
    // B-only: spawned since A — draw at once rather than a tick late.
    for (let i = 0; i < bCount; i++) {
      const id = b[i * PROJ_STRIDE + P.ID];
      if (seen.has(id)) continue;
      seen.add(id);
      this.place(
        id, b[i * PROJ_STRIDE + P.BOT], b[i * PROJ_STRIDE + P.WEAPON] ?? 0,
        b[i * PROJ_STRIDE + P.X], b[i * PROJ_STRIDE + P.Y],
        b[i * PROJ_STRIDE + P.VX], b[i * PROJ_STRIDE + P.VY], onTrail,
      );
    }
    // Hide vanished, show current.
    for (const [id, s] of this.sprites) {
      const vis = seen.has(id);
      s.ball.visible = vis;
      s.glow.visible = vis;
    }
  }

  /** Upsert one bullet sprite and fire its trail callback. */
  private place(
    id: number, bot: number, weapon: number,
    x: number, y: number, vx: number, vy: number,
    onTrail: (x: number, y: number, color: number, intense: boolean) => void,
  ): void {
    // Bullets are gun-colored (pea stays owner-colored) — you read the
    // threat before you read the shooter.
    const col = weapon === 0
      ? parseInt(botColor(bot).slice(1), 16)
      : parseInt(WEAPONS[weapon]?.color.slice(1) ?? "ffffff", 16);
    const key = weapon * 1000 + bot;
    let s = this.sprites.get(id);
    if (!s) {
      const ball = new Graphics();
      const glow = new Sprite(this.glowTex);
      glow.anchor.set(0.5);
      glow.blendMode = "add";
      glow.alpha = 0.4;
      this.container.addChild(glow, ball);
      s = { ball, glow, key: -1 };
      this.sprites.set(id, s);
    }
    if (s.key !== key) {
      s.ball.clear();
      drawBullet(s.ball, weapon, col);
      s.ball.scale.set(this.enlarge);
      s.glow.tint = col;
      s.glow.scale.set((weapon === 3 ? 0.26 : weapon === 6 ? 0.24 : weapon === 1 ? 0.12 : 0.17) * this.enlarge);
      s.key = key;
    }
    s.ball.position.set(x, y);
    s.ball.rotation = Math.atan2(vy, vx);
    s.glow.position.set(x, y);
    s.ball.visible = true;
    s.glow.visible = true;
    onTrail(x, y, col, weapon === 3 || weapon === 6);
  }
}

/** Pickups: bobbing candy tins; gun pickups wear a bullet badge. */
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
        const isGun = kind >= 4;
        const col = parseInt((KIND_COLORS[kind] ?? "#ffffff").slice(1), 16);
        if (isGun) {
          // Beacon: a soft light pillar so a gun across the vision circle
          // reads as "something is here" before the tin itself resolves.
          const beam = new Sprite(this.stage.glowTex);
          beam.anchor.set(0.5, 0.72);
          beam.position.set(0, -8);
          beam.tint = col;
          beam.alpha = 0.4;
          beam.blendMode = "add";
          beam.scale.set(0.5, 2.1);
          root.addChild(beam);
        }
        const glow = new Sprite(this.stage.glowTex);
        glow.anchor.set(0.5);
        glow.tint = col;
        glow.alpha = isGun ? 0.45 : 0.3;
        glow.blendMode = "add";
        glow.scale.set(isGun ? 0.62 : 0.5);
        const g = new Graphics();
        if (isGun) {
          // Ammo tin: hexagon-ish tin + white bullet badge, extra sparkle.
          g.roundRect(-11, -9, 22, 18, 6).fill({ color: shade(col, 0.8) })
            .stroke({ width: 2.4, color: INK, alpha: 0.9 });
          g.roundRect(-5, -7, 10, 7, 2).fill({ color: 0xffffff });
          g.roundRect(-5.5, 0.5, 11, 6, 3).fill({ color: 0xffffff })
            .stroke({ width: 1.2, color: INK, alpha: 0.5 });
          g.circle(6.5, 6.5, 2.2).fill({ color: 0xffffff, alpha: 0.9 });
        } else {
          // Rounded candy tin + ink edge + glaze shine.
          g.roundRect(-9, -9, 18, 18, 7).fill({ color: col }).stroke({ width: 2.2, color: INK, alpha: 0.85 });
          g.roundRect(-5.5, -6, 11, 5, 2.5).fill({ color: 0xffffff, alpha: 0.6 });
        }
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

