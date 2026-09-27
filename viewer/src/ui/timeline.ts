/** Timeline scrubber with kill markers + playback controls. */

import { KillMarker, botColor } from "../types.js";

export class Timeline {
  root = document.getElementById("bottombar")!;
  private btnPlay = document.getElementById("btn-play")!;
  private tickLabel = document.getElementById("tick-label")!;
  private track = document.getElementById("timeline-progress")!;
  private markers = document.getElementById("timeline-markers")!;
  private input = document.getElementById("timeline") as HTMLInputElement;
  private speedGroup = document.getElementById("speed-group")!;
  camSelect = document.getElementById("cam-select") as HTMLSelectElement;

  onPlayToggle?: () => void;
  onSeek?: (frac: number) => void;
  onSpeed?: (s: number) => void;
  onCam?: (mode: string) => void;

  constructor() {
    this.btnPlay.addEventListener("click", () => this.onPlayToggle?.());
    this.input.addEventListener("input", () => this.onSeek?.(Number(this.input.value) / 1000));
    this.input.addEventListener("mousedown", () => { this.scrubbing = true; });
    window.addEventListener("mouseup", () => { this.scrubbing = false; });
    this.speedGroup.querySelectorAll(".speed").forEach((b) => {
      b.addEventListener("click", () => {
        this.speedGroup.querySelectorAll(".speed").forEach((x) => x.classList.remove("active"));
        b.classList.add("active");
        this.onSpeed?.(Number((b as HTMLElement).dataset.speed));
      });
    });
    this.camSelect.addEventListener("change", () => this.onCam?.(this.camSelect.value));
  }

  scrubbing = false;

  show(): void {
    this.root.classList.remove("hidden");
  }

  setPlaying(playing: boolean): void {
    this.btnPlay.textContent = playing ? "❚❚" : "▶";
  }

  setRange(_totalTicks: number): void {
    this.input.max = String(1000);
  }

  update(tickFrac: number, tick: number, playing: boolean): void {
    this.track.style.width = `${tickFrac * 100}%`;
    this.tickLabel.textContent = `t${tick} · ${(tick / 10).toFixed(1)}s`;
    this.btnPlay.textContent = playing ? "❚❚" : "▶";
  }

  addMarkers(markers: KillMarker[], names: string[]): void {
    this.markers.innerHTML = "";
    for (const m of markers) {
      const el = document.createElement("div");
      el.className = "tl-marker";
      el.style.left = `${m.at * 100}%`;
      el.style.color = m.killer === null ? "#e6455f" : botColor(m.killer);
      el.style.background = m.killer === null ? "#e6455f" : botColor(m.killer);
      el.title = m.killer === null
        ? `t${m.tick}: the zone eliminated ${names[m.victim]}`
        : `t${m.tick}: ${names[m.killer]} eliminated ${names[m.victim]}`;
      this.markers.appendChild(el);
    }
  }

  setCamOptions(names: string[], onSelect: (mode: string) => void): void {
    this.camSelect.innerHTML = "";
    const opts: [string, string][] = [
      ["auto", "🎥 Auto-director"],
      ["global", "🌍 Global view"],
    ];
    names.forEach((n, b) => {
      opts.push([`follow:${b}`, `◉ Follow ${n}`]);
      opts.push([`cam:${b}`, `👁 Bot-cam ${n} (fog)`]);
    });
    for (const [v, label] of opts) {
      const o = document.createElement("option");
      o.value = v;
      o.textContent = label;
      this.camSelect.appendChild(o);
    }
    void onSelect;
  }
}
