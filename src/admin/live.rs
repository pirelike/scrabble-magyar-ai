//! Admin panel: a futó szerver élő állapota — áttekintés, riasztások, szobák és beavatkozások.
//!
//! A modul a `server::core` függvényein keresztül avatkozik be (`emit_all_states`, `schedule_bot_turn`,
//! `room.invalidate_*`, `save_game_to_db`...), sosem írja át közvetlenül a játékállapotot (kivétel az átnevezés).
//! A függvények a hívó által zárolt `ServerState`-en dolgoznak.

use super::*;
use crate::app::App;
use crate::db::{RowExt, fetch_all, fetch_one};
use crate::room::Room;
use crate::server::core;
use crate::state::ServerState;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// ennyi mozdulatlanság után jelzünk időkorlát nélküli játékot
pub const STUCK_IDLE_SECONDS: f64 = 1800.0;
/// a soron lévő robot ennyi ideje nem lépett, és nincs ütemezett lépése
pub const STUCK_BOT_SECONDS: f64 = 90.0;
/// a lejárt időzítő / szavazás ennyi késés után jelez
pub const STUCK_OVERDUE_SECONDS: f64 = 60.0;
pub const ROOM_ACTIONS: [&str; 13] =
    ["message", "kick", "transfer", "skip", "extend", "pause", "resume", "resolve_vote", "reschedule_bot", "save", "end", "void", "disband"];
pub const EXTEND_CHOICES: [i64; 4] = [30, 60, 120, 300];
pub const MAX_SYSTEM_MESSAGE: usize = 200;
pub const ADMIN_ALERT_WINDOW: i64 = 15 * 60;

// ===== Online felhasználók =====

pub fn online_user_ids(st: &ServerState) -> HashSet<i64> {
    st.get_online_user_ids()
}

/// Hol van most a felhasználó: online-e, hány kapcsolata van, melyik szobákban.
pub fn user_live(st: &ServerState, user_id: i64) -> Value {
    let sids: HashSet<String> = st.get_user_sids(user_id).into_iter().collect();
    let mut rooms = Vec::new();
    for (room_id, room) in &st.rooms {
        let mut role = None;
        for sid in &sids {
            if st.player_rooms.get(sid) == Some(room_id) {
                role = Some("player");
            } else if st.spectator_rooms.get(sid) == Some(room_id) {
                role = Some("spectator");
            }
        }
        if role.is_none() && room.is_async && room.known_user_ids.values().any(|uid| *uid == Some(user_id)) {
            role = Some("async_player");
        }
        if let Some(role) = role {
            rooms.push(json!({"room_id": room_id, "code": room.join_code, "name": room.name, "role": role, "since": room.created_at}));
        }
    }
    json!({"online": !sids.is_empty(), "connections": sids.len(), "rooms": rooms})
}

/// A felhasználó összes élő kapcsolatának bontása (kitiltás, kijelentkeztetés): előbb értesítés, aztán bontás. A
/// játékban lévő játékos a szokásos módon `disconnected` lesz (a disconnect esemény kezeli). A türelmi időben lévő
/// (már lecsatlakozott) kapcsolatainak újracsatlakozó tokenjét is érvényteleníti.
pub fn kick_user(app: &Arc<App>, st: &mut ServerState, user_id: i64, event: &str, payload: Option<&Value>) -> usize {
    let stale: Vec<String> = st
        .reconnect_tokens
        .iter()
        .filter(|(token, info)| {
            info.auth_info.as_ref().is_some_and(|a| a.user_id == Some(user_id) && !a.is_guest) && st.disconnected_players.contains_key(*token)
        })
        .map(|(token, _)| token.clone())
        .collect();
    for token in stale {
        st.disconnected_players.remove(&token);
        st.reconnect_tokens.remove(&token);
    }
    let sids = st.get_user_sids(user_id);
    for sid in &sids {
        if let Some(payload) = payload {
            app.emit_to(sid, event, payload);
        }
        app.disconnect_sid(sid); // a kapcsolat közben megszűnhetett
    }
    sids.len()
}

/// Minden online felhasználó kapcsolatának bontása (tömeges kijelentkeztetés), a kivételek nélkül.
pub fn kick_all(app: &Arc<App>, st: &mut ServerState, except: &HashSet<i64>) -> usize {
    let mut total = 0;
    for user_id in st.get_online_user_ids() {
        if !except.contains(&user_id) {
            total += kick_user(app, st, user_id, "account_banned", None);
        }
    }
    total
}

fn players_of_user(room: &Room, st: &ServerState, user_id: i64) -> Vec<usize> {
    room.game
        .players
        .iter()
        .enumerate()
        .filter(|(_, p)| !p.is_bot && core::user_id_for_player(room, p, &st.player_auth) == Some(user_id))
        .map(|(i, _)| i)
        .collect()
}

/// Azok a futó szobák, ahol az új név ütközne egy másik játékos nevével.
pub fn rename_conflicts(st: &ServerState, user_id: i64, new_name: &str) -> Vec<String> {
    let mut clash = Vec::new();
    for room in st.rooms.values() {
        let mine = players_of_user(room, st, user_id);
        if !mine.is_empty() && room.game.players.iter().enumerate().any(|(i, p)| util::casefold(&p.name) == util::casefold(new_name) && !mine.contains(&i)) {
            clash.push(room.name.clone());
        }
    }
    clash
}

/// A felhasználó nevének frissítése a futó szobákban (játékos, lépésnapló, ismert nevek, tulajdonos). Visszatér: az
/// érintett szobák azonosítói.
pub fn rename_live(app: &Arc<App>, st: &mut ServerState, user_id: i64, old: &str, new: &str) -> Vec<String> {
    let mut changed: Vec<String> = Vec::new();
    let room_ids: Vec<String> = st.rooms.keys().cloned().collect();
    for room_id in room_ids {
        let mine = {
            let room = &st.rooms[&room_id];
            players_of_user(room, st, user_id)
        };
        if mine.is_empty() {
            continue;
        }
        let mut renamed_ids: Vec<String> = Vec::new();
        {
            let room = st.rooms.get_mut(&room_id).expect("létező szoba");
            for index in mine {
                let player = &mut room.game.players[index];
                if player.name != old {
                    continue;
                }
                renamed_ids.push(player.id.clone());
                player.name = new.to_string();
            }
            if let Some(known) = room.known_user_ids.remove(old) {
                room.known_user_ids.insert(new.to_string(), known);
            }
            if room.owner_name == old {
                room.owner_name = new.to_string();
            }
            for mv in &mut room.game.move_log {
                if mv.player_name == old {
                    mv.player_name = new.to_string();
                }
            }
            if let Some(identity) = room.late_join_names.remove(old) {
                room.late_join_names.insert(new.to_string(), identity);
            }
            if let Some(expected) = room.expected_players.as_mut() {
                for name in expected.iter_mut() {
                    if name == old {
                        *name = new.to_string();
                    }
                }
            }
        }
        for id in renamed_ids {
            if st.player_names.contains_key(&id) {
                st.player_names.insert(id, new.to_string());
            }
        }
        changed.push(room_id);
    }
    for info in st.reconnect_tokens.values_mut() {
        if info.player_name == old && info.auth_info.as_ref().is_some_and(|a| a.user_id == Some(user_id)) {
            info.player_name = new.to_string();
        }
    }
    for info in st.disconnected_players.values_mut() {
        if info.player_name == old && info.auth_info.as_ref().is_some_and(|a| a.user_id == Some(user_id)) {
            info.player_name = new.to_string();
        }
    }
    for room_id in &changed {
        core::emit_all_states(app, st, room_id);
    }
    changed
}

// ===== Szobák: összegzés =====

pub fn room_kind(room: &Room) -> &'static str {
    if room.is_puzzle {
        "puzzle"
    } else if room.is_async {
        "async"
    } else if room.is_restored {
        "restore"
    } else {
        "game"
    }
}

pub fn room_status(room: &Room) -> &'static str {
    let game = &room.game;
    if game.finished {
        "finished"
    } else if !game.started {
        "waiting"
    } else if game.pending_challenge.is_some() {
        "voting"
    } else {
        "playing"
    }
}

/// Miért tűnik elakadtnak a játék (vagy None): bot_idle / vote_overdue / timer_overdue / no_timer / idle.
pub fn stuck_reason(room: &Room, now: f64) -> Option<&'static str> {
    let game = &room.game;
    if !game.started || game.finished || room.is_puzzle {
        return None;
    }
    if let Some(pending) = &game.pending_challenge {
        return if now > pending.expires_at + STUCK_OVERDUE_SECONDS { Some("vote_overdue") } else { None };
    }
    let current = game.current_player()?;
    if current.is_bot {
        if core::bot_should_move(room) && room.bot_scheduled_at.is_none_or(|s| now - s > STUCK_BOT_SECONDS) {
            return Some("bot_idle");
        }
        return None;
    }
    if game.turn_time_limit != 0 && !room.is_async && room.timer_paused_left.is_none() {
        return match room.turn_timer_expires_at {
            None => (now - room.last_activity > STUCK_OVERDUE_SECONDS).then_some("no_timer"),
            Some(expires) => (now > expires + STUCK_OVERDUE_SECONDS).then_some("timer_overdue"),
        };
    }
    if !room.is_async && game.turn_time_limit == 0 && now - room.last_activity > STUCK_IDLE_SECONDS && game.has_connected_human() {
        return Some("idle");
    }
    None
}

fn player_row(st: &ServerState, room: &Room, player: &crate::player::Player) -> Value {
    json!({
        "name": player.name, "is_bot": player.is_bot, "difficulty": player.difficulty.map(|d| d.to_json()),
        "score": player.score, "online": player.is_bot || !player.disconnected, "hand_count": player.hand.len(),
        "user_id": if player.is_bot { Value::Null } else { json!(core::user_id_for_player(room, player, &st.player_auth)) },
        "sid": if player.is_bot { Value::Null } else { json!(player.id) }, "resigned": player.resigned,
        "owner": !player.is_bot && room.owner.as_deref() == Some(player.id.as_str()),
    })
}

/// A szoba egy sora a listához (és a `admin_room_update` eseményhez).
pub fn room_summary(_app: &Arc<App>, st: &ServerState, room: &Room, now: Option<f64>) -> Value {
    let now = now.unwrap_or_else(util::now);
    let game = &room.game;
    let current = if game.started && !game.finished { game.current_player() } else { None };
    json!({
        "id": room.id, "code": room.join_code, "name": room.name, "kind": room_kind(room), "status": room_status(room),
        "private": room.is_private, "owner": room.owner_name,
        "players": game.players.iter().map(|p| player_row(st, room, p)).collect::<Vec<_>>(), "spectators": room.spectators.len(),
        "turn_time_limit": game.turn_time_limit, "challenge_mode": game.challenge_mode,
        "has_bots": game.players.iter().any(|p| p.is_bot), "created_at": room.created_at,
        "idle_seconds": (now - room.last_activity) as i64, "stuck": stuck_reason(room, now),
        "turn_number": game.turn_number, "current_player": current.map(|p| p.name.clone()),
        "db_game_id": room.db_game_id, "paused": room.timer_paused_left.is_some(),
    })
}

/// Az összes szoba a memóriából, szűrhetően: q (kód / név / id / játékos), kind, status, stuck, bots.
pub fn list_rooms(app: &Arc<App>, st: &ServerState, args: &HashMap<String, String>) -> Value {
    let now = util::now();
    let q = args.get("q").map(|q| q.trim().to_lowercase()).unwrap_or_default();
    let flag = |key: &str| matches!(args.get(key).map(|s| s.as_str()), Some("1") | Some("true"));
    let mut rows: Vec<Value> = Vec::new();
    for room in st.rooms.values() {
        let summary = room_summary(app, st, room, Some(now));
        if args.get("kind").is_some_and(|k| !k.is_empty() && summary["kind"] != json!(k)) {
            continue;
        }
        if args.get("status").is_some_and(|s| !s.is_empty() && summary["status"] != json!(s)) {
            continue;
        }
        if flag("stuck") && summary["stuck"].is_null() {
            continue;
        }
        if flag("bots") && summary["has_bots"] != json!(true) {
            continue;
        }
        if !q.is_empty()
            && !(room.join_code.contains(&q)
                || room.id.to_lowercase().contains(&q)
                || room.name.to_lowercase().contains(&q)
                || summary["players"].as_array().is_some_and(|ps| ps.iter().any(|p| p["name"].as_str().is_some_and(|n| n.to_lowercase().contains(&q)))))
        {
            continue;
        }
        rows.push(summary);
    }
    rows.sort_by(|a, b| b["created_at"].as_f64().partial_cmp(&a["created_at"].as_f64()).unwrap_or(std::cmp::Ordering::Equal));
    let total = rows.len();
    json!({"items": rows, "total": total})
}

/// Szoba azonosító vagy 6 jegyű kód alapján (404, ha nincs).
pub fn find_room_id(st: &ServerState, key: &str) -> AdminResult<String> {
    if st.rooms.contains_key(key) {
        return Ok(key.to_string());
    }
    if let Some(room_id) = st.join_codes.get(key).filter(|id| st.rooms.contains_key(*id)) {
        return Ok(room_id.clone());
    }
    Err(AdminError::new("A szoba nem található.", 404))
}

fn chat_rows(room: &Room) -> Vec<Value> {
    let missing = room.chat_messages.len().saturating_sub(room.chat_meta.len());
    let row = |msg: &Value, ts: Value, user_id: Value| json!({"name": msg["name"], "message": msg["message"], "system": util::truthy(&msg["system"]), "ts": ts, "user_id": user_id});
    let mut rows: Vec<Value> = Vec::new();
    // A régebbi (meta nélküli) üzenetek időbélyeg nélkül
    for msg in room.chat_messages.iter().take(missing) {
        rows.push(row(msg, Value::Null, Value::Null));
    }
    for (msg, meta) in room.chat_messages.iter().skip(missing).zip(room.chat_meta.iter()) {
        rows.push(row(msg, json!(meta.ts), json!(meta.user_id)));
    }
    rows
}

/// Az összes futó szoba chatje egy helyen (időrendben), keresővel; a tiltott szavas üzenetek jelölve. Naplózva:
/// `view.chat`.
pub fn live_chat(app: &Arc<App>, st: &ServerState, ctx: &AdminContext, args: &HashMap<String, String>) -> AdminResult<Value> {
    let q = args.get("q").map(|q| q.trim().to_lowercase()).unwrap_or_default();
    let banned = app.banned_word_list();
    let mut rows: Vec<Value> = Vec::new();
    for room in st.rooms.values() {
        for mut item in chat_rows(room) {
            let message = item["message"].as_str().unwrap_or("").to_string();
            let name = item["name"].as_str().unwrap_or("").to_string();
            if !q.is_empty() && !message.to_lowercase().contains(&q) && !name.to_lowercase().contains(&q) && !room.name.to_lowercase().contains(&q) {
                continue;
            }
            item["room_id"] = json!(room.id);
            item["room_name"] = json!(room.name);
            item["code"] = json!(room.join_code);
            item["flagged"] = json!(super::moderation::contains_banned(&banned, &message));
            rows.push(item);
        }
    }
    rows.sort_by(|a, b| a["ts"].as_f64().unwrap_or(0.0).partial_cmp(&b["ts"].as_f64().unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal));
    let limit = int_arg(args.get("limit").map(|s| s.as_str()), 200, 1, 500) as usize;
    app.db.with(|tx| {
        record(tx, ctx, "view.chat", Some("chat"), None, &json!({"source": "live", "q": if q.is_empty() { Value::Null } else { json!(q) }})).map(|_| ())
    })?;
    let total = rows.len();
    let skip = total.saturating_sub(limit);
    Ok(json!({"items": rows.into_iter().skip(skip).collect::<Vec<_>>(), "total": total}))
}

/// A türelmi időben lévő játékosok hátralévő ideje (a lecsatlakozás idejéből és a beállított időkből).
fn grace_info(app: &Arc<App>, st: &ServerState, room: &Room) -> Vec<Value> {
    let now = util::now();
    let mut rows = Vec::new();
    for (token, info) in &st.disconnected_players {
        if info.room_id != room.id {
            continue;
        }
        let is_owner = room.owner_token.as_deref() == Some(token.as_str());
        let grace = if room.game.started || !is_owner { app.settings.get_i64("grace_disconnect") } else { app.settings.get_i64("grace_waiting_owner") };
        rows.push(json!({
            "name": info.player_name, "sid": info.sid,
            "remaining": (grace as f64 - (now - info.at)).max(0.0) as i64,
            "token_prefix": token.chars().take(6).collect::<String>(),
        }));
    }
    rows
}

/// A szoba teljes képe: tábla, pontok, zsák, szavazás, történet, chat, kapcsolatok. A kezek NEM szerepelnek (azokhoz
/// külön, naplózott kérés kell: `room_racks`).
pub fn room_detail(app: &Arc<App>, st: &ServerState, room_id: &str) -> Value {
    let Some(room) = st.rooms.get(room_id) else { return Value::Null };
    let game = &room.game;
    let mut detail = room_summary(app, st, room, None);
    let pending = game.pending_challenge.as_ref().map(|pc| {
        let names: HashMap<&str, &str> = game.players.iter().map(|p| (p.id.as_str(), p.name.as_str())).collect();
        let votes: Map<String, Value> =
            pc.votes.iter().map(|(pid, vote)| (names.get(pid.as_str()).unwrap_or(&pid.as_str()).to_string(), json!(vote.as_str()))).collect();
        let voters: Vec<&str> = game.voter_ids().iter().filter_map(|id| names.get(id.as_str()).copied()).collect();
        json!({
            "player": game.players[pc.player_idx].name, "words": pc.word_strs, "score": pc.score, "votes": votes,
            "voters": voters, "expires_at": pc.expires_at,
        })
    });
    let mut bag: Map<String, Value> = Map::new();
    for tile in &game.bag.tiles {
        let key = if tile.is_blank() { "?".to_string() } else { tile.as_str().to_string() };
        let n = bag.get(&key).and_then(|v| v.as_i64()).unwrap_or(0);
        bag.insert(key, json!(n + 1));
    }
    let mut spectators: Vec<&String> = room.spectators.values().collect();
    spectators.sort();
    let extra = json!({
        "board": game.board.to_json(), "bag": bag, "bag_count": game.bag.remaining(),
        "timer_expires_at": room.turn_timer_expires_at, "timer_paused_left": room.timer_paused_left,
        "pending": pending, "history": game.history(), "chat": chat_rows(room), "connections": grace_info(app, st, room),
        "spectator_names": spectators,
        "async": if game.async_mode { json!({"turn_hours": game.turn_hours, "deadline": game.turn_deadline}) } else { Value::Null },
        "last_action": game.last_action, "max_players": room.max_players,
        "puzzle": game.puzzle.as_ref().map(|p| json!({"date": p.date, "score": p.score})),
        "saved": room.db_game_id, "server_time": util::now(),
    });
    if let (Some(base), Some(more)) = (detail.as_object_mut(), extra.as_object()) {
        for (k, v) in more {
            base.insert(k.clone(), v.clone());
        }
    }
    detail
}

/// A játékosok keze (csak kifejezett kérésre; naplózva `view.racks`).
pub fn room_racks(app: &Arc<App>, st: &ServerState, ctx: &AdminContext, key: &str) -> AdminResult<Value> {
    let room_id = find_room_id(st, key)?;
    let room = &st.rooms[&room_id];
    app.db.with(|tx| record(tx, ctx, "view.racks", Some("room"), Some(&room.id), &json!({"code": room.join_code, "name": room.name})).map(|_| ()))?;
    Ok(json!(room
        .game
        .players
        .iter()
        .map(|p| json!({"name": p.name, "hand": p.hand.iter().map(|t| if t.is_blank() { "?".to_string() } else { t.as_str().to_string() }).collect::<Vec<_>>()}))
        .collect::<Vec<_>>()))
}

// ===== Szobák: beavatkozások =====

fn find_human(room: &Room, name: &str) -> AdminResult<(String, String)> {
    room.game
        .players
        .iter()
        .find(|p| p.name == name && !p.is_bot)
        .map(|p| (p.id.clone(), p.name.clone()))
        .ok_or_else(|| AdminError::field("A játékos nem található.", 404, "player"))
}

/// Beavatkozás után: időzítő, állapot küldése, szükség esetén robotlépés.
fn after_change(app: &Arc<App>, st: &mut ServerState, room_id: &str, restart_timer: bool) {
    let (pending, finished, is_async) = {
        let room = st.rooms.get_mut(room_id).expect("létező szoba");
        room.invalidate_turn_timer();
        (room.game.pending_challenge.is_some(), room.game.finished, room.is_async)
    };
    if pending {
        core::start_challenge_timer(app, st, room_id);
    } else if finished {
        st.rooms.get_mut(room_id).expect("létező szoba").invalidate_challenge_timer();
        core::save_game_to_db(app, st, room_id);
    } else if restart_timer {
        core::start_turn_timer(app, st, room_id, None);
    }
    core::emit_all_states(app, st, room_id);
    if finished {
        core::broadcast_rooms(app, st);
        if is_async {
            core::cleanup_finished_async(app, st, room_id);
        }
    }
    core::schedule_bot_turn(app, st, room_id);
}

/// Játékos eltávolítása a szobából: a token érvénytelen, aktív játékban `disconnected`, várakozóban törlés.
fn remove_player(app: &Arc<App>, st: &mut ServerState, room_id: &str, player_id: &str, player_name: &str) {
    let (started, was_owner, is_async) = {
        let room = &st.rooms[room_id];
        (room.game.started && !room.game.finished, room.owner.as_deref() == Some(player_id), room.is_async)
    };
    app.leave_room(player_id, room_id);
    st.player_rooms.remove(player_id);
    st.cleanup_player_token(player_id);
    app.emit_to(player_id, "room_left", &json!({}));
    app.emit_to(player_id, "error", &json!({"message": "Eltávolítottak a szobából."}));
    {
        let game = &mut st.rooms.get_mut(room_id).expect("létező szoba").game;
        if started {
            game.mark_disconnected(player_id);
        } else {
            game.remove_player(player_id);
        }
    }
    let (no_humans, manually_saved, db_game_id) = {
        let room = &st.rooms[room_id];
        (room.game.human_players().is_empty() || (!is_async && !room.game.has_connected_human()), room.manually_saved, room.db_game_id)
    };
    if no_humans {
        if started && !manually_saved {
            match db_game_id {
                Some(id) => app.db.abandon_game_by_id(id),
                None => app.db.abandon_game(room_id),
            }
        }
        core::cleanup_room(app, st, room_id);
    } else {
        if was_owner && !is_async {
            core::transfer_ownership(app, st, room_id);
        }
        if started {
            core::advance_turn_if_needed(app, st, room_id);
        }
        core::emit_all_states(app, st, room_id);
        app.emit_room(room_id, "player_left", &json!({"name": player_name}));
    }
    app.emit_all("rooms_list", &st.get_rooms_list());
}

fn timer_remaining(room: &Room) -> Option<f64> {
    if let Some(left) = room.timer_paused_left {
        return Some(left);
    }
    room.turn_timer_expires_at.map(|expires| (expires - util::now()).max(0.0))
}

/// Beavatkozás egy szobába. A törzs: {action, reason, ...}. Minden művelet naplózva, indoklással (a rendszerüzenet
/// kivétel: annak a szövege kerül a naplóba). A naplósor a művelet ELŐTT íródik, így ha a naplózás hibázik, a művelet
/// nem történik meg. Visszatér: rövid eredmény.
pub fn room_action(app: &Arc<App>, st: &mut ServerState, ctx: &AdminContext, room_key: &str, body: &Value) -> AdminResult<Value> {
    let act = body
        .get("action")
        .and_then(|a| a.as_str())
        .filter(|a| ROOM_ACTIONS.contains(a))
        .ok_or_else(|| AdminError::field("Ismeretlen művelet.", 400, "action"))?;
    let room_id = find_room_id(st, room_key)?;
    let (join_code, room_name) = {
        let room = &st.rooms[&room_id];
        (room.join_code.clone(), room.name.clone())
    };
    let target = json!({"code": join_code, "name": room_name});
    let with_target = |extra: Value| -> Value {
        let mut base = target.clone();
        if let (Some(b), Some(e)) = (base.as_object_mut(), extra.as_object()) {
            for (k, v) in e {
                b.insert(k.clone(), v.clone());
            }
        }
        base
    };
    let log = |name: &str, reason: Option<&Value>, details: Value| -> AdminResult<()> {
        action(&app.db, ctx, name, Some("room"), Some(room_id.clone()), reason, details, true, |_| Ok(()))
    };

    if act == "message" {
        let text = clean_text(body.get("message"), "message", MAX_SYSTEM_MESSAGE, true)?;
        app.db.with(|tx| record(tx, ctx, "room.message", Some("room"), Some(&room_id), &with_target(json!({"message": text}))).map(|_| ()))?;
        st.rooms.get_mut(&room_id).expect("létező szoba").add_chat_message("Rendszer", &text, None, true);
        app.emit_room(&room_id, "chat_message", &json!({"name": "Rendszer", "message": text, "system": true}));
        return Ok(json!({"message": text}));
    }

    let reason = body.get("reason");
    normalize_reason(reason)?; // indoklás nélkül nincs beavatkozás (az állapot-ellenőrzések előtt)
    let (started, puzzle) = {
        let game = &st.rooms[&room_id].game;
        (game.started && !game.finished, game.puzzle.is_some())
    };
    let name = format!("room.{act}");

    match act {
        "kick" => {
            let wanted = clean_text(body.get("player"), "player", 40, true)?;
            let (player_id, player_name) = find_human(&st.rooms[&room_id], &wanted)?;
            log(&name, reason, with_target(json!({"player": player_name})))?;
            remove_player(app, st, &room_id, &player_id, &player_name);
            Ok(json!({"player": player_name}))
        }
        "transfer" => {
            let wanted = clean_text(body.get("player"), "player", 40, true)?;
            let (player_id, player_name) = find_human(&st.rooms[&room_id], &wanted)?;
            let room = &st.rooms[&room_id];
            if room.is_async || room.is_puzzle {
                return Err(AdminError::new("Ebben a szobában nincs tulajdonos.", 409));
            }
            if room.game.players.iter().any(|p| p.id == player_id && p.disconnected) {
                return Err(AdminError::field("A játékos nincs online.", 409, "player"));
            }
            log(&name, reason, with_target(json!({"before": {"owner": room.owner_name}, "after": {"owner": player_name}})))?;
            let token = st.get_reconnect_token_for_sid(&player_id);
            st.rooms.get_mut(&room_id).expect("létező szoba").transfer_ownership(&player_id, &player_name, token.as_deref());
            app.emit_to(&player_id, "room_code", &json!({"code": join_code}));
            core::emit_all_states(app, st, &room_id);
            Ok(json!({"owner": player_name}))
        }
        "skip" => {
            if !started || puzzle {
                return Err(AdminError::new("Nincs aktív játék.", 409));
            }
            let room = &st.rooms[&room_id];
            if room.game.pending_challenge.is_some() {
                return Err(AdminError::new("Szavazás közben nem lehet átugrani a kört.", 409));
            }
            let (current_id, current_name, turn) = {
                let current = room.game.current_player().expect("aktív játékban van soron lévő");
                (current.id.clone(), current.name.clone(), room.game.turn_number)
            };
            log(&name, reason, with_target(json!({"player": current_name, "turn": turn})))?;
            let is_async = room.is_async;
            {
                let game = &mut st.rooms.get_mut(&room_id).expect("létező szoba").game;
                if game.pass_turn(&current_id, is_async).is_err() {
                    return Err(AdminError::new("A kör nem ugorható át.", 409));
                }
                // A lépésnaplóban „admin” jelöléssel
                if let Some(last) = game.move_log.last_mut() {
                    if !last.details.is_object() {
                        last.details = json!({});
                    }
                    last.details["admin"] = json!(true);
                }
            }
            after_change(app, st, &room_id, true);
            Ok(json!({"player": current_name}))
        }
        "extend" | "pause" | "resume" => {
            let room = &st.rooms[&room_id];
            if !started || room.game.turn_time_limit == 0 || room.is_async {
                return Err(AdminError::new("Ebben a játékban nincs körönkénti időzítő.", 409));
            }
            if room.game.pending_challenge.is_some() {
                return Err(AdminError::new("Szavazás közben nem módosítható az időzítő.", 409));
            }
            let remaining = timer_remaining(room);
            if act == "extend" {
                let seconds = body.get("seconds").and_then(|s| s.as_i64()).filter(|s| EXTEND_CHOICES.contains(s));
                let Some(seconds) = seconds else { return Err(AdminError::field("Érvénytelen időtartam.", 400, "seconds")) };
                log(&name, reason, with_target(json!({"seconds": seconds})))?;
                let paused = room.timer_paused_left.is_some();
                let total = remaining.unwrap_or(0.0) + seconds as f64;
                if paused {
                    st.rooms.get_mut(&room_id).expect("létező szoba").timer_paused_left = Some(total);
                } else {
                    core::start_turn_timer(app, st, &room_id, Some(total));
                }
                core::emit_all_states(app, st, &room_id);
                return Ok(json!({"seconds": total as i64}));
            }
            if act == "pause" {
                if room.timer_paused_left.is_some() {
                    return Err(AdminError::new("Az időzítő már szünetel.", 409));
                }
                let Some(remaining) = remaining else { return Err(AdminError::new("Nincs futó időzítő.", 409)) };
                log(&name, reason, with_target(json!({"remaining": remaining as i64})))?;
                let room = st.rooms.get_mut(&room_id).expect("létező szoba");
                room.invalidate_turn_timer();
                room.timer_paused_left = Some(remaining);
                core::emit_all_states(app, st, &room_id);
                return Ok(json!({"remaining": remaining as i64}));
            }
            let Some(left) = room.timer_paused_left else { return Err(AdminError::new("Az időzítő nem szünetel.", 409)) };
            log(&name, reason, target.clone())?;
            core::start_turn_timer(app, st, &room_id, Some(left.max(1.0)));
            core::emit_all_states(app, st, &room_id);
            Ok(json!({}))
        }
        "resolve_vote" => {
            let game = &st.rooms[&room_id].game;
            let Some(pending) = &game.pending_challenge else { return Err(AdminError::new("Nincs függő lerakás.", 409)) };
            let Some(accept) = body.get("accept").and_then(|a| a.as_bool()) else { return Err(AdminError::field("Érvénytelen kérés.", 400, "accept")) };
            log(&name, reason, with_target(json!({"accept": accept, "words": pending.word_strs})))?;
            let outcome = st.rooms.get_mut(&room_id).expect("létező szoba").game.force_resolve_pending(accept);
            let (result, message) = outcome.map_err(|m| AdminError::new(&m, 409))?;
            core::handle_challenge_result(app, st, &room_id, result, &message);
            Ok(json!({"result": result.as_str()}))
        }
        "reschedule_bot" => {
            let room = &st.rooms[&room_id];
            let current = if started { room.game.current_player() } else { None };
            let Some(current) = current.filter(|p| p.is_bot) else { return Err(AdminError::new("Nem robot következik.", 409)) };
            if !core::bot_should_move(room) {
                return Err(AdminError::new("A robot most nem léphet (nincs jelen ember és néző).", 409));
            }
            let bot_name = current.name.clone();
            log(&name, reason, with_target(json!({"bot": bot_name})))?;
            core::schedule_bot_turn(app, st, &room_id);
            Ok(json!({"bot": bot_name}))
        }
        "save" => {
            if !st.rooms[&room_id].game.started || puzzle {
                return Err(AdminError::new("Nincs mit menteni.", 409));
            }
            log(&name, reason, target.clone())?;
            let (ok, _) = core::save_game_to_db(app, st, &room_id);
            if !ok {
                return Err(AdminError::new("Mentési hiba.", 500));
            }
            let room = st.rooms.get_mut(&room_id).expect("létező szoba");
            room.manually_saved = true;
            Ok(json!({"game_id": room.db_game_id}))
        }
        "end" => {
            if !started || puzzle {
                return Err(AdminError::new("Nincs aktív játék.", 409));
            }
            let scores: Map<String, Value> = st.rooms[&room_id].game.players.iter().map(|p| (p.name.clone(), json!(p.score))).collect();
            log(&name, reason, with_target(json!({"scores": scores})))?;
            let room = st.rooms.get_mut(&room_id).expect("létező szoba");
            room.game.force_end();
            room.invalidate_challenge_timer();
            room.invalidate_bot_turn();
            let winners: Vec<String> = room.game.winners.iter().map(|p| p.name.clone()).collect();
            after_change(app, st, &room_id, false);
            Ok(json!({"winners": winners}))
        }
        "void" => {
            if !started || puzzle {
                return Err(AdminError::new("Nincs aktív játék.", 409));
            }
            log(&name, reason, target.clone())?;
            let (ok, _) = core::save_game_to_db(app, st, &room_id);
            let db_game_id = st.rooms[&room_id].db_game_id;
            let Some(db_game_id) = db_game_id.filter(|_| ok) else { return Err(AdminError::new("Mentési hiba.", 500)) };
            app.db.set_game_status(db_game_id, "voided")?;
            st.rooms.get_mut(&room_id).expect("létező szoba").manually_saved = true; // a feloszlatás ne írja át „abandoned”-re
            core::disband_active_room(app, st, &room_id, "A játék érvénytelenítve lett.", None);
            app.emit_all("rooms_list", &st.get_rooms_list());
            Ok(json!({"game_id": null}))
        }
        _ => {
            // disband
            let save = body.get("save").and_then(|s| s.as_bool()).unwrap_or(false);
            let message = clean_text(body.get("message"), "message", MAX_SYSTEM_MESSAGE, false)?;
            let message = if message.is_empty() { "A szobát feloszlatták.".to_string() } else { message };
            log(&name, reason, with_target(json!({"save": save, "message": message})))?;
            let (started_unfinished, puzzle) = {
                let game = &st.rooms[&room_id].game;
                (game.started && !game.finished, game.puzzle.is_some())
            };
            if save && started_unfinished && !puzzle {
                let (ok, _) = core::save_game_to_db(app, st, &room_id);
                if ok {
                    st.rooms.get_mut(&room_id).expect("létező szoba").manually_saved = true;
                }
            }
            core::disband_active_room(app, st, &room_id, &message, None);
            app.emit_all("rooms_list", &st.get_rooms_list());
            Ok(json!({}))
        }
    }
}

// ===== Áttekintés =====

fn count(tx: &Transaction, sql: &str, params: impl rusqlite::Params) -> AdminResult<i64> {
    Ok(fetch_one(tx, sql, params)?.map(|r| r.int("n")).unwrap_or(0))
}

/// Mai számlálók (UTC nap): regisztrációk, befejezett játékok, napi feladvány próbálkozások, szótár-építő döntések,
/// kizárt szavak.
pub fn today_counters(app: &App) -> AdminResult<Value> {
    let today = util::utcnow().format("%Y-%m-%d").to_string();
    let mut data = txn(&app.db, |tx| {
        Ok(json!({
            "registrations": count(tx, "SELECT COUNT(*) AS n FROM users WHERE date(created_at) = ?", [&today])?,
            "finished_games": count(tx, "SELECT COUNT(*) AS n FROM saved_games WHERE status = 'finished' AND date(updated_at) = ?", [&today])?,
            "daily_attempts": count(tx, "SELECT COALESCE(SUM(attempts), 0) AS n FROM daily_scores WHERE puzzle_date = ?", [crate::daily::today_str()])?,
            "review_decisions": count(tx, "SELECT COUNT(*) AS n FROM word_reviews WHERE date(created_at) = ?", [&today])?,
            "review_rejections": count(tx, "SELECT COUNT(*) AS n FROM word_reviews WHERE verdict = 0 AND date(created_at) = ?", [&today])?,
        }))
    })?;
    data["rejected_words"] = json!(crate::dictionary::listed_rejected().len() + crate::dictionary::voted_rejected().len());
    Ok(data)
}

/// Az élő számlálók (a szerver memóriájából).
pub fn live_counters(app: &App, st: &ServerState) -> Value {
    let rooms: Vec<&Room> = st.rooms.values().collect();
    let guests = st.player_names.keys().filter(|sid| st.player_auth.get(*sid).map(|a| a.is_guest).unwrap_or(true)).count();
    let sockets = app.io().map(|io| io.sockets().len()).unwrap_or(0).max(st.player_names.len());
    let waiting = rooms.iter().filter(|r| !r.game.started && !r.game.finished).count();
    let running: Vec<&&Room> = rooms.iter().filter(|r| r.game.started && !r.game.finished).collect();
    json!({
        "online_users": st.get_online_user_ids().len(), "online_guests": guests, "sockets": sockets,
        "rooms_total": rooms.len(), "rooms_waiting": waiting,
        "rooms_active": running.iter().filter(|r| !r.is_async && !r.is_puzzle).count(),
        "rooms_async": rooms.iter().filter(|r| r.is_async).count(), "rooms_puzzle": rooms.iter().filter(|r| r.is_puzzle).count(),
        "rooms_public": rooms.iter().filter(|r| !r.is_private).count(), "rooms_private": rooms.iter().filter(|r| r.is_private).count(),
        "games_running": running.len(),
        "games_with_bots": running.iter().filter(|r| r.game.players.iter().any(|p| p.is_bot)).count(),
        "spectators": rooms.iter().map(|r| r.spectators.len()).sum::<usize>(),
        "pending_votes": running.iter().filter(|r| r.game.pending_challenge.is_some()).count(),
        "turn_timers": running.iter().filter(|r| r.turn_timer_expires_at.is_some()).count(),
        "grace_players": st.disconnected_players.len(),
    })
}

/// Figyelmeztető kártyák: [{code, level: 'red'|'yellow', count?, detail?}].
pub fn alerts(app: &App, st: &ServerState) -> AdminResult<Vec<Value>> {
    let mut found: Vec<Value> = Vec::new();
    let mut add = |code: &str, level: &str, extra: Value| {
        let mut item = json!({"code": code, "level": level});
        if let (Some(i), Some(e)) = (item.as_object_mut(), extra.as_object()) {
            for (k, v) in e {
                i.insert(k.clone(), v.clone());
            }
        }
        found.push(item);
    };
    if !crate::dictionary::is_available() {
        add("dictionary_down", "red", json!({}));
    }
    if !app.mailer.is_configured() {
        add("smtp_missing", "yellow", json!({}));
    }
    if !app.push.is_available() {
        add("push_missing", "yellow", json!({}));
    }
    let now = util::now();
    let stuck: Vec<(String, &str)> = st.rooms.values().filter_map(|r| stuck_reason(r, now).map(|reason| (r.name.clone(), reason))).collect();
    if !stuck.is_empty() {
        add(
            "stuck_games",
            "red",
            json!({"count": stuck.len(), "detail": stuck.iter().take(5).map(|(n, r)| format!("{n}: {r}")).collect::<Vec<_>>().join(", ")}),
        );
    }
    let overdue =
        st.rooms.values().filter(|r| r.is_async && r.game.started && !r.game.finished && r.game.turn_deadline.is_some_and(|d| now > d + 300.0)).count();
    if overdue > 0 {
        add("async_overdue", "yellow", json!({"count": overdue}));
    }
    let window = util::format_ts(util::utcnow() - chrono::Duration::seconds(ADMIN_ALERT_WINDOW));
    let spike_window = util::format_ts(util::utcnow() - chrono::Duration::minutes(10));
    let (by_ip, by_account, spike) = txn(&app.db, |tx| {
        let by_ip = fetch_all(
            tx,
            "SELECT ip, COUNT(*) AS n FROM login_events WHERE success = 0 AND created_at >= ? AND ip IS NOT NULL GROUP BY ip HAVING n >= 10 ORDER BY n DESC LIMIT 3",
            [&window],
        )?;
        let by_account = fetch_all(
            tx,
            "SELECT email, COUNT(*) AS n FROM login_events WHERE success = 0 AND created_at >= ? AND email != '' GROUP BY email HAVING n >= 5 ORDER BY n DESC LIMIT 3",
            [&window],
        )?;
        let spike = fetch_all(
            tx,
            "SELECT user_id, COUNT(*) AS n FROM word_reviews WHERE verdict = 0 AND created_at >= ? GROUP BY user_id HAVING n >= 25 ORDER BY n DESC LIMIT 3",
            [&spike_window],
        )?;
        Ok((by_ip, by_account, spike))
    })?;
    if !by_ip.is_empty() || !by_account.is_empty() {
        let total: i64 = by_ip.iter().chain(by_account.iter()).map(|r| r.int("n")).sum();
        let detail: Vec<String> = by_ip
            .iter()
            .map(|r| format!("{}: {}", r.text("ip"), r.int("n")))
            .chain(by_account.iter().map(|r| format!("{}: {}", r.text("email"), r.int("n"))))
            .collect();
        add("failed_logins", "yellow", json!({"count": total, "detail": detail.join(", ")}));
    }
    if !spike.is_empty() {
        let detail: Vec<String> = spike.iter().map(|r| format!("#{}: {}", r.int("user_id"), r.int("n"))).collect();
        add("review_spike", "yellow", json!({"count": spike.len(), "detail": detail.join(", ")}));
    }
    let last_backup = super::system::last_backup_time(app);
    if last_backup.is_none_or(|b| now - b > 24.0 * 3600.0) {
        add("backup_old", "yellow", json!({}));
    }
    Ok(found)
}

/// Az áttekintő oldal teljes tartalma.
pub fn overview(app: &Arc<App>, st: &ServerState) -> AdminResult<Value> {
    Ok(json!({
        "live": live_counters(app, st), "today": today_counters(app)?, "server": super::system::process_info(app),
        "services": super::system::services_info(app), "alerts": alerts(app, st)?,
        "maintenance": app.settings.maintenance(), "generated_at": util::now(),
        "versions": {"git": super::system::git_commit(app), "asset_version": crate::server::http::asset_version(app)},
    }))
}

/// A 5 másodpercenként küldött élő számlálók (Socket.IO `admin_overview`).
pub fn overview_light(app: &Arc<App>, st: &ServerState) -> Value {
    json!({"live": live_counters(app, st), "server": super::system::process_info(app), "generated_at": util::now()})
}

// ===== Grafikonok (utolsó 30 nap) =====

pub fn day_range(days: i64) -> Vec<String> {
    let today = util::utcnow().date();
    (0..days).rev().map(|i| (today - chrono::Duration::days(i)).format("%Y-%m-%d").to_string()).collect()
}

fn fill(days: &[String], rows: &[crate::db::Row]) -> Value {
    let mapping: HashMap<String, i64> = rows.iter().map(|r| (r.text("day"), r.int("n"))).collect();
    json!(days.iter().map(|d| json!({"day": d, "value": mapping.get(d).copied().unwrap_or(0)})).collect::<Vec<_>>())
}

pub const ACTIVITY_SQL_PUB: &str = "SELECT date(created_at) AS day, user_id FROM login_events WHERE success = 1 AND user_id IS NOT NULL \
     UNION SELECT date(sg.updated_at), gp.user_id FROM game_players gp JOIN saved_games sg ON sg.id = gp.game_id \
     WHERE gp.user_id IS NOT NULL AND sg.status = 'finished' \
     UNION SELECT puzzle_date, user_id FROM daily_scores WHERE attempts > 0 \
     UNION SELECT date(created_at), user_id FROM word_reviews";

/// Napi bontású idősorok: regisztrációk, aktív felhasználók (DAU), befejezett játékok, napi feladvány.
pub fn charts(app: &App, days: i64) -> AdminResult<Value> {
    let days = int_field(Some(&json!(days)), "days", 7, 365)?;
    let span = day_range(days);
    let since = span[0].clone();
    txn(&app.db, |tx| {
        let registrations = fetch_all(tx, "SELECT date(created_at) AS day, COUNT(*) AS n FROM users WHERE date(created_at) >= ? GROUP BY day", [&since])?;
        let active = fetch_all(tx, &format!("SELECT day, COUNT(*) AS n FROM ({ACTIVITY_SQL_PUB}) WHERE day >= ? GROUP BY day"), [&since])?;
        let human = fetch_all(
            tx,
            "SELECT date(updated_at) AS day, COUNT(*) AS n FROM saved_games WHERE status = 'finished' AND has_bots = 0 AND date(updated_at) >= ? GROUP BY day",
            [&since],
        )?;
        let robot = fetch_all(
            tx,
            "SELECT date(updated_at) AS day, COUNT(*) AS n FROM saved_games WHERE status = 'finished' AND has_bots = 1 AND date(updated_at) >= ? GROUP BY day",
            [&since],
        )?;
        let daily = fetch_all(tx, "SELECT puzzle_date AS day, COUNT(*) AS n FROM daily_scores WHERE attempts > 0 AND puzzle_date >= ? GROUP BY day", [&since])?;
        Ok(json!({
            "days": span, "registrations": fill(&span, &registrations), "active_users": fill(&span, &active),
            "games_human": fill(&span, &human), "games_bots": fill(&span, &robot), "daily_players": fill(&span, &daily),
        }))
    })
}
