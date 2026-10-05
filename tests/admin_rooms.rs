//! Admin panel: élő szobák — lista, részletek, beavatkozások; a játék közben sem romolhat el semmi (a Python
//! `test_admin_rooms.py` megfelelője).

mod common;
use common::admin::*;
use common::*;
use scrabble::admin::live::stuck_reason;
use scrabble::tiles::TILE_DISTRIBUTION;
use scrabble::util;
use serde_json::{Value, json};

const REASON: &str = "Teszt beavatkozás";

struct Duo {
    server: TestServer,
    api: Http,
    anna: Sio,
    bela: Sio,
    id: String,
    code: String,
}

async fn start() -> (TestServer, Http) {
    let server = TestServer::start().await;
    // a robotok a tesztek alatt ne lépjenek maguktól
    set_setting(&server, "bot_think_multiplier", json!(5.0));
    let api = server.admin_http().await;
    (server, api)
}

async fn duo_with(options: Value) -> Duo {
    let (server, api) = start().await;
    let anna = server.guest("Anna").await;
    let bela = server.guest("Béla").await;
    let (id, code) = live_room(&server, &anna, &[&bela], options).await;
    Duo { server, api, anna, bela, id, code }
}

async fn duo() -> Duo {
    duo_with(json!({})).await
}

async fn action(api: &Http, room_id: &str, body: Value) -> Resp {
    let mut payload = json!({"reason": REASON});
    for (k, v) in body.as_object().unwrap() {
        payload[k] = v.clone();
    }
    api.admin_post(&format!("/rooms/{room_id}/action"), payload).await
}

/// A zsákban, a kezekben és a táblán lévő zsetonok száma: 100 kell legyen.
fn tile_total(server: &TestServer) -> usize {
    with_room(server, |room| {
        let game = &room.game;
        let on_board = game.board.cells.iter().flatten().filter(|c| c.is_some()).count();
        let pending = game.pending_challenge.as_ref().map(|c| c.removed_from_hand.len()).unwrap_or(0);
        game.bag.remaining() + game.players.iter().map(|p| p.hand.len()).sum::<usize>() + on_board + pending
    })
}

/// Az első lerakás két zsetonnal (megtámadásos módban a szótár nem számít).
async fn first_move(server: &TestServer, client: &Sio, player_name: &str) {
    let tiles: Vec<Value> = with_room(server, |room| {
        let player = room.game.players.iter().find(|p| p.name == player_name).unwrap();
        player.hand.iter().filter(|t| !t.is_blank()).take(2).enumerate().map(|(i, t)| json!({"row": 7, "col": 7 + i, "letter": t.as_str(), "is_blank": false})).collect()
    });
    client.call("place_tiles", json!({"tiles": tiles})).await;
}

fn audit(server: &TestServer, action: &str) -> Vec<Value> {
    audit_of(server, action)
}

fn room_names(response: &Resp) -> Vec<String> {
    items_of(response, "name").iter().map(|n| n.as_str().unwrap().to_string()).collect()
}

fn names_of(room_players: &Value) -> Vec<String> {
    room_players.as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap().to_string()).collect()
}

// ===================================================================================================
// Lista
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn lists_rooms_with_their_state() {
    let d = duo().await;
    let data = d.api.admin_get("/rooms").await.json();
    assert_eq!(data["total"], 1);
    let row = &data["items"][0];
    assert_eq!((row["code"].clone(), row["status"].clone(), row["kind"].clone()), (json!(d.code), json!("playing"), json!("game")));
    assert_eq!(names_of(&row["players"]), vec!["Anna", "Béla"]);
    assert_eq!(row["owner"], "Anna");
    assert_eq!(row["players"][0]["online"], true);
    assert!(row["stuck"].is_null());
}

#[tokio::test(flavor = "multi_thread")]
async fn room_filters() {
    let d = duo().await;
    let waiting = d.server.guest("Cili").await;
    waiting.call("create_room", json!({"name": "Várakozó", "max_players": 4})).await;
    let names = |q: String| {
        let api = d.api.clone();
        async move { room_names(&api.admin_get(&format!("/rooms{q}")).await) }
    };
    assert_eq!(names("?status=waiting".into()).await, vec!["Várakozó"]);
    assert_eq!(names("?status=playing".into()).await, vec!["Teszt szoba"]);
    assert!(names("?kind=async".into()).await.is_empty());
    assert!(names("?bots=1".into()).await.is_empty());
    assert_eq!(names("?q=cili".into()).await, vec!["Várakozó"]);
    assert_eq!(names(format!("?q={}", d.code)).await, vec!["Teszt szoba"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn robots_show_their_level() {
    let (server, api) = start().await;
    let solo = server.guest("Anna").await;
    live_room(&server, &solo, &[], json!({"ai_players": [7], "max_players": 2})).await;
    let row = api.admin_get("/rooms?bots=1").await.json()["items"][0].clone();
    assert_eq!(row["has_bots"], true);
    let levels: Vec<Value> = row["players"].as_array().unwrap().iter().filter(|p| p["is_bot"] == true).map(|p| p["difficulty"].clone()).collect();
    assert_eq!(levels, vec![json!(7)]);
}

#[tokio::test(flavor = "multi_thread")]
async fn rooms_csv() {
    let d = duo().await;
    let text = d.api.admin_get("/rooms?format=csv").await.text();
    assert!(text.lines().next().unwrap().starts_with("code,id,name"), "{text}");
    assert!(text.contains("Anna; Béla"), "{text}");
}

// ===================================================================================================
// Részletek
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn the_detail_has_no_hands() {
    let d = duo().await;
    let data = d.api.admin_get(&format!("/rooms/{}", d.code)).await.json()["room"].clone();
    assert_eq!(data["id"], json!(d.id));
    assert_eq!(data["board"].as_array().unwrap().len(), 15);
    let bag: i64 = data["bag"].as_object().unwrap().values().map(|v| v.as_i64().unwrap()).sum();
    assert_eq!(bag, 100 - 14);
    assert_eq!(data["bag_count"], 100 - 14);
    assert!(data["players"].as_array().unwrap().iter().all(|p| p.get("hand").is_none())); // csak a darabszám látszik
    assert!(audit(&d.server, "view.racks").is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn racks_are_logged() {
    let d = duo().await;
    let racks = d.api.admin_get(&format!("/rooms/{}/racks", d.id)).await.json()["racks"].clone();
    assert_eq!(racks.as_array().unwrap().iter().map(|r| r["name"].clone()).collect::<Vec<_>>(), vec![json!("Anna"), json!("Béla")]);
    assert!(racks.as_array().unwrap().iter().all(|r| r["hand"].as_array().unwrap().len() == 7));
    let row = audit(&d.server, "view.racks").remove(0);
    assert_eq!((row["target_id"].clone(), row["details"]["code"].clone()), (json!(d.id), json!(d.code)));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_room() {
    let (_server, api) = start().await;
    assert_eq!(api.admin_get("/rooms/nincs").await.status, 404);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_chat_has_timestamps() {
    let d = duo().await;
    d.anna.call("send_chat", json!({"message": "Szia"})).await;
    let chat = d.api.admin_get(&format!("/rooms/{}", d.id)).await.json()["room"]["chat"].clone();
    assert_eq!((chat[0]["name"].clone(), chat[0]["message"].clone()), (json!("Anna"), json!("Szia")));
    assert!((chat[0]["ts"].as_f64().unwrap() - util::now()).abs() < 60.0);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_pending_vote_state() {
    let d = duo_with(json!({"challenge_mode": true})).await;
    first_move(&d.server, &d.anna, "Anna").await;
    let data = d.api.admin_get(&format!("/rooms/{}", d.id)).await.json()["room"].clone();
    assert_eq!(data["status"], "voting");
    assert_eq!(data["pending"]["player"], "Anna");
    assert_eq!((data["pending"]["voters"].clone(), data["pending"]["votes"].clone()), (json!(["Béla"]), json!({})));
}

// ===================================================================================================
// Rendszerüzenet
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn a_message_reaches_everyone_and_the_history() {
    let d = duo().await;
    let response = action(&d.api, &d.id, json!({"action": "message", "message": "Figyelem!"})).await;
    assert_eq!(response.status, 200);
    d.anna.settle().await;
    for client in [&d.anna, &d.bela] {
        let messages: Vec<Value> = client.take().into_iter().filter(|(n, _)| n == "chat_message").map(|(_, d)| d).collect();
        assert_eq!(messages, vec![json!({"name": "Rendszer", "message": "Figyelem!", "system": true})]);
    }
    assert_eq!(with_room(&d.server, |room| room.chat_messages.last().cloned()), Some(json!({"name": "Rendszer", "message": "Figyelem!", "system": true})));
    assert_eq!(audit(&d.server, "room.message")[0]["details"]["message"], "Figyelem!");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_empty_or_long_message() {
    let d = duo().await;
    for text in ["".to_string(), "   ".to_string(), "x".repeat(201)] {
        assert_eq!(action(&d.api, &d.id, json!({"action": "message", "message": text})).await.status, 400);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn nobody_can_pose_as_the_system() {
    let server = TestServer::start().await;
    let _client = server.guest("Rendszer").await;
    let st = server.app.state.lock();
    assert_eq!(st.player_names.values().next().map(|s| s.as_str()), Some("Névtelen"));
}

// ===================================================================================================
// Minden beavatkozás naplózott
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn the_reason_is_required_for_every_intervention() {
    let bodies = [
        json!({"action": "kick", "player": "Béla"}),
        json!({"action": "transfer", "player": "Béla"}),
        json!({"action": "skip"}),
        json!({"action": "save"}),
        json!({"action": "extend", "seconds": 60}),
        json!({"action": "pause"}),
        json!({"action": "resume"}),
        json!({"action": "reschedule_bot"}),
        json!({"action": "resolve_vote", "accept": true}),
    ];
    let d = duo_with(json!({"turn_time_limit": 60})).await;
    for body in bodies {
        let before = audit_rows(&d.server).len();
        let response = d.api.admin_post(&format!("/rooms/{}/action", d.id), body.clone()).await;
        assert_eq!(response.status, 400, "{body}");
        assert_eq!(response.json()["field"], "reason", "{body}");
        assert_eq!(audit_rows(&d.server).len(), before, "{body}");
        assert_eq!(with_room(&d.server, |room| room.game.players.iter().map(|p| p.name.clone()).collect::<Vec<_>>()), vec!["Anna", "Béla"]);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_action() {
    let d = duo().await;
    assert_eq!(action(&d.api, &d.id, json!({"action": "robbants"})).await.status, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_heaviest_actions_need_sudo() {
    let d = duo().await;
    for name in ["end", "void", "disband"] {
        let response = action(&d.api, &d.id, json!({"action": name})).await;
        assert_eq!(response.status, 401, "{name}");
        assert_eq!(response.json()["sudo_required"], true);
    }
    assert!(d.server.app.state.lock().rooms.contains_key(&d.id));
    assert!(!with_room(&d.server, |room| room.game.finished));
}

// ===================================================================================================
// Kirúgás
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn kicking_from_a_running_game() {
    let (server, api) = start().await;
    let (anna, bela, cili) = (server.guest("Anna").await, server.guest("Béla").await, server.guest("Cili").await);
    let (id, _) = live_room(&server, &anna, &[&bela, &cili], json!({})).await;
    let token = {
        let st = server.app.state.lock();
        let sid = st.rooms[&id].game.players.iter().find(|p| p.name == "Béla").unwrap().id.clone();
        st.get_reconnect_token_for_sid(&sid).expect("token")
    };
    assert_eq!(action(&api, &id, json!({"action": "kick", "player": "Béla"})).await.status, 200);
    bela.settle().await;
    assert!(with_room(&server, |room| room.game.players.iter().find(|p| p.name == "Béla").unwrap().disconnected));
    assert!(server.app.state.lock().get_token_info(&token).is_none()); // a token érvénytelen
    let events: Vec<String> = bela.take().into_iter().map(|(n, _)| n).collect();
    assert!(events.contains(&"room_left".to_string()) && events.contains(&"error".to_string()), "{events:?}");
    bela.settle().await;
    assert!(bela.take().is_empty());
    assert_eq!(tile_total(&server), 100);
    assert_eq!(audit(&server, "room.kick")[0]["details"]["player"], "Béla");
}

#[tokio::test(flavor = "multi_thread")]
async fn kicking_the_current_player_passes_the_turn() {
    let (server, api) = start().await;
    let (anna, bela, cili) = (server.guest("Anna").await, server.guest("Béla").await, server.guest("Cili").await);
    let (id, _) = live_room(&server, &anna, &[&bela, &cili], json!({})).await;
    assert_eq!(with_room(&server, |room| room.game.current_player().unwrap().name.clone()), "Anna");
    action(&api, &id, json!({"action": "kick", "player": "Anna"})).await;
    assert_eq!(with_room(&server, |room| room.game.current_player().unwrap().name.clone()), "Béla");
    let owner = with_room(&server, |room| room.owner_name.clone());
    assert!(["Béla", "Cili"].contains(&owner.as_str()), "{owner}"); // a tulajdonjog továbbszállt
}

#[tokio::test(flavor = "multi_thread")]
async fn kicking_from_a_waiting_room_removes_the_player() {
    let (server, api) = start().await;
    let (anna, bela) = (server.guest("Anna").await, server.guest("Béla").await);
    anna.emit("create_room", json!({"name": "Vár", "max_players": 4}));
    let code = anna.wait_code().await;
    bela.call("join_room", json!({"code": code})).await;
    let id = server.app.state.lock().rooms.keys().next().cloned().unwrap();
    action(&api, &id, json!({"action": "kick", "player": "Béla"})).await;
    assert_eq!(with_room(&server, |room| room.game.players.iter().map(|p| p.name.clone()).collect::<Vec<_>>()), vec!["Anna"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn kicking_the_last_human_closes_the_room() {
    let d = duo().await;
    action(&d.api, &d.id, json!({"action": "kick", "player": "Béla"})).await;
    action(&d.api, &d.id, json!({"action": "kick", "player": "Anna"})).await;
    assert!(!d.server.app.state.lock().rooms.contains_key(&d.id));
}

#[tokio::test(flavor = "multi_thread")]
async fn kicking_an_unknown_player_or_a_bot() {
    let (server, api) = start().await;
    let anna = server.guest("Anna").await;
    let (id, _) = live_room(&server, &anna, &[], json!({"ai_players": [3], "max_players": 2})).await;
    assert_eq!(action(&api, &id, json!({"action": "kick", "player": "Senki"})).await.status, 404);
    let bot = with_room(&server, |room| room.game.players.iter().find(|p| p.is_bot).unwrap().name.clone());
    assert_eq!(action(&api, &id, json!({"action": "kick", "player": bot})).await.status, 404);
}

// ===================================================================================================
// Tulajdonjog
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn transferring_the_ownership() {
    let d = duo().await;
    assert_eq!(action(&d.api, &d.id, json!({"action": "transfer", "player": "Béla"})).await.status, 200);
    let (owner_name, owner, bela_id) = with_room(&d.server, |room| (room.owner_name.clone(), room.owner.clone(), room.game.players.iter().find(|p| p.name == "Béla").unwrap().id.clone()));
    assert_eq!(owner_name, "Béla");
    assert_eq!(owner, Some(bela_id));
    d.bela.settle().await;
    let codes: Vec<Value> = d.bela.take().into_iter().filter(|(n, _)| n == "room_code").map(|(_, d)| d).collect();
    assert_eq!(codes, vec![json!({"code": d.code})]);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_offline_player_cannot_be_the_owner() {
    let d = duo().await;
    d.bela.close();
    d.bela.settle_long().await;
    assert_eq!(action(&d.api, &d.id, json!({"action": "transfer", "player": "Béla"})).await.status, 409);
}

// ===================================================================================================
// Kör átugrása
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn skipping_marks_the_move_and_advances() {
    let d = duo().await;
    assert_eq!(action(&d.api, &d.id, json!({"action": "skip"})).await.status, 200);
    assert_eq!(with_room(&d.server, |room| room.game.current_player().unwrap().name.clone()), "Béla");
    let (kind, details) = with_room(&d.server, |room| {
        let last = room.game.move_log.last().unwrap();
        (last.action_type.clone(), last.details.clone())
    });
    assert_eq!(kind, "pass");
    assert_eq!(details["admin"], true);
    assert_eq!(tile_total(&d.server), 100);
    d.bela.settle().await;
    let bela_id = with_room(&d.server, |room| room.game.players.iter().find(|p| p.name == "Béla").unwrap().id.clone());
    assert_eq!(d.bela.state()["current_player"], json!(bela_id));
}

#[tokio::test(flavor = "multi_thread")]
async fn skipping_is_refused_during_a_vote() {
    let d = duo_with(json!({"challenge_mode": true})).await;
    first_move(&d.server, &d.anna, "Anna").await;
    assert_eq!(action(&d.api, &d.id, json!({"action": "skip"})).await.status, 409);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_next_bot_is_scheduled_after_a_skip() {
    let (server, api) = start().await;
    let anna = server.guest("Anna").await;
    let (id, _) = live_room(&server, &anna, &[], json!({"ai_players": [3], "max_players": 2})).await;
    action(&api, &id, json!({"action": "skip"})).await;
    let (is_bot, scheduled) = with_room(&server, |room| (room.game.current_player().unwrap().is_bot, room.bot_scheduled_at));
    assert!(is_bot);
    assert!(scheduled.is_some_and(|s| util::now() - s < 5.0), "{scheduled:?}");
}

// ===================================================================================================
// Időzítő
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn extend_pause_and_resume() {
    let d = duo_with(json!({"turn_time_limit": 60})).await;
    let (first_id, expires) = with_room(&d.server, |room| (room.turn_timer_id(), room.turn_timer_expires_at.unwrap()));
    let remaining = expires - util::now();
    assert!(remaining > 55.0 && remaining <= 60.0, "{remaining}");
    assert_eq!(action(&d.api, &d.id, json!({"action": "extend", "seconds": 120})).await.status, 200);
    let (expires, id) = with_room(&d.server, |room| (room.turn_timer_expires_at.unwrap(), room.turn_timer_id()));
    assert!(expires - util::now() > remaining + 100.0 && id != first_id);
    assert_eq!(action(&d.api, &d.id, json!({"action": "pause"})).await.status, 200);
    let (expires, left) = with_room(&d.server, |room| (room.turn_timer_expires_at, room.timer_paused_left));
    assert!(expires.is_none() && left.unwrap() > 100.0);
    assert_eq!(action(&d.api, &d.id, json!({"action": "pause"})).await.status, 409);
    assert_eq!(d.api.admin_get(&format!("/rooms/{}", d.id)).await.json()["room"]["paused"], true);
    assert_eq!(action(&d.api, &d.id, json!({"action": "resume"})).await.status, 200);
    let (left, expires) = with_room(&d.server, |room| (room.timer_paused_left, room.turn_timer_expires_at.unwrap()));
    assert!(left.is_none() && expires - util::now() > 100.0);
    assert_eq!(action(&d.api, &d.id, json!({"action": "resume"})).await.status, 409);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_move_while_paused_starts_a_fresh_timer() {
    let d = duo_with(json!({"turn_time_limit": 60})).await;
    action(&d.api, &d.id, json!({"action": "pause"})).await;
    d.anna.call("pass_turn", Value::Null).await;
    let (left, expires) = with_room(&d.server, |room| (room.timer_paused_left, room.turn_timer_expires_at));
    assert!(left.is_none() && expires.is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn no_timer_means_no_extension() {
    let d = duo().await;
    assert_eq!(action(&d.api, &d.id, json!({"action": "extend", "seconds": 60})).await.status, 409);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_invalid_extension() {
    let d = duo_with(json!({"turn_time_limit": 60})).await;
    assert_eq!(action(&d.api, &d.id, json!({"action": "extend", "seconds": 7})).await.status, 400);
}

// ===================================================================================================
// Szavazás lezárása
// ===================================================================================================

async fn voting() -> Duo {
    let d = duo_with(json!({"challenge_mode": true})).await;
    first_move(&d.server, &d.anna, "Anna").await;
    assert!(with_room(&d.server, |room| room.game.pending_challenge.is_some()));
    d
}

#[tokio::test(flavor = "multi_thread")]
async fn a_forced_acceptance() {
    let d = voting().await;
    assert_eq!(action(&d.api, &d.id, json!({"action": "resolve_vote", "accept": true})).await.status, 200);
    let (pending, cell, current) = with_room(&d.server, |room| (room.game.pending_challenge.is_some(), room.game.board.cells[7][7].is_some(), room.game.current_player().unwrap().name.clone()));
    assert!(!pending && cell);
    assert_eq!(current, "Béla");
    assert_eq!(tile_total(&d.server), 100);
    d.bela.settle().await;
    let results: Vec<Value> = d.bela.take().into_iter().filter(|(n, _)| n == "challenge_result").map(|(_, d)| d["challenge_won"].clone()).collect();
    assert_eq!(results, vec![json!(false)]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_forced_rejection_returns_the_tiles() {
    let d = voting().await;
    assert_eq!(action(&d.api, &d.id, json!({"action": "resolve_vote", "accept": false})).await.status, 200);
    let (pending, cell, hand) = with_room(&d.server, |room| (room.game.pending_challenge.is_some(), room.game.board.cells[7][7].is_some(), room.game.players.iter().find(|p| p.name == "Anna").unwrap().hand.len()));
    assert!(!pending && !cell);
    assert_eq!(hand, 7);
    assert_eq!(tile_total(&d.server), 100);
}

#[tokio::test(flavor = "multi_thread")]
async fn nothing_to_resolve() {
    let d = duo().await;
    assert_eq!(action(&d.api, &d.id, json!({"action": "resolve_vote", "accept": true})).await.status, 409);
}

#[tokio::test(flavor = "multi_thread")]
async fn resolving_needs_a_boolean() {
    let d = voting().await;
    assert_eq!(action(&d.api, &d.id, json!({"action": "resolve_vote", "accept": "igen"})).await.status, 400);
}

// ===================================================================================================
// Robotok
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn rescheduling_a_bot() {
    let (server, api) = start().await;
    let anna = server.guest("Anna").await;
    let (id, _) = live_room(&server, &anna, &[], json!({"ai_players": [3], "max_players": 2})).await;
    assert_eq!(action(&api, &id, json!({"action": "reschedule_bot"})).await.status, 409); // ember következik
    with_room(&server, |room| {
        let current = room.game.current_player().unwrap().id.clone();
        room.game.pass_turn(&current, false).unwrap();
        room.bot_scheduled_at = None;
    });
    assert_eq!(action(&api, &id, json!({"action": "reschedule_bot"})).await.status, 200);
    assert!(with_room(&server, |room| room.bot_scheduled_at).is_some_and(|s| util::now() - s < 5.0));
    assert!(!audit(&server, "room.reschedule_bot").is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stuck_bot_is_flagged() {
    let (server, api) = start().await;
    let anna = server.guest("Anna").await;
    let (id, _) = live_room(&server, &anna, &[], json!({"ai_players": [3], "max_players": 2})).await;
    let reason = |server: &TestServer| with_room(server, |room| stuck_reason(room, util::now()));
    with_room(&server, |room| {
        let current = room.game.current_player().unwrap().id.clone();
        room.game.pass_turn(&current, false).unwrap(); // a robot jön, de nincs ütemezett lépése
        room.bot_scheduled_at = None;
        room.invalidate_bot_turn();
    });
    assert_eq!(reason(&server), Some("bot_idle"));
    with_room(&server, |room| room.bot_scheduled_at = Some(util::now()));
    assert_eq!(reason(&server), None);
    with_room(&server, |room| room.bot_scheduled_at = Some(util::now() - 600.0));
    assert_eq!(reason(&server), Some("bot_idle"));
    let flagged = items_of(&api.admin_get("/rooms?stuck=1").await, "id");
    assert_eq!(flagged, vec![json!(id)]);
    let alerts = api.admin_get("/overview").await.json()["alerts"].clone();
    assert!(alerts.as_array().unwrap().iter().any(|a| a["code"] == "stuck_games"), "{alerts}");
}

// ===================================================================================================
// Elakadás-érzékelés
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn an_overdue_timer_and_idle_detection() {
    let d = duo().await;
    let reason = |server: &TestServer| with_room(server, |room| stuck_reason(room, util::now()));
    assert_eq!(reason(&d.server), None);
    with_room(&d.server, |room| {
        room.game.turn_time_limit = 60;
        room.turn_timer_expires_at = Some(util::now() - 600.0);
    });
    assert_eq!(reason(&d.server), Some("timer_overdue"));
    with_room(&d.server, |room| {
        room.turn_timer_expires_at = None;
        room.last_activity = util::now() - 600.0;
    });
    assert_eq!(reason(&d.server), Some("no_timer"));
    with_room(&d.server, |room| room.timer_paused_left = Some(30.0)); // a szüneteltetett időzítő nem elakadás
    assert_eq!(reason(&d.server), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_long_idle_game_without_a_time_limit() {
    let d = duo().await;
    with_room(&d.server, |room| room.last_activity = util::now() - 3600.0);
    assert_eq!(with_room(&d.server, |room| stuck_reason(room, util::now())), Some("idle"));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_overdue_challenge() {
    let d = voting().await;
    with_room(&d.server, |room| room.game.pending_challenge.as_mut().unwrap().expires_at = util::now() - 600.0);
    assert_eq!(with_room(&d.server, |room| stuck_reason(room, util::now())), Some("vote_overdue"));
}

// ===================================================================================================
// Mentés, befejezés, érvénytelenítés, feloszlatás
// ===================================================================================================

async fn registered_duo() -> (TestServer, Http, Sio, Sio, i64, i64, String) {
    let (server, api) = start().await;
    let a_id = server.create_user("anna@example.com", "Anna");
    let b_id = server.create_user("bela@example.com", "Béla");
    let anna = sio_user(&server, a_id, "Anna").await;
    let bela = sio_user(&server, b_id, "Béla").await;
    let (id, _) = live_room(&server, &anna, &[&bela], json!({})).await;
    (server, api, anna, bela, a_id, b_id, id)
}

#[tokio::test(flavor = "multi_thread")]
async fn saving_without_the_owner() {
    let d = duo().await;
    let response = action(&d.api, &d.id, json!({"action": "save"})).await;
    assert_eq!(response.status, 200);
    let db_id = with_room(&d.server, |room| room.db_game_id);
    assert_eq!(response.json()["game_id"], json!(db_id));
    use scrabble::db::RowExt;
    assert_eq!(d.server.app.db.get_game_by_id(db_id.unwrap()).unwrap().unwrap().text("status"), "active");
    assert!(with_room(&d.server, |room| room.manually_saved));
}

#[tokio::test(flavor = "multi_thread")]
async fn ending_finishes_the_game_normally() {
    let (server, api, anna, _bela, a_id, b_id, id) = registered_duo().await;
    with_room(&server, |room| {
        room.game.players[0].score = 80;
        room.game.players[1].score = 20;
    });
    api.sudo().await;
    let data = action(&api, &id, json!({"action": "end"})).await.json();
    assert_eq!(data["winners"], json!(["Anna"]));
    assert!(with_room(&server, |room| room.game.finished));
    use scrabble::db::RowExt;
    let db_id = with_room(&server, |room| room.db_game_id).unwrap();
    assert_eq!(server.app.db.get_game_by_id(db_id).unwrap().unwrap().text("status"), "finished");
    assert!(server.app.db.get_user_by_id(a_id).unwrap().unwrap().rating > 1200);
    assert!(server.app.db.get_user_by_id(b_id).unwrap().unwrap().rating < 1200);
    anna.settle().await;
    assert!(anna.take().iter().any(|(n, _)| n == "rating_update"));
    assert_eq!(audit(&server, "room.end").iter().map(|r| r["action"].as_str().unwrap().to_string()).collect::<Vec<_>>(), vec!["room.end"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn ending_returns_the_pending_tiles_first() {
    let d = voting().await;
    d.api.sudo().await;
    action(&d.api, &d.id, json!({"action": "end"})).await;
    let (finished, pending, hands) = with_room(&d.server, |room| (room.game.finished, room.game.pending_challenge.is_some(), room.game.players.iter().map(|p| p.hand.len()).collect::<Vec<_>>()));
    assert!(finished && !pending);
    assert!(hands.iter().all(|h| *h == 7), "{hands:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn voiding_removes_the_game_from_the_ranking() {
    let (server, api, _anna, bela, _a, _b, id) = registered_duo().await;
    api.sudo().await;
    assert_eq!(action(&api, &id, json!({"action": "void"})).await.status, 200);
    assert!(!server.app.state.lock().rooms.contains_key(&id));
    assert!(!audit(&server, "room.void").is_empty());
    use scrabble::db::RowExt;
    assert_eq!(server.app.db.get_game_by_id(1).unwrap().unwrap().text("status"), "voided");
    assert!(server.app.db.get_leaderboard("wins", 50, None, false).unwrap().0.is_empty());
    bela.settle().await;
    assert!(bela.take().iter().any(|(n, _)| n == "room_disbanded"));
}

#[tokio::test(flavor = "multi_thread")]
async fn disbanding_with_a_save() {
    let d = duo().await;
    d.api.sudo().await;
    assert_eq!(action(&d.api, &d.id, json!({"action": "disband", "save": true, "message": "Karbantartás"})).await.status, 200);
    assert!(!d.server.app.state.lock().rooms.contains_key(&d.id));
    d.bela.settle().await;
    let messages: Vec<Value> = d.bela.take().into_iter().filter(|(n, _)| n == "room_disbanded").map(|(_, d)| d["message"].clone()).collect();
    assert_eq!(messages, vec![json!("Karbantartás")]);
    use scrabble::db::RowExt;
    assert_eq!(d.server.app.db.get_game_by_id(1).unwrap().unwrap().text("status"), "active"); // a mentés megmaradt
}

#[tokio::test(flavor = "multi_thread")]
async fn disbanding_without_a_save_abandons_the_game() {
    let d = duo().await;
    d.api.sudo().await;
    action(&d.api, &d.id, json!({"action": "disband"})).await;
    use scrabble::db::RowExt;
    assert_eq!(d.server.app.db.get_game_by_id(1).unwrap().unwrap().text("status"), "abandoned");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_waiting_room_can_be_disbanded() {
    let (server, api) = start().await;
    let anna = server.guest("Anna").await;
    anna.call("create_room", json!({"name": "Vár", "max_players": 4})).await;
    let id = server.app.state.lock().rooms.keys().next().cloned().unwrap();
    api.sudo().await;
    assert_eq!(action(&api, &id, json!({"action": "disband"})).await.status, 200);
    assert!(server.app.state.lock().rooms.is_empty());
}

// ===================================================================================================
// Élő események
// ===================================================================================================

fn names(events: &[(String, Value)]) -> Vec<String> {
    events.iter().map(|(n, _)| n.clone()).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn only_an_admin_can_subscribe() {
    let (server, _api) = start().await;
    let admin_id = server.user_id(ADMIN_EMAIL);
    let client = Sio::connect(&server).await;
    assert!(client.call("admin_subscribe", json!({"auth_token": "hamis"})).await.is_empty());
    assert!(client.call("admin_subscribe", Value::Null).await.is_empty());
    let got = client.call("admin_subscribe", json!({"auth_token": server.socket_token(admin_id)})).await;
    assert_eq!(names(&got), vec!["admin_subscribed", "admin_overview"]);
    assert!(!server.app.state.lock().get_online_user_ids().contains(&admin_id)); // nem számít online játékosnak
}

#[tokio::test(flavor = "multi_thread")]
async fn a_normal_user_token_is_not_enough() {
    let (server, _api) = start().await;
    let uid = server.create_user("anna@example.com", "Anna");
    let client = Sio::connect(&server).await;
    assert!(client.call("admin_subscribe", json!({"auth_token": server.socket_token(uid)})).await.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn room_updates_reach_the_admin_room_and_watchers() {
    let (server, _api) = start().await;
    let admin_id = server.user_id(ADMIN_EMAIL);
    let watcher = Sio::connect(&server).await;
    watcher.call("admin_subscribe", json!({"auth_token": server.socket_token(admin_id)})).await;
    let (anna, bela) = (server.guest("Anna").await, server.guest("Béla").await);
    let (id, code) = live_room(&server, &anna, &[&bela], json!({})).await;
    watcher.settle().await;
    let updates: Vec<Value> = watcher.take().into_iter().filter(|(n, _)| n == "admin_room_update").map(|(_, d)| d).collect();
    assert!(!updates.is_empty() && updates.last().unwrap()["id"] == json!(id), "{updates:?}");
    let got = watcher.call("admin_watch_room", json!({"room_id": code})).await;
    assert_eq!(names(&got), vec!["admin_room_state"]);
    anna.emit0("pass_turn");
    watcher.settle().await;
    let states: Vec<Value> = watcher.take().into_iter().filter(|(n, _)| n == "admin_room_state").map(|(_, d)| d).collect();
    assert!(!states.is_empty() && states.last().unwrap()["turn_number"] == 1, "{states:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn watching_does_not_make_the_admin_a_spectator() {
    let (server, _api) = start().await;
    let admin_id = server.user_id(ADMIN_EMAIL);
    let watcher = Sio::connect(&server).await;
    watcher.call("admin_subscribe", json!({"auth_token": server.socket_token(admin_id)})).await;
    let (anna, bela) = (server.guest("Anna").await, server.guest("Béla").await);
    let (id, _) = live_room(&server, &anna, &[&bela], json!({})).await;
    watcher.call("admin_watch_room", json!({"room_id": id})).await;
    let (spectators, watchers) = with_room(&server, |room| (room.spectators.len(), room.admin_watchers.len()));
    assert_eq!((spectators, watchers), (0, 1));
    watcher.close();
    watcher.settle_long().await;
    assert!(with_room(&server, |room| room.admin_watchers.is_empty()));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_non_admin_cannot_watch() {
    let d = duo().await;
    d.bela.call("admin_watch_room", json!({"room_id": d.id})).await;
    assert!(with_room(&d.server, |room| room.admin_watchers.is_empty()));
}

// ===================================================================================================
// Zsetonmegmaradás
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn no_intervention_loses_a_tile() {
    for name in ["skip", "pause", "save"] {
        let d = duo_with(json!({"turn_time_limit": 60})).await;
        action(&d.api, &d.id, json!({"action": name})).await;
        assert_eq!(tile_total(&d.server), TILE_DISTRIBUTION.iter().map(|(_, _, c)| *c as usize).sum::<usize>(), "{name}");
    }
}
