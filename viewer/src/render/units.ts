/** Unit sprites: glowing mains (arrow + bloom + hp bar + name) and
 * companions (orb + orbit dot). Prebuilt for all slots, updated per frame
 * from the compact frame arrays. Frame slots follow the Rust layout:
 * [main₀, comp₀, main₁, comp₁, …] — slot i → bot i>>1, main if i&1==0. */

import { Container, Graphics, Sprite, Text } from "pixi.js";
import { botColor, U, UNIT_STRIDE, UF_ALIVE, UF_DASH, UF_SHIELD, UF_SPRINT } from "../types.js";
import { Stage } from "./stage.js";

const MAIN_R = 14;
const COMP_R = 10;

function mainShape(color: number): Graphics {
  const g = new Graphics();
  // Arrow pointing +X; rotated to facing later.
  g.moveTo(MAIN_R + 5, 0)
    .lineTo(-MAIN_R * 0.85, MAIN_R * 0.85)
    .lineTo(-MAIN_R * 0.35, 0)
    .lineTo(-MAIN_R * 0.85, -MAIN_R * 0.85)
    .closePath()
    .fill({ color: 0x0c1220, alpha: 0.9 })
    .stroke({ width: 2.4, color, alpha: 1 });
  g.circle(0, 0, 3.2).fill({ color });
  return g;
}

function compShape(color: number): Graphics {
  const g = new Graphics();
  g.circle(0, 0, COMP_R).fill({ color: 0x0c1220, alpha: 0.9 }).stroke({ width: 2, color, alpha: 1 });
  g.circle(0, 0, 2.4).fill({ color });
  return g;
}

export class UnitView {
  root = new Container();
  glow: Sprite;
  shape: Graphics;
  shield: Graphics;
  hpBar: Graphics;
  name: Text;
  orbit: Graphics;
  kindMain: boolean;
  bot: number;

  constructor(stage: Stage, slot: number, name: string) {
    this.bot = slot >> 1;
    this.kindMain = (slot & 1) === 0;
    const colHex = botColor(this.bot);
    const col = parseInt(colHex.slice(1), 16);

    this.glow = new Sprite(stage.glowTex);
    this.glow.anchor.set(0.5);
    this.glow.tint = col;
    this.glow.blendMode = "add";

    this.shape = this.kindMain ? mainShape(col) : compShape(col);
    this.shield = new Graphics();
    this.hpBar = new Graphics();
    this.orbit = new Graphics();

    this.name = new Text({
      text: name,
      style: { fontFamily: "Inter, sans-serif", fontSize: 11, fontWeight: "700", fill: colHex, letterSpacing: 0.5 },
    });
    this.name.anchor.set(0.5, 1);
    this.name.alpha = 0.85;

    this.root.addChild(this.glow, this.shape, this.orbit, this.shield, this.hpBar, this.name);
    this.root.visible = false;
    stage.unitLayer.addChild(this.root);
  }

  update(units: Float32Array, slot: number, showNames: boolean): void {
    const o = slot * UNIT_STRIDE;
    const flags = units[o + U.FLAGS];
    const alive = (flags & UF_ALIVE) !== 0;
    this.root.visible = alive;
    if (!alive) return;
    this.root.position.set(units[o + U.X], units[o + U.Y]);
    const hp01 = units[o + U.HP01];
    if (this.kindMain) {
      this.shape.rotation = (units[o + U.FACING] * Math.PI) / 180;
      this.glow.scale.set(1.9 + ((flags & UF_SPRINT) !== 0 ? 0.35 : 0) + ((flags & UF_DASH) !== 0 ? 0.7 : 0));
      this.glow.alpha = 0.42 + ((flags & UF_DASH) !== 0 ? 0.25 : 0);
      const w = 34;
      this.hpBar.clear();
      this.hpBar.rect(-w / 2, -MAIN_R - 13, w, 3.6).fill({ color: 0x0a0f1e, alpha: 0.85 });
      this.hpBar.rect(-w / 2, -MAIN_R - 13, w * hp01, 3.6).fill({
        color: hp01 > 0.55 ? 0x58ff9b : hp01 > 0.25 ? 0xffd54f : 0xff4f6d,
      });
      this.name.position.set(0, -MAIN_R - 16);
      this.name.visible = showNames;
      this.shield.clear();
      if ((flags & UF_SHIELD) !== 0) {
        this.shield.circle(0, 0, MAIN_R + 7).stroke({ width: 1.6, color: 0x9fd8ff, alpha: 0.85 });
        this.shield.circle(0, 0, MAIN_R + 11).stroke({ width: 0.8, color: 0x9fd8ff, alpha: 0.35 });
      }
    } else {
      this.glow.scale.set(1.15);
      this.glow.alpha = 0.3;
      const t = performance.now() / 1000;
      this.orbit.clear();
      this.orbit.circle(Math.cos(t * 3) * 16, Math.sin(t * 3) * 16, 1.8).fill({ color: 0xbfd8ff, alpha: 0.8 });
      const w = 22;
      this.hpBar.clear();
      this.hpBar.rect(-w / 2, -COMP_R - 9, w, 2.6).fill({ color: 0x0a0f1e, alpha: 0.85 });
      this.hpBar.rect(-w / 2, -COMP_R - 9, w * hp01, 2.6).fill({ color: hp01 > 0.5 ? 0x58ff9b : 0xff4f6d });
      this.name.visible = false;
    }
  }
}

export class UnitViews {
  views: UnitView[] = [];

  constructor(stage: Stage, names: string[]) {
    for (let i = 0; i < names.length * 2; i++) {
      const bot = i >> 1;
      const isMain = (i & 1) === 0;
      this.views.push(new UnitView(stage, i, isMain ? names[bot] : `${names[bot]}·pet`));
    }
  }

  update(units: Float32Array, unitCount: number, showNames: boolean): void {
    const slots = Math.min(unitCount, units.length / UNIT_STRIDE);
    for (let i = 0; i < slots; i++) {
      this.views[i]?.update(units, i, showNames);
    }
  }
}
