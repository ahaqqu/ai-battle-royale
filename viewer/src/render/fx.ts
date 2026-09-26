/** Juicy-but-cheap particle system: pooled additive sprites + expanding
 * rings. All feel, zero server cost (PLAN §7.3). */

import { Container, Graphics, Sprite } from "pixi.js";
import { botColor } from "../types.js";
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
}

interface Ring {
  g: Graphics;
  life: number;
  maxLife: number;
  r0: number;
  r1: number;
  width: number;
}

const POOL_SIZE = 420;

export class Fx {
  private particles: Particle[] = [];
  private free: Sprite[] = [];
  private rings: Ring[] = [];
  private container: Container;

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

  private spawn(x: number, y: number, tint: number, opts: Partial<Particle> & { count?: number; speed?: number; spread?: number; dirDeg?: number }): void {
    const count = opts.count ?? 8;
    const speed = opts.speed ?? 120;
    for (let i = 0; i < count; i++) {
      const s = this.free.pop();
      if (!s) return;
      const dir = opts.dirDeg !== undefined
        ? ((opts.dirDeg + (Math.random() - 0.5) * (opts.spread ?? 360)) * Math.PI) / 180
        : Math.random() * Math.PI * 2;
      const v = speed * (0.4 + Math.random() * 0.8);
      const p: Particle = {
        sprite: s,
        vx: Math.cos(dir) * v,
        vy: Math.sin(dir) * v,
        life: 0,
        maxLife: opts.maxLife ?? 0.5,
        size0: opts.size0 ?? 10,
        size1: opts.size1 ?? 0,
        drag: opts.drag ?? 2.5,
      };
      s.position.set(x, y);
      s.tint = tint;
      s.visible = true;
      this.particles.push(p);
    }
  }

  private ring(x: number, y: number, _color: number, r0: number, r1: number, life: number, width: number): void {
    const g = new Graphics();
    g.position.set(x, y);
    this.container.addChild(g);
    this.rings.push({ g, life: 0, maxLife: life, r0, r1, width });
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
          const dir = ((e.dir as number) * Math.PI) / 180;
          this.spawn(from[0], from[1], col, { count: 5, speed: 260, dirDeg: (e.dir as number), spread: 50, maxLife: 0.16, size0: 12, size1: 2 });
          this.spawn(from[0], from[1], 0xffffff, { count: 3, speed: 60, maxLife: 0.12, size0: 22, size1: 4 });
          void dir;
          break;
        }
        case "hit": {
          if (!at) break;
          const col = parseInt(botColor(e.bot as number).slice(1), 16);
          this.spawn(at[0], at[1], col, { count: 10, speed: 200, maxLife: 0.38, size0: 9, size1: 1 });
          this.spawn(at[0], at[1], 0xffffff, { count: 3, speed: 90, maxLife: 0.14, size0: 18, size1: 3 });
          this.stage.shake(1.6);
          break;
        }
        case "death": {
          if (!at) break;
          const col = parseInt(botColor(e.bot as number).slice(1), 16);
          this.spawn(at[0], at[1], col, { count: 26, speed: 320, maxLife: 0.8, size0: 14, size1: 2, drag: 1.8 });
          this.spawn(at[0], at[1], 0xffffff, { count: 8, speed: 140, maxLife: 0.4, size0: 24, size1: 4 });
          this.ring(at[0], at[1], col, 10, 130, 0.55, 3);
          this.stage.shake(7);
          break;
        }
        case "companion_down": {
          if (!at) break;
          const col = parseInt(botColor(e.bot as number).slice(1), 16);
          this.spawn(at[0], at[1], col, { count: 10, speed: 160, maxLife: 0.5, size0: 8, size1: 1 });
          break;
        }
        case "sonar": {
          if (!at) break;
          this.ring(at[0], at[1], 0x55e6ff, 8, 600, 0.9, 2.4);
          this.ring(at[0], at[1], 0x55e6ff, 4, 300, 0.6, 1.4);
          break;
        }
        case "pickup": {
          if (!at) break;
          this.spawn(at[0], at[1], 0xffd54f, { count: 12, speed: 130, maxLife: 0.4, size0: 10, size1: 2 });
          this.ring(at[0], at[1], 0xffd54f, 4, 60, 0.35, 2);
          break;
        }
        case "zone_locked": {
          // Handled by HUD banner; small global pulse.
          this.stage.shake(2.5);
          break;
        }
      }
    }
  }

  /** Projectile tracers for visible projectiles — call every rendered frame. */
  tracer(x: number, y: number, color: number, intense: boolean): void {
    const s = this.free.pop();
    if (!s) return;
    const p: Particle = {
      sprite: s, vx: 0, vy: 0, life: 0,
      maxLife: intense ? 0.28 : 0.18,
      size0: intense ? 13 : 8, size1: 0, drag: 0,
    };
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
      p.sprite.x += p.vx * dt;
      p.sprite.y += p.vy * dt;
      p.sprite.alpha = 1 - t;
      p.sprite.scale.set((p.size0 + (p.size1 - p.size0) * t) / 128);
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
      r.g.circle(0, 0, radius).stroke({ width: r.width * (1 - t) + 0.5, color: 0xffffff, alpha: 0.9 * (1 - t) });
      r.g.tint = 0x55e6ff;
    }
  }
}
