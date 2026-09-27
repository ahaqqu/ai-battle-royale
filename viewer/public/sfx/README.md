# SFX assets (optional real-sample overrides)

Drop mp3 files here, named exactly:

    shot.mp3    hit.mp3    hurt.mp3   dash.mp3   sonar.mp3  pickup.mp3
    kill.mp3    boom.mp3   zone.mp3   victory.mp3 defeat.mp3 click.mp3

Each file replaces the built-in synthesized sound of the same name; any
missing file just stays synthesized (no build step needed — `npm run build`
copies this folder into `dist/sfx/`).

## Where to get sounds (both licenses are free, no attribution required)

- Mixkit — https://mixkit.co/free-sound-effects/game/ (Mixkit Free License)
- Pixabay — https://pixabay.com/sound-effects/search/video%20game/ (Content License)

Suggested mapping for the battle-royale feel:

| file        | Mixkit search term            | note                          |
|-------------|-------------------------------|-------------------------------|
| shot.mp3    | "laser gun shot"              | main weapon                   |
| hit.mp3     | "small hit in a game"         | projectile impact             |
| hurt.mp3    | "player losing or failing"    | you take damage               |
| dash.mp3    | "fast whoosh transition"      | SPACE dash                    |
| sonar.mp3   | "sonar ping" / "sci-fi beep"  | E sonar (played by the pet)   |
| pickup.mp3  | "bonus earned in video game"  | pickup grab                   |
| kill.mp3    | "casino bling achievement"    | you eliminate someone         |
| boom.mp3    | "explosion"                   | big kill / zone blast         |
| zone.mp3    | "warning alarm"               | zone about to lock            |
| victory.mp3 | "winning a coin video game"   | #1 place                      |
| defeat.mp3  | "game blood pop slide" (soft) | elimination                   |
| click.mp3   | "video game retro click"      | UI buttons                    |

Keep files short (<1s for one-shots, ~2s max) and quiet-mastered; the
engine applies its own volume/pan mix on top.

## Shipped samples (Mixkit, free license)

These ship in the repo, trimmed to role length (mono 128 kbps), sourced
from https://mixkit.co/free-sound-effects/game/ under the Mixkit Free
License (free for commercial and noncommercial use, no attribution
required):

| file        | Mixkit title                       | id   |
|-------------|------------------------------------|------|
| shot.mp3    | Retro video game bubble laser      | 276  |
| hit.mp3     | Small hit in a game                | 226  |
| hurt.mp3    | Boxer getting hit                  | 277  |
| pickup.mp3  | Winning a coin, video game         | 211  |
| kill.mp3    | Casino bling achievement           | 2058 |
| zone.mp3    | Ominous drums                      | 257  |
| victory.mp3 | Medieval show fanfare announcement | 2361 |
| defeat.mp3  | Player losing or failing           | 2073 |
| click.mp3   | Video game retro click             | 266  |

`dash` / `sonar` / `boom` had no strong match on that page and stay
synthesized — drop your own files here to override them.
