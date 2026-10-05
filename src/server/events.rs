//! Socket.IO eseménykezelők, 1. rész: kapcsolat életciklusa (csatlakozás, lecsatlakozás, türelmi idő), azonosság,
//! szobák létrehozása / csatlakozás / kilépés, játék indítása és mentése. A függvények a hívó által zárolt
//! `ServerState`-en dolgoznak (a zárat a `server::register` makrója veszi fel).

use super::core::*;
use crate::admin::moderation;
use crate::ai;
use crate::app::App;
use crate::game::{ALLOWED_HINT_LIMITS, DEFAULT_HINT_LIMIT, Game, MoveLog};
use crate::room::{Identity, Room};
use crate::socket_auth::{TOKEN_MAX_AGE, verify_socket_token};
use crate::state::{AuthInfo, ServerState};
use crate::util;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

pub const TOO_MANY: &str = "Túl sok kérés, várj egy kicsit.";
pub const ALREADY_IN_ROOM: &str = "Már egy szobában vagy. Előbb lépj ki belőle.";

// --- Közös segédek ---

pub fn action_result(app: &Arc<App>, sid: &str, success: bool, message: &str) {
    app.emit_to(sid, "action_result", &json!({"success": success, "message": message}));
}

/// Forgalomkorlát + a játékos szobája (hibaüzenettel, ha a korlát elfogyott).
pub fn room_context(app: &Arc<App>, st: &ServerState, sid: &str, event: &str) -> Option<String> {
    if !app.limiter.check_socket(sid, event) {
        app.emit_error(sid, TOO_MANY);
        return None;
    }
    st.room_id_for_player(sid)
}

/// A kapcsolat regisztrált felhasználójának azonosítója (vendégnél None).
pub fn registered_user_id(st: &ServerState, sid: &str) -> Option<i64> {
    st.player_auth.get(sid).and_then(|a| a.registered_id())
}

/// Igaz, ha a kapcsolat a szoba tulajdonosáé (token vagy SID alapján).
pub fn is_owner(st: &ServerState, room: &Room, sid: &str) -> bool {
    let token = st.get_reconnect_token_for_sid(sid);
    (room.owner_token.is_some() && room.owner_token == token) || room.owner.as_deref() == Some(sid)
}

/// Igaz, ha a kapcsolat bejelentkezett, admin e-mail című felhasználóé (a kliens állításában nem bízunk: a
/// user_id a `set_name` aláírt tokenjéből jön).
pub fn is_admin_sid(app: &Arc<App>, st: &ServerState, sid: &str) -> bool {
    if st.admin_sids.contains_key(sid) {
        return true;
    }
    st.get_user_id_for_sid(sid)
        .and_then(|uid| app.db.get_user_by_id(uid).ok().flatten())
        .is_some_and(|user| app.is_admin_email(&user.email))
}

/// Karbantartási módban új játék nem indítható (az admin kivétel). Visszatér: hibaüzenet vagy None.
pub fn maintenance_block(app: &Arc<App>, st: &ServerState, sid: &str) -> Option<&'static str> {
    if app.settings.maintenance().is_some() && !is_admin_sid(app, st, sid) {
        return Some("A szerver karbantartás alatt van, új játék most nem indítható.");
    }
    None
}

/// Kikapcsolt funkció (napi feladvány, levelezős játék...) vagy karbantartás miatti tiltás üzenete, vagy None.
pub fn feature_block(app: &Arc<App>, st: &ServerState, sid: &str, feature: &str) -> Option<&'static str> {
    if !app.settings.get_bool(feature) && !is_admin_sid(app, st, sid) {
        return Some("Ez a funkció jelenleg ki van kapcsolva.");
    }
    maintenance_block(app, st, sid)
}

/// A `room_joined` esemény közös tartalma.
pub fn room_joined_payload(room: &Room, token: &str, is_owner: bool) -> Value {
    let game = &room.game;
    json!({
        "room_id": room.id,
        "room_name": room.name,
        "is_owner": is_owner,
        "challenge_mode": game.challenge_mode,
        "is_private": room.is_private,
        "turn_time_limit": game.turn_time_limit,
        "hint_limit": game.hint_limit,
        "reconnect_token": token,
        "chat_messages": room.chat_messages,
    })
}

/// Értesíti az online barátokat, ha egy felhasználó státusza változott.
pub fn notify_friends_presence(app: &Arc<App>, st: &ServerState, user_id: i64, online: bool) {
    use crate::db::RowExt;
    for friend in app.db.get_friends(user_id) {
        for fsid in st.get_user_sids(friend.int("id")) {
            app.emit_to(&fsid, "friend_presence_changed", &json!({"friend_id": user_id, "online": online}));
        }
    }
}

fn new_room_id() -> String {
    util::short_id()
}

// --- Kapcsolat ---

/// A lecsatlakozott játékosnak járó türelmi idő (mp): a várakozó szoba tulajdonosának hosszabb, mindenki másnak (és
/// az aktív játékban mindenkinek) a rövidebb; az admin panelen állítható.
pub fn grace_seconds(app: &App, game_started: bool, was_owner: bool) -> i64 {
    if game_started || !was_owner { app.settings.get_i64("grace_disconnect") } else { app.settings.get_i64("grace_waiting_owner") }
}

/// Lecsatlakozás: megfigyelő / játékos eltávolítása vagy türelmi idő indítása.
pub fn handle_disconnect(app: &Arc<App>, st: &mut ServerState, sid: &str) {
    st.admin_sids.remove(sid);
    for room in st.rooms.values_mut() {
        room.admin_watchers.remove(sid);
    }
    if let Some(spectated) = st.remove_spectator(sid) {
        if st.rooms.contains_key(&spectated) {
            emit_all_states(app, st, &spectated);
        }
    }
    let room_id = st.player_rooms.get(sid).cloned();
    let token = st.get_reconnect_token_for_sid(sid);
    let registered = st.player_auth.get(sid).and_then(|a| a.registered_id());
    let was_online = registered.is_some_and(|u| st.is_user_online(u));
    let name = st.player_names.get(sid).cloned().unwrap_or_else(|| "?".to_string());
    let auth = st.player_auth.get(sid).cloned();
    let mut kept_for_grace = false;

    match room_id.filter(|id| st.rooms.contains_key(id)) {
        Some(room_id) => {
            let (is_async, started, finished, was_owner) = {
                let room = &st.rooms[&room_id];
                let was_owner = room.owner.as_deref() == Some(sid) || (room.owner_token.is_some() && room.owner_token == token);
                (room.is_async, room.game.started, room.game.finished, was_owner)
            };
            if is_async && started && !finished {
                // Levelezős játék: a játék tovább él, a játékos bármikor visszatérhet (nincs türelmi idő)
                st.rooms.get_mut(&room_id).expect("létező szoba").game.mark_disconnected(sid);
                app.leave_room(sid, &room_id);
                if let Some(token) = &token {
                    st.mark_disconnected(token, sid, &room_id, &name, auth.clone());
                }
                emit_all_states(app, st, &room_id);
                app.emit_room(&room_id, "player_disconnected", &json!({"name": name}));
                st.player_rooms.remove(sid);
            } else if let (Some(token), false) = (token.clone(), finished) {
                // Aktív játék és várakozó szoba: türelmi idő, a játékos a tokenjével visszatérhet
                kept_for_grace = true;
                let grace = grace_seconds(app, started, was_owner);
                st.rooms.get_mut(&room_id).expect("létező szoba").game.mark_disconnected(sid);
                app.leave_room(sid, &room_id);
                let seq = st.mark_disconnected(&token, sid, &room_id, &name, auth.clone());
                let (app2, token2) = (app.clone(), token.clone());
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(grace.max(0) as u64)).await;
                    let mut st = app2.state.lock();
                    if st.disconnect_is_current(&token2, seq) {
                        finalize_player_disconnect(&app2, &mut st, &token2);
                    }
                });
                emit_all_states(app, st, &room_id);
                app.emit_room(&room_id, "player_disconnected", &json!({"name": name}));
                st.player_rooms.remove(sid);
            } else {
                // Befejezett játék (vagy token nélküli játékos): azonnali eltávolítás
                let (no_humans, was_owner_sid) = {
                    let room = st.rooms.get_mut(&room_id).expect("létező szoba");
                    room.game.remove_player(sid);
                    (
                        room.game.human_players().is_empty() || (room.is_async && !room.game.has_connected_human()),
                        room.owner.as_deref() == Some(sid),
                    )
                };
                app.leave_room(sid, &room_id);
                if no_humans {
                    cleanup_room(app, st, &room_id);
                } else {
                    if was_owner_sid {
                        transfer_ownership(app, st, &room_id);
                    }
                    emit_all_states(app, st, &room_id);
                    app.emit_room(&room_id, "player_left", &json!({"name": name}));
                }
                st.player_rooms.remove(sid);
                app.emit_all("rooms_list", &st.get_rooms_list());
                st.cleanup_player_token(sid);
            }
        }
        None => {
            // Nem volt szobában, de lehet tokenje
            if token.is_some() {
                st.cleanup_player_token(sid);
            }
        }
    }

    st.player_names.remove(sid);
    st.hidden_sids.remove(sid);
    st.remove_online_user(sid);
    if let Some(uid) = registered {
        if was_online && !st.is_user_online(uid) {
            notify_friends_presence(app, st, uid, false);
        }
    }
    // player_auth törlése: csak ha NEM türelmi időben van (aktív játék vagy várakozó szoba). Türelmi idő esetén az
    // auth info a disconnected_players-ben van mentve, és a finalize_player_disconnect törli.
    if !kept_for_grace {
        st.player_auth.remove(sid);
    }
    app.limiter.clear_sid(sid);
    app.emit_all("rooms_list", &st.get_rooms_list());
}

/// Türelmi idő lejárt: játékos végleges eltávolítása.
pub fn finalize_player_disconnect(app: &Arc<App>, st: &mut ServerState, token: &str) {
    let Some(info) = st.finalize_disconnect(token) else { return };
    let (room_id, old_sid) = (info.room_id.clone(), info.sid.clone());
    // A türelmi idő alatt megőrzött auth info törlése
    st.player_auth.remove(&old_sid);
    if !st.rooms.contains_key(&room_id) {
        return;
    }
    let (is_owner, is_active_game, has_players) = {
        let room = &st.rooms[&room_id];
        let game = &room.game;
        (room.owner.as_deref() == Some(old_sid.as_str()), game.started && !game.finished, !game.players.is_empty())
    };

    // A tulajdonos időtúllépése aktív játék közben: feloszlatás
    if is_owner && is_active_game && has_players {
        // Automatikus mentés, ha a tulajdonos időtúllép
        let (success, _) = save_game_to_db(app, st, &room_id);
        if success {
            if let Some(room) = st.rooms.get_mut(&room_id) {
                room.manually_saved = true;
            }
        }
        disband_active_room(
            app,
            st,
            &room_id,
            "A szoba tulajdonosa végleg lecsatlakozott. A játék automatikusan mentésre került, és a szoba feloszlott.",
            Some(&old_sid),
        );
        app.emit_all("rooms_list", &st.get_rooms_list());
        return;
    }

    // Ha aktív a játék, NEM vesszük ki a Game objektumból, hogy megmaradjon a mentésben. Csak disconnected=True marad
    // (amit már a disconnect eventnél beállítottunk).
    let (no_humans, started_unfinished, manually_saved, db_game_id) = {
        let room = st.rooms.get_mut(&room_id).expect("létező szoba");
        if !is_active_game {
            room.game.remove_player(&old_sid);
        }
        (
            !room.game.has_connected_human(),
            room.game.started && !room.game.finished,
            room.manually_saved,
            room.db_game_id,
        )
    };
    if no_humans {
        // Ha minden ember lecsatlakozott:
        if started_unfinished && !manually_saved {
            match db_game_id {
                Some(id) => app.db.abandon_game_by_id(id),
                None => app.db.abandon_game(&room_id),
            }
        }
        cleanup_room(app, st, &room_id);
    } else {
        if st.rooms[&room_id].owner.as_deref() == Some(old_sid.as_str()) {
            transfer_ownership(app, st, &room_id);
        }
        // Ha az ő köre volt, léptetjük
        if is_active_game {
            advance_turn_if_needed(app, st, &room_id);
        }
        emit_all_states(app, st, &room_id);
        app.emit_room(&room_id, "player_left", &json!({"name": info.player_name}));
    }
    app.emit_all("rooms_list", &st.get_rooms_list());
}

// --- Azonosság ---

pub fn set_name(app: &Arc<App>, st: &mut ServerState, sid: &str, data: Value) {
    if !app.limiter.check_socket(sid, "set_name") {
        app.emit_error(sid, TOO_MANY);
        return;
    }
    let Some(object) = data.as_object() else { return };
    let mut name = sanitize_name(object.get("name").unwrap_or(&json!(""))).filter(|n| util::casefold(n) != "rendszer").unwrap_or_else(|| "Névtelen".to_string());

    let mut user_id: Option<i64> = None;
    let mut is_guest = true;
    let guest_flag = object.get("is_guest");
    let explicit_guest = matches!(guest_flag, Some(Value::Bool(true)) | None);
    let explicit_registered = matches!(guest_flag, Some(Value::Bool(false)));
    if explicit_guest && !app.settings.get_bool("guest_allowed") {
        app.emit_error(sid, "A vendég mód jelenleg ki van kapcsolva.");
        return;
    }
    if !explicit_registered && moderation::contains_banned(&app.banned_word_list(), &name) {
        app.emit_error(sid, "Ez a név nem engedélyezett.");
        name = "Névtelen".to_string();
    }
    if explicit_registered {
        // Regisztrált felhasználónak csak aláírt tokennel adhatja ki magát valaki: a kliens által küldött user_id
        // önmagában nem bizonyít semmit.
        let verified = verify_socket_token(&app.config.secret_key, object.get("auth_token").and_then(|t| t.as_str()), TOKEN_MAX_AGE);
        let user = verified.and_then(|id| app.db.get_user_by_id(id).ok().flatten());
        if let Some(user) = &user {
            let ban = user.ban();
            if ban.is_some() || user.deleted_at.is_some() {
                app.emit_to(sid, "account_banned", &json!({"reason": ban.as_ref().map(|b| b.reason.clone()).unwrap_or_default(), "until": ban.as_ref().and_then(|b| b.until.clone())}));
                app.emit_error(sid, "A fiókod ki van tiltva.");
                return;
            }
            user_id = Some(user.id);
            is_guest = false;
            name = sanitize_name_str(&user.display_name).unwrap_or(name);
        } else {
            app.emit_error(sid, "A munkamenet lejárt, jelentkezz be újra.");
        }
    }

    let previous_user_id = st.get_user_id_for_sid(sid);
    let was_online = user_id.is_some_and(|u| st.is_user_online(u));
    if previous_user_id != user_id {
        app.leave_room(sid, ADMIN_ROOM); // másik azonosság → az admin feliratkozás megszűnik
    }
    st.register_player(sid, &name, AuthInfo { user_id, is_guest });

    if let Some(previous) = previous_user_id {
        if Some(previous) != user_id && !st.is_user_online(previous) {
            notify_friends_presence(app, st, previous, false);
        }
    }
    if let Some(uid) = user_id {
        if !was_online && st.is_user_online(uid) {
            notify_friends_presence(app, st, uid, true);
        }
    }
}

pub fn logout(app: &Arc<App>, st: &mut ServerState, sid: &str, _data: Value) {
    app.leave_room(sid, ADMIN_ROOM); // a kijelentkezett kapcsolat ne kapjon több admin eseményt
    if st.player_rooms.get(sid).is_some() {
        leave_room(app, st, sid, Value::Null);
    }
    if st.get_spectated_room(sid).is_some() {
        super::extras::leave_spectate(app, st, sid, Value::Null);
    }
    let previous = st.get_user_id_for_sid(sid);
    st.remove_online_user(sid);
    st.player_auth.remove(sid);
    st.player_names.remove(sid);
    if let Some(prev) = previous {
        if !st.is_user_online(prev) {
            notify_friends_presence(app, st, prev, false);
        }
    }
}

/// Aktív játékban vagy várakozó szobában lévő, még „élő” kapcsolatot lecsatlakozottnak jelöl, hogy a token új
/// kapcsolatról átvehető legyen. Visszaadja a régi SID-t, vagy None-t, ha nincs mit átvenni.
fn release_live_session(app: &Arc<App>, st: &mut ServerState, token: &str) -> Option<String> {
    let info = st.get_token_info(token)?.clone();
    let room = st.rooms.get_mut(&info.room_id)?;
    if room.game.finished || st.player_rooms.get(&info.sid) != Some(&info.room_id) {
        return None;
    }
    room.game.mark_disconnected(&info.sid);
    app.leave_room(&info.sid, &info.room_id);
    st.player_rooms.remove(&info.sid);
    let name = st.player_names.get(&info.sid).cloned().unwrap_or(info.player_name.clone());
    let auth = st.player_auth.get(&info.sid).cloned().or(info.auth_info.clone());
    st.mark_disconnected(token, &info.sid, &info.room_id, &name, auth);
    Some(info.sid)
}

/// Újracsatlakozás egy aktív játékba reconnect tokennel.
pub fn rejoin_room(app: &Arc<App>, st: &mut ServerState, sid: &str, data: Value) {
    if !app.limiter.check_socket(sid, "rejoin_room") {
        app.emit_error(sid, TOO_MANY);
        return;
    }
    let fail = |message: &str| app.emit_to(sid, "rejoin_failed", &json!({"message": message}));
    let Some(object) = data.as_object() else {
        fail("Érvénytelen kérés.");
        return;
    };
    let token = match object.get("token") {
        Some(Value::String(t)) if !t.is_empty() => t.clone(),
        _ => {
            fail("Hiányzó token.");
            return;
        }
    };

    let mut dc_info = st.get_disconnected_info(&token).cloned();
    let mut token_info = st.get_token_info(&token).cloned();

    // Munkamenet-átvétel: a régi kapcsolatot a szerver még élőnek hiszi (pl. a telefon háttérbe került, vagy
    // újratöltődött az oldal), de a token már új kapcsolatról érkezik.
    let mut takeover_old_sid = None;
    if dc_info.is_none() && token_info.is_some() {
        takeover_old_sid = release_live_session(app, st, &token);
        dc_info = st.get_disconnected_info(&token).cloned();
        token_info = st.get_token_info(&token).cloned();
    }
    let (Some(dc_info), Some(token_info)) = (dc_info, token_info) else {
        fail("Érvénytelen vagy lejárt token.");
        return;
    };
    if let Some(uid) = token_info.auth_info.as_ref().and_then(|a| a.registered_id()) {
        if app.db.get_ban(uid).is_some() {
            fail("A fiókod ki van tiltva.");
            return;
        }
    }

    let room_id = dc_info.room_id.clone();
    let old_sid = dc_info.sid.clone();
    if !st.rooms.contains_key(&room_id) {
        st.disconnected_players.remove(&token);
        st.reconnect_tokens.remove(&token);
        fail("A szoba már nem létezik.");
        return;
    }
    let replaced = st.rooms.get_mut(&room_id).expect("létező szoba").game.replace_player_sid(&old_sid, sid);
    if !replaced {
        fail("Nem sikerült újracsatlakozni.");
        return;
    }
    st.complete_rejoin(&token, sid);

    let owns = {
        let room = st.rooms.get_mut(&room_id).expect("létező szoba");
        let owns = room.owner.as_deref() == Some(old_sid.as_str()) || room.owner_token.as_deref() == Some(token.as_str());
        if owns {
            room.owner = Some(sid.to_string());
            room.owner_token = Some(token.clone());
        }
        owns
    };

    let player_name = dc_info.player_name.clone();
    st.player_rooms.insert(sid.to_string(), room_id.clone());
    st.player_names.insert(sid.to_string(), player_name.clone());
    st.player_auth.remove(&old_sid);
    // Visszaállítjuk az auth info-t is, és az online követést (a register_player logikája)
    let auth_info = token_info.auth_info.clone().unwrap_or_else(AuthInfo::guest);
    st.player_auth.insert(sid.to_string(), auth_info.clone());
    if let Some(uid) = auth_info.registered_id() {
        st.add_online_user(sid, uid);
    }

    app.join_room(sid, &room_id);
    let (payload, code) = {
        let room = &st.rooms[&room_id];
        (room_joined_payload(room, &token, owns), room.join_code.clone())
    };
    app.emit_to(sid, "room_joined", &payload);
    if owns {
        app.emit_to(sid, "room_code", &json!({"code": code}));
    }
    emit_all_states(app, st, &room_id);
    app.emit_room(&room_id, "player_reconnected", &json!({"name": player_name}));
    app.emit_all("rooms_list", &st.get_rooms_list());
    schedule_bot_turn(app, st, &room_id);

    if let Some(old) = takeover_old_sid.filter(|o| o != sid) {
        // A régi (elavult) kapcsolat lezárása, miután az új már átvette a helyét.
        app.disconnect_sid(&old);
    }
}

pub fn get_rooms(app: &Arc<App>, st: &mut ServerState, sid: &str, _data: Value) {
    if !app.limiter.check_socket(sid, "get_rooms") {
        return;
    }
    app.emit_to(sid, "rooms_list", &st.get_rooms_list());
    app.emit_to(sid, "live_games", &st.get_live_games());
}

// --- Szobák ---

pub fn create_room(app: &Arc<App>, st: &mut ServerState, sid: &str, data: Value) {
    if !app.limiter.check_socket(sid, "create_room") {
        app.emit_error(sid, "Túl sok szoba létrehozás, várj egy kicsit.");
        return;
    }
    let Some(object) = data.as_object() else { return };
    if st.room_id_for_player(sid).is_some() {
        app.emit_error(sid, ALREADY_IN_ROOM);
        return;
    }
    let mut blocked = maintenance_block(app, st, sid);
    let policy = app.settings.get_string("room_creation");
    if blocked.is_none() && policy == "none" && !is_admin_sid(app, st, sid) {
        blocked = Some("Új szoba létrehozása jelenleg le van tiltva.");
    }
    if blocked.is_none() && policy == "registered" && registered_user_id(st, sid).is_none() {
        blocked = Some("Szobát csak regisztrált felhasználó hozhat létre.");
    }
    if let Some(message) = blocked {
        app.emit_error(sid, message);
        return;
    }
    let raw_name = object.get("name").cloned().unwrap_or_else(|| json!(""));
    if raw_name.as_str().is_some_and(|n| moderation::contains_banned(&app.banned_word_list(), n)) {
        app.emit_error(sid, "Ez a szobanév nem engedélyezett.");
        return;
    }

    let name = sanitize_room_name(&raw_name).unwrap_or_else(|| "Szoba".to_string());
    let max_players = object.get("max_players").and_then(util::py_int).unwrap_or(4).clamp(2, 4) as usize;
    let challenge_mode = object.get("challenge_mode").is_some_and(util::truthy);
    let is_private = object.get("is_private").is_some_and(util::truthy);
    let mut turn_time_limit = object.get("turn_time_limit").and_then(util::py_int).unwrap_or(0);
    if !ALLOWED_TURN_TIME_LIMITS.contains(&turn_time_limit) {
        turn_time_limit = 0;
    }
    let default_hint = app.settings.get_i64("default_hint_limit");
    let mut hint_limit = match object.get("hint_limit") {
        None => default_hint,
        Some(v) => util::py_int(v).unwrap_or(default_hint),
    };
    if !ALLOWED_HINT_LIMITS.contains(&hint_limit) {
        hint_limit = default_hint;
    }
    let player_name = st.player_names.get(sid).cloned().unwrap_or_else(|| "Névtelen".to_string());

    // Számítógépes ellenfelek: a nehézségek listája; a robotok is férőhelyet foglalnak
    let mut levels: Vec<ai::Difficulty> = Vec::new();
    if app.settings.get_bool("bots_enabled") {
        if let Some(list) = object.get("ai_players").and_then(|v| v.as_array()) {
            levels = list.iter().filter_map(ai::parse_difficulty).collect();
        }
    }
    levels.truncate(MAX_BOTS.min(app.settings.get_i64("max_bots").max(0) as usize));
    levels.truncate(max_players - 1);

    let room_id = new_room_id();
    let join_code = st.generate_join_code();
    let mut game = Game::new(&room_id, challenge_mode, turn_time_limit, hint_limit);
    let _ = game.add_player(sid, &player_name);
    for level in levels {
        let bot_name = bot_name(&game, Some(level));
        let _ = game.add_bot(&bot_name, &level.to_json());
    }
    let auth = st.player_auth.get(sid).cloned();
    let token = st.generate_reconnect_token(&app.db, sid, &room_id, &player_name, auth.as_ref());
    let room = Room::new(&room_id, game, Some(sid), &player_name, &name, max_players, &join_code, is_private, Some(&token));
    let payload = room_joined_payload(&room, &token, true);
    st.add_room(room);
    st.player_rooms.insert(sid.to_string(), room_id.clone());
    app.join_room(sid, &room_id);

    app.emit_to(sid, "room_joined", &payload);
    app.emit_to(sid, "room_code", &json!({"code": join_code}));
    emit_all_states(app, st, &room_id);
    app.emit_all("rooms_list", &st.get_rooms_list());
}

/// A mentett játék helyét az foglalhatja el, akié volt: regisztrált játékos csak a saját fiókjával, vendég hely
/// pedig csak vendéggel (név alapú azonosításnál nincs más igazolás).
pub fn identity_matches(expected: Identity, auth: Option<&AuthInfo>) -> bool {
    match expected {
        Identity::Any => true,
        Identity::Guest => auth.and_then(|a| a.user_id).filter(|u| *u != 0).is_none(),
        Identity::User(uid) => auth.is_some_and(|a| a.user_id == Some(uid) && !a.is_guest),
    }
}

/// Visszaállított, már elindult játékba a hiányzó játékos utólag becsatlakozik. Visszatér: igaz, ha sikerült (a
/// válaszokat is elküldte).
fn try_late_join(app: &Arc<App>, st: &mut ServerState, sid: &str, room_id: &str, player_name: &str, auth: Option<&AuthInfo>) -> bool {
    let (old_id, expected) = {
        let Some(room) = st.rooms.get(room_id) else { return false };
        let game = &room.game;
        if game.finished {
            return false;
        }
        let Some(expected) = room.late_join_names.get(player_name).copied() else { return false };
        let Some(player) = game.players.iter().find(|p| p.name == player_name && p.disconnected) else { return false };
        (player.id.clone(), expected)
    };
    if !identity_matches(expected, auth) {
        return false;
    }
    {
        let room = st.rooms.get_mut(room_id).expect("létező szoba");
        room.game.replace_player_sid(&old_id, sid);
        room.late_join_names.remove(player_name);
    }
    st.player_rooms.insert(sid.to_string(), room_id.to_string());
    app.join_room(sid, room_id);
    let token = st.generate_reconnect_token(&app.db, sid, room_id, player_name, auth);
    let payload = room_joined_payload(&st.rooms[room_id], &token, false);
    app.emit_to(sid, "room_joined", &payload);
    emit_all_states(app, st, room_id);
    app.emit_to(sid, "game_started", &json!({}));
    app.emit_room(room_id, "player_reconnected", &json!({"name": player_name}));
    app.emit_all("rooms_list", &st.get_rooms_list());
    schedule_bot_turn(app, st, room_id);
    true
}

fn is_six_digits(code: &str) -> bool {
    code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit())
}

pub fn join_room(app: &Arc<App>, st: &mut ServerState, sid: &str, data: Value) {
    if !app.limiter.check_socket(sid, "join_room") {
        app.emit_error(sid, TOO_MANY);
        return;
    }
    let Some(object) = data.as_object() else { return };
    if st.room_id_for_player(sid).is_some() {
        app.emit_error(sid, ALREADY_IN_ROOM);
        return;
    }
    let code = object.get("code").and_then(|c| c.as_str()).map(|c| c.trim().to_string()).unwrap_or_default();
    let room_id: Option<String>;
    let mut by_code = false;
    if !code.is_empty() {
        if !is_six_digits(&code) {
            app.emit_error(sid, "Érvénytelen kód. 6 számjegyű kódot adj meg.");
            return;
        }
        match st.join_codes.get(&code) {
            Some(id) => room_id = Some(id.clone()),
            None => {
                app.emit_error(sid, "Nincs ilyen kódú szoba.");
                return;
            }
        }
        by_code = true;
    } else if let Some(raw) = object.get("room_id").filter(|v| util::truthy(v)) {
        match raw.as_str() {
            Some(id) if id.chars().count() <= 8 => room_id = Some(id.to_string()),
            _ => {
                app.emit_error(sid, "Érvénytelen szoba azonosító.");
                return;
            }
        }
    } else {
        app.emit_error(sid, "Szoba kód vagy azonosító szükséges.");
        return;
    }
    let room_id = room_id.expect("a szoba azonosító be van állítva");
    if !st.rooms.contains_key(&room_id) {
        app.emit_error(sid, "A szoba nem létezik.");
        return;
    }
    let player_name = st.player_names.get(sid).cloned().unwrap_or_else(|| "Névtelen".to_string());
    let auth = st.player_auth.get(sid).cloned();

    let error = {
        let room = &st.rooms[&room_id];
        let game = &room.game;
        if room.is_private && !by_code {
            Some("Ez egy privát szoba. Csatlakozáshoz kód szükséges.")
        } else if room.is_restored && room.expected_players.as_ref().is_some_and(|e| !e.is_empty()) {
            // Restore lobby: csak az elvárt játékosok csatlakozhatnak
            let expected = room.expected_players.as_ref().expect("elvárt játékosok");
            if !expected.contains(&player_name) {
                Some("Csak a mentett játék eredeti játékosai csatlakozhatnak.")
            } else if game.players.iter().any(|p| p.name == player_name) {
                Some("Ezzel a névvel már csatlakozott valaki.")
            } else {
                let ident = match room.known_user_ids.get(&player_name) {
                    None => Identity::Any,
                    Some(uid) => Identity::from_user_id(*uid),
                };
                if identity_matches(ident, auth.as_ref()) { None } else { Some("Ez a hely egy másik játékosé a mentett játékban.") }
            }
        } else {
            None
        }
    };
    if let Some(message) = error {
        app.emit_error(sid, message);
        return;
    }
    if st.rooms[&room_id].game.started {
        if try_late_join(app, st, sid, &room_id, &player_name, auth.as_ref()) {
            return;
        }
        app.emit_error(sid, "A játék már elkezdődött.");
        return;
    }
    {
        let room = &st.rooms[&room_id];
        if room.game.players.len() >= room.max_players {
            app.emit_error(sid, "A szoba megtelt.");
            return;
        }
        if room.game.players.iter().any(|p| util::casefold(&p.name) == util::casefold(&player_name)) {
            app.emit_error(sid, "Ilyen nevű játékos már van a szobában.");
            return;
        }
    }
    if let Err(message) = st.rooms.get_mut(&room_id).expect("létező szoba").game.add_player(sid, &player_name) {
        app.emit_error(sid, &message);
        return;
    }
    st.player_rooms.insert(sid.to_string(), room_id.clone());
    app.join_room(sid, &room_id);
    let token = st.generate_reconnect_token(&app.db, sid, &room_id, &player_name, auth.as_ref());
    let mut payload = room_joined_payload(&st.rooms[&room_id], &token, false);
    {
        let room = &st.rooms[&room_id];
        if room.is_restored {
            payload["is_restore_lobby"] = json!(true);
            payload["expected_players"] = json!(room.expected_players);
        }
    }
    app.emit_to(sid, "room_joined", &payload);
    emit_all_states(app, st, &room_id);
    app.emit_room(&room_id, "player_joined", &json!({"name": player_name}));
    app.emit_all("rooms_list", &st.get_rooms_list());
}

pub fn leave_room(app: &Arc<App>, st: &mut ServerState, sid: &str, _data: Value) {
    let Some(room_id) = st.room_id_for_player(sid) else { return };
    let token = st.get_reconnect_token_for_sid(sid);
    let player_name = st.player_names.get(sid).cloned().unwrap_or_else(|| "?".to_string());
    let auth = st.player_auth.get(sid).cloned();
    let (is_async, is_owner, is_active_game) = {
        let room = &st.rooms[&room_id];
        (room.is_async, room.owner.as_deref() == Some(sid), room.game.started && !room.game.finished)
    };

    // Levelezős játék: a kilépés csak a nézetet zárja be, a játék megmarad, és később folytatható
    if is_async && is_active_game {
        st.rooms.get_mut(&room_id).expect("létező szoba").game.mark_disconnected(sid);
        if let Some(token) = &token {
            st.mark_disconnected(token, sid, &room_id, &player_name, auth);
        }
        app.leave_room(sid, &room_id);
        st.player_rooms.remove(sid);
        app.emit_to(sid, "room_left", &json!({}));
        emit_all_states(app, st, &room_id);
        app.emit_room(&room_id, "player_disconnected", &json!({"name": player_name}));
        return;
    }

    // A tulajdonos kilép egy aktív játékból: mindenkit kiléptetünk, a szoba feloszlik
    if is_owner && is_active_game {
        disband_active_room(app, st, &room_id, "A szoba tulajdonosa kilépett, a játék véget ért.", Some(sid));
        // A tulajdonos eltávolítása (a szoba már nincs, de a játékosnyilvántartás tisztítása a hívóra marad)
        app.leave_room(sid, &room_id);
        st.player_rooms.remove(sid);
        st.cleanup_player_token(sid);
        app.emit_to(sid, "room_left", &json!({}));
        app.emit_all("rooms_list", &st.get_rooms_list());
        return;
    }

    // Normál kilépés (nem a tulajdonos, vagy nem aktív a játék)
    if is_active_game && token.is_some() {
        // Aktív játékban csak lecsatlakozottnak jelöljük, nem töröljük
        st.rooms.get_mut(&room_id).expect("létező szoba").game.mark_disconnected(sid);
        st.mark_disconnected(token.as_deref().expect("token"), sid, &room_id, &player_name, auth);
        // Ha az ő köre volt, léptetjük
        advance_turn_if_needed(app, st, &room_id);
        emit_all_states(app, st, &room_id);
        app.emit_room(&room_id, "player_left", &json!({"name": player_name}));
        app.leave_room(sid, &room_id);
        st.player_rooms.remove(sid);
    } else {
        st.rooms.get_mut(&room_id).expect("létező szoba").game.remove_player(sid);
        st.cleanup_player_token(sid);
        app.leave_room(sid, &room_id);
        st.player_rooms.remove(sid);
        let (no_humans, was_owner) = {
            let room = &st.rooms[&room_id];
            (
                room.game.human_players().is_empty() || (room.is_async && !room.game.has_connected_human()),
                room.owner.as_deref() == Some(sid),
            )
        };
        if no_humans {
            cleanup_room(app, st, &room_id);
        } else {
            if was_owner {
                transfer_ownership(app, st, &room_id);
            }
            emit_all_states(app, st, &room_id);
            app.emit_room(&room_id, "player_left", &json!({"name": player_name}));
        }
    }
    app.emit_to(sid, "room_left", &json!({}));
    app.emit_all("rooms_list", &st.get_rooms_list());
}

pub fn start_game(app: &Arc<App>, st: &mut ServerState, sid: &str, _data: Value) {
    let Some(room_id) = st.room_id_for_player(sid) else { return };
    if !is_owner(st, &st.rooms[&room_id], sid) {
        app.emit_error(sid, "Csak a szoba tulajdonosa indíthatja a játékot.");
        return;
    }

    // Restore lobby: visszaállítás mentett állapotból
    if st.rooms[&room_id].restore_save_data.is_some() {
        restore_start(app, st, &room_id, sid);
        return;
    }

    let started = st.rooms.get_mut(&room_id).expect("létező szoba").game.start();
    if let Err(message) = started {
        app.emit_error(sid, &message);
        return;
    }
    st.remove_invites_for_room(&room_id);
    start_turn_timer(app, st, &room_id, None);
    emit_all_states(app, st, &room_id);
    app.emit_room(&room_id, "game_started", &json!({}));
    app.emit_all("rooms_list", &st.get_rooms_list());
    app.emit_all("live_games", &st.get_live_games());
    schedule_bot_turn(app, st, &room_id);
    // Kezdeti mentés, hogy a játékos lista perzisztens legyen
    save_game_to_db(app, st, &room_id);
}

/// A mentett játék visszaállítása a várakozó szobából indításkor.
fn restore_start(app: &Arc<App>, st: &mut ServerState, room_id: &str, sid: &str) {
    use crate::db::RowExt;
    let Some(save) = st.rooms[room_id].restore_save_data.clone() else { return };
    let save_id = save.int("id");
    let state: Value = match serde_json::from_str(&save.text("state_json")) {
        Ok(state) => state,
        Err(e) => {
            println!("[restore] Hiba a visszaállításnál: {e}");
            app.emit_error(sid, "Hiba a játék visszaállításánál.");
            return;
        }
    };
    let mut restored = match Game::from_save_dict(&state) {
        Ok(game) => game,
        Err(e) => {
            println!("[restore] Hiba a visszaállításnál: {e}");
            app.emit_error(sid, "Hiba a játék visszaállításánál.");
            return;
        }
    };
    // Összepárosítás: a várakozó szobában lévő játékosok + a mentett játékosok
    let joined: HashMap<String, String> = st.rooms[room_id].game.players.iter().map(|p| (p.name.clone(), p.id.clone())).collect();
    for player in &mut restored.players {
        if player.is_bot {
            player.disconnected = false;
        } else if let Some(current) = joined.get(&player.name) {
            player.id = current.clone();
            player.disconnected = false;
        } else {
            // Aki nincs ott, azt lecsatlakozottnak jelöljük, de nem töröljük
            player.disconnected = true;
        }
    }
    if !restored.human_players().iter().any(|p| !p.disconnected) {
        app.emit_error(sid, "Nincs csatlakozott játékos a mentett játékból.");
        return;
    }
    if restored.current_player_idx >= restored.players.len() {
        restored.current_player_idx = 0;
    }
    // Ha a soron lévő játékos nincs jelen, ne akadjon el a játék rajta
    restored.skip_disconnected_current();
    // A korábbi lépések átvétele, hogy a visszajátszás a teljes játékot mutassa
    restored.move_log = app
        .db
        .get_game_moves(save_id)
        .unwrap_or_default()
        .iter()
        .map(|m| MoveLog::from_stored(m.move_number, &m.player_name, &m.action_type, m.details_json.as_deref(), m.board_snapshot_json.as_deref()))
        .collect();
    {
        let room = st.rooms.get_mut(room_id).expect("létező szoba");
        room.last_saved_move_count = 0;
        // A hiányzók menet közben még becsatlakozhatnak (név + fiók alapján)
        room.late_join_names = restored
            .players
            .iter()
            .filter(|p| p.disconnected)
            .map(|p| {
                let ident = match room.known_user_ids.get(&p.name) {
                    None => Identity::Any,
                    Some(uid) => Identity::from_user_id(*uid),
                };
                (p.name.clone(), ident)
            })
            .collect();
        room.game = restored;
        room.restore_save_data = None;
        room.is_restored = false;
        // A régi mentést lezárjuk: most már egy új szobában játszunk
        room.db_game_id = None;
    }
    app.db.abandon_game_by_id(save_id);

    start_turn_timer(app, st, room_id, None);
    emit_all_states(app, st, room_id);
    app.emit_room(room_id, "game_started", &json!({}));
    app.emit_all("rooms_list", &st.get_rooms_list());
    app.emit_all("live_games", &st.get_live_games());
    schedule_bot_turn(app, st, room_id);
    // Új mentés az új szobához (játékoslista, lépések) — így nem vész el a játék
    save_game_to_db(app, st, room_id);
}

/// Manuális mentés — csak a szoba tulajdonosa használhatja.
pub fn save_game(app: &Arc<App>, st: &mut ServerState, sid: &str, _data: Value) {
    let Some(room_id) = room_context(app, st, sid, "save_game") else { return };
    if !is_owner(st, &st.rooms[&room_id], sid) {
        action_result(app, sid, false, "Csak a szoba tulajdonosa menthet.");
        return;
    }
    {
        let game = &st.rooms[&room_id].game;
        if !game.started || game.finished {
            action_result(app, sid, false, "Nincs aktív játék a mentéshez.");
            return;
        }
    }
    let (success, message) = save_game_to_db(app, st, &room_id);
    if success {
        if let Some(room) = st.rooms.get_mut(&room_id) {
            room.manually_saved = true;
        }
    }
    action_result(app, sid, success, &message);
}

/// Mentett játék visszaállítása: várakozó szoba létrehozása.
pub fn restore_game(app: &Arc<App>, st: &mut ServerState, sid: &str, data: Value) {
    use crate::db::RowExt;
    if !app.limiter.check_socket(sid, "restore_game") {
        app.emit_error(sid, TOO_MANY);
        return;
    }
    let Some(object) = data.as_object() else { return };
    let Some(game_id) = object.get("game_id").and_then(util::strict_int) else {
        app.emit_error(sid, "Érvénytelen játék azonosító.");
        return;
    };
    if st.room_id_for_player(sid).is_some() {
        app.emit_error(sid, ALREADY_IN_ROOM);
        return;
    }
    if let Some(message) = maintenance_block(app, st, sid) {
        app.emit_error(sid, message);
        return;
    }
    // Auth ellenőrzés
    let Some(user_id) = st.player_auth.get(sid).and_then(|a| a.user_id).filter(|u| *u != 0) else {
        app.emit_error(sid, "Csak regisztrált felhasználók állíthatnak vissza játékot.");
        return;
    };
    let Some(row) = app.db.get_game_by_id(game_id).ok().flatten() else {
        app.emit_error(sid, "Mentett játék nem található.");
        return;
    };
    if row.text("status") != "active" {
        app.emit_error(sid, "A mentett játék nem aktív.");
        return;
    }
    if row.flag("is_async") {
        app.emit_error(sid, "A levelezős játékot a Levelezős fülről nyithatod meg.");
        return;
    }
    // Ellenőrzés: a hívó játékos részese-e a játéknak
    if !app.db.is_user_in_game(game_id, user_id) {
        app.emit_error(sid, "Nem vagy részese ennek a mentett játéknak.");
        return;
    }
    // Owner ellenőrzés: csak az eredeti owner állíthat vissza
    let player_name = st.player_names.get(sid).cloned().unwrap_or_else(|| "Névtelen".to_string());
    let saved_owner = row.text("owner_name");
    if !saved_owner.is_empty() && saved_owner != player_name {
        app.emit_error(sid, "Csak a mentés tulajdonosa állíthatja vissza a játékot.");
        return;
    }
    let Ok(saved_state) = serde_json::from_str::<Value>(&row.text("state_json")) else {
        app.emit_error(sid, "Hibás mentett állapot.");
        return;
    };
    let saved_players = saved_state.get("players").and_then(|p| p.as_array()).cloned().unwrap_or_default();
    let expected: Vec<String> = saved_players
        .iter()
        .filter(|p| !p.get("is_bot").is_some_and(util::truthy))
        .filter_map(|p| p.get("name").and_then(|n| n.as_str()).map(|n| n.to_string()))
        .collect();
    if expected.is_empty() {
        app.emit_error(sid, "Nincs játékos a mentett játékban.");
        return;
    }

    // Várakozó szoba létrehozása
    let room_id = new_room_id();
    let join_code = st.generate_join_code();
    let challenge_mode = saved_state.get("challenge_mode").is_some_and(util::truthy);
    let hint_limit = saved_state.get("hint_limit").and_then(util::py_int).unwrap_or(DEFAULT_HINT_LIMIT);
    let mut new_game = Game::new(&room_id, challenge_mode, 0, hint_limit);
    let _ = new_game.add_player(sid, &player_name);
    let auth = st.player_auth.get(sid).cloned();
    let token = st.generate_reconnect_token(&app.db, sid, &room_id, &player_name, auth.as_ref());
    let room_name = row.opt_text("room_name").filter(|n| !n.is_empty()).unwrap_or_else(|| "Visszaállított szoba".to_string());
    let mut room = Room::new(&room_id, new_game, Some(sid), &player_name, &room_name, saved_players.len().max(2), &join_code, true, Some(&token));
    room.is_restored = true;
    room.restore_save_data = Some(row);
    room.expected_players = Some(expected.clone());
    room.known_user_ids = app.db.get_game_players(game_id).unwrap_or_default().into_iter().map(|(name, uid, _)| (name, uid)).collect();
    let payload = json!({
        "room_id": room_id,
        "room_name": room.name,
        "is_owner": true,
        "challenge_mode": room.game.challenge_mode,
        "is_private": true,
        "reconnect_token": token,
        "is_restore_lobby": true,
        "expected_players": expected,
        "hint_limit": room.game.hint_limit,
        "chat_messages": room.chat_messages,
    });
    st.add_room(room);
    st.player_rooms.insert(sid.to_string(), room_id.clone());
    app.join_room(sid, &room_id);
    app.emit_to(sid, "room_joined", &payload);
    app.emit_to(sid, "room_code", &json!({"code": join_code}));
    emit_all_states(app, st, &room_id);
    app.emit_all("rooms_list", &st.get_rooms_list());
}

pub(super) fn new_id() -> String {
    new_room_id()
}
