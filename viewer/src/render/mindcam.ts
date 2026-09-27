/** Mind-cam overlay (PLAN §6.3): renders a bot's published belief heat map
 * plus its intent shout. "Where bot #7 thinks everyone is." */

import { Container, Graphics, Text } from "pixi.js";
import { botColor, FONT, INK_HEX } from "../types.js";

const GRID = 64;
const CELL = 3200 / GRID;

export class MindCam {
  private heat = new Graphics();
  private bubbles = new Map<number, Text>();
  private bubbleLayer: Container;
  visible = false;
  private lastStamp = -1;

  constructor(layer: Container, bubbleLayer: Container) {
    this.heat.alpha = 0.55;
    layer.addChild(this.heat);
    this.bubbleLayer = bubbleLayer;
  }

  toggle(): boolean {
    this.setVisible(!this.visible);
    return this.visible;
  }

  setVisible(v: boolean): void {
    this.visible = v;
    if (!v) {
      this.heat.clear();
      for (const t of this.bubbles.values()) t.visible = false;
    }
  }

  /** minds: bot → {intent, belief}; stamp = tick/5 (mind updates are rate-limited). */
  update(
    minds: Record<string, { intent?: string | null; belief?: number[] | null }>,
    stamp: number,
    botPositions: Map<number, { x: number; y: number }>,
  ): void {
    if (!this.visible) return;
    if (stamp !== this.lastStamp) {
      this.lastStamp = stamp;
      this.heat.clear();
      for (const [botKey, mind] of Object.entries(minds)) {
        if (!mind.belief) continue;
        const col = parseInt(botColor(Number(botKey)).slice(1), 16);
        for (let gy = 0; gy < GRID; gy++) {
          for (let gx = 0; gx < GRID; gx++) {
            const v = mind.belief[gy * GRID + gx] ?? 0;
            if (v < 8) continue;
            this.heat.rect(gx * CELL, gy * CELL, CELL, CELL).fill({
              color: col,
              alpha: Math.min(0.85, v / 255 + 0.08),
            });
          }
        }
      }
    }
    // Intent bubbles above each bot's main.
    const alive = new Set<number>();
    for (const [botKey, mind] of Object.entries(minds)) {
      const bot = Number(botKey);
      if (!mind.intent) continue;
      const pos = botPositions.get(bot);
      if (!pos) continue;
      alive.add(bot);
      let t = this.bubbles.get(bot);
      if (!t) {
        t = new Text({
          text: "",
          style: {
            fontFamily: FONT, fontSize: 12, fontWeight: "800",
            fill: 0xffffff, letterSpacing: 0.4,
            stroke: { color: INK_HEX, width: 3, join: "round" },
          },
        });
        t.anchor.set(0.5, 1);
        this.bubbleLayer.addChild(t);
        this.bubbles.set(bot, t);
      }
      if (t.text !== mind.intent) t.text = mind.intent;
      t.position.set(pos.x, pos.y - 30);
      t.visible = true;
    }
    for (const [bot, t] of this.bubbles) {
      if (!alive.has(bot)) t.visible = false;
    }
  }
}
