/** Home-screen mascots: a lazy, grumpy sharpshooter tarsius who lives inside
 * the menu card — napping between chores, shuffling to the next one only when
 * the nagging becomes unbearable — and his jalak, a cheerful hype-bird who
 * never stops moving and never stops shouting his trademark 「GUNBATTE！」 at
 * his best friend. The tarsius plays with the menu (hopping on the ENTER
 * button, poking the name field, bopping the tagline) strictly when necessary.
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
/** Roam speed in card pixels/second — a lazy shamble, not a walk. */
const WALK_SPEED = 70;
const MARGIN = 34;                 // keep this far from the card's edges
/** Bird orbit, relative to the tarsius (held above it). */
const ORBIT_RX = 74;
const ORBIT_RY = 24;
const ORBIT_CY = -52;
const ORBIT_T = 2.4;
/** Tarsius's lines: grumpy, lazy, deadpan. He'd rather nap. */
const TARSIUS_LINES = [
  "five more minutes.",
  "ugh. fine. ENTER THE ARENA.",
  "I only miss once. that was practice.",
  "shush, bird.",
  "naps > battles.",
  "I move when necessary. this is necessary.",
  "wake me when someone wins.",
];
/** Grumbles he fires back when the trademark catches him mid-nap. */
const GRUMBLES = [
  "ganbatte… I mean GUNBATTE. ugh.",
  "GUNBATTE yourself.",
  "I heard you the first time.",
  "shush, bird.",
];
/** Jalak's lines: cheerful, loud, endless. GUNBATTE! is his trademark. */
const JALAK_LINES = [
  "GUNBATTE!!!",
  "GUNBATTE, TARSIUS!!",
  "がんばって！！",
  "GUNBATTE!!! gun up!!",
  "wakey wakey, battle time!!",
  "best shot in the arena!! GUNBATTE!!",
  "last tarsius standing!! that's you!!",
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
let birdBubble: Container | null = null;
let birdBubbleBg: Graphics | null = null;
let birdBubbleText: Text | null = null;
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
/** Roam state machine: walk to a spot → do its bit → lounge or freestyle → repeat. */
let state: "walk" | "act" | "dance" | "lounge" = "dance";
let stateT = 1.4;
let phase = Math.random() * 6.28;
let orbit = { x: 200, y: 260 };
let bubbleT = 0;
let bubbleCd = 6;
let birdBubbleT = 0;
/** The bird cannot stay quiet for long — he opens with a shout. */
let birdBubbleCd = 2.2;
/** A scheduled grumble: the reply to the trademark, fired a beat later. */
let replyText: string | null = null;
let replyT = 0;
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

  // The jalak's own bubble — the GUNBATTE! trademark lives here.
  birdBubble = new Container();
  birdBubbleBg = new Graphics();
  birdBubbleText = new Text({
    text: "",
    style: {
      fontFamily: FONT, fontSize: 12.5, fontWeight: "800",
      fill: 0x3a2c5a, align: "center", wordWrap: true, wordWrapWidth: 200,
    },
  });
  birdBubbleText.anchor.set(0.5);
  birdBubble.addChild(birdBubbleBg, birdBubbleText);
  birdBubble.visible = false;
  a.stage.addChild(birdBubble);

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

/** The bird pipes up. Shouting the trademark at his best friend usually earns
 * a grumble back a beat later — that's the whole bit. */
function birdSay(text: string, hold = 3.2, provokeReply = true): void {
  if (!birdBubbleText) return;
  birdBubbleText.text = text;
  birdBubbleT = hold;
  if (provokeReply && text.startsWith("GUNBATTE") && Math.random() < 0.6) {
    replyText = GRUMBLES[Math.floor(Math.random() * GRUMBLES.length)];
    replyT = 1.2 + Math.random() * 0.9;
  }
}

/** Redraw a bubble's body + tail so the tail always points at its speaker.
 * The body is centered on the text, and the tail's white fill overlaps the
 * body's edge so the border reads as one continuous outline: only the tail's
 * two free sides get stroked, so no seam line crosses its base. */
function drawBubble(bg: Graphics, text: Text, tailDown: boolean, stroke = 0x6b5b9a): void {
  const w = Math.max(96, text.width + 24);
  const h = text.height + 14;
  const half = h / 2;
  const dir = tailDown ? 1 : -1;          // +1: tail hangs from the bottom edge
  const edge = half - 2;                  // tail base tucks 2px inside the body
  const tip = half + 9;
  const fill = { color: 0xffffff, alpha: 0.96 };
  const line = { width: 2, color: stroke, alpha: 0.55, join: "round" } as const;
  bg.clear();
  bg.roundRect(-w / 2, -half, w, h, 10).fill(fill).stroke(line);
  bg.moveTo(-6, edge * dir).lineTo(0, tip * dir).lineTo(6, edge * dir)
    .closePath().fill(fill);
  bg.moveTo(-6, edge * dir).lineTo(0, tip * dir).lineTo(6, edge * dir)
    .stroke(line);
}

/** Frame driver: roam → act → freestyle, with the bird orbiting the whole way
 * and the invitations surfacing from time to time. */
function tick(t: number): void {
  if (!app || !tarsius || !jalak || !birdShadow || !bubble || !bubbleBg || !bubbleText) return;
  if (!birdBubble || !birdBubbleBg || !birdBubbleText) return;
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
      phase += dt * 11;   // a shamble, not a strut
      stateT -= dt;
    }
  } else {
    stateT -= dt;
    if (stateT <= 0) {
      if (state === "act") {
        // Lazy: more often than not he flops down right there instead of
        // dancing. The bird treats every flop as an invitation to nag.
        if (Math.random() < 0.55) {
          state = "lounge";
          stateT = 2.6 + Math.random() * 2.8;
          if (Math.random() < 0.75) {
            birdSay(JALAK_LINES[Math.floor(Math.random() * JALAK_LINES.length)]);
          }
        } else {
          state = "dance";
          stateT = 0.8 + Math.random() * 1.3;
          if (Math.random() < 0.4) say(TARSIUS_LINES[Math.floor(Math.random() * TARSIUS_LINES.length)]);
        }
      } else if (state === "lounge") {
        // Getting up is a whole thing, and he wants everyone to know it.
        if (Math.random() < 0.5) say(TARSIUS_LINES[Math.floor(Math.random() * TARSIUS_LINES.length)]);
        pickSpot();
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
        say("ugh. fine. ENTER THE ARENA.", 2.6);
        birdSay("GUNBATTE!!!", 2.4, false);      // the eruption, reply-free
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
  } else if (state === "lounge") {
    // The flop: flat, ears drooped, one ear twitching, absolutely done.
    const breathe = Math.sin(t * 1.7);
    tarsius.root.rotation = 0.07 * breathe;
    tarsius.root.position.set(pos.x, pos.y + 7);
    tarsius.root.scale.set(SCALE * 1.1, SCALE * (0.78 + breathe * 0.018));
    tarsius.footL.x = 15; tarsius.footR.x = 15;
    tarsius.footL.y = -5.5; tarsius.footR.y = 5.5;
    tarsius.earL.rotation = 0.8 + breathe * 0.05;
    tarsius.earR.rotation = -0.8 - Math.sin(t * 3.1) * 0.06;
    tarsius.armL.rotation = 2.95;
    tarsius.armR.rotation = -2.95;
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

  // --- jalak: circles the tarsius, held above it. When the big guy flops,
  // the orbit tightens and speeds up — buzzing the nap like it's urgent. ---
  orbit.x += (pos.x - orbit.x) * Math.min(1, dt * 2.2);
  orbit.y += (pos.y + ORBIT_CY - orbit.y) * Math.min(1, dt * 2.2);
  const buzzing = state === "lounge";
  const oa = (t / (buzzing ? ORBIT_T * 0.55 : ORBIT_T)) * Math.PI * 2;
  const rx = ORBIT_RX * (buzzing ? 0.78 : 1);
  const bx = orbit.x + Math.cos(oa) * rx;
  const by = orbit.y + Math.sin(oa) * ORBIT_RY;
  jalak.root.position.set(bx, by);
  const bank = Math.cos(oa) * 0.3;
  jalak.root.rotation = Math.atan2(Math.cos(oa) * ORBIT_RY, -Math.sin(oa) * rx) + bank * 0.35;
  const flap = Math.sin(t * 14);
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

  // --- speech bubbles: the bird's chatter, then the tarsius's grumbles ---
  bubbleCd -= dt;
  if (bubbleCd <= 0 && bubbleT <= 0) {
    bubbleCd = 13 + Math.random() * 9;
    say(TARSIUS_LINES[Math.floor(Math.random() * TARSIUS_LINES.length)]);
  }
  birdBubbleCd -= dt;
  if (birdBubbleCd <= 0 && birdBubbleT <= 0) {
    birdBubbleCd = 6.5 + Math.random() * 5.5;    // he talks a LOT
    birdSay(JALAK_LINES[Math.floor(Math.random() * JALAK_LINES.length)]);
  }
  if (replyText) {
    replyT -= dt;
    if (replyT <= 0) {
      say(replyText, 2.8);   // the grumble, right on cue
      replyText = null;
    }
  }
  if (bubbleT > 0) {
    bubbleT -= dt;
    bubble.visible = true;
    // Flip the bubble below the tarsius when it roams high in the card, so the
    // grumble never covers the heading or the name field.
    const bubbleAbove = tarsius.root.y > 96;
    drawBubble(bubbleBg, bubbleText, bubbleAbove);   // bubble above ⇒ tail hangs down at it
    const w = bubbleText.width / 2 + 14;
    const px = Math.min(Math.max(tarsius.root.x, w), app.screen.width - w);
    const py = bubbleAbove ? tarsius.root.y - 78 : tarsius.root.y + 74;
    bubble.position.set(px, py);
    bubble.alpha = Math.min(1, bubbleT / 0.35);
  } else {
    bubble.visible = false;
  }
  if (birdBubbleT > 0) {
    birdBubbleT -= dt;
    birdBubble.visible = true;
    // The bird's bubble always hangs above him, pink-stroked, tail down.
    drawBubble(birdBubbleBg, birdBubbleText, true, 0xff5fd0);
    const w2 = birdBubbleText.width / 2 + 14;
    const px2 = Math.min(Math.max(jalak.root.x, w2), app.screen.width - w2);
    const py2 = Math.max(28, jalak.root.y - 54);
    birdBubble.position.set(px2, py2);
    birdBubble.alpha = Math.min(1, birdBubbleT / 0.35);
  } else {
    birdBubble.visible = false;
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
  birdBubble = null;
  birdBubbleBg = null;
  birdBubbleText = null;
  replyText = null;
  host = null;
  cardEl = null;
  spots = [];
  spotI = -1;
}
