/** WebAudio SFX engine — fully synthesized (zero assets), so all the game
 * feel lives client-side next to the particles (PLAN §5: juice can never
 * desync a match). The context is created on the first user gesture to
 * satisfy browser autoplay policy; every call before that is a no-op.
 *
 * Sounds take a stereo `pan` in [-1, 1] and a `vol` multiplier so distant
 * events are quiet and off-center events sit on their side of the screen. */

type SndName =
  | "shot" | "hit" | "hurt" | "dash" | "pickup" | "kill"
  | "boom" | "zone" | "victory" | "defeat" | "click";

const MASTER = 0.5;
/** Min seconds between two plays of the same sound (anti-spam at 10Hz). */
const THROTTLE: Partial<Record<SndName, number>> = {
  shot: 0.045, hit: 0.08, boom: 0.1, dash: 0.12, zone: 0.8,
};

/** Playback cap per sound (seconds). Dropped samples can be long — an 8s
 * "hit" would stack into mud at a 10Hz tick — so each role caps its length
 * and the tail fades out over ≤60ms (masked by the sample's own decay). */
const MAX_DUR: Partial<Record<SndName, number>> = {
  shot: 0.35, hit: 0.25, hurt: 0.4, dash: 0.3, pickup: 0.4,
  kill: 1.2, boom: 0.6, zone: 0.8, victory: 1.7, defeat: 0.6, click: 0.12,
};

/** Real-sample override: drop `viewer/public/sfx/<name>.mp3` (Mixkit /
 * Pixabay both ship free-license game SFX) and it replaces the synth for
 * that sound. Missing files fall back to the synthesized version. */
const SAMPLE_URL: Record<SndName, string> = {
  shot: "/sfx/shot.mp3", hit: "/sfx/hit.mp3", hurt: "/sfx/hurt.mp3",
  dash: "/sfx/dash.mp3", pickup: "/sfx/pickup.mp3",
  kill: "/sfx/kill.mp3", boom: "/sfx/boom.mp3", zone: "/sfx/zone.mp3",
  victory: "/sfx/victory.mp3", defeat: "/sfx/defeat.mp3", click: "/sfx/click.mp3",
};

export class Sfx {
  private ctx: AudioContext | null = null;
  private master: GainNode | null = null;
  private noise: AudioBuffer | null = null;
  private last: Partial<Record<SndName, number>> = {};
  /** Decoded samples per sound; a sound without a file stays on the synth. */
  private samples: Partial<Record<SndName, AudioBuffer>> = {};
  private probing = new Set<SndName>();
  /** Menu-theme interval handle (null = not playing). */
  private themeTimer: number | null = null;
  muted = false;

  constructor() {
    try {
      this.muted = localStorage.getItem("abr-mute") === "1";
    } catch { /* private mode */ }
    // Autoplay policy: browsers only allow audio after a user gesture.
    const unlock = () => {
      if (!this.ctx) {
        try {
          this.ctx = new AudioContext();
          this.master = this.ctx.createGain();
          this.master.gain.value = this.muted ? 0 : MASTER;
          this.master.connect(this.ctx.destination);
          this.noise = this.makeNoise();
        } catch { /* no audio available: stay silent */ }
      }
      void this.ctx?.resume();
      // Home menu is usually visible at first gesture — start its theme.
      const pickerEl = document.getElementById("picker");
      if (pickerEl && !pickerEl.classList.contains("hidden")) this.startMenuTheme();
    };
    window.addEventListener("pointerdown", unlock);
    window.addEventListener("keydown", unlock);
  }

  toggleMute(): boolean {
    this.muted = !this.muted;
    if (this.master) this.master.gain.value = this.muted ? 0 : MASTER;
    if (this.muted) this.stopMenuTheme();
    try { localStorage.setItem("abr-mute", this.muted ? "1" : "0"); } catch { /* noop */ }
    return this.muted;
  }

  /** Soft synth pad that loops under the home menu (I–vi–IV–V, very quiet).
   * Starts only after the audio unlock gesture; muted sessions stay silent. */
  startMenuTheme(): void {
    if (!this.ctx || !this.master || this.themeTimer !== null || this.muted) return;
    const chords = [
      [261.6, 329.6, 392.0], // C
      [220.0, 261.6, 329.6], // Am
      [174.6, 220.0, 261.6], // F
      [196.0, 246.9, 293.7], // G
    ];
    let bar = 0;
    const playBar = (): void => {
      if (!this.ctx || !this.master || this.muted) return;
      const t = this.ctx.currentTime + 0.05;
      for (const f of chords[bar % chords.length]) {
        const o = this.ctx.createOscillator();
        const g = this.ctx.createGain();
        o.type = "triangle";
        o.frequency.value = f;
        g.gain.setValueAtTime(0.0001, t);
        g.gain.exponentialRampToValueAtTime(0.028, t + 0.5);
        g.gain.exponentialRampToValueAtTime(0.0001, t + 2.1);
        o.connect(g);
        g.connect(this.master);
        o.start(t);
        o.stop(t + 2.2);
      }
      bar++;
    };
    playBar();
    this.themeTimer = window.setInterval(playBar, 2000);
  }

  stopMenuTheme(): void {
    if (this.themeTimer !== null) {
      clearInterval(this.themeTimer);
      this.themeTimer = null;
    }
  }

  private makeNoise(): AudioBuffer {
    const ctx = this.ctx!;
    const buf = ctx.createBuffer(1, ctx.sampleRate, ctx.sampleRate);
    const d = buf.getChannelData(0);
    for (let i = 0; i < d.length; i++) d[i] = Math.random() * 2 - 1;
    return buf;
  }

  /** Probe for a real sample once per sound; failures cache as "none". */
  private probe(name: SndName): void {
    if (!this.ctx || this.samples[name] !== undefined || this.probing.has(name)) return;
    this.probing.add(name);
    fetch(SAMPLE_URL[name])
      .then((r) => (r.ok ? r.arrayBuffer() : null))
      .then((buf) => {
        this.probing.delete(name);
        if (!buf || !this.ctx) { this.samples[name] = null as unknown as AudioBuffer; return; }
        return this.ctx.decodeAudioData(buf).then(
          (decoded) => { this.samples[name] = decoded; },
          () => { this.samples[name] = null as unknown as AudioBuffer; },
        );
      })
      .catch(() => { this.probing.delete(name); this.samples[name] = null as unknown as AudioBuffer; });
  }

  /** Pan in [-1,1], vol in [0,1]. Safe to call before unlock / while muted. */
  play(name: SndName, pan = 0, vol = 1): void {
    if (this.muted || !this.ctx || !this.master || !this.noise) return;
    const now = this.ctx.currentTime;
    const min = THROTTLE[name];
    if (min !== undefined) {
      if ((this.last[name] ?? -1e9) + min > now) return;
      this.last[name] = now;
    }
    vol = Math.max(0, Math.min(1, vol));
    if (vol <= 0.001) return;
    const out = this.ctx.createStereoPanner();
    out.pan.value = Math.max(-1, Math.min(1, pan));
    out.connect(this.master);
    const g = this.ctx.createGain();
    g.connect(out);
    const sample = this.samples[name];
    if (sample === undefined) this.probe(name);
    if (sample) {
      // Real SFX file, capped to its role's length with a quick tail fade
      // (MAX_DUR) so long source files can't stack at the 10Hz tick.
      const src = this.ctx.createBufferSource();
      src.buffer = sample;
      src.connect(g);
      const dur = Math.min(sample.duration, MAX_DUR[name] ?? sample.duration);
      const fade = Math.min(0.06, dur * 0.3);
      g.gain.setValueAtTime(vol, now);
      g.gain.setValueAtTime(vol, now + dur - fade);
      g.gain.exponentialRampToValueAtTime(0.001, now + dur);
      src.start(now);
      src.stop(now + dur + 0.02);
      return;
    }
    switch (name) {
      case "shot": this.burst(g, now, vol * 0.5, 1900, 0.07, 3); this.tone(g, now, "square", 210, 90, vol * 0.22); break;
      case "hit": this.tone(g, now, "triangle", 720, 480, vol * 0.5, 0.09); this.burst(g, now, vol * 0.3, 2600, 0.05, 2); break;
      case "hurt": this.tone(g, now, "sine", 140, 66, vol * 0.85, 0.28); this.burst(g, now, vol * 0.5, 420, 0.16, 1); break;
      case "dash": this.sweep(g, now, vol * 0.4, 500, 2400, 0.22); break;
      case "pickup": this.tone(g, now, "triangle", 660, 660, vol * 0.35, 0.1); this.tone(g, now + 0.09, "triangle", 990, 990, vol * 0.35, 0.14); break;
      case "kill": [523, 659, 784].forEach((f, i) => this.tone(g, now + i * 0.085, "square", f, f, vol * 0.3, 0.1)); break;
      case "boom": this.tone(g, now, "sine", 180, 46, vol * 0.9, 0.42); this.burst(g, now, vol * 0.7, 260, 0.4, 1); break;
      case "zone": this.tone(g, now, "square", 392, 392, vol * 0.3, 0.09); this.tone(g, now + 0.13, "square", 392, 392, vol * 0.3, 0.09); break;
      case "victory": [523, 659, 784, 1046].forEach((f, i) => this.tone(g, now + i * 0.13, "square", f, f, vol * 0.32, 0.17)); break;
      case "defeat": [392, 330, 262].forEach((f, i) => this.tone(g, now + i * 0.18, "triangle", f, f, vol * 0.4, 0.24)); break;
      case "click": this.tone(g, now, "triangle", 880, 880, vol * 0.25, 0.05); break;
    }
  }

  /** Noise burst through a bandpass at `f`, exponential decay `dur`. */
  private burst(g: GainNode, t: number, vol: number, f: number, dur: number, q: number): void {
    const ctx = this.ctx!;
    const src = ctx.createBufferSource();
    src.buffer = this.noise!;
    const bp = ctx.createBiquadFilter();
    bp.type = "bandpass";
    bp.frequency.value = f;
    bp.Q.value = q;
    src.connect(bp).connect(g);
    g.gain.setValueAtTime(vol, t);
    g.gain.exponentialRampToValueAtTime(0.001, t + dur);
    src.start(t);
    src.stop(t + dur + 0.02);
  }

  /** A single oscillator with a pitch glide and decay envelope. */
  private tone(g: GainNode, t: number, type: OscillatorType, f0: number, f1: number, vol: number, dur = 0.12): void {
    const ctx = this.ctx!;
    const o = ctx.createOscillator();
    o.type = type;
    o.frequency.setValueAtTime(f0, t);
    o.frequency.exponentialRampToValueAtTime(Math.max(30, f1), t + dur);
    o.connect(g);
    g.gain.setValueAtTime(vol, t);
    g.gain.exponentialRampToValueAtTime(0.001, t + dur);
    o.start(t);
    o.stop(t + dur + 0.02);
  }

  /** Filtered-noise whoosh: bandpass sweeps f0 → f1. */
  private sweep(g: GainNode, t: number, vol: number, f0: number, f1: number, dur: number): void {
    const ctx = this.ctx!;
    const src = ctx.createBufferSource();
    src.buffer = this.noise!;
    const bp = ctx.createBiquadFilter();
    bp.type = "bandpass";
    bp.Q.value = 1.6;
    bp.frequency.setValueAtTime(f0, t);
    bp.frequency.exponentialRampToValueAtTime(f1, t + dur);
    src.connect(bp).connect(g);
    g.gain.setValueAtTime(0.001, t);
    g.gain.exponentialRampToValueAtTime(vol, t + dur * 0.3);
    g.gain.exponentialRampToValueAtTime(0.001, t + dur);
    src.start(t);
    src.stop(t + dur + 0.02);
  }
}

/** Singleton — both the replay loop and the play client share one context. */
export const sfx = new Sfx();

/** Stereo pan + volume for a world point heard from `cam` at `zoom`:
 * north-up camera, so pan is the horizontal offset, volume falls with
 * distance. `span` is the world width of half the viewport. */
export function panVol(
  cam: { x: number; y: number; zoom: number },
  at: [number, number],
  viewW: number,
): { pan: number; vol: number } {
  const dx = at[0] - cam.x;
  const dy = at[1] - cam.y;
  const d = Math.hypot(dx, dy);
  const half = viewW / cam.zoom / 2;
  return {
    pan: Math.max(-1, Math.min(1, (dx / Math.max(1, half)) * 0.85)),
    vol: Math.max(0, Math.min(1, 1 - d / (half * 2.6))),
  };
}
