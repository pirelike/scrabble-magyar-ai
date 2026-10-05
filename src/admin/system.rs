//! Admin panel: a rendszer állapota — szerver, adatbázis (mentés, takarítás), naplók, folyamatok, konfiguráció,
//! verziók. Csak olvasható / karbantartó funkciók; a futó játékokat nem érintik.

use super::*;
use crate::app::{AnalysisJob, App};
use crate::db::{fetch_all, fetch_one, RowExt};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use regex::Regex;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::OnceLock;

// ===== Szerver naplója: gyűrűpuffer =====

pub const LOG_BUFFER_SIZE: usize = 2000;
pub const LOG_LEVELS: [&str; 5] = ["DEBUG", "INFO", "WARNING", "ERROR", "CRITICAL"];
const MAX_LOG_MESSAGE: usize = 2000;

fn level_order(level: &str) -> usize {
    LOG_LEVELS.iter().position(|l| *l == level).unwrap_or(1)
}

static PRINT_ERROR_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\b(hiba|error|traceback|exception|panicked)\b").expect("érvényes regex"));
static PRINT_WARN_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\b(figyelem|warning|warn)\b").expect("érvényes regex"));

#[derive(Clone, Debug)]
pub struct LogRecord {
    pub id: u64,
    pub ts: f64,
    pub level: String,
    pub logger: String,
    pub message: String,
    pub exc_type: Option<String>,
    pub location: Option<String>,
}

impl LogRecord {
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id, "ts": self.ts, "level": self.level, "logger": self.logger, "message": self.message,
            "exc_type": self.exc_type, "location": self.location,
        })
    }
}

/// A szerver naplósorainak gyűrűpuffere (az admin panel „Naplók” nézetéhez).
pub struct LogBuffer {
    inner: Mutex<(VecDeque<LogRecord>, u64)>,
    size: usize,
}

impl LogBuffer {
    pub fn new(size: usize) -> LogBuffer {
        LogBuffer { inner: Mutex::new((VecDeque::new(), 0)), size }
    }

    pub fn add(&self, level: &str, logger: &str, message: &str, exc_type: Option<&str>, location: Option<&str>, ts: Option<f64>) {
        let mut inner = self.inner.lock();
        inner.1 += 1;
        let id = inner.1;
        inner.0.push_back(LogRecord {
            id,
            ts: ts.unwrap_or_else(util::now),
            level: level.to_string(),
            logger: logger.to_string(),
            message: message.chars().take(MAX_LOG_MESSAGE).collect(),
            exc_type: exc_type.map(|s| s.to_string()),
            location: location.map(|s| s.to_string()),
        });
        while inner.0.len() > self.size {
            inner.0.pop_front();
        }
    }

    /// A legutóbbi sorok (régebbitől az újabbig), szint szerint (az adott szinttől felfelé) és szöveg szerint szűrve.
    pub fn query(&self, level: Option<&str>, q: Option<&str>, since_id: u64, limit: usize) -> Vec<LogRecord> {
        let floor = level.map(|l| LOG_LEVELS.iter().position(|x| *x == l).unwrap_or(0)).unwrap_or(0);
        let needle = q.unwrap_or("").to_lowercase();
        let records: Vec<LogRecord> = self.inner.lock().0.iter().cloned().collect();
        let rows: Vec<LogRecord> = records
            .into_iter()
            .filter(|r| {
                r.id > since_id
                    && level_order(&r.level) >= floor
                    && (needle.is_empty() || r.message.to_lowercase().contains(&needle) || r.logger.to_lowercase().contains(&needle))
            })
            .collect();
        let skip = rows.len().saturating_sub(limit);
        rows.into_iter().skip(skip).collect()
    }

    /// A hibák csoportosítva (kivétel típusa + hely), darabszámmal és az utolsó előfordulással.
    pub fn error_groups(&self) -> Vec<Value> {
        let records: Vec<LogRecord> = self.inner.lock().0.iter().cloned().collect();
        let mut order: Vec<(String, String, String)> = Vec::new();
        let mut groups: std::collections::HashMap<(String, String, String), (Value, i64, f64)> = std::collections::HashMap::new();
        for r in records {
            if level_order(&r.level) < level_order("ERROR") {
                continue;
            }
            let key = (
                r.exc_type.clone().unwrap_or_default(),
                r.location.clone().unwrap_or_default(),
                if r.exc_type.is_some() { String::new() } else { r.message.chars().take(80).collect() },
            );
            let entry = groups.entry(key.clone()).or_insert_with(|| {
                order.push(key.clone());
                (
                    json!({"exc_type": r.exc_type, "location": r.location, "sample": r.message.chars().take(300).collect::<String>(), "logger": r.logger}),
                    0,
                    0.0,
                )
            });
            entry.1 += 1;
            entry.2 = entry.2.max(r.ts);
        }
        let mut list: Vec<Value> = order
            .into_iter()
            .filter_map(|key| groups.remove(&key))
            .map(|(mut group, count, last_ts)| {
                group["count"] = json!(count);
                group["last_ts"] = json!(last_ts);
                group
            })
            .collect();
        list.sort_by(|a, b| b["last_ts"].as_f64().partial_cmp(&a["last_ts"].as_f64()).unwrap_or(std::cmp::Ordering::Equal));
        list
    }
}

pub static LOG_BUFFER: Lazy<LogBuffer> = Lazy::new(|| LogBuffer::new(LOG_BUFFER_SIZE));

/// A konzolra írt üzenetek gyűjtése az admin panel naplónézetéhez: a folyamat stdout / stderr kimenete egy csövön
/// megy át, amelyet egy szál másol a gyűrűpufferbe és az eredeti kimenetre (a Python `capture_output` megfelelője).
/// Az eredeti stdout / stderr fájlleírója (újraindítás előtt visszaállítjuk, hogy a csőre írás ne törjön meg).
#[cfg(unix)]
static ORIGINAL_FDS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(-1);

/// A kimenet visszaállítása az eredeti fájlleírókra (a naplógyűjtés előtti állapot). Újraindítás előtt hívandó.
pub fn restore_output() {
    #[cfg(unix)]
    {
        let packed = ORIGINAL_FDS.load(std::sync::atomic::Ordering::SeqCst);
        if packed >= 0 {
            let (out, err) = ((packed >> 32) as i32, (packed & 0xffff_ffff) as i32);
            // SAFETY: a mentett leírók a naplógyűjtés indításakor készültek és még nyitva vannak.
            unsafe {
                libc::dup2(out, 1);
                libc::dup2(err, 2);
            }
        }
    }
}

pub fn install_log_capture(_app: &Arc<App>) {
    #[cfg(unix)]
    {
        use std::io::{BufRead, BufReader, Write};
        use std::os::fd::FromRawFd;
        static INSTALLED: OnceLock<()> = OnceLock::new();
        if INSTALLED.set(()).is_err() {
            return;
        }
        // SAFETY: egyszerű POSIX fájlleíró-műveletek az indulás elején, a szálak előtt; a leírók érvényességét ellenőrizzük.
        unsafe {
            let mut fds = [0i32; 2];
            if libc::pipe(fds.as_mut_ptr()) != 0 {
                return;
            }
            let (read_fd, write_fd) = (fds[0], fds[1]);
            let saved_out = libc::dup(1);
            let saved_err = libc::dup(2);
            if saved_out < 0 || saved_err < 0 {
                return;
            }
            ORIGINAL_FDS.store(((saved_out as i64) << 32) | saved_err as i64, std::sync::atomic::Ordering::SeqCst);
            libc::dup2(write_fd, 1);
            libc::dup2(write_fd, 2);
            libc::close(write_fd);
            let reader = std::fs::File::from_raw_fd(read_fd);
            let mut original = std::fs::File::from_raw_fd(saved_out);
            std::thread::Builder::new()
                .name("log-capture".into())
                .spawn(move || {
                    let mut lines = BufReader::new(reader);
                    let mut raw = Vec::new();
                    loop {
                        raw.clear();
                        match lines.read_until(b'\n', &mut raw) {
                            Ok(0) | Err(_) => break,
                            Ok(_) => {}
                        }
                        let _ = original.write_all(&raw);
                        let _ = original.flush();
                        let line = String::from_utf8_lossy(&raw);
                        let line = line.trim();
                        if !line.is_empty() {
                            let level = if PRINT_ERROR_RE.is_match(line) {
                                "ERROR"
                            } else if PRINT_WARN_RE.is_match(line) {
                                "WARNING"
                            } else {
                                "INFO"
                            };
                            LOG_BUFFER.add(level, "stdout", line, None, None, None);
                        }
                    }
                })
                .ok();
        }
    }
}

// ===== Folyamatok (háttérfeladatok) =====

pub const KNOWN_JOBS: [&str; 3] = ["async_sweeper", "admin_broadcaster", "maintenance_tasks"];

pub fn jobs_status(app: &App) -> Vec<Value> {
    let jobs = app.jobs.lock();
    KNOWN_JOBS
        .iter()
        .map(|name| match jobs.get(*name) {
            Some(job) => json!({"name": name, "runs": job.runs, "errors": job.failures, "last_run": job.last_run, "ok": job.ok, "info": job.detail}),
            None => json!({"name": name, "runs": 0, "errors": 0, "last_run": null, "ok": null, "info": null}),
        })
        .collect()
}

/// A futó (és hibás) játékelemzések: [{game_id, done, total, error}].
pub fn analysis_queue(app: &App) -> Vec<Value> {
    app.analysis_jobs
        .lock()
        .iter()
        .map(|(id, job)| match job {
            AnalysisJob::Running { done, total } => json!({"game_id": id, "done": done, "total": total, "error": false}),
            AnalysisJob::Failed { .. } => json!({"game_id": id, "done": 0, "total": 0, "error": true}),
        })
        .collect()
}

// ===== Szerver =====

fn rss_bytes() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/self/status").ok()?;
    text.lines().find(|l| l.starts_with("VmRSS:"))?.split_whitespace().nth(1)?.parse::<u64>().ok().map(|kb| kb * 1024)
}

fn thread_count() -> Option<usize> {
    Some(std::fs::read_dir("/proc/self/task").ok()?.count())
}

/// A folyamat eddig felhasznált CPU-ideje (mp).
fn process_cpu_seconds() -> Option<f64> {
    #[cfg(unix)]
    {
        let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        // SAFETY: érvényes mutató a lokális `ts`-re.
        let ok = unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut ts) } == 0;
        ok.then(|| ts.tv_sec as f64 + ts.tv_nsec as f64 / 1e9)
    }
    #[cfg(not(unix))]
    {
        None
    }
}

static CPU_STATE: Lazy<Mutex<(Option<f64>, Option<f64>, Option<f64>)>> = Lazy::new(|| Mutex::new((None, None, None)));

/// A folyamat átlagos CPU-terhelése az előző hívás óta (%).
fn cpu_percent() -> Option<f64> {
    let (wall, cpu) = (util::now(), process_cpu_seconds()?);
    let mut state = CPU_STATE.lock();
    let (last_wall, last_cpu, percent) = *state;
    *state = (Some(wall), Some(cpu), percent);
    match (last_wall, last_cpu) {
        (Some(lw), Some(lc)) if wall - lw >= 0.5 => {
            let value = ((100.0 * (cpu - lc) / (wall - lw)) * 10.0).round() / 10.0;
            state.2 = Some(value);
            Some(value)
        }
        _ => percent,
    }
}

pub fn db_file_size(app: &App) -> u64 {
    ["", "-wal", "-shm"].iter().filter_map(|suffix| std::fs::metadata(format!("{}{}", app.db.path, suffix)).ok()).map(|m| m.len()).sum()
}

pub fn runtime_version() -> String {
    format!("Rust {}", env!("SCRABBLE_RUSTC_VERSION"))
}

pub fn process_info(app: &App) -> Value {
    let tasks = tokio::runtime::Handle::try_current().ok().map(|h| h.metrics().num_alive_tasks());
    json!({
        "uptime": (util::now() - app.started_at) as i64, "rss": rss_bytes(), "cpu_percent": cpu_percent(),
        "greenlets": tasks, "threads": thread_count(), "db_size": db_file_size(app), "python": runtime_version(),
    })
}

/// A szolgáltatások elérhetősége: szótár, robot szókincse, push, SMTP, tunnel.
pub fn services_info(app: &App) -> Value {
    json!({
        "dictionary": crate::dictionary::is_available(),
        "vocabulary": crate::ai::vocabulary_loaded(),
        "push": app.push.is_available(),
        "smtp": app.mailer.is_configured(),
        "tunnel": app.tunnel.status(),
    })
}

// ===== Verziók =====

static GIT_COMMIT: OnceLock<Option<String>> = OnceLock::new();

/// A futó kód git commitja (rövid azonosító + az első sor), vagy None. Egyszer olvasódik.
pub fn git_commit(app: &App) -> Option<String> {
    GIT_COMMIT
        .get_or_init(|| {
            let out = std::process::Command::new("git").arg("-C").arg(&app.base_dir).args(["log", "-1", "--format=%h %s"]).output().ok()?;
            let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
            (out.status.success() && !text.is_empty()).then(|| text.chars().take(200).collect())
        })
        .clone()
}

/// A tesztek száma a CLAUDE.md-ből („Összesen: N teszt”), vagy None.
pub fn test_count(app: &App) -> Option<i64> {
    let text = std::fs::read_to_string(app.base_dir.join("CLAUDE.md")).ok()?;
    static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"Összesen:\s*(\d+)\s*teszt").expect("érvényes regex"));
    RE.captures(&text)?.get(1)?.as_str().parse().ok()
}

const PACKAGES: [&str; 8] = ["axum", "socketioxide", "tokio", "rusqlite", "tower-http", "lettre", "serde_json", "ureq"];

/// A fontosabb függőségek verziója a `Cargo.lock`-ból.
fn package_versions(app: &App) -> Value {
    let text = std::fs::read_to_string(app.base_dir.join("Cargo.lock")).unwrap_or_default();
    let mut map = Map::new();
    for name in PACKAGES {
        let marker = format!("name = \"{name}\"\nversion = \"");
        let version = text.find(&marker).and_then(|at| text[at + marker.len()..].split('"').next()).map(|v| json!(v)).unwrap_or(Value::Null);
        map.insert(name.to_string(), version);
    }
    Value::Object(map)
}

pub fn versions(app: &App) -> Value {
    json!({
        "git": git_commit(app), "asset_version": crate::server::http::asset_version(app),
        "admin_asset_version": crate::server::http::admin_asset_version(app), "python": runtime_version(),
        "platform": std::env::consts::OS, "packages": package_versions(app), "tests": test_count(app),
    })
}

// ===== Konfiguráció (csak olvasható, a titkok maszkolva) =====

pub const MASK: &str = "••••";

/// A konfiguráció értékei és a releváns környezeti változók; a titkoknál csak a „beállítva / nincs” látszik.
pub fn config_view(app: &App) -> Vec<Value> {
    let plain = |key: &str, value: Value| {
        let is_set = !(value.is_null() || value == json!("") || value == json!([]));
        json!({"key": key, "value": value, "secret": false, "set": is_set})
    };
    let secret = |key: &str, is_set: bool| json!({"key": key, "value": if is_set { MASK } else { "" }, "secret": true, "set": is_set});
    let mail = app.mailer.current();
    let env_set = |key: &str| std::env::var(key).is_ok_and(|v| !v.is_empty());
    let mut admin_emails: Vec<&String> = app.config.admin_emails.iter().collect();
    admin_emails.sort();
    let mut allowlist: Vec<String> = app.config.admin_ip_allowlist.iter().map(|n| n.to_string()).collect();
    allowlist.sort();
    let limits: Map<String, Value> = crate::config::AUTH_RATE_LIMITS.iter().map(|(k, (a, b))| (k.to_string(), json!([a, b]))).collect();
    vec![
        plain("SMTP_HOST", json!(mail.host)), plain("SMTP_PORT", json!(mail.port)), plain("SMTP_SECURITY", json!(mail.security)),
        plain("SMTP_USER", json!(mail.username)), plain("SMTP_FROM", json!(mail.from_address)),
        secret("SMTP_PASSWORD", !mail.password.is_empty()),
        plain("SMTP_CONFIGURED", json!(mail.configured)), plain("SMTP_SOURCE", json!(mail.source)),
        secret("SECRET_KEY", env_set("SECRET_KEY")),
        secret("VAPID_PRIVATE_KEY", env_set("VAPID_PRIVATE_KEY")),
        plain("VAPID_SUBJECT", json!(app.push.subject())),
        plain("DB_PATH", json!(app.config.db_path)), plain("BACKUP_DIR", json!(app.config.backup_dir)),
        plain("PORT", json!(std::env::var("PORT").unwrap_or_else(|_| "5000".to_string()))),
        plain("SESSION_MAX_AGE_DAYS", json!(crate::config::SESSION_MAX_AGE_DAYS)),
        plain("VERIFICATION_CODE_EXPIRY_MINUTES", json!(crate::config::VERIFICATION_CODE_EXPIRY_MINUTES)),
        plain("VERIFICATION_MAX_ATTEMPTS", json!(crate::config::VERIFICATION_MAX_ATTEMPTS)),
        plain("EMAIL_VERIFIED_WINDOW_MINUTES", json!(crate::config::EMAIL_VERIFIED_WINDOW_MINUTES)),
        plain("WORD_REJECT_THRESHOLD", json!(app.config.word_reject_threshold)),
        plain("ADMIN_EMAILS", json!(admin_emails)),
        plain("ADMIN_SESSION_IDLE_MINUTES", json!(app.config.admin_session_idle_minutes)),
        plain("ADMIN_SUDO_MINUTES", json!(app.config.admin_sudo_minutes)),
        plain("ADMIN_IP_ALLOWLIST", json!(allowlist)),
        plain("AUTH_RATE_LIMITS", Value::Object(limits)),
    ]
}

// ===== Adatbázis =====

static TABLE_NAME_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$").expect("érvényes regex"));

pub fn table_counts(app: &App) -> AdminResult<Vec<Value>> {
    txn(&app.db, |tx| {
        let names: Vec<String> = fetch_all(tx, "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name", [])?
            .iter()
            .map(|r| r.text("name"))
            .collect();
        let mut out = Vec::new();
        for name in names.iter().filter(|n| TABLE_NAME_RE.is_match(n)) {
            let rows = fetch_one(tx, &format!("SELECT COUNT(*) AS n FROM \"{name}\""), [])?.map(|r| r.int("n")).unwrap_or(0);
            out.push(json!({"name": name, "rows": rows}));
        }
        Ok(out)
    })
}

pub fn backup_dir(app: &App) -> PathBuf {
    let path = Path::new(&app.config.backup_dir);
    if path.is_absolute() { path.to_path_buf() } else { app.base_dir.join(path) }
}

static BACKUP_NAME_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^scrabble-\d{8}-\d{6}\.db$").expect("érvényes regex"));

pub fn is_backup_name(name: &str) -> bool {
    BACKUP_NAME_RE.is_match(name)
}

/// A mentések: [{name, size, mtime}], a legújabbal kezdve.
pub fn list_backups(app: &App) -> Vec<Value> {
    let mut items: Vec<Value> = Vec::new();
    if let Ok(dir) = std::fs::read_dir(backup_dir(app)) {
        for entry in dir.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if BACKUP_NAME_RE.is_match(&name) {
                if let Ok(meta) = entry.metadata() {
                    let mtime = meta.modified().ok().and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs_f64()).unwrap_or(0.0);
                    items.push(json!({"name": name, "size": meta.len(), "mtime": mtime}));
                }
            }
        }
    }
    items.sort_by(|a, b| b["name"].as_str().cmp(&a["name"].as_str()));
    items
}

pub fn last_backup_time(app: &App) -> Option<f64> {
    list_backups(app).first().and_then(|b| b["mtime"].as_f64())
}

/// Konzisztens pillanatkép az SQLite backup API-val. Visszatér: a fájl útja.
pub fn make_backup(app: &App, directory: Option<&Path>) -> AdminResult<PathBuf> {
    let folder = directory.map(|d| d.to_path_buf()).unwrap_or_else(|| backup_dir(app));
    std::fs::create_dir_all(&folder).map_err(|e| AdminError::new(&format!("A mentési mappa nem hozható létre: {e}"), 500))?;
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let mut path = folder.join(format!("scrabble-{stamp}.db"));
    let mut counter = 1;
    while path.exists() {
        // ugyanabban a másodpercben készült második mentés
        counter += 1;
        path = folder.join(format!("scrabble-{stamp}-{counter}.db"));
    }
    let source = rusqlite::Connection::open(&app.db.path)?;
    let mut dest = rusqlite::Connection::open(&path)?;
    {
        let backup = rusqlite::backup::Backup::new(&source, &mut dest)?;
        backup.run_to_completion(64, std::time::Duration::from_millis(5), None)?;
    }
    Ok(path)
}

/// A legutóbbi `keep` mentés megtartása. Visszatér: a törölt fájlok száma.
pub fn prune_backups(app: &App, keep: usize) -> usize {
    let mut removed = 0;
    for item in list_backups(app).iter().skip(keep.max(1)) {
        if let Some(name) = item["name"].as_str() {
            if std::fs::remove_file(backup_dir(app).join(name)).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}

/// VACUUM és ANALYZE. Visszatér: (előtte, utána) bájt.
pub fn maintenance_vacuum(app: &App) -> AdminResult<(u64, u64)> {
    let before = db_file_size(app);
    {
        let conn = app.db.raw();
        conn.execute_batch("VACUUM")?;
        conn.execute_batch("ANALYZE")?;
        // a VACUUM a WAL-on át megy: a napló visszavágása nélkül „nagyobb” lenne az adatbázis a művelet után
        let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    }
    Ok((before, db_file_size(app)))
}

// ===== Takarítás =====

pub const ABANDONED_DEFAULT_DAYS: i64 = 30;
const CLEANUP_ITEMS: [&str; 7] = ["sessions", "codes", "verified_emails", "abandoned_games", "chat_log", "ip_bans", "login_events"];
const LOGIN_EVENT_KEEP_DAYS: i64 = 180;

fn cleanup_queries(app: &App, days: i64) -> Vec<(&'static str, &'static str, String)> {
    let now = util::utcnow();
    let stamp = util::format_ts(now);
    vec![
        ("sessions", "FROM sessions WHERE expires_at < ?", stamp.clone()),
        ("codes", "FROM verification_codes WHERE expires_at < ? OR used = 1", stamp.clone()),
        ("verified_emails", "FROM verified_emails WHERE expires_at < ?", stamp.clone()),
        ("abandoned_games", "FROM saved_games WHERE status = 'abandoned' AND updated_at < ?", util::format_ts(now - chrono::Duration::days(days))),
        ("chat_log", "FROM chat_log WHERE created_at < ?", util::format_ts(now - chrono::Duration::days(app.settings.get_i64("chat_log_days")))),
        ("ip_bans", "FROM ip_bans WHERE expires_at IS NOT NULL AND expires_at < ?", stamp),
        ("login_events", "FROM login_events WHERE created_at < ?", util::format_ts(now - chrono::Duration::days(LOGIN_EVENT_KEEP_DAYS))),
    ]
}

/// Mit törölne a takarítás: {tétel: darabszám}.
pub fn cleanup_preview(app: &App, days: Option<&Value>) -> AdminResult<Value> {
    let days = match days {
        None => ABANDONED_DEFAULT_DAYS,
        Some(d) => int_field(Some(d), "days", 1, 3650)?,
    };
    let queries = cleanup_queries(app, days);
    txn(&app.db, |tx| {
        let mut out = Map::new();
        for (name, sql, param) in &queries {
            let n = fetch_one(tx, &format!("SELECT COUNT(*) AS n {sql}"), [param])?.map(|r| r.int("n")).unwrap_or(0);
            out.insert(name.to_string(), json!(n));
        }
        Ok(Value::Object(out))
    })
}

/// A kijelölt tételek takarítása (naplózva, egy tranzakcióban). Visszatér: {tétel: törölt sorok}.
pub fn cleanup_run(app: &App, ctx: &AdminContext, items: Option<&Value>, days: Option<&Value>, reason: Option<&Value>) -> AdminResult<Value> {
    let days = match days {
        None => ABANDONED_DEFAULT_DAYS,
        Some(d) => int_field(Some(d), "days", 1, 3650)?,
    };
    let items: Vec<String> = match items.and_then(|i| i.as_array()) {
        Some(list) if !list.is_empty() && list.iter().all(|i| i.as_str().is_some_and(|s| CLEANUP_ITEMS.contains(&s))) => {
            list.iter().filter_map(|i| i.as_str().map(|s| s.to_string())).collect()
        }
        _ => return Err(AdminError::field("Érvénytelen kérés.", 400, "items")),
    };
    let queries = cleanup_queries(app, days);
    action(&app.db, ctx, "system.cleanup", Some("system"), None, reason, json!({"days": days}), true, |act| {
        let mut removed = Map::new();
        for name in &items {
            let (_, sql, param) = queries.iter().find(|(n, _, _)| n == name).expect("ismert tétel");
            let n = act.tx.execute(&format!("DELETE {sql}"), [param])?;
            removed.insert(name.clone(), json!(n));
        }
        act.details.insert("removed".into(), Value::Object(removed.clone()));
        Ok(Value::Object(removed))
    })
}

/// Napi feladatok: a napi mentés (ha be van kapcsolva) és a régi chat napló törlése. Visszatér: a végzett feladatok.
pub fn run_scheduled_maintenance(app: &App) -> Vec<&'static str> {
    let now = util::now();
    let mut done = Vec::new();
    if app.settings.get_bool("backup_daily") {
        let last = last_backup_time(app);
        if last.is_none_or(|l| now - l >= 23.0 * 3600.0) && make_backup(app, None).is_ok() {
            prune_backups(app, app.settings.get_i64("backup_keep").max(1) as usize);
            done.push("backup");
        }
    }
    if app.settings.get_bool("chat_log_enabled") {
        let cutoff = util::format_ts(util::utcnow() - chrono::Duration::days(app.settings.get_i64("chat_log_days")));
        let removed = app.db.with(|tx| tx.execute("DELETE FROM chat_log WHERE created_at < ?", [cutoff]));
        if removed.is_ok_and(|n| n > 0) {
            done.push("chat_log");
        }
    }
    done
}
