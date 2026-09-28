/** Unit sprites: chunky tarsius heroes — round fur ball (team color), huge
 * cyan ears with purple cores, cream face mask, big glossy yellow eyes, cyan
 * paws with claw tips — plus swinging arms, stepping feet, soft drop shadows
 * and squash-stretch juice. Companions are jalak (Bali starling) birds that
 * flap along beside their owner.
 *
 * The art lives in the exported makeTarsius / makeJalak factories so the
 * home-screen mascot (render/hero.ts) can reuse the exact same characters.
 * UnitView adds the per-frame choreography: positions/facings are
 * interpolated between the two crossed sim frames (60fps motion from 10Hz
 * sim), feet/arms run a speed-driven walk cycle, ears wobble with the gait,
 * wings flap with speed, dashing pops a squash-stretch pulse, shielding
 * inflates a bubble, taking a hit flashes white.
 *
 * Frame slots follow the Rust layout: [main₀, comp₀, main₁, comp₁, …] —
 * slot i → bot i>>1, main if i&1==0. */

import { Container, Graphics, Sprite, Text } from "pixi.js";
import { botColor, FONT, INK, INK_HEX, U, UNIT_STRIDE, UF_ALIVE, UF_DASH, UF_SHIELD, UF_SPRINT } from "../types.js";
import { Stage } from "./stage.js";

const MAIN_RX = 20;   // body radius
const MAIN_RY = 20;

/** Species-constant tarsius colors (only the fur takes the team color).
 * Ear/paw cyan is deliberately deeper than the sky-blue team color so the
 * ears never blend into a blue bot's fur. */
const EAR_CYAN = 0x1fb2e8;
const EAR_BLUE = 0x3a41d6;
const EAR_INNER = 0x8b3fd9;
const PAW_CYAN = 0x1fb2e8;
const CLAW = 0x8a46e8;
const FACE_CREAM = 0xf5e7c8;
const EYE_YELLOW = 0xf7d308;
const NOSE_PURPLE = 0x8b3fd9;
const COMP_R = 11;
const FOOT_X = 13;    // paw rest position (front of body)
const FOOT_Y = 8.5;

/** Jalak (Bali starling) plumage — the white/black/blue signature. */
const JALAK_WHITE = 0xfbfdff;
const JALAK_TIP = 0x25293f;
const JALAK_EYE_PATCH = 0x1e7ff0;
const JALAK_BEAK = 0xd9d2b0;
const JALAK_LEG = 0x8d93b5;

function lerpAngleDeg(a: number, b: number, t: number): number {
  const d = ((b - a + 540) % 360) - 180;
  return a + d * t;
}

/** Tarsius art. Exported so the home-screen mascot can reuse the same
 * character; `blinkTarget` scales flat to blink, `flash` is the hit wash. */
export interface TarsiusArt {
  kind: "tarsius";
  root: Container;
  earL: Graphics;
  earR: Graphics;
  armL: Graphics;
  armR: Graphics;
  footL: Graphics;
  footR: Graphics;
  blinkTarget: Container;
  flash: Graphics;
}

/** Jalak art: wingL/wingR pivot at the shoulders for flapping. */
export interface JalakArt {
  kind: "jalak";
  root: Container;
  wingL: Graphics;
  wingR: Graphics;
  tail: Graphics;
  head: Container;
  blinkTarget: Container;
  flash: Graphics;
}

/** The tarsius: cyan clawed paws, huge dish ears behind the head, round fur
 * ball, cream face mask with big yellow eyes, purple nose and a smile — all
 * facing local +X so the caller can point it along a bearing. */
export function makeTarsius(col: number): TarsiusArt {
  const root = new Container();
  // Paws: cyan mittens with two purple claw tips.
  const paw = (rx: number, ry: number, ow: number): Graphics => {
    const g = new Graphics();
    g.ellipse(0, 0, rx, ry).fill({ color: PAW_CYAN }).stroke({ width: ow, color: INK, alpha: 0.85 });
    g.circle(rx * 0.55, -ry * 0.38, 1.35).fill({ color: CLAW });
    g.circle(rx * 0.62, ry * 0.34, 1.35).fill({ color: CLAW });
    return g;
  };
  const armL = paw(5.4, 3.8, 2);
  const armR = paw(5.4, 3.8, 2);
  armL.position.set(-2, -16);
  armR.position.set(-2, 16);
  const footL = paw(6, 4.6, 2.2);
  const footR = paw(6, 4.6, 2.2);
  footL.position.set(FOOT_X, -FOOT_Y);
  footR.position.set(FOOT_X, FOOT_Y);

  // Ears: the signature read — big swept-back dishes rising well above the
  // head, cyan rim with a dark-blue then purple inner. Pivots sit behind the
  // body's sides so the attachment is hidden; the caller wobbles them.
  const ear = (dir: 1 | -1): Graphics => {
    const g = new Graphics();
    g.ellipse(0, dir * 12, 7.4, 15.4).fill({ color: EAR_CYAN }).stroke({ width: 2.6, color: INK, alpha: 0.9 });
    g.ellipse(-1, dir * 12.5, 4.5, 10.2).fill({ color: EAR_BLUE });
    g.ellipse(-1.6, dir * 12, 2.5, 6.4).fill({ color: EAR_INNER });
    return g;
  };
  const earL = ear(-1);
  const earR = ear(1);
  earL.position.set(-4, -12.5);
  earR.position.set(-4, 12.5);

  const body = new Graphics();
  body.circle(0, 0, MAIN_RX).fill({ color: col });
  body.circle(0, 0, MAIN_RX).stroke({ width: 3, color: INK, alpha: 0.9 });
  // Cream face mask: two eye lobes merged with a wide muzzle patch. Stroke
  // every shape first, then fill them all — interior strokes get buried under
  // the fills, leaving one clean outline around the union.
  const mask = [
    { x: 9.4, y: -6.8, rx: 8.8, ry: 8.8 },
    { x: 9.4, y: 6.8, rx: 8.8, ry: 8.8 },
    { x: 10.4, y: 0, rx: 9.6, ry: 9.4 },
  ];
  for (const m of mask) {
    body.ellipse(m.x, m.y, m.rx, m.ry).stroke({ width: 3, color: INK, alpha: 0.9 });
  }
  for (const m of mask) {
    body.ellipse(m.x, m.y, m.rx, m.ry).fill({ color: FACE_CREAM });
  }
  // The reference's signature notch: a fur wedge between the eyes, narrow at
  // the back of the mask and widening toward the muzzle.
  body.moveTo(5.4, 0).lineTo(13, -2.6).lineTo(13, 2.6).closePath().fill({ color: col });

  const face = new Container();
  // Big wide-open yellow eyes — no pupils (the reference's wide-eyed stare),
  // just a large white shine up-left and a small one below-right.
  for (const dy of [-6.4, 6.4]) {
    const eye = new Graphics();
    eye.circle(0, 0, 5.9).fill({ color: EYE_YELLOW }).stroke({ width: 1.9, color: INK, alpha: 0.9 });
    eye.circle(-1.7, -1.9, 2).fill({ color: 0xffffff });
    eye.circle(1.6, 2, 0.9).fill({ color: 0xffffff, alpha: 0.9 });
    eye.position.set(11.4, dy);
    face.addChild(eye);
  }
  const nose = new Graphics();
  nose.ellipse(0, 0, 2.3, 1.7).fill({ color: NOSE_PURPLE }).stroke({ width: 1.1, color: INK, alpha: 0.8 });
  nose.position.set(13.4, 0);
  face.addChild(nose);
  // Smile: a short arc bulging toward the muzzle's front.
  const mouth = new Graphics();
  mouth.arc(0, 0, 3.2, -Math.PI * 0.36, Math.PI * 0.36).stroke({ width: 1.5, color: INK, alpha: 0.8, cap: "round" });
  mouth.position.set(15.2, 0);
  face.addChild(mouth);

  const flash = new Graphics();
  flash.circle(0, 0, MAIN_RX + 1.5).fill({ color: 0xffffff, alpha: 0 });

  root.addChild(earL, earR, armL, armR, footL, footR, body, face, flash);
  return { kind: "tarsius", root, earL, earR, armL, armR, footL, footR, blinkTarget: face, flash };
}

/** The jalak: white body with black wing tips and tail tip, cobalt eye patch,
 * pale beak and the drooping crest. `col` (team color) rides a collar band so
 * each companion still reads as its owner's. Faces local +X.
 *
 * Shapes are kept bold and few: at ~25px on screen, layered outlines swallow
 * the white plumage and the bird reads as a dark blob. */
export function makeJalak(col: number): JalakArt {
  const root = new Container();

  // Tail: one fan polygon with two feather lines and a black tip band.
  const tail = new Graphics();
  tail.moveTo(-7, 0).lineTo(-17, -6).lineTo(-17, 0).lineTo(-17, 6).closePath()
    .fill({ color: JALAK_WHITE }).stroke({ width: 1.2, color: INK, alpha: 0.7 });
  tail.moveTo(-14.9, -5.1).lineTo(-17, -6).lineTo(-17, -3.2).closePath().fill({ color: JALAK_TIP });
  tail.moveTo(-14.9, 5.1).lineTo(-17, 6).lineTo(-17, 3.2).closePath().fill({ color: JALAK_TIP });

  // Wings: white blades with a small black outer tip. The span runs along
  // local ±Y; animateJalak foreshortens that span to flap (a real bird beats
  // its wings vertically, which from a top-down camera reads as the wings
  // growing and shrinking, not rotating across the screen).
  const wing = (dir: 1 | -1): Graphics => {
    const g = new Graphics();
    g.ellipse(0, dir * 7, 4.4, 7.4).fill({ color: JALAK_WHITE }).stroke({ width: 1.2, color: INK, alpha: 0.7 });
    g.ellipse(0, dir * 11, 2.1, 1.8).fill({ color: JALAK_TIP });
    return g;
  };
  const wingL = wing(-1);
  const wingR = wing(1);
  wingL.position.set(-1.5, -2.6);
  wingR.position.set(-1.5, 2.6);

  // Body: plump white bird, team-colored collar at the neck, tucked legs.
  const body = new Graphics();
  body.ellipse(0, 0, 10, 7.4).fill({ color: JALAK_WHITE }).stroke({ width: 1.5, color: INK, alpha: 0.8 });
  body.moveTo(3.2, -5.8).lineTo(5, 0).lineTo(3.2, 5.8).stroke({ width: 2.6, color: col, cap: "round" });
  for (const dy of [-3.6, 3.6]) {
    body.moveTo(-0.5, dy * 0.5).lineTo(-2, dy * 0.95).stroke({ width: 1.8, color: JALAK_LEG, cap: "round" });
  }

  // Head: white circle, slim crest sweeping back, cobalt eye patch, dark eye
  // with a shine, short pale beak.
  const head = new Container();
  const skull = new Graphics();
  // Crest first (behind the skull circle) — a slim swept fan, not a blob.
  skull.moveTo(3.4, -4.2).lineTo(-6.6, -8.2).lineTo(-4.4, -6).lineTo(-7.6, -4.6)
    .lineTo(-4.4, -3).closePath().fill({ color: JALAK_WHITE }).stroke({ width: 1.2, color: INK, alpha: 0.7 });
  skull.circle(0, 0, 5.8).fill({ color: JALAK_WHITE }).stroke({ width: 1.5, color: INK, alpha: 0.8 });
  const patch = new Graphics();
  patch.ellipse(1.4, -1.4, 3, 2.5).fill({ color: JALAK_EYE_PATCH });
  const eye = new Graphics();
  eye.circle(1.9, -1.4, 1.35).fill({ color: INK });
  eye.circle(1.4, -1.9, 0.55).fill({ color: 0xffffff });
  const beak = new Graphics();
  beak.moveTo(5, 0.3).lineTo(10.4, -0.6).lineTo(10.4, 1.1).closePath()
    .fill({ color: JALAK_BEAK }).stroke({ width: 1.1, color: INK, alpha: 0.7 });
  head.addChild(skull, patch, eye, beak);
  head.position.set(6.6, -1);

  const flash = new Graphics();
  flash.ellipse(0, 0, 17, 15).fill({ color: 0xffffff, alpha: 0 });

  root.addChild(tail, wingL, wingR, body, head, flash);
  return { kind: "jalak", root, wingL, wingR, tail, head, blinkTarget: eye, flash };
}

export class UnitView {
  root = new Container();
  shadow: Sprite;
  glow: Sprite;
  /** Whole character; rotates to facing. */
  char = new Container();
  art: TarsiusArt | JalakArt;
  shield: Graphics;
  hpBar: Graphics;
  name: Text;
  orbit: Graphics;
  kindMain: boolean;
  bot: number;

  // Animation state.
  private phase = Math.random() * Math.PI * 2; // walk cycle / wing beat
  private facingCur = 0;
  private lastHp01 = 1;
  private flashT = 0;
  private punchT = 0;
  private dashT = 0;
  private dashWas = false;
  private shieldVis = 0;
  private blinkAt = performance.now() + 1500 + Math.random() * 2500;
  private blinkT = 0;
  private lastT = -1;

  constructor(stage: Stage, slot: number, name: string) {
    this.bot = slot >> 1;
    this.kindMain = (slot & 1) === 0;
    const colHex = botColor(this.bot);
    const col = parseInt(colHex.slice(1), 16);

    this.shadow = new Sprite(stage.glowTex);
    this.shadow.anchor.set(0.5);
    this.shadow.tint = 0x2a3a6a;
    this.shadow.alpha = 0.3;

    this.glow = new Sprite(stage.glowTex);
    this.glow.anchor.set(0.5);
    this.glow.tint = col;
    this.glow.blendMode = "add";

    this.art = this.kindMain ? makeTarsius(col) : makeJalak(col);
    this.char.addChild(this.art.root);

    this.shield = new Graphics();
    this.hpBar = new Graphics();
    this.orbit = new Graphics();

    this.name = new Text({
      text: name,
      style: {
        fontFamily: FONT, fontSize: 12.5, fontWeight: "800",
        fill: 0xffffff, letterSpacing: 0.4,
        stroke: { color: INK_HEX, width: 3.5, join: "round" },
      },
    });
    this.name.anchor.set(0.5, 1);
    this.name.alpha = 0.95;

    this.root.addChild(this.shadow, this.glow, this.char, this.orbit, this.shield, this.hpBar, this.name);
    this.root.visible = false;
    stage.unitLayer.addChild(this.root);
  }

  update(
    unitsA: Float32Array, unitsB: Float32Array | null, t: number, showNames: boolean,
  ): void {
    const now = performance.now() / 1000;
    const dt = this.lastT < 0 ? 0.016 : Math.min(0.06, now - this.lastT);
    this.lastT = now;

    const o = this.slotOffset(unitsA);
    if (o < 0) { this.root.visible = false; return; }
    const flags = unitsA[o + U.FLAGS];
    const alive = (flags & UF_ALIVE) !== 0;
    this.root.visible = alive;
    if (!alive) return;

    // Interpolate position/facing against the next sim frame (60fps motion).
    let x = unitsA[o + U.X], y = unitsA[o + U.Y];
    let facing = unitsA[o + U.FACING];
    if (unitsB && t > 0 && this.slotOffset(unitsB) >= 0 && (unitsB[o + U.FLAGS] & UF_ALIVE) !== 0) {
      x += (unitsB[o + U.X] - x) * t;
      y += (unitsB[o + U.Y] - y) * t;
      facing = lerpAngleDeg(facing, unitsB[o + U.FACING], t);
    }
    this.root.position.set(x, y);
    this.facingCur = lerpAngleDeg(this.facingCur, facing, Math.min(1, dt * 18));
    // Sim bearings are 0 = +Y (screen down), clockwise; the sprite's front is
    // local +X, so the rotation that points it along the bearing is 90° − f.
    this.char.rotation = ((90 - this.facingCur) * Math.PI) / 180;

    const hp01 = unitsA[o + U.HP01];
    const sp = Math.hypot(unitsA[o + U.VX], unitsA[o + U.VY]);
    const sprint = (flags & UF_SPRINT) !== 0;
    const dash = (flags & UF_DASH) !== 0;

    if (this.art.kind === "tarsius") this.animateTarsius(dt, sp, sprint, dash, flags, this.art);
    else this.animateJalak(dt, sp, this.art);

    // HP pill.
    const w = this.kindMain ? 52 : 26;
    const h = this.kindMain ? 8 : 6;
    const yTop = this.kindMain ? -MAIN_RY - 22 : -COMP_R - 16;
    const colFill = hp01 > 0.55 ? 0x43d66e : hp01 > 0.25 ? 0xffc93c : 0xff5f7e;
    this.hpBar.clear();
    this.hpBar.roundRect(-w / 2 - 2, yTop - 2, w + 4, h + 4, 5.5).fill({ color: 0xffffff, alpha: 0.95 });
    this.hpBar.roundRect(-w / 2, yTop, Math.max(h, w * hp01), h, 3.5).fill({ color: colFill });
    this.hpBar.roundRect(-w / 2 - 2, yTop - 2, w + 4, h + 4, 5.5).stroke({ width: 1.6, color: INK, alpha: 0.55 });

    this.name.position.set(0, yTop - 8);
    this.name.visible = showNames && this.kindMain;

    // Hit flash (hp dropped since last frame): white-out + scale punch.
    if (hp01 < this.lastHp01 - 0.02) {
      this.flashT = 0.24;
      this.punchT = 0.22;
    }
    this.lastHp01 = hp01;
    if (this.flashT > 0) {
      this.flashT -= dt;
      this.art.flash.alpha = Math.max(0, this.flashT / 0.24);
    } else this.art.flash.alpha = 0;
  }

  private animateTarsius(dt: number, sp: number, sprint: boolean, dash: boolean, flags: number, art: TarsiusArt): void {
    // Walk cycle: speed-driven phase; big foot steps, strong arm swing.
    const moving = sp > 12;
    if (moving) this.phase += dt * (8.5 + sp * 0.035) * (sprint ? 1.45 : 1);
    const sw = moving ? Math.sin(this.phase) : 0;
    const ease = Math.min(1, dt * 12);
    art.footL.x += ((FOOT_X + sw * 10.5) - art.footL.x) * ease;
    art.footL.y += ((-FOOT_Y - Math.max(0, Math.cos(this.phase)) * 4.5) - art.footL.y) * ease;
    art.footR.x += ((FOOT_X - sw * 10.5) - art.footR.x) * ease;
    art.footR.y += ((FOOT_Y - Math.max(0, -Math.cos(this.phase)) * 4.5) - art.footR.y) * ease;
    art.armL.rotation += ((1.15 + sw * 0.65) - art.armL.rotation) * ease;
    art.armR.rotation += ((-1.15 + sw * 0.65) - art.armR.rotation) * ease;

    // Ears: swept slightly forward, flapping with the gait (gentle idle sway
    // when standing still) — the big signature read of the character.
    const flap = moving ? Math.sin(this.phase) * 0.16 : Math.sin(now2() * 2.6 + this.bot) * 0.07;
    art.earL.rotation = 0.3 + flap;
    art.earR.rotation = -0.3 - flap;

    // Squash & stretch: motion stretch + walk bounce + dash pulse + hit punch.
    const st = Math.min(0.16, sp / 1600);
    const bounce = moving ? Math.abs(Math.cos(this.phase)) * 0.08 : Math.sin(now2() * 2.6 + this.bot) * 0.04;
    if (dash && !this.dashWas) this.dashT = 0.32;
    this.dashWas = dash;
    if (this.dashT > 0) this.dashT -= dt;
    const pulse = this.dashT > 0 ? Math.sin((this.dashT / 0.32) * Math.PI) : 0;
    if (this.punchT > 0) this.punchT -= dt;
    const punch = this.punchT > 0 ? Math.sin((this.punchT / 0.22) * Math.PI) * 0.2 : 0;
    this.char.scale.set(1 + st + pulse * 0.5 + punch, 1 - st * 0.75 + bounce - pulse * 0.34 + punch);

    // Shield bubble: a loud glass dome — strong fill, thick rim, rotating
    // energy arcs and a gentle pulse so it reads even on the bright floor.
    const target = (flags & UF_SHIELD) !== 0 ? 1 : 0;
    this.shieldVis += (target - this.shieldVis) * Math.min(1, dt * 14);
    this.shield.clear();
    if (this.shieldVis > 0.02) {
      const now = now2();
      const a = this.shieldVis;
      const r = (MAIN_RX + 22) * (0.55 + 0.45 * a) * (1 + Math.sin(now * 7) * 0.035);
      this.shield.circle(0, 0, r).fill({ color: 0x35c1f0, alpha: 0.3 * a });
      this.shield.circle(0, 0, r * 0.72).fill({ color: 0xbfe9ff, alpha: 0.24 * a });
      this.shield.circle(0, 0, r).stroke({ width: 5, color: 0xffffff, alpha: 0.95 * a });
      this.shield.circle(0, 0, r + 4).stroke({ width: 2.5, color: 0x35c1f0, alpha: 0.8 * a });
      for (const off of [0, Math.PI]) {
        this.shield.arc(0, 0, r + 8, now * 2.2 + off, now * 2.2 + off + 1.1)
          .stroke({ width: 3, color: 0xffffff, alpha: 0.75 * a, cap: "round" });
      }
      this.shield.circle(-r * 0.35, -r * 0.45, 3).fill({ color: 0xffffff, alpha: 0.9 * a });
      this.shield.circle(r * 0.3, r * 0.42, 1.8).fill({ color: 0xffffff, alpha: 0.7 * a });
    }

    // Blink.
    this.blink(dt, art);

    this.glow.scale.set(1.7 + (sprint ? 0.55 : 0) + (dash ? 0.9 : 0));
    this.glow.alpha = 0.24 + (sprint ? 0.12 : 0) + (dash ? 0.28 : 0);
    this.shadow.scale.set(0.62, 0.4);
  }

  /** Jalak flight: the bird hovers beside its owner, wings beating faster the
   * harder it works. The beat foreshortens the wing span (how a real wing
   * looks from above as it swings up and down) — a screen-plane rotation would
   * just read as noise from this camera. */
  private animateJalak(dt: number, sp: number, art: JalakArt): void {
    const beat = 9.5 + Math.min(9, sp / 30);
    this.phase += dt * beat;
    const flap = Math.sin(this.phase);
    // Wings sweep: span compresses on the upstroke, tips pull in with it.
    const span = 0.62 + Math.abs(flap) * 0.5;
    art.wingL.scale.set(1 - Math.abs(flap) * 0.16, span);
    art.wingR.scale.set(1 - Math.abs(flap) * 0.16, span);
    art.wingL.position.y = -2.6 * span - flap * 1.1;
    art.wingR.position.y = 2.6 * span + flap * 1.1;
    // Tail fans and counter-sways a beat behind the wings.
    const tailSw = Math.sin(this.phase - 0.7);
    art.tail.rotation = tailSw * 0.16;
    art.tail.scale.set(1, 1 + Math.abs(tailSw) * 0.16);
    // Head leads slightly into the glide, nodding on the beat.
    art.head.rotation = -0.1 + Math.sin(this.phase * 0.5) * 0.08;

    const bob = Math.sin(this.phase * 0.5) * 1.6;
    this.char.y = bob - Math.min(9, sp / 32);
    // Body rides the beat: compress on the upstroke, stretch on the down.
    this.char.scale.set(1 - Math.abs(flap) * 0.04, 1 + flap * 0.06);

    this.blink(dt, art);
    const t2 = now2();
    this.orbit.clear();
    this.orbit.circle(Math.cos(t2 * 3) * 15, Math.sin(t2 * 3) * 15 - 6, 1.7).fill({ color: 0xffffff, alpha: 0.85 });
    this.glow.scale.set(0.9);
    this.glow.alpha = 0.2;
    this.shadow.scale.set(0.34, 0.22);
    this.shadow.alpha = 0.22;
  }

  /** Eyes squeeze shut for a beat every few seconds (per-species target:
   * the tarsius squints its whole face, the jalak just its eye). */
  private blink(dt: number, art: TarsiusArt | JalakArt): void {
    if (this.blinkT > 0) {
      this.blinkT -= dt;
      art.blinkTarget.scale.y = 0.12;
    } else {
      art.blinkTarget.scale.y = 1;
      if (now2() > this.blinkAt) {
        this.blinkT = 0.13;
        this.blinkAt = now2() + 1800 + Math.random() * 2800;
      }
    }
  }

  /** Slot offset for this view inside a units array, or -1 if out of range. */
  private slotOffset(units: Float32Array): number {
    const o = (this.bot * 2 + (this.kindMain ? 0 : 1)) * UNIT_STRIDE;
    return o + UNIT_STRIDE <= units.length ? o : -1;
  }
}

function now2(): number {
  return performance.now() / 1000;
}

export class UnitViews {
  views: UnitView[] = [];

  constructor(stage: Stage, names: string[]) {
    for (let i = 0; i < names.length * 2; i++) {
      const bot = i >> 1;
      const isMain = (i & 1) === 0;
      this.views.push(new UnitView(stage, i, isMain ? names[bot] : `${names[bot]}·jalak`));
    }
  }

  /** Draw all slots, interpolating A→B by t (0..1). */
  update(unitsA: Float32Array, unitsB: Float32Array | null, t: number, unitCount: number, showNames: boolean): void {
    const slots = Math.min(unitCount, unitsA.length / UNIT_STRIDE);
    for (let i = 0; i < slots; i++) {
      this.views[i]?.update(unitsA, unitsB, t, showNames);
    }
  }
}
