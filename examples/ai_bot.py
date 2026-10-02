#!/usr/bin/env python3
"""AI brain bridge for GUNBATTE (see docs/AI-BOTS.md).

Holds the WebSocket to the gateway and answers every 10 Hz observation
instantly with the currently held action (the game's momentum makes this
seamless), while an OpenAI-compatible LLM is asked for a fresh decision at a
human pace. The LLM answers in human units (throttle 0..1, coordinates
0..3200); this bridge converts to the wire's Q16.16 fixed-point integers —
the one mistake that silently drops every hand-rolled client's messages.

Requires: pip install websockets
"""

import argparse
import asyncio
import json
import math
import os
import sys
import time
import urllib.request

try:
    import websockets
except ImportError:
    sys.exit("pip install websockets")

FIX_ONE = 65536  # Q16.16: 1.0 on the wire
ARENA = 3200.0

SYSTEM_PROMPT = """You are the brain of a GUNBATTE battle-royale bot. You control two units on a
3200x3200 map, top-left origin, headings in degrees (0 = north, clockwise).

WORLD RULES
- 10 ticks/second. You are asked for a decision only every few seconds; your
  last order keeps executing in between. Answer FAST and CONCRETE.
- Strict fog: you know only what the digest shows. Sounds (gunshot/dash/
  footstep with bearing and distance band) hint at unseen actors.
- The zone shrinks. Outside it you take rising damage. Always steer toward
  the CURRENT zone circle, and drift toward the NEXT one as it locks.
- Loot on the floor: hp_kit heals, energy refuels abilities, mod_* tunes your
  gun, weapon_<gun> swaps your gun. Walking over a pickup picks it up.
- Guns: pea (starter), sprinkler (SMG), scatter (shotgun, brutal close),
  lance (sniper), bouncer (wall ricochet), skewer (pierces), popper (splash).
  Sprinting blocks firing. Firing has a cooldown (yours is given).
- Your companion is a second life. If you never command it, it follows and
  shoots on its own. You may take direct control at any time.
- Everyone else is an enemy. Last unit standing wins. Dying is permanent for
  the match, so: low HP -> break line of sight, heal, avoid fair fights.

HOW YOU RECEIVE THE WORLD
Each decision you get a compact text digest: your units (position, hp,
energy, gun, cooldown, status), the zone (current + next), enemies and
projectiles you can see (with distance and compass bearing), pickups,
sounds, and the kill feed. Distances and coordinates are in map units
(0..3200). Bearings are degrees from north, clockwise.

HOW YOU ANSWER
Reply with ONE JSON object and NOTHING else - no prose, no markdown fence:

{"main": {"move": {"dir": <0..359 int>, "throttle": <0.0..1.0>},
          "action": <ACTION or null>},
 "companion": {"move": {"dir": <0..359 int>, "throttle": <0.0..1.0>},
               "action": <ACTION or null>},
 "intent": "<optional <=64-char shout>"}

ACTION is one of:
  {"type":"fire","target":{"x":<0..3200>,"y":<0..3200>}}
  {"type":"dash"} | {"type":"shield"} | {"type":"sprint","on":<true|false>}
  {"type":"heel"}                                      // companion only
Use null for "no action this decision". To lead a moving target, aim ahead
of it along its velocity. Example answer:

{"main": {"move": {"dir": 30, "throttle": 1.0},
          "action": {"type": "fire", "target": {"x": 1840, "y": 760}}},
 "companion": {"move": {"dir": 30, "throttle": 0.8}, "action": null},
 "intent": ""}

DEFAULT DOCTRINE (override when the situation demands)
1. Outside the zone or near its edge -> run to the zone center, full throttle.
2. Enemy visible and my cooldown ready -> fire at it (lead the target), and
   strafe (keep moving perpendicular). Enemy HP low or it faces away -> press.
3. Outgunned, low HP, or outnumbered -> retreat away from the threat toward
   the zone, use dash to break line, heal on hp_kit.
4. Nothing visible -> move toward the nearest pickup or the next zone, listen
   to gunfire bearings and drift toward fresh fights you can win.
5. Keep the companion near you; use shield when closing distance, sprint
   only to travel (you cannot fire while sprinting).
"""


def bearing_deg(frm, to):
    dx, dy = to[0] - frm[0], to[1] - frm[1]
    return round(math.degrees(math.atan2(dx, dy))) % 360


def dist(a, b):
    return math.hypot(a[0] - b[0], a[1] - b[1])


def to_fix(v):
    return max(0, min(FIX_ONE, round(float(v) * FIX_ONE)))


def to_wire(decision, tick):
    """Human-unit decision -> wire JSON (Q16.16 integers, exact action tags)."""
    out = {"tick": tick}
    for who in ("main", "companion"):
        d = decision.get(who) if isinstance(decision, dict) else None
        d = d if isinstance(d, dict) else {}
        mv = d.get("move") if isinstance(d.get("move"), dict) else {}
        try:
            ddir = int(mv.get("dir", 0)) % 360
        except (TypeError, ValueError):
            ddir = 0
        unit = {"move": {"dir": ddir, "throttle": to_fix(mv.get("throttle", 0))}}
        act = d.get("action")
        if isinstance(act, dict) and isinstance(act.get("type"), str):
            a = {"type": act["type"]}
            if act["type"] == "fire" and isinstance(act.get("target"), dict):
                a["target"] = {
                    "x": round(float(act["target"].get("x", 0)) * FIX_ONE),
                    "y": round(float(act["target"].get("y", 0)) * FIX_ONE),
                }
            elif act["type"] == "sprint":
                a["on"] = bool(act.get("on", True))
            unit["action"] = a
        out[who] = unit
    intent = decision.get("intent")
    if isinstance(intent, str) and intent.strip():
        out["intent"] = intent[:64]
    return out


def digest(obs):
    """Compact world text for the LLM. Coordinates are human units."""
    g, you = obs.get("global", {}), obs.get("you", {})
    main, comp = you.get("main", {}), you.get("companion", {})
    z = g.get("zone", {})
    zc = z.get("center") or [1600, 1600]
    zn = z.get("next") or {}
    lines = [
        f"tick {obs.get('tick')} | alive {g.get('alive')}/{g.get('bots')} | "
        f"time_left {g.get('match_time_left_s')}s",
        f"zone center=({zc[0]:.0f},{zc[1]:.0f}) radius={z.get('radius')}",
        f"next zone center={zn.get('center')} radius={zn.get('radius')} "
        f"locks@tick {zn.get('locks_at_tick')}",
    ]
    for name, u in (("main", main), ("companion", comp)):
        if not u:
            continue
        extra = f" respawns_in {u.get('respawn_in_s')}s" if u.get("respawn_in_s") else ""
        lines.append(
            f"{name} id={u.get('id')} alive={u.get('alive')} pos=({u.get('pos', [0, 0])[0]:.0f},"
            f"{u.get('pos', [0, 0])[1]:.0f}) facing={u.get('facing')} hp={u.get('hp')} "
            f"energy={u.get('energy')} gun={u.get('weapon')} fire_cd={u.get('cooldown', {}).get('fire')} "
            f"status={u.get('status')}{extra}"
        )
    me = main.get("pos", [0, 0])
    for p in obs.get("seen", {}).get("players", []):
        lines.append(
            f"enemy id={p.get('id')} kind={p.get('kind', 'main')} pos=({p['pos'][0]:.0f},{p['pos'][1]:.0f}) "
            f"dist={p.get('range')} bearing={bearing_deg(me, p['pos'])} vel={p.get('vel')} "
            f"hp={p.get('hp')} gun={p.get('weapon')} status={p.get('status')}"
        )
    for pr in obs.get("seen", {}).get("projectiles", []):
        lines.append(f"incoming projectile from owner={pr.get('owner')} pos=({pr['pos'][0]:.0f},{pr['pos'][1]:.0f}) vel={pr.get('vel')}")
    for pk in obs.get("seen", {}).get("pickups", []):
        lines.append(f"pickup {pk.get('kind')} at ({pk['pos'][0]:.0f},{pk['pos'][1]:.0f}) dist={dist(me, pk['pos']):.0f}")
    for h in obs.get("heard", [])[-8:]:
        lines.append(f"heard {h.get('kind')} bearing={h.get('bearing')} band={h.get('band')}")
    for k in g.get("kill_feed", [])[-5:]:
        lines.append(f"kill: {k.get('killer')} eliminated {k.get('victim')}")
    return "\n".join(lines)


def ask_llm(cfg, digest_text):
    body = json.dumps({
        "model": cfg.model,
        "temperature": 0.2,
        "max_tokens": 300,
        "messages": [
            {"role": "system", "content": cfg.system_prompt},
            {"role": "user", "content": digest_text},
        ],
    }).encode()
    req = urllib.request.Request(
        f"{cfg.base_url.rstrip('/')}/chat/completions",
        data=body,
        headers={"Content-Type": "application/json", "Authorization": f"Bearer {cfg.api_key}"},
    )
    with urllib.request.urlopen(req, timeout=cfg.llm_timeout_s) as r:
        payload = json.loads(r.read())
    return payload["choices"][0]["message"]["content"]


def parse_action_reply(text):
    """Extract the JSON object; tolerate a stray fence or prose around it."""
    i, j = text.find("{"), text.rfind("}")
    if i < 0 or j <= i:
        raise ValueError(f"no JSON in LLM reply: {text[:120]!r}")
    decision = json.loads(text[i : j + 1])
    if not isinstance(decision, dict) or "main" not in decision:
        raise ValueError(f"not a decision object: {text[:120]!r}")
    return decision


async def main():
    p = argparse.ArgumentParser(description="LLM brain bridge for GUNBATTE")
    p.add_argument("url", help="gateway, e.g. wss://host/ws/bot")
    p.add_argument("--name", required=True)
    p.add_argument("--rated", action="store_true", help="enroll on the ladder")
    p.add_argument("--token", default="", help="explicit issued secret")
    p.add_argument("--token-file", default=None)
    p.add_argument("--decision-every-s", type=float, default=2.0)
    p.add_argument("--base-url", default=os.environ.get("OPENAI_BASE_URL", "https://api.openai.com/v1"))
    p.add_argument("--model", default="gpt-4o-mini")
    p.add_argument("--api-key", default=os.environ.get("OPENAI_API_KEY", ""))
    p.add_argument("--prompt-file", default=None)
    p.add_argument("--llm-timeout-s", type=float, default=30.0)
    cfg = p.parse_args()

    cfg.token_file = cfg.token_file or f"{cfg.name}.token"
    cfg.system_prompt = (
        open(cfg.prompt_file).read() if cfg.prompt_file else SYSTEM_PROMPT
    )
    token = cfg.token
    if not token and cfg.rated and os.path.exists(cfg.token_file):
        token = open(cfg.token_file).read().strip()
    if cfg.rated and not (token or cfg.api_key):
        print("note: --rated without OPENAI_API_KEY still works if --base-url needs no auth")

    async with websockets.connect(cfg.url) as ws:
        reg = {"type": "register", "name": cfg.name, "token": token,
               "rated": cfg.rated, "decision_rate": 1}
        await ws.send(json.dumps(reg))

        held = None          # the LLM's last accepted decision (human units)
        last_ask = 0.0
        llm_busy = False

        async def maybe_ask(obs):
            """Fire one LLM decision if the pace allows; store the answer."""
            nonlocal held, last_ask, llm_busy
            now = time.monotonic()
            if llm_busy or now - last_ask < cfg.decision_every_s:
                return
            last_ask, llm_busy = now, True
            text_digest = digest(obs)

            def run():
                return ask_llm(cfg, text_digest)

            async def work():
                nonlocal held, llm_busy
                try:
                    reply = await asyncio.to_thread(run)
                    decision = parse_action_reply(reply)
                    held = decision
                    print(f"[brain] tick {obs.get('tick')}: {json.dumps(held)[:160]}")
                except Exception as e:  # keep playing on the held action
                    print(f"[brain] rejected: {e}", file=sys.stderr)
                finally:
                    llm_busy = False

            asyncio.create_task(work())

        async for raw in ws:
            try:
                msg = json.loads(raw)
            except json.JSONDecodeError:
                continue
            mtype = msg.get("type")
            if mtype == "registered":
                if msg.get("token"):
                    with open(cfg.token_file, "w") as f:
                        f.write(msg["token"])
                    print(f"identity secret saved to {cfg.token_file}")
                tier = "ladder" if msg.get("rated") else "casual"
                print(f"registered as {msg.get('you')} ({tier})")
            elif mtype == "match_start":
                print(f"match start on {msg.get('map_id')} as {msg.get('role')} "
                      f"of {len(msg.get('bots', []))} — brain: {cfg.model}")
                held = None
            elif mtype == "match_over":
                print(f"match over — place {msg.get('place')} — elo {msg.get('new_elo')} — "
                      f"replay {msg.get('replay')}")
            elif mtype == "error":
                print(f"server error: {msg.get('error')}", file=sys.stderr)
                if msg.get("error") == "bad token":
                    print("hint: enroll once with --rated (the secret is saved to "
                          f"{cfg.token_file}) or restore that file", file=sys.stderr)
                return
            else:
                # An observation: answer instantly with the held action so the
                # deadline is never missed, then let the brain steer.
                await maybe_ask(msg)
                if held is None:
                    zc = msg.get("global", {}).get("zone", {}).get("center", [1600, 1600])
                    pos = msg.get("you", {}).get("main", {}).get("pos", [1600, 1600])
                    held = {"main": {"move": {
                        "dir": bearing_deg(pos, zc), "throttle": 1.0}}}
                await ws.send(json.dumps(to_wire(held, msg.get("tick", 0))))


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        pass
