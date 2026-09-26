/** Viewer entrypoint: replay loading, playback state, render loop. */

import { CamMode, Frame, ReplayData } from "./types.js";
import { buildPlayerCam, LoadedReplay, loadReplay } from "./sim.js";
import { Stage } from "./render/stage.js";
import { drawArena } from "./render/arena.js";
import { UnitViews } from "./render/units.js";
import { Fx } from "./render/fx.js";
import { PickupLayer, ProjectileLayer, ZoneLayerView } from "./render/world.js";
import { FogView } from "./render/fog.js";
import { Director } from "./render/director.js";
import { Hud } from "./ui/hud.js";
import { Timeline } from "./ui/timeline.js";

const TICK_RATE = 10;

(window as unknown as { __errors: string[] }).__errors = [];
window.addEventListener("error", (e) => {
  (window as unknown as { __errors: string[] }).__errors.push(String(e.message));
});

const loading = document.getElementById("loading")!;
const loadStatus = document.getElementById("load-status")!;
const loadBar = document.getElementById("load-bar")!;
const picker = document.getElementById("picker")!;

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

let idx = 0;            // fractional frame cursor
let playing = true;
let speed = 1;
let lastTs = performance.now();
let firedEvents = new Set<number>();
let camData: { bot: number; frames: unknown[] } | null = null;
let placementsFinal: number[] = [];
let winnerShown = false;

function setProgress(p: number, label: string): void {
  loadBar.style.width = `${Math.min(100, p * 100)}%`;
  loadStatus.textContent = label;
}

async function boot(): Promise<void> {
  const params = new URLSearchParams(location.search);
  const replayParam = params.get("replay");
  if (replayParam) {
    await startReplayUrl(replayParam);
  } else {
    await showPicker();
  }
}

async function showPicker(): Promise<void> {
  loading.classList.add("hidden");
  picker.classList.remove("hidden");
  const list = document.getElementById("picker-list")!;
  try {
    const res = await fetch("./api/replays");
    const items: { name: string; url: string; size_kb: number }[] = await res.json();
    if (items.length === 0) {
      list.innerHTML = `<div style="color:var(--text-dim);font-size:13px;padding:12px 0">No replays found on the server.<br>Generate one: <code>abr-runner run --preset default16 --seed 42 --out replays/match.json</code></div>`;
    }
    for (const it of items) {
      const row = document.createElement("div");
      row.className = "picker-row";
      row.innerHTML = `<span>${it.name}</span><span class="picker-meta">${it.size_kb.toFixed(0)} KB</span>`;
      row.addEventListener("click", () => startReplayUrl(it.url));
      list.appendChild(row);
    }
  } catch {
    list.innerHTML = `<div style="color:var(--text-dim);font-size:13px;padding:12px 0">No replay server detected.<br>Open a replay file directly:</div>`;
  }
  const fileInput = document.getElementById("file-input") as HTMLInputElement;
  fileInput.addEventListener("change", async () => {
    const f = fileInput.files?.[0];
    if (!f) return;
    const text = await f.text();
    await startReplay(text, f.name);
  });
}

async function startReplayUrl(url: string): Promise<void> {
  const name = decodeURIComponent(url.split("/").pop() ?? "replay");
  const res = await fetch(url);
  const json = await res.text();
  await startReplay(json, name);
}

async function startReplay(json: string, name: string): Promise<void> {
  picker.classList.add("hidden");
  loading.classList.remove("hidden");
  setProgress(0.01, "loading replay…");
  await stage.init(document.getElementById("stage-host")!, (s) => setProgress(0.005, s));
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

  // placements for the podium: derive from kill feed order + winner.
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
    idx += dt * TICK_RATE * speed;
    if (idx >= total - 1) {
      idx = total - 1;
      playing = false;
      if (!winnerShown && data.winner !== null) {
        winnerShown = true;
        hud.winner(data.winner, data.botNames, placementsFinal);
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
      if (e.type === "death") {
        const bot = e.bot as number;
        hud.legendDead(bot);
        hud.legendElim(bot, `t${frameB.tick}`);
        hud.kill((e.killer as number | null) ?? null, bot, data.botNames);
      }
    }
  }

  // World layers.
  unitViews.update(frameA.units, frameA.unitCount, true);
  projs!.update(
    frameA.projs, frameA.projCount,
    frameB.projs, frameB.projCount, t,
    (x, y, color, intense) => fx!.tracer(x, y, color, intense),
  );
  pickups!.update(frameA.pickups, frameA.pickupCount, frameA.tick);
  const shrinking = frameA.zone[5] > 0 && frameA.zone[5] < frameA.zone[2] + 1 && frameA.zone[5] > 0 &&
    Math.abs(frameB.zone[2] - frameA.zone[2]) > 0.0001;
  zoneView!.update(frameA.zone, shrinking, frameA.zone[5] > 0);

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
    // main of bot = slot bot*2 in the interleaved layout; stride 11, x@3, y@4
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
  // Derive the phase label from the current radius vs the standard ladder.
  const r = frame.zone[2];
  const ladder = [1600, 1200, 850, 550, 300, 0];
  for (let i = 0; i < ladder.length; i++) {
    if (r > ladder[i] - 1) return i;
  }
  return ladder.length - 1;
}

// Keyboard shortcuts.
window.addEventListener("keydown", (e) => {
  if (e.code === "Space") { playing = !playing; e.preventDefault(); }
  if (e.code === "ArrowRight" && replay) { idx = Math.min(replay.data.totalTicks - 1, idx + TICK_RATE); }
  if (e.code === "ArrowLeft" && replay) { idx = Math.max(0, idx - TICK_RATE); }
  if (e.code === "Escape") { hud.hideWinner(); setCamMode("auto"); }
});

boot();
