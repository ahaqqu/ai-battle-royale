/** Pixi stage with layered world container + smooth camera + shake. */

import { Application, Container, Renderer, Texture } from "pixi.js";

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
    await this.app.init({ resizeTo: window, background: 0x070a14, antialias: true });
    mark('renderer ready');
    host.appendChild(this.app.canvas);

    this.glowTex = makeGlowTexture(null, 128, "rgba(255,255,255,0.9)", "rgba(255,255,255,0.28)");
    this.softTex = makeGlowTexture(null, 256, "rgba(255,255,255,0.55)", "rgba(255,255,255,0.16)");

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
