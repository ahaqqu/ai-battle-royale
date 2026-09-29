//! Ladder page: standings + recent matches + replay links. One static HTML
//! string with a tiny fetch-and-render script — the whole "ladder site" for
//! v1 (PLAN §8.1).

use gunbatte_node::db::Db;

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
                r#"<tr><td>{}</td><td class="name" style="color:#43d66e">{}</td><td>{}</td><td><a href="/?replay={}" target="_blank">▶ watch</a></td></tr>"#,
                html_escape(&m.ended_at),
                html_escape(m.winner.as_deref().unwrap_or("—")),
                m.num_bots,
                html_escape(&m.replay_path)
            )
        })
        .collect();

    format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><title>GUNBATTE ROYALE — Ladder</title>
<style>
  :root {{ --bg:#7ec9f5; --panel:#ffffff; --border:rgba(58,44,105,.15); --neon:#ff9d3b; --text:#3a2c5a; --dim:#8d82b5; }}
  * {{ box-sizing:border-box }}
  body {{ background:radial-gradient(ellipse at 30% 0%, #a8dcff 0%, #7ec9f5 60%, #5db2ea 100%); color:var(--text); font:14px/1.5 "Baloo 2","Trebuchet MS",Inter,system-ui,sans-serif; margin:0; padding:40px 20px; }}
  .wrap {{ max-width:900px; margin:0 auto }}
  h1 {{ letter-spacing:.14em; font-weight:800; font-size:24px; color:var(--text) }}
  h1 span {{ color:#ff5fd0 }}
  .sub {{ color:var(--dim); font-size:12px; letter-spacing:.08em; margin-bottom:28px; font-weight:600 }}
  table {{ width:100%; border-collapse:collapse; background:var(--panel); border:2px solid var(--border); border-radius:16px; overflow:hidden; box-shadow:0 4px 0 rgba(58,44,105,.15) }}
  th {{ text-align:left; font-size:10px; letter-spacing:.2em; color:var(--dim); padding:10px 14px; border-bottom:2px solid var(--border) }}
  td {{ padding:8px 14px; border-bottom:1px solid rgba(58,44,105,.08) }}
  td.rank {{ color:var(--dim) }}
  td.name {{ font-weight:800 }}
  a {{ color:var(--neon); text-decoration:none; font-weight:700 }}
  a:hover {{ text-decoration:underline }}
  h2 {{ font-size:13px; letter-spacing:.2em; color:var(--dim); margin:34px 0 10px; text-transform:uppercase }}
  .foot {{ margin-top:30px; color:var(--dim); font-size:12px }}
  code {{ color:#e0457f; background:rgba(255,255,255,.7); padding:1px 6px; border-radius:6px }}
</style></head><body><div class="wrap">
<h1>GUNBATTE<span>★</span>ROYALE</h1>
<div class="sub">player-hosted AI bots · strict fog of war · every match a shareable replay · がんばって！</div>
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
  Run locally: <code>gunbatte-runner run --preset default8 --seed 42</code> · <code>make viewer &amp;&amp; make serve</code>
</div>
</div></body></html>"#,
        standings.join("\n"),
        matches.join("\n"),
    )
}

fn bot_hex(i: usize) -> &'static str {
    // Mirrors the viewer's candy palette (viewer/src/types.ts) — first 8
    // are the standard lineup, maximally distinct hues.
    const COLORS: [&str; 16] = [
        "#ff5f7e", "#35c1f0", "#ffd93b", "#8a5cff", "#43d66e", "#ff9d3b", "#ff5fd0", "#f2f6ff",
        "#a8e04c", "#35d6b5", "#4f7dff", "#c06bff", "#c97a4a", "#e6455f", "#ff8fb8", "#00e0c8",
    ];
    COLORS[i % 16]
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
