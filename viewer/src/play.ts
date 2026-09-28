/** Human play client (hybrid play): a human enters the SAME match queue as
 * AI bots over the same WebSocket protocol — the gateway cannot tell the
 * difference. The player sees strictly through their own observation (fog)
 * and drives WASD + mouse; the server resolves their actions every tick. */


export interface PlayYouUnit {
  id: number;
  alive: boolean;
  pos: [number, number];
  vel: [number, number];
  facing: number;
  hp: number;
  energy: number;
  cooldown: { fire?: number; sonar?: number };
  status: string[];
  respawn_in_s?: number;
  /** Mains only: the gun currently equipped ("pea", "scatter", …). */
  weapon?: string;
}

export interface PlayObs {
  tick: number;
  you: { main: PlayYouUnit; companion: PlayYouUnit };
  seen: {
    players: { id: number; pos: [number, number]; vel?: [number, number]; facing?: number; detail: string; hp?: number; weapon?: string; viaSonar?: boolean }[];
    companions: { id: number; owner: number; pos: [number, number]; detail: string }[];
    projectiles: { id: number; pos: [number, number]; vel: [number, number]; owner: number; weapon?: string }[];
    pickups: { id: number; pos: [number, number]; kind: string }[];
  };
  heard: { kind: string; bearing: number; band: string }[];
  global: {
    bots: number;
    alive: number;
    zone: { center: [number, number]; radius: number; next?: { center: [number, number]; radius: number; locks_at_tick: number } | null };
    match_time_left_s: number;
    kill_feed: { tick: number; killer: number | null; victim: number }[];
  };
}

export type PlayStatus = "connecting" | "queued" | "playing" | "over" | "disconnected";

export interface PlayCallbacks {
  onStatus: (s: PlayStatus, detail?: string) => void;
  onStart: (youIndex: number, entrants: string[]) => void;
  onObs: (obs: PlayObs) => void;
  onOver: (place: number, replay: string | null) => void;
}

interface InputState {
  keys: Set<string>;
  mouseDown: boolean;
  /** A click shorter than one observation tick would be missed by the
   * level-sampled mouseDown — latch it so a tap still fires one shot. */
  fireQueued: boolean;
  dashQueued: boolean;
  sonarQueued: boolean;
  sprintToggled: boolean;
}

/** The sim's `Fix` is a raw Q16.16 i64 on the wire (1.0 = 65536); JS numbers
 * must be scaled before sending or the gateway reads 1 as 1/65536 — and a
 * fractional value fails i64 parsing, dropping the WHOLE input message. */
const FIX_ONE = 65536;
const toFix = (v: number): number => Math.round(v * FIX_ONE);

export class PlayClient {
  private ws: WebSocket | null = null;
  private state: InputState = {
    keys: new Set(),
    mouseDown: false,
    fireQueued: false,
    dashQueued: false,
    sonarQueued: false,
    sprintToggled: false,
  };

  playing = false;
  sprinting = false;
  /** Latest observation, for HUD rendering between ticks. */
  lastObs: PlayObs | null = null;
  /** Where the reticle sits on screen (HUD renders it each frame). */
  mouseScreen = { x: 0, y: 0 };
  /** True while the left button is held (HUD reticle state). */
  firing = false;

  constructor(
    private name: string,
    private cb: PlayCallbacks,
    /** "boss" queues a Slain-the-Boss raid instead of a royale. */
    private mode: "royale" | "boss" = "royale",
  ) {}

  connect(url: string): void {
    this.setStatus("connecting");
    const ws = new WebSocket(url);
    this.ws = ws;
    ws.onopen = () => {
      // `human` marks this entrant for house-bot fill: the server tops the
      // match up to 8 with reference brains so solo play never waits.
      ws.send(JSON.stringify({ type: "register", name: this.name, decision_rate: 1, auto_heel: false, human: true, mode: this.mode }));
    };
    ws.onmessage = (ev) => {
      let v: any;
      try {
        v = JSON.parse(ev.data as string);
      } catch {
        return;
      }
      if (v.type === "registered") {
        this.setStatus("queued");
      } else if (v.type === "match_start") {
        this.playing = true;
        this.setStatus("playing");
        this.cb.onStart(v.you_index ?? 0, v.bots ?? []);
      } else if (v.type === "match_over") {
        this.playing = false;
        this.setStatus("over");
        this.cb.onOver(v.place ?? 0, v.replay ?? null);
      } else if (v.type === "error") {
        this.setStatus("disconnected", v.error);
      } else {
        // Observation.
        const obs = v as PlayObs;
        this.lastObs = obs;
        if (this.playing) this.sendInput(obs);
        this.cb.onObs(obs);
      }
    };
    ws.onclose = () => {
      this.playing = false;
      this.setStatus("disconnected");
    };
  }

  /** Attach global input listeners (keyboard + mouse). Mouse → world
   * conversion is delegated to the renderer via screenToWorld. */
  attachInput(host: HTMLElement, screenToWorld: (x: number, y: number) => { x: number; y: number }): void {
    window.addEventListener("keydown", (e) => {
      const k = e.key.toLowerCase();
      if (k === " ") e.preventDefault();
      if (k === " " && !this.state.keys.has(" ")) this.state.dashQueued = true;
      if (k === "e" && !this.state.keys.has("e")) this.state.sonarQueued = true;
      if (k === "q" && !this.state.keys.has("q")) this.state.sprintToggled = true;
      this.state.keys.add(k);
    });
    window.addEventListener("keyup", (e) => this.state.keys.delete(e.key.toLowerCase()));
    host.addEventListener("mousedown", (e) => {
      if (e.button === 0) {
        this.state.mouseDown = true;
        this.state.fireQueued = true;
        this.firing = true;
      }
    });
    window.addEventListener("mouseup", (e) => {
      if (e.button === 0) {
        this.state.mouseDown = false;
        this.firing = false;
      }
    });
    host.addEventListener("mousemove", (e) => {
      this.mouseScreen = { x: e.clientX, y: e.clientY };
    });
    host.addEventListener("contextmenu", (e) => e.preventDefault());
    this.screenToWorld = screenToWorld;
  }

  private screenToWorld: (x: number, y: number) => { x: number; y: number } = (x, y) => ({ x, y });

  /** Bearing (0 = north, clockwise) of the WASD chord; null = stand still.
   * The sim's compass 0 is world +Y, which renders as screen DOWN (the
   * viewer has no Y flip), so screen-up keys must negate dy. */
  private moveDir(): { dir: number; throttle: number } | null {
    const k = this.state.keys;
    let dx = 0;
    let dy = 0;
    if (k.has("w") || k.has("arrowup")) dy += 1;
    if (k.has("s") || k.has("arrowdown")) dy -= 1;
    if (k.has("d") || k.has("arrowright")) dx += 1;
    if (k.has("a") || k.has("arrowleft")) dx -= 1;
    if (dx === 0 && dy === 0) return null;
    const deg = ((Math.atan2(dx, -dy) * 180) / Math.PI + 360) % 360;
    return { dir: Math.round(deg) % 360, throttle: 1 };
  }

  /** Companion input: the jalak trails the reticle (scout where you aim),
   * E pings sonar, F recalls it to your side (PLAN §2.3 leash clamps). */
  private companionInput(obs: PlayObs): { mv: { dir: number; throttle: number }; action?: Record<string, unknown> } {
    const comp = obs.you.companion;
    if (this.state.keys.has("f")) {
      return { mv: { dir: 0, throttle: 0 }, action: { type: "heel" } };
    }
    if (!comp.alive) return { mv: { dir: 0, throttle: 0 } };
    const w = this.screenToWorld(this.mouseScreen.x, this.mouseScreen.y);
    const dx = w.x - comp.pos[0];
    const dy = w.y - comp.pos[1];
    const d = Math.hypot(dx, dy);
    if (d < 26) return { mv: { dir: 0, throttle: 0 } };
    const dir = ((Math.atan2(dx, dy) * 180) / Math.PI + 360) % 360;
    return { mv: { dir: Math.round(dir) % 360, throttle: Math.min(1, d / 90) } };  }

  private sendInput(obs: PlayObs): void {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) return;
    const mv = this.moveDir();
    let action: Record<string, unknown> | undefined;
    if (this.state.dashQueued) {
      // The sim dashes toward facing when standing still, so no move check.
      action = { type: "dash" };
    } else if (this.state.sprintToggled) {
      // Sprint is a server-side toggle (PLAN §2.2); it takes the action slot
      // this tick, and firing stays blocked while it is on.
      this.sprinting = !this.sprinting;
      this.state.sprintToggled = false;
      action = { type: "sprint", on: this.sprinting };
    } else if (this.state.keys.has("shift")) {
      action = { type: "shield" };
    } else if ((this.state.mouseDown || this.state.fireQueued) && !this.sprinting) {
      const w = this.screenToWorld(this.mouseScreen.x, this.mouseScreen.y);
      action = { type: "fire", target: { x: toFix(w.x), y: toFix(w.y) } };
    }
    this.state.fireQueued = false;
    this.state.dashQueued = false;

    const compIn = this.companionInput(obs);
    let compAction = compIn.action;
    if (this.state.sonarQueued) {
      // Ignored by the sim while on cooldown, so firing blind is free.
      compAction = { type: "sonar" };
      this.state.sonarQueued = false;
    }

    const msg = {
      tick: obs.tick,
      main: {
        move: { dir: mv?.dir ?? 0, throttle: mv ? toFix(mv.throttle) : 0 },
        action,
      },
      companion: {
        move: { dir: compIn.mv.dir, throttle: toFix(compIn.mv.throttle) },
        action: compAction,
      },
    };
    this.ws.send(JSON.stringify(msg));
  }

  leave(): void {
    this.ws?.close();
    this.ws = null;
    this.playing = false;
  }

  private setStatus(s: PlayStatus, detail?: string): void {
    this.cb.onStatus(s, detail);
  }
}
