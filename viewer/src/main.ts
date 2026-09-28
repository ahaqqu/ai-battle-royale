/** Viewer entrypoint: replay playback, auto-director, mind-cam, and the
 * hybrid human play client (humans join the same queue as AI bots). */

import { Graphics } from "pixi.js";
import { CamMode, Frame, MapData, ReplayData, botColor } from "./types.js";
import { buildPlayerCam, LoadedReplay, loadReplay } from "./sim.js";
import { PlayClient } from "./play.js";
import { Stage } from "./render/stage.js";
import { drawArena } from "./render/arena.js";
import { UnitViews } from "./render/units.js";
import { Fx } from "./render/fx.js";
import { PickupLayer, ProjectileLayer, ZoneLayerView } from "./render/world.js";
import { FogView } from "./render/fog.js";
import { MindCam } from "./render/mindcam.js";
import { Director } from "./render/director.js";
import { Hud } from "./ui/hud.js";
import { Timeline } from "./ui/timeline.js";
import { sfx, panVol } from "./audio.js";
import { startAmbient, stopAmbient } from "./ambient.js";
import { startHero, stopHero } from "./render/hero.js";

const TICK_RATE = 10;

(window as unknown as { __errors: string[] }).__errors = [];
window.addEventListener("error", (e) => {
  (window as unknown as { __errors: string[] }).__errors.push(String(e.message));
});
window.addEventListener("unhandledrejection", (e) => {
  const reason = (e as PromiseRejectionEvent).reason;
  (window as unknown as { __errors: string[] }).__errors.push("rejection: " + String(reason?.stack ?? reason));
});

const loading = document.getElementById("loading")!;
const loadStatus = document.getElementById("load-status")!;
const loadBar = document.getElementById("load-bar")!;
const picker = document.getElementById("picker")!;
const replaysPage = document.getElementById("replays-page")!;

const stage = new Stage();
const hud = new Hud();
const timeline = new Timeline();
const director = new Director();

let replay: LoadedReplay | null = null;
let unitViews: UnitViews | null = null;
let projs: ProjectileLayer | null = null;
let pickups: PickupLayer | null = null;
let zoneView: ZoneLayerView | null = null;
let fx: Fx | null = null;
let fog: FogView | null = null;
let mindcam: MindCam | null = null;
let mapCache: MapData | null = null;
let mindHeat: import("pixi.js").Graphics | null = null;
let mindBubbles: import("pixi.js").Graphics | null = null;

let idx = 0;            // fractional frame cursor
let playing = true;
let speed = 1;
let lastTs = performance.now();
let firedEvents = new Set<number>();
let camData: { bot: number; frames: unknown[] } | null = null;
let placementsFinal: number[] = [];
let winnerShown = false;
/** Juice: freeze/slow the timeline on big moments (PLAN §7.3). */
let hitstopUntil = 0;

function setProgress(p: number, label: string): void {
  loadBar.style.width = `${Math.min(100, p * 100)}%`;
  loadStatus.textContent = label;
}

async function boot(): Promise<void> {
  try {
    const params = new URLSearchParams(location.search);
    const replayParam = params.get("replay");
    if (replayParam) {
      await startReplayUrl(replayParam);
    } else if (params.has("play")) {
      await startPlay((params.get("name") || "human").replace(/[^a-zA-Z0-9_-]/g, "").slice(0, 16) || "human");
    } else {
      await showPicker();
    }
  } catch (e) {
    // Never leave a black screen: surface fatal boot errors on the overlay.
    loadStatus.textContent = "failed to start: " + String((e as Error)?.message ?? e);
    (window as unknown as { __errors: string[] }).__errors.push("boot: " + String((e as Error)?.stack ?? e));
  }
}

interface ReplayItem { name: string; url: string; size_kb: number }

const REPLAYS_PER_PAGE = 10;
let replaysCache: ReplayItem[] | null = null;
let replaysPageNum = 0;

function hideMenus(): void {
  picker.classList.add("hidden");
  replaysPage.classList.add("hidden");
  stopHero();
  sfx.stopMenuTheme();
}

async function listReplays(): Promise<ReplayItem[]> {
  if (replaysCache) return replaysCache;
  const res = await fetch("./api/replays");
  replaysCache = await res.json();
  return replaysCache!;
}

async function showPicker(): Promise<void> {
  loading.classList.add("hidden");
  hideMenus();
  picker.classList.remove("hidden");
  sfx.startMenuTheme();
  // Home mascots: the dancing tarsius with its jalak circling overhead.
  void startHero(document.getElementById("hero-host")!);
  // Ambient home screen: the newest recorded match plays behind the menu.
  startAmbient(stage, ensureStage);

  // Badge the library button with the replay count (metadata-only fetch).
  listReplays().then((items) => {
    if (items.length === 0) return;
    const badge = document.getElementById("replay-count")!;
    badge.textContent = String(items.length);
    badge.classList.remove("hidden");
  }).catch(() => { /* offline: button still opens the library page */ });

  const fileInput = document.getElementById("file-input") as HTMLInputElement;
  fileInput.addEventListener("change", async () => {
    const f = fileInput.files?.[0];
    if (!f) return;
    const text = await f.text();
    await startReplay(text, f.name);
  });

  // Hybrid play: humans enter the same queue as the AI bots.
  const playBtn = document.getElementById("play-btn") as HTMLButtonElement | null;
  const nameInput = document.getElementById("play-name") as HTMLInputElement | null;
  if (playBtn && nameInput) {
    playBtn.addEventListener("click", () => {
      const name = (nameInput.value || "human").replace(/[^a-zA-Z0-9_-]/g, "").slice(0, 16) || "human";
      location.href = "?play=1&name=" + encodeURIComponent(name);
    });
  }

  document.getElementById("browse-replays-btn")!.addEventListener("click", () => {
    location.hash = "#replays";
  });
  document.getElementById("replays-back")!.addEventListener("click", () => {
    location.hash = "";
  });
  document.getElementById("replays-prev")!.addEventListener("click", () => {
    if (replaysPageNum > 0) { replaysPageNum--; renderReplays(); }
  });
  document.getElementById("replays-next")!.addEventListener("click", () => {
    const items = replaysCache ?? [];
    if ((replaysPageNum + 1) * REPLAYS_PER_PAGE < items.length) { replaysPageNum++; renderReplays(); }
  });
  window.addEventListener("hashchange", () => {
    if (location.hash === "#replays") {
      // Don't overlay the library on a running replay/play session.
      if (!replay && !playClient) void openReplays();
      else location.hash = "";
    } else if (!picker.classList.contains("hidden") || !replaysPage.classList.contains("hidden")) {
      hideMenus();
      picker.classList.remove("hidden");
      void startHero(document.getElementById("hero-host")!);
    }
  });

  if (location.hash === "#replays") void openReplays();
}

async function openReplays(): Promise<void> {
  hideMenus();
  replaysPage.classList.remove("hidden");
  const list = document.getElementById("replays-list")!;
  const indicator = document.getElementById("replays-page-indicator")!;
  let items: ReplayItem[];
  try {
    items = await listReplays();
  } catch {
    list.innerHTML = `<div style="color:var(--text-dim);font-size:13px;padding:12px 0">No replay server detected.<br>Open a replay file from the home screen.</div>`;
    indicator.textContent = "—";
    return;
  }
  if (items.length === 0) {
    list.innerHTML = `<div style="color:var(--text-dim);font-size:13px;padding:12px 0">No replays found on the server.<br>Generate one: <code>abr-runner run --preset default16 --seed 42 --out replays/match.json</code></div>`;
    indicator.textContent = "—";
    return;
  }
  replaysPageNum = Math.min(replaysPageNum, Math.floor((items.length - 1) / REPLAYS_PER_PAGE));
  renderReplays();
}

function renderReplays(): void {
  const items = replaysCache ?? [];
  const list = document.getElementById("replays-list")!;
  list.innerHTML = "";
  const start = replaysPageNum * REPLAYS_PER_PAGE;
  for (const it of items.slice(start, start + REPLAYS_PER_PAGE)) {
    const row = document.createElement("div");
    row.className = "picker-row";
    const name = document.createElement("span");
    name.textContent = it.name;
    const meta = document.createElement("span");
    meta.className = "picker-meta";
    meta.textContent = `${it.size_kb.toFixed(0)} KB`;
    row.append(name, meta);
    row.addEventListener("click", () => startReplayUrl(it.url));
    list.appendChild(row);
  }
  const pages = Math.max(1, Math.ceil(items.length / REPLAYS_PER_PAGE));
  document.getElementById("replays-page-indicator")!.textContent = `${replaysPageNum + 1} / ${pages}`;
  (document.getElementById("replays-prev") as HTMLButtonElement).disabled = replaysPageNum <= 0;
  (document.getElementById("replays-next") as HTMLButtonElement).disabled = replaysPageNum + 1 >= pages;
}

// Stage init is shared by the ambient background, replay playback and live
// play; memoize the in-flight promise so concurrent callers (e.g. the user
// clicks a replay while the ambient background is still initializing) wait
// on the same init instead of racing past didInit into an uninitialized stage.
let stageReady: Promise<void> | null = null;

function ensureStage(): Promise<void> {
  stageReady ??= initStage();
  return stageReady;
}

async function initStage(): Promise<void> {
  try {
    // Give the display font a beat to load so Pixi-canvas text doesn't bake
    // in the fallback; never block longer than ~1.2s (offline is fine).
    try {
      await Promise.race([
        (document as Document & { fonts?: FontFaceSet }).fonts?.load('800 16px "Baloo 2"') ?? Promise.resolve(),
        new Promise((r) => setTimeout(r, 1200)),
      ]);
    } catch { /* font stays fallback */ }
    await stage.init(document.getElementById("stage-host")!, (s) => setProgress(0.005, s));
    if (!mindcam && stage.didInit) {
      mindHeat = new Graphics();
      mindBubbles = new Graphics();
      stage.fogLayer.addChild(mindHeat, mindBubbles);
      mindcam = new MindCam(mindHeat, mindBubbles);
    }
  } catch (e) {
    stageReady = null; // allow a later attempt to actually retry
    throw e;
  }
}

async function fetchMap(mapId: string): Promise<MapData> {
  if (mapCache && (mapCache as any).id === mapId) return mapCache;
  try {
    const res = await fetch(`./api/map/${mapId}`);
    if (res.ok) {
      mapCache = await res.json();
      return mapCache!;
    }
  } catch { /* offline */ }
  return { id: mapId, size: 3200, walls: [], spawns: [] };
}

async function startReplayUrl(url: string): Promise<void> {
  const name = decodeURIComponent(url.split("/").pop() ?? "replay");
  const res = await fetch(url);
  const json = await res.text();
  await startReplay(json, name);
}

async function startReplay(json: string, name: string): Promise<void> {
  hideMenus();
  stopAmbient(stage);
  loading.classList.remove("hidden");
  setProgress(0.01, "loading replay…");
  await ensureStage();
  replay = await loadReplay(json, setProgress);
  const data = replay.data;

  document.title = `${name} — AI Battle Royale`;
  loading.classList.add("hidden");

  drawArena(stage, data.map);
  unitViews = new UnitViews(stage, data.botNames);
  projs = new ProjectileLayer(stage);
  pickups = new PickupLayer(stage);
  zoneView = new ZoneLayerView(stage);
  fx = new Fx(stage);
  fog = new FogView(stage);

  hud.showAll();
  hud.setHeader(data.botNames, data.mapId, data.seed);
  hud.buildLegend(data.botNames, (bot) => {
    setCamMode(`follow:${bot}`);
  });
  timeline.show();
  timeline.setRange(data.totalTicks);
  timeline.addMarkers(data.killMarkers, data.botNames);
  timeline.setCamOptions(data.botNames, setCamMode);
  timeline.onPlayToggle = () => { playing = !playing; };
  timeline.onSeek = (frac) => { idx = frac * (data.totalTicks - 1); winnerShown = false; hud.hideWinner(); };
  timeline.onSpeed = (s) => { speed = s; };
  timeline.onCam = setCamMode;

  stage.onWheel = (delta) => {
    const t = stage.getTarget();
    stage.setTarget(t.x, t.y, Math.min(3, Math.max(0.12, t.zoom * (delta > 0 ? 0.88 : 1.14))));
    director.mode = "global"; // manual zoom takes over
    timeline.camSelect.value = "global";
  };
  stage.onDrag = (dx, dy) => {
    const t = stage.getTarget();
    stage.setTarget(t.x - dx / t.zoom, t.y - dy / t.zoom, t.zoom);
    director.mode = "global";
    timeline.camSelect.value = "global";
  };

  placementsFinal = computePlacements(data);

  requestAnimationFrame(loop);
}

function setCamMode(mode: string): void {
  director.mode = mode as CamMode;
  timeline.camSelect.value = mode;
  const camBot = mode.startsWith("cam:") ? Number(mode.slice(4)) : null;
  if (camBot !== null) {
    fog?.show();
    if (!camData || camData.bot !== camBot) {
      // Build the fog view lazily with a mini progress overlay.
      playing = false;
      loading.classList.remove("hidden");
      setProgress(0, `reconstructing ${replay!.data.botNames[camBot]}'s memory…`);
      buildPlayerCam(replay!.sim, camBot, (p) => setProgress(p, `reconstructing memory… ${Math.round(p * 100)}%`))
        .then((cam) => {
          camData = { bot: camBot, frames: cam.frames };
          loading.classList.add("hidden");
          playing = true;
        });
    }
  } else {
    fog?.hide();
  }
  hud.legendActive(mode.startsWith("follow:") ? Number(mode.slice(7)) : mode.startsWith("cam:") ? Number(mode.slice(4)) : null);
}

/** Placement order: winner first, then elimination order reversed. */
function computePlacements(data: ReplayData): number[] {
  const n = data.botNames.length;
  const order: number[] = [];
  if (data.winner !== null) order.push(data.winner);
  const seen = new Set(order);
  for (let i = data.killMarkers.length - 1; i >= 0; i--) {
    const v = data.killMarkers[i].victim;
    if (!seen.has(v)) { seen.add(v); order.push(v); }
  }
  for (let b = 0; b < n; b++) if (!seen.has(b)) { order.push(b); seen.add(b); }
  return order;
}

function loop(ts: number): void {
  requestAnimationFrame(loop);
  const dt = Math.min(0.1, (ts - lastTs) / 1000);
  lastTs = ts;
  if (!replay || !unitViews) return;
  const data = replay.data;
  const total = data.totalTicks;

  if (playing) {
    // Slow-mo kill cam: the final seconds play at 30% speed (PLAN §6.2).
    const effSpeed = idx > total - 25 ? speed * 0.3 : speed;
    if (ts >= hitstopUntil) {
      idx += dt * TICK_RATE * effSpeed;
    }
    if (idx >= total - 1) {
      idx = total - 1;
      playing = false;
      if (!winnerShown && data.winner !== null) {
        winnerShown = true;
        hud.winner(data.winner, data.botNames, placementsFinal);
        sfx.play("victory", 0, 1);
      }
    }
  }
  const iA = Math.min(total - 1, Math.max(0, Math.floor(idx)));
  const iB = Math.min(total - 1, iA + 1);
  const t = idx - iA;
  const frameA = data.frames[iA];
  const frameB = data.frames[iB];

  // Fire FX once per crossed frame.
  if (!firedEvents.has(iB)) {
    firedEvents.add(iB);
    for (const e of frameB.events) {
      fx!.handleEvents([e]);
      // Audio: distance-attenuated, stereo-panned around the camera.
      const at = (e.at ?? e.from) as [number, number] | undefined;
      if (at) {
        const { pan, vol } = panVol(stage.cam, at, window.innerWidth);
        switch (e.type) {
          case "shot": sfx.play("shot", pan, vol); break;
          case "hit": sfx.play("hit", pan, vol); break;
          case "death": sfx.play("boom", pan, Math.max(0.55, vol)); break;
          case "companion_down": sfx.play("boom", pan, vol * 0.45); break;
          case "sonar": sfx.play("sonar", pan, vol); break;
          case "pickup": sfx.play("pickup", pan, vol * 0.8); break;
        }
      } else if (e.type === "zone_locked" || e.type === "zone_shrink_started") {
        sfx.play("zone", 0, 0.85);
      }
      if (e.type === "death") {
        const bot = e.bot as number;
        hud.legendDead(bot);
        hud.legendElim(bot, `t${frameB.tick}`);
        hud.kill((e.killer as number | null) ?? null, bot, data.botNames);
        // Hitstop on every elimination: 120ms freeze (PLAN §7.3 juice).
        hitstopUntil = ts + 120;
      }
    }
  }

  // World layers (units interpolate A→B for smooth 60fps motion).
  unitViews.update(frameA.units, frameB.units, t, frameA.unitCount, true);
  projs!.update(
    frameA.projs, frameA.projCount,
    frameB.projs, frameB.projCount, t,
    (x, y, color, intense) => fx!.tracer(x, y, color, intense),
  );
  pickups!.update(frameA.pickups, frameA.pickupCount, frameA.tick);
  const shrinking = Math.abs(frameB.zone[2] - frameA.zone[2]) > 0.0001;
  zoneView!.update(frameA.zone, shrinking, frameA.zone[5] > 0);

  // Dash afterimages: dashing mains leave a glowing wake; sprinters kick dust.
  for (let s = 0; s < frameA.unitCount; s++) {
    const o = s * 11;
    const fl = frameA.units[o + 9];
    if ((fl & 4) !== 0) {
      const bot = frameA.units[o + 1];
      fx!.tracer(frameA.units[o + 3], frameA.units[o + 4], parseInt(botColor(bot).slice(1), 16), false);
    } else if ((fl & 3) === 3 && Math.random() < 0.1) {
      fx!.dust(frameA.units[o + 3] - frameA.units[o + 5] * 0.05, frameA.units[o + 4] - frameA.units[o + 6] * 0.05);
    }
  }

  // Mind-cam overlay (M toggles).
  const botPositions = new Map<number, { x: number; y: number }>();
  for (let s = 0; s < frameA.unitCount; s += 2) {
    if ((frameA.units[s * 11 + 9] & 1) !== 0) {
      botPositions.set(frameA.units[s * 11 + 1], { x: frameA.units[s * 11 + 3], y: frameA.units[s * 11 + 4] });
    }
  }
  mindcam?.update(frameA.minds, Math.floor(frameA.tick / 5), botPositions);

  // Fog (player-cam).
  const mode = director.mode as CamMode;
  if (mode.startsWith("cam:") && camData) {
    const camFrame = (camData.frames as (import("./types.js").CamFrame | null)[])[iB] ?? (camData.frames as unknown as (import("./types.js").CamFrame | null)[])[iA];
    if (camFrame) {
      fog!.update(camFrame, camData.bot);
    }
  }

  // Camera.
  const camBot = mode.startsWith("follow:") ? Number(mode.slice(7))
    : mode.startsWith("cam:") ? Number(mode.slice(4)) : null;
  let botPos: { x: number; y: number } | null = null;
  if (camBot !== null && camBot * 2 < frameA.unitCount) {
    const o = (camBot * 2) * 11;
    botPos = { x: frameA.units[o + 3], y: frameA.units[o + 4] };
  }
  const target = director.targetFor(frameA, frameB, botPos);
  stage.setTarget(target.x, target.y, target.zoom);

  // HUD.
  hud.stats(frameA.tick, frameA.zone[6], zonePhaseOf(frameA), shrinking);
  timeline.update(idx / Math.max(1, total - 1), frameA.tick, playing);

  fx!.update(dt);
}

function zonePhaseOf(frame: Frame): number {
  const r = frame.zone[2];
  const ladder = [1600, 1200, 850, 550, 300, 0];
  for (let i = 0; i < ladder.length; i++) {
    if (r > ladder[i] - 1) return i;
  }
  return ladder.length - 1;
}

// ---------------------------------------------------------------------------
// Hybrid play: a human enters the same bot queue as the AI (PLAN §4.1).
// The page loads fresh via ?play=1&name=<name> so no replay state lingers.
// ---------------------------------------------------------------------------

let playClient: PlayClient | null = null;
let playUnits: UnitViews | null = null;
let playZone: ZoneLayerView | null = null;
let playFx: Fx | null = null;
let playFog: FogView | null = null;
let playLoopRunning = false;
let playEntrants: string[] = [];
let playYouIndex = 0;
// Combat-feel state: diffs between the last two observations drive the
// hit confirms, hurt flashes, kill feed and audio (all client-side juice).
let prevHp = 100;
let prevEnergy = 100;
let prevMainAlive = true;
let prevCompAlive = true;
let prevEnemyHp = new Map<number, number>();
let prevMyShots = new Set<number>();
let killProcessed = 0;
let lastZoneBeep = 0;
let bannerTimer = 0;
// Play-mode FX state: last-seen enemy positions (death blasts), dash/shield
// rising edges, last companion position, and a camera zoom punch on kills.
let lastSeenPos = new Map<number, [number, number]>();
let prevDashOn = false;
let prevShieldOn = false;
let prevCompPos: [number, number] | null = null;
let zoomPunch = 0;

const reticle = document.getElementById("reticle")!;
const vignette = document.getElementById("vignette")!;
const dmgArrow = document.getElementById("dmg-arrow")!;
const playBanner = document.getElementById("play-banner")!;

/** One-shot center banner ("ELIMINATED!", zone warnings). */
function showBanner(text: string, color: string, ms = 1600): void {
  playBanner.textContent = text;
  playBanner.style.color = color;
  playBanner.classList.remove("hidden");
  playBanner.style.animation = "none";
  void playBanner.offsetWidth; // restart the pop animation
  playBanner.style.animation = "";
  clearTimeout(bannerTimer);
  bannerTimer = window.setTimeout(() => playBanner.classList.add("hidden"), ms);
}

function flashVignette(strength: number): void {
  vignette.style.opacity = String(Math.min(0.9, strength));
  setTimeout(() => { vignette.style.opacity = "0"; }, 60);
}

/** Direction the last hit came from: rotate the edge arrow, auto-hide. */
function showDamageArrow(angleRad: number): void {
  dmgArrow.classList.remove("hidden");
  dmgArrow.style.transform = `translate(-50%, -50%) rotate(${angleRad}rad)`;
  clearTimeout((showDamageArrow as unknown as { t?: number }).t);
  (showDamageArrow as unknown as { t?: number }).t = window.setTimeout(
    () => dmgArrow.classList.add("hidden"), 700,
  );
}

function updateReticle(): void {
  if (!playClient) return;
  const m = playClient.mouseScreen;
  reticle.style.transform = `translate(${m.x}px, ${m.y}px) translate(-50%, -50%)`;
  reticle.classList.toggle("fire", playClient.firing);
  const obs = playClient.lastObs;
  reticle.classList.toggle("cd", !!obs && (obs.you.main.cooldown.fire ?? 0) > 0);
  reticle.classList.toggle("sprint", playClient.sprinting);
}

function playOverShow(crown: string, title: string, sub: string): void {
  const card = document.querySelector("#play-over .crown")!;
  card.textContent = crown;
  document.getElementById("play-over-title")!.textContent = title;
  const subEl = document.getElementById("play-over-sub")!;
  subEl.innerHTML = sub;
  document.getElementById("play-over")!.classList.remove("hidden");
}

function playOverHide(): void {
  document.getElementById("play-over")!.classList.add("hidden");
}

function setPlayStatus(s: string, detail?: string): void {
  const el = document.getElementById("play-status")!;
  el.textContent = detail ? s + " — " + detail : s;
  document.getElementById("play-hud")!.classList.remove("hidden");
}

async function startPlay(name: string): Promise<void> {
  hideMenus();
  stopAmbient(stage);
  await ensureStage();
  loading.classList.add("hidden");
  document.body.classList.add("playing");

  const map = await fetchMap("arena-1");
  drawArena(stage, map);
  playZone = new ZoneLayerView(stage);
  playFx = new Fx(stage);
  playFog = new FogView(stage);

  document.getElementById("topbar")!.classList.remove("hidden");
  document.getElementById("killfeed")!.classList.remove("hidden");
  document.getElementById("play-hud")!.classList.remove("hidden");
  reticle.classList.remove("hidden");
  setPlayStatus("connecting…");

  playClient = new PlayClient(name, {
    onStatus: setPlayStatus,
    onStart: (youIndex, entrants) => {
      playEntrants = entrants;
      playYouIndex = youIndex;
      const realNames = entrants.map((n, i) => (i === youIndex ? n + " (YOU)" : n));
      playUnits = new UnitViews(stage, realNames);
      hud.setHeader(realNames, "live match", 0);
      prevHp = 100; prevEnergy = 100;
      prevMainAlive = true; prevCompAlive = true;
      prevEnemyHp = new Map(); prevMyShots = new Set();
      killProcessed = 0;
      lastSeenPos = new Map();
      prevDashOn = false; prevShieldOn = false; prevCompPos = null;
      zoomPunch = 0;
      playOverHide();
      setPlayStatus("in match — good luck");
    },
    onObs: () => {
      if (!playLoopRunning) {
        playLoopRunning = true;
        requestAnimationFrame(playLoop);
      }
    },
    onOver: (place, replayUrl) => {
      setPlayStatus("match over — place " + place);
      const watch = replayUrl
        ? `<a href='${replayUrl}' target='_blank'>▶ watch the replay</a> · `
        : "";
      if (place === 1) {
        sfx.play("victory", 0, 1);
        playOverShow("👑", "VICTORY ROYALE!", `${watch}you outlasted the whole lobby`);
      } else {
        sfx.play("defeat", 0, 0.9);
        playOverShow("💀", `#${place} PLACE`, `${watch}next match starts soon — you're re-queued`);
      }
    },
  });

  document.getElementById("play-leave")!.addEventListener("click", () => {
    playClient?.leave();
    location.href = location.pathname; // back to the home screen
  });

  const wsProto = location.protocol === "https:" ? "wss" : "ws";
  playClient.attachInput(document.getElementById("stage-host")!, (x, y) => stage.screenToWorld(x, y));
  playClient.connect(wsProto + "://" + location.host + "/ws/bot");
  requestAnimationFrame(playLoop);
}

function playLoop(ts: number): void {
  if (!playClient) { playLoopRunning = false; return; }
  requestAnimationFrame(playLoop);
  const dt = Math.min(0.1, (ts - lastTs) / 1000);
  lastTs = ts;
  updateReticle();
  const obs = playClient.lastObs;
  if (!obs || !playEntrants.length) {
    playFx?.update(dt);
    return;
  }
  if (!playUnits) return;
  const bots = obs.global.bots;
  const me = obs.you.main;
  const names = playEntrants.map((n, i) => (i === playYouIndex ? n + " (YOU)" : n));

  // Frame-shaped float view: self from obs.you, enemies through fog.
  const units = new Float32Array(bots * 2 * 11);
  const setUnit = (slot: number, id: number, bot: number, kind: number, u: { pos: [number, number]; vel?: [number, number]; facing?: number; hp?: number; alive: boolean; status?: string[]; maxhp: number }) => {
    const o = slot * 11;
    units[o] = id; units[o + 1] = bot; units[o + 2] = kind;
    units[o + 3] = u.pos[0]; units[o + 4] = u.pos[1];
    units[o + 5] = u.vel?.[0] ?? 0; units[o + 6] = u.vel?.[1] ?? 0;
    units[o + 7] = u.facing ?? 0;
    units[o + 8] = (u.hp ?? 0) / u.maxhp;
    units[o + 9] = (u.alive ? 1 : 0) | (u.status?.includes("sprint") ? 2 : 0) | (u.status?.includes("dashing") ? 4 : 0) | (u.status?.includes("shielding") ? 8 : 0);
    units[o + 10] = u.maxhp;
  };
  setUnit(playYouIndex * 2, 1 + playYouIndex, playYouIndex, 0, {
    pos: me.pos, vel: me.vel, facing: me.facing, hp: me.hp,
    alive: me.alive, status: me.status, maxhp: 100,
  });
  if (obs.you.companion.pos && obs.you.companion.alive) {
    setUnit(playYouIndex * 2 + 1, 101 + playYouIndex, playYouIndex, 1, {
      pos: obs.you.companion.pos, vel: obs.you.companion.vel, facing: obs.you.companion.facing,
      hp: obs.you.companion.hp, alive: true, maxhp: 30,
    });
  }
  for (const p of obs.seen.players) {
    const bot = p.id - 1;
    if (bot === playYouIndex || bot < 0 || bot >= bots) continue;
    lastSeenPos.set(p.id, p.pos);
    setUnit(bot * 2, p.id, bot, 0, { pos: p.pos, vel: p.vel, facing: p.facing, hp: p.hp, alive: true, maxhp: 100 });
  }

  // Rising-edge status FX for the local tarsius: dash streak, shield pop.
  const dashOn = !!me.status?.includes("dashing");
  const shieldOn = !!me.status?.includes("shielding");
  if (dashOn && !prevDashOn) {
    playFx!.dashStreak(me.pos[0], me.pos[1], me.facing ?? 0, playYouIndex);
    zoomPunch = Math.max(zoomPunch, 0.12);
  }
  if (shieldOn && !prevShieldOn) playFx!.shieldPop(me.pos[0], me.pos[1]);
  prevDashOn = dashOn;
  prevShieldOn = shieldOn;
  playUnits.update(units, units, 0, bots * 2, true);

  const zoneArr = Float32Array.of(
    obs.global.zone.center[0], obs.global.zone.center[1], obs.global.zone.radius,
    obs.global.zone.next?.center[0] ?? 0, obs.global.zone.next?.center[1] ?? 0,
    obs.global.zone.next?.radius ?? 0, obs.global.alive,
  );
  playZone!.update(zoneArr, false, !!obs.global.zone.next);

  // The human always sees through the fog.
  playFog!.show();
  playFog!.update({
    me: {
      main: { pos: me.pos, alive: me.alive },
      comp: { pos: obs.you.companion.pos, alive: obs.you.companion.alive },
    },
    seenPlayers: obs.seen.players,
    seenCompanions: obs.seen.companions,
    seenProjectiles: obs.seen.projectiles,
    seenPickups: obs.seen.pickups,
    heard: obs.heard,
    zone: { center: obs.global.zone.center, radius: obs.global.zone.radius, next: obs.global.zone.next },
  }, playYouIndex);

  // Camera rides the player; zoomPunch kicks in on kills/hits/dashes.
  stage.setTarget(me.pos[0], me.pos[1], 1.05 + zoomPunch);
  zoomPunch *= Math.exp(-dt * 5);

  // ---------------- combat feel: diff this observation against the last one
  if (me.alive && prevMainAlive) {
    const hpDrop = prevHp - me.hp;
    if (hpDrop > 0.5) {
      sfx.play("hurt", 0, 0.9);
      flashVignette(0.45 + Math.min(0.4, hpDrop / 30));
      stage.shake(2.5);
      zoomPunch = Math.max(zoomPunch, 0.18);
      // Point the damage arrow at the likeliest shooter: the closest enemy
      // projectile in flight, else the strongest recent gunshot bearing.
      const threat = obs.seen.projectiles
        .filter((p) => p.owner !== me.id)
        .map((p) => ({ p, d: Math.hypot(p.pos[0] - me.pos[0], p.pos[1] - me.pos[1]) }))
        .sort((a, b) => a.d - b.d)[0];
      if (threat) {
        showDamageArrow(Math.atan2(threat.p.pos[1] - me.pos[1], threat.p.pos[0] - me.pos[0]));
      } else {
        const gun = obs.heard.find((h) => h.kind === "gunshot");
        if (gun) showDamageArrow((gun.bearing * Math.PI) / 180 - Math.PI / 2);
      }
    }
    const hpGain = me.hp - prevHp;
    if (hpGain > 4 || me.energy - prevEnergy > 10) sfx.play("pickup", 0, 0.8);

    // Hit confirms: a seen enemy's HP dropped → your shot (or an ally's) landed.
    for (const p of obs.seen.players) {
      if (p.hp === undefined) continue;
      const before = prevEnemyHp.get(p.id);
      if (before !== undefined && before - p.hp > 0.5) {
        const dx = p.pos[0] - me.pos[0];
        const dy = p.pos[1] - me.pos[1];
        const d = Math.hypot(dx, dy);
        const pan = Math.max(-1, Math.min(1, (dx / Math.max(60, d)) * 0.85));
        sfx.play("hit", pan, 0.75);
        playFx!.hitmark(p.pos[0], p.pos[1]);
      }
      prevEnemyHp.set(p.id, p.hp);
    }

    // Your own muzzle: new projectiles you own since last tick.
    const myShots = new Set(obs.seen.projectiles.filter((p) => p.owner === me.id).map((p) => p.id));
    for (const id of myShots) if (!prevMyShots.has(id)) sfx.play("shot", 0, 0.5);
    prevMyShots = myShots;
  }
  prevHp = me.hp;
  prevEnergy = me.energy;

  // Companion status pips.
  if (obs.you.companion.alive && obs.you.companion.pos) prevCompPos = obs.you.companion.pos;
  if (!prevCompAlive && obs.you.companion.alive && obs.you.companion.pos) {
    playFx!.sparkle(obs.you.companion.pos[0], obs.you.companion.pos[1]);
  }
  if (prevCompAlive && !obs.you.companion.alive) {
    sfx.play("boom", 0, 0.4);
    if (prevCompPos) playFx!.deathBlast(prevCompPos[0], prevCompPos[1], playYouIndex);
    showBanner("companion down", "#ff9d3b", 1300);
  }
  prevCompAlive = obs.you.companion.alive;

  // Kill feed (obs.global.kill_feed only ever appends).
  const feed = obs.global.kill_feed;
  while (killProcessed < feed.length) {
    const k = feed[killProcessed++];
    hud.kill(k.killer, k.victim, names);
    // Death blast at the victim's last seen position (own death is handled
    // by the elimination block below — skip it here to avoid doubling).
    if (k.victim !== playYouIndex) {
      const pos = lastSeenPos.get(k.victim + 1);
      if (pos) playFx!.deathBlast(pos[0], pos[1], k.victim);
    }
    if (k.killer === playYouIndex) {
      sfx.play("kill", 0, 1);
      zoomPunch = Math.max(zoomPunch, 0.24);
      showBanner(`eliminated ${names[k.victim]}!`, "#43d66e");
    }
  }

  // Your elimination.
  if (prevMainAlive && !me.alive) {
    sfx.play("boom", 0, 1);
    playFx!.deathBlast(me.pos[0], me.pos[1], playYouIndex);
    flashVignette(0.9);
    zoomPunch = Math.max(zoomPunch, 0.3);
    showBanner("you were eliminated", "#e6455f", 2400);
    playOverShow("💀", "ELIMINATED", `place revealed at match end — ${obs.global.alive} still fighting`);
  }
  prevMainAlive = me.alive;

  // Zone discipline: beep + banner while taking zone damage.
  const zd = Math.hypot(me.pos[0] - obs.global.zone.center[0], me.pos[1] - obs.global.zone.center[1]);
  const outside = me.alive && zd > obs.global.zone.radius;
  if (outside && ts - lastZoneBeep > 900) {
    lastZoneBeep = ts;
    sfx.play("zone", 0, 0.9);
  }
  playBanner.classList.toggle("zone-warn", outside);

  // Heard events → positional audio (gunshot / dash / sonar).
  for (const h of obs.heard) {
    const a = (h.bearing * Math.PI) / 180;
    const pan = Math.sin(a);
    const vol = h.band === "near" ? 0.85 : h.band === "mid" ? 0.5 : 0.26;
    if (h.kind === "gunshot") sfx.play("shot", pan, vol);
    else if (h.kind === "dash") sfx.play("dash", pan, vol * 0.8);
    else if (h.kind === "sonar") sfx.play("sonar", pan, vol * 0.9);
  }

  // HUD.
  hud.stats(obs.tick, obs.global.alive, zonePhaseOfFloat(obs.global.zone.radius), false);
  const fireCd = me.cooldown.fire ?? 0;
  const sonarCd = obs.you.companion.cooldown.sonar ?? 0;
  const hpColor = me.hp > 55 ? "#43d66e" : me.hp > 25 ? "#ffc93c" : "#ff5f7e";
  const zoneLeft = obs.global.zone.next
    ? ` · zone locks ${Math.max(0, Math.round((obs.global.zone.next.locks_at_tick - obs.tick) / 10))}s`
    : "";
  const comp = obs.you.companion.alive
    ? `<div class="pbar"><span>JALAK ${Math.round(obs.you.companion.hp)}</span><div><i style="width:${Math.max(0, obs.you.companion.hp / 30 * 100)}%;background:#35c1f0"></i></div></div>`
    : `<div class="pcd">jalak respawning ${obs.you.companion.respawn_in_s ? obs.you.companion.respawn_in_s.toFixed(0) + "s" : "…"}</div>`;
  document.getElementById("play-bars")!.innerHTML = `
    <div class="pbar"><span>HP ${Math.round(me.hp)}</span><div><i style="width:${Math.max(0, me.hp)}%;background:${hpColor}"></i></div></div>
    <div class="pbar"><span>EN ${Math.round(me.energy)}</span><div><i style="width:${me.energy}%;background:#35c1f0"></i></div></div>
    ${comp}
    <div class="pbar mini"><span>FIRE</span><div><i style="width:${(1 - Math.min(1, fireCd / 0.5)) * 100}%;background:${fireCd > 0 ? "#8d82b5" : "#ffd93b"}"></i></div></div>
    <div class="pbar mini"><span>SONAR</span><div><i style="width:${(1 - Math.min(1, sonarCd / 15)) * 100}%;background:${sonarCd > 0 ? "#8d82b5" : "#35c1f0"}"></i></div></div>
    <div class="pcd">sprint ${playClient.sprinting ? "ON (no firing)" : "off"} · ${outside ? "<b style='color:#e6455f'>OUTSIDE ZONE — RUN!</b>" : "zone ok"}${zoneLeft}</div>`;

  playFx!.update(dt);
}

function zonePhaseOfFloat(r: number): number {
  const ladder = [1600, 1200, 850, 550, 300, 0];
  for (let i = 0; i < ladder.length; i++) {
    if (r > ladder[i] - 1) return i;
  }
  return ladder.length - 1;
}

// Keyboard shortcuts (replay mode only — play mode uses its own handlers).
window.addEventListener("keydown", (e) => {
  if (playClient?.playing) {
    if (e.code === "KeyM") mindcam?.toggle();
    return;
  }
  if (e.code === "Space") { playing = !playing; e.preventDefault(); }
  if (e.code === "ArrowRight" && replay) { idx = Math.min(replay.data.totalTicks - 1, idx + TICK_RATE); }
  if (e.code === "ArrowLeft" && replay) { idx = Math.max(0, idx - TICK_RATE); }
  if (e.code === "KeyM") mindcam?.toggle();
  if (location.hash === "#replays" && !replay && !playClient) {
    location.hash = ""; // library open: Esc goes home instead
    return;
  }
  if (e.code === "Escape") { hud.hideWinner(); if (replay) setCamMode("auto"); }
});

// Sound toggle — persists, works in both replay and play modes.
{
  const btn = document.getElementById("btn-audio")!;
  const paint = () => { btn.textContent = sfx.muted ? "🔇" : "🔊"; };
  paint();
  btn.addEventListener("click", () => { sfx.toggleMute(); paint(); });
}

boot();
