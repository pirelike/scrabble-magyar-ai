//! Socket.IO eseménykezelők, 3. rész: napi feladvány, levelezős játék, megfigyelés, barátok és meghívók, admin
//! feliratkozás.

use super::core::*;
use super::events::*;
use crate::admin::live;
use crate::ai;
use crate::app::App;
use crate::async_games;
use crate::daily;
use crate::db::RowExt;
use crate::room::Room;
use crate::socket_auth::{TOKEN_MAX_AGE, verify_socket_token};
use crate::state::ServerState;
use crate::util;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::Arc;

// --- Napi feladvány ---

fn daily_started_payload(app: &Arc<App>, st: &ServerState, sid: &str, date: &str) -> Value {
    let user_id = registered_user_id(st, sid);
    json!({
        "date": date,
        "registered": user_id.is_some(),
        "my": user_id.and_then(|uid| app.db.get_daily_entry(date, uid)).map(|e| e.to_json()),
    })
}

/// Napi feladvány indítása: egyjátékos, nem listázott szoba a nap közös állásával.
pub async fn start_daily(app: Arc<App>, sid: String) {
    {
        let st = app.state.lock();
        if !app.limiter.check_socket(&sid, "start_daily") {
            app.emit_error(&sid, TOO_MANY);
            return;
        }
        if st.room_id_for_player(&sid).is_some() {
            app.emit_error(&sid, ALREADY_IN_ROOM);
            return;
        }
        if let Some(message) = feature_block(&app, &st, &sid, "feature_daily") {
            app.emit_error(&sid, message);
            return;
        }
    }
    let puzzle = tokio::task::spawn_blocking({
        let db = app.db.clone();
        move || daily::ensure_puzzle(&db, None, &ai::get_vocabulary())
    })
    .await;
    let puzzle = match puzzle {
        Ok(Ok(puzzle)) => puzzle,
        Ok(Err(e)) => {
            println!("[daily] Nem sikerült a feladvány: {e}");
            app.emit_error(&sid, "A napi feladvány most nem érhető el.");
            return;
        }
        Err(e) => {
            println!("[daily] Nem sikerült a feladvány: {e}");
            app.emit_error(&sid, "A napi feladvány most nem érhető el.");
            return;
        }
    };
    let mut st = app.state.lock();
    // A feladvány előállítása alatt egy második kérés már szobát nyithatott
    if st.room_id_for_player(&sid).is_some() {
        return;
    }
    let player_name = st.player_names.get(&sid).cloned().unwrap_or_else(|| "Névtelen".to_string());
    let room_id = new_id();
    let join_code = st.generate_join_code();
    let game = daily::build_game(&room_id, &sid, &player_name, &puzzle);
    let auth = st.player_auth.get(&sid).cloned();
    let token = st.generate_reconnect_token(&app.db, &sid, &room_id, &player_name, auth.as_ref());
    let mut room = Room::new(&room_id, game, Some(&sid), &player_name, "Napi feladvány", 1, &join_code, true, Some(&token));
    room.is_puzzle = true;
    st.add_room(room);
    st.player_rooms.insert(sid.clone(), room_id.clone());
    app.join_room(&sid, &room_id);

    app.emit_to(
        &sid,
        "room_joined",
        &json!({
            "room_id": room_id, "room_name": "Napi feladvány", "is_owner": true, "challenge_mode": false,
            "is_private": true, "turn_time_limit": 0, "hint_limit": 0,
            "reconnect_token": token, "chat_messages": [],
        }),
    );
    emit_all_states(&app, &mut st, &room_id);
    app.emit_to(&sid, "game_started", &json!({}));
    let payload = daily_started_payload(&app, &st, &sid, &puzzle.date);
    app.emit_to(&sid, "daily_started", &payload);
}

/// A beküldött lépés rögzítése (ranglista, kitüntetés) és az eredmény elküldése.
pub fn finish_puzzle(app: &Arc<App>, st: &mut ServerState, sid: &str, room_id: &str) {
    let (date, score, details) = {
        let game = &st.rooms[room_id].game;
        let Some(puzzle) = &game.puzzle else { return };
        let details = game.move_log.last().map(|m| m.details.clone()).unwrap_or(Value::Null);
        (puzzle.date.clone(), puzzle.score.unwrap_or(0), details)
    };
    let Some(puzzle) = app.db.get_daily_puzzle(&date) else { return };
    let user_id = registered_user_id(st, sid);
    let empty = json!([]);
    let result = daily::record_result(&app.db, &puzzle, user_id, score as i64, details.get("tiles").unwrap_or(&empty), details.get("words").unwrap_or(&empty));
    if let Some(uid) = user_id
        && result["recorded"] == json!(true)
        && result["is_best"] == json!(true)
    {
        let new = app.db.grant_achievements(uid, &HashSet::from(["daily_best".to_string()]), None);
        if !new.is_empty() {
            app.emit_to(sid, "achievements_earned", &json!({"badges": new}));
        }
    }
    app.emit_to(sid, "daily_result", &result);
}

/// Új próbálkozás ugyanarra a feladványra (a legjobb eredmény számít).
pub fn retry_daily(app: &Arc<App>, st: &mut ServerState, sid: &str, _data: Value) {
    let Some(room_id) = room_context(app, st, sid, "retry_daily") else { return };
    let date = {
        let game = &st.rooms[&room_id].game;
        match &game.puzzle {
            Some(puzzle) if game.finished => puzzle.date.clone(),
            _ => return,
        }
    };
    let Some(puzzle) = app.db.get_daily_puzzle(&date) else { return };
    daily::reset_game(&mut st.rooms.get_mut(&room_id).expect("létező szoba").game, &puzzle);
    emit_all_states(app, st, &room_id);
    let payload = daily_started_payload(app, st, sid, &puzzle.date);
    app.emit_to(sid, "daily_started", &payload);
}

/// A megoldás megmutatása: innentől a további próbálkozások nem kerülnek a ranglistára.
pub fn reveal_daily(app: &Arc<App>, st: &mut ServerState, sid: &str, _data: Value) {
    let Some(room_id) = room_context(app, st, sid, "reveal_daily") else { return };
    let Some(date) = st.rooms[&room_id].game.puzzle.as_ref().map(|p| p.date.clone()) else { return };
    let Some(puzzle) = app.db.get_daily_puzzle(&date) else { return };
    if let Some(uid) = registered_user_id(st, sid) {
        app.db.mark_daily_revealed(&puzzle.date, uid);
    }
    app.emit_to(sid, "daily_solution", &json!({"tiles": puzzle.best["tiles"], "words": puzzle.best["words"], "score": puzzle.best_score, "ranked": false}));
}

// --- Levelezős (aszinkron) játék ---

/// Levelezős játék indítása barátokkal: a barátok a Levelezős fülön látják, és értesítést kapnak.
pub fn create_async_game(app: &Arc<App>, st: &mut ServerState, sid: &str, data: Value) {
    if !app.limiter.check_socket(sid, "create_async_game") {
        app.emit_error(sid, TOO_MANY);
        return;
    }
    let Some(object) = data.as_object() else { return };
    let Some(user_id) = registered_user_id(st, sid) else {
        app.emit_error(sid, "Csak regisztrált felhasználók játszhatnak levelezős játékot.");
        return;
    };
    if st.room_id_for_player(sid).is_some() {
        app.emit_error(sid, ALREADY_IN_ROOM);
        return;
    }
    if let Some(message) = feature_block(app, st, sid, "feature_async") {
        app.emit_error(sid, message);
        return;
    }
    let ids: Option<Vec<i64>> = object.get("friend_ids").and_then(|v| v.as_array()).and_then(|list| list.iter().map(util::strict_int).collect());
    let unique = |ids: &[i64]| ids.iter().collect::<HashSet<_>>().len() == ids.len();
    let ids = match ids {
        Some(ids) if (1..=async_games::MAX_FRIENDS).contains(&ids.len()) && unique(&ids) => ids,
        _ => {
            app.emit_error(sid, "Válassz egy-három barátot a játékhoz.");
            return;
        }
    };
    let friends_rows = app.db.get_friends(user_id);
    let friend_name = |id: i64| friends_rows.iter().find(|f| f.int("id") == id).map(|f| f.text("display_name"));
    if ids.iter().any(|id| friend_name(*id).is_none()) {
        app.emit_error(sid, "Csak barátaidat hívhatod meg.");
        return;
    }
    let mut turn_hours = object.get("turn_hours").and_then(util::py_int).unwrap_or(async_games::DEFAULT_TURN_HOURS);
    if !async_games::ALLOWED_TURN_HOURS.contains(&turn_hours) {
        turn_hours = async_games::DEFAULT_TURN_HOURS;
    }
    let name = sanitize_room_name(object.get("name").unwrap_or(&json!(""))).unwrap_or_else(|| "Levelezős játék".to_string());
    let player_name = st.player_names.get(sid).cloned().unwrap_or_else(|| "Névtelen".to_string());
    let room_id = new_id();
    let friends: Vec<(i64, String)> =
        ids.iter().map(|id| (*id, sanitize_name_str(&friend_name(*id).unwrap_or_default()).unwrap_or_else(|| "Játékos".to_string()))).collect();
    let (mut game, known) = async_games::build_game(&room_id, sid, &player_name, user_id, &friends, turn_hours, &mut rand::rng());
    if let Some(first) = *app.async_starter.lock() {
        game.current_player_idx = first % game.players.len();
    }

    let auth = st.player_auth.get(sid).cloned();
    let token = st.generate_reconnect_token(&app.db, sid, &room_id, &player_name, auth.as_ref());
    let starter = game.current_player().map(|p| p.name.clone()).unwrap_or_default();
    let max_players = game.players.len();
    let mut room = Room::new(&room_id, game, Some(sid), &player_name, &name, max_players, &st.generate_join_code(), true, Some(&token));
    room.is_async = true;
    room.known_user_ids = known.iter().map(|(n, id)| (n.clone(), Some(*id))).collect();
    st.add_room(room);
    st.player_rooms.insert(sid.to_string(), room_id.clone());
    app.join_room(sid, &room_id);

    app.emit_to(
        sid,
        "room_joined",
        &json!({
            "room_id": room_id, "room_name": name, "is_owner": false, "challenge_mode": false,
            "is_private": true, "turn_time_limit": 0, "hint_limit": 0, "is_async": true,
            "reconnect_token": token, "chat_messages": [],
        }),
    );
    emit_all_states(app, st, &room_id); // elmenti a játékot, és értesíti a kezdő játékost, ha nem te vagy az
    app.emit_to(sid, "game_started", &json!({}));

    let db_game_id = st.rooms.get(&room_id).and_then(|r| r.db_game_id);
    for (friend_id, _) in &friends {
        for friend_sid in st.get_user_sids(*friend_id) {
            app.emit_to(&friend_sid, "async_invited", &json!({"game_id": db_game_id, "room_name": name, "from_name": player_name}));
        }
        if known.get(&starter) != Some(friend_id) {
            // a kezdő játékos a „Te jössz!” értesítést kapja
            app.push.notify_turn(*friend_id, &name, &room_id, "invite");
        }
    }
}

/// Egy folyamatban lévő levelezős játék megnyitása (a lobby listájából).
pub fn open_async_game(app: &Arc<App>, st: &mut ServerState, sid: &str, data: Value) {
    if !app.limiter.check_socket(sid, "open_async_game") {
        app.emit_error(sid, TOO_MANY);
        return;
    }
    let Some(object) = data.as_object() else { return };
    let Some(game_id) = object.get("game_id").and_then(util::strict_int) else {
        app.emit_error(sid, "Érvénytelen játék azonosító.");
        return;
    };
    let Some(user_id) = registered_user_id(st, sid) else {
        app.emit_error(sid, "Csak regisztrált felhasználók játszhatnak levelezős játékot.");
        return;
    };
    if st.room_id_for_player(sid).is_some() {
        app.emit_error(sid, ALREADY_IN_ROOM);
        return;
    }
    let row = app.db.get_game_by_id(game_id).ok().flatten().filter(|r| r.flag("is_async") && r.text("status") == "active");
    let Some(row) = row else {
        app.emit_error(sid, "Ez a levelezős játék nem található.");
        return;
    };
    if !app.db.is_user_in_game(game_id, user_id) {
        app.emit_error(sid, "Nem vagy részese ennek a játéknak.");
        return;
    }
    let Some(room_id) = async_room_for_db_game(app, st, &row) else {
        app.emit_error(sid, "Ez a levelezős játék nem tölthető be.");
        return;
    };
    let (player_id, player_name, player_disconnected) = {
        let room = &st.rooms[&room_id];
        let my_name = room.known_user_ids.iter().find(|(_, uid)| **uid == Some(user_id)).map(|(n, _)| n.clone());
        let player = room.game.players.iter().find(|p| Some(&p.name) == my_name.as_ref());
        let Some(player) = player else {
            app.emit_error(sid, "Nem vagy részese ennek a játéknak.");
            return;
        };
        (player.id.clone(), player.name.clone(), player.disconnected)
    };
    if !player_disconnected && st.player_rooms.get(&player_id) == Some(&room_id) {
        // Másik eszközön már meg van nyitva: onnan átvesszük
        app.leave_room(&player_id, &room_id);
        st.player_rooms.remove(&player_id);
        st.cleanup_player_token(&player_id);
        app.emit_to(&player_id, "room_left", &json!({}));
    }
    st.rooms.get_mut(&room_id).expect("létező szoba").game.replace_player_sid(&player_id, sid);
    st.player_rooms.insert(sid.to_string(), room_id.clone());
    app.join_room(sid, &room_id);
    let auth = st.player_auth.get(sid).cloned();
    let token = st.generate_reconnect_token(&app.db, sid, &room_id, &player_name, auth.as_ref());
    let (room_name, chat) = {
        let room = &st.rooms[&room_id];
        (room.name.clone(), room.chat_messages.clone())
    };
    app.emit_to(
        sid,
        "room_joined",
        &json!({
            "room_id": room_id, "room_name": room_name, "is_owner": false, "challenge_mode": false,
            "is_private": true, "turn_time_limit": 0, "hint_limit": 0, "is_async": true,
            "reconnect_token": token, "chat_messages": chat,
        }),
    );
    emit_all_states(app, st, &room_id);
    app.emit_to(sid, "game_started", &json!({}));
    app.emit_room(&room_id, "player_reconnected", &json!({"name": player_name}));
}

/// Levelezős játék feladása: a játék véget ér, a feladó nem lehet győztes.
pub fn resign_game(app: &Arc<App>, st: &mut ServerState, sid: &str, _data: Value) {
    let Some(room_id) = room_context(app, st, sid, "resign_game") else { return };
    if !st.rooms[&room_id].is_async {
        return;
    }
    let outcome = st.rooms.get_mut(&room_id).expect("létező szoba").game.resign(sid);
    let (success, message) = match outcome {
        Ok(message) => (true, message.to_string()),
        Err(message) => (false, message),
    };
    let mut payload = json!({"success": success, "message": message});
    if success {
        payload["own_turn"] = json!(true);
    }
    app.emit_to(sid, "action_result", &payload);
    if success {
        emit_all_states(app, st, &room_id); // elmenti a végeredményt
    }
}

// --- Megfigyelő mód ---

/// Folyamatban lévő játék megfigyelése játékosként való részvétel nélkül.
pub fn spectate_room(app: &Arc<App>, st: &mut ServerState, sid: &str, data: Value) {
    if !app.limiter.check_socket(sid, "spectate_room") {
        app.emit_error(sid, TOO_MANY);
        return;
    }
    let Some(object) = data.as_object() else { return };
    let spectated_exists = st.get_spectated_room(sid).is_some_and(|id| st.rooms.contains_key(&id));
    if st.room_id_for_player(sid).is_some() || spectated_exists {
        app.emit_error(sid, ALREADY_IN_ROOM);
        return;
    }
    let code = object.get("code").and_then(|c| c.as_str()).map(|c| c.trim().to_string()).unwrap_or_default();
    let mut by_code = false;
    let room_id: String;
    if !code.is_empty() {
        if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
            app.emit_error(sid, "Érvénytelen kód. 6 számjegyű kódot adj meg.");
            return;
        }
        match st.join_codes.get(&code) {
            Some(id) => room_id = id.clone(),
            None => {
                app.emit_error(sid, "Nincs ilyen kódú szoba.");
                return;
            }
        }
        by_code = true;
    } else {
        match object.get("room_id").and_then(|r| r.as_str()).filter(|r| !r.is_empty() && r.chars().count() <= 8) {
            Some(id) => room_id = id.to_string(),
            None => {
                app.emit_error(sid, "Szoba kód vagy azonosító szükséges.");
                return;
            }
        }
    }
    let max_spectators = app.settings.get_i64("max_spectators").max(0) as usize;
    let error = match st.rooms.get(&room_id) {
        None => Some("A szoba nem létezik."),
        Some(room) if room.is_private && !by_code => Some("Ez egy privát szoba. A megfigyeléshez kód szükséges."),
        Some(room) if !room.game.started || room.game.finished => Some("Csak folyamatban lévő játékot lehet megfigyelni."),
        Some(room) if room.spectators.len() >= max_spectators => Some("A szobának nem fér több megfigyelője."),
        Some(_) => None,
    };
    if let Some(message) = error {
        app.emit_error(sid, message);
        return;
    }
    app.join_room(sid, &room_id);
    let name = st.player_names.get(sid).cloned().unwrap_or_else(|| "Névtelen".to_string());
    st.rooms.get_mut(&room_id).expect("létező szoba").spectators.insert(sid.to_string(), name);
    st.add_spectator(sid, &room_id);
    let payload = {
        let room = &st.rooms[&room_id];
        json!({
            "room_id": room_id,
            "room_name": room.name,
            "challenge_mode": room.game.challenge_mode,
            "turn_time_limit": room.game.turn_time_limit,
            "hint_limit": room.game.hint_limit,
            "chat_messages": room.chat_messages,
        })
    };
    app.emit_to(sid, "spectate_joined", &payload);
    emit_all_states(app, st, &room_id);
    // Ha csak robotok vannak soron, a néző jelenléte újra elindítja a robotlépéseket
    schedule_bot_turn(app, st, &room_id);
}

/// Kilépés a megfigyelésből.
pub fn leave_spectate(app: &Arc<App>, st: &mut ServerState, sid: &str, _data: Value) {
    let Some(room_id) = st.remove_spectator(sid) else { return };
    if st.rooms.contains_key(&room_id) {
        app.leave_room(sid, &room_id);
        emit_all_states(app, st, &room_id);
    }
    app.emit_to(sid, "spectate_left", &json!({}));
}

// --- Barátok és meghívók ---

fn registered_or_none(st: &ServerState, sid: &str) -> Option<i64> {
    registered_user_id(st, sid)
}

pub fn send_friend_request(app: &Arc<App>, st: &mut ServerState, sid: &str, data: Value) {
    if !app.limiter.check_socket(sid, "send_friend_request") {
        return;
    }
    let Some(object) = data.as_object() else { return };
    let Some(user_id) = registered_or_none(st, sid) else {
        app.emit_to(sid, "friend_request_result", &json!({"success": false, "message": "Csak regisztrált felhasználók küldhetnek barátkérést."}));
        return;
    };
    let Some(friend_id) = object.get("friend_id").and_then(util::strict_int) else { return };
    let (success, message) = app.db.send_friend_request(user_id, friend_id);
    app.emit_to(sid, "friend_request_result", &json!({"success": success, "message": message}));
    if success {
        // Értesítés a fogadónak, ha online
        let sender_name = st.player_names.get(sid).cloned().unwrap_or_else(|| "Ismeretlen".to_string());
        for fsid in st.get_user_sids(friend_id) {
            app.emit_to(&fsid, "friend_request_received", &json!({"from_user_id": user_id, "from_name": sender_name}));
        }
    }
}

pub fn accept_friend_request(app: &Arc<App>, st: &mut ServerState, sid: &str, data: Value) {
    if !app.limiter.check_socket(sid, "accept_friend_request") {
        return;
    }
    let Some(object) = data.as_object() else { return };
    let Some(user_id) = registered_or_none(st, sid) else { return };
    let Some(requester_id) = object.get("requester_id").and_then(util::strict_int) else { return };
    let (success, message) = app.db.accept_friend_request(user_id, requester_id);
    app.emit_to(sid, "friend_request_result", &json!({"success": success, "message": message}));
    if success {
        let user_name = st.player_names.get(sid).cloned().unwrap_or_else(|| "Ismeretlen".to_string());
        for rsid in st.get_user_sids(requester_id) {
            app.emit_to(&rsid, "friend_request_accepted", &json!({"user_id": user_id, "display_name": user_name}));
        }
    }
}

pub fn decline_friend_request(app: &Arc<App>, st: &mut ServerState, sid: &str, data: Value) {
    if !app.limiter.check_socket(sid, "decline_friend_request") {
        return;
    }
    let Some(object) = data.as_object() else { return };
    let Some(user_id) = registered_or_none(st, sid) else { return };
    let Some(requester_id) = object.get("requester_id").and_then(util::strict_int) else { return };
    let (success, message) = app.db.decline_friend_request(user_id, requester_id);
    app.emit_to(sid, "friend_request_result", &json!({"success": success, "message": message}));
}

pub fn remove_friend(app: &Arc<App>, st: &mut ServerState, sid: &str, data: Value) {
    if !app.limiter.check_socket(sid, "remove_friend") {
        return;
    }
    let Some(object) = data.as_object() else { return };
    let Some(user_id) = registered_or_none(st, sid) else { return };
    let Some(friend_id) = object.get("friend_id").and_then(util::strict_int) else { return };
    let (success, message) = app.db.remove_friend(user_id, friend_id);
    app.emit_to(sid, "friend_request_result", &json!({"success": success, "message": message}));
}

pub fn invite_to_room(app: &Arc<App>, st: &mut ServerState, sid: &str, data: Value) {
    if !app.limiter.check_socket(sid, "invite_to_room") {
        return;
    }
    let Some(object) = data.as_object() else { return };
    let sent = |success: bool, message: &str| app.emit_to(sid, "invite_sent", &json!({"success": success, "message": message}));
    let Some(user_id) = registered_or_none(st, sid) else {
        sent(false, "Csak regisztrált felhasználók hívhatnak meg barátokat.");
        return;
    };
    let Some(friend_id) = object.get("friend_id").and_then(util::strict_int) else { return };
    if !app.db.get_friends(user_id).iter().any(|f| f.int("id") == friend_id) {
        sent(false, "Csak barátaidat hívhatod meg.");
        return;
    }
    let Some(room_id) = st.room_id_for_player(sid) else {
        sent(false, "Nem vagy szobában.");
        return;
    };
    let (started, full, room_name, join_code) = {
        let room = &st.rooms[&room_id];
        (room.game.started, room.game.players.len() >= room.max_players, room.name.clone(), room.join_code.clone())
    };
    if started {
        sent(false, "A játék már elkezdődött.");
        return;
    }
    if full {
        sent(false, "A szoba megtelt.");
        return;
    }
    if !st.is_user_online(friend_id) {
        sent(false, "A barátod jelenleg nincs online.");
        return;
    }
    // Ellenőrzés: a barát már szobában van-e
    let friend_sids = st.get_user_sids(friend_id);
    if friend_sids.iter().any(|fsid| st.player_rooms.contains_key(fsid)) {
        sent(false, "A barátod már egy másik szobában van.");
        return;
    }
    let invite_id = st.create_invite(user_id, friend_id, &room_id, sid);
    let sender_name = st.player_names.get(sid).cloned().unwrap_or_else(|| "Ismeretlen".to_string());
    for fsid in friend_sids {
        app.emit_to(
            &fsid,
            "game_invite",
            &json!({"invite_id": invite_id, "from_name": sender_name, "room_name": room_name, "room_id": room_id, "join_code": join_code}),
        );
    }
    sent(true, "Meghívó elküldve!");
}

pub fn respond_invite(app: &Arc<App>, st: &mut ServerState, sid: &str, data: Value) {
    if !app.limiter.check_socket(sid, "respond_invite") {
        return;
    }
    let Some(object) = data.as_object() else { return };
    let Some(user_id) = st.player_auth.get(sid).and_then(|a| a.user_id).filter(|u| *u != 0) else { return };
    let accept = object.get("accept").is_some_and(util::truthy);
    let Some(invite_id) = object.get("invite_id").and_then(util::strict_int) else { return };
    let Some(invite) = st.get_invite(invite_id).filter(|i| i.to_user_id == user_id).cloned() else {
        app.emit_error(sid, "Meghívó nem található vagy lejárt.");
        return;
    };
    st.remove_invite(invite_id);
    if accept {
        let Some(room) = st.rooms.get(&invite.room_id) else {
            app.emit_error(sid, "A szoba már nem létezik.");
            return;
        };
        app.emit_to(sid, "invite_accepted", &json!({"room_id": invite.room_id, "join_code": room.join_code}));
    } else {
        app.emit_to(sid, "invite_declined", &json!({"success": true}));
    }
}

// --- Admin (Socket.IO) ---

/// Az admin felhasználó azonosítója a kapcsolathoz, vagy None. Vagy bejelentkezett játékos kapcsolata
/// (`set_name`), vagy külön aláírt token (az admin panel oldala nem regisztrálja magát online játékosként).
fn admin_user_id_for_sid(app: &Arc<App>, st: &mut ServerState, sid: &str, data: &Value) -> Option<i64> {
    if let Some(uid) = st.get_user_id_for_sid(sid)
        && app.db.get_user_by_id(uid).ok().flatten().is_some_and(|u| app.is_admin_email(&u.email))
    {
        return Some(uid);
    }
    let token = data.get("auth_token").and_then(|t| t.as_str());
    let verified = verify_socket_token(&app.config.secret_key, token, TOKEN_MAX_AGE)?;
    let user = app.db.get_user_by_id(verified).ok().flatten()?;
    if app.is_admin_email(&user.email) && user.ban().is_none() && user.deleted_at.is_none() {
        st.admin_sids.insert(sid.to_string(), user.id);
        return Some(user.id);
    }
    None
}

/// Belépés az `admin` szobába (élő admin események). Nem adminnak csendben nem történik semmi: sem hibaüzenet,
/// sem visszaigazolás, hogy ne derüljön ki, hogy ilyen esemény létezik.
pub fn admin_subscribe(app: &Arc<App>, st: &mut ServerState, sid: &str, data: Value) {
    if !app.limiter.check_socket(sid, "admin_subscribe") {
        return;
    }
    if admin_user_id_for_sid(app, st, sid, &data).is_none() {
        return;
    }
    app.join_room(sid, ADMIN_ROOM);
    app.emit_to(sid, "admin_subscribed", &json!({}));
    app.emit_to(sid, "admin_overview", &live::overview_light(app, st));
}

/// Egy élő szoba figyelése az admin panelről: a szoba minden változásáról `admin_room_state` érkezik. Nem megfigyelő
/// a játék szempontjából. Nem adminnak csendben semmi.
pub fn admin_watch_room(app: &Arc<App>, st: &mut ServerState, sid: &str, data: Value) {
    if !app.limiter.check_socket(sid, "admin_watch_room") || !is_admin_sid(app, st, sid) {
        return;
    }
    let key = data.get("room_id").and_then(|k| k.as_str()).map(|k| k.to_string());
    for room in st.rooms.values_mut() {
        room.admin_watchers.remove(sid); // egyszerre egy szobát figyel
    }
    let Some(key) = key else { return };
    let room_id = if st.rooms.contains_key(&key) { Some(key.clone()) } else { st.join_codes.get(&key).cloned() };
    let Some(room_id) = room_id.filter(|id| st.rooms.contains_key(id)) else { return };
    st.rooms.get_mut(&room_id).expect("létező szoba").admin_watchers.insert(sid.to_string());
    let detail = live::room_detail(app, st, &room_id);
    app.emit_to(sid, "admin_room_state", &detail);
}
