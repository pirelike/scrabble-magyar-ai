//! Mentett játékok, játékosaik, lépésnapló, értékszám-frissítés, megosztás és ranglista.

use super::{Db, Row, RowExt, fetch_all, fetch_one, placeholders};
use crate::elo;
use crate::util::{self, token_urlsafe};
use rusqlite::{OptionalExtension, Transaction, params, params_from_iter};
use serde_json::{Value, json};
use std::collections::HashMap;

/// Egy játékos adatai mentéshez / befejezéshez.
#[derive(Clone, Debug, Default)]
pub struct GamePlayerData {
    pub player_name: String,
    pub user_id: Option<i64>,
    pub score: i64,
    pub is_winner: bool,
    pub resigned: bool,
}

/// A lépésnapló egy tárolt sora.
#[derive(Clone, Debug)]
pub struct StoredMove {
    pub move_number: i64,
    pub player_name: String,
    pub action_type: String,
    pub details_json: Option<String>,
    pub board_snapshot_json: Option<String>,
}

pub const LEADERBOARD_METRICS: [&str; 5] = ["rating", "wins", "win_rate", "avg_score", "best_game"];
pub const LEADERBOARD_MAX_LIMIT: i64 = 100;

/// A százalékos / átlag alapú listákra csak elég sok játékkal lehet kerülni (különben egy nyert játék után
/// 100%-kal vezetne valaki).
pub fn leaderboard_min_games(metric: &str) -> i64 {
    match metric {
        "rating" => elo::MIN_RATED_GAMES_FOR_RANKING,
        "wins" => 1,
        "win_rate" => 3,
        "avg_score" => 3,
        "best_game" => 1,
        _ => 1,
    }
}

/// A rendezés (a metrikához tartozó, rögzített SQL-részlet; felhasználói adat nem kerül bele).
fn leaderboard_order(metric: &str) -> &'static str {
    match metric {
        "rating" => "u.rating DESC, games_won DESC, total_score DESC",
        "wins" => "games_won DESC, (games_won * 1.0 / games_played) DESC, total_score DESC",
        "win_rate" => "(games_won * 1.0 / games_played) DESC, games_played DESC, games_won DESC",
        "avg_score" => "(total_score * 1.0 / games_played) DESC, games_played DESC",
        _ => "best_score DESC, games_won DESC",
    }
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round_ties_even() / 10.0
}

fn upsert_game_players(tx: &Transaction, game_id: i64, players: &[GamePlayerData]) -> rusqlite::Result<()> {
    for pd in players {
        let updated = tx.execute(
            "UPDATE game_players SET final_score = ?, user_id = COALESCE(?, user_id) WHERE game_id = ? AND player_name = ?",
            params![pd.score, pd.user_id, game_id, pd.player_name],
        )?;
        if updated == 0 {
            tx.execute(
                "INSERT INTO game_players (game_id, user_id, player_name, final_score) VALUES (?, ?, ?, ?)",
                params![game_id, pd.user_id, pd.player_name, pd.score],
            )?;
        }
    }
    Ok(())
}

/// ELO: a regisztrált játékosok értékszámának frissítése a végeredmény alapján (legalább két regisztrált
/// játékos kell; a vendégek nem vesznek részt). A változás a game_players sorban is rögzül.
fn apply_ratings(tx: &Transaction, game_id: i64, players: &[GamePlayerData]) -> rusqlite::Result<()> {
    let mut ranked: Vec<(i64, i64, i64, i64)> = Vec::new(); // (uid, rating, rated_games, score)
    for pd in players {
        let Some(uid) = pd.user_id.filter(|u| *u != 0) else { continue };
        if ranked.iter().any(|(u, ..)| *u == uid) {
            continue;
        }
        let row: Option<(i64, i64)> = tx.query_row("SELECT rating, rated_games FROM users WHERE id = ?", [uid], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        if let Some((rating, rated_games)) = row {
            // Aki feladta, az pontszámától függetlenül mindenki mögé kerül
            let score = if pd.resigned { -1_000_000_000 } else { pd.score };
            ranked.push((uid, rating, rated_games, score));
        }
    }
    if ranked.len() < 2 {
        return Ok(());
    }
    let changes = elo::rating_changes(&ranked);
    for (uid, before, _games, _score) in &ranked {
        let after = before + changes[uid];
        tx.execute("UPDATE users SET rating = ?, rated_games = rated_games + 1 WHERE id = ?", params![after, uid])?;
        tx.execute("UPDATE game_players SET rating_before = ?, rating_after = ? WHERE game_id = ? AND user_id = ?", params![before, after, game_id, uid])?;
    }
    Ok(())
}

fn opponents_by_game(rows: Vec<Row>, shape: fn(&Row) -> Value) -> HashMap<i64, Vec<Value>> {
    let mut map: HashMap<i64, Vec<Value>> = HashMap::new();
    for row in rows {
        map.entry(row.int("game_id")).or_default().push(shape(&row));
    }
    map
}

impl Db {
    // --- Game persistence ---

    /// Játék mentése (upsert: room_id + active alapján). Visszaadja a game_id-t.
    #[allow(clippy::too_many_arguments)]
    pub fn save_game(
        &self,
        room_id: &str,
        room_name: &str,
        state_json: &str,
        challenge_mode: bool,
        players: &[GamePlayerData],
        owner_name: &str,
        owner_token: Option<&str>,
        has_bots: bool,
        is_async: bool,
    ) -> rusqlite::Result<i64> {
        self.with(|tx| {
            let now = util::now_ts();
            // Atomic upsert: UPDATE first, INSERT only if no row was updated
            let updated = tx.execute(
                "UPDATE saved_games SET state_json = ?, updated_at = ?, owner_name = ?, owner_token = ?, has_bots = ? \
                 WHERE room_id = ? AND status = 'active'",
                params![state_json, now, owner_name, owner_token, has_bots as i64, room_id],
            )?;
            let game_id = if updated > 0 {
                tx.query_row("SELECT id FROM saved_games WHERE room_id = ? AND status = 'active'", [room_id], |r| r.get(0))?
            } else {
                tx.execute(
                    "INSERT INTO saved_games (room_id, room_name, state_json, status, challenge_mode, owner_name, \
                     owner_token, has_bots, is_async) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    params![room_id, room_name, state_json, "active", challenge_mode as i64, owner_name, owner_token, has_bots as i64, is_async as i64],
                )?;
                tx.last_insert_rowid()
            };
            if !players.is_empty() {
                upsert_game_players(tx, game_id, players)?;
            }
            Ok(game_id)
        })
    }

    /// Játék befejezése: status='finished', game_players frissítés, a felhasználók statisztikája.
    /// Robotos játék nem számít az értékszámba (és a ranglistába sem).
    pub fn finish_game(&self, room_id: &str, state_json: &str, players: &[GamePlayerData], room_name: &str, has_bots: bool) -> rusqlite::Result<i64> {
        self.with(|tx| {
            let now = util::now_ts();
            let existing: Option<i64> =
                tx.query_row("SELECT id FROM saved_games WHERE room_id = ? AND status = 'active'", [room_id], |r| r.get(0)).optional()?;
            let game_id = match existing {
                None => {
                    tx.execute(
                        "INSERT INTO saved_games (room_id, room_name, state_json, status, has_bots) VALUES (?, ?, ?, ?, ?)",
                        params![room_id, room_name, state_json, "finished", has_bots as i64],
                    )?;
                    tx.last_insert_rowid()
                }
                Some(id) => {
                    tx.execute(
                        "UPDATE saved_games SET state_json = ?, status = ?, updated_at = ?, has_bots = ? WHERE id = ?",
                        params![state_json, "finished", now, has_bots as i64, id],
                    )?;
                    id
                }
            };
            for pd in players {
                let found: Option<i64> = tx
                    .query_row("SELECT id FROM game_players WHERE game_id = ? AND player_name = ?", params![game_id, pd.player_name], |r| r.get(0))
                    .optional()?;
                match found {
                    Some(id) => {
                        tx.execute(
                            "UPDATE game_players SET final_score = ?, is_winner = ?, user_id = COALESCE(?, user_id) WHERE id = ?",
                            params![pd.score, pd.is_winner as i64, pd.user_id, id],
                        )?;
                    }
                    None => {
                        tx.execute(
                            "INSERT INTO game_players (game_id, user_id, player_name, final_score, is_winner) VALUES (?, ?, ?, ?, ?)",
                            params![game_id, pd.user_id, pd.player_name, pd.score, pd.is_winner as i64],
                        )?;
                    }
                }
                if let Some(uid) = pd.user_id.filter(|u| *u != 0) {
                    tx.execute(
                        "UPDATE users SET games_played = games_played + 1, games_won = games_won + ?, total_score = total_score + ? WHERE id = ?",
                        params![pd.is_winner as i64, pd.score, uid],
                    )?;
                }
            }
            if !has_bots {
                apply_ratings(tx, game_id, players)?;
            }
            Ok(game_id)
        })
    }

    /// Lépés hozzáadása a játékhoz.
    pub fn add_game_move(&self, game_id: i64, mv: &StoredMove) -> rusqlite::Result<()> {
        self.with(|tx| {
            tx.execute(
                "INSERT INTO game_moves (game_id, move_number, player_name, action_type, details_json, board_snapshot_json) \
                 VALUES (?, ?, ?, ?, ?, ?)",
                params![game_id, mv.move_number, mv.player_name, mv.action_type, mv.details_json, mv.board_snapshot_json],
            )?;
            Ok(())
        })
    }

    /// Több lépés egyetlen tranzakcióban.
    pub fn add_game_moves(&self, game_id: i64, moves: &[StoredMove]) -> rusqlite::Result<()> {
        self.with(|tx| {
            for mv in moves {
                tx.execute(
                    "INSERT INTO game_moves (game_id, move_number, player_name, action_type, details_json, board_snapshot_json) \
                     VALUES (?, ?, ?, ?, ?, ?)",
                    params![game_id, mv.move_number, mv.player_name, mv.action_type, mv.details_json, mv.board_snapshot_json],
                )?;
            }
            Ok(())
        })
    }

    /// Az aktív játékok (frissítés szerint csökkenő).
    pub fn load_active_games(&self) -> rusqlite::Result<Vec<Row>> {
        self.with(|tx| fetch_all(tx, "SELECT * FROM saved_games WHERE status = 'active' ORDER BY updated_at DESC", []))
    }

    /// Lépések move_number sorrendben.
    pub fn get_game_moves(&self, game_id: i64) -> rusqlite::Result<Vec<StoredMove>> {
        self.with(|tx| self.get_game_moves_in(tx, game_id))
    }

    /// Ugyanez egy már nyitott tranzakcióban (az admin műveletek blokkjában nem szabad újra zárolni az adatbázist).
    pub fn get_game_moves_in(&self, tx: &Transaction, game_id: i64) -> rusqlite::Result<Vec<StoredMove>> {
        let rows = fetch_all(tx, "SELECT * FROM game_moves WHERE game_id = ? ORDER BY move_number", [game_id])?;
        Ok(rows
            .iter()
            .map(|r| StoredMove {
                move_number: r.int("move_number"),
                player_name: r.text("player_name"),
                action_type: r.text("action_type"),
                details_json: r.opt_text("details_json"),
                board_snapshot_json: r.opt_text("board_snapshot_json"),
            })
            .collect())
    }

    /// Befejezett játékok + ellenfelek a felhasználóhoz (egyetlen query, N+1 fix).
    pub fn get_user_game_history(&self, user_id: i64, limit: i64) -> rusqlite::Result<Vec<Value>> {
        self.with(|tx| {
            let rows = fetch_all(
                tx,
                "SELECT sg.id as game_id, sg.room_name, sg.created_at, \
                 gp.final_score, gp.is_winner, gp.player_name, gp.rating_before, gp.rating_after \
                 FROM saved_games sg JOIN game_players gp ON sg.id = gp.game_id AND gp.user_id = ? \
                 WHERE sg.status = 'finished' ORDER BY sg.created_at DESC LIMIT ?",
                params![user_id, limit],
            )?;
            if rows.is_empty() {
                return Ok(Vec::new());
            }
            let ids: Vec<i64> = rows.iter().map(|r| r.int("game_id")).collect();
            let sql = format!(
                "SELECT game_id, player_name, final_score, is_winner FROM game_players WHERE game_id IN ({}) AND user_id IS NOT ?",
                placeholders(ids.len())
            );
            let mut args: Vec<i64> = ids.clone();
            args.push(user_id);
            let opponents = opponents_by_game(
                fetch_all(tx, &sql, params_from_iter(args))?,
                |o| json!({"player_name": o.text("player_name"), "final_score": o.int("final_score"), "is_winner": o.int("is_winner")}),
            );
            Ok(rows
                .iter()
                .map(|r| {
                    json!({
                        "game_id": r.int("game_id"),
                        "room_name": r.text("room_name"),
                        "created_at": r.text("created_at"),
                        "final_score": r.int("final_score"),
                        "is_winner": r.int("is_winner"),
                        "player_name": r.text("player_name"),
                        "rating_before": r.get("rating_before").cloned().unwrap_or(Value::Null),
                        "rating_after": r.get("rating_after").cloned().unwrap_or(Value::Null),
                        "opponents": opponents.get(&r.int("game_id")).cloned().unwrap_or_default(),
                    })
                })
                .collect())
        })
    }

    /// Egyetlen játék sor.
    pub fn get_game_by_id(&self, game_id: i64) -> rusqlite::Result<Option<Row>> {
        self.with(|tx| fetch_one(tx, "SELECT * FROM saved_games WHERE id = ?", [game_id]))
    }

    /// Az összes folyamatban lévő levelezős játék (szerverindításkor a szobák visszaépítéséhez).
    pub fn get_active_async_games(&self) -> rusqlite::Result<Vec<Row>> {
        self.with(|tx| fetch_all(tx, "SELECT * FROM saved_games WHERE status = 'active' AND is_async = 1 ORDER BY id", []))
    }

    /// A felhasználó folyamatban lévő levelezős játékai: a mentett állapot + a saját játékosneve.
    pub fn get_user_async_games(&self, user_id: i64) -> rusqlite::Result<Vec<Row>> {
        self.with(|tx| {
            fetch_all(
                tx,
                "SELECT sg.id AS game_id, sg.room_id, sg.room_name, sg.state_json, sg.updated_at, \
                 sg.created_at, gp.player_name AS my_name \
                 FROM game_players gp JOIN saved_games sg ON sg.id = gp.game_id \
                 WHERE gp.user_id = ? AND sg.status = 'active' AND sg.is_async = 1 \
                 ORDER BY sg.updated_at DESC",
                [user_id],
            )
        })
    }

    /// Felhasználó aktív (mentett) játékai — csak ahol ő az owner.
    pub fn get_user_active_games(&self, user_id: i64, reconnect_token: Option<&str>) -> rusqlite::Result<Vec<Row>> {
        self.with(|tx| {
            let rows = match reconnect_token.filter(|t| !t.is_empty()) {
                Some(token) => fetch_all(
                    tx,
                    "SELECT gp.game_id, gp.player_name, gp.final_score, \
                     sg.room_name, sg.room_id, sg.created_at, sg.updated_at, sg.challenge_mode, sg.owner_name, sg.owner_token \
                     FROM game_players gp JOIN saved_games sg ON gp.game_id = sg.id \
                     WHERE gp.user_id = ? AND sg.status = 'active' AND sg.owner_token = ? AND sg.is_async = 0 \
                     ORDER BY sg.updated_at DESC",
                    params![user_id, token],
                )?,
                None => fetch_all(
                    tx,
                    "SELECT gp.game_id, gp.player_name, gp.final_score, \
                     sg.room_name, sg.room_id, sg.created_at, sg.updated_at, sg.challenge_mode, sg.owner_name, sg.owner_token \
                     FROM game_players gp JOIN saved_games sg ON gp.game_id = sg.id \
                     JOIN game_players gp_owner ON gp_owner.game_id = sg.id AND gp_owner.user_id = ? AND gp_owner.player_name = sg.owner_name \
                     WHERE gp.user_id = ? AND sg.status = 'active' AND sg.is_async = 0 \
                     ORDER BY sg.updated_at DESC",
                    params![user_id, user_id],
                )?,
            };
            if rows.is_empty() {
                return Ok(Vec::new());
            }
            let ids: Vec<i64> = rows.iter().map(|r| r.int("game_id")).collect();
            let sql = format!("SELECT game_id, player_name, final_score FROM game_players WHERE game_id IN ({}) AND user_id IS NOT ?", placeholders(ids.len()));
            let mut args: Vec<i64> = ids;
            args.push(user_id);
            let opponents =
                opponents_by_game(fetch_all(tx, &sql, params_from_iter(args))?, |o| json!({"name": o.text("player_name"), "score": o.int("final_score")}));
            Ok(rows
                .into_iter()
                .map(|mut r| {
                    let game_id = r.int("game_id");
                    r.insert("opponents".into(), Value::Array(opponents.get(&game_id).cloned().unwrap_or_default()));
                    r
                })
                .collect())
        })
    }

    /// Ellenőrzi, hogy a felhasználó részese-e a játéknak.
    pub fn is_user_in_game(&self, game_id: i64, user_id: i64) -> bool {
        self.with(|tx| {
            tx.query_row("SELECT id FROM game_players WHERE game_id = ? AND user_id = ?", params![game_id, user_id], |r| r.get::<_, i64>(0)).optional()
        })
        .ok()
        .flatten()
        .is_some()
    }

    /// A befejezett játék visszajátszásának megosztási tokenje (ha még nincs, létrehozza).
    /// Visszatér: token, vagy None, ha a játék nem létezik / még nem fejeződött be.
    pub fn get_or_create_share_token(&self, game_id: i64) -> rusqlite::Result<Option<String>> {
        self.with(|tx| {
            let row: Option<(String, Option<String>)> =
                tx.query_row("SELECT status, share_token FROM saved_games WHERE id = ?", [game_id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
            let Some((status, token)) = row else { return Ok(None) };
            if status != "finished" {
                return Ok(None);
            }
            if let Some(token) = token.filter(|t| !t.is_empty()) {
                return Ok(Some(token));
            }
            let token = token_urlsafe(9);
            tx.execute("UPDATE saved_games SET share_token = ? WHERE id = ?", params![token, game_id])?;
            Ok(Some(token))
        })
    }

    /// A megosztási tokenhez tartozó befejezett játék (vagy None).
    pub fn get_game_by_share_token(&self, token: &str) -> rusqlite::Result<Option<Row>> {
        if token.is_empty() {
            return Ok(None);
        }
        self.with(|tx| fetch_one(tx, "SELECT * FROM saved_games WHERE share_token = ? AND status = 'finished'", [token]))
    }

    /// A játék végeredménye: pontszám szerint csökkenően.
    pub fn get_game_results(&self, game_id: i64) -> rusqlite::Result<Vec<Value>> {
        self.with(|tx| {
            let rows =
                fetch_all(tx, "SELECT player_name, final_score, is_winner FROM game_players WHERE game_id = ? ORDER BY final_score DESC, id", [game_id])?;
            Ok(rows
                .iter()
                .map(|r| json!({"player_name": r.text("player_name"), "final_score": r.int("final_score"), "is_winner": r.flag("is_winner")}))
                .collect())
        })
    }

    /// Egy játék értékszám-változásai: {user_id: (előtte, utána)} (csak az értékelt játékosok).
    pub fn get_game_rating_changes(&self, game_id: i64) -> rusqlite::Result<HashMap<i64, (i64, i64)>> {
        self.with(|tx| {
            let rows = fetch_all(
                tx,
                "SELECT user_id, rating_before, rating_after FROM game_players \
                 WHERE game_id = ? AND user_id IS NOT NULL AND rating_after IS NOT NULL",
                [game_id],
            )?;
            Ok(rows.iter().map(|r| (r.int("user_id"), (r.int("rating_before"), r.int("rating_after")))).collect())
        })
    }

    /// Visszaadja a játék játékosait: (név, user_id, végpont).
    pub fn get_game_players(&self, game_id: i64) -> rusqlite::Result<Vec<(String, Option<i64>, i64)>> {
        self.with(|tx| {
            let rows = fetch_all(tx, "SELECT player_name, user_id, final_score FROM game_players WHERE game_id = ?", [game_id])?;
            Ok(rows.iter().map(|r| (r.text("player_name"), r.opt_int("user_id"), r.int("final_score"))).collect())
        })
    }

    /// Egy mentett játék állapotának átállítása (az `updated_at` nem változik: az értékszám-újraszámolás a
    /// befejezés idejéből dolgozik).
    pub fn set_game_status(&self, game_id: i64, status: &str) -> rusqlite::Result<()> {
        self.with(|tx| tx.execute("UPDATE saved_games SET status = ? WHERE id = ?", params![status, game_id]).map(|_| ()))
    }

    /// Játék elhagyása: status='abandoned'.
    pub fn abandon_game(&self, room_id: &str) {
        let _ = self.with(|tx| {
            tx.execute("UPDATE saved_games SET status = 'abandoned', updated_at = datetime('now') WHERE room_id = ? AND status = 'active'", [room_id])
        });
    }

    /// Játék elhagyása ID alapján: status='abandoned'.
    pub fn abandon_game_by_id(&self, game_id: i64) {
        let _ = self
            .with(|tx| tx.execute("UPDATE saved_games SET status = 'abandoned', updated_at = datetime('now') WHERE id = ? AND status = 'active'", [game_id]));
    }

    // --- Ranglista ---

    /// Regisztrált játékosok ranglistája (csak befejezett, robot nélküli játékokból).
    ///
    /// Visszatér: (a legjobb `limit` játékos rank-kal, a megadott felhasználó helyezése — akkor is, ha nincs
    /// a top listában — vagy None).
    pub fn get_leaderboard(&self, metric: &str, limit: i64, user_id: Option<i64>, ignore_min: bool) -> rusqlite::Result<(Vec<Value>, Option<Value>)> {
        let metric = if LEADERBOARD_METRICS.contains(&metric) { metric } else { "wins" };
        let limit = limit.clamp(1, LEADERBOARD_MAX_LIMIT);
        let min_games = if ignore_min { 1 } else { leaderboard_min_games(metric) };
        // Az értékszám-ranglistára csak elég sok értékelt (regisztráltak közti) játék után lehet kerülni
        let rated_min = if ignore_min { 0 } else { min_games };
        let rated_only = if metric == "rating" { "AND u.rated_games >= ? " } else { "" };
        let args: Vec<i64> = if metric == "rating" { vec![rated_min, min_games] } else { vec![min_games] };
        let sql = format!(
            "SELECT gp.user_id AS user_id, u.display_name AS display_name, \
             COUNT(*) AS games_played, COALESCE(SUM(gp.is_winner), 0) AS games_won, \
             COALESCE(SUM(gp.final_score), 0) AS total_score, MAX(gp.final_score) AS best_score, \
             u.rating AS rating, u.rated_games AS rated_games \
             FROM game_players gp \
             JOIN saved_games sg ON sg.id = gp.game_id AND sg.status = 'finished' AND sg.has_bots = 0 \
             JOIN users u ON u.id = gp.user_id AND u.deleted_at IS NULL \
             WHERE gp.user_id IS NOT NULL \
             {rated_only}\
             GROUP BY gp.user_id \
             HAVING COUNT(*) >= ? \
             ORDER BY {}, gp.user_id ASC",
            leaderboard_order(metric)
        );
        let rows = self.with(|tx| fetch_all(tx, &sql, params_from_iter(args)))?;
        let entry = |row: &Row, rank: usize| {
            let played = row.int("games_played");
            json!({
                "rank": rank,
                "user_id": row.int("user_id"),
                "display_name": row.text("display_name"),
                "games_played": played,
                "games_won": row.int("games_won"),
                "win_rate": if played > 0 { json!(round1(row.int("games_won") as f64 / played as f64 * 100.0)) } else { json!(0) },
                "avg_score": if played > 0 { json!(round1(row.int("total_score") as f64 / played as f64)) } else { json!(0) },
                "total_score": row.int("total_score"),
                "best_score": row.int("best_score"),
                "rating": row.int("rating"),
                "rated_games": row.int("rated_games"),
            })
        };
        let mut entries = Vec::new();
        let mut me = None;
        for (i, row) in rows.iter().enumerate() {
            let rank = i + 1;
            if rank as i64 <= limit {
                entries.push(entry(row, rank));
            }
            if user_id.is_some() && Some(row.int("user_id")) == user_id {
                me = Some(entry(row, rank));
            }
        }
        Ok((entries, me))
    }
}
