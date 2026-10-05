//! Közös segédek az integrációs tesztekhez: valódi szerver ideiglenes adatbázissal és véletlen porton, HTTP és
//! Socket.IO kliens, játék-segédek (szó keresése a kézből).
//!
//! Minden teszt saját szervert indít (saját adatbázis és állapot), így a tesztek egymástól függetlenek.

#![allow(dead_code)]

pub mod push;

use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use scrabble::app::App;
use scrabble::config::Config;
use scrabble::db::Db;
use scrabble::server;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

pub const ADMIN_EMAIL: &str = "admin@example.com";
pub const PASSWORD: &str = "secret12";

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Egy futó szerver ideiglenes mappával; a `Drop` leállítja és törli.
pub struct TestServer {
    pub app: Arc<App>,
    pub port: u16,
    pub dir: PathBuf,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

pub fn temp_dir(prefix: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("scrabble-{prefix}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn test_config(dir: &std::path::Path) -> Config {
    Config {
        smtp_host: "localhost".into(),
        smtp_port: 25,
        smtp_user: String::new(),
        smtp_password: String::new(),
        smtp_from: String::new(),
        db_path: dir.join("test.db").to_string_lossy().to_string(),
        backup_dir: dir.join("backups").to_string_lossy().to_string(),
        word_reject_threshold: 1,
        admin_emails: [ADMIN_EMAIL.to_string()].into_iter().collect(),
        admin_session_idle_minutes: 30,
        admin_sudo_minutes: 10,
        admin_ip_allowlist: Vec::new(),
        port: 0,
        secret_key: "teszt-titkos-kulcs".into(),
    }
}

impl TestServer {
    pub async fn start() -> TestServer {
        TestServer::start_with(|_| {}).await
    }

    pub async fn start_with(tweak: impl FnOnce(&mut Config)) -> TestServer {
        // a szótár betöltése egyszer (mint az éles indításkor): az első lerakás ne várjon másodperceket
        static WARM: std::sync::Once = std::sync::Once::new();
        tokio::task::spawn_blocking(|| WARM.call_once(|| { scrabble::dictionary::warm_up(); })).await.unwrap();
        let dir = temp_dir("srv");
        let mut config = test_config(&dir);
        tweak(&mut config);
        let db = Arc::new(Db::open(&config.db_path).expect("adatbázis"));
        db.init().expect("séma");
        let app = App::new(Arc::new(config), db);
        // a próbák sok kérést küldenek egy címről: az IP-alapú korlátok lazítva (a külön tesztek visszaállítják)
        relax_ip_limits(&app);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let a = app.clone();
        tokio::spawn(async move {
            let _ = server::serve_on(a, listener, async move {
                let _ = rx.await;
            })
            .await;
        });
        TestServer { app, port, dir, shutdown: Some(tx) }
    }

    pub fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Regisztrált felhasználó közvetlenül az adatbázisban (jelszó: `secret12`).
    pub fn create_user(&self, email: &str, name: &str) -> i64 {
        self.app.db.create_user(email, name, PASSWORD).expect("felhasználó")
    }

    pub fn user_id(&self, email: &str) -> i64 {
        self.app.db.get_user_by_email(email).unwrap().unwrap().id
    }

    /// Az `uN@example.com` / `JatekosN` felhasználó (létrehozza, ha még nincs).
    pub fn ensure_user(&self, n: u32) -> i64 {
        let email = format!("u{n}@example.com");
        match self.app.db.get_user_by_email(&email).unwrap() {
            Some(user) => user.id,
            None => self.create_user(&email, &format!("Jatekos{n}")),
        }
    }

    pub fn socket_token(&self, user_id: i64) -> String {
        scrabble::socket_auth::create_socket_token(&self.app.config.secret_key, user_id)
    }

    pub fn http(&self) -> Http {
        Http::new(self.base())
    }

    /// Bejelentkezett HTTP kliens az adott felhasználóval.
    pub async fn http_as(&self, email: &str) -> Http {
        let http = self.http();
        let resp = http.post("/api/auth/login", json!({"email": email, "password": PASSWORD})).await;
        assert_eq!(resp.status, 200, "belépés: {}", resp.text());
        http
    }

    /// Az admin (létrehozza a fiókot, ha kell) bejelentkezve.
    pub async fn admin_http(&self) -> Http {
        if self.app.db.get_user_by_email(ADMIN_EMAIL).unwrap().is_none() {
            self.create_user(ADMIN_EMAIL, "Admin");
        }
        self.http_as(ADMIN_EMAIL).await
    }

    /// Socket.IO kapcsolat az `uN` regisztrált játékosként (a `set_name` már lement).
    pub async fn player(&self, n: u32) -> Sio {
        let id = self.ensure_user(n);
        let sio = Sio::connect(self).await;
        let before = self.app.state.lock().player_names.len();
        sio.emit("set_name", json!({"name": format!("Jatekos{n}"), "is_guest": false, "auth_token": self.socket_token(id)}));
        self.wait_registered(before).await;
        sio.settle().await;
        let events = sio.take();
        let errors: Vec<_> = events.iter().filter(|(name, _)| name == "error").collect();
        assert!(errors.is_empty(), "a set_name hibát adott: {errors:?}");
        assert!(self.app.state.lock().is_user_online(id), "a felhasználó nem online a set_name után: {events:?}");
        let mut sio = sio;
        sio.name_n = n;
        sio
    }

    pub async fn guest(&self, name: &str) -> Sio {
        let sio = Sio::connect(self).await;
        let before = self.app.state.lock().player_names.len();
        sio.emit("set_name", json!({"name": name, "is_guest": true}));
        self.wait_registered(before).await;
        sio.settle().await;
        sio.take();
        sio
    }

    /// Megvárja, hogy a szerver feldolgozza a `set_name`-et (a sikeres válasz nem küld eseményt): a regisztrált
    /// nevek száma megnő. Betöltött gépen a rögzített várakozás nem elég.
    async fn wait_registered(&self, before: usize) {
        let end = tokio::time::Instant::now() + Duration::from_secs(10);
        while self.app.state.lock().player_names.len() <= before && tokio::time::Instant::now() < end {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Az IP-alapú forgalomkorlátok (belépés, kód küldés, regisztráció, admin) felemelése a próbák idejére.
pub fn relax_ip_limits(app: &App) {
    let mut ip = HashMap::new();
    for key in ["request_code", "login", "register", "admin", "admin_danger", "practice", "dictionary", "daily", "word_review", "search_users"] {
        ip.insert(key.to_string(), (100_000u32, 60u32));
    }
    app.limiter.set_overrides(ip, HashMap::new());
}

/// A megadott Socket.IO események forgalomkorlátjának felemelése (a hosszú, gépi játékokhoz); az IP-korlátok is lazák.
pub fn relax_socket_limits(app: &App, events: &[&str]) {
    let mut ip = HashMap::new();
    for key in ["request_code", "login", "register", "admin", "admin_danger", "practice", "dictionary", "daily", "word_review", "search_users"] {
        ip.insert(key.to_string(), (100_000u32, 60u32));
    }
    let socket: HashMap<String, (u32, u32)> = events.iter().map(|e| (e.to_string(), (100_000u32, 10u32))).collect();
    app.limiter.set_overrides(ip, socket);
}

/// Az alapértelmezett (éles) korlátok visszaállítása.
pub fn restore_default_limits(app: &App) {
    app.limiter.set_overrides(HashMap::new(), HashMap::new());
}

// ===================================================================================================
// HTTP
// ===================================================================================================

pub struct Resp {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Resp {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }

    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|e| panic!("nem JSON ({e}): {}", self.text().chars().take(200).collect::<String>()))
    }

    pub fn header(&self, name: &str) -> Option<String> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone())
    }

    pub fn headers_all(&self, name: &str) -> Vec<String> {
        self.headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone()).collect()
    }
}

/// Egyszerű HTTP kliens munkamenet-sütivel.
#[derive(Clone)]
pub struct Http {
    base: String,
    cookie: Arc<Mutex<Option<String>>>,
    /// extra fejlécek minden kéréshez
    extra: Arc<Mutex<Vec<(String, String)>>>,
}

impl Http {
    pub fn new(base: String) -> Http {
        Http { base, cookie: Arc::new(Mutex::new(None)), extra: Arc::new(Mutex::new(Vec::new())) }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    pub fn set_cookie(&self, token: Option<&str>) {
        *self.cookie.lock() = token.map(|t| format!("session_token={t}"));
    }

    pub fn session_token(&self) -> Option<String> {
        self.cookie.lock().as_ref().and_then(|c| c.strip_prefix("session_token=").map(|t| t.to_string()))
    }

    pub fn add_header(&self, name: &str, value: &str) {
        self.extra.lock().push((name.to_string(), value.to_string()));
    }

    pub async fn request(&self, method: &str, path: &str, body: Option<Value>, headers: &[(&str, &str)]) -> Resp {
        let url = format!("{}{}", self.base, path);
        let method = method.to_string();
        let cookie = self.cookie.lock().clone();
        let mut all: Vec<(String, String)> = headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        all.extend(self.extra.lock().iter().cloned());
        let (resp, set_cookie) = tokio::task::spawn_blocking(move || {
            let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).timeout_global(Some(Duration::from_secs(60))).build().into();
            let mut builder = ureq::http::Request::builder().method(method.as_str()).uri(&url);
            if let Some(c) = &cookie {
                builder = builder.header("Cookie", c);
            }
            for (k, v) in &all {
                builder = builder.header(k.as_str(), v.as_str());
            }
            let request = match &body {
                Some(b) => builder.header("Content-Type", "application/json").body(b.to_string().into_bytes()),
                None => builder.body(Vec::new()),
            }
            .expect("kérés");
            let response = agent.run(request).expect("HTTP kérés");
            let status = response.status().as_u16();
            let headers: Vec<(String, String)> = response.headers().iter().map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string())).collect();
            let mut body = response.into_body();
            let bytes = body.read_to_vec().unwrap_or_default();
            let set_cookie: Vec<String> = headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case("set-cookie")).map(|(_, v)| v.clone()).collect();
            (Resp { status, headers, body: bytes }, set_cookie)
        })
        .await
        .unwrap();
        for c in set_cookie {
            if let Some(rest) = c.strip_prefix("session_token=") {
                let value = rest.split(';').next().unwrap_or("");
                *self.cookie.lock() = if value.is_empty() { None } else { Some(format!("session_token={value}")) };
            }
        }
        resp
    }

    pub async fn get(&self, path: &str) -> Resp {
        self.request("GET", path, None, &[]).await
    }

    pub async fn post(&self, path: &str, body: Value) -> Resp {
        self.request("POST", path, Some(body), &[]).await
    }

    pub async fn delete(&self, path: &str) -> Resp {
        self.request("DELETE", path, None, &[]).await
    }

    /// Admin kérés: a CSRF-védelemhez szükséges fejlécekkel (`X-Admin-Request`, `Origin`).
    pub async fn admin(&self, method: &str, path: &str, body: Option<Value>) -> Resp {
        let origin = self.base.clone();
        self.request(method, &format!("/api/admin{path}"), body, &[("X-Admin-Request", "1"), ("Origin", &origin)]).await
    }

    pub async fn admin_get(&self, path: &str) -> Resp {
        self.admin("GET", path, None).await
    }

    pub async fn admin_post(&self, path: &str, body: Value) -> Resp {
        self.admin("POST", path, Some(body)).await
    }

    pub async fn admin_patch(&self, path: &str, body: Value) -> Resp {
        self.admin("PATCH", path, Some(body)).await
    }

    pub async fn admin_delete(&self, path: &str, body: Value) -> Resp {
        self.admin("DELETE", path, Some(body)).await
    }

    /// Sudo mód bekapcsolása (az admin jelszavával).
    pub async fn sudo(&self) {
        let resp = self.admin_post("/sudo", json!({"password": PASSWORD})).await;
        assert_eq!(resp.status, 200, "sudo: {}", resp.text());
    }
}

// ===================================================================================================
// Socket.IO
// ===================================================================================================

/// Socket.IO (Engine.IO v4, websocket) kliens. Az eseményeket sorba gyűjti.
pub struct Sio {
    tx: mpsc::UnboundedSender<String>,
    events: Arc<Mutex<Vec<(String, Value)>>>,
    pub state: Arc<Mutex<Option<Value>>>,
    pub room_code: Arc<Mutex<Option<String>>>,
    pub room_id: Arc<Mutex<Option<String>>>,
    pub reconnect_token: Arc<Mutex<Option<String>>>,
    closed: Arc<std::sync::atomic::AtomicBool>,
    /// az `uN` játékos sorszáma (0: vendég / névtelen)
    pub name_n: u32,
}

impl Sio {
    pub async fn connect(server: &TestServer) -> Sio {
        Sio::connect_url(&format!("ws://127.0.0.1:{}/socket.io/?EIO=4&transport=websocket", server.port)).await
    }

    pub async fn connect_url(url: &str) -> Sio {
        let (stream, _) = tokio_tungstenite::connect_async(url).await.expect("websocket");
        let (mut sink, mut source) = stream.split();
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        let events: Arc<Mutex<Vec<(String, Value)>>> = Arc::new(Mutex::new(Vec::new()));
        let state = Arc::new(Mutex::new(None));
        let room_code = Arc::new(Mutex::new(None));
        let room_id = Arc::new(Mutex::new(None));
        let reconnect_token = Arc::new(Mutex::new(None));
        let closed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let connected = Arc::new(tokio::sync::Notify::new());

        tokio::spawn(async move {
            while let Some(text) = rx.recv().await {
                if sink.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
            let _ = sink.close().await;
        });
        let (ev, st, rc, ri, rt, cl, cn, ptx) = (events.clone(), state.clone(), room_code.clone(), room_id.clone(), reconnect_token.clone(), closed.clone(), connected.clone(), tx.clone());
        tokio::spawn(async move {
            while let Some(Ok(message)) = source.next().await {
                let Message::Text(text) = message else { continue };
                let text = text.to_string();
                if text.starts_with('0') {
                    let _ = ptx.send("40".to_string());
                } else if text == "2" {
                    let _ = ptx.send("3".to_string());
                } else if text.starts_with("40") {
                    cn.notify_one();
                } else if text.starts_with("41") {
                    break;
                } else if let Some(body) = text.strip_prefix("42") {
                    let Ok(Value::Array(mut parts)) = serde_json::from_str::<Value>(body) else { continue };
                    if parts.is_empty() {
                        continue;
                    }
                    let name = parts.remove(0).as_str().unwrap_or("").to_string();
                    let data = if parts.is_empty() { Value::Null } else { parts.remove(0) };
                    match name.as_str() {
                        "game_state" => *st.lock() = Some(data.clone()),
                        "room_code" => *rc.lock() = data["code"].as_str().map(|s| s.to_string()),
                        "room_joined" => {
                            if let Some(id) = data["room_id"].as_str() {
                                *ri.lock() = Some(id.to_string());
                            }
                            if let Some(t) = data["reconnect_token"].as_str() {
                                *rt.lock() = Some(t.to_string());
                            }
                        }
                        _ => {}
                    }
                    ev.lock().push((name, data));
                }
            }
            cl.store(true, Ordering::SeqCst);
        });
        let _ = tokio::time::timeout(Duration::from_secs(5), connected.notified()).await;
        Sio { tx, events, state, room_code, room_id, reconnect_token, closed, name_n: 0 }
    }

    /// Esemény küldése; `Value::Null` → adat nélkül (mint a kliens a paraméter nélküli eseményeknél).
    pub fn emit(&self, event: &str, data: Value) {
        let payload = if data.is_null() { json!([event]) } else { json!([event, data]) };
        let _ = self.tx.send(format!("42{payload}"));
    }

    pub fn emit0(&self, event: &str) {
        self.emit(event, Value::Null);
    }

    /// Rövid várakozás, hogy a szerver válaszai megérkezzenek.
    pub async fn settle(&self) {
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    pub async fn settle_long(&self) {
        tokio::time::sleep(Duration::from_millis(700)).await;
    }

    /// A beérkezett események (és a sor kiürítése).
    pub fn take(&self) -> Vec<(String, Value)> {
        std::mem::take(&mut *self.events.lock())
    }

    /// Esemény keresése a sorban (nem vesz ki semmit).
    pub fn peek(&self, event: &str) -> Vec<Value> {
        self.events.lock().iter().filter(|(n, _)| n == event).map(|(_, d)| d.clone()).collect()
    }

    pub fn names(&self) -> Vec<String> {
        self.events.lock().iter().map(|(n, _)| n.clone()).collect()
    }

    /// Küld, vár és visszaadja az összes eseményt.
    pub async fn call(&self, event: &str, data: Value) -> Vec<(String, Value)> {
        self.emit(event, data);
        self.settle().await;
        self.take()
    }

    /// Az első `event` nevű esemény adata (kiveszi a sorból), legfeljebb `secs` másodpercig várva.
    pub async fn wait(&self, event: &str, secs: u64) -> Option<Value> {
        let end = tokio::time::Instant::now() + Duration::from_secs(secs);
        loop {
            {
                let mut events = self.events.lock();
                if let Some(pos) = events.iter().position(|(n, _)| n == event) {
                    return Some(events.remove(pos).1);
                }
            }
            if tokio::time::Instant::now() >= end {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    }

    /// Várakozás egy feltételre a `game_state`-en.
    pub async fn wait_state(&self, secs: u64, cond: impl Fn(&Value) -> bool) -> bool {
        let end = tokio::time::Instant::now() + Duration::from_secs(secs);
        loop {
            if let Some(state) = self.state.lock().as_ref() {
                if cond(state) {
                    return true;
                }
            }
            if tokio::time::Instant::now() >= end {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    }

    pub fn state(&self) -> Value {
        self.state.lock().clone().unwrap_or(Value::Null)
    }

    pub fn code(&self) -> String {
        self.room_code.lock().clone().expect("nincs szobakód")
    }

    pub fn room(&self) -> String {
        self.room_id.lock().clone().expect("nincs szoba")
    }

    pub fn token(&self) -> String {
        self.reconnect_token.lock().clone().expect("nincs újracsatlakozó token")
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    pub fn close(&self) {
        let _ = self.tx.send("41".to_string());
    }

    // --- játékállapot ---

    /// A saját kéz (a `game_state` kéz nélküli játékosa a többi).
    pub fn hand(&self) -> Vec<String> {
        let state = self.state();
        state["players"]
            .as_array()
            .and_then(|ps| ps.iter().find(|p| p.get("hand").is_some()))
            .and_then(|p| p["hand"].as_array())
            .map(|h| h.iter().map(|t| t.as_str().unwrap_or("").to_string()).collect())
            .unwrap_or_default()
    }

    pub fn my_id(&self) -> Option<String> {
        let state = self.state();
        state["players"].as_array().and_then(|ps| ps.iter().find(|p| p.get("hand").is_some())).and_then(|p| p["id"].as_str().map(|s| s.to_string()))
    }

    pub fn my_turn(&self) -> bool {
        let state = self.state();
        state["finished"] != json!(true) && state["current_player"].as_str().is_some() && state["current_player"].as_str().map(|s| s.to_string()) == self.my_id()
    }

    pub fn board(&self) -> Value {
        self.state()["board"].clone()
    }
}

/// A soron lévő játékos azok közül, akik `Sio`-k.
pub fn current<'a>(players: &[&'a Sio]) -> &'a Sio {
    let cid = players[0].state()["current_player"].as_str().map(|s| s.to_string());
    players.iter().copied().find(|p| p.my_id() == cid).expect("nincs soron lévő játékos")
}

pub fn others<'a>(players: &[&'a Sio], cur: &Sio) -> Vec<&'a Sio> {
    players.iter().copied().filter(|p| !std::ptr::eq(*p, cur)).collect()
}

/// Egy szoba két emberrel (az első a tulajdonos), elindítva.
pub async fn started_pair(server: &TestServer, options: Value) -> (Sio, Sio) {
    let a = server.player(1).await;
    let b = server.player(2).await;
    a.emit("create_room", options);
    a.settle().await;
    b.emit("join_room", json!({"code": a.code()}));
    b.settle().await;
    a.emit0("start_game");
    a.settle_long().await;
    a.take();
    b.take();
    (a, b)
}

// ===================================================================================================
// Szavak keresése a kézből
// ===================================================================================================

const DIGRAPHS: [&str; 7] = ["CS", "GY", "LY", "NY", "SZ", "TY", "ZS"];

/// A szó kirakása a kézből: (zseton, betű, joker-e) zsetononként. A `fixed` a szó azon pozíciói, amelyeket a
/// táblán már álló betű ad (karakterindex → betű).
fn assign(word: &str, hand: &[String], fixed: &HashMap<usize, String>) -> Option<Vec<Option<(String, String, bool)>>> {
    fn rec(chars: &[char], i: usize, hand: &mut Vec<String>, fixed: &HashMap<usize, String>, out: &mut Vec<Option<(String, String, bool)>>) -> bool {
        if i == chars.len() {
            return true;
        }
        if let Some(letter) = fixed.get(&i) {
            let width = letter.chars().count();
            if chars[i..].iter().take(width).collect::<String>() != *letter {
                return false;
            }
            out.push(None);
            if rec(chars, i + width, hand, fixed, out) {
                return true;
            }
            out.pop();
            return false;
        }
        let two: String = chars[i..].iter().take(2).collect();
        if two.chars().count() == 2 && DIGRAPHS.contains(&two.as_str()) {
            if let Some(pos) = hand.iter().position(|t| *t == two) {
                let tile = hand.remove(pos);
                out.push(Some((tile.clone(), tile.clone(), false)));
                if rec(chars, i + 2, hand, fixed, out) {
                    return true;
                }
                out.pop();
                hand.push(tile);
            }
            return false; // a kétjegyű betű csak a saját zsetonjával
        }
        let one = chars[i].to_string();
        if let Some(pos) = hand.iter().position(|t| *t == one) {
            let tile = hand.remove(pos);
            out.push(Some((tile.clone(), tile.clone(), false)));
            if rec(chars, i + 1, hand, fixed, out) {
                return true;
            }
            out.pop();
            hand.push(tile);
        }
        if let Some(pos) = hand.iter().position(|t| t.is_empty()) {
            let tile = hand.remove(pos);
            out.push(Some((String::new(), one.clone(), true)));
            if rec(chars, i + 1, hand, fixed, out) {
                return true;
            }
            out.pop();
            hand.push(tile);
        }
        false
    }
    let chars: Vec<char> = word.chars().collect();
    let mut pool = hand.to_vec();
    let mut out = Vec::new();
    if rec(&chars, 0, &mut pool, fixed, &mut out) { Some(out) } else { None }
}

fn short_words() -> Vec<String> {
    let vocab = scrabble::ai::get_vocabulary();
    let mut words: Vec<String> = vocab.iter().filter(|w| (2..=7).contains(&w.chars().count())).map(|w| w.to_uppercase()).collect();
    words.sort_by(|a, b| b.chars().count().cmp(&a.chars().count()).then(a.cmp(b)));
    words
}

/// A `place_tiles` bemenete: a középső mezőn átmenő vízszintes szó a kézből, `size` zsetonból (ha megadott).
pub fn first_move(hand: &[String], size: Option<usize>) -> Option<Vec<Value>> {
    for word in short_words() {
        let Some(assigned) = assign(&word, hand, &HashMap::new()) else { continue };
        let tiles: Vec<(String, String, bool)> = assigned.into_iter().flatten().collect();
        if size.is_some_and(|s| tiles.len() != s) {
            continue;
        }
        let n = tiles.len();
        let c0 = 7 - n / 2;
        return Some(tiles.iter().enumerate().map(|(i, (_, letter, blank))| json!({"row": 7, "col": c0 + i, "letter": letter, "is_blank": blank})).collect());
    }
    None
}

/// Függőleges szó egy már a táblán álló betűn át, keresztszó nélkül; `size` új zsetonnal (ha megadott).
pub fn cross_move(board: &Value, hand: &[String], size: Option<usize>) -> Option<Vec<Value>> {
    let cell = |r: i64, c: i64| -> Option<String> {
        if (0..15).contains(&r) && (0..15).contains(&c) { board[r as usize][c as usize]["letter"].as_str().map(|s| s.to_string()) } else { None }
    };
    let words = short_words();
    for r in 0..15i64 {
        for c in 0..15i64 {
            let Some(letter) = cell(r, c) else { continue };
            for word in words.iter().filter(|w| (3..=6).contains(&w.chars().count())) {
                let chars: Vec<char> = word.chars().collect();
                let width = letter.chars().count();
                for k in 0..chars.len() {
                    if chars[k..].iter().take(width).collect::<String>() != letter {
                        continue;
                    }
                    let fixed = HashMap::from([(k, letter.clone())]);
                    let Some(assigned) = assign(word, hand, &fixed) else { continue };
                    // a cellák: a már álló betű helye üres (None)
                    let kidx = assigned.iter().position(|t| t.is_none())? as i64;
                    let top = r - kidx;
                    let n = assigned.len() as i64;
                    if top < 0 || top + n > 15 {
                        continue;
                    }
                    let mut ok = true;
                    let mut placed = Vec::new();
                    for (i, t) in assigned.iter().enumerate() {
                        let rr = top + i as i64;
                        if let Some((_, letter, blank)) = t {
                            if cell(rr, c).is_some() || cell(rr, c - 1).is_some() || cell(rr, c + 1).is_some() {
                                ok = false;
                                break;
                            }
                            placed.push(json!({"row": rr, "col": c, "letter": letter, "is_blank": blank}));
                        }
                    }
                    if !ok || placed.is_empty() || cell(top - 1, c).is_some() || cell(top + n, c).is_some() {
                        continue;
                    }
                    if size.is_some_and(|s| placed.len() != s) {
                        continue;
                    }
                    return Some(placed);
                }
            }
        }
    }
    None
}

/// Az események között az adott nevű első adata.
pub fn find(events: &[(String, Value)], name: &str) -> Option<Value> {
    events.iter().find(|(n, _)| n == name).map(|(_, d)| d.clone())
}

pub fn event_names(events: &[(String, Value)]) -> Vec<&str> {
    events.iter().map(|(n, _)| n.as_str()).collect()
}

/// A hibaüzenet (`error` esemény vagy `action_result` sikertelen).
pub fn message_of(events: &[(String, Value)]) -> Option<String> {
    events
        .iter()
        .find(|(n, d)| n == "error" || (n == "action_result" && d["success"] == json!(false)) || n.ends_with("_result"))
        .and_then(|(_, d)| d["message"].as_str().map(|s| s.to_string()))
}

// ===================================================================================================
// Állapot-segédek (a szerver memóriájának közvetlen kezelése, mint a Python tesztekben)
// ===================================================================================================

/// A játékos kezének felülírása a szerver állapotában (betűk; `""` a joker).
pub fn set_hand(server: &TestServer, name: &str, letters: &[&str]) {
    let mut st = server.app.state.lock();
    for room in st.rooms.values_mut() {
        for p in room.game.players.iter_mut() {
            if p.name == name {
                p.hand = letters.iter().map(|l| scrabble::tiles::Tile::from_str(l).expect("ismert betű")).collect();
            }
        }
    }
}

/// Az első (egyetlen) szoba soron lévő játékosának neve.
pub fn current_name(server: &TestServer) -> String {
    let st = server.app.state.lock();
    let room = st.rooms.values().next().expect("nincs szoba");
    room.game.current_player().expect("nincs soron lévő").name.clone()
}

/// Az `uN` játékos `Sio`-ja a soron lévő nevével.
pub fn by_name<'a>(players: &[&'a Sio], name: &str) -> &'a Sio {
    let n: u32 = name.trim_start_matches("Jatekos").parse().expect("Jatekos<N> név");
    players.iter().copied().find(|p| p.name_n == n).expect("nincs ilyen játékos")
}

/// A kéz betűi sztringként.
pub fn hand_of(server: &TestServer, name: &str) -> Vec<String> {
    let st = server.app.state.lock();
    for room in st.rooms.values() {
        for p in &room.game.players {
            if p.name == name {
                return p.hand.iter().map(|t| t.as_str().to_string()).collect();
            }
        }
    }
    Vec::new()
}

pub fn room_count(server: &TestServer) -> usize {
    server.app.state.lock().rooms.len()
}

pub fn with_room<R>(server: &TestServer, f: impl FnOnce(&mut scrabble::room::Room) -> R) -> R {
    let mut st = server.app.state.lock();
    let room = st.rooms.values_mut().next().expect("nincs szoba");
    f(room)
}

/// A `place_tiles` bemenete: egy szó vízszintesen a (sor, oszlop)-tól, betűnként egy zseton.
pub fn word_tiles(row: usize, col: usize, letters: &[&str]) -> Value {
    json!({"tiles": letters.iter().enumerate().map(|(i, l)| json!({"row": row, "col": col + i, "letter": l, "is_blank": false})).collect::<Vec<_>>()})
}
