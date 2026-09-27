/** Ambient home-screen background: the newest recorded match streams behind
 * the menu (muted, HUD-less, auto-director camera), so a visitor sees real
 * gameplay seconds after the page opens. The browser re-simulates the replay
 * deterministically; instead of pre-simulating the whole match like the
 * replay viewer does, it fast-forwards to mid-game in yielding slices, then
 * plays the rest in real time. Polls the replay list — a newer match swaps
 * in at the next loop point instead of interrupting the one on screen. */

import { Frame, MapData, botColor } from "./types.js";
import { ReplaySim, buildFrame, ensureWasm } from "./sim.js";
import { Stage } from "./render/stage.js";
import { drawArena } from "./render/arena.js";
import { UnitViews } from "./render/units.js";
import { Fx } from "./render/fx.js";
import { PickupLayer, ProjectileLayer, ZoneLayerView } from "./render/world.js";
import { Director } from "./render/director.js";

const TICK_S = 0.1; // sim runs at 10Hz
const POLL_MS = 15000;
/** Final-frame hold before looping/switching, so the podium beat reads. */
const END_HOLD_MS = 2200;
/** Start mid-match: skip the opening standoff, land where the action is. */
const START_AT_FRAC = 0.45;

interface AmbientScene {
  name: string;
  json: string;
  sim: ReplaySim;
  units: UnitViews;
  projs: ProjectileLayer;
  pickups: PickupLayer;
  zone: ZoneLayerView;
  fx: Fx;
}

let active = false;
/** True while a fetch / fast-forward is in flight; the render loop pauses
 * stepping while this is set so it never races the loader's sim. */
let loading = false;
let scene: AmbientScene | null = null;
let stageRef: Stage | null = null;
const director = new Director();

let prev: Frame | null = null;
let curr: Frame | null = null;
let acc = 0;
let finished = false;
let endHoldUntil = 0;
let currentName: string | null = null;
let nextUrl: string | null = null;
let raf = 0;
let lastTs = 0;
let pollTimer: number | undefined;

const nextFrame = (): Promise<void> =>
  new Promise((r) => requestAnimationFrame(() => r()));

export function startAmbient(stage: Stage, ensureStage: () => Promise<void>): void {
  if (active) return;
  active = true;
  stageRef = stage;
  document.body.classList.add("ambient");
  // Fire and forget: the menu is usable while the wasm engine compiles and
  // the first match fast-forwards in the background.
  void (async () => {
    try {
      await ensureStage();
    } catch { /* stage failed before; replay/play paths still work */ }
    if (active) void poll();
  })();
  pollTimer = window.setInterval(() => void poll(), POLL_MS);
}

export function stopAmbient(stage: Stage): void {
  if (!active && !scene && !raf) return;
  active = false;
  document.body.classList.remove("ambient");
  if (pollTimer !== undefined) { clearInterval(pollTimer); pollTimer = undefined; }
  if (raf) { cancelAnimationFrame(raf); raf = 0; }
  scene = null;
  nextUrl = null;
  wipeWorldLayers(stage);
}

async function poll(): Promise<void> {
  if (!active || loading) return;
  let items: { name: string; url: string }[] = [];
  try {
    const res = await fetch("./api/replays");
    items = await res.json();
  } catch { return; } // offline / no server: stay on the plain menu
  if (!active || items.length === 0) return;
  const top = items[0]; // server sorts match-<ms>.json newest-first
  if (!scene) {
    await loadScene(top.url, top.name);
  } else if (top.name !== currentName && top.name > (currentName ?? "")) {
    nextUrl = top.url; // applied when the current match finishes
  }
}

/** Fetch + stream a replay: fast-forward to mid-game, then play realtime. */
async function loadScene(url: string, name: string): Promise<void> {
  if (!active || loading) return;
  loading = true;
  try {
    const res = await fetch(url);
    if (!res.ok) return;
    const json = await res.text();
    if (!active) return;
    await ensureWasm(() => { /* silent: it's a backdrop */ });
    if (!active) return;
    const stage = stageRef!;
    const sim = new ReplaySim(json);
    const map: MapData = JSON.parse(sim.map_json());
    const botNames: string[] = [];
    for (let b = 0; b < sim.bots(); b++) botNames.push(sim.bot_name(b) ?? `bot ${b}`);

    wipeWorldLayers(stage);
    drawArena(stage, map);
    scene = {
      name, json, sim,
      units: new UnitViews(stage, botNames),
      projs: new ProjectileLayer(stage),
      pickups: new PickupLayer(stage),
      zone: new ZoneLayerView(stage),
      fx: new Fx(stage),
    };
    currentName = name;
    nextUrl = null;
    prev = curr = null;
    acc = 0;
    finished = false;
    endHoldUntil = 0;
    // Snap (no slow pan from wherever the previous match ended).
    stage.setTarget(1600, 1600, 0.35, true);
    await fastForward(scene);
    if (!raf) {
      lastTs = performance.now();
      raf = requestAnimationFrame(loop);
    }
  } catch { /* keep whatever is on screen; retry on the next poll */ }
  finally { loading = false; }
}

/** Sim through the opening stretch without rendering it — pure Rust steps,
 * yielded every ~12ms so the menu stays responsive. Rendering resumes fresh
 * from the fast-forward point. */
async function fastForward(scene: AmbientScene): Promise<void> {
  const total = scene.sim.total_ticks();
  const skip = Math.max(0, Math.min(total - 1, Math.floor(total * START_AT_FRAC)));
  let sliceStart = performance.now();
  for (let i = 0; i < skip; i++) {
    if (scene.sim.step() === null) break;
    if (performance.now() - sliceStart > 12) {
      await nextFrame();
      if (!active) return;
      sliceStart = performance.now();
    }
  }
}

/** Loop the same match: fresh sim over the cached JSON, fast-forward again.
 * Old frames keep rendering (frozen) until the new stream takes over. */
async function restart(scene: AmbientScene): Promise<void> {
  if (loading) return;
  loading = true;
  try {
    scene.sim = new ReplaySim(scene.json);
    prev = curr = null;
    acc = 0;
    finished = false;
    endHoldUntil = 0;
    await fastForward(scene);
  } finally { loading = false; }
}

function loop(ts: number): void {
  if (!active || !scene || !stageRef) { raf = 0; return; }
  raf = requestAnimationFrame(loop);
  const stage = stageRef;
  const dt = Math.min(0.1, (ts - lastTs) / 1000);
  lastTs = ts;

  if (endHoldUntil > 0) {
    scene.fx.update(dt);
    if (ts < endHoldUntil) return;
    endHoldUntil = 0;
    if (nextUrl) { void loadScene(nextUrl, nameOf(nextUrl)); return; }
    void restart(scene);
    return;
  }

  if (!finished && !loading) {
    acc += dt;
    while (acc >= TICK_S && !finished && !loading) {
      acc -= TICK_S;
      advance(scene);
    }
    if (finished) endHoldUntil = ts + END_HOLD_MS;
  }

  if (prev && curr) {
    const t = Math.max(0, Math.min(1, acc / TICK_S));
    render(stage, scene, prev, curr, t);
  }
  scene.fx.update(dt);
}

function advance(scene: AmbientScene): void {
  const rawJson = scene.sim.step();
  if (rawJson === null || rawJson === undefined) { finished = true; return; }
  const f = buildFrame(JSON.parse(rawJson));
  prev = curr;
  curr = f;
  // Visual event FX only — the ambient loop never touches audio.
  scene.fx.handleEvents(f.events);
}

function render(stage: Stage, scene: AmbientScene, A: Frame, B: Frame, t: number): void {
  scene.units.update(A.units, B.units, t, A.unitCount, true);
  scene.projs.update(
    A.projs, A.projCount,
    B.projs, B.projCount, t,
    (x, y, color, intense) => scene.fx.tracer(x, y, color, intense),
  );
  scene.pickups.update(A.pickups, A.pickupCount, A.tick);
  const shrinking = Math.abs(B.zone[2] - A.zone[2]) > 0.0001;
  scene.zone.update(A.zone, shrinking, A.zone[5] > 0);

  // Dash afterimages, same as the replay viewer.
  for (let s = 0; s < A.unitCount; s++) {
    const o = s * 11;
    if ((A.units[o + 9] & 4) !== 0) {
      scene.fx.tracer(A.units[o + 3], A.units[o + 4], parseInt(botColor(A.units[o + 1]).slice(1), 16), false);
    }
  }

  const target = director.targetFor(A, B, null);
  stage.setTarget(target.x, target.y, target.zoom);
}

function nameOf(url: string): string {
  return decodeURIComponent(url.split("/").pop() ?? "replay");
}

/** Remove everything the ambient (or any) scene drew from the shared world
 * layers. fogLayer is skipped — it holds the shared mind-cam graphics. */
function wipeWorldLayers(stage: Stage): void {
  for (const layer of [
    stage.bgLayer, stage.wallLayer, stage.zoneLayer, stage.pickupLayer,
    stage.trailLayer, stage.unitLayer, stage.projLayer, stage.fxLayer,
  ]) {
    for (const child of layer.removeChildren()) child.destroy({ children: true });
  }
}
