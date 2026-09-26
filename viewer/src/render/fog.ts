/** Player-cam rendering: strict-fog view through one bot's eyes — vision
 * holes punched out of a dark overlay, silhouettes vs full detail, and
 * audio-bearing wedges for heard events. */

import { Container, Graphics, Text } from "pixi.js";
import { botColor, CamFrame } from "../types.js";
import { Stage } from "./stage.js";

const MAIN_VISION = 450;
const COMP_VISION = 250;
const KIND_COLORS: Record<string, number> = {
  gunshot: 0xff4f6d,
  dash: 0xffa54f,
  footstep: 0xffd54f,
  sonar: 0x55e6ff,
};

export class FogView {
  private overlay = new Graphics();
  private holes = new Graphics();
  private marks = new Graphics();
  private heardWedges = new Graphics();
  private emptyHint: Text;

  constructor(stage: Stage) {
    this.holes.blendMode = "erase";
    stage.fogLayer.addChild(this.overlay, this.holes, this.marks, this.heardWedges);
    this.emptyHint = new Text({
      text: "FOLLOWING BOT VISION — what this bot can see and hear",
      style: { fontFamily: "Inter, sans-serif", fontSize: 13, fontWeight: "700", fill: "#8fa8d8", letterSpacing: 2 },
    });
    this.emptyHint.anchor.set(0.5);
    stage.fogLayer.addChild(this.emptyHint);
    this.hide();
  }

  show(): void {
    this.overlay.visible = true;
    this.holes.visible = true;
    this.marks.visible = true;
    this.heardWedges.visible = true;
    this.emptyHint.visible = true;
  }

  hide(): void {
    this.overlay.visible = false;
    this.holes.visible = false;
    this.marks.visible = false;
    this.heardWedges.visible = false;
    this.emptyHint.visible = false;
  }

  update(cam: CamFrame, viewerBot: number): void {
    const mainPos = cam.me.main.pos;
    const showAt = { x: mainPos[0], y: mainPos[1] - 480 };
    this.emptyHint.position.set(showAt.x, showAt.y);

    // Darkness + vision holes (union of main + companion circles).
    this.overlay.clear();
    this.overlay.rect(-2000, -2000, 7200, 7200).fill({ color: 0x02040a, alpha: 0.93 });
    this.holes.clear();
    if (cam.me.main.alive) {
      this.holes.circle(mainPos[0], mainPos[1], MAIN_VISION).fill({ color: 0xffffff });
    }
    if (cam.me.comp.alive && cam.me.comp.pos) {
      this.holes.circle(cam.me.comp.pos[0], cam.me.comp.pos[1], COMP_VISION).fill({ color: 0xffffff });
      // faint leash line
      this.marks.moveTo(mainPos[0], mainPos[1]).lineTo(cam.me.comp.pos[0], cam.me.comp.pos[1])
        .stroke({ width: 0.8, color: 0x9fd8ff, alpha: 0.12 });
    }

    // Seen entities.
    this.marks.clear();
    const viewerCol = parseInt(botColor(viewerBot).slice(1), 16);
    this.marks.circle(mainPos[0], mainPos[1], 20).stroke({ width: 1.6, color: viewerCol, alpha: 0.7 });
    for (const p of cam.seenPlayers) {
      const col = p.detail === "full" ? parseInt(botColor(ownerOf(p.id)).slice(1), 16) : 0xcfd8ff;
      if (p.detail === "full") {
        this.marks.circle(p.pos[0], p.pos[1], 14).stroke({ width: 2.4, color: col });
        if (p.hp !== undefined) {
          this.marks.rect(p.pos[0] - 17, p.pos[1] - 27, 34 * p.hp, 3.4).fill({ color: p.hp > 0.55 ? 0x58ff9b : p.hp > 0.25 ? 0xffd54f : 0xff4f6d });
        }
      } else {
        // Silhouette: pale position-only blob.
        this.marks.circle(p.pos[0], p.pos[1], p.viaSonar ? 11 : 9).fill({ color: 0xaebbd8, alpha: p.viaSonar ? 0.5 : 0.75 });
      }
    }
    for (const c of cam.seenCompanions) {
      if (c.detail === "full") {
        this.marks.circle(c.pos[0], c.pos[1], 9).stroke({ width: 2, color: 0x9fb8e8 });
      } else {
        this.marks.circle(c.pos[0], c.pos[1], 7).fill({ color: 0xaebbd8, alpha: 0.6 });
      }
    }
    for (const p of cam.seenProjectiles) {
      this.marks.moveTo(p.pos[0], p.pos[1]).lineTo(p.pos[0] - p.vel[0] * 0.04, p.pos[1] - p.vel[1] * 0.04)
        .stroke({ width: 2.4, color: parseInt(botColor(p.owner).slice(1), 16) });
    }
    for (const pk of cam.seenPickups) {
      this.marks.moveTo(pk.pos[0], pk.pos[1] - 7).lineTo(pk.pos[0] + 7, pk.pos[1]).lineTo(pk.pos[0], pk.pos[1] + 7).lineTo(pk.pos[0] - 7, pk.pos[1]).closePath()
        .stroke({ width: 1.4, color: 0xffd54f, alpha: 0.9 });
    }

    // Heard events: bearing wedges from the listener.
    this.heardWedges.clear();
    for (const h of cam.heard) {
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
