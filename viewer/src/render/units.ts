/** Unit sprites: chunky tarsius characters — round fur ball (team color),
 * huge cyan ears with purple cores, cream face mask, big glossy yellow eyes,
 * cyan paws with claw tips — plus swinging arms, stepping feet, soft drop
 * shadows and squash-stretch juice. Companions are floating puffball "pets".
 *
 * Everything animates procedurally per rendered frame: positions/facings are
 * interpolated between the two crossed sim frames (60fps motion from 10Hz
 * sim), feet/arms run a speed-driven walk cycle, ears wobble with the gait,
 * dashing pops a squash-stretch pulse, shielding inflates a bubble, taking a
 * hit flashes white.
 *
 * Frame slots follow the Rust layout: [main₀, comp₀, main₁, comp₁, …] —
 * slot i → bot i>>1, main if i&1==0. */

import { Container, Graphics, Sprite, Text } from "pixi.js";
import { botColor, FONT, INK, INK_HEX, shade, U, UNIT_STRIDE, UF_ALIVE, UF_DASH, UF_SHIELD, UF_SPRINT } from "../types.js";
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

function lerpAngleDeg(a: number, b: number, t: number): number {
  const d = ((b - a + 540) % 360) - 180;
  return a + d * t;
}

/** One stubby foot/arm: rounded nub with an ink outline, pivot at origin. */
function nub(rx: number, ry: number, col: number, outline: number): Graphics {
  const g = new Graphics();
  g.ellipse(0, 0, rx, ry).fill({ color: col }).stroke({ width: outline, color: INK, alpha: 0.85 });
  return g;
}

export class UnitView {
  root = new Container();
  shadow: Sprite;
  glow: Sprite;
  /** Whole character; rotates to facing. */
  char = new Container();
  body!: Graphics;
  earL!: Graphics;
  earR!: Graphics;
  armL!: Graphics;
  armR!: Graphics;
  footL!: Graphics;
  footR!: Graphics;
  face = new Container();
  flash!: Graphics;
  shield: Graphics;
  hpBar: Graphics;
  name: Text;
  orbit: Graphics;
  kindMain: boolean;
  bot: number;

  // Animation state.
  private phase = Math.random() * Math.PI * 2; // walk cycle
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

    if (this.kindMain) this.buildTarsius(col);
    else this.buildPet(col);

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

  /** The tarsius: cyan clawed paws (animated), huge dish ears behind the
   * head, round fur ball, cream face mask with big yellow eyes, purple nose
   * and a tiny smile — all facing local +X, which the char container points
   * along the movement bearing. */
  private buildTarsius(col: number): void {
    // Paws: cyan mittens with two purple claw tips.
    const paw = (rx: number, ry: number, ow: number): Graphics => {
      const g = new Graphics();
      g.ellipse(0, 0, rx, ry).fill({ color: PAW_CYAN }).stroke({ width: ow, color: INK, alpha: 0.85 });
      g.circle(rx * 0.55, -ry * 0.38, 1.35).fill({ color: CLAW });
      g.circle(rx * 0.62, ry * 0.34, 1.35).fill({ color: CLAW });
      return g;
    };
    this.armL = paw(5.4, 3.8, 2);
    this.armR = paw(5.4, 3.8, 2);
    this.armL.position.set(-2, -16);
    this.armR.position.set(-2, 16);
    this.footL = paw(6, 4.6, 2.2);
    this.footR = paw(6, 4.6, 2.2);
    this.footL.position.set(FOOT_X, -FOOT_Y);
    this.footR.position.set(FOOT_X, FOOT_Y);

    // Ears: the signature read — big swept-back dishes rising well above the
    // head, cyan rim with a dark-blue then purple inner. Pivots sit behind the
    // body's sides so the attachment is hidden; animateTarsius wobbles them.
    const ear = (dir: 1 | -1): Graphics => {
      const g = new Graphics();
      g.ellipse(0, dir * 12, 7.4, 15.4).fill({ color: EAR_CYAN }).stroke({ width: 2.6, color: INK, alpha: 0.9 });
      g.ellipse(-1, dir * 12.5, 4.5, 10.2).fill({ color: EAR_BLUE });
      g.ellipse(-1.6, dir * 12, 2.5, 6.4).fill({ color: EAR_INNER });
      return g;
    };
    this.earL = ear(-1);
    this.earR = ear(1);
    this.earL.position.set(-4, -12.5);
    this.earR.position.set(-4, 12.5);

    this.body = new Graphics();
    this.body.circle(0, 0, MAIN_RX).fill({ color: col });
    this.body.circle(0, 0, MAIN_RX).stroke({ width: 3, color: INK, alpha: 0.9 });
    // Cream face mask: two eye lobes merged with a wide muzzle patch, plus the
    // small V that runs up between the eyes (the reference's forehead notch).
    // Stroke every shape first, then fill them all — interior strokes get
    // buried under the fills, leaving one clean outline around the union.
    const mask = [
      { x: 9.4, y: -6.8, rx: 8.8, ry: 8.8 },
      { x: 9.4, y: 6.8, rx: 8.8, ry: 8.8 },
      { x: 10.4, y: 0, rx: 9.6, ry: 9.4 },
    ];
    for (const m of mask) {
      this.body.ellipse(m.x, m.y, m.rx, m.ry).stroke({ width: 3, color: INK, alpha: 0.9 });
    }
    for (const m of mask) {
      this.body.ellipse(m.x, m.y, m.rx, m.ry).fill({ color: FACE_CREAM });
    }
    // The reference's signature notch: a fur wedge between the eyes, narrow at
    // the back of the mask and widening toward the muzzle. Mostly hidden
    // behind the near-touching eyes — a subtle read at high zoom only.
    this.body.moveTo(5.4, 0).lineTo(13, -2.6).lineTo(13, 2.6).closePath().fill({ color: col });

    // Big wide-open yellow eyes — no pupils (the reference's wide-eyed stare),
    // just a large white shine up-left and a small one below-right.
    for (const dy of [-6.4, 6.4]) {
      const eye = new Graphics();
      eye.circle(0, 0, 5.9).fill({ color: EYE_YELLOW }).stroke({ width: 1.9, color: INK, alpha: 0.9 });
      eye.circle(-1.7, -1.9, 2).fill({ color: 0xffffff });
      eye.circle(1.6, 2, 0.9).fill({ color: 0xffffff, alpha: 0.9 });
      eye.position.set(11.4, dy);
      this.face.addChild(eye);
    }
    const nose = new Graphics();
    nose.ellipse(0, 0, 2.3, 1.7).fill({ color: NOSE_PURPLE }).stroke({ width: 1.1, color: INK, alpha: 0.8 });
    nose.position.set(13.4, 0);
    this.face.addChild(nose);
    // Smile: a short arc bulging toward the muzzle's front (the face's "chin"
    // direction is local +X, so the smile opens back toward the eyes).
    const mouth = new Graphics();
    mouth.arc(0, 0, 3.2, -Math.PI * 0.36, Math.PI * 0.36).stroke({ width: 1.5, color: INK, alpha: 0.8, cap: "round" });
    mouth.position.set(15.2, 0);
    this.face.addChild(mouth);

    this.flash = new Graphics();
    this.flash.circle(0, 0, MAIN_RX + 1.5).fill({ color: 0xffffff, alpha: 0 });

    this.char.addChild(this.earL, this.earR, this.armL, this.armR, this.footL, this.footR, this.body, this.face, this.flash);
  }

  /** Floating puffball pet with a face and tiny nub feet. */
  private buildPet(col: number): void {
    this.armL = nub(2.6, 1.8, shade(col, 0.72), 1.4);
    this.armR = nub(2.6, 1.8, shade(col, 0.72), 1.4);
    this.armL.position.set(-3, -9.5);
    this.armR.position.set(-3, 9.5);
    this.footL = nub(2.8, 2.1, shade(col, 0.72), 1.6);
    this.footR = nub(2.8, 2.1, shade(col, 0.72), 1.6);
    this.footL.position.set(5, -4.8);
    this.footR.position.set(5, 4.8);

    this.body = new Graphics();
    this.body.circle(0, 0, COMP_R).fill({ color: col }).stroke({ width: 2.4, color: INK, alpha: 0.9 });
    this.body.circle(-3.6, -3.6, 2).fill({ color: 0xffffff, alpha: 0.65 });
    for (const dy of [-3.4, 3.4]) {
      const eye = new Graphics();
      eye.circle(0, 0, 2.9).fill({ color: 0xffffff }).stroke({ width: 0.9, color: INK, alpha: 0.5 });
      eye.circle(1.3, 0, 1.5).fill({ color: INK });
      eye.circle(1.9, -0.8, 0.6).fill({ color: 0xffffff });
      eye.position.set(5, dy);
      this.face.addChild(eye);
    }
    this.flash = new Graphics();
    this.flash.circle(0, 0, COMP_R + 1).fill({ color: 0xffffff, alpha: 0 });

    this.char.addChild(this.armL, this.armR, this.footL, this.footR, this.body, this.face, this.flash);
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

    if (this.kindMain) this.animateTarsius(dt, sp, sprint, dash, hp01, flags);
    else this.animatePet(dt, sp);

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
      this.flash.alpha = Math.max(0, this.flashT / 0.24);
    } else this.flash.alpha = 0;
  }

  private animateTarsius(dt: number, sp: number, sprint: boolean, dash: boolean, _hp01: number, flags: number): void {
    // Walk cycle: speed-driven phase; big foot steps, strong arm swing.
    const moving = sp > 12;
    if (moving) this.phase += dt * (8.5 + sp * 0.035) * (sprint ? 1.45 : 1);
    const sw = moving ? Math.sin(this.phase) : 0;
    const ease = Math.min(1, dt * 12);
    this.footL.x += ((FOOT_X + sw * 10.5) - this.footL.x) * ease;
    this.footL.y += ((-FOOT_Y - Math.max(0, Math.cos(this.phase)) * 4.5) - this.footL.y) * ease;
    this.footR.x += ((FOOT_X - sw * 10.5) - this.footR.x) * ease;
    this.footR.y += ((FOOT_Y - Math.max(0, -Math.cos(this.phase)) * 4.5) - this.footR.y) * ease;
    this.armL.rotation += ((1.15 + sw * 0.65) - this.armL.rotation) * ease;
    this.armR.rotation += ((-1.15 + sw * 0.65) - this.armR.rotation) * ease;

    // Ears: swept slightly forward, flapping with the gait (gentle idle sway
    // when standing still) — the big signature read of the character.
    const flap = moving ? Math.sin(this.phase) * 0.16 : Math.sin(now2() * 2.6 + this.bot) * 0.07;
    this.earL.rotation = 0.3 + flap;
    this.earR.rotation = -0.3 - flap;

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
    this.blink(dt);

    this.glow.scale.set(1.7 + (sprint ? 0.55 : 0) + (dash ? 0.9 : 0));
    this.glow.alpha = 0.24 + (sprint ? 0.12 : 0) + (dash ? 0.28 : 0);
    this.shadow.scale.set(0.62, 0.4);
  }

  private animatePet(dt: number, sp: number): void {
    // Float bob, faster when the owner moves; nubs trail a mini cycle.
    this.phase += dt * (3 + sp * 0.02);
    const bob = Math.sin(this.phase) * (4.2 + Math.min(4, sp / 70));
    this.char.y = bob;
    this.char.scale.set(1 + Math.cos(this.phase) * 0.05);
    this.footL.y = -4.8 + Math.sin(this.phase * 2) * 1.2;
    this.footR.y = 4.8 - Math.sin(this.phase * 2) * 1.2;
    this.blink(dt);
    const t2 = now2();
    this.orbit.clear();
    this.orbit.circle(Math.cos(t2 * 3) * 16, Math.sin(t2 * 3) * 16 - 6, 2).fill({ color: 0xffffff, alpha: 0.9 });
    this.glow.scale.set(0.85);
    this.glow.alpha = 0.16;
    this.shadow.scale.set(0.36, 0.24);
    this.shadow.alpha = 0.2;
  }

  /** Both eyes squeeze shut for a beat every few seconds. */
  private blink(dt: number): void {
    if (this.blinkT > 0) {
      this.blinkT -= dt;
      this.face.scale.y = 0.12;
    } else {
      this.face.scale.y = 1;
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
      this.views.push(new UnitView(stage, i, isMain ? names[bot] : `${names[bot]}·pet`));
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
