/** Human play client (hybrid play): a human enters the SAME match queue as
 * AI bots over the same WebSocket protocol — the gateway cannot tell the
 * difference. The player sees strictly through their own observation (fog)
 * and drives WASD + mouse; the server resolves their actions every tick. */


export interface PlayYouUnit {
  alive: boolean;
  pos: [number, number];
  facing: number;
  hp: number;
  energy: number;
  cooldown: { fire?: number; sonar?: number };
  status: string[];
  respawn_in_s?: number;
}

export interface PlayObs {
  tick: number;
  you: { main: PlayYouUnit; companion: PlayYouUnit };
  seen: {
    players: { id: number; pos: [number, number]; detail: string; hp?: number; viaSonar?: boolean }[];
    companions: { id: number; owner: number; pos: [number, number]; detail: string }[];
    projectiles: { id: number; pos: [number, number]; vel: [number, number]; owner: number }[];
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
  /** Screen-space mouse position; converted to world by the owner. */
  mouseScreen: { x: number; y: number };
  dashQueued: boolean;
  sonarQueued: boolean;
  sprintToggled: boolean;
}

export class PlayClient {
  private ws: WebSocket | null = null;
  private state: InputState = {
    keys: new Set(),
    mouseDown: false,
    mouseScreen: { x: 0, y: 0 },
    dashQueued: false,
    sonarQueued: false,
    sprintToggled: false,
  };

  playing = false;
  sprinting = false;
  /** Latest observation, for HUD rendering between ticks. */
  lastObs: PlayObs | null = null;

  constructor(private name: string, private cb: PlayCallbacks) {}

  connect(url: string): void {
    this.setStatus("connecting");
    const ws = new WebSocket(url);
    this.ws = ws;
    ws.onopen = () => {
      ws.send(JSON.stringify({ type: "register", name: this.name, decision_rate: 1, auto_heel: false }));
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
      if (e.button === 0) this.state.mouseDown = true;
    });
    window.addEventListener("mouseup", (e) => {
      if (e.button === 0) this.state.mouseDown = false;
    });
    host.addEventListener("mousemove", (e) => {
      this.state.mouseScreen = { x: e.clientX, y: e.clientY };
    });
    host.addEventListener("contextmenu", (e) => e.preventDefault());
    this.screenToWorld = screenToWorld;
  }

  private screenToWorld: (x: number, y: number) => { x: number; y: number } = (x, y) => ({ x, y });

  /** Bearing (0 = north, clockwise) of the WASD chord; null = stand still. */
  private moveDir(): { dir: number; throttle: number } | null {
    const k = this.state.keys;
    let dx = 0;
    let dy = 0;
    if (k.has("w") || k.has("arrowup")) dy += 1;
    if (k.has("s") || k.has("arrowdown")) dy -= 1;
    if (k.has("d") || k.has("arrowright")) dx += 1;
    if (k.has("a") || k.has("arrowleft")) dx -= 1;
    if (dx === 0 && dy === 0) return null;
    const deg = ((Math.atan2(dx, dy) * 180) / Math.PI + 360) % 360;
    return { dir: Math.round(deg) % 360, throttle: 1 };
  }

  private sendInput(obs: PlayObs): void {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) return;
    const mv = this.moveDir();
    let action: Record<string, unknown> | undefined;
    if (this.state.dashQueued && mv) {
      action = { type: "dash" };
    } else if (this.state.keys.has("shift")) {
      action = { type: "shield" };
    } else if (this.state.mouseDown && !this.sprinting) {
      const w = this.screenToWorld(this.state.mouseScreen.x, this.state.mouseScreen.y);
      action = { type: "fire", target: { x: w.x, y: w.y } };
    }
    this.state.dashQueued = false;

    if (this.state.sprintToggled) {
      this.sprinting = !this.sprinting;
      this.state.sprintToggled = false;
    }
    const companionAction = this.state.sonarQueued ? { type: "sonar" } : { type: "heel" };
    this.state.sonarQueued = false;

    const msg = {
      tick: obs.tick,
      main: {
        move: { dir: mv?.dir ?? 0, throttle: mv?.throttle ?? 0 },
        action,
      },
      companion: {
        move: { dir: 0, throttle: 0 },
        action: companionAction,
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
