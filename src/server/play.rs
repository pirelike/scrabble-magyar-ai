//! Socket.IO eseménykezelők, 2. rész: játékmenet (lerakás, csere, passz, szavazás, visszavonás), chat, bejelentés,
//! előnézet, tipp, láthatóság.

use super::core::*;
use super::events::*;
use crate::admin::moderation;
use crate::ai;
use crate::app::App;
use crate::state::ServerState;
use crate::util;
use serde_json::{Value, json};
use std::sync::Arc;

pub fn place_tiles(app: &Arc<App>, st: &mut ServerState, sid: &str, data: Value) {
    let Some(room_id) = room_context(app, st, sid, "place_tiles") else { return };
    let Some(object) = data.as_object() else { return };
    let tiles = match validate_tiles_input(object.get("tiles").unwrap_or(&json!([]))) {
        Ok(tiles) => tiles,
        Err(message) => {
            action_result(app, sid, false, &message);
            return;
        }
    };
    let (outcome, timer_expires_at) = {
        let room = st.rooms.get_mut(&room_id).expect("létező szoba");
        let expires = room.turn_timer_expires_at;
        (room.game.place_tiles(sid, &tiles), expires)
    };
    let (message, score) = match outcome {
        Ok(ok) => ok,
        Err(message) => {
            action_result(app, sid, false, &message);
            return;
        }
    };
    let (pending, finished, puzzle) = {
        let room = st.rooms.get_mut(&room_id).expect("létező szoba");
        room.invalidate_turn_timer();
        (room.game.pending_challenge.is_some(), room.game.finished, room.game.puzzle.is_some())
    };
    app.emit_to(sid, "action_result", &json!({"success": true, "message": message, "score": score, "own_turn": true}));
    if pending {
        // Ha a lerakó visszavonja a lerakását, csak a maradék idejét kapja vissza
        if let Some(room) = st.rooms.get_mut(&room_id) {
            room.withdraw_time_left = timer_expires_at.map(|t| (t - util::now()).max(0.0));
        }
        start_challenge_timer(app, st, &room_id);
    } else if finished && puzzle {
        super::extras::finish_puzzle(app, st, sid, &room_id);
    } else if finished {
        save_game_to_db(app, st, &room_id);
    } else {
        start_turn_timer(app, st, &room_id, None);
    }
    emit_all_states(app, st, &room_id);
    schedule_bot_turn(app, st, &room_id);
}

pub fn exchange_tiles(app: &Arc<App>, st: &mut ServerState, sid: &str, data: Value) {
    let Some(room_id) = room_context(app, st, sid, "exchange_tiles") else { return };
    let Some(object) = data.as_object() else { return };
    let empty = json!([]);
    let Some(list) = object.get("indices").unwrap_or(&empty).as_array().filter(|l| l.len() <= 7) else {
        action_result(app, sid, false, "Érvénytelen csere.");
        return;
    };
    let indices: Option<Vec<i64>> = list.iter().map(util::py_int).collect();
    let Some(indices) = indices else {
        action_result(app, sid, false, "Érvénytelen index.");
        return;
    };
    let outcome = st.rooms.get_mut(&room_id).expect("létező szoba").game.exchange_tiles(sid, &indices);
    finish_turn_action(app, st, sid, &room_id, outcome);
}

pub fn pass_turn(app: &Arc<App>, st: &mut ServerState, sid: &str, _data: Value) {
    let Some(room_id) = room_context(app, st, sid, "pass_turn") else { return };
    let outcome = st.rooms.get_mut(&room_id).expect("létező szoba").game.pass_turn(sid, false).map(|m| m.to_string());
    finish_turn_action(app, st, sid, &room_id, outcome);
}

/// A csere / passz közös folytatása: időzítők, mentés, állapotküldés, robot.
fn finish_turn_action(app: &Arc<App>, st: &mut ServerState, sid: &str, room_id: &str, outcome: Result<String, String>) {
    match outcome {
        Ok(message) => {
            let finished = {
                let room = st.rooms.get_mut(room_id).expect("létező szoba");
                room.invalidate_turn_timer();
                room.game.finished
            };
            app.emit_to(sid, "action_result", &json!({"success": true, "message": message, "own_turn": true}));
            if finished {
                save_game_to_db(app, st, room_id);
            } else {
                start_turn_timer(app, st, room_id, None);
            }
            emit_all_states(app, st, room_id);
            schedule_bot_turn(app, st, room_id);
        }
        Err(message) => action_result(app, sid, false, &message),
    }
}

pub fn accept_words(app: &Arc<App>, st: &mut ServerState, sid: &str, _data: Value) {
    vote(app, st, sid, "accept_words", true);
}

pub fn reject_words(app: &Arc<App>, st: &mut ServerState, sid: &str, _data: Value) {
    vote(app, st, sid, "reject_words", false);
}

fn vote(app: &Arc<App>, st: &mut ServerState, sid: &str, event: &str, accept: bool) {
    let Some(room_id) = room_context(app, st, sid, event) else { return };
    let game = &mut st.rooms.get_mut(&room_id).expect("létező szoba").game;
    let outcome = if accept { game.accept_pending_by_player(sid) } else { game.reject_pending_by_player(sid) };
    match outcome {
        Ok((result, message)) => handle_challenge_result(app, st, &room_id, result, &message),
        Err(message) => action_result(app, sid, false, &message),
    }
}

/// A lerakó visszavonja a szavazásra váró lerakását (amíg senki sem szavazott).
pub fn withdraw_words(app: &Arc<App>, st: &mut ServerState, sid: &str, _data: Value) {
    let Some(room_id) = room_context(app, st, sid, "withdraw_words") else { return };
    let outcome = st.rooms.get_mut(&room_id).expect("létező szoba").game.withdraw_pending(sid);
    match outcome {
        Ok(message) => {
            let left = {
                let room = st.rooms.get_mut(&room_id).expect("létező szoba");
                room.invalidate_challenge_timer();
                room.withdraw_time_left
            };
            // A lerakás + visszavonás ne adjon új, teljes kört: a lerakáskor hátralévő idő folytatódik
            start_turn_timer(app, st, &room_id, left.map(|l| l.max(MIN_TIME_AFTER_WITHDRAW)));
            app.emit_to(sid, "action_result", &json!({"success": true, "message": message, "own_turn": true}));
            emit_all_states(app, st, &room_id);
        }
        Err(message) => action_result(app, sid, false, &message),
    }
}

pub fn send_chat(app: &Arc<App>, st: &mut ServerState, sid: &str, data: Value) {
    let Some(room_id) = room_context(app, st, sid, "send_chat") else { return };
    let Some(object) = data.as_object() else { return };
    let Some(message) = object.get("message").and_then(|m| m.as_str()) else { return };
    let message = message.trim();
    if message.is_empty() || message.chars().count() as i64 > app.settings.get_i64("chat_max_length") {
        return;
    }
    let user_id = registered_user_id(st, sid);
    if user_id.is_some_and(|uid| app.db.is_chat_muted(uid)) {
        return; // a némított felhasználó üzenete csendben eldobódik
    }
    let drop = app.settings.get_string("banned_word_action") == "drop";
    let (message, flagged) = moderation::filter_chat(&app.banned_word_list(), drop, message);
    let Some(message) = message else { return }; // tiltott szó, „eldobás” beállítással
    let player_name = st.player_names.get(sid).cloned().unwrap_or_else(|| "?".to_string());
    let room = st.rooms.get_mut(&room_id).expect("létező szoba");
    room.add_chat_message(&player_name, &message, user_id, false);
    moderation::log_chat(&app.db, app.settings.get_bool("chat_log_enabled"), room, user_id, &player_name, &message);
    let room_name = room.name.clone();
    app.emit_room(&room_id, "chat_message", &json!({"name": player_name, "message": message, "sid": sid}));
    if flagged && app.room_member_count(ADMIN_ROOM) > 0 {
        app.emit_room(ADMIN_ROOM, "admin_alert", &json!({"code": "banned_word", "room": room_name, "name": player_name}));
    }
}

/// Egy játékos vagy chat üzenet bejelentése a moderációnak (csak regisztrált, a szobában lévő játékos).
pub fn report_content(app: &Arc<App>, st: &mut ServerState, sid: &str, data: Value) {
    let report = |success: bool, message: &str| app.emit_to(sid, "report_result", &json!({"success": success, "message": message}));
    if !app.limiter.check_socket(sid, "report_content") {
        report(false, "Túl sok kérés, várj egy kicsit.");
        return;
    }
    let Some(object) = data.as_object() else { return };
    let room_id = st.room_id_for_player(sid);
    let reporter = registered_user_id(st, sid);
    let Some(room_id) = room_id else {
        report(false, "Nem vagy szobában.");
        return;
    };
    let Some(reporter) = reporter else {
        report(false, "Bejelentést csak regisztrált felhasználó tehet.");
        return;
    };
    let room = &st.rooms[&room_id];
    let wanted = object.get("name").and_then(|n| n.as_str());
    let target = room.game.players.iter().find(|p| !p.is_bot && Some(p.name.as_str()) == wanted);
    let Some(target) = target.filter(|t| t.id != sid) else {
        report(false, "Érvénytelen bejelentés.");
        return;
    };
    let target_user = user_id_for_player(room, target, &st.player_auth);
    let reporter_name = st.player_names.get(sid).cloned().unwrap_or_else(|| "?".to_string());
    let null = Value::Null;
    let report_id = moderation::create_report(
        &app.db,
        reporter,
        &reporter_name,
        target_user,
        &target.name,
        object.get("kind").unwrap_or(&null),
        object.get("message").unwrap_or(&null),
        object.get("reason").unwrap_or(&null),
        room,
    );
    let Some(report_id) = report_id else {
        report(false, "Érvénytelen bejelentés.");
        return;
    };
    report(true, "Bejelentés elküldve. Köszönjük!");
    if app.room_member_count(ADMIN_ROOM) > 0 {
        app.emit_room(ADMIN_ROOM, "admin_report", &json!({"id": report_id, "room": room.name, "reported": target.name}));
    }
    moderation::notify_admins_of_report(app, &room.name);
}

/// A jelenlegi lerakás kipróbálása véglegesítés nélkül: szavak és pontszám (élő előnézet).
pub fn preview_move(app: &Arc<App>, st: &mut ServerState, sid: &str, data: Value) {
    if !app.limiter.check_socket(sid, "preview_move") {
        return; // az előnézet nem kritikus, csendben eldobjuk
    }
    let Some(room_id) = st.room_id_for_player(sid) else { return };
    let Some(object) = data.as_object() else { return };
    match validate_tiles_input(object.get("tiles").unwrap_or(&json!([]))) {
        Ok(tiles) => {
            let preview = st.rooms[&room_id].game.preview_placement(sid, &tiles);
            app.emit_to(sid, "move_preview", &preview);
        }
        Err(message) => app.emit_to(sid, "move_preview", &json!({"valid": false, "score": 0, "words": [], "message": message})),
    }
}

/// A kliens jelzi, ha a lapja háttérbe került: ilyenkor a saját körről push értesítést kap.
pub fn set_visibility(app: &Arc<App>, st: &mut ServerState, sid: &str, data: Value) {
    if !app.limiter.check_socket(sid, "set_visibility") {
        return;
    }
    let Some(object) = data.as_object() else { return };
    if object.get("hidden") == Some(&Value::Bool(true)) {
        st.hidden_sids.insert(sid.to_string());
        if let Some(room_id) = st.room_id_for_player(sid) {
            maybe_push_turn(app, st, &room_id); // már az ő köre van: azonnal értesítjük
        }
    } else {
        st.hidden_sids.remove(sid);
    }
}

/// Tipp kérése: a legjobb lépések. Csak egyedül (robotok ellen) játszva elérhető.
pub async fn request_hint(app: Arc<App>, sid: String) {
    struct Snapshot {
        room_id: String,
        board: crate::board::Board,
        hand: Vec<crate::tiles::Tile>,
        turn_number: i64,
    }
    let hint = |success: bool, message: &str, extra: Option<i64>| {
        let mut payload = json!({"success": success, "message": message});
        if let Some(left) = extra {
            payload["hints_left"] = json!(left);
        }
        app.emit_to(&sid, "hint_result", &payload);
    };
    if !app.limiter.check_socket(&sid, "request_hint") {
        hint(false, "Túl sok kérés, várj egy kicsit.", None);
        return;
    }
    let snapshot = {
        let st = app.state.lock();
        let Some(room_id) = st.room_id_for_player(&sid) else { return };
        let game = &st.rooms[&room_id].game;
        if game.hint_limit == 0 {
            hint(false, "A tippek ki vannak kapcsolva ebben a szobában.", None);
            return;
        }
        if !game.started || game.finished || game.pending_challenge.is_some() {
            hint(false, "Most nem kérhetsz tippet.", None);
            return;
        }
        if game.hints_left() <= 0 {
            hint(false, "Elfogytak a tippjeid.", Some(0));
            return;
        }
        if game.human_players().len() != 1 {
            hint(false, "Tippet csak egyedül, robotok ellen játszva kérhetsz.", None);
            return;
        }
        let Some(player) = game.current_player().filter(|p| p.id == sid) else {
            hint(false, "Nem te következel.", None);
            return;
        };
        Snapshot { room_id, board: game.board.clone(), hand: player.hand.clone(), turn_number: game.turn_number }
    };

    let moves = tokio::task::spawn_blocking({
        let (board, hand) = (snapshot.board.clone(), snapshot.hand.clone());
        move || {
            let vocab = ai::get_vocabulary();
            ai::best_moves(&board, &hand, 3, &vocab, None)
        }
    })
    .await;
    let moves = match moves {
        Ok(moves) => moves,
        Err(e) => {
            // pl. hiányzó szótárfájl: a játék ettől még folytatható
            println!("[hint] Hiba a tipp keresésében ({}): {e}", snapshot.room_id);
            hint(false, "Nem sikerült tippet adni.", None);
            return;
        }
    };

    let mut st = app.state.lock();
    // A keresés közben elfogyhatott a tipp vagy véget érhetett a kör
    let Some(room) = st.rooms.get_mut(&snapshot.room_id) else { return };
    let game = &mut room.game;
    if game.finished || game.turn_number != snapshot.turn_number || game.current_player().is_none_or(|p| p.id != sid) {
        hint(false, "Most nem kérhetsz tippet.", None);
        return;
    }
    if !moves.is_empty() && !game.use_hint() {
        hint(false, "Elfogytak a tippjeid.", Some(0));
        return;
    }
    let hints_left = game.hints_left();
    if !moves.is_empty() {
        emit_all_states(&app, &mut st, &snapshot.room_id); // a hátralévő tippek száma frissül
    }
    app.emit_to(
        &sid,
        "hint_result",
        &json!({
            "success": true,
            "message": "",
            "hints_left": hints_left,
            "moves": moves.iter().map(|m| json!({"tiles": m.tiles_json(), "words": m.words, "score": m.score})).collect::<Vec<_>>(),
        }),
    );
}
