/** Static arena art, candy-arcade style: bright sky with soft clouds, a floating
 * pastel island floor with polka dots + sprinkles, and chunky rounded candy
 * walls with thick outlines. Drawn once per load. */

import { Graphics, Sprite } from "pixi.js";
import { ARENA, INK, MapData, shade } from "../types.js";
import { Stage } from "./stage.js";

/** Rotating candy-block colors for the big walls. */
const WALL_COLORS = [0xff6f91, 0xffa94d, 0x4fd0c0, 0x9d7bff];

/** Tiny deterministic pseudo-random (cloud placement is stable). */
function rand(seed: number): number {
  const x = Math.sin(seed * 127.1 + 311.7) * 43758.5453;
  return x - Math.floor(x);
}

export function drawArena(stage: Stage, map: MapData): void {
  const bg = stage.bgLayer;
  const size = map.size || ARENA;

  // Sun glow in the top-left sky.
  const sun = new Sprite(stage.softTex);
  sun.anchor.set(0.5);
  sun.position.set(size * 0.1, size * 0.08);
  sun.scale.set((size * 1.3) / 256);
  sun.tint = 0xffffff;
  sun.alpha = 0.85;
  bg.addChild(sun);

  // Puffy clouds drifting around the island (clusters of soft blobs).
  for (let i = 0; i < 9; i++) {
    const cx = rand(i * 3 + 1) * (size + 900) - 450;
    const cy = rand(i * 7 + 2) * (size + 900) - 450;
    const s = 0.8 + rand(i * 13 + 3) * 0.9;
    for (const [dx, dy, r] of [[-55, 8, 1], [0, -12, 1.35], [55, 10, 1]] as const) {
      const puff = new Sprite(stage.softTex);
      puff.anchor.set(0.5);
      puff.position.set(cx + dx * s, cy + dy * s);
      puff.scale.set((150 * r * s) / 256);
      puff.tint = 0xffffff;
      puff.alpha = 0.9;
      bg.addChild(puff);
    }
  }

  // Floating island: soft drop shadow, pastel slab, white rim.
  const floor = new Graphics();
  floor.roundRect(22, 30, size, size, 48).fill({ color: INK, alpha: 0.16 });
  floor.roundRect(0, 0, size, size, 44).fill({ color: 0xbfe4f6 });
  floor.roundRect(0, 0, size, size, 44).fill({ color: 0xcae9f8, alpha: 0.5 });
  bg.addChild(floor);

  // Polka-dot floor pattern (cell centers only, so corners stay clean).
  const dots = new Graphics();
  for (let y = 100; y < size; y += 200) {
    for (let x = 100; x < size; x += 200) {
      const major = (x % 800 === 100) && (y % 800 === 100);
      dots.circle(x, y, major ? 30 : 22).fill({ color: 0xffffff, alpha: major ? 0.5 : 0.32 });
    }
  }
  bg.addChild(dots);

  // Island rim: chunky white border with a soft ink outline.
  const rim = new Graphics();
  rim.roundRect(-10, -10, size + 20, size + 20, 52).stroke({ width: 14, color: 0xffffff, alpha: 0.97 });
  rim.roundRect(-20, -20, size + 40, size + 40, 60).stroke({ width: 2.5, color: INK, alpha: 0.22 });
  bg.addChild(rim);

  // Spawns: bouncy-looking pads (public knowledge, PLAN §2.4).
  const pads = new Graphics();
  for (const s of map.spawns) {
    pads.circle(s[0], s[1], 32).fill({ color: 0xffffff, alpha: 0.55 });
    pads.circle(s[0], s[1], 32).stroke({ width: 4, color: 0xffffff, alpha: 0.95 });
    pads.circle(s[0], s[1], 24).stroke({ width: 2, color: INK, alpha: 0.25 });
    pads.circle(s[0], s[1], 6).fill({ color: 0xffd1e6, alpha: 0.9 });
  }
  bg.addChild(pads);

  // Walls: chunky candy blocks with thick outlines and toy-like highlights.
  const walls = new Graphics();
  let i = 0;
  for (const w of map.walls) {
    const x = w.min[0], y = w.min[1];
    const hw = w.max[0] - w.min[0], hh = w.max[1] - w.min[1];
    if (w.kind === "wall") {
      const col = WALL_COLORS[i % WALL_COLORS.length];
      walls.roundRect(x + 7, y + 12, hw, hh, 20).fill({ color: INK, alpha: 0.16 });
      walls.roundRect(x, y, hw, hh, 20).fill({ color: col });
      walls.roundRect(x, y, hw, hh, 20).stroke({ width: 4, color: shade(col, 0.62) });
      walls.roundRect(x + 6, y + 6, Math.max(2, hw - 12), Math.max(2, Math.min(9, hh * 0.28)), 7)
        .fill({ color: 0xffffff, alpha: 0.4 });
    } else {
      // Low cover: pale foam bumper.
      walls.roundRect(x + 4, y + 7, hw, hh, 12).fill({ color: INK, alpha: 0.1 });
      walls.roundRect(x, y, hw, hh, 12).fill({ color: 0xfff1bf });
      walls.roundRect(x, y, hw, hh, 12).stroke({ width: 3, color: 0xf5b83d });
      walls.roundRect(x + 4, y + 4, Math.max(2, hw - 8), Math.max(2, Math.min(7, hh * 0.3)), 5)
        .fill({ color: 0xffffff, alpha: 0.55 });
    }
    i++;
  }
  stage.wallLayer.addChild(walls);
}
