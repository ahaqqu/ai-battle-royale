/** Home-screen mascots: a tarsius that lives inside the menu card, dancing,
 * roaming between the menu's own controls and playing with them (hopping on
 * the ENTER button, poking the name field, bopping the tagline) while
 * inviting the visitor in — and its jalak, which circles overhead.
 *
 * Reuses the exact in-game art (makeTarsius / makeJalak) so the menu shows
 * what the match delivers. The overlay canvas covers the card exactly and is
 * pointer-transparent; waypoints are read from the real DOM elements, and the
 * mascot's "interactions" are cosmetic CSS animations — it never clicks
 * anything on the user's behalf. */

import { Application, Container, Graphics, Sprite, Text } from "pixi.js";
import { FONT, botColor } from "../types.js";
import { JalakArt, TarsiusArt, makeJalak, makeTarsius } from "./units.js";
import { makeGlowTexture } from "./stage.js";

const HERO_COL = parseInt(botColor(0).slice(1), 16);   // watermelon tarsius
const BIRD_COL = parseInt(botColor(2).slice(1), 16);   // banana jalak accents
/** Dance beat: 132 BPM reads as "party", not "idle". */
const BEAT = 132 / 60;
const SCALE = 0.86;
/** Roam speed in card pixels/second. */
const WALK_SPEED = 96;
const MARGIN = 34;                 // keep this far from the card's edges
/** Bird orbit, relative to the tarsius (held above it). */
const ORBIT_RX = 74;
const ORBIT_RY = 24;
const ORBIT_CY = -52;
const ORBIT_T = 3.6;
/** Invitations the tarsius pipes up with. */
const INVITES = [
  "press ENTER THE ARENA!",
  "come play with us!",
  "race you to the arena!",
  "click ENTER — I'm ready!",
  "dance now, battle later!",
  "your name? make one up!",
  "I'll do the winning, you click.",
];

type Action = "press" | "poke" | "boop" | "dance";
interface Spot { x: number; y: number; action: Action; el?: HTMLElement; }

let app: Application | null = null;
let host: HTMLElement | null = null;
let cardEl: HTMLElement | null = null;
let ro: ResizeObserver | null = null;
let hero: Container | null = null;
let tarsius: TarsiusArt | null = null;
let jalak: JalakArt | null = null;
let birdShadow: Sprite | null = null;
let bubble: Container | null = null;
let bubbleBg: Graphics | null = null;
let bubbleText: Text | null = null;
let onTick: (() => void) | null = null;

let spots: Spot[] = [];
let spotI = -1;
/** Current pose target in card (canvas) coordinates. */
let pos = { x: 200, y: 300 };
let facing = 0;
let targetX = 200;
let targetY = 300;
let action: Action = "dance";
let actionEl: HTMLElement | undefined;
/** Roam state machine: walk to a spot → do its bit → freestyle → repeat. */
let state: "walk" | "act" | "dance" = "dance";
let stateT = 1.4;
let phase = Math.random() * 6.28;
let orbit = { x: 200, y: 260 };
let bubbleT = 0;
let bubbleCd = 6;
let actionFired = false;

/** Build + start the mascot scene, sized to the menu card it lives in. */
export async function startHero(hostEl: HTMLElement): Promise<void> {
  if (app) return;
  const card = hostEl.parentElement?.querySelector<HTMLElement>(".picker-card") ?? null;
  cardEl = card ?? hostEl.parentElement;
  host = hostEl;
  const a = new Application();
  await a.init({
    width: Math.max(200, hostEl.clientWidth || 470),
    height: Math.max(200, hostEl.clientHeight || 560),
    backgroundAlpha: 0,
    antialias: true,
    resolution: Math.min(2, window.devicePixelRatio || 1),
    autoDensity: true,
  });
  app = a;
  a.canvas.style.pointerEvents = "none";
  hostEl.appendChild(a.canvas);

  hero = new Container();
  tarsius = makeTarsius(HERO_COL);
  tarsius.root.scale.set(SCALE);
  jalak = makeJalak(BIRD_COL);
  birdShadow = new Sprite(makeGlowTexture(null, 64, "rgba(58,44,90,0.5)", "rgba(58,44,90,0.16)"));
  birdShadow.anchor.set(0.5);
  birdShadow.scale.set(0.5, 0.26);
  birdShadow.alpha = 0.3;
  hero.addChild(birdShadow, tarsius.root, jalak.root);
  a.stage.addChild(hero);

  // Speech bubble for the invitations.
  bubble = new Container();
  bubbleBg = new Graphics();
  bubbleText = new Text({
    text: "",
    style: {
      fontFamily: FONT, fontSize: 12.5, fontWeight: "800",
      fill: 0x3a2c5a, align: "center", wordWrap: true, wordWrapWidth: 220,
    },
  });
  bubbleText.anchor.set(0.5);
  bubble.addChild(bubbleBg, bubbleText);
  bubble.visible = false;
  a.stage.addChild(bubble);

  measureSpots();
  const start = spots.find((s) => s.action === "dance");
  if (start) { pos = { x: start.x, y: start.y }; targetX = start.x; targetY = start.y; }
  orbit = { x: pos.x, y: pos.y + ORBIT_CY };

  ro = new ResizeObserver(() => resize());
  if (cardEl) ro.observe(cardEl);
  window.addEventListener("resize", resize);

  onTick = () => tick(performance.now() / 1000);
  a.ticker.add(onTick);
}

/** Read the menu's real controls and turn them into roam targets, so the
 * mascot interacts with whatever the card actually lays out. */
function measureSpots(): void {
  if (!app || !cardEl) return;
  const cr = cardEl.getBoundingClientRect();
  const local = (el: Element): { x: number; y: number } => {
    const r = el.getBoundingClientRect();
    return { x: r.left - cr.left + r.width / 2, y: r.top - cr.top + r.height / 2 };
  };
  const q = <T extends HTMLElement>(sel: string): T | null => cardEl!.querySelector<T>(sel);
  const next: Spot[] = [];
  const play = q("#play-btn");
  if (play) next.push({ ...local(play), action: "press", el: play });
  const replays = q("#browse-replays-btn");
  if (replays) next.push({ ...local(replays), action: "press", el: replays });
  const input = q("#play-name");
  if (input) next.push({ ...local(input), action: "poke", el: input });
  const h3 = q(".play-join h3");
  if (h3) next.push({ ...local(h3), action: "boop", el: h3 });
  const tip = q(".home-tip");
  if (tip) next.push({ ...local(tip), action: "boop", el: tip });

  // Freestyle floor spots, so it also just dances around the card.
  const w = cr.width, h = cr.height;
  const cols = 3, rows = 4;
  for (let i = 0; i < cols * rows; i++) {
    const cx = MARGIN + ((i % cols) + 0.5) * ((w - MARGIN * 2) / cols);
    const cy = MARGIN + (Math.floor(i / cols) + 0.5) * ((h - MARGIN * 2) / rows);
    next.push({ x: cx, y: cy, action: "dance" });
  }
  spots = next;
}

function resize(): void {
  if (!app || !host || !cardEl) return;
  const w = cardEl.clientWidth, h = cardEl.clientHeight;
  if (w < 50 || h < 50) return;
  app.renderer.resize(w, h);
  measureSpots();
}

/** Pick the next thing to go do — anything but the spot we're on. */
function pickSpot(): void {
  if (spots.length === 0) { state = "dance"; stateT = 2; return; }
  let i = spotI;
  for (let tries = 0; tries < 8; i = Math.floor(Math.random() * spots.length), tries++) {
    if (i !== spotI) break;
  }
  spotI = i;
  const s = spots[i];
  targetX = s.x; targetY = s.y;
  action = s.action;
  actionEl = s.el;
  actionFired = false;
  state = "walk";
  stateT = 30;   // safety cap; arrival ends the walk
}

function say(text: string, hold = 3.4): void {
  if (!bubbleText) return;
  bubbleText.text = text;
  bubbleT = hold;
}

/** Redraw the bubble's body + tail so the tail always points at the tarsius.
 * The body is centered on the text, and the tail's white fill overlaps the
 * body's edge so the border reads as one continuous outline: only the tail's
 * two free sides get stroked, so no seam line crosses its base. */
function drawBubble(tailDown: boolean): void {
  if (!bubbleText || !bubbleBg) return;
  const w = Math.max(96, bubbleText.width + 24);
  const h = bubbleText.height + 14;
  const half = h / 2;
  const dir = tailDown ? 1 : -1;          // +1: tail hangs from the bottom edge
  const edge = half - 2;                  // tail base tucks 2px inside the body
  const tip = half + 9;
  const fill = { color: 0xffffff, alpha: 0.96 };
  const line = { width: 2, color: 0x6b5b9a, alpha: 0.5, join: "round" } as const;
  bubbleBg.clear();
  bubbleBg.roundRect(-w / 2, -half, w, h, 10).fill(fill).stroke(line);
  bubbleBg.moveTo(-6, edge * dir).lineTo(0, tip * dir).lineTo(6, edge * dir)
    .closePath().fill(fill);
  bubbleBg.moveTo(-6, edge * dir).lineTo(0, tip * dir).lineTo(6, edge * dir)
    .stroke(line);
}

/** Frame driver: roam → act → freestyle, with the bird orbiting the whole way
 * and the invitations surfacing from time to time. */
function tick(t: number): void {
  if (!app || !tarsius || !jalak || !birdShadow || !bubble || !bubbleText) return;
  const dt = Math.min(0.05, app.ticker.deltaMS / 1000);
  const bt = t * BEAT * Math.PI;          // beat phase
  const sway = Math.sin(bt * 0.5);

  let walking = false;
  if (state === "walk") {
    const dx = targetX - pos.x, dy = targetY - pos.y;
    const d = Math.hypot(dx, dy);
    if (d < 5 || stateT <= 0) {
      // Arrived: play the spot's bit.
      pos.x = targetX; pos.y = targetY;
      state = "act";
      stateT = action === "dance" ? 1.9 : 1.35;
    } else {
      const step = Math.min(d, WALK_SPEED * dt);
      pos.x += (dx / d) * step;
      pos.y += (dy / d) * step;
      facing = Math.atan2(dy, dx);
      walking = true;
      phase += dt * 15;
      stateT -= dt;
    }
  } else {
    stateT -= dt;
    if (stateT <= 0) {
      if (state === "act") {
        // Settle into a short freestyle before wandering off again.
        state = "dance";
        stateT = 0.9 + Math.random() * 1.6;
        if (Math.random() < 0.45) say(INVITES[Math.floor(Math.random() * INVITES.length)]);
      } else {
        pickSpot();
      }
    }
  }

  // --- tarsius pose ---
  if (walking) {
    // Step it out toward the target, with the ears streaming.
    const sw = Math.sin(phase);
    tarsius.root.rotation = facing;
    tarsius.root.position.set(pos.x, pos.y - Math.abs(Math.cos(phase)) * 2.5);
    tarsius.footL.x = 13 + sw * 10;
    tarsius.footR.x = 13 - sw * 10;
    tarsius.footL.y = -8.5 - Math.max(0, Math.cos(phase)) * 4;
    tarsius.footR.y = 8.5 - Math.max(0, -Math.cos(phase)) * 4;
    tarsius.armL.rotation = 1.2 + sw * 0.7;
    tarsius.armR.rotation = -1.2 + sw * 0.7;
    tarsius.earL.rotation = 0.3 + sw * 0.2;
    tarsius.earR.rotation = -0.3 - sw * 0.2;
    const st = 0.05 + Math.abs(Math.cos(phase)) * 0.05;
    tarsius.root.scale.set(SCALE * (1 - st * 0.6), SCALE * (1 + st));
  } else if (state === "act") {
    // The spot's bit: press (hop + squash), poke (lean in), boop (bounce).
    const p = 1 - Math.max(0, stateT) / 1.35;
    tarsius.root.rotation = 0;
    tarsius.footL.x = 13; tarsius.footR.x = 13;
    tarsius.footL.y = -8.5; tarsius.footR.y = 8.5;
    tarsius.earL.rotation = 0.3 + Math.sin(bt * 2) * 0.5;
    tarsius.earR.rotation = -0.3 - Math.sin(bt * 2) * 0.5;
    if (action === "press") {
      const hop = Math.sin(Math.min(1, p * 1.35) * Math.PI);
      tarsius.root.position.set(pos.x, pos.y - hop * 20);
      tarsius.root.scale.set(SCALE * (1 + hop * 0.1), SCALE * (1 + hop * 0.16 - hop * hop * 0.2));
      tarsius.armL.rotation = 2.5 - hop * 0.6;
      tarsius.armR.rotation = -2.5 + hop * 0.6;
      if (!actionFired && p > 0.45) {
        actionFired = true;
        pressEl(actionEl);                       // cosmetic bounce only
        say("press ENTER THE ARENA!", 2.6);
      }
    } else if (action === "poke") {
      tarsius.root.position.set(pos.x - 14 + Math.sin(p * 26) * 2, pos.y + 6);
      tarsius.root.rotation = -0.12;
      tarsius.root.scale.set(SCALE, SCALE);
      tarsius.armL.rotation = 2.1 + Math.sin(p * 26) * 0.5;
      tarsius.armR.rotation = -1.4;
      if (!actionFired && p > 0.4) { actionFired = true; pokeEl(actionEl); }
    } else if (action === "boop") {
      const b = Math.abs(Math.sin(p * Math.PI * 2));
      tarsius.root.position.set(pos.x, pos.y + 8 - b * 12);
      tarsius.root.scale.set(SCALE * (1 - b * 0.06), SCALE * (1 + b * 0.12));
      tarsius.armL.rotation = 2.4;
      tarsius.armR.rotation = -2.4;
      if (!actionFired && p > 0.3) { actionFired = true; boopEl(actionEl); }
    } else {
      const hop = Math.max(0, Math.sin(bt)) ** 1.5;
      tarsius.root.position.set(pos.x, pos.y - hop * 12);
      dancePose(bt, SCALE);
      tarsius.armL.rotation = 2.3 + Math.sin(bt) * 1.05;
      tarsius.armR.rotation = -2.3 - Math.sin(bt) * 1.05;
    }
  } else {
    // Freestyle dance: hop on the beat, arms pumping, big ear flaps.
    const hop = Math.max(0, Math.sin(bt)) ** 1.5;
    tarsius.root.position.set(pos.x, pos.y - hop * 12);
    tarsius.root.rotation = sway * 0.16;
    dancePose(bt, SCALE);
    tarsius.footL.y = -8.5 - hop * 4;
    tarsius.footR.y = 8.5 - hop * 4;
    tarsius.footL.x = 13 + sway * 3.5;
    tarsius.footR.x = 13 - sway * 3.5;
    tarsius.armL.rotation = 2.3 + Math.sin(bt) * 1.05;
    tarsius.armR.rotation = -2.3 - Math.sin(bt) * 1.05;
    tarsius.earL.rotation = 0.3 + Math.sin(bt * 2) * 0.55;
    tarsius.earR.rotation = -0.3 - Math.sin(bt * 2) * 0.55;
  }
  tarsius.blinkTarget.scale.y = 1;

  // --- jalak: circles the dancing tarsius, held above it ---
  orbit.x += (pos.x - orbit.x) * Math.min(1, dt * 2.2);
  orbit.y += (pos.y + ORBIT_CY - orbit.y) * Math.min(1, dt * 2.2);
  const oa = (t / ORBIT_T) * Math.PI * 2;
  const bx = orbit.x + Math.cos(oa) * ORBIT_RX;
  const by = orbit.y + Math.sin(oa) * ORBIT_RY;
  jalak.root.position.set(bx, by);
  const bank = Math.cos(oa) * 0.3;
  jalak.root.rotation = Math.atan2(Math.cos(oa) * ORBIT_RY, -Math.sin(oa) * ORBIT_RX) + bank * 0.35;
  const flap = Math.sin(t * 11);
  const span = 0.62 + Math.abs(flap) * 0.5;
  jalak.wingL.scale.set(1 - Math.abs(flap) * 0.16, span);
  jalak.wingR.scale.set(1 - Math.abs(flap) * 0.16, span);
  jalak.wingL.position.y = -2.6 * span - flap * 1.1;
  jalak.wingR.position.y = 2.6 * span + flap * 1.1;
  jalak.tail.rotation = Math.sin(t * 11 - 0.7) * 0.22;
  jalak.head.rotation = -0.08;
  jalak.root.scale.set(SCALE * (1 - Math.abs(flap) * 0.03), SCALE * (1 + flap * 0.05));
  birdShadow.position.set(bx, orbit.y + 42);
  birdShadow.scale.set(0.5 - Math.abs(bank) * 0.08, 0.26 - Math.abs(bank) * 0.04);

  // --- speech bubble ---
  bubbleCd -= dt;
  if (bubbleCd <= 0 && bubbleT <= 0) {
    bubbleCd = 11 + Math.random() * 9;
    say(INVITES[Math.floor(Math.random() * INVITES.length)]);
  }
  if (bubbleT > 0) {
    bubbleT -= dt;
    bubble.visible = true;
    // Flip the bubble below the tarsius when it roams high in the card, so the
    // invitation never covers the heading or the name field.
    const bubbleAbove = tarsius.root.y > 96;
    drawBubble(bubbleAbove);           // bubble above ⇒ tail hangs down at it
    const w = bubbleText.width / 2 + 14;
    const px = Math.min(Math.max(tarsius.root.x, w), app.screen.width - w);
    const py = bubbleAbove ? tarsius.root.y - 78 : tarsius.root.y + 74;
    bubble.position.set(px, py);
    bubble.alpha = Math.min(1, bubbleT / 0.35);
  } else {
    bubble.visible = false;
  }
}

/** Freestyle: squash on the landing, stretch at the top of the hop. */
function dancePose(bt: number, s: number): void {
  if (!tarsius) return;
  const hop = Math.max(0, Math.sin(bt)) ** 1.5;
  const st = hop * 0.18 - (1 - hop) * 0.055 * Math.abs(Math.cos(bt));
  tarsius.root.scale.set(s * (1 - st * 0.7), s * (1 + st));
}

/** Cosmetic menu reactions — the mascot never actually activates a control. */
function pressEl(el?: HTMLElement): void {
  if (!el) return;
  el.classList.remove("mascot-press");
  void el.offsetWidth;             // restart the animation
  el.classList.add("mascot-press");
  window.setTimeout(() => el.classList.remove("mascot-press"), 520);
}
function pokeEl(el?: HTMLElement): void {
  if (!el) return;
  el.classList.remove("mascot-poke");
  void el.offsetWidth;
  el.classList.add("mascot-poke");
  window.setTimeout(() => el.classList.remove("mascot-poke"), 650);
}
function boopEl(el?: HTMLElement): void {
  if (!el) return;
  el.classList.remove("mascot-boop");
  void el.offsetWidth;
  el.classList.add("mascot-boop");
  window.setTimeout(() => el.classList.remove("mascot-boop"), 620);
}

export function stopHero(): void {
  if (!app) return;
  if (onTick) app.ticker.remove(onTick);
  onTick = null;
  ro?.disconnect();
  ro = null;
  window.removeEventListener("resize", resize);
  app.destroy(true, { children: true });
  app = null;
  hero = null;
  tarsius = null;
  jalak = null;
  birdShadow = null;
  bubble = null;
  bubbleBg = null;
  bubbleText = null;
  host = null;
  cardEl = null;
  spots = [];
  spotI = -1;
}
