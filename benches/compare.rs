//! Teljesítményteszt a Python és a Rust verzió között (`cargo bench --bench compare`).
//!
//! Mindkét szervert külön folyamatként indítja, ugyanarról a (Rusttal előállított) mintaadatbázisról, és ugyanazt a
//! terhelést adja rájuk:
//!
//! 1. **Indulás**: a folyamat indításától az első sikeres kérésig eltelt idő, és az üresjárati memória (RSS).
//! 2. **HTTP**: több végpont párhuzamos terheléssel (kérés / mp, késleltetés p50 / p95 / p99, a kiszolgáló CPU-ideje).
//! 3. **Socket.IO**: sok egyidejű játékos; párok teljes játéka (szoba, indítás, passzok, játék vége, mentés) és
//!    chat / szobalista körülfordulási idő (p50 / p95 / p99).
//! 4. **Robot**: a bot–bot játékok sebessége (`bot_arena`, egy szálon), és az eredmény egyezése (pont / kör).
//! 5. **Memória a terhelés végén**.
//!
//! A Python verziót a `python-final` git címke tartalmazza; a `scripts/perf.sh` ebből készít munkamásolatot és virtuális
//! környezetet. Közvetlenül: `cargo bench --bench compare -- --python-dir DIR --python-bin PYTHON [--out docs/PERFORMANCE.md]`.
//! Kapcsolók: `--rust-bin`, `--only rust|python`, `--duration MP` (végpontonként), `--pairs N`, `--arena-games N`, `--quick`.
//! A kérések `X-Forwarded-For` fejléccel különböző kliens-IP-ket mutatnak (különben az IP-alapú forgalomkorlát a
//! terhelést 429-cel megfojtaná; mindkét verzió a helyi proxy fejlécét használja).

use futures_util::{SinkExt, StreamExt};
use scrabble::db::{Db, GamePlayerData};
use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

const PASSWORD: &str = "secret12";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Python,
    Rust,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Python => "Python",
            Kind::Rust => "Rust",
        }
    }
}

struct Options {
    python_dir: Option<PathBuf>,
    python_bin: String,
    rust_bin: PathBuf,
    repo: PathBuf,
    only: Option<Kind>,
    duration: Duration,
    pairs: usize,
    users: usize,
    games: usize,
    arena_games: usize,
    out: Option<PathBuf>,
}

fn parse_options() -> Options {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut options = Options {
        python_dir: None,
        python_bin: "python3".into(),
        rust_bin: repo.join("target/release/scrabble"),
        repo,
        only: None,
        duration: Duration::from_secs(4),
        pairs: 40,
        users: 400,
        games: 4000,
        arena_games: 3,
        out: None,
    };
    let args: Vec<String> = std::env::args().skip(1).filter(|a| a != "--bench").collect();
    let mut i = 0;
    while i < args.len() {
        let value = args.get(i + 1).cloned();
        match args[i].as_str() {
            "--python-dir" => options.python_dir = value.map(PathBuf::from),
            "--python-bin" => options.python_bin = value.unwrap_or_default(),
            "--rust-bin" => options.rust_bin = value.map(PathBuf::from).unwrap_or_default(),
            "--only" => options.only = Some(if value.as_deref() == Some("python") { Kind::Python } else { Kind::Rust }),
            "--duration" => options.duration = Duration::from_secs(value.and_then(|v| v.parse().ok()).unwrap_or(4)),
            "--pairs" => options.pairs = value.and_then(|v| v.parse().ok()).unwrap_or(40),
            "--arena-games" => options.arena_games = value.and_then(|v| v.parse().ok()).unwrap_or(3),
            "--out" => options.out = value.map(PathBuf::from),
            "--quick" => {
                options.duration = Duration::from_secs(2);
                options.pairs = 10;
                options.users = 100;
                options.games = 800;
                options.arena_games = 1;
                i += 1;
                continue;
            }
            _ => {
                i += 1;
                continue;
            }
        }
        i += 2;
    }
    options
}

// ===================================================================================================
// Mintaadatbázis
// ===================================================================================================

/// A felhasználók (az első `pairs * 2` játékos belép a terhelésben), és befejezett játékok a ranglistához.
fn build_seed_db(path: &Path, options: &Options) {
    let _ = std::fs::remove_file(path);
    let db = Db::open(path.to_str().unwrap()).expect("adatbázis");
    db.init().expect("séma");
    let players = options.pairs * 2;
    for n in 0..players {
        db.create_user(&format!("perf{n}@example.com"), &format!("Játékos{n}"), PASSWORD).expect("felhasználó");
    }
    // a további felhasználók ugyanazzal a jelszó-hash-sel (a hash számítása drága)
    db.with(|tx| {
        for n in players..options.users {
            tx.execute(
                "INSERT INTO users (email, email_lower, display_name, password_hash) SELECT ?, ?, ?, password_hash FROM users WHERE id = 1",
                rusqlite::params![format!("perf{n}@example.com"), format!("perf{n}@example.com"), format!("Játékos{n}")],
            )?;
        }
        Ok(())
    })
    .expect("felhasználók");
    let mut seed = 12345u64;
    let mut next = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) as usize
    };
    for g in 0..options.games {
        let (a, b) = (next() % options.users, next() % options.users);
        if a == b {
            continue;
        }
        let (sa, sb) = (150 + (next() % 200) as i64, 150 + (next() % 200) as i64);
        let ids = [a as i64 + 1, b as i64 + 1];
        let players: Vec<GamePlayerData> = [(ids[0], sa), (ids[1], sb)]
            .iter()
            .map(|(uid, score)| GamePlayerData {
                player_name: format!("Játékos{}", uid - 1),
                user_id: Some(*uid),
                score: *score,
                is_winner: *score == sa.max(sb),
                resigned: false,
            })
            .collect();
        let state = json!({"players": players.iter().map(|p| json!({"name": p.player_name, "resigned": false, "is_bot": false})).collect::<Vec<_>>(), "finished": true});
        db.finish_game(&format!("seed{g}"), &state.to_string(), &players, &format!("Mintajáték {g}"), false).expect("játék");
    }
}

// ===================================================================================================
// Szerverfolyamat
// ===================================================================================================

struct Server {
    kind: Kind,
    child: Child,
    port: u16,
    started: Instant,
    #[allow(dead_code)]
    dir: PathBuf,
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

impl Server {
    fn start(kind: Kind, options: &Options, db_seed: &Path) -> Server {
        let dir = std::env::temp_dir().join(format!("scrabble-perf-{}-{}", kind.label(), std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("scrabble.db");
        std::fs::copy(db_seed, &db).unwrap();
        let port = free_port();
        let mut command = match kind {
            Kind::Rust => {
                let mut c = Command::new(&options.rust_bin);
                c.arg("--no-tunnel").current_dir(&options.repo).env("SCRABBLE_BASE_DIR", &options.repo);
                c
            }
            Kind::Python => {
                let mut c = Command::new(&options.python_bin);
                c.args(["server.py", "--no-tunnel"]).current_dir(options.python_dir.as_ref().expect("--python-dir"));
                c
            }
        };
        command
            .env("PORT", port.to_string())
            .env("SCRABBLE_DB_PATH", &db)
            .env("SCRABBLE_BACKUP_DIR", dir.join("backups"))
            .env("PYTHONUNBUFFERED", "1")
            .env_remove("ADMIN_EMAILS")
            .stdout(Stdio::from(std::fs::File::create(dir.join("out.log")).unwrap()))
            .stderr(Stdio::from(std::fs::File::create(dir.join("err.log")).unwrap()));
        let started = Instant::now();
        let child = command.spawn().unwrap_or_else(|e| panic!("{} indítása: {e}", kind.label()));
        Server { kind, child, port, started, dir }
    }

    fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Várakozás az első sikeres kérésre. Visszatér: az indulási idő.
    fn wait_ready(&mut self) -> Duration {
        let agent = agent();
        loop {
            if let Ok(Some(status)) = self.child.try_wait() {
                panic!("a(z) {} szerver kilépett ({status}): {}", self.kind.label(), std::fs::read_to_string(self.dir.join("err.log")).unwrap_or_default());
            }
            if let Ok(response) = agent.get(&format!("{}/api/announcements", self.base())).call()
                && response.status().as_u16() == 200
            {
                return self.started.elapsed();
            }
            assert!(self.started.elapsed() < Duration::from_secs(180), "a szerver nem indult el");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    /// A folyamat memóriahasználata (RSS, MiB).
    fn rss_mib(&self) -> f64 {
        let text = std::fs::read_to_string(format!("/proc/{}/status", self.pid())).unwrap_or_default();
        text.lines().find(|l| l.starts_with("VmRSS:")).and_then(|l| l.split_whitespace().nth(1)).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0) / 1024.0
    }

    /// A folyamat eddigi CPU-ideje (mp).
    fn cpu_secs(&self) -> f64 {
        let text = std::fs::read_to_string(format!("/proc/{}/stat", self.pid())).unwrap_or_default();
        let after = text.rsplit(')').next().unwrap_or("");
        let fields: Vec<&str> = after.split_whitespace().collect();
        let ticks = |i: usize| fields.get(i).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
        (ticks(11) + ticks(12)) / 100.0
    }

    fn threads(&self) -> usize {
        std::fs::read_dir(format!("/proc/{}/task", self.pid())).map(|d| d.count()).unwrap_or(0)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).timeout_global(Some(Duration::from_secs(60))).build().into()
}

// ===================================================================================================
// HTTP terhelés
// ===================================================================================================

#[derive(Default, Clone)]
struct Stats {
    requests: usize,
    errors: usize,
    seconds: f64,
    cpu: f64,
    latencies_ms: Vec<f64>,
}

impl Stats {
    fn rps(&self) -> f64 {
        self.requests as f64 / self.seconds.max(1e-9)
    }

    fn percentile(&self, p: f64) -> f64 {
        if self.latencies_ms.is_empty() {
            return 0.0;
        }
        let mut sorted = self.latencies_ms.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        sorted[((sorted.len() - 1) as f64 * p).round() as usize]
    }
}

struct Request {
    method: &'static str,
    path: String,
    body: Option<String>,
    cookie: Option<String>,
}

fn fake_ip(counter: u64) -> String {
    format!("10.{}.{}.{}", (counter >> 16) & 255, (counter >> 8) & 255, (counter & 255).max(1))
}

fn http_load(server: &Server, workers: usize, duration: Duration, make: impl Fn(u64) -> Request + Sync) -> Stats {
    let base = server.base();
    let counter = AtomicU64::new(1);
    let deadline = Instant::now() + duration;
    let cpu_before = server.cpu_secs();
    let started = Instant::now();
    let results: Vec<(usize, usize, Vec<f64>)> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let agent = agent();
                    let (mut ok, mut errors, mut latencies) = (0usize, 0usize, Vec::new());
                    while Instant::now() < deadline {
                        let n = counter.fetch_add(1, Ordering::Relaxed);
                        let request = make(n);
                        let mut builder =
                            ureq::http::Request::builder().method(request.method).uri(format!("{base}{}", request.path)).header("X-Forwarded-For", fake_ip(n));
                        if let Some(cookie) = &request.cookie {
                            builder = builder.header("Cookie", cookie);
                        }
                        let built = match &request.body {
                            Some(body) => builder.header("Content-Type", "application/json").body(body.clone().into_bytes()),
                            None => builder.body(Vec::new()),
                        }
                        .expect("kérés");
                        let t0 = Instant::now();
                        match agent.run(built) {
                            Ok(response) => {
                                let status = response.status().as_u16();
                                let _ = response.into_body().read_to_vec();
                                latencies.push(t0.elapsed().as_secs_f64() * 1000.0);
                                if status == 200 { ok += 1 } else { errors += 1 }
                            }
                            Err(_) => errors += 1,
                        }
                    }
                    (ok, errors, latencies)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let mut stats = Stats { seconds: started.elapsed().as_secs_f64(), cpu: server.cpu_secs() - cpu_before, ..Default::default() };
    for (ok, errors, latencies) in results {
        stats.requests += ok;
        stats.errors += errors;
        stats.latencies_ms.extend(latencies);
    }
    stats
}

fn get(path: &str) -> impl Fn(u64) -> Request + Sync + '_ {
    move |_| Request { method: "GET", path: path.to_string(), body: None, cookie: None }
}

const WORDS: [&str; 24] = [
    "alma", "körte", "szék", "asztal", "ablak", "barack", "szilva", "kutya", "macska", "ház", "kert", "folyó", "hegy", "tenger", "erdő", "város", "falu",
    "utca", "híd", "tér", "vonat", "busz", "hajó", "repülő",
];

fn dictionary_request(n: u64) -> Request {
    let words: Vec<String> =
        (0..8).map(|i| WORDS[((n as usize) * 3 + i) % WORDS.len()].to_string() + if (n + i as u64).is_multiple_of(5) { "k" } else { "" }).collect();
    Request { method: "POST", path: "/api/dictionary/check".into(), body: Some(json!({"words": words}).to_string()), cookie: None }
}

/// Egy felhasználó munkamenet-sütije.
fn login_cookie(base: &str, email: &str, ip: &str) -> Option<String> {
    let agent = agent();
    let request = ureq::http::Request::builder()
        .method("POST")
        .uri(format!("{base}/api/auth/login"))
        .header("Content-Type", "application/json")
        .header("X-Forwarded-For", ip)
        .body(json!({"email": email, "password": PASSWORD}).to_string().into_bytes())
        .unwrap();
    let response = agent.run(request).ok()?;
    if response.status().as_u16() != 200 {
        return None;
    }
    response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find_map(|c| c.strip_prefix("session_token=").map(|r| format!("session_token={}", r.split(';').next().unwrap_or(""))))
}

fn json_get(base: &str, path: &str, cookie: &str, ip: &str) -> Value {
    let request = ureq::http::Request::builder()
        .method("GET")
        .uri(format!("{base}{path}"))
        .header("Cookie", cookie)
        .header("X-Forwarded-For", ip)
        .body(Vec::new())
        .unwrap();
    let response = agent().run(request).expect("kérés");
    serde_json::from_slice(&response.into_body().read_to_vec().unwrap_or_default()).unwrap_or(Value::Null)
}

// ===================================================================================================
// Socket.IO terhelés
// ===================================================================================================

type Events = mpsc::UnboundedReceiver<(String, Value)>;

struct Client {
    tx: mpsc::UnboundedSender<String>,
    rx: Events,
}

async fn connect(port: u16, ip: &str) -> Client {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mut request = format!("ws://127.0.0.1:{port}/socket.io/?EIO=4&transport=websocket").into_client_request().unwrap();
    request.headers_mut().insert("X-Forwarded-For", ip.parse().unwrap());
    let (stream, _) = tokio_tungstenite::connect_async(request).await.expect("websocket");
    let (mut sink, mut source) = stream.split();
    let (tx, mut out_rx) = mpsc::unbounded_channel::<String>();
    let (ev_tx, rx) = mpsc::unbounded_channel::<(String, Value)>();
    tokio::spawn(async move {
        while let Some(text) = out_rx.recv().await {
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });
    let pong = tx.clone();
    tokio::spawn(async move {
        while let Some(Ok(message)) = source.next().await {
            let Message::Text(text) = message else { continue };
            let text = text.to_string();
            if text.starts_with('0') {
                let _ = pong.send("40".into());
            } else if text == "2" {
                let _ = pong.send("3".into());
            } else if let Some(body) = text.strip_prefix("42")
                && let Ok(Value::Array(mut parts)) = serde_json::from_str::<Value>(body)
            {
                if parts.is_empty() {
                    continue;
                }
                let name = parts.remove(0).as_str().unwrap_or("").to_string();
                let data = if parts.is_empty() { Value::Null } else { parts.remove(0) };
                let _ = ev_tx.send((name, data));
            }
        }
    });
    Client { tx, rx }
}

impl Client {
    fn emit(&self, event: &str, data: Value) {
        let payload = if data.is_null() { json!([event]) } else { json!([event, data]) };
        let _ = self.tx.send(format!("42{payload}"));
    }

    /// Várakozás egy névre; a közben érkező más események eldobódnak.
    async fn wait(&mut self, name: &str, secs: u64) -> Option<Value> {
        let end = tokio::time::Instant::now() + Duration::from_secs(secs);
        loop {
            let left = end.checked_duration_since(tokio::time::Instant::now())?;
            match tokio::time::timeout(left, self.rx.recv()).await {
                Ok(Some((n, data))) if n == name => return Some(data),
                Ok(Some(_)) => continue,
                _ => return None,
            }
        }
    }
}

struct Player {
    client: Client,
    user: usize,
}

async fn login_player(server_port: u16, base: &str, user: usize) -> Option<Player> {
    let (base, email, ip) = (base.to_string(), format!("perf{user}@example.com"), fake_ip(1_000_000 + user as u64));
    let (token, ip2) = tokio::task::spawn_blocking(move || {
        let cookie = login_cookie(&base, &email, &ip)?;
        let data = json_get(&base, "/api/auth/socket-token", &cookie, &ip);
        data["token"].as_str().map(|t| (t.to_string(), ip.clone()))
    })
    .await
    .ok()??;
    let mut client = connect(server_port, &ip2).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    client.emit("set_name", json!({"name": format!("Játékos{user}"), "is_guest": false, "auth_token": token}));
    tokio::time::sleep(Duration::from_millis(300)).await;
    while client.rx.try_recv().is_ok() {}
    Some(Player { client, user })
}

/// Egy pár teljes játéka: szoba, csatlakozás, indítás, passzok a játék végéig. Visszatér: a játék ideje (mp) a
/// kliensek két játékosától együtt, és a két kliens (a későbbi chat méréshez).
async fn play_pair(mut owner: Player, mut partner: Player) -> Option<(f64, Player, Player)> {
    owner.client.emit("create_room", json!({"name": format!("Perf {}", owner.user), "max_players": 2, "challenge_mode": false, "is_private": true}));
    let code = owner.client.wait("room_code", 20).await?["code"].as_str()?.to_string();
    partner.client.emit("join_room", json!({"code": code}));
    partner.client.wait("room_joined", 20).await?;
    let started = Instant::now();
    owner.client.emit("start_game", Value::Null);
    let (a, b) = (drive(&mut owner.client), drive(&mut partner.client));
    let (ra, rb) = tokio::join!(a, b);
    if !(ra && rb) {
        return None;
    }
    Some((started.elapsed().as_secs_f64(), owner, partner))
}

/// A kliens passzol, ha ő következik, a játék végéig. Igaz, ha a játék befejeződött.
async fn drive(client: &mut Client) -> bool {
    let mut last_turn = -1i64;
    let end = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        let Some(left) = end.checked_duration_since(tokio::time::Instant::now()) else { return false };
        let Ok(Some((name, state))) = tokio::time::timeout(left, client.rx.recv()).await else { return false };
        if name != "game_state" {
            continue;
        }
        if state["finished"] == json!(true) {
            return true;
        }
        let me = state["players"].as_array().and_then(|ps| ps.iter().find(|p| p.get("hand").is_some())).and_then(|p| p["id"].as_str().map(|s| s.to_string()));
        let turn = state["turn_number"].as_i64().unwrap_or(0);
        if me.is_some() && state["current_player"].as_str().map(|s| s.to_string()) == me && turn != last_turn {
            last_turn = turn;
            client.emit("pass_turn", Value::Null);
        }
    }
}

#[derive(Default)]
struct SocketResult {
    games: usize,
    game_seconds: Vec<f64>,
    wall: f64,
    chat_ms: Vec<f64>,
    rooms_ms: Vec<f64>,
    events: usize,
    clients: usize,
}

async fn socket_scenario(server: &Server, options: &Options) -> SocketResult {
    let (port, base) = (server.port, server.base());
    // belépés és kapcsolódás
    let mut players = Vec::new();
    let mut set = Vec::new();
    for user in 0..options.pairs * 2 {
        let base = base.clone();
        set.push(tokio::spawn(async move { login_player(port, &base, user).await }));
    }
    for handle in set {
        if let Ok(Some(player)) = handle.await {
            players.push(player);
        }
    }
    let mut result = SocketResult { clients: players.len(), ..Default::default() };
    // játékok
    let started = Instant::now();
    let cpu0 = server.cpu_secs();
    let mut games = Vec::new();
    while players.len() >= 2 {
        let a = players.remove(0);
        let b = players.remove(0);
        games.push(tokio::spawn(play_pair(a, b)));
    }
    let mut finished: Vec<Player> = Vec::new();
    for handle in games {
        if let Ok(Some((seconds, a, b))) = handle.await {
            result.games += 1;
            result.game_seconds.push(seconds);
            finished.push(a);
            finished.push(b);
        }
    }
    result.wall = started.elapsed().as_secs_f64();
    let _ = cpu0;
    // chat és szobalista körülfordulás
    let stop = Arc::new(AtomicBool::new(false));
    let chat = Arc::new(parking_lot::Mutex::new(Vec::<f64>::new()));
    let rooms = Arc::new(parking_lot::Mutex::new(Vec::<f64>::new()));
    let events = Arc::new(AtomicU64::new(0));
    let mut tasks = Vec::new();
    for (i, mut player) in finished.into_iter().enumerate() {
        let (stop, chat, rooms, events) = (stop.clone(), chat.clone(), rooms.clone(), events.clone());
        tasks.push(tokio::spawn(async move {
            let mut next_chat = tokio::time::Instant::now() + Duration::from_millis(37 * (i as u64 % 30));
            let mut next_rooms = next_chat + Duration::from_millis(500);
            let (mut chat_sent, mut rooms_sent): (Option<(String, Instant)>, Option<Instant>) = (None, None);
            let mut counter = 0u32;
            while !stop.load(Ordering::SeqCst) {
                let now = tokio::time::Instant::now();
                if now >= next_chat && chat_sent.is_none() {
                    counter += 1;
                    let text = format!("üzenet {i}-{counter}");
                    chat_sent = Some((text.clone(), Instant::now()));
                    player.client.emit("send_chat", json!({"message": text}));
                    next_chat = now + Duration::from_millis(1100);
                }
                if now >= next_rooms && rooms_sent.is_none() {
                    rooms_sent = Some(Instant::now());
                    player.client.emit("get_rooms", Value::Null);
                    next_rooms = now + Duration::from_millis(700);
                }
                if let Ok(Some((name, data))) = tokio::time::timeout(Duration::from_millis(20), player.client.rx.recv()).await {
                    events.fetch_add(1, Ordering::Relaxed);
                    if name == "chat_message" {
                        if let Some((text, t0)) = &chat_sent
                            && data["message"].as_str() == Some(text.as_str())
                        {
                            chat.lock().push(t0.elapsed().as_secs_f64() * 1000.0);
                            chat_sent = None;
                        }
                    } else if name == "rooms_list"
                        && let Some(t0) = rooms_sent.take()
                    {
                        rooms.lock().push(t0.elapsed().as_secs_f64() * 1000.0);
                    }
                }
            }
        }));
    }
    tokio::time::sleep(options.duration * 2).await;
    stop.store(true, Ordering::SeqCst);
    for task in tasks {
        let _ = task.await;
    }
    result.chat_ms = chat.lock().clone();
    result.rooms_ms = rooms.lock().clone();
    result.events = events.load(Ordering::Relaxed) as usize;
    result
}

fn percentile(values: &[f64], p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

// ===================================================================================================
// Robot-aréna
// ===================================================================================================

/// A bot–bot játékok ideje és a mért átlagos pont/kör (egy szálon). Visszatér: (mp, a fokozatok pont/kör értékei).
fn arena(kind: Kind, options: &Options) -> Option<(f64, Vec<f64>)> {
    let games = options.arena_games.to_string();
    let started = Instant::now();
    let output = match kind {
        Kind::Rust => {
            Command::new(options.repo.join("target/release/bot_arena")).args(["ladder", "-n", &games, "-j", "1"]).current_dir(&options.repo).output().ok()?
        }
        Kind::Python => Command::new(&options.python_bin)
            .args(["tools/bot_arena.py", "ladder", "-n", &games, "-j", "1"])
            .current_dir(options.python_dir.as_ref()?)
            .output()
            .ok()?,
    };
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    let values: Vec<f64> = text.lines().filter_map(|l| l.split_whitespace().nth(1)?.parse::<f64>().ok()).collect();
    Some((started.elapsed().as_secs_f64(), values))
}

// ===================================================================================================
// Mérés és jelentés
// ===================================================================================================

#[derive(Default)]
struct Report {
    cold_quiz_ms: f64,
    startup_ms: f64,
    idle_rss: f64,
    threads: usize,
    http: Vec<(String, Stats)>,
    rss_after_http: f64,
    socket: SocketResult,
    rss_after_socket: f64,
    cpu_total: f64,
    arena: Option<(f64, Vec<f64>)>,
}

fn measure(kind: Kind, options: &Options, db_seed: &Path) -> Report {
    eprintln!("== {} ==", kind.label());
    let mut server = Server::start(kind, options, db_seed);
    let startup = server.wait_ready();
    let mut report = Report { startup_ms: startup.as_secs_f64() * 1000.0, ..Default::default() };
    std::thread::sleep(Duration::from_millis(500));
    report.idle_rss = server.rss_mib();
    eprintln!("  indulás: {:.0} ms, memória {:.0} MiB", report.startup_ms, report.idle_rss);

    // bejelentkezett felhasználók a bejelentkezés-terheléshez
    let cookie = login_cookie(&server.base(), "perf0@example.com", "10.250.0.1");
    let duration = options.duration;
    let workers = 16;
    let mut run = |label: &str, workers: usize, make: &(dyn Fn(u64) -> Request + Sync)| {
        let stats = http_load(&server, workers, duration, make);
        eprintln!("  {label}: {:.0} kérés/mp, p50 {:.1} ms, p99 {:.1} ms, hiba {}", stats.rps(), stats.percentile(0.5), stats.percentile(0.99), stats.errors);
        report.http.push((label.to_string(), stats));
    };
    // az első (hideg) kérések: a szótár és a szókincs betöltése
    let cold = Instant::now();
    let _ = agent().get(&format!("{}/api/practice/quiz?n=10", server.base())).header("X-Forwarded-For", "10.251.0.1").call();
    let cold_ms = cold.elapsed().as_secs_f64() * 1000.0;
    eprintln!("  az első szókvíz kérés (szókincs betöltése): {cold_ms:.0} ms");
    report.cold_quiz_ms = cold_ms;
    run("GET / (index sablon)", workers, &get("/"));
    run("GET /static/app.js (statikus fájl)", workers, &get("/static/app.js"));
    run("GET /api/leaderboard (ranglista, DB)", workers, &get("/api/leaderboard?metric=wins"));
    run("POST /api/dictionary/check (8 szó)", workers, &dictionary_request);
    run("GET /api/practice/quiz (szókvíz)", workers, &get("/api/practice/quiz?n=10"));
    run("GET /api/practice/rack (betűvadász, szókeresés)", 8, &get("/api/practice/rack?kind=hunt"));
    run("GET /api/practice/short-words?length=3", workers, &get("/api/practice/short-words?length=3"));
    let _ = cookie;
    run("POST /api/auth/login (PBKDF2, CPU-igényes)", 8, &|n| Request {
        method: "POST",
        path: "/api/auth/login".into(),
        body: Some(json!({"email": format!("perf{}@example.com", n % 50), "password": PASSWORD}).to_string()),
        cookie: None,
    });
    report.rss_after_http = server.rss_mib();

    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(4).enable_all().build().unwrap();
    report.socket = runtime.block_on(socket_scenario(&server, options));
    eprintln!(
        "  Socket.IO: {} kliens, {} játék {:.1} mp alatt; chat p50 {:.1} ms p99 {:.1} ms",
        report.socket.clients,
        report.socket.games,
        report.socket.wall,
        percentile(&report.socket.chat_ms, 0.5),
        percentile(&report.socket.chat_ms, 0.99)
    );
    drop(runtime);
    report.rss_after_socket = server.rss_mib();
    report.threads = server.threads();
    report.cpu_total = server.cpu_secs();
    drop(server);
    report.arena = arena(kind, options);
    if let Some((seconds, values)) = &report.arena {
        eprintln!("  robot-aréna: {seconds:.1} mp ({} fokozat mérve)", values.len());
    }
    report
}

fn ratio(python: f64, rust: f64, higher_is_better: bool) -> String {
    if python <= 0.0 || rust <= 0.0 {
        return "–".into();
    }
    let r = if higher_is_better { rust / python } else { python / rust };
    format!("{r:.1}×")
}

fn row(label: &str, python: Option<f64>, rust: Option<f64>, unit: &str, higher_is_better: bool, digits: usize) -> String {
    let fmt = |v: Option<f64>| v.map(|v| format!("{v:.digits$} {unit}")).unwrap_or_else(|| "–".into());
    let r = match (python, rust) {
        (Some(p), Some(r)) => ratio(p, r, higher_is_better),
        _ => "–".into(),
    };
    format!("| {label} | {} | {} | {r} |\n", fmt(python), fmt(rust))
}

fn render(python: Option<&Report>, rust: Option<&Report>, options: &Options) -> String {
    let mut out = String::new();
    out.push_str("# Teljesítmény: Python és Rust verzió\n\n");
    out.push_str("A mérést a `cargo bench --bench compare` (`scripts/perf.sh`) állítja elő; mindkét szerver külön folyamat, ugyanarról a mintaadatbázisról indul, és ugyanazt a terhelést kapja. Az „arány” oszlop a Rust előnye (nagyobb = a Rust jobb).\n\n");
    let cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    let cpu_model = std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|t| t.lines().find(|l| l.starts_with("model name")).map(|l| l.split(':').nth(1).unwrap_or("").trim().to_string()))
        .unwrap_or_default();
    let python_version =
        Command::new(&options.python_bin).arg("--version").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    out.push_str(&format!(
        "- Gép: {cpu_model}, {cpus} logikai mag\n- Python: {python_version} (Flask + Socket.IO, gevent, egy folyamat)\n- Rust: axum + socketioxide (tokio, többszálú), `--release`\n- Terhelés: {} mp végpontonként, 16 párhuzamos kapcsolat; {} pár egyidejű játék; mintaadatbázis: {} felhasználó, {} befejezett játék\n- Dátum: {}\n\n",
        options.duration.as_secs(),
        options.pairs,
        options.users,
        options.games,
        chrono::Utc::now().format("%Y-%m-%d")
    ));
    out.push_str("## Összefoglaló\n\n| Mérőszám | Python | Rust | arány |\n|---|---|---|---|\n");
    let p = |f: &dyn Fn(&Report) -> f64| python.map(f);
    let r = |f: &dyn Fn(&Report) -> f64| rust.map(f);
    out.push_str(&row("Indulás az első kérésig", p(&|x| x.startup_ms), r(&|x| x.startup_ms), "ms", false, 0));
    out.push_str(&row("Memória üresjáratban (RSS)", p(&|x| x.idle_rss), r(&|x| x.idle_rss), "MiB", false, 0));
    out.push_str(&row("Memória a HTTP terhelés után", p(&|x| x.rss_after_http), r(&|x| x.rss_after_http), "MiB", false, 0));
    out.push_str(&row("Memória a Socket.IO terhelés után", p(&|x| x.rss_after_socket), r(&|x| x.rss_after_socket), "MiB", false, 0));
    out.push_str(&row("Az első szókvíz-kérés (szókincs betöltése)", p(&|x| x.cold_quiz_ms), r(&|x| x.cold_quiz_ms), "ms", false, 0));
    let cpu_per_request = |x: &Report| {
        let (cpu, requests) = x.http.iter().fold((0.0, 0usize), |(c, n), (_, s)| (c + s.cpu, n + s.requests));
        if requests > 0 { cpu / requests as f64 * 1000.0 } else { 0.0 }
    };
    out.push_str(&row("CPU-idő kérésenként (a HTTP végpontok átlaga)", p(&cpu_per_request), r(&cpu_per_request), "ms", false, 2));
    let games = |x: &Report| if x.socket.wall > 0.0 { x.socket.games as f64 / x.socket.wall } else { 0.0 };
    out.push_str(&row("Teljes játékok (szoba + passzok + mentés)", p(&games), r(&games), "játék/mp", true, 1));
    out.push_str(&row("Egy játék ideje (átlag)", p(&|x| mean(&x.socket.game_seconds)), r(&|x| mean(&x.socket.game_seconds)), "mp", false, 2));
    out.push_str(&row("Chat körülfordulás p50", p(&|x| percentile(&x.socket.chat_ms, 0.5)), r(&|x| percentile(&x.socket.chat_ms, 0.5)), "ms", false, 1));
    out.push_str(&row("Chat körülfordulás p99", p(&|x| percentile(&x.socket.chat_ms, 0.99)), r(&|x| percentile(&x.socket.chat_ms, 0.99)), "ms", false, 1));
    out.push_str(&row(
        "Szobalista körülfordulás p99",
        p(&|x| percentile(&x.socket.rooms_ms, 0.99)),
        r(&|x| percentile(&x.socket.rooms_ms, 0.99)),
        "ms",
        false,
        1,
    ));
    out.push_str(&row(
        "Robot-aréna (bot–bot játékok, egy szál)",
        python.and_then(|x| x.arena.as_ref().map(|a| a.0)),
        rust.and_then(|x| x.arena.as_ref().map(|a| a.0)),
        "mp",
        false,
        1,
    ));
    out.push_str("\n## HTTP végpontok\n\n| Végpont | Python kérés/mp | Rust kérés/mp | arány | Python p50 / p99 | Rust p50 / p99 | CPU-mp / 1000 kérés (Py → Rust) |\n|---|---|---|---|---|---|---|\n");
    let labels: Vec<String> = rust.or(python).map(|x| x.http.iter().map(|(l, _)| l.clone()).collect()).unwrap_or_default();
    for label in labels {
        let find = |rep: Option<&Report>| rep.and_then(|x| x.http.iter().find(|(l, _)| *l == label).map(|(_, s)| s.clone()));
        let (ps, rs) = (find(python), find(rust));
        let cell = |s: &Option<Stats>| s.as_ref().map(|s| format!("{:.0}", s.rps())).unwrap_or_else(|| "–".into());
        let lat = |s: &Option<Stats>| s.as_ref().map(|s| format!("{:.1} / {:.1} ms", s.percentile(0.5), s.percentile(0.99))).unwrap_or_else(|| "–".into());
        let cpu = |s: &Option<Stats>| s.as_ref().map(|s| if s.requests > 0 { s.cpu / s.requests as f64 * 1000.0 } else { 0.0 }).unwrap_or(0.0);
        let r = match (&ps, &rs) {
            (Some(p), Some(r)) => ratio(p.rps(), r.rps(), true),
            _ => "–".into(),
        };
        out.push_str(&format!("| {label} | {} | {} | {r} | {} | {} | {:.2} → {:.2} |\n", cell(&ps), cell(&rs), lat(&ps), lat(&rs), cpu(&ps), cpu(&rs)));
    }
    out.push_str("\n## Robot-aréna: a fokozatok ereje (pont/kör)\n\nA két verzió robotja ugyanazt a skálát adja (a kis játékszám miatt kis eltérés természetes).\n\n| Fokozat | Python | Rust |\n|---|---|---|\n");
    let strengths = |x: Option<&Report>| x.and_then(|r| r.arena.as_ref().map(|a| a.1.clone())).unwrap_or_default();
    let (ps, rs) = (strengths(python), strengths(rust));
    for level in 0..ps.len().max(rs.len()) {
        let fmt = |v: &Vec<f64>| v.get(level).map(|x| format!("{x:.1}")).unwrap_or_else(|| "–".into());
        out.push_str(&format!("| {} | {} | {} |\n", level + 1, fmt(&ps), fmt(&rs)));
    }
    out.push_str("\n## Megjegyzések\n\n- A Python verzió egyetlen gevent folyamat: a CPU-igényes kérések (szókeresés, jelszó-hash, robot) egyetlen magot használnak, és egymást várakoztatják. A Rust szerver többszálú, ezért a párhuzamos terhelésnél a magok számával is skálázódik.\n- Mindkét szerver az első kérés előtt betölti a szótárat, a robot szókincsét (a ragozott alakokkal) és a napi feladványt; az indulási idő ezt is tartalmazza.\n- A ranglista végpont ideje főleg az SQLite összesítő lekérdezésére megy el (mindkét verzió ugyanazt a sémát és lekérdezést használja), ezért ott kisebb az eltérés.\n- A kérések `X-Forwarded-For` fejlécével különböző kliens-IP-k látszanak, így az IP-alapú forgalomkorlát nem torzítja a méréseket.\n- A Python verzió a `python-final` git címkén van; a mintaadatbázist a Rust kód állítja elő, és mindkét szerver ugyanazt a fájlt használja (a két verzió adatbázis-sémája azonos).\n- Újrafuttatás: `scripts/perf.sh --out docs/PERFORMANCE.md` (gyors próba: `--quick`).\n");
    out
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len().max(1) as f64
}

fn main() {
    let options = parse_options();
    if !options.rust_bin.exists() && options.only != Some(Kind::Python) {
        eprintln!("hiányzik a Rust szerver ({}): előbb `cargo build --release`", options.rust_bin.display());
        std::process::exit(1);
    }
    let dir = std::env::temp_dir().join(format!("scrabble-perf-seed-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let seed = dir.join("seed.db");
    eprintln!("mintaadatbázis: {} felhasználó, {} játék …", options.users, options.games);
    build_seed_db(&seed, &options);

    let run_python = options.python_dir.is_some() && options.only != Some(Kind::Rust);
    let run_rust = options.only != Some(Kind::Python);
    let python = if run_python { Some(measure(Kind::Python, &options, &seed)) } else { None };
    let rust = if run_rust { Some(measure(Kind::Rust, &options, &seed)) } else { None };
    let text = render(python.as_ref(), rust.as_ref(), &options);
    println!("{text}");
    if let Some(path) = &options.out {
        std::fs::File::create(path).and_then(|mut f| f.write_all(text.as_bytes())).expect("jelentés írása");
        eprintln!("jelentés: {}", path.display());
    }
    let _ = std::fs::remove_dir_all(&dir);
}
