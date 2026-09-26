//! SQLite persistence: bot registry, ELO, match history (PLAN §8.1 —
//! "Postgres or SQLite for ladder/ELO and bot registry").

use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

pub struct Db {
    conn: Mutex<Connection>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct BotRow {
    pub id: i64,
    pub name: String,
    pub elo: i64,
    pub wins: i64,
    pub games: i64,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct MatchRow {
    pub id: i64,
    pub ended_at: String,
    pub winner: Option<String>,
    pub num_bots: i64,
    pub replay_path: String,
}

impl Db {
    pub fn open(path: &Path) -> rusqlite::Result<Db> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS bots(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT UNIQUE NOT NULL,
                token TEXT NOT NULL DEFAULT '',
                elo INTEGER NOT NULL DEFAULT 1000,
                wins INTEGER NOT NULL DEFAULT 0,
                games INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
             );
             CREATE TABLE IF NOT EXISTS matches(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                started_at TEXT NOT NULL DEFAULT (datetime('now')),
                ended_at TEXT,
                winner_bot INTEGER,
                num_bots INTEGER NOT NULL,
                seed INTEGER NOT NULL,
                ticks INTEGER NOT NULL DEFAULT 0,
                replay_path TEXT NOT NULL DEFAULT ''
             );
             CREATE TABLE IF NOT EXISTS placements(
                match_id INTEGER NOT NULL REFERENCES matches(id),
                bot_id INTEGER NOT NULL REFERENCES bots(id),
                place INTEGER NOT NULL,
                kills INTEGER NOT NULL DEFAULT 0
             );",
        )?;
        Ok(Db {
            conn: Mutex::new(conn),
        })
    }

    /// Register or re-register a bot; returns the db id.
    pub fn register_bot(&self, name: &str, token: &str) -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO bots(name, token) VALUES(?1, ?2)
             ON CONFLICT(name) DO UPDATE SET token=excluded.token",
            [name, token],
        )?;
        conn.query_row("SELECT id FROM bots WHERE name=?1", [name], |r| r.get(0))
    }

    pub fn verify_token(&self, name: &str, token: &str) -> bool {
        let conn = self.conn.lock().unwrap();
        match conn.query_row("SELECT token FROM bots WHERE name=?1", [name], |r| {
            r.get::<_, String>(0)
        }) {
            Ok(stored) => stored.is_empty() || stored == token,
            Err(_) => true, // unknown bot: first connection registers it
        }
    }

    pub fn elo_of(&self, name: &str) -> i64 {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT elo FROM bots WHERE name=?1", [name], |r| r.get(0))
            .unwrap_or(1000)
    }

    pub fn standings(&self) -> Vec<BotRow> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT id, name, elo, wins, games FROM bots ORDER BY elo DESC, name LIMIT 100",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map([], |r| {
            Ok(BotRow {
                id: r.get(0)?,
                name: r.get(1)?,
                elo: r.get(2)?,
                wins: r.get(3)?,
                games: r.get(4)?,
            })
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }

    pub fn record_match(
        &self,
        seed: u64,
        num_bots: i64,
        ticks: u64,
        replay_path: &str,
        // (bot name, place, kills) in rank order
        results: &[(String, i64, i64)],
        elos: &[(String, i64, i64)], // name, old_elo, new_elo
    ) -> Option<i64> {
        let conn = self.conn.lock().unwrap();
        let winner = results.first().map(|(n, _, _)| n.as_str());
        conn.execute(
            "INSERT INTO matches(ended_at, winner_bot, num_bots, seed, ticks, replay_path)
             VALUES(datetime('now'),
                    (SELECT id FROM bots WHERE name=?1),
                    ?2, ?3, ?4, ?5)",
            rusqlite::params![winner, num_bots, seed as i64, ticks as i64, replay_path],
        )
        .ok()?;
        let mid = conn.last_insert_rowid();
        for (name, place, kills) in results {
            conn.execute(
                "INSERT INTO placements(match_id, bot_id, place, kills)
                 VALUES(?1, (SELECT id FROM bots WHERE name=?2), ?3, ?4)",
                rusqlite::params![mid, name, place, kills],
            )
            .ok()?;
            conn.execute(
                "UPDATE bots SET games = games + 1,
                                  wins = wins + (CASE WHEN ?2 = 1 THEN 1 ELSE 0 END),
                                  elo = ?3
                                WHERE name = ?1",
                rusqlite::params![
                    name,
                    place,
                    elos.iter()
                        .find(|(n, _, _)| n == name)
                        .map(|(_, _, e)| e)
                        .unwrap_or(&1000)
                ],
            )
            .ok()?;
        }
        Some(mid)
    }

    pub fn recent_matches(&self, limit: i64) -> Vec<MatchRow> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT m.id, COALESCE(m.ended_at, m.started_at), b.name, m.num_bots, m.replay_path
             FROM matches m LEFT JOIN bots b ON b.id = m.winner_bot
             WHERE m.ended_at IS NOT NULL
             ORDER BY m.id DESC LIMIT ?1",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map([limit], |r| {
            Ok(MatchRow {
                id: r.get(0)?,
                ended_at: r.get(1)?,
                winner: r.get(2)?,
                num_bots: r.get(3)?,
                replay_path: r.get(4)?,
            })
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }
}

/// Pairwise multi-player ELO (PLAN §8.2): every placement compares against
/// every other entrant; winner takes from everyone it beat.
pub fn elo_update(
    ratings: &[(String, i64)],
    places: &std::collections::HashMap<String, i64>,
    k: f64,
) -> Vec<(String, i64, i64)> {
    let mut out = Vec::new();
    for (name, ra) in ratings {
        let pa = *places.get(name).unwrap_or(&i64::MAX);
        let mut delta = 0.0f64;
        for (other, rb) in ratings {
            if other == name {
                continue;
            }
            let pb = *places.get(other).unwrap_or(&i64::MAX);
            let expected = 1.0 / (1.0 + 10f64.powf((*rb - *ra) as f64 / 400.0));
            let actual = match pa.cmp(&pb) {
                std::cmp::Ordering::Less => 1.0,
                std::cmp::Ordering::Equal => 0.5,
                std::cmp::Ordering::Greater => 0.0,
            };
            delta += k * (actual - expected);
        }
        let new = (*ra as f64 + delta).round() as i64;
        out.push((name.clone(), *ra, new));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elo_winner_gains_loses_drop() {
        let ratings = vec![
            ("a".into(), 1000),
            ("b".into(), 1000),
            ("c".into(), 1000),
            ("d".into(), 1000),
        ];
        let mut places = std::collections::HashMap::new();
        places.insert("a".to_string(), 1);
        places.insert("b".to_string(), 2);
        places.insert("c".to_string(), 3);
        places.insert("d".to_string(), 4);
        let out = elo_update(&ratings, &places, 32.0);
        let (_, _, a) = out.iter().find(|(n, _, _)| n == "a").unwrap();
        let (_, _, d) = out.iter().find(|(n, _, _)| n == "d").unwrap();
        assert!(*a > 1000, "winner gains: {a}");
        assert!(*d < 1000, "last loses: {d}");
        // Zero-sum-ish (symmetric pairs).
        let total: i64 = out.iter().map(|(_, _, e)| *e).sum();
        assert_eq!(total, 4000);
    }

    #[test]
    fn db_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("t.db")).unwrap();
        db.register_bot("hunter1", "").unwrap();
        assert!(db.verify_token("hunter1", ""));
        db.record_match(
            42,
            2,
            500,
            "/replays/x.json",
            &[("hunter1".into(), 1, 3), ("hunter2".into(), 2, 0)],
            &[
                ("hunter1".into(), 1000, 1016),
                ("hunter2".into(), 1000, 984),
            ],
        );
        let s = db.standings();
        assert_eq!(s[0].name, "hunter1");
        assert_eq!(s[0].wins, 1);
        let m = db.recent_matches(5);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].winner.as_deref(), Some("hunter1"));
    }
}
