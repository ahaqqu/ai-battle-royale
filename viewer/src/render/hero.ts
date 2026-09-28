/** Home-screen mascots: a tarsius dancing in place on its little stage while
 * its jalak circles overhead — the two playable characters introduced on the
 * front page. Reuses the exact in-game art (makeTarsius / makeJalak) so the
 * menu promises what the match delivers.
 *
 * Runs on its own tiny Pixi app beside the menu card, independent of the
 * ambient gameplay backdrop, and is torn down when the menu is left. */

import { Application, Container, Graphics, Sprite, Text } from "pixi.js";
import { FONT, INK_HEX, botColor } from "../types.js";
import { JalakArt, TarsiusArt, makeJalak, makeTarsius } from "./units.js";
import { makeGlowTexture } from "./stage.js";

const W = 340;
const H = 300;
const HERO_COL = parseInt(botColor(0).slice(1), 16);   // watermelon tarsius
const BIRD_COL = parseInt(botColor(2).slice(1), 16);   // banana jalak accents
/** Dance beat: 132 BPM reads as "party", not "idle". */
const BEAT = 132 / 60;
/** Orbit radii for the circling jalak (tilted ellipse, held above the hero). */
const ORBIT_RX = 96;
const ORBIT_RY = 34;
const ORBIT_CY = -64;
const ORBIT_T = 4.6;

let app: Application | null = null;
let hero: Container | null = null;
let tarsius: TarsiusArt | null = null;
let jalak: JalakArt | null = null;
let birdShadow: Sprite | null = null;
let onTick: (() => void) | null = null;

/** Build + start the mascot scene on the given host element. */
export async function startHero(host: HTMLElement): Promise<void> {
  if (app) return;
  const a = new Application();
  await a.init({
    width: W,
    height: H,
    backgroundAlpha: 0,
    antialias: true,
    resolution: Math.min(2, window.devicePixelRatio || 1),
    autoDensity: true,
  });
  app = a;
  a.canvas.style.width = `${W}px`;
  a.canvas.style.height = `${H}px`;
  a.canvas.style.pointerEvents = "none";
  host.appendChild(a.canvas);

  // Stage: a spotlight pool on the menu sky + the orbit path the bird flies.
  const heroY = 178;
  const ground = new Graphics();
  ground.ellipse(W / 2, 236, 62, 15).fill({ color: 0x3a2c5a, alpha: 0.12 });
  ground.ellipse(W / 2, 234, 76, 19).fill({ color: 0xffffff, alpha: 0.3 });
  ground.ellipse(W / 2, heroY + ORBIT_CY, ORBIT_RX, ORBIT_RY).stroke({ width: 1.6, color: 0xffffff, alpha: 0.34 });
  a.stage.addChild(ground);

  hero = new Container();
  hero.position.set(W / 2, heroY);
  tarsius = makeTarsius(HERO_COL);
  jalak = makeJalak(BIRD_COL);
  // The bird reads as a companion: slightly smaller, with its own soft shadow
  // that slides along the orbit beneath it.
  jalak.root.scale.set(0.9);
  birdShadow = new Sprite(makeGlowTexture(null, 64, "rgba(58,44,90,0.5)", "rgba(58,44,90,0.16)"));
  birdShadow.anchor.set(0.5);
  birdShadow.scale.set(0.6, 0.32);
  birdShadow.alpha = 0.35;
  hero.addChild(birdShadow, tarsius.root, jalak.root);
  a.stage.addChild(hero);

  // Name tags under the stage, in the same chunky style as the in-game labels.
  const tag = (text: string, col: string, y: number): Text => {
    const t = new Text({
      text,
      style: {
        fontFamily: FONT, fontSize: 15, fontWeight: "800",
        fill: col, letterSpacing: 1.6,
        stroke: { color: INK_HEX, width: 4.5, join: "round" },
      },
    });
    t.anchor.set(0.5, 0);
    t.position.set(W / 2, y);
    return t;
  };
  a.stage.addChild(tag("TARSIUS", "#ff8fb8", H - 34));
  a.stage.addChild(tag("JALAK", "#ffd93b", H - 15));

  onTick = () => dance(performance.now() / 1000);
  a.ticker.add(onTick);
}

/** One frame of choreography: the tarsius bounces and sways on the beat with
 * its ears flapping, the jalak orbits in a tilted ellipse, flapping faster. */
function dance(t: number): void {
  if (!tarsius || !jalak || !birdShadow) return;
  const bt = t * BEAT * Math.PI;          // beat phase

  // --- tarsius: two-step dance ---
  const sway = Math.sin(bt * 0.5);
  const hop = Math.max(0, Math.sin(bt)) ** 1.5;
  tarsius.root.y = -hop * 15;
  tarsius.root.rotation = sway * 0.16;
  // Squash on the land, stretch at the top of the hop.
  const st = hop * 0.18 - (1 - hop) * 0.055 * Math.abs(Math.cos(bt));
  tarsius.root.scale.set(1 - st * 0.7, 1 + st);

  // Arms pump up on the beat, feet keep a little step.
  const pump = Math.sin(bt) * 1.05;
  tarsius.armL.rotation = 2.3 + pump;
  tarsius.armR.rotation = -2.3 - pump;
  tarsius.footL.y = -8.5 - hop * 4;
  tarsius.footR.y = 8.5 - hop * 4;
  tarsius.footL.x = 13 + sway * 3.5;
  tarsius.footR.x = 13 - sway * 3.5;
  // Ears flap big — the signature read, doubled for the dance.
  tarsius.earL.rotation = 0.3 + Math.sin(bt * 2) * 0.55;
  tarsius.earR.rotation = -0.3 - Math.sin(bt * 2) * 0.55;
  // Eyes stay open (dancing, not blinking) — reset any stale blink scale.
  tarsius.blinkTarget.scale.y = 1;

  // --- jalak: circling flight ---
  const oa = (t / ORBIT_T) * Math.PI * 2;
  const ox = Math.cos(oa) * ORBIT_RX;
  const oy = ORBIT_CY + Math.sin(oa) * ORBIT_RY;
  jalak.root.position.set(ox, oy);
  // Face along the orbit tangent. The bird banks into the turn, dipping a wing
  // toward the inside of the circle.
  const tx = -Math.sin(oa) * ORBIT_RX;
  const ty = Math.cos(oa) * ORBIT_RY;
  const bank = Math.cos(oa) * 0.3;
  jalak.root.rotation = Math.atan2(ty, tx) + bank * 0.35;
  // Wing beat a touch faster than the dance, foreshortening the span the same
  // way the in-game bird does.
  const flap = Math.sin(t * 11);
  const span = 0.62 + Math.abs(flap) * 0.5;
  jalak.wingL.scale.set(1 - Math.abs(flap) * 0.16, span);
  jalak.wingR.scale.set(1 - Math.abs(flap) * 0.16, span);
  jalak.wingL.position.y = -2.6 * span - flap * 1.1;
  jalak.wingR.position.y = 2.6 * span + flap * 1.1;
  jalak.tail.rotation = Math.sin(t * 11 - 0.7) * 0.22;
  jalak.head.rotation = -0.08;
  jalak.root.scale.set(0.9 - Math.abs(flap) * 0.03, 0.9 + flap * 0.05);
  // Shadow tracks the bird on the stage ellipse, shrinking as it "climbs".
  birdShadow.position.set(ox * 0.92, 60);
  birdShadow.scale.set(0.6 - Math.abs(bank) * 0.1, 0.32 - Math.abs(bank) * 0.05);
  birdShadow.alpha = 0.3 - Math.abs(bank) * 0.08;
}

export function stopHero(): void {
  if (!app) return;
  if (onTick) app.ticker.remove(onTick);
  onTick = null;
  app.destroy(true, { children: true });
  app = null;
  hero = null;
  tarsius = null;
  jalak = null;
  birdShadow = null;
}
