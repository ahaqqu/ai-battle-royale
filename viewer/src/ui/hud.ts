/** HTML HUD: top stats, kill feed, legend, winner modal. */

import { botColor, fmtTime } from "../types.js";

export class Hud {
  topbar = document.getElementById("topbar")!;
  matchSub = document.getElementById("match-sub")!;
  statAlive = document.getElementById("stat-alive")!;
  statZone = document.getElementById("stat-zone")!;
  statTime = document.getElementById("stat-time")!;
  killfeed = document.getElementById("killfeed")!;
  legend = document.getElementById("legend")!;
  winnerModal = document.getElementById("winner-modal")!;
  winnerName = document.getElementById("winner-name")!;
  winnerSub = document.getElementById("winner-sub")!;
  podium = document.getElementById("podium")!;
  private feedRows: HTMLElement[] = [];

  showAll(): void {
    for (const el of [this.topbar, this.killfeed, this.legend]) el.classList.remove("hidden");
  }

  setHeader(names: string[], mapId: string, seed: number): void {
    this.matchSub.textContent = `map ${mapId} · seed ${seed} · ${names.length} entrants`;
  }

  stats(tick: number, alive: number, zonePhase: number, shrinking: boolean): void {
    this.statAlive.textContent = String(alive);
    this.statTime.textContent = fmtTime(tick / 10);
    this.statZone.textContent = shrinking ? `P${zonePhase} ⚠` : `P${zonePhase}`;
    this.statZone.style.color = shrinking ? "#ff4f6d" : "#ffb1c0";
  }

  kill(killer: number | null, victim: number, names: string[]): void {
    const row = document.createElement("div");
    row.className = "feed-row";
    if (killer === null) {
      row.innerHTML = `<span class="zone-kill">☠ zone</span> <span class="victim" style="color:${botColor(victim)}">${names[victim]}</span>`;
    } else {
      row.innerHTML = `<span style="color:${botColor(killer)}">${names[killer]}</span> ⚡ <span class="victim" style="color:${botColor(victim)}">${names[victim]}</span>`;
    }
    this.killfeed.appendChild(row);
    this.feedRows.push(row);
    while (this.feedRows.length > 6) {
      const old = this.feedRows.shift()!;
      old.classList.add("fading");
      setTimeout(() => old.remove(), 700);
    }
    setTimeout(() => { row.classList.add("fading"); setTimeout(() => row.remove(), 700); }, 7000);
  }

  buildLegend(names: string[], onSelect: (bot: number) => void): void {
    this.legend.innerHTML = "";
    names.forEach((name, bot) => {
      const row = document.createElement("div");
      row.className = "legend-row";
      row.innerHTML = `<span class="legend-dot" style="background:${botColor(bot)};color:${botColor(bot)}"></span><span>${name}</span><span class="legend-elim" data-bot="${bot}"></span>`;
      row.addEventListener("click", () => onSelect(bot));
      this.legend.appendChild(row);
    });
  }

  legendDead(bot: number): void {
    const row = this.legend.children[bot] as HTMLElement | undefined;
    if (row) row.classList.add("dead");
  }

  legendElim(bot: number, text: string): void {
    const row = this.legend.children[bot] as HTMLElement | undefined;
    if (row) row.querySelector(".legend-elim")!.textContent = text;
  }

  legendActive(bot: number | null): void {
    for (let i = 0; i < this.legend.children.length; i++) {
      (this.legend.children[i] as HTMLElement).classList.toggle("active", i === bot);
    }
  }

  winner(winner: number | null, names: string[], placements: number[]): void {
    if (winner === null) {
      this.winnerName.textContent = "NOBODY";
      this.winnerName.style.color = "#8fa8d8";
    } else {
      this.winnerName.textContent = names[winner];
      this.winnerName.style.color = botColor(winner);
    }
    this.podium.innerHTML = "";
    placements.slice(0, 5).forEach((bot, rank) => {
      const li = document.createElement("li");
      li.innerHTML = `<span class="rank">#${rank + 1}</span><span style="color:${botColor(bot)}">${names[bot]}</span>`;
      this.podium.appendChild(li);
    });
    this.winnerModal.classList.remove("hidden");
  }

  hideWinner(): void {
    this.winnerModal.classList.add("hidden");
  }
}
