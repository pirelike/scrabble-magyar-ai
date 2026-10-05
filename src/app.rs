//! Az alkalmazás közös állapota: konfiguráció, adatbázis, beállítások, szerver állapot, forgalomkorlát, Socket.IO
//! kibocsátó, push és levelezés. Az eseménykezelők és a HTTP útvonalak ezt kapják.

use crate::admin::moderation::BannedWords;
use crate::admin::security::IpBans;
use crate::config::{self, Config};
use crate::db::Db;
use crate::mail::Mailer;
use crate::push::Push;
use crate::ratelimit::RateLimiter;
use crate::settings::Settings;
use crate::state::ServerState;
use crate::tunnel::Tunnel;
use crate::util;
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::Value;
use socketioxide::SocketIo;
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

/// A Socket.IO események (SID-enkénti) forgalomkorlátai: (kérés, ablak mp).
pub const SOCKET_RATE_LIMITS: &[(&str, (u32, u32))] = &[
    ("set_name", (5, 10)),
    ("create_room", (3, 30)),
    ("join_room", (5, 10)),
    ("place_tiles", (10, 10)),
    ("exchange_tiles", (5, 10)),
    ("pass_turn", (5, 10)),
    ("get_rooms", (10, 5)),
    ("accept_words", (5, 10)),
    ("reject_words", (5, 10)),
    ("withdraw_words", (5, 10)),
    ("send_chat", (10, 10)),
    ("rejoin_room", (5, 10)),
    ("save_game", (3, 30)),
    ("restore_game", (3, 30)),
    ("send_friend_request", (5, 30)),
    ("accept_friend_request", (10, 10)),
    ("decline_friend_request", (10, 10)),
    ("remove_friend", (5, 30)),
    ("invite_to_room", (10, 30)),
    ("respond_invite", (10, 10)),
    ("preview_move", (30, 10)),
    ("request_hint", (3, 30)),
    ("set_visibility", (20, 10)),
    ("create_async_game", (3, 30)),
    ("open_async_game", (10, 10)),
    ("resign_game", (3, 30)),
    ("start_daily", (3, 30)),
    ("retry_daily", (10, 30)),
    ("reveal_daily", (5, 30)),
    ("spectate_room", (5, 10)),
    ("leave_spectate", (5, 10)),
    ("admin_subscribe", (5, 10)),
    ("admin_watch_room", (20, 10)),
    ("report_content", (3, 60)),
];

/// A futó (vagy hibás) játékelemzés állapota.
#[derive(Clone, Debug)]
pub enum AnalysisJob {
    Running { done: usize, total: usize },
    Failed { at: f64 },
}

/// Egy háttérfolyamat állapota az admin rendszer-nézetnek.
#[derive(Clone, Debug)]
pub struct JobStatus {
    pub last_run: f64,
    pub ok: bool,
    pub detail: Option<String>,
    pub runs: u64,
    pub failures: u64,
}

pub struct App {
    pub config: Arc<Config>,
    pub db: Arc<Db>,
    pub settings: Settings,
    pub state: Mutex<ServerState>,
    pub limiter: RateLimiter,
    pub mailer: Arc<Mailer>,
    pub push: Arc<Push>,
    pub tunnel: Tunnel,
    pub banned_words: BannedWords,
    pub ip_bans: IpBans,
    pub base_dir: PathBuf,
    pub started_at: f64,
    pub analysis_jobs: Mutex<HashMap<i64, AnalysisJob>>,
    pub jobs: Mutex<HashMap<String, JobStatus>>,
    /// a futó folyamat indulási commitja (frissítés után az újraindítás jelzéséhez)
    pub running_commit: Mutex<Option<String>>,
    /// a levelezős játék kezdő játékosa a létrehozás sorrendjében (`None`: véletlen); a tesztek állítják be
    pub async_starter: Mutex<Option<usize>>,
    /// a most feldolgozás alatt álló Socket.IO események száma (a tesztek ebből tudják, mikor ért véget a munka)
    pub events_in_flight: AtomicUsize,
    io: OnceLock<SocketIo>,
}

/// A feldolgozás alatt álló esemény jelzése (a hatókör végén csökken).
pub struct InFlight<'a>(&'a AtomicUsize);

impl<'a> InFlight<'a> {
    pub fn new(counter: &'a AtomicUsize) -> InFlight<'a> {
        counter.fetch_add(1, Ordering::SeqCst);
        InFlight(counter)
    }
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl App {
    /// Új alkalmazás-állapot a megadott konfigurációval és adatbázissal.
    pub fn new(config: Arc<Config>, db: Arc<Db>) -> Arc<App> {
        App::with_base_dir(config, db, config::base_dir().to_path_buf())
    }

    /// Ugyanez a program mappájának megadásával (a frissítés tesztjeihez: ideiglenes git tár).
    pub fn with_base_dir(config: Arc<Config>, db: Arc<Db>, base_dir: PathBuf) -> Arc<App> {
        let settings = Settings::new(db.clone(), config.word_reject_threshold);
        let mailer = Arc::new(Mailer::new(db.clone(), config.clone()));
        let mailer_for_push = mailer.clone();
        let push = Arc::new(Push::new(db.clone(), Box::new(move || mailer_for_push.from_address())));
        let ip_limits: Vec<(&str, (u32, u32))> = config::AUTH_RATE_LIMITS.to_vec();
        Arc::new(App {
            config,
            db,
            settings,
            state: Mutex::new(ServerState::new()),
            limiter: RateLimiter::new(SOCKET_RATE_LIMITS, &ip_limits),
            mailer,
            push,
            tunnel: Tunnel::new(),
            banned_words: BannedWords::default(),
            ip_bans: IpBans::default(),
            base_dir,
            started_at: util::now(),
            analysis_jobs: Mutex::new(HashMap::new()),
            jobs: Mutex::new(HashMap::new()),
            running_commit: Mutex::new(None),
            async_starter: Mutex::new(None),
            events_in_flight: AtomicUsize::new(0),
            io: OnceLock::new(),
        })
    }

    // --- Socket.IO ---

    /// A Socket.IO kibocsátó beállítása (egyszer, a szerver indításakor).
    pub fn set_io(&self, io: SocketIo) {
        let _ = self.io.set(io);
    }

    pub fn io(&self) -> Option<&SocketIo> {
        self.io.get()
    }

    /// Esemény egyetlen kapcsolatnak. Az elküldés szinkron (a sorrend megmarad: a kapcsolat saját sorába kerül).
    pub fn emit_to<T: Serialize + ?Sized>(&self, sid: &str, event: &str, data: &T) {
        if let Some(socket) = self.socket(sid) {
            let _ = socket.emit(event, data);
        }
    }

    /// Esemény egy szobának (Socket.IO szoba).
    pub fn emit_room<T: Serialize + ?Sized>(&self, room: &str, event: &str, data: &T) {
        if let Some(io) = self.io.get() {
            for socket in io.to(room.to_string()).sockets() {
                let _ = socket.emit(event, data);
            }
        }
    }

    /// Esemény minden csatlakozott kapcsolatnak.
    pub fn emit_all<T: Serialize + ?Sized>(&self, event: &str, data: &T) {
        if let Some(io) = self.io.get() {
            for socket in io.sockets() {
                let _ = socket.emit(event, data);
            }
        }
    }

    pub fn emit_error(&self, sid: &str, message: &str) {
        self.emit_to(sid, "error", &serde_json::json!({"message": message}));
    }

    /// A kapcsolat belépése egy Socket.IO szobába (háttérszálból is hívható).
    pub fn join_room(&self, sid: &str, room: &str) {
        if let Some(socket) = self.socket(sid) {
            socket.join(room.to_string());
        }
    }

    /// A kapcsolat kilépése egy Socket.IO szobából.
    pub fn leave_room(&self, sid: &str, room: &str) {
        if let Some(socket) = self.socket(sid) {
            socket.leave(room.to_string());
        }
    }

    /// A kapcsolat bezárása a szerver oldaláról.
    pub fn disconnect_sid(&self, sid: &str) {
        if let Some(socket) = self.socket(sid) {
            let _ = socket.disconnect();
        }
    }

    fn socket(&self, sid: &str) -> Option<socketioxide::extract::SocketRef> {
        let io = self.io.get()?;
        let id = socketioxide::socket::Sid::from_str(sid).ok()?;
        io.get_socket(id)
    }

    /// Hány kapcsolat van az adott Socket.IO szobában (admin: élő tagok).
    pub fn room_member_count(&self, room: &str) -> usize {
        let Some(io) = self.io.get() else { return 0 };
        io.to(room.to_string()).sockets().len()
    }

    // --- Háttérfolyamatok állapota (admin rendszer-nézet) ---

    pub fn job_ran(&self, name: &str, ok: bool, detail: Option<String>) {
        let mut jobs = self.jobs.lock();
        let entry = jobs.entry(name.to_string()).or_insert(JobStatus { last_run: 0.0, ok: true, detail: None, runs: 0, failures: 0 });
        entry.last_run = util::now();
        entry.ok = ok;
        entry.detail = detail;
        entry.runs += 1;
        if !ok {
            entry.failures += 1;
        }
    }

    // --- Futásidejű beállítások alkalmazása ---

    /// A beállításokból a futás közben érvényesülő részek újraalkalmazása: forgalomkorlátok (a chat sebességkorlátja
    /// is), a szótár-építő kizárási küszöbe. Szerverindításkor és minden beállítás-módosítás után hívódik.
    pub fn apply_runtime_settings(&self) {
        self.settings.invalidate();
        let to_map = |value: Value| -> HashMap<String, (u32, u32)> {
            value
                .as_object()
                .map(|o| {
                    o.iter()
                        .filter_map(|(k, v)| {
                            let pair = v.as_array()?;
                            Some((k.clone(), (pair.first()?.as_u64()? as u32, pair.get(1)?.as_u64()? as u32)))
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        let http = to_map(self.settings.get("rate_limits_http"));
        let mut socket = to_map(self.settings.get("rate_limits_socket"));
        if self.settings.is_overridden("chat_rate_count") || self.settings.is_overridden("chat_rate_window") {
            socket
                .entry("send_chat".to_string())
                .or_insert((self.settings.get_i64("chat_rate_count") as u32, self.settings.get_i64("chat_rate_window") as u32));
        }
        self.limiter.set_overrides(http, socket);
        self.banned_words.invalidate();
        crate::word_review::refresh(&self.db, &self.settings);
    }

    // --- Gyakori lekérdezések ---

    pub fn is_admin_email(&self, email: &str) -> bool {
        crate::db::is_admin_email(email, &self.config.admin_emails)
    }

    /// A tiltott szavak (ékezet- és kisbetű nélküli alakban).
    pub fn banned_word_list(&self) -> Vec<String> {
        self.banned_words.words(&self.db)
    }
}
