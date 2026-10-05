//! Admin panel: moderáció — tiltott szavak, chat napló, bejelentések (a játékosoknál is), nevek átnézése (a Python
//! `test_admin_moderation.py` megfelelője).

mod common;
use common::admin::*;
use common::push::PushSink;
use common::*;
use serde_json::{Value, json};

const REASON: &str = "Teszt indoklás";

async fn start() -> (TestServer, Http) {
    let server = TestServer::start().await;
    set_setting(&server, "bot_think_multiplier", json!(5.0));
    let api = server.admin_http().await;
    (server, api)
}

async fn add_word(api: &Http, word: &str) -> Resp {
    api.admin_post("/moderation/words", json!({"word": word, "reason": REASON})).await
}

fn audit(server: &TestServer, action: &str) -> Vec<Value> {
    audit_of(server, action)
}

/// Két vendég egy szobában.
async fn chat_room(server: &TestServer, names: (&str, &str), room_name: &str) -> (Sio, Sio, String) {
    let (a, b) = (server.guest(names.0).await, server.guest(names.1).await);
    let (id, _) = live_room(server, &a, &[&b], json!({"name": room_name})).await;
    (a, b, id)
}

fn messages(client: &Sio) -> Vec<String> {
    client.take().into_iter().filter(|(n, _)| n == "chat_message").map(|(_, d)| d["message"].as_str().unwrap().to_string()).collect()
}

fn names_of(response: &Resp) -> Vec<String> {
    items_of(response, "name").iter().map(|n| n.as_str().unwrap().to_string()).collect()
}

// ===================================================================================================
// Tiltott szavak
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn banned_word_management() {
    let (server, api) = start().await;
    assert_eq!(add_word(&api, "Hülye").await.json()["word"], "hulye");
    let list = api.admin_get("/moderation/words").await;
    assert_eq!(items_of(&list, "word"), vec![json!("hulye")]);
    assert_eq!(add_word(&api, "HÜLYE").await.status, 409);
    assert_eq!(add_word(&api, "a").await.status, 400);
    assert_eq!(api.admin_post("/moderation/words", json!({"word": "x"})).await.status, 400);
    assert_eq!(api.admin_delete("/moderation/words", json!({"word": "hülye", "reason": REASON})).await.status, 200);
    assert_eq!(api.admin_delete("/moderation/words", json!({"word": "hülye", "reason": REASON})).await.status, 404);
    let actions: Vec<String> = audit(&server, "mod.word").iter().map(|r| r["action"].as_str().unwrap().to_string()).collect();
    assert_eq!(actions, vec!["mod.word_remove", "mod.word_add"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn matching_ignores_case_and_accents() {
    use scrabble::admin::moderation::{contains_banned, mask_banned};
    let (server, api) = start().await;
    add_word(&api, "hulye").await;
    let words = server.app.banned_word_list();
    for text in ["HÜLYE vagy", "te hülye!", "hulyeség"] {
        assert!(contains_banned(&words, text), "{text}");
    }
    assert!(!contains_banned(&words, "hülő"));
    assert_eq!(mask_banned(&words, "Te HÜLYE vagy, hülye!"), ("Te ***** vagy, *****!".to_string(), true));
    assert_eq!(mask_banned(&words, "rendben"), ("rendben".to_string(), false));
}

#[tokio::test(flavor = "multi_thread")]
async fn chat_messages_are_masked_by_default() {
    let (server, api) = start().await;
    add_word(&api, "hulye").await;
    let (anna, bela, _) = chat_room(&server, ("Anna", "Béla"), "Szoba").await;
    anna.emit("send_chat", json!({"message": "Te hülye vagy"}));
    bela.settle().await;
    assert_eq!(messages(&bela), vec!["Te ***** vagy"]);
    assert_eq!(with_room(&server, |room| room.chat_messages.last().unwrap()["message"].clone()), json!("Te ***** vagy"));
}

#[tokio::test(flavor = "multi_thread")]
async fn chat_messages_can_be_dropped_instead() {
    let (server, api) = start().await;
    add_word(&api, "hulye").await;
    api.admin_patch("/settings", json!({"changes": {"banned_word_action": "drop"}, "reason": REASON})).await;
    let (anna, bela, _) = chat_room(&server, ("Anna", "Béla"), "Szoba").await;
    anna.emit("send_chat", json!({"message": "Te hülye vagy"}));
    anna.emit("send_chat", json!({"message": "Szia"}));
    bela.settle().await;
    assert_eq!(messages(&bela), vec!["Szia"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn names_and_room_names_are_rejected() {
    let (server, api) = start().await;
    add_word(&api, "hulye").await;
    let guest = Sio::connect(&server).await;
    let before = server.app.state.lock().player_names.len();
    guest.emit("set_name", json!({"name": "Hülye Hugó", "is_guest": true}));
    guest.settle().await;
    let errors: Vec<String> = guest.take().into_iter().filter(|(n, _)| n == "error").map(|(_, d)| d["message"].as_str().unwrap().to_string()).collect();
    assert_eq!(errors, vec!["Ez a név nem engedélyezett."]);
    {
        let st = server.app.state.lock();
        assert_eq!(st.player_names.len(), before + 1);
        assert!(st.player_names.values().any(|n| n == "Névtelen"));
    }
    guest.emit("create_room", json!({"name": "Hülye szoba"}));
    guest.settle().await;
    let errors: Vec<String> = guest.take().into_iter().filter(|(n, _)| n == "error").map(|(_, d)| d["message"].as_str().unwrap().to_string()).collect();
    assert_eq!(errors, vec!["Ez a szobanév nem engedélyezett."]);
    assert!(server.app.state.lock().rooms.is_empty());
    // a regisztráció is elutasítja
    let code = server.app.db.create_verification_code("uj@example.com").unwrap();
    assert!(server.app.db.verify_code("uj@example.com", &code).0);
    let response = server.http().post("/api/auth/register", json!({"email": "uj@example.com", "password": "secret12", "display_name": "Hülye"})).await;
    assert_eq!(response.status, 400);
    assert_eq!(response.json()["message"], "Ez a név nem engedélyezett.");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_flagged_message_alerts_the_admin_room() {
    let (server, api) = start().await;
    add_word(&api, "hulye").await;
    let watcher = Sio::connect(&server).await;
    watcher.call("admin_subscribe", json!({"auth_token": server.socket_token(server.user_id(ADMIN_EMAIL))})).await;
    watcher.take();
    let (anna, _bela, _) = chat_room(&server, ("Anna", "Béla"), "Riasztós").await;
    anna.emit("send_chat", json!({"message": "hülye"}));
    watcher.settle().await;
    let alerts = watcher.peek("admin_alert");
    assert_eq!(alerts, vec![json!({"code": "banned_word", "room": "Riasztós", "name": "Anna"})]);
}

// ===================================================================================================
// Élő chat
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn every_room_in_one_place() {
    let (server, api) = start().await;
    let (anna, _bela, room1) = chat_room(&server, ("Anna", "Béla"), "Első").await;
    let (cili, _dani, _room2) = chat_room(&server, ("Cili", "Dani"), "Második").await;
    anna.emit("send_chat", json!({"message": "első szoba"}));
    anna.settle().await;
    cili.emit("send_chat", json!({"message": "második szoba"}));
    cili.settle().await;
    let said = api.admin_post(&format!("/rooms/{room1}/action"), json!({"action": "message", "message": "Rendszerüzenet", "reason": REASON})).await;
    assert_eq!(said.status, 200, "{}", said.text());
    let data = api.admin_get("/moderation/chat").await.json();
    let rows: Vec<(String, String)> =
        data["items"].as_array().unwrap().iter().map(|m| (m["name"].as_str().unwrap().to_string(), m["message"].as_str().unwrap().to_string())).collect();
    assert_eq!(
        rows,
        vec![
            ("Anna".to_string(), "első szoba".to_string()),
            ("Cili".to_string(), "második szoba".to_string()),
            ("Rendszer".to_string(), "Rendszerüzenet".to_string())
        ]
    );
    assert_eq!(data["items"][2]["system"], true);
    assert_eq!(data["items"][0]["room_name"], "Első");
    let found = api.admin_get(&format!("/moderation/chat{}", qs(&[("q", "második")]))).await;
    assert_eq!(items_of(&found, "message"), vec![json!("második szoba")]);
    assert_eq!(audit(&server, "view.chat")[0]["details"]["source"], "live");
}

#[tokio::test(flavor = "multi_thread")]
async fn banned_words_are_highlighted() {
    let (server, api) = start().await;
    add_word(&api, "hulye").await;
    api.admin_patch("/settings", json!({"changes": {"banned_word_action": "drop"}, "reason": REASON})).await;
    let (_anna, _bela, _) = chat_room(&server, ("Anna", "Béla"), "Szoba").await;
    // egy korábbi, szűrés előtti üzenet
    with_room(&server, |room| room.add_chat_message("Anna", "hülye ez", None, false));
    let items = api.admin_get("/moderation/chat").await.json()["items"].clone();
    assert_eq!(items[0]["flagged"], true);
}

// ===================================================================================================
// Tartós chat napló
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn the_log_is_off_by_default() {
    let (server, api) = start().await;
    let (anna, _bela, _) = chat_room(&server, ("Anna", "Béla"), "Szoba").await;
    anna.emit("send_chat", json!({"message": "Szia"}));
    anna.settle().await;
    assert_eq!(api.admin_get("/moderation/chat?source=log").await.json()["total"], 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn logging_and_searching() {
    let (server, api) = start().await;
    api.admin_patch("/settings", json!({"changes": {"chat_log_enabled": true}, "reason": REASON})).await;
    let a = server.create_user("a@example.com", "Anna");
    let anna = sio_user(&server, a, "Anna").await;
    let bela = server.guest("Béla").await;
    let (room_id, _) = live_room(&server, &anna, &[&bela], json!({})).await;
    anna.emit("send_chat", json!({"message": "Szia Béla"}));
    anna.settle().await;
    bela.emit("send_chat", json!({"message": "Szia Anna"}));
    bela.settle().await;
    let data = api.admin_get("/moderation/chat?source=log").await.json();
    let rows: Vec<(String, Value, String)> = data["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| (m["name"].as_str().unwrap().to_string(), m["user_id"].clone(), m["message"].as_str().unwrap().to_string()))
        .collect();
    assert_eq!(rows, vec![("Béla".to_string(), Value::Null, "Szia Anna".to_string()), ("Anna".to_string(), json!(a), "Szia Béla".to_string())]);
    assert_eq!(data["items"][0]["room_id"], json!(room_id));
    let who = |query: String| {
        let api = api.clone();
        async move { names_of(&api.admin_get(&format!("/moderation/chat?source=log&{query}")).await) }
    };
    assert_eq!(who("q=Béla".into()).await, vec!["Anna"]);
    assert_eq!(who(format!("user={a}")).await, vec!["Anna"]);
    assert_eq!(who("user=b%C3%A9la".into()).await, vec!["Béla"]);
    assert_eq!(who(format!("room={room_id}")).await, vec!["Béla", "Anna"]);
    assert!(who("since=2999-01-01".into()).await.is_empty());
    assert_eq!(api.admin_get(&format!("/moderation/chat{}", qs(&[("source", "log"), ("since", "hibás")]))).await.status, 400);
    assert!(!audit(&server, "view.chat").is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_stored_text_is_the_filtered_one_and_csv() {
    let (server, api) = start().await;
    add_word(&api, "hulye").await;
    api.admin_patch("/settings", json!({"changes": {"chat_log_enabled": true}, "reason": REASON})).await;
    let (anna, _bela, _) = chat_room(&server, ("Anna", "Béla"), "Szoba").await;
    anna.emit("send_chat", json!({"message": "hülye"}));
    anna.settle().await;
    let text = api.admin_get("/moderation/chat?source=log&format=csv").await.text();
    assert!(text.contains("*****") && !text.contains("hülye"), "{text}");
}

// ===================================================================================================
// Játékosok bejelentései
// ===================================================================================================

struct Pair {
    server: TestServer,
    api: Http,
    anna: Sio,
    a: i64,
    bela: Sio,
    b: i64,
    room_id: String,
}

async fn pair() -> Pair {
    let (server, api) = start().await;
    let a = server.create_user("a@example.com", "Anna");
    let b = server.create_user("b@example.com", "Béla");
    let anna = sio_user(&server, a, "Anna").await;
    let bela = sio_user(&server, b, "Béla").await;
    let (room_id, _) = live_room(&server, &anna, &[&bela], json!({"name": "Bejelentős"})).await;
    Pair { server, api, anna, a, bela, b, room_id }
}

fn results(client: &Sio) -> Vec<Value> {
    client.take().into_iter().filter(|(n, _)| n == "report_result").map(|(_, d)| d).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn report_a_player() {
    let p = pair().await;
    let sink = PushSink::start();
    let sub = sink.subscription("admin-device");
    p.server.app.db.save_push_subscription(p.server.user_id(ADMIN_EMAIL), &sub.endpoint, &sub.p256dh, &sub.auth, "hu").unwrap();
    p.bela.emit("send_chat", json!({"message": "sértő szöveg"}));
    p.bela.settle().await;
    p.anna.take();
    p.anna.emit("report_content", json!({"kind": "player", "name": "Béla", "reason": "Zaklat"}));
    p.anna.settle().await;
    assert_eq!(results(&p.anna), vec![json!({"success": true, "message": "Bejelentés elküldve. Köszönjük!"})]);
    let data = p.api.admin_get("/reports").await.json();
    assert_eq!((data["total"].clone(), data["new"].clone()), (json!(1), json!(1)));
    let report = &data["items"][0];
    assert_eq!(
        (report["reporter_name"].clone(), report["reported_name"].clone(), report["kind"].clone(), report["status"].clone()),
        (json!("Anna"), json!("Béla"), json!("player"), json!("new"))
    );
    assert_eq!((report["reporter_user_id"].clone(), report["reported_user_id"].clone(), report["reason"].clone()), (json!(p.a), json!(p.b), json!("Zaklat")));
    // az adminok push értesítést kapnak a szoba nevével
    let got = sink.wait_for(1, 5).await;
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].payload["body"], "Új bejelentés érkezett · Bejelentős");
}

#[tokio::test(flavor = "multi_thread")]
async fn report_a_chat_message_with_a_snapshot() {
    let p = pair().await;
    p.bela.emit("send_chat", json!({"message": "sértő szöveg"}));
    p.bela.settle().await;
    p.anna.emit("report_content", json!({"kind": "chat", "name": "Béla", "message": "sértő szöveg", "reason": ""}));
    p.anna.settle().await;
    let report_id = p.api.admin_get("/reports").await.json()["items"][0]["id"].clone();
    let report = p.api.admin_get(&format!("/reports/{report_id}")).await.json()["report"].clone();
    assert_eq!((report["message"].clone(), report["room_name"].clone()), (json!("sértő szöveg"), json!("Bejelentős")));
    assert_eq!(report["snapshot"]["chat"], json!([{"name": "Béla", "message": "sértő szöveg"}]));
    let mut players: Vec<String> = report["snapshot"]["players"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap().to_string()).collect();
    players.sort();
    assert_eq!(players, vec!["Anna", "Béla"]);
    assert_eq!(audit(&p.server, "view.report")[0]["target_id"], json!(report_id.to_string()));
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_reports_are_rejected() {
    let long = "x".repeat(201);
    let long_reason = "x".repeat(301);
    let cases = vec![
        json!({"kind": "player", "name": "Senki"}),
        json!({"kind": "player", "name": "Anna"}),
        json!({"kind": "rossz", "name": "Béla"}),
        json!({"kind": "chat", "name": "Béla", "message": ""}),
        json!({"kind": "chat", "name": "Béla", "message": long}),
        json!({"kind": "player", "name": "Béla", "reason": long_reason}),
        json!({"kind": "player"}),
    ];
    for payload in cases {
        let p = pair().await;
        p.anna.emit("report_content", payload.clone());
        p.anna.settle().await;
        let got = results(&p.anna);
        assert_eq!(got.len(), 1, "{payload}");
        assert_eq!(got[0]["success"], false, "{payload}");
        assert_eq!(p.api.admin_get("/reports").await.json()["total"], 0, "{payload}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn guests_and_outsiders_cannot_report() {
    let p = pair().await;
    let guest = p.server.guest("Vendég").await;
    guest.emit("report_content", json!({"kind": "player", "name": "Béla"}));
    guest.settle().await;
    assert_eq!(results(&guest), vec![json!({"success": false, "message": "Nem vagy szobában."})]);
    // másik szobában lévő vendég
    let loner = p.server.guest("Cili").await;
    live_room(&p.server, &loner, &[], json!({"name": "Másik"})).await;
    loner.emit("report_content", json!({"kind": "player", "name": "Béla"}));
    loner.settle().await;
    assert_eq!(results(&loner)[0]["success"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_guest_in_a_room_cannot_report() {
    let (server, _api) = start().await;
    let (anna, _bela, _) = chat_room(&server, ("Anna", "Béla"), "Szoba").await;
    anna.emit("report_content", json!({"kind": "player", "name": "Béla"}));
    anna.settle().await;
    assert_eq!(results(&anna), vec![json!({"success": false, "message": "Bejelentést csak regisztrált felhasználó tehet."})]);
}

#[tokio::test(flavor = "multi_thread")]
async fn reports_are_rate_limited() {
    let p = pair().await;
    for _ in 0..4 {
        p.anna.emit("report_content", json!({"kind": "player", "name": "Béla"}));
    }
    p.anna.settle().await;
    let got = results(&p.anna);
    assert_eq!(got.iter().map(|r| r["success"].as_bool().unwrap()).collect::<Vec<_>>(), vec![true, true, true, false]);
    assert_eq!(got[3]["message"], "Túl sok kérés, várj egy kicsit.");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_admin_room_gets_a_live_event() {
    let p = pair().await;
    let watcher = Sio::connect(&p.server).await;
    watcher.call("admin_subscribe", json!({"auth_token": p.server.socket_token(p.server.user_id(ADMIN_EMAIL))})).await;
    watcher.take();
    p.anna.emit("report_content", json!({"kind": "player", "name": "Béla"}));
    p.anna.settle().await;
    watcher.settle().await;
    let events = watcher.peek("admin_report");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["reported"], "Béla");
    assert_eq!(events[0]["room"], "Bejelentős");
    let _ = (&p.bela, &p.room_id);
}

#[tokio::test(flavor = "multi_thread")]
async fn handling_a_report() {
    let p = pair().await;
    p.anna.emit("report_content", json!({"kind": "player", "name": "Béla"}));
    p.anna.settle().await;
    let api = &p.api;
    let report_id = api.admin_get("/reports").await.json()["items"][0]["id"].clone();
    let url = format!("/reports/{report_id}");
    assert_eq!(api.admin_patch(&url, json!({"status": "handled"})).await.status, 400); // indoklás nélkül
    assert_eq!(api.admin_patch(&url, json!({"status": "nincs", "reason": REASON})).await.status, 400);
    assert_eq!(api.admin_patch(&url, json!({"status": "handled", "reason": "Figyelmeztettem"})).await.status, 200);
    let report = api.admin_get(&url).await.json()["report"].clone();
    assert_eq!((report["status"].clone(), report["handler_note"].clone()), (json!("handled"), json!("Figyelmeztettem")));
    assert_eq!(report["handled_by"], json!(p.server.user_id(ADMIN_EMAIL)));
    assert!(report["handled_at"].is_string());
    assert_eq!(api.admin_get("/reports?status=new").await.json()["items"], json!([]));
    assert_eq!(api.admin_get("/reports?status=handled").await.json()["items"].as_array().unwrap().len(), 1);
    assert_eq!(api.admin_patch(&url, json!({"status": "new", "reason": REASON})).await.status, 200);
    assert_eq!(api.admin_get(&url).await.json()["report"]["handled_by"], Value::Null);
    assert_eq!(api.admin_get("/reports?status=rossz").await.status, 400);
    assert_eq!(api.admin_get("/reports/999").await.status, 404);
    assert_eq!(audit(&p.server, "mod.report_update")[0]["details"]["before"], json!({"status": "handled"}));
}

// ===================================================================================================
// Nevek átnézése
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn recent_names_and_renames() {
    let (server, api) = start().await;
    let a = server.create_user("a@example.com", "Anna");
    let b = server.create_user("b@example.com", "Hülye Béla");
    add_word(&api, "hulye").await;
    assert_eq!(api.admin_patch(&format!("/users/{a}"), json!({"display_name": "Annácska", "reason": REASON})).await.status, 200);
    let items = api.admin_get("/moderation/names").await.json()["items"].clone();
    let flag = |name: &str, kind: &str| -> Value {
        items.as_array().unwrap().iter().find(|i| i["display_name"] == name && i["kind"] == kind).map(|i| i["flagged"].clone()).unwrap_or(Value::Null)
    };
    assert_eq!(flag("Hülye Béla", "registered"), json!(true));
    assert_eq!(flag("Annácska", "registered"), json!(false));
    assert_eq!(flag("Annácska", "renamed"), json!(false));
    assert_eq!(api.admin_patch(&format!("/users/{b}"), json!({"display_name": "Béla", "reason": REASON})).await.status, 200);
    assert!(!api.admin_get("/moderation/names?limit=1").await.json()["items"].as_array().unwrap().is_empty());
}
