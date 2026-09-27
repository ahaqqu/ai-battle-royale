/** Pixi stage with layered world container + smooth camera + shake. */

import { Application, Container, Graphics, Renderer, Texture } from "pixi.js";

export interface CameraTarget {
  x: number;
  y: number;
  zoom: number;
}

/** Radial glow texture, built once — used for every bloom in the scene. */
export function makeGlowTexture(_renderer: Renderer | null, size = 128, inner = "rgba(255,255,255,1)", mid = "rgba(255,255,255,0.35)"): Texture {
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = size;
  const ctx = canvas.getContext("2d")!;
  const g = ctx.createRadialGradient(size / 2, size / 2, 0, size / 2, size / 2, size / 2);
  g.addColorStop(0, inner);
  g.addColorStop(0.35, mid);
  g.addColorStop(1, "rgba(255,255,255,0)");
  ctx.fillStyle = g;
  ctx.fillRect(0, 0, size, size);
  return Texture.from(canvas);
}

/** Chunky rounded-square confetti piece, tinted per particle. */
export function makeConfettiTexture(size = 26, radius = 7): Texture {
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = size;
  const ctx = canvas.getContext("2d")!;
  ctx.fillStyle = "#ffffff";
  ctx.beginPath();
  ctx.roundRect(1, 1, size - 2, size - 2, radius);
  ctx.fill();
  return Texture.from(canvas);
}

/** Distance along a ray from (cx,cy) to the rect boundary. */
function rayToRect(x0: number, y0: number, x1: number, y1: number, cx: number, cy: number, dx: number, dy: number): number {
  let t = Infinity;
  if (dx > 1e-6) t = Math.min(t, (x1 - cx) / dx);
  else if (dx < -1e-6) t = Math.min(t, (x0 - cx) / dx);
  if (dy > 1e-6) t = Math.min(t, (y1 - cy) / dy);
  else if (dy < -1e-6) t = Math.min(t, (y0 - cy) / dy);
  return Number.isFinite(t) ? Math.max(0, t) : 4000;
}

/** Fill `g` with the region inside `bounds` but outside the shape whose
 * inner radius along each ray from (cx,cy) is `innerAt(angle)` — a polar
 * fan mesh. Pure geometry, so it works identically in WebGL/WebGPU/canvas
 * (blend-mode "erase" punches through the whole canvas and renders black
 * over opaque backgrounds, which the bright theme made obvious). */
export function drawOutsideOverlay(
  g: Graphics,
  x0: number, y0: number, x1: number, y1: number,
  cx: number, cy: number,
  innerAt: (angle: number) => number,
  color: number, alpha: number, segs = 96,
): void {
  for (let i = 0; i < segs; i++) {
    const a0 = (i / segs) * Math.PI * 2;
    const a1 = ((i + 1) / segs) * Math.PI * 2;
    const c0x = Math.cos(a0), c0y = Math.sin(a0);
    const c1x = Math.cos(a1), c1y = Math.sin(a1);
    const e0 = rayToRect(x0, y0, x1, y1, cx, cy, c0x, c0y);
    const e1 = rayToRect(x0, y0, x1, y1, cx, cy, c1x, c1y);
    const i0 = Math.min(innerAt(a0), e0);
    const i1 = Math.min(innerAt(a1), e1);
    if (i0 >= e0 && i1 >= e1) continue;
    g.moveTo(cx + c0x * i0, cy + c0y * i0)
      .lineTo(cx + c0x * e0, cy + c0y * e0)
      .lineTo(cx + c1x * e1, cy + c1y * e1)
      .lineTo(cx + c1x * i1, cy + c1y * i1)
      .closePath()
      .fill({ color, alpha });
  }
}

export class Stage {
  app: Application = new Application();
  didInit = false;
  world = new Container();
  bgLayer = new Container();
  wallLayer = new Container();
  zoneLayer = new Container();
  pickupLayer = new Container();
  trailLayer = new Container();
  unitLayer = new Container();
  projLayer = new Container();
  fxLayer = new Container();
  fogLayer = new Container();

  glowTex!: Texture;
  softTex!: Texture;
  confettiTex!: Texture;

  cam: CameraTarget = { x: 1600, y: 1600, zoom: 0.35 };
  private target: CameraTarget = { x: 1600, y: 1600, zoom: 0.35 };
  private shakeAmount = 0;
  private shakeX = 0;
  private shakeY = 0;

  onWheel?: (delta: number) => void;
  onDrag?: (dx: number, dy: number) => void;

  async init(host: HTMLElement, status?: (s: string) => void): Promise<void> {
    if (this.didInit) return;
    this.didInit = true;
    const mark = (m: string) => { try { status?.(m); } catch { /* noop */ } };
    mark('creating renderer…');
    this.app = new Application();
    // Bright Fall-Guys sky instead of deep space.
    await this.app.init({ resizeTo: window, background: 0x7ec9f5, antialias: true });
    mark('renderer ready');
    host.appendChild(this.app.canvas);

    this.glowTex = makeGlowTexture(null, 128, "rgba(255,255,255,0.9)", "rgba(255,255,255,0.28)");
    this.softTex = makeGlowTexture(null, 256, "rgba(255,255,255,0.55)", "rgba(255,255,255,0.16)");
    this.confettiTex = makeConfettiTexture();

    // Isolate the world as its own render group BEFORE first render —
    // avoids pixi v8 structure-rebuild misses for subtrees populated later.
    this.world.addChild(this.bgLayer, this.wallLayer, this.zoneLayer, this.pickupLayer, this.trailLayer, this.unitLayer, this.projLayer, this.fxLayer, this.fogLayer);
    this.app.stage.addChild(this.world);
    mark('layers ready');
    (window as unknown as { __stage?: Stage }).__stage = this;

    // Wheel zoom.
    host.addEventListener("wheel", (e) => {
      e.preventDefault();
      this.onWheel?.(e.deltaY);
    }, { passive: false });

    // Drag pan.
    let dragging = false;
    let lastX = 0, lastY = 0;
    host.addEventListener("pointerdown", (e) => {
      dragging = true; lastX = e.clientX; lastY = e.clientY;
    });
    window.addEventListener("pointermove", (e) => {
      if (!dragging) return;
      this.onDrag?.(e.clientX - lastX, e.clientY - lastY);
      lastX = e.clientX; lastY = e.clientY;
    });
    window.addEventListener("pointerup", () => { dragging = false; });

    this.app.ticker.add((t) => this.update(t.deltaMS / 1000));
  }

  setTarget(x: number, y: number, zoom: number, snap = false): void {
    this.target.x = x; this.target.y = y; this.target.zoom = zoom;
    if (snap) { this.cam.x = x; this.cam.y = y; this.cam.zoom = zoom; }
  }

  getTarget(): CameraTarget {
    return { ...this.target };
  }

  shake(amount: number): void {
    this.shakeAmount = Math.min(24, this.shakeAmount + amount);
  }

  private update(dt: number): void {
    const k = 1 - Math.exp(-dt * 4.5);
    this.cam.x += (this.target.x - this.cam.x) * k;
    this.cam.y += (this.target.y - this.cam.y) * k;
    this.cam.zoom += (this.target.zoom - this.cam.zoom) * k;

    this.shakeAmount *= Math.exp(-dt * 7);
    if (this.shakeAmount < 0.1) this.shakeAmount = 0;
    this.shakeX = (Math.random() * 2 - 1) * this.shakeAmount;
    this.shakeY = (Math.random() * 2 - 1) * this.shakeAmount;

    const w = this.app.screen.width;
    const h = this.app.screen.height;
    this.world.scale.set(this.cam.zoom);
    this.world.position.set(
      w / 2 - (this.cam.x + this.shakeX) * this.cam.zoom,
      h / 2 - (this.cam.y + this.shakeY) * this.cam.zoom,
    );
  }

  screenToWorld(sx: number, sy: number): { x: number; y: number } {
    const w = this.app.screen.width;
    const h = this.app.screen.height;
    return {
      x: (sx - w / 2) / this.cam.zoom + this.cam.x,
      y: (sy - h / 2) / this.cam.zoom + this.cam.y,
    };
  }
}
