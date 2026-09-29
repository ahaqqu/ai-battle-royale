/** Juicy-but-cheap particle system, candy-arcade style: pooled glow puffs plus
 * spinning confetti pieces with gravity. All feel, zero server cost (PLAN §7.3).
 * Tuned LOUD: deaths are bombastic set pieces (core flash + shockwaves +
 * confetti cannon + embers + smoke), every action pops at a glance. */

import { Container, Graphics, Sprite } from "pixi.js";
import { BOT_COLORS, botColor, WEAPONS, weaponIdx } from "../types.js";
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
  alphaMax: number;
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

const POOL_SIZE = 900;

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
    shape?: "glow" | "confetti" | "smoke"; colors?: boolean;
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
      } else if (opts.shape === "smoke") {
        s.texture = this.stage.softTex;
        s.blendMode = "normal";
        s.tint = tint;
        s.rotation = 0;
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
        alphaMax: opts.alphaMax ?? 1,
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
          // Sim bearings are 0 = +Y screen-down; screen angle = 90 − bearing.
          const dirScreen = 90 - (e.dir as number);
          const w = weaponIdx(typeof e.weapon === "string" ? e.weapon : undefined);
          const col = w === 0
            ? parseInt(botColor(e.bot as number).slice(1), 16)
            : parseInt(WEAPONS[w].color.slice(1), 16);
          this.muzzleFlash(from[0], from[1], dirScreen, w, col);
          break;
        }
        case "bounce": {
          if (!at) break;
          this.bounce(at[0], at[1]);
          break;
        }
        case "explosion": {
          if (!at) break;
          this.explosion(at[0], at[1], (e.radius as number) || 90);
          break;
        }
        case "hit": {
          if (!at) break;
          const col = parseInt(botColor(e.bot as number).slice(1), 16);
          this.spawn(at[0], at[1], col, { count: 12, speed: 260, maxLife: 0.4, size0: 9, size1: 1 });
          this.spawn(at[0], at[1], col, { count: 6, shape: "confetti", colors: true, speed: 220, maxLife: 0.55, size0: 10, size1: 5, grav: 180, drag: 1.6 });
          this.spawn(at[0], at[1], 0xffffff, { count: 4, speed: 110, maxLife: 0.15, size0: 20, size1: 4 });
          this.stage.shake(2.4);
          break;
        }
        case "death": {
          if (!at) break;
          this.deathBlast(at[0], at[1], e.bot as number);
          break;
        }
        case "dash": {
          if (!at) break;
          const col = parseInt(botColor(e.bot as number).slice(1), 16);
          this.dashStreak(at[0], at[1], e.dir as number, col);
          break;
        }
        case "shield": {
          if (!at) break;
          this.shieldPop(at[0], at[1]);
          break;
        }
        case "companion_down": {
          if (!at) break;
          const col = parseInt(botColor(e.bot as number).slice(1), 16);
          this.spawn(at[0], at[1], 0xffffff, { count: 1, speed: 0, maxLife: 0.24, size0: 10, size1: 130, drag: 0 });
          this.spawn(at[0], at[1], col, { count: 22, shape: "confetti", colors: true, speed: 260, maxLife: 0.8, size0: 10, size1: 5, grav: 260, drag: 1.6 });
          this.ring(at[0], at[1], col, 6, 110, 0.45, 4);
          this.stage.shake(3.5);
          break;
        }
        case "companion_back": {
          if (!at) break;
          this.sparkle(at[0], at[1]);
          break;
        }
        case "projectile_end": {
          if (!at) break;
          // Wall thud: gray chip puff; range fizzle: a softer fading blink.
          const wall = e.wall === true;
          this.spawn(at[0], at[1], wall ? 0x9a94b8 : 0xffffff, {
            count: wall ? 6 : 3, speed: wall ? 130 : 60,
            maxLife: wall ? 0.24 : 0.18, size0: wall ? 10 : 8, size1: 2, alphaMax: 0.75,
          });
          break;
        }
        case "pickup": {
          if (!at) break;
          this.spawn(at[0], at[1], 0xffc93c, { count: 14, shape: "confetti", colors: true, speed: 210, maxLife: 0.7, size0: 10, size1: 4, grav: 200, drag: 1.8 });
          this.spawn(at[0], at[1], 0xffffff, { count: 4, speed: 80, maxLife: 0.16, size0: 18, size1: 4 });
          this.ring(at[0], at[1], 0xffc93c, 4, 80, 0.45, 3.5);
          break;
        }
        case "zone_shrink_started": {
          const c = e.center as [number, number] | undefined;
          if (!c) break;
          const r = e.radius as number;
          this.ring(c[0], c[1], 0xff5fae, r * 0.85, r * 1.06, 0.85, 6);
          this.ring(c[0], c[1], 0xffffff, r * 0.6, r * 0.98, 0.6, 3);
          this.stage.shake(3);
          break;
        }
        case "match_ended": {
          // Winner celebration: confetti rains across the whole viewport.
          this.confettiRain();
          this.stage.shake(4);
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

  /** The big one: white-hot core flash, triple shockwave, confetti cannon,
   * glowing embers and rising smoke. Called from the death event and from
   * play mode (kill feed / own elimination). */
  deathBlast(x: number, y: number, bot: number): void {
    const col = parseInt(botColor(bot).slice(1), 16);
    this.spawn(x, y, 0xffffff, { count: 1, speed: 0, maxLife: 0.3, size0: 30, size1: 330, drag: 0 });
    this.spawn(x, y, col, { count: 1, speed: 0, maxLife: 0.42, size0: 18, size1: 240, drag: 0 });
    this.spawn(x, y, 0, { count: 60, shape: "confetti", colors: true, speed: 460, maxLife: 1.35, size0: 15, size1: 7, grav: 360, drag: 1.4 });
    this.spawn(x, y, col, { count: 12, speed: 280, maxLife: 0.55, size0: 16, size1: 2, grav: 340, drag: 1.8 });
    this.spawn(x, y, 0x6b6f87, { count: 7, shape: "smoke", speed: 34, maxLife: 0.9, size0: 14, size1: 30, drag: 1.2, grav: -46, alphaMax: 0.4 });
    this.ring(x, y, 0xffffff, 10, 170, 0.42, 6.5);
    this.ring(x, y, col, 14, 250, 0.6, 5);
    this.ring(x, y, col, 30, 330, 0.85, 2.5);
    this.stage.shake(13);
  }

  /** Dash launch: exhaust cone blown backward + snap ring + white pop. */
  dashStreak(x: number, y: number, dirDeg: number, col: number): void {
    this.spawn(x, y, col, { count: 12, speed: 300, dirDeg: 90 - dirDeg + 180, spread: 42, maxLife: 0.3, size0: 13, size1: 3, drag: 3 });
    this.spawn(x, y, 0xffffff, { count: 4, speed: 120, maxLife: 0.14, size0: 14, size1: 3 });
    this.ring(x, y, col, 6, 64, 0.3, 3);
  }

  /** Shield raise: glass pop ring + sparkles (matches the bubble in units.ts). */
  shieldPop(x: number, y: number): void {
    this.ring(x, y, 0x9fe0ff, 6, 70, 0.35, 4);
    this.ring(x, y, 0xffffff, 4, 44, 0.25, 2.5);
    this.spawn(x, y, 0xbfe9ff, { count: 10, speed: 190, maxLife: 0.35, size0: 9, size1: 2 });
  }

  /** Muzzle flash: directional cone + white core, scaled per gun (Gungeon:
   * the shot itself must read as an event even before the bullet flies). */
  muzzleFlash(x: number, y: number, dirDeg: number, weapon = 0, col = 0xffffff): void {
    const s = weapon === 1 ? 0.55 : weapon === 2 ? 1.6 : weapon === 3 ? 1.3 : weapon === 6 ? 1.25 : 1;
    const spread = weapon === 2 ? 46 : weapon === 3 ? 8 : weapon === 1 ? 16 : 26;
    this.spawn(x, y, col, {
      count: Math.round(6 * s) + 2, speed: 330, dirDeg, spread,
      maxLife: 0.14, size0: 11 * s + 4, size1: 2,
    });
    this.spawn(x, y, 0xffffff, { count: 3, speed: 70, maxLife: 0.1, size0: 22 * s + 6, size1: 6 });
    if (weapon === 3) {
      // Lance: a hot streak down the barrel.
      this.spawn(x, y, 0xffffff, { count: 4, speed: 700, dirDeg, spread: 5, maxLife: 0.16, size0: 16, size1: 2 });
    }
    this.ring(x, y, col, 4, 34 * s + 10, 0.18, 2.5);
  }

  /** Bouncer ricochet: rubbery ping puff on the wall. */
  bounce(x: number, y: number): void {
    this.spawn(x, y, 0x9fe0ff, { count: 6, speed: 160, maxLife: 0.2, size0: 8, size1: 2 });
    this.ring(x, y, 0x35d6b5, 3, 30, 0.22, 2.5);
  }

  /** Pop Rock detonation: orange blast core + twin rings + smoke. */
  explosion(x: number, y: number, radius = 90): void {
    this.spawn(x, y, 0xffffff, { count: 1, speed: 0, maxLife: 0.22, size0: 26, size1: 110, drag: 0 });
    this.spawn(x, y, 0xff6a00, { count: 22, speed: 270, maxLife: 0.45, size0: 12, size1: 2 });
    this.spawn(x, y, 0xffd93b, { count: 10, speed: 160, maxLife: 0.3, size0: 10, size1: 2 });
    this.spawn(x, y, 0x6b6f87, { count: 4, shape: "smoke", speed: 40, maxLife: 0.7, size0: 12, size1: 26, drag: 1.2, grav: -40, alphaMax: 0.4 });
    this.ring(x, y, 0xff6a00, 8, radius, 0.4, 5);
    this.ring(x, y, 0xffd93b, 4, radius * 0.6, 0.28, 3);
    this.stage.shake(6);
  }

  /** Companion respawn / revive sparkle: happy candy fountain. */
  sparkle(x: number, y: number): void {
    this.spawn(x, y, 0, { count: 14, shape: "confetti", colors: true, speed: 230, maxLife: 0.8, size0: 10, size1: 5, grav: 240, drag: 1.6 });
    this.ring(x, y, 0xffc93c, 4, 70, 0.5, 3);
    this.spawn(x, y, 0xffffff, { count: 4, speed: 70, maxLife: 0.2, size0: 16, size1: 4 });
  }

  /** Little dust puff at the feet of a sprinting unit (called sparsely). */
  dust(x: number, y: number): void {
    this.spawn(x, y, 0xffffff, { count: 1, shape: "smoke", speed: 18, maxLife: 0.35, size0: 5, size1: 11, drag: 2, alphaMax: 0.35 });
  }

  /** Full-viewport candy rain (match end). */
  confettiRain(): void {
    const cam = this.stage.cam;
    const w = this.stage.app.screen.width;
    const h = this.stage.app.screen.height;
    const halfW = w / cam.zoom / 2;
    const top = cam.y - h / cam.zoom / 2;
    const left = cam.x - halfW;
    for (let i = 0; i < 90; i++) {
      const s = this.take();
      if (!s) return;
      s.texture = this.stage.confettiTex;
      s.blendMode = "normal";
      s.tint = candyColor(this.confettiN++);
      s.rotation = Math.random() * Math.PI;
      const vy = 320 + Math.random() * 220;
      const x = left + Math.random() * halfW * 2;
      const y = top - Math.random() * 300;
      s.position.set(x, y);
      s.visible = true;
      this.particles.push({
        sprite: s,
        vx: (Math.random() - 0.5) * 70,
        vy,
        life: 0,
        maxLife: (h / cam.zoom + 420) / vy,
        size0: 13, size1: 9,
        drag: 0.05, grav: 130,
        spin: (Math.random() - 0.5) * 18,
        alphaMax: 1,
      });
    }
  }

  /** Hit-confirm: white X-shaped spark burst where YOUR shot landed. */
  hitmark(x: number, y: number): void {
    this.spawn(x, y, 0xffffff, { count: 10, speed: 240, maxLife: 0.24, size0: 10, size1: 1 });
    this.ring(x, y, 0xffffff, 3, 40, 0.26, 3);
    this.stage.shake(0.9);
  }

  /** Projectile tracers for visible projectiles — call every rendered frame. */
  tracer(x: number, y: number, color: number, intense: boolean): void {
    const s = this.take();
    if (!s) return;
    const p: Particle = {
      sprite: s, vx: 0, vy: 0, life: 0,
      maxLife: intense ? 0.3 : 0.18,
      size0: intense ? 15 : 9, size1: 0, drag: 0, grav: 0, spin: 0,
      alphaMax: 1,
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
      p.sprite.alpha = (1 - t) * p.alphaMax;
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
