//! A játékszerver közös műveletei (a Python `server.py` segédfüggvényei): állapotküldés, mentés, időzítők, robotok,
//! szobák életciklusa. Minden függvény a hívó által már zárolt `ServerState`-en dolgozik (a zárat az eseménykezelők,
//! időzítők és HTTP útvonalak belépési pontjai veszik fel — egy függvény sem zárol újra).

use crate::achievements;
use crate::admin::live;
use crate::ai::{self, Action, Difficulty};
use crate::app::App;
use crate::board::Placed;
use crate::db::GamePlayerData;
use crate::game::{CHALLENGE_TIMEOUT, Game, VoteResult};
use crate::player::Player;
use crate::room::Room;
use crate::state::ServerState;
use crate::tiles::Tile;
use crate::util;
use rand::RngExt;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

pub const DISCONNECT_GRACE_PERIOD: i64 = 120;
/// Várakozó szoba (a játék indítása előtt): a tulajdonos ennyi ideig lehet távol, mielőtt a szoba megszűnik.
/// Telefonon az üzenetküldő appra váltás (a kód / meghívó link elküldése) megszakítja a kapcsolatot, ezért ez
/// hosszabb, mint a játék közbeni türelmi idő.
pub const WAITING_OWNER_GRACE_PERIOD: i64 = 600;
pub const ALLOWED_TURN_TIME_LIMITS: [i64; 6] = [0, 60, 90, 120, 180, 300];
/// A visszavont lerakás után legalább ennyi idő marad a körből (mp).
pub const MIN_TIME_AFTER_WITHDRAW: f64 = 10.0;
pub const MAX_BOTS: usize = 3;
pub const ADMIN_ROOM: &str = "admin";

const VALID_LETTERS: [&str; 38] = [
    "A", "Á", "B", "C", "CS", "D", "E", "É", "F", "G", "GY", "H", "I", "Í", "J", "K", "L", "LY", "M", "N", "NY", "O", "Ó", "Ö", "Ő", "P", "R",
    "S", "SZ", "T", "TY", "U", "Ú", "Ü", "Ű", "V", "Z", "ZS",
];

// --- Input validáció ---

fn name_char_ok(c: char, extra: &str) -> bool {
    util::is_word_char(c) || c.is_whitespace() || ".-".contains(c) || extra.contains(c)
}

fn sanitize_text(value: &Value, extra: &str, max_len: usize) -> Option<String> {
    let text = value.as_str()?.trim();
    if text.is_empty() || text.chars().count() > max_len {
        return None;
    }
    text.chars().all(|c| name_char_ok(c, extra)).then(|| text.to_string())
}

/// Játékos név validálása és tisztítása.
pub fn sanitize_name(value: &Value) -> Option<String> {
    sanitize_text(value, "", 20)
}

pub fn sanitize_name_str(name: &str) -> Option<String> {
    sanitize_name(&Value::from(name))
}

/// Szoba név validálása és tisztítása.
pub fn sanitize_room_name(value: &Value) -> Option<String> {
    sanitize_text(value, "!?", 30)
}

/// Validálja a tiles listát a place_tiles handlerhez. Visszatér: a lerakott zsetonok vagy a hibaüzenet.
pub fn validate_tiles_input(tiles: &Value) -> Result<Vec<Placed>, String> {
    let Some(list) = tiles.as_array().filter(|l| !l.is_empty() && l.len() <= 7) else {
        return Err("Érvénytelen lerakás.".to_string());
    };
    let mut placed = Vec::new();
    for t in list {
        let Some(object) = t.as_object() else { return Err("Érvénytelen adat.".to_string()) };
        let (Some(row), Some(col)) = (object.get("row").and_then(util::py_int), object.get("col").and_then(util::py_int)) else {
            return Err("Érvénytelen pozíció.".to_string());
        };
        if !(0..=14).contains(&row) || !(0..=14).contains(&col) {
            return Err("Pozíció a táblán kívül.".to_string());
        }
        let letter = object.get("letter").cloned().unwrap_or_else(|| json!(""));
        let is_blank = object.get("is_blank").map(util::truthy).unwrap_or(false);
        let Some(letter) = letter.as_str() else { return Err("Érvénytelen betű.".to_string()) };
        if !VALID_LETTERS.contains(&letter) {
            return Err(format!("Érvénytelen betű: {letter}"));
        }
        placed.push(Placed::new(row as i32, col as i32, Tile::from_str(letter).expect("érvényes betű"), is_blank));
    }
    Ok(placed)
}

// --- Robotok ---

/// A robot fokozatának sávja (név és gondolkodási idő): 1–3 könnyű, 4–7 közepes, 8–10 nehéz.
pub fn bot_tier(difficulty: Option<Difficulty>) -> &'static str {
    let level = difficulty.and_then(|d| d.level()).unwrap_or(ai::DEFAULT_LEVEL);
    if level <= 3 {
        "easy"
    } else if level <= 7 {
        "medium"
    } else {
        "hard"
    }
}

/// Robot "gondolkodási" ideje másodpercben (min, max): a lépés ennyi várakozás után jelenik meg.
fn think_delay(tier: &str) -> (f64, f64) {
    match tier {
        "easy" => (1.5, 3.0),
        "medium" => (1.2, 2.6),
        _ => (1.0, 2.2),
    }
}

fn bot_name_pool(tier: &str) -> [&'static str; 3] {
    match tier {
        "easy" => ["Robi", "Rozi", "Rudi"],
        "medium" => ["Rita", "Ricsi", "Réka"],
        _ => ["Rezső", "Róbert", "Rómeó"],
    }
}

/// Szabad robotnév az adott nehézséghez (nem ütközik a szobában lévő nevekkel).
pub fn bot_name(game: &Game, difficulty: Option<Difficulty>) -> String {
    let taken: HashSet<String> = game.players.iter().map(|p| util::casefold(&p.name)).collect();
    let pool = bot_name_pool(bot_tier(difficulty));
    for name in pool {
        if !taken.contains(&util::casefold(name)) {
            return name.to_string();
        }
    }
    let mut n = 2;
    while taken.contains(&util::casefold(&format!("{} {n}", pool[0]))) {
        n += 1;
    }
    format!("{} {n}", pool[0])
}

/// Lépnie kell-e most a soron lévő robotnak?
pub fn bot_should_move(room: &Room) -> bool {
    let game = &room.game;
    if !game.started || game.finished || game.pending_challenge.is_some() {
        return false;
    }
    let Some(current) = game.current_player() else { return false };
    if !current.is_bot {
        return false;
    }
    // Emberi néző nélkül a robotok nem játszanak egymás ellen
    game.has_connected_human() || !room.spectators.is_empty()
}

// --- Állapotküldés ---

/// Minden játékosnak (és megfigyelőnek) elküldi a saját állapotát; mentés / push / admin értesítés is itt fut.
pub fn emit_all_states(app: &Arc<App>, st: &mut ServerState, room_id: &str) {
    {
        let Some(room) = st.rooms.get_mut(room_id) else { return };
        room.touch();
        let expires_at = room.turn_timer_expires_at;
        let spectator_count = room.spectators.len();
        // A lecsatlakozott / kilépett játékos SID-je még élhet (a lobbyban van, pl. levelezős játékból kilépve): neki
        // nem küldünk állapotot, különben a kliense visszaugrana a játékképernyőre
        let away: HashSet<String> = room.game.players.iter().filter(|p| p.disconnected).map(|p| p.id.clone()).collect();
        for (player_id, mut state) in room.game.get_all_states() {
            if away.contains(&player_id) {
                continue;
            }
            state["turn_timer_expires_at"] = json!(expires_at);
            state["spectator_count"] = json!(spectator_count);
            app.emit_to(&player_id, "game_state", &state);
        }
        if !room.spectators.is_empty() {
            let mut state = room.game.get_spectator_state();
            state["turn_timer_expires_at"] = json!(expires_at);
            state["spectator_count"] = json!(spectator_count);
            for sid in room.spectators.keys() {
                app.emit_to(sid, "game_state", &state);
            }
        }
    }
    persist_async(app, st, room_id);
    maybe_push_turn(app, st, room_id);
    notify_admins(app, st, room_id);
}

/// Élő szoba-változás az admin panelnek: összegzés az `admin` szobának, teljes állapot a szobát figyelőknek.
/// Csak akkor számol, ha van, aki kapja.
pub fn notify_admins(app: &Arc<App>, st: &mut ServerState, room_id: &str) {
    let Some(room) = st.rooms.get(room_id) else { return };
    if app.room_member_count(ADMIN_ROOM) > 0 {
        app.emit_room(ADMIN_ROOM, "admin_room_update", &live::room_summary(app, st, room, None));
    }
    if !room.admin_watchers.is_empty() {
        let detail = live::room_detail(app, st, room_id);
        for watcher in &st.rooms[room_id].admin_watchers {
            app.emit_to(watcher, "admin_room_state", &detail);
        }
    }
}

/// Levelezős játék: minden változás után menti az állapotot (a játék tartós otthona az adatbázis).
fn persist_async(app: &Arc<App>, st: &mut ServerState, room_id: &str) {
    let Some(room) = st.rooms.get_mut(room_id) else { return };
    if !room.is_async || !room.game.started {
        return;
    }
    let game = &room.game;
    let signature = (game.move_log.len(), game.finished, game.turn_number);
    if room.persisted_sig != Some(signature) {
        room.persisted_sig = Some(signature);
        save_game_to_db(app, st, room_id);
    }
}

/// A játékos regisztrált user_id-ja: élő auth info, különben a mentésből ismert (visszaállított játéknál a még meg
/// nem érkezett játékosok is megtartják a fiókjukat).
pub fn user_id_for_player(room: &Room, player: &Player, player_auth: &HashMap<String, crate::state::AuthInfo>) -> Option<i64> {
    player_auth
        .get(&player.id)
        .and_then(|a| a.user_id)
        .or_else(|| room.known_user_ids.get(&player.name).copied().flatten())
}

/// Web Push „Te jössz!”, ha a soron lévő regisztrált játékos nincs jelen (lecsatlakozott vagy a böngészőlapja
/// háttérbe került). Körönként legfeljebb egyszer.
pub fn maybe_push_turn(app: &Arc<App>, st: &mut ServerState, room_id: &str) {
    let (user_id, room_name, turn_number, is_async, notify_in_app) = {
        let Some(room) = st.rooms.get_mut(room_id) else { return };
        let game = &room.game;
        if room.is_puzzle || !game.started || game.finished || game.pending_challenge.is_some() {
            return;
        }
        let Some(player) = game.current_player() else { return };
        if player.is_bot || room.pushed_turn == game.turn_number {
            return;
        }
        if !(player.disconnected || st.hidden_sids.contains(&player.id)) {
            return;
        }
        let Some(user_id) = user_id_for_player(room, player, &st.player_auth) else { return };
        let turn_number = game.turn_number;
        let notify_in_app = room.is_async && room.notified_turn != turn_number;
        if notify_in_app {
            room.notified_turn = turn_number;
        }
        (user_id, room.name.clone(), turn_number, room.is_async, notify_in_app)
    };
    if is_async && notify_in_app {
        // A lobbyban (más képernyőn) lévő játékos az alkalmazáson belül is értesül
        let db_game_id = st.rooms.get(room_id).and_then(|r| r.db_game_id);
        for sid in st.get_user_sids(user_id) {
            app.emit_to(&sid, "async_your_turn", &json!({"game_id": db_game_id, "room_name": room_name}));
        }
    }
    if app.push.notify_turn(user_id, &room_name, room_id, "turn") {
        if let Some(room) = st.rooms.get_mut(room_id) {
            room.pushed_turn = turn_number;
        }
    }
}

/// A nyilvános szobák és az élő játékok listájának frissítése minden kliensnek.
pub fn broadcast_rooms(app: &Arc<App>, st: &ServerState) {
    app.emit_all("rooms_list", &st.get_rooms_list());
    app.emit_all("live_games", &st.get_live_games());
}

pub fn emit_rooms_list(app: &Arc<App>, st: &ServerState) {
    app.emit_all("rooms_list", &st.get_rooms_list());
}

// --- Szobák életciklusa ---

/// Szoba törlése; a megfigyelőket értesíti és leválasztja.
pub fn cleanup_room(app: &Arc<App>, st: &mut ServerState, room_id: &str) {
    if let Some(room) = st.rooms.get_mut(room_id) {
        if !room.spectators.is_empty() {
            app.emit_room(room_id, "room_disbanded", &json!({"message": "A játék véget ért, a szoba megszűnt."}));
            let spectators: Vec<String> = room.spectators.keys().cloned().collect();
            for sid in spectators {
                app.leave_room(&sid, room_id);
            }
        }
        room.invalidate_bot_turn();
    }
    st.cleanup_room(room_id);
}

/// Szoba tulajdonjogának átadása az első online játékosnak.
pub fn transfer_ownership(app: &Arc<App>, st: &mut ServerState, room_id: &str) {
    let Some(room) = st.rooms.get(room_id) else { return };
    let game = &room.game;
    if game.players.is_empty() {
        return;
    }
    let humans: Vec<&Player> = game.players.iter().filter(|p| !p.is_bot).collect();
    let candidates: Vec<&Player> = if humans.is_empty() { game.players.iter().collect() } else { humans };
    let new_owner = candidates.iter().find(|p| !p.disconnected).unwrap_or(&candidates[0]);
    let (owner_id, owner_name) = (new_owner.id.clone(), new_owner.name.clone());
    let token = st.get_reconnect_token_for_sid(&owner_id);
    let code = room.join_code.clone();
    if let Some(room) = st.rooms.get_mut(room_id) {
        room.transfer_ownership(&owner_id, &owner_name, token.as_deref());
    }
    app.emit_to(&owner_id, "room_code", &json!({"code": code}));
}

/// Ha a soron lévő játékos lecsatlakozott, továbbadja a kört és újraindítja az időzítőt.
pub fn advance_turn_if_needed(app: &Arc<App>, st: &mut ServerState, room_id: &str) {
    let advanced = match st.rooms.get_mut(room_id) {
        Some(room) => room.game.skip_disconnected_current(),
        None => false,
    };
    if advanced {
        if let Some(room) = st.rooms.get_mut(room_id) {
            room.invalidate_turn_timer();
        }
        start_turn_timer(app, st, room_id, None);
        schedule_bot_turn(app, st, room_id);
    }
}

/// Aktív játék szoba feloszlatása: broadcast, játékosok kitakarítása, room törlés. `owner_sid`: ezt a SID-t nem
/// küldi room_left-nek (az owner maga kezeli).
pub fn disband_active_room(app: &Arc<App>, st: &mut ServerState, room_id: &str, message: &str, owner_sid: Option<&str>) {
    let Some(room) = st.rooms.get_mut(room_id) else { return };
    room.invalidate_turn_timer();
    let started_unfinished = room.game.started && !room.game.finished;
    let manually_saved = room.manually_saved;
    let db_game_id = room.db_game_id;
    let spectators: Vec<String> = room.spectators.keys().cloned().collect();
    let players: Vec<(String, bool)> = room.game.players.iter().map(|p| (p.id.clone(), p.is_bot)).collect();

    st.remove_invites_for_room(room_id);
    app.emit_room(room_id, "room_disbanded", &json!({"message": message}));
    for sid in &spectators {
        app.leave_room(sid, room_id);
    }
    if let Some(room) = st.rooms.get_mut(room_id) {
        room.invalidate_bot_turn();
    }
    for (pid, is_bot) in players {
        if is_bot {
            continue;
        }
        if Some(pid.as_str()) != owner_sid {
            app.leave_room(&pid, room_id);
            st.player_rooms.remove(&pid);
            st.cleanup_player_token(&pid);
            app.emit_to(&pid, "room_left", &json!({}));
        }
    }
    st.cleanup_room_tokens(room_id);
    if started_unfinished && !manually_saved {
        match db_game_id {
            Some(id) => app.db.abandon_game_by_id(id),
            None => app.db.abandon_game(room_id),
        }
    }
    st.cleanup_room(room_id);
}

// --- Időzítők ---

/// Challenge timeout visszaszámlálás.
pub fn start_challenge_timer(app: &Arc<App>, st: &mut ServerState, room_id: &str) {
    let Some(room) = st.rooms.get_mut(room_id) else { return };
    let timer_id = room.invalidate_challenge_timer();
    let (app, room_id) = (app.clone(), room_id.to_string());
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(CHALLENGE_TIMEOUT)).await;
        let mut st = app.state.lock();
        let Some(room) = st.rooms.get_mut(&room_id) else { return };
        if room.challenge_timer_id() != timer_id || room.game.pending_challenge.is_none() {
            return;
        }
        let Ok((result, message)) = room.game.accept_pending() else { return };
        let finished = room.game.finished;
        broadcast_challenge_result(&app, &room_id, result, &message);
        if finished {
            emit_all_states(&app, &mut st, &room_id);
            save_game_to_db(&app, &mut st, &room_id);
        } else {
            start_turn_timer(&app, &mut st, &room_id, None);
            emit_all_states(&app, &mut st, &room_id);
            schedule_bot_turn(&app, &mut st, &room_id);
        }
    });
}

/// Kör visszaszámlálás indítása. Ha lejár, auto-passz. `seconds`: a teljes körnél rövidebb hátralévő idő (pl. a
/// visszavont lerakás után).
pub fn start_turn_timer(app: &Arc<App>, st: &mut ServerState, room_id: &str, seconds: Option<f64>) {
    let Some(room) = st.rooms.get_mut(room_id) else { return };
    if room.game.turn_time_limit == 0 || room.game.pending_challenge.is_some() {
        return; // nincs időlimit, vagy challenge fut: nem indítunk kör timert
    }
    room.timer_paused_left = None;
    let timer_id = room.invalidate_turn_timer();
    let limit = seconds.unwrap_or(room.game.turn_time_limit as f64);
    room.turn_timer_expires_at = Some(util::now() + limit);
    let (app, room_id) = (app.clone(), room_id.to_string());
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs_f64(limit.max(0.0))).await;
        let mut st = app.state.lock();
        let Some(room) = st.rooms.get_mut(&room_id) else { return };
        if room.turn_timer_id() != timer_id {
            return; // már érvénytelen (új kör, vagy játék vége)
        }
        let game = &mut room.game;
        if game.finished || !game.started {
            return;
        }
        let Some(current) = game.current_player().cloned() else { return };
        // Auto-passz a jelenlegi játékos nevében
        if game.pass_turn(&current.id, false).is_ok() {
            let finished = game.finished;
            app.emit_room(&room_id, "action_result", &json!({"success": true, "message": format!("Időtúllépés: {} passzolt.", current.name)}));
            if finished {
                if let Some(room) = st.rooms.get_mut(&room_id) {
                    room.invalidate_turn_timer();
                }
                emit_all_states(&app, &mut st, &room_id);
                save_game_to_db(&app, &mut st, &room_id);
            } else {
                start_turn_timer(&app, &mut st, &room_id, None); // következő kör timere
                emit_all_states(&app, &mut st, &room_id);
                schedule_bot_turn(&app, &mut st, &room_id);
            }
        }
    });
}

/// Challenge/szavazás eredmény broadcast (ha vote).
pub fn broadcast_challenge_result(app: &Arc<App>, room_id: &str, result: VoteResult, message: &str) {
    if result == VoteResult::Accepted || result == VoteResult::Rejected {
        app.emit_room(room_id, "challenge_result", &json!({"challenge_won": result == VoteResult::Rejected, "message": message}));
    }
}

/// Challenge/szavazás eredmény feldolgozása: timer és broadcast.
pub fn handle_challenge_result(app: &Arc<App>, st: &mut ServerState, room_id: &str, result: VoteResult, message: &str) {
    if result != VoteResult::Recorded {
        if let Some(room) = st.rooms.get_mut(room_id) {
            room.invalidate_challenge_timer();
        }
    }
    broadcast_challenge_result(app, room_id, result, message);
    let finished = st.rooms.get(room_id).is_some_and(|r| r.game.finished);
    if finished {
        save_game_to_db(app, st, room_id);
    } else {
        start_turn_timer(app, st, room_id, None);
    }
    emit_all_states(app, st, room_id);
    schedule_bot_turn(app, st, room_id);
}

// --- Robot ellenfelek ---

/// Ha a soron lévő játékos robot, háttérfeladatban elindítja a lépését.
pub fn schedule_bot_turn(app: &Arc<App>, st: &mut ServerState, room_id: &str) {
    let Some(room) = st.rooms.get_mut(room_id) else { return };
    if !bot_should_move(room) {
        return;
    }
    let (turn_number, tier) = {
        let bot = room.game.current_player().expect("a robot soron van");
        (room.game.turn_number, bot_tier(bot.difficulty))
    };
    let turn_id = room.invalidate_bot_turn();
    let (min, max) = think_delay(tier);
    let delay = rand::rng().random_range(min..max) * app.settings.get_f64("bot_think_multiplier");
    room.bot_scheduled_at = Some(util::now());
    let (app, room_id) = (app.clone(), room_id.to_string());
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs_f64(delay.max(0.0))).await;
        play_bot_turn(app, room_id, Some(turn_id), Some(turn_number)).await;
    });
}

/// Egy robotlépés után: időzítők, állapot küldése, esetleg a következő robot.
fn after_bot_move(app: &Arc<App>, st: &mut ServerState, room_id: &str) {
    let (pending, finished) = match st.rooms.get_mut(room_id) {
        Some(room) => {
            room.invalidate_turn_timer();
            (room.game.pending_challenge.is_some(), room.game.finished)
        }
        None => return,
    };
    if pending {
        start_challenge_timer(app, st, room_id);
    } else if finished {
        save_game_to_db(app, st, room_id);
    } else {
        start_turn_timer(app, st, room_id, None);
    }
    emit_all_states(app, st, room_id);
    if finished {
        broadcast_rooms(app, st);
    }
    schedule_bot_turn(app, st, room_id);
}

struct BotSnapshot {
    bot_id: String,
    turn_number: i64,
    board: crate::board::Board,
    hand: Vec<Tile>,
    level: u8,
    bag_remaining: usize,
    avoid: HashSet<Vec<Placed>>,
}

/// A soron lévő robot meglép egy lépést. Visszatér: igaz, ha lépett.
///
/// `turn_id` / `turn_number`: ha megadva, csak akkor lép, ha közben nem érvénytelenedett a lépés (kilépés, új kör,
/// mentés-visszaállítás). A keresés alatt (háttérszálon) az állapot megváltozhat, ezért a lépés előtt újraellenőriz.
pub async fn play_bot_turn(app: Arc<App>, room_id: String, turn_id: Option<u64>, turn_number: Option<i64>) -> bool {
    let snapshot = {
        let st = app.state.lock();
        let Some(room) = st.rooms.get(&room_id) else { return false };
        if turn_id.is_some_and(|id| room.bot_turn_id() != id) || !bot_should_move(room) {
            return false;
        }
        let game = &room.game;
        let bot = game.current_player().expect("a robot soron van");
        if turn_number.is_some_and(|n| game.turn_number != n) {
            return false;
        }
        let mut rng = rand::rng();
        BotSnapshot {
            bot_id: bot.id.clone(),
            turn_number: game.turn_number,
            board: game.board.clone(),
            hand: bot.hand.clone(),
            level: game.bot_level(bot, &mut rng, None),
            bag_remaining: game.bag.remaining(),
            avoid: game.rejected_placements.clone(),
        }
    };
    let action = {
        let (board, hand, level, bag, avoid) = (snapshot.board.clone(), snapshot.hand.clone(), snapshot.level, snapshot.bag_remaining, snapshot.avoid.clone());
        tokio::task::spawn_blocking(move || {
            let vocab = ai::get_vocabulary();
            let mut rng = rand::rng();
            ai::choose_action(&board, &hand, level, bag, &mut rng, &vocab, Some(&avoid), ai::TIME_BUDGET)
        })
        .await
        .unwrap_or_else(|e| {
            println!("[bot] Hiba a lépés keresésében ({room_id}): {e}");
            Action::Pass
        })
    };

    let mut st = app.state.lock();
    let Some(room) = st.rooms.get_mut(&room_id) else { return false };
    let game = &mut room.game;
    // A keresés közben megváltozhatott az állapot
    let unchanged = !game.finished
        && game.pending_challenge.is_none()
        && game.current_player().is_some_and(|p| p.id == snapshot.bot_id)
        && game.turn_number == snapshot.turn_number
        && turn_id.is_none_or(|id| room.bot_turn_id() == id);
    if !unchanged {
        return false;
    }
    let game = &mut room.game;
    let mut success = false;
    match &action {
        Action::Place { tiles, .. } => success = game.place_tiles(&snapshot.bot_id, tiles).is_ok(),
        Action::Exchange { indices } => {
            let indices: Vec<i64> = indices.iter().map(|i| *i as i64).collect();
            success = game.exchange_tiles(&snapshot.bot_id, &indices).is_ok();
        }
        Action::Pass => {}
    }
    if !success {
        success = game.pass_turn(&snapshot.bot_id, false).is_ok();
    }
    if !success {
        return false;
    }
    after_bot_move(&app, &mut st, &room_id);
    true
}

// --- Játék mentése ---

/// Játék mentése az adatbázisba. Visszatér: (sikerült-e, üzenet).
pub fn save_game_to_db(app: &Arc<App>, st: &mut ServerState, room_id: &str) -> (bool, String) {
    struct Input {
        state_json: String,
        owner_name: String,
        owner_token: Option<String>,
        has_bots: bool,
        finished: bool,
        players: Vec<GamePlayerData>,
        room_name: String,
        challenge_mode: bool,
        is_async: bool,
        new_moves: Vec<crate::db::StoredMove>,
        move_count: usize,
    }
    let input = {
        let Some(room) = st.rooms.get(room_id) else { return (false, "A szoba nem létezik.".to_string()) };
        let game = &room.game;
        if game.puzzle.is_some() {
            return (true, "Játék mentve.".to_string()); // a napi feladvány nem mentődik (az eredmény külön rögzül)
        }
        if !game.started {
            return (false, "A játék még nem indult el.".to_string());
        }
        if game.finished && room.result_saved {
            // A végeredményt már rögzítettük (statisztika ne duplázódjon).
            return (true, "Játék mentve.".to_string());
        }
        let owner_name = room
            .owner
            .as_ref()
            .and_then(|sid| st.player_names.get(sid).cloned())
            .unwrap_or_else(|| room.owner_name.clone());
        let winners: HashSet<&str> = game.winners.iter().map(|w| w.name.as_str()).collect();
        let players: Vec<GamePlayerData> = game
            .players
            .iter()
            .map(|p| GamePlayerData {
                player_name: p.name.clone(),
                user_id: user_id_for_player(room, p, &st.player_auth),
                score: p.score as i64,
                is_winner: winners.contains(p.name.as_str()),
                resigned: p.resigned,
            })
            .collect();
        let new_moves: Vec<crate::db::StoredMove> = game.move_log[room.last_saved_move_count.min(game.move_log.len())..]
            .iter()
            .map(|m| crate::db::StoredMove {
                move_number: m.move_number,
                player_name: m.player_name.clone(),
                action_type: m.action_type.clone(),
                details_json: Some(m.details_json()),
                board_snapshot_json: Some(m.board_snapshot_json()),
            })
            .collect();
        Input {
            state_json: game.to_save_dict().to_string(),
            owner_name,
            owner_token: room.owner_token.clone(),
            has_bots: game.players.iter().any(|p| p.is_bot),
            finished: game.finished,
            players,
            room_name: room.name.clone(),
            challenge_mode: game.challenge_mode,
            is_async: room.is_async,
            new_moves,
            move_count: game.move_log.len(),
        }
    };

    let db_id = if input.finished {
        app.db.finish_game(room_id, &input.state_json, &input.players, &input.room_name, input.has_bots)
    } else {
        app.db.save_game(
            room_id,
            &input.room_name,
            &input.state_json,
            input.challenge_mode,
            &input.players,
            &input.owner_name,
            input.owner_token.as_deref(),
            input.has_bots,
            input.is_async,
        )
    };
    let db_id = match db_id {
        Ok(id) => id,
        Err(e) => {
            println!("[save] Hiba a mentésnél ({room_id}): {e}");
            return (false, "Mentési hiba.".to_string());
        }
    };
    if let Some(room) = st.rooms.get_mut(room_id) {
        room.db_game_id = Some(db_id);
        if input.finished {
            room.result_saved = true;
        }
    }
    if input.finished {
        award_achievements(app, st, room_id, &input.players, db_id);
        announce_rating_changes(app, st, room_id, &input.players, db_id);
        announce_saved_game(app, st, room_id, &input.players, db_id);
        app.emit_all("rooms_list", &st.get_rooms_list());
        app.emit_all("live_games", &st.get_live_games());
    }
    // Új lépések mentése
    if let Err(e) = app.db.add_game_moves(db_id, &input.new_moves) {
        println!("[save] Hiba a lépések mentésénél ({room_id}): {e}");
        return (false, "Mentési hiba.".to_string());
    }
    if let Some(room) = st.rooms.get_mut(room_id) {
        room.last_saved_move_count = input.move_count;
    }
    (true, "Játék mentve.".to_string())
}

fn name_to_user(players: &[GamePlayerData]) -> HashMap<String, i64> {
    players.iter().filter_map(|p| p.user_id.filter(|u| *u != 0).map(|u| (p.player_name.clone(), u))).collect()
}

/// A befejezett játék kitüntetéseinek rögzítése; az újakról a játékos értesítést kap.
fn award_achievements(app: &Arc<App>, st: &ServerState, room_id: &str, players: &[GamePlayerData], db_id: i64) {
    let Some(room) = st.rooms.get(room_id) else { return };
    let names = name_to_user(players);
    let per_game = achievements::evaluate_game(&room.game, &names);
    for player in &room.game.players {
        let Some(uid) = names.get(&player.name) else { continue };
        if player.is_bot {
            continue;
        }
        let mut badges: HashSet<String> = per_game.get(uid).map(|b| b.iter().map(|s| s.to_string()).collect()).unwrap_or_default();
        if let Ok(Some(user)) = app.db.get_user_by_id(*uid) {
            badges.extend(achievements::cumulative_badges(user.games_played, user.games_won).into_iter().map(|s| s.to_string()));
        }
        let new = app.db.grant_achievements(*uid, &badges, Some(db_id));
        if !new.is_empty() {
            app.emit_to(&player.id, "achievements_earned", &json!({"badges": new}));
        }
    }
}

/// A regisztrált játékosoknak elküldi a befejezett játék azonosítóját (visszajátszás, elemzés).
fn announce_saved_game(app: &Arc<App>, st: &ServerState, room_id: &str, players: &[GamePlayerData], db_id: i64) {
    let Some(room) = st.rooms.get(room_id) else { return };
    let names = name_to_user(players);
    for player in &room.game.players {
        if !player.is_bot && names.contains_key(&player.name) {
            app.emit_to(&player.id, "game_saved", &json!({"game_id": db_id}));
        }
    }
}

/// Az értékelt játékosoknak elküldi az új értékszámukat és a változást.
fn announce_rating_changes(app: &Arc<App>, st: &ServerState, room_id: &str, players: &[GamePlayerData], db_id: i64) {
    let Some(room) = st.rooms.get(room_id) else { return };
    let Ok(changes) = app.db.get_game_rating_changes(db_id) else { return };
    let names = name_to_user(players);
    for player in &room.game.players {
        if player.is_bot {
            continue;
        }
        if let Some((before, after)) = names.get(&player.name).and_then(|uid| changes.get(uid)) {
            app.emit_to(&player.id, "rating_update", &json!({"rating": after, "change": after - before}));
        }
    }
}

/// Szerver induláskor befejezett állapotú aktív mentések lezárása.
pub fn cleanup_finished_saves(app: &Arc<App>) {
    let Ok(active) = app.db.load_active_games() else { return };
    for saved in active {
        let state_json = saved.get("state_json").and_then(|v| v.as_str()).unwrap_or("");
        let finished = serde_json::from_str::<Value>(state_json).ok().and_then(|s| s.get("finished").cloned()).is_some_and(|f| util::truthy(&f));
        if finished {
            let room_id = saved.get("room_id").and_then(|v| v.as_str()).unwrap_or("");
            if let Err(e) = app.db.finish_game(room_id, state_json, &[], "", false) {
                println!("[cleanup] Hiba a mentés lezárásnál (game #{}): {e}", saved.get("id").unwrap_or(&Value::Null));
            }
        }
    }
}

// --- Levelezős (aszinkron) játék ---

/// A mentett levelezős játékból szobát épít (minden játékos lecsatlakozottként). None, ha hibás.
pub fn load_async_room(app: &Arc<App>, st: &mut ServerState, row: &crate::db::Row) -> Option<String> {
    use crate::db::RowExt;
    let game_id = row.int("id");
    let state: Value = serde_json::from_str(&row.text("state_json")).ok()?;
    let mut game = match Game::from_save_dict(&state) {
        Ok(game) => game,
        Err(e) => {
            println!("[async] A mentett játék nem tölthető be (game #{game_id}): {e}");
            return None;
        }
    };
    game.async_mode = true;
    let room_id = row.text("room_id");
    let known: HashMap<String, Option<i64>> =
        app.db.get_game_players(game_id).unwrap_or_default().into_iter().map(|(name, uid, _)| (name, uid)).collect();
    for player in &mut game.players {
        if !player.is_bot {
            player.disconnected = true;
            if let Some(Some(uid)) = known.get(&player.name) {
                player.id = crate::async_games::placeholder_id(&room_id, *uid);
            }
        }
    }
    game.move_log = app
        .db
        .get_game_moves(game_id)
        .unwrap_or_default()
        .iter()
        .map(|m| crate::game::MoveLog::from_stored(m.move_number, &m.player_name, &m.action_type, m.details_json.as_deref(), m.board_snapshot_json.as_deref()))
        .collect();
    let owner_name = row.text("owner_name");
    let room_name = row.opt_text("room_name").filter(|n| !n.is_empty()).unwrap_or_else(|| "Levelezős játék".to_string());
    let max_players = game.players.len();
    let sig = (game.move_log.len(), game.finished, game.turn_number);
    let move_count = game.move_log.len();
    let mut room = Room::new(&room_id, game, None, &owner_name, &room_name, max_players, &st.generate_join_code(), true, None);
    room.is_async = true;
    room.db_game_id = Some(game_id);
    room.known_user_ids = known;
    room.last_saved_move_count = move_count;
    room.persisted_sig = Some(sig);
    st.add_room(room);
    Some(room_id)
}

/// A levelezős játék szobája: a memóriában lévő, ennek híján az adatbázisból visszaépített.
pub fn async_room_for_db_game(app: &Arc<App>, st: &mut ServerState, row: &crate::db::Row) -> Option<String> {
    use crate::db::RowExt;
    let game_id = row.int("id");
    if let Some(room) = st.rooms.values().find(|r| r.is_async && r.db_game_id == Some(game_id)) {
        return Some(room.id.clone());
    }
    load_async_room(app, st, row)
}

/// Szerverindításkor: a folyamatban lévő levelezős játékok szobáinak visszaépítése.
pub fn restore_async_games(app: &Arc<App>) -> usize {
    let mut st = app.state.lock();
    let rows = app.db.get_active_async_games().unwrap_or_default();
    rows.iter().filter(|row| async_room_for_db_game(app, &mut st, row).is_some()).count()
}

/// A befejezett levelezős játék szobája megszűnik, ha senki sem néz éppen.
pub fn cleanup_finished_async(app: &Arc<App>, st: &mut ServerState, room_id: &str) {
    let remove = st.rooms.get(room_id).is_some_and(|r| r.is_async && r.game.finished && !r.game.has_connected_human());
    if remove {
        cleanup_room(app, st, room_id);
    }
}

/// Lejárt határidejű körök: automatikus passz (három egymás utáni után a játékos feladja). Visszatér: a
/// léptetett játékok száma.
pub fn expire_async_turns(app: &Arc<App>, now: Option<f64>) -> usize {
    let now = now.unwrap_or_else(util::now);
    let mut st = app.state.lock();
    let mut expired = 0;
    let ids: Vec<String> = st.rooms.keys().cloned().collect();
    for room_id in ids {
        let Some(room) = st.rooms.get_mut(&room_id) else { continue };
        let game = &mut room.game;
        if !room.is_async || !game.started || game.finished {
            continue;
        }
        if game.turn_deadline.is_some_and(|d| now > d) && game.expire_turn() {
            expired += 1;
            emit_all_states(app, &mut st, &room_id);
            cleanup_finished_async(app, &mut st, &room_id);
        }
    }
    expired
}

/// A közlemények élő frissítése minden kliensnek: a bejelentkezetteknek és a vendégeknek szóló lista is megy, a
/// kliens a sajátját választja.
pub fn broadcast_announcements(app: &Arc<App>) {
    app.emit_all(
        "announcement",
        &json!({
            "registered": crate::admin::comm::active_announcements(app, true),
            "guests": crate::admin::comm::active_announcements(app, false),
        }),
    );
}
