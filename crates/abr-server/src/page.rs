//! Ladder page: standings + recent matches + replay links. One static HTML
//! string with a tiny fetch-and-render script — the whole "ladder site" for
//! v1 (PLAN §8.1).

use crate::db::Db;

pub fn ladder_html(db: &Db) -> String {
    let standings: Vec<String> = db
        .standings()
        .iter()
        .enumerate()
        .map(|(i, b)| {
            format!(
                r#"<tr><td class="rank">{}</td><td class="name" style="color:{}">{}</td><td>{}</td><td>{}</td><td>{}</td></tr>"#,
                i + 1,
                bot_hex(i),
                html_escape(&b.name),
                b.elo,
                b.wins,
                b.games
            )
        })
        .collect();

    let matches: Vec<String> = db
        .recent_matches(30)
        .iter()
        .map(|m| {
            format!(
                r#"<tr><td>{}</td><td class="name" style="color:#7cff4f">{}</td><td>{}</td><td><a href="/viewer/?replay={}" target="_blank">▶ watch</a></td></tr>"#,
                html_escape(&m.ended_at),
                html_escape(m.winner.as_deref().unwrap_or("—")),
                m.num_bots,
                html_escape(&m.replay_path)
            )
        })
        .collect();

    format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><title>AI Battle Royale — Ladder</title>
<style>
  :root {{ --bg:#070a14; --panel:rgba(13,18,33,.85); --border:rgba(90,120,255,.18); --neon:#55e6ff; --text:#cfe3ff; --dim:#6d82a6; }}
  * {{ box-sizing:border-box }}
  body {{ background:var(--bg); color:var(--text); font:14px/1.5 Inter,system-ui,sans-serif; margin:0; padding:40px 20px; }}
  .wrap {{ max-width:900px; margin:0 auto }}
  h1 {{ letter-spacing:.28em; font-weight:900; font-size:22px; color:var(--text) }}
  h1 span {{ color:var(--neon); text-shadow:0 0 16px rgba(85,230,255,.8) }}
  .sub {{ color:var(--dim); font-size:12px; letter-spacing:.1em; margin-bottom:28px }}
  table {{ width:100%; border-collapse:collapse; background:var(--panel); border:1px solid var(--border); border-radius:10px; overflow:hidden }}
  th {{ text-align:left; font-size:10px; letter-spacing:.24em; color:var(--dim); padding:10px 14px; border-bottom:1px solid var(--border) }}
  td {{ padding:8px 14px; border-bottom:1px solid rgba(90,120,255,.07) }}
  td.rank {{ color:var(--dim) }}
  td.name {{ font-weight:700 }}
  a {{ color:var(--neon); text-decoration:none }}
  a:hover {{ text-shadow:0 0 10px rgba(85,230,255,.7) }}
  h2 {{ font-size:13px; letter-spacing:.24em; color:var(--dim); margin:34px 0 10px; text-transform:uppercase }}
  .foot {{ margin-top:30px; color:var(--dim); font-size:12px }}
  code {{ color:var(--neon) }}
</style></head><body><div class="wrap">
<h1>AI <span>BATTLE</span> ROYALE</h1>
<div class="sub">player-hosted AI bots · strict fog of war · every match a shareable replay</div>
<h2>Standings</h2>
<table><thead><tr><th>#</th><th>Bot</th><th>Elo</th><th>Wins</th><th>Games</th></tr></thead>
<tbody>
{}
</tbody></table>
<h2>Recent matches</h2>
<table><thead><tr><th>Ended</th><th>Winner</th><th>Entrants</th><th>Replay</th></tr></thead>
<tbody>
{}
</tbody></table>
<div class="foot">
  Connect a bot: <code>wss://host/ws/bot</code> · first message <code>{{"type":"register","name":"mybot"}}</code>, then reply to each tick's observation with an action.<br>
  Run locally: <code>abr-runner run --preset default16 --seed 42</code> · <code>make viewer &amp;&amp; make serve</code>
</div>
</div></body></html>"#,
        standings.join("\n"),
        matches.join("\n"),
    )
}

fn bot_hex(i: usize) -> &'static str {
    const COLORS: [&str; 16] = [
        "#00e5ff", "#ff4fd8", "#7cff4f", "#ffd54f", "#ff6b3d", "#4f7cff", "#b44fff", "#4fffb0",
        "#ff4f4f", "#e8ff4f", "#4fd8ff", "#ff9ff3", "#9dff4f", "#ffb84f", "#4fff6b", "#c04fff",
    ];
    COLORS[i % 16]
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
