/** Juicy-but-cheap particle system, Fall Guys style: pooled glow puffs plus
 * spinning confetti pieces with gravity. All feel, zero server cost (PLAN §7.3). */

import { Container, Graphics, Sprite } from "pixi.js";
import { BOT_COLORS, botColor } from "../types.js";
import { Stage } from "./stage.js";

interface Particle {
  sprite: Sprite;
  vx: number;
  vy: number;
  life: number;
  maxLife: number;
  size0: number;
  size1: number;
  drag: number;
  grav: number;
  spin: number;
}

interface Ring {
  g: Graphics;
  life: number;
  maxLife: number;
  r0: number;
  r1: number;
  width: number;
  color: number;
}

const POOL_SIZE = 520;

/** A random candy color (half the time white, for sparkle contrast). */
function candyColor(i: number): number {
  if (i % 3 === 0) return 0xffffff;
  return parseInt(BOT_COLORS[(i * 7 + 3) % BOT_COLORS.length].slice(1), 16);
}

export class Fx {
  private particles: Particle[] = [];
  private free: Sprite[] = [];
  private rings: Ring[] = [];
  private container: Container;
  private confettiN = 0;

  constructor(private stage: Stage) {
    this.container = stage.fxLayer;
    for (let i = 0; i < POOL_SIZE; i++) {
      const s = new Sprite(stage.glowTex);
      s.anchor.set(0.5);
      s.blendMode = "add";
      s.visible = false;
      this.container.addChild(s);
      this.free.push(s);
    }
  }

  private take(): Sprite | null {
    const s = this.free.pop();
    return s ?? null;
  }

  private spawn(x: number, y: number, tint: number, opts: Partial<Particle> & {
    count?: number; speed?: number; spread?: number; dirDeg?: number;
    shape?: "glow" | "confetti"; colors?: boolean;
  }): void {
    const count = opts.count ?? 8;
    const speed = opts.speed ?? 120;
    for (let i = 0; i < count; i++) {
      const s = this.take();
      if (!s) return;
      const dir = opts.dirDeg !== undefined
        ? ((opts.dirDeg + (Math.random() - 0.5) * (opts.spread ?? 360)) * Math.PI) / 180
        : Math.random() * Math.PI * 2;
      const v = speed * (0.4 + Math.random() * 0.8);
      if (opts.shape === "confetti") {
        s.texture = this.stage.confettiTex;
        s.blendMode = "normal";
        s.tint = opts.colors ? candyColor(this.confettiN++) : tint;
        s.rotation = Math.random() * Math.PI;
      } else {
        s.texture = this.stage.glowTex;
        s.blendMode = "add";
        s.tint = tint;
        s.rotation = 0;
      }
      const p: Particle = {
        sprite: s,
        vx: Math.cos(dir) * v,
        vy: Math.sin(dir) * v,
        life: 0,
        maxLife: opts.maxLife ?? 0.5,
        size0: opts.size0 ?? 10,
        size1: opts.size1 ?? 0,
        drag: opts.drag ?? 2.5,
        grav: opts.grav ?? 0,
        spin: opts.shape === "confetti" ? (Math.random() - 0.5) * 22 : 0,
      };
      s.position.set(x, y);
      s.visible = true;
      this.particles.push(p);
    }
  }

  private ring(x: number, y: number, color: number, r0: number, r1: number, life: number, width: number): void {
    const g = new Graphics();
    g.position.set(x, y);
    this.container.addChild(g);
    this.rings.push({ g, life: 0, maxLife: life, r0, r1, width, color });
  }

  /** Called once per crossed frame with that frame's events. */
  handleEvents(events: { type: string;[k: string]: unknown }[]): void {
    for (const e of events) {
      const at = e.at as [number, number] | undefined;
      const from = e.from as [number, number] | undefined;
      switch (e.type) {
        case "shot": {
          if (!from) break;
          const col = parseInt(botColor(e.bot as number).slice(1), 16);
          this.spawn(from[0], from[1], col, { count: 4, speed: 240, dirDeg: (e.dir as number), spread: 50, maxLife: 0.15, size0: 11, size1: 2 });
          this.spawn(from[0], from[1], 0xffffff, { count: 3, speed: 60, maxLife: 0.12, size0: 18, size1: 4 });
          break;
        }
        case "hit": {
          if (!at) break;
          const col = parseInt(botColor(e.bot as number).slice(1), 16);
          this.spawn(at[0], at[1], col, { count: 8, speed: 200, maxLife: 0.36, size0: 8, size1: 1 });
          this.spawn(at[0], at[1], col, { count: 4, shape: "confetti", colors: true, speed: 170, maxLife: 0.5, size0: 9, size1: 5, grav: 160, drag: 1.6 });
          this.spawn(at[0], at[1], 0xffffff, { count: 3, speed: 90, maxLife: 0.13, size0: 16, size1: 3 });
          this.stage.shake(1.2);
          break;
        }
        case "death": {
          if (!at) break;
          // The big confetti cannon.
          this.spawn(at[0], at[1], 0, {
            count: 34, shape: "confetti", colors: true, speed: 340, spread: 360,
            maxLife: 1.15, size0: 13, size1: 6, grav: 330, drag: 1.5,
          });
          const col = parseInt(botColor(e.bot as number).slice(1), 16);
          this.spawn(at[0], at[1], col, { count: 8, speed: 160, maxLife: 0.4, size0: 20, size1: 4 });
          this.ring(at[0], at[1], col, 10, 130, 0.55, 4);
          this.stage.shake(5);
          break;
        }
        case "companion_down": {
          if (!at) break;
          const col = parseInt(botColor(e.bot as number).slice(1), 16);
          this.spawn(at[0], at[1], col, { count: 8, shape: "confetti", colors: true, speed: 170, maxLife: 0.6, size0: 8, size1: 4, grav: 220, drag: 1.7 });
          break;
        }
        case "sonar": {
          if (!at) break;
          this.ring(at[0], at[1], 0x35c1f0, 8, 600, 0.9, 3);
          this.ring(at[0], at[1], 0x35c1f0, 4, 300, 0.6, 1.6);
          break;
        }
        case "pickup": {
          if (!at) break;
          this.spawn(at[0], at[1], 0xffc93c, { count: 10, shape: "confetti", colors: true, speed: 150, maxLife: 0.6, size0: 9, size1: 4, grav: 180, drag: 1.8 });
          this.ring(at[0], at[1], 0xffc93c, 4, 60, 0.35, 2.5);
          break;
        }
        case "zone_locked": {
          // Handled by HUD banner; small global pulse.
          this.stage.shake(2);
          break;
        }
      }
    }
  }

  /** Projectile tracers for visible projectiles — call every rendered frame. */
  tracer(x: number, y: number, color: number, intense: boolean): void {
    const s = this.take();
    if (!s) return;
    const p: Particle = {
      sprite: s, vx: 0, vy: 0, life: 0,
      maxLife: intense ? 0.26 : 0.16,
      size0: intense ? 11 : 7, size1: 0, drag: 0, grav: 0, spin: 0,
    };
    s.texture = this.stage.glowTex;
    s.blendMode = "add";
    s.rotation = 0;
    s.position.set(x, y);
    s.tint = color;
    s.visible = true;
    this.particles.push(p);
  }

  update(dt: number): void {
    for (let i = this.particles.length - 1; i >= 0; i--) {
      const p = this.particles[i];
      p.life += dt;
      if (p.life >= p.maxLife) {
        p.sprite.visible = false;
        this.free.push(p.sprite);
        this.particles.splice(i, 1);
        continue;
      }
      const t = p.life / p.maxLife;
      const dragK = Math.exp(-p.drag * dt);
      p.vx *= dragK;
      p.vy *= dragK;
      p.vy += p.grav * dt;
      p.sprite.x += p.vx * dt;
      p.sprite.y += p.vy * dt;
      if (p.spin !== 0) p.sprite.rotation += p.spin * dt;
      p.sprite.alpha = 1 - t;
      const base = p.sprite.texture === this.stage.confettiTex ? 26 : 128;
      p.sprite.scale.set((p.size0 + (p.size1 - p.size0) * t) / base);
    }
    for (let i = this.rings.length - 1; i >= 0; i--) {
      const r = this.rings[i];
      r.life += dt;
      if (r.life >= r.maxLife) {
        r.g.destroy();
        this.rings.splice(i, 1);
        continue;
      }
      const t = r.life / r.maxLife;
      const radius = r.r0 + (r.r1 - r.r0) * (1 - (1 - t) * (1 - t));
      r.g.clear();
      r.g.circle(0, 0, radius).stroke({ width: r.width * (1 - t) + 0.5, color: r.color, alpha: 0.9 * (1 - t) });
    }
  }
}
