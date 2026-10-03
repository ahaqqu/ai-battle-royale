/** Player-cam rendering: strict-fog view through one bot's eyes — vision
 * holes punched out of a dark overlay, silhouettes vs full detail, and
 * audio-bearing wedges for heard events. */

import { Container, Graphics, Text } from "pixi.js";
import { botColor, CamFrame, FONT, INK_HEX } from "../types.js";
import { drawOutsideOverlay, Stage } from "./stage.js";

const MAIN_VISION = 450;
const COMP_VISION = 250;
/** Gunshots are deliberately absent: heard-gunshot wedges strobed red every
 * observation and read as noise, so gunfire intel is client-off. The quieter
 * close-range cues stay — they are the flank warning. */
const KIND_COLORS: Record<string, number> = {
  dash: 0xff9d3b,
  footstep: 0xffd93b,
};

export class FogView {
  private overlay = new Graphics();
  private marks = new Graphics();
  private heardWedges = new Graphics();
  private emptyHint: Text;

  constructor(stage: Stage) {
    stage.fogLayer.addChild(this.overlay, this.marks, this.heardWedges);
    this.emptyHint = new Text({
      text: "FOLLOWING BOT VISION — what this bot can see and hear",
      style: {
        fontFamily: FONT, fontSize: 13, fontWeight: "700",
        fill: 0xffffff, letterSpacing: 2,
        stroke: { color: INK_HEX, width: 3.5, join: "round" },
      },
    });
    this.emptyHint.anchor.set(0.5);
    stage.fogLayer.addChild(this.emptyHint);
    this.hide();
  }

  show(): void {
    this.overlay.visible = true;
    this.marks.visible = true;
    this.heardWedges.visible = true;
    this.emptyHint.visible = true;
  }

  hide(): void {
    this.overlay.visible = false;
    this.marks.visible = false;
    this.heardWedges.visible = false;
    this.emptyHint.visible = false;
  }

  update(cam: CamFrame, viewerBot: number): void {
    const mainPos = cam.me.main.pos;
    const showAt = { x: mainPos[0], y: mainPos[1] - 480 };
    this.emptyHint.position.set(showAt.x, showAt.y);

    // Darkness outside the vision union (fan mesh, no blend tricks).
    this.overlay.clear();
    const hasComp = cam.me.comp.alive && !!cam.me.comp.pos;
    const comp = cam.me.comp.pos ?? [0, 0];
    if (hasComp) {
      const d = Math.hypot(comp[0] - mainPos[0], comp[1] - mainPos[1]);
      const tc = Math.atan2(comp[1] - mainPos[1], comp[0] - mainPos[0]);
      drawOutsideOverlay(this.overlay, -2000, -2000, 7200, 7200, mainPos[0], mainPos[1], (a) => {
        const cosD = Math.cos(a - tc);
        const disc = COMP_VISION * COMP_VISION - d * d * (1 - cosD * cosD);
        let inner = MAIN_VISION;
        if (disc > 0) {
          const tExit = d * cosD + Math.sqrt(disc);
          if (tExit > inner) inner = tExit;
        }
        return inner;
      }, 0x140b2c, 0.9);
    } else {
      drawOutsideOverlay(this.overlay, -2000, -2000, 7200, 7200, mainPos[0], mainPos[1], () => MAIN_VISION, 0x140b2c, 0.9);
    }

    // Seen entities.
    this.marks.clear();
    const viewerCol = parseInt(botColor(viewerBot).slice(1), 16);
    this.marks.circle(mainPos[0], mainPos[1], 20).stroke({ width: 2, color: viewerCol, alpha: 0.8 });
    for (const p of cam.seenPlayers) {
      const col = p.detail === "full" ? parseInt(botColor(ownerOf(p.id)).slice(1), 16) : 0xbfd0e8;
      if (p.detail === "full") {
        this.marks.circle(p.pos[0], p.pos[1], 14).fill({ color: col, alpha: 0.3 });
        this.marks.circle(p.pos[0], p.pos[1], 14).stroke({ width: 2.4, color: col });
        if (p.hp !== undefined) {
          this.marks.roundRect(p.pos[0] - 17, p.pos[1] - 28, 34 * p.hp, 4, 2).fill({
            color: p.hp > 0.55 ? 0x43d66e : p.hp > 0.25 ? 0xffc93c : 0xff5f7e,
          });
        }
      } else {
        // Silhouette: pale position-only blob.
        this.marks.circle(p.pos[0], p.pos[1], 9).fill({ color: 0xc7d4ec, alpha: 0.75 });
      }
    }
    for (const c of cam.seenCompanions) {
      if (c.detail === "full") {
        this.marks.circle(c.pos[0], c.pos[1], 9).stroke({ width: 2, color: 0xffffff });
      } else {
        this.marks.circle(c.pos[0], c.pos[1], 7).fill({ color: 0xc7d4ec, alpha: 0.6 });
      }
    }
    // Projectiles + pickups render in their own world layers (candy pellets,
    // pickup tins) which sit under the fog overlay — seen ones are inside the
    // vision hole and stay fully lit; the strict obs list keeps fog honest.

    // Heard events: bearing wedges from the listener.
    this.heardWedges.clear();
    for (const h of cam.heard) {
      if (h.kind === "gunshot") continue;
      const col = KIND_COLORS[h.kind] ?? 0xffffff;
      const r = h.band === "near" ? 130 : h.band === "mid" ? 260 : 420;
      const a = (h.bearing * Math.PI) / 180;
      const spread = 0.34; // ~19° wedge, wider than the 15° quantization
      const x0 = mainPos[0] + Math.sin(a - spread) * 40;
      const y0 = mainPos[1] + Math.cos(a - spread) * 40;
      const x1 = mainPos[0] + Math.sin(a + spread) * 40;
      const y1 = mainPos[1] + Math.cos(a + spread) * 40;
      const xt = mainPos[0] + Math.sin(a) * r;
      const yt = mainPos[1] + Math.cos(a) * r;
      this.heardWedges.moveTo(x0, y0).lineTo(xt, yt).lineTo(x1, y1)
        .fill({ color: col, alpha: 0.16 });
      this.heardWedges.moveTo(mainPos[0] + Math.sin(a) * 40, mainPos[1] + Math.cos(a) * 40)
        .lineTo(xt, yt).stroke({ width: 1.2, color: col, alpha: 0.5 });
    }
  }
}

/** main ids are 1+bot in the protocol — map back for coloring. */
function ownerOf(mainId: number): number {
  return mainId - 1;
}

export type { Container };
