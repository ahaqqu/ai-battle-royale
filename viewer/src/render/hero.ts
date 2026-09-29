/** Home-screen mascots: Tarsius, the arena's laziest sharpshooter, lives on
 * the menu card — he flops down for a nap and only shuffles to a new spot when
 * even that gets stale, grumbling the whole way — and Jalak, his tireless
 * hype-bird, who zips around the menu introducing every feature (the queue
 * buttons, the name field, the replay library) with a shouted GUNBATTE! When
 * the nagging gets loud enough, the tarsius sits up, grumbles back, and lies
 * down again.
 *
 * Reuses the exact in-game art (makeTarsius / makeJalak) so the menu shows
 * what the match delivers. The overlay canvas covers the card exactly and is
 * pointer-transparent; waypoints are read from the real DOM elements, and the
 * mascots' "interactions" are cosmetic CSS animations — nothing is ever
 * clicked on the user's behalf. */

import { Application, Container, Graphics, Sprite, Text } from "pixi.js";
import { FONT, botColor } from "../types.js";
import { JalakArt, TarsiusArt, makeJalak, makeTarsius } from "./units.js";
import { makeGlowTexture } from "./stage.js";

const HERO_COL = parseInt(botColor(0).slice(1), 16);   // watermelon tarsius
const BIRD_COL = parseInt(botColor(2).slice(1), 16);   // banana jalak accents
const SCALE = 0.86;
/** Tarsius roam speed: a lazy amble, not a walk (card px/s). */
const SHUFFLE_SPEED = 20;
/** Jalak cruise speed — the bird is the energetic one (card px/s). */
const FLY_SPEED = 150;
/** Bird orbit around the napping tarsius. */
const ORBIT_RX = 64;
const ORBIT_RY = 20;
const ORBIT_CY = -54;
const ORBIT_T = 3.4;
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
/** Jalak's idle chatter: cheerful, loud, endless. GUNBATTE! is his trademark. */
const JALAK_LINES = [
  "GUNBATTE!!!",
  "GUNBATTE, TARSIUS!!",
  "がんばって！！",
  "GUNBATTE!!! gun up!!",
  "wakey wakey, battle time!!",
  "best shot in the arena!! GUNBATTE!!",
  "last tarsius standing!! that's you!!",
];

/** A menu control on the jalak's tour: where it sits, the cosmetic bounce it
 * gets, and the line he shouts about it. */
interface FeatureSpot { x: number; y: number; anim: "press" | "poke" | "boop"; el?: HTMLElement; intro: string; }

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

let features: FeatureSpot[] = [];
let featI = -1;
/** Tarsius real-estate: home nap spots in the top-left pocket, plus `far`
 * stroll spots he lazily ambles out to, pauses at, and shuffles back from. */
let napSpots: { x: number; y: number; far?: boolean }[] = [];
let napI = 0;
/** The spot he is currently walking toward (or flopped on). */
let tSpot: { x: number; y: number; far?: boolean } | null = null;

let pos = { x: 200, y: 300 };
let facing = 0;
let phase = Math.random() * 6.28;
/** Tarsius state machine: flop (nap) → occasionally stir → usually a slow
 * lazy walk, pausing a moment when a stroll reaches a far spot. */
let tState: "flop" | "sit" | "shuffle" | "pause" = "flop";
let tStateT = 7;

let jpos = { x: 200, y: 240 };
let jTarget = { x: 200, y: 240 };
/** Jalak state machine: orbit the tarsius → fly to a control → intro it. */
let jState: "orbit" | "fly" | "intro" = "orbit";
let jStateT = 2.4;
let jIntroFired = false;

let bubbleT = 0;
let bubbleCd = 9;
let birdBubbleT = 0;
/** The bird cannot stay quiet for long — he opens with a shout. */
let birdBubbleCd = 2.2;
/** A scheduled grumble: the reply to the trademark, fired a beat later. */
let replyText: string | null = null;
let replyT = 0;

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

  // Speech bubble for the tarsius's grumbles.
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

  // The jalak's own bubble — the tour and the GUNBATTE! trademark live here.
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
  // Lazy from frame one: he starts flopped; the bird opens with a shout.
  const nap = napSpots[0] ?? { x: 200, y: 300 };
  pos = { x: nap.x, y: nap.y };
  napI = 0;
  tSpot = null;
  tState = "flop";
  tStateT = 5;
  featI = -1;
  jState = "orbit";
  jStateT = 2.4;
  jpos = { x: pos.x, y: pos.y + ORBIT_CY };
  jTarget = { ...jpos };
  jIntroFired = false;
  bubbleT = 0;
  bubbleCd = 9;
  birdBubbleT = 0;
  birdBubbleCd = 2.2;
  replyText = null;
  replyT = 0;

  ro = new ResizeObserver(() => resize());
  if (cardEl) ro.observe(cardEl);
  window.addEventListener("resize", resize);

  onTick = () => tick(performance.now() / 1000);
  a.ticker.add(onTick);
}

/** Read the menu's real controls: the jalak's tour stops, and the floor spots
 * where the tarsius flops down. */
function measureSpots(): void {
  if (!cardEl) return;
  const cr = cardEl.getBoundingClientRect();
  const local = (el: Element): { x: number; y: number } => {
    const r = el.getBoundingClientRect();
    return { x: r.left - cr.left + r.width / 2, y: r.top - cr.top + r.height / 2 };
  };
  const q = <T extends HTMLElement>(sel: string): T | null => cardEl!.querySelector<T>(sel);
  const next: FeatureSpot[] = [];
  const add = (sel: string, anim: FeatureSpot["anim"], intro: string): void => {
    const el = q(sel);
    if (el) next.push({ ...local(el), anim, el, intro });
  };
  add("#play-btn", "press", "this one!! GUNBATTE queue — last tarsius standing!!");
  add("#boss-btn", "press", "or raid the BOSS!! one giant tarsius — good luck!!");
  add("#play-name", "poke", "type your battle name!! be a Sardine!!");
  add(".play-join h3", "boop", "humans welcome!! same queue as the bots!!");
  add("#browse-replays-btn", "press", "old battles sleep here!! watch the highlights!!");
  add(".home-keys", "boop", "WASD move!! SPACE dash!! E sonar!!");
  add(".home-tip", "boop", "grab guns!! GUNBATTE!!!");
  features = next;

  const w = cr.width;
  // Home base is the card's roomy top-left pocket (balancing the hanko on the
  // top right); the far spots give him a reason to amble out and back.
  napSpots = [
    { x: w * 0.17, y: 100 },
    { x: w * 0.14, y: 168 },
    { x: w * 0.24, y: 136 },
    { x: w * 0.8, y: 75, far: true },
    { x: w * 0.12, y: 330, far: true },
  ];
}

function resize(): void {
  if (!app || !host || !cardEl) return;
  const w = cardEl.clientWidth, h = cardEl.clientHeight;
  if (w < 50 || h < 50) return;
  app.renderer.resize(w, h);
  measureSpots();
  // Keep the napper on the resized card; the bird follows him anyway.
  pos.x = Math.min(Math.max(pos.x, 40), w - 40);
  pos.y = Math.min(pos.y, h - 30);
  jpos.x = Math.min(Math.max(jpos.x, 30), w - 30);
  jpos.y = Math.min(Math.max(jpos.y, 20), h - 30);
}

/** Send him shuffling toward a new spot (never the one he's on): a short
 * amble between home spots, or a longer lazy stroll out to a far one. */
function pickNapSpot(far: boolean): void {
  const cur = napSpots[napI];
  if (!cur) { tState = "flop"; tStateT = 5; return; }
  const pool = napSpots.filter((s) => !!s.far === far && s !== cur);
  const options = pool.length > 0 ? pool : napSpots.filter((s) => s !== cur);
  tSpot = options.length > 0 ? options[Math.floor(Math.random() * options.length)] : cur;
  tState = "shuffle";
  tStateT = 45;   // safety cap; arrival ends the walk
}

function say(text: string, hold = 3.4): void {
  if (!bubbleText) return;
  bubbleText.text = text;
  bubbleT = hold;
}

/** The bird pipes up. Shouting the trademark at his best friend usually earns
 * a grumble back a beat later — that's the whole bit. Tour intros pass
 * provokeReply=false: the feature pitch isn't a personal attack. */
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

/** Frame driver: the tarsius naps through it all while the jalak runs his
 * feature tour and never, ever stops talking. */
function tick(t: number): void {
  if (!app || !tarsius || !jalak || !birdShadow || !bubble || !bubbleBg || !bubbleText) return;
  if (!birdBubble || !birdBubbleBg || !birdBubbleText) return;
  const dt = Math.min(0.05, app.ticker.deltaMS / 1000);

  // ---- tarsius: nap → (usually) a slow lazy walk somewhere → flop again
  tStateT -= dt;
  if (tStateT <= 0 && tState !== "shuffle") {
    if (tState === "flop") {
      const roll = Math.random();
      if (roll < 0.18) {
        // A brief stir: sit up, grumble about it, flop right back down.
        tState = "sit";
        tStateT = 1.8 + Math.random() * 1.8;
        if (Math.random() < 0.6) say(TARSIUS_LINES[Math.floor(Math.random() * TARSIUS_LINES.length)]);
      } else {
        // Up again — mostly a short shuffle inside the pocket, sometimes a
        // long lazy stroll out to a far spot (and back home after a pause).
        pickNapSpot(Math.random() < 0.3);
      }
    } else if (tState === "sit") {
      tState = "flop";
      tStateT = 4 + Math.random() * 4;
    } else {
      // Stroll over: shuffle home to the pocket and flop down.
      pickNapSpot(false);
    }
  }

  let shuffling = false;
  if (tState === "shuffle" && tSpot) {
    const dx = tSpot.x - pos.x, dy = tSpot.y - pos.y;
    const d = Math.hypot(dx, dy);
    if (d < 3 || tStateT <= 0) {
      pos.x = tSpot.x; pos.y = tSpot.y;
      napI = Math.max(0, napSpots.indexOf(tSpot));
      if (tSpot.far) {
        // A lazy pause: stand there a moment, absolutely unbothered.
        tState = "pause";
        tStateT = 2.2 + Math.random() * 1.2;
      } else {
        tState = "flop";
        tStateT = 4 + Math.random() * 4;
      }
    } else {
      const step = Math.min(d, SHUFFLE_SPEED * dt);
      pos.x += (dx / d) * step;
      pos.y += (dy / d) * step;
      facing = Math.atan2(dy, dx);
      phase += dt * 3.4;   // slow feet for a slow walk
      shuffling = true;
    }
  }

  // --- tarsius pose ---
  if (shuffling) {
    // The shuffle: low to the ground, ears drooped, feet barely lifting.
    const sw = Math.sin(phase);
    tarsius.root.rotation = facing + sw * 0.03;
    tarsius.root.position.set(pos.x, pos.y - Math.abs(Math.cos(phase)) * 1.4);
    tarsius.footL.x = 13 + sw * 5;
    tarsius.footR.x = 13 - sw * 5;
    tarsius.footL.y = -7 - Math.max(0, Math.cos(phase)) * 1.6;
    tarsius.footR.y = 7 - Math.max(0, -Math.cos(phase)) * 1.6;
    tarsius.armL.rotation = 2.75 + sw * 0.1;
    tarsius.armR.rotation = -2.75 - sw * 0.1;
    tarsius.earL.rotation = 0.6 + sw * 0.04;
    tarsius.earR.rotation = -0.6 - sw * 0.04;
    tarsius.root.scale.set(SCALE * 1.03, SCALE * 0.94);
    tarsius.blinkTarget.scale.y = 1;
  } else if (tState === "sit" || tState === "pause") {
    // Sat up, still half asleep: slow sway, drooped ears, heavy breathing,
    // one long blink every few seconds.
    const breathe = Math.sin(t * 1.5);
    tarsius.root.rotation = Math.sin(t * 0.8) * 0.05;
    tarsius.root.position.set(pos.x, pos.y);
    tarsius.root.scale.set(SCALE, SCALE * (1 + breathe * 0.016));
    tarsius.footL.x = 13; tarsius.footR.x = 13;
    tarsius.footL.y = -8; tarsius.footR.y = 8;
    tarsius.earL.rotation = 0.55 + Math.sin(t * 2.1) * 0.04;
    tarsius.earR.rotation = -0.55 - Math.sin(t * 2.7) * 0.05;
    tarsius.armL.rotation = 2.8;
    tarsius.armR.rotation = -2.8;
    tarsius.blinkTarget.scale.y = (t % 3.6) < 0.22 ? 0.12 : 1;
  } else {
    // The flop: flat, ears drooped, eyes shut, one ear twitching, done.
    const breathe = Math.sin(t * 1.3);
    tarsius.root.rotation = 0.07 * breathe;
    tarsius.root.position.set(pos.x, pos.y + 7);
    tarsius.root.scale.set(SCALE * 1.1, SCALE * (0.78 + breathe * 0.018));
    tarsius.footL.x = 15; tarsius.footR.x = 15;
    tarsius.footL.y = -5.5; tarsius.footR.y = 5.5;
    tarsius.earL.rotation = 0.8 + breathe * 0.05;
    tarsius.earR.rotation = -0.8 - Math.sin(t * 3.1) * 0.06;
    tarsius.armL.rotation = 2.95;
    tarsius.armR.rotation = -2.95;
    tarsius.blinkTarget.scale.y = 0.14 + Math.max(0, Math.sin(t * 1.9)) * 0.1;
  }

  // ---- jalak: the tour guide. Orbits his best friend between stops, then
  // zips to a control, boops it, and shouts what it does. ----
  jStateT -= dt;
  const pjx = jpos.x, pjy = jpos.y;
  if (jState === "orbit") {
    const oa = (t / ORBIT_T) * Math.PI * 2;
    const tx = pos.x + Math.cos(oa) * ORBIT_RX;
    const ty = pos.y + ORBIT_CY + Math.sin(oa) * ORBIT_RY;
    jpos.x += (tx - jpos.x) * Math.min(1, dt * 3.4);
    jpos.y += (ty - jpos.y) * Math.min(1, dt * 3.4);
    if (jStateT <= 0 && features.length > 0) {
      let i = featI;
      for (let tries = 0; tries < 8; i = Math.floor(Math.random() * features.length), tries++) {
        if (i !== featI) break;
      }
      featI = i;
      jTarget = { x: features[i].x, y: features[i].y - 26 };   // hover above it
      jState = "fly";
      jIntroFired = false;
    }
  } else if (jState === "fly") {
    const dx = jTarget.x - jpos.x, dy = jTarget.y - jpos.y;
    const d = Math.hypot(dx, dy);
    if (d < 5) {
      jpos.x = jTarget.x; jpos.y = jTarget.y;
      jState = "intro";
      jStateT = 3.4;
    } else {
      const step = Math.min(d, FLY_SPEED * dt);
      jpos.x += (dx / d) * step;
      jpos.y += (dy / d) * step;
    }
  } else {
    // Hover over the control with a gentle bob while the intro plays.
    jpos.x = jTarget.x + Math.sin(t * 2.4) * 3;
    jpos.y = jTarget.y + Math.sin(t * 4.4) * 4;
    if (!jIntroFired) {
      jIntroFired = true;
      const f = features[featI];
      if (f) {
        if (f.anim === "press") pressEl(f.el);
        else if (f.anim === "poke") pokeEl(f.el);
        else boopEl(f.el);
        birdSay(f.intro, 3.1, false);
      }
    }
    if (jStateT <= 0) {
      jState = "orbit";
      jStateT = 4.5 + Math.random() * 5;
    }
  }

  // The bird faces where he's going; hovering, he eases level for the pitch.
  jalak.root.position.set(jpos.x, jpos.y);
  const jvx = jpos.x - pjx, jvy = jpos.y - pjy;
  if (jState === "intro") {
    jalak.root.rotation *= Math.max(0, 1 - dt * 5);
  } else if (Math.hypot(jvx, jvy) > dt * 24) {
    jalak.root.rotation = Math.atan2(jvy, jvx);
  }

  // Flapping never stops — wings, tail, head, the whole bird is busy.
  const flap = Math.sin(t * 14);
  const span = 0.62 + Math.abs(flap) * 0.5;
  jalak.wingL.scale.set(1 - Math.abs(flap) * 0.16, span);
  jalak.wingR.scale.set(1 - Math.abs(flap) * 0.16, span);
  jalak.wingL.position.y = -2.6 * span - flap * 1.1;
  jalak.wingR.position.y = 2.6 * span + flap * 1.1;
  jalak.tail.rotation = Math.sin(t * 11 - 0.7) * 0.22;
  jalak.head.rotation = -0.08;
  jalak.root.scale.set(SCALE * (1 - Math.abs(flap) * 0.03), SCALE * (1 + flap * 0.05));
  // Shadow slides along the tarsius's floor under the bird; higher flight =
  // fainter, smaller shadow.
  const groundY = pos.y + 34;
  const shrink = Math.min(0.5, Math.max(0, groundY - jpos.y) / 420);
  birdShadow.position.set(jpos.x, groundY);
  birdShadow.alpha = 0.3 - shrink * 0.35;
  birdShadow.scale.set(0.5 - shrink * 0.3, 0.26 - shrink * 0.16);

  // --- speech bubbles: the bird's tour + chatter, the tarsius's grumbles ---
  bubbleCd -= dt;
  if (bubbleCd <= 0 && bubbleT <= 0 && tState !== "shuffle") {
    bubbleCd = 16 + Math.random() * 10;   // grumbles are rare; he's asleep
    say(TARSIUS_LINES[Math.floor(Math.random() * TARSIUS_LINES.length)]);
  }
  birdBubbleCd -= dt;
  if (birdBubbleCd <= 0 && birdBubbleT <= 0 && jState !== "intro") {
    birdBubbleCd = 5.5 + Math.random() * 5;    // he talks a LOT
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
    // He naps up top now, so the grumble hangs below him; flip it above only
    // when he's near the card's bottom edge, so it never leaves the card.
    const bubbleAbove = tarsius.root.y > app.screen.height - 180;
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

/** Cosmetic menu reactions — the mascots never actually activate a control. */
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
  features = [];
  featI = -1;
  napSpots = [];
  tSpot = null;
  napI = 0;
}
