/** Static arena art: deep-space background, grid, walls with neon edges,
 * arena frame. Drawn once per load. */

import { Graphics, Sprite } from "pixi.js";
import { ARENA, MapData } from "../types.js";
import { Stage } from "./stage.js";

export function drawArena(stage: Stage, map: MapData): void {
  const bg = stage.bgLayer;
  const size = map.size || ARENA;

  // Soft nebula backdrop.
  const neb = new Sprite(stage.softTex);
  neb.anchor.set(0.5);
  neb.position.set(size / 2, size / 2);
  neb.scale.set((size * 1.6) / 256);
  neb.tint = 0x1a2450;
  neb.alpha = 0.55;
  bg.addChild(neb);
  const neb2 = new Sprite(stage.softTex);
  neb2.anchor.set(0.5);
  neb2.position.set(size * 0.2, size * 0.75);
  neb2.scale.set((size * 1.1) / 256);
  neb2.tint = 0x301b4d;
  neb2.alpha = 0.35;
  bg.addChild(neb2);
  const neb3 = new Sprite(stage.softTex);
  neb3.anchor.set(0.5);
  neb3.position.set(size * 0.85, size * 0.25);
  neb3.scale.set((size * 0.9) / 256);
  neb3.tint = 0x0d2f3f;
  neb3.alpha = 0.4;
  bg.addChild(neb3);

  // Arena floor — slightly lighter than the void outside the map.
  const floor = new Graphics();
  floor.rect(0, 0, size, size).fill({ color: 0x0c1220 });
  bg.addChild(floor);

  // Grid: minor every 200u, major every 800u.
  const grid = new Graphics();
  for (let i = 0; i <= size; i += 200) {
    const major = i % 800 === 0;
    grid.moveTo(i, 0).lineTo(i, size).stroke({ width: 1, color: major ? 0x25335c : 0x141b31, alpha: major ? 0.55 : 0.4 });
    grid.moveTo(0, i).lineTo(size, i).stroke({ width: 1, color: major ? 0x25335c : 0x141b31, alpha: major ? 0.55 : 0.4 });
  }
  bg.addChild(grid);

  // Arena frame glow.
  const frame = new Graphics();
  frame.rect(0, 0, size, size).stroke({ width: 3, color: 0x3d5afe, alpha: 0.8 });
  frame.rect(-6, -6, size + 12, size + 12).stroke({ width: 1, color: 0x55e6ff, alpha: 0.25 });
  bg.addChild(frame);

  // Spawns: subtle pads (public knowledge, PLAN §2.4).
  const pads = new Graphics();
  for (const s of map.spawns) {
    pads.circle(s[0], s[1], 22).stroke({ width: 1, color: 0x2a3a66, alpha: 0.7 });
    pads.circle(s[0], s[1], 3).fill({ color: 0x2a3a66, alpha: 0.9 });
  }
  bg.addChild(pads);

  // Walls.
  const walls = new Graphics();
  for (const w of map.walls) {
    const x = w.min[0], y = w.min[1];
    const hw = w.max[0] - w.min[0], hh = w.max[1] - w.min[1];
    if (w.kind === "wall") {
      walls.rect(x, y, hw, hh).fill({ color: 0x131a30 });
      walls.rect(x, y, hw, hh).stroke({ width: 2, color: 0x3f57a8, alpha: 0.95 });
      // Top highlight strip for a slight 2.5D read.
      walls.rect(x + 2, y + 2, hw - 4, 2).fill({ color: 0x6d86d8, alpha: 0.5 });
    } else {
      // Low cover: shorter, warmer, dashed edge.
      walls.rect(x, y, hw, hh).fill({ color: 0x1a1f33 });
      const dash = 14;
      const per = (hw + hh) * 2;
      for (let d = 0; d < per; d += dash * 2) {
        let rem = dash;
        // top
        if (d < hw) { const sx = Math.min(x + d, x + hw); walls.moveTo(sx, y).lineTo(Math.min(sx + rem, x + hw), y).stroke({ width: 2, color: 0x8a6a3a, alpha: 0.9 }); }
        const d1 = d - hw;
        if (d1 >= 0 && d1 < hh) { const sy = Math.min(y + d1, y + hh); walls.moveTo(x + hw, sy).lineTo(x + hw, Math.min(sy + rem, y + hh)).stroke({ width: 2, color: 0x8a6a3a, alpha: 0.9 }); }
        const d2 = d1 - hh;
        if (d2 >= 0 && d2 < hw) { const sx = Math.max(x + hw - d2, x); walls.moveTo(sx, y + hh).lineTo(Math.max(sx - rem, x), y + hh).stroke({ width: 2, color: 0x8a6a3a, alpha: 0.9 }); }
        const d3 = d2 - hw;
        if (d3 >= 0 && d3 < hh) { const sy = Math.max(y + hh - d3, y); walls.moveTo(x, sy).lineTo(x, Math.max(sy - rem, y)).stroke({ width: 2, color: 0x8a6a3a, alpha: 0.9 }); }
        void rem;
      }
    }
  }
  stage.wallLayer.addChild(walls);
}
