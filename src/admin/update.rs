//! Admin panel: frissítés GitHubról (a legfrissebb vagy egy megadott ág), és a szerver újraindítása.
//!
//! Szándékosan szűk, biztonságos művelet-készlet (a kód lecserélése a legkényesebb admin művelet):
//!
//! * csak a rögzített `origin` távoli tárral dolgozik (az admin nem adhat meg tetszőleges URL-t);
//! * az ág neve szigorúan ellenőrzött, és szerepelnie kell a GitHubról éppen lekért ágak között;
//! * csak előretekerés (fast-forward) történik, tiszta munkafán (a helyi módosítást nem írja felül);
//! * ha a Rust forrás (vagy a `Cargo.*`) változott, a kért újrafordítás (`cargo build --release`) a frissítés
//!   része; a statikus fájlok és a sablonok a lemezről élnek, azok azonnal érvényesek;
//! * a git kimenetében szereplő hozzáférési adatok (`https://felhasznalo:token@...`) maszkolva vannak.
//!
//! A futó folyamat a régi kódot használja, amíg újra nem indul: ezt a `restart_needed` jelzi (a folyamat induláskor
//! rögzíti a commitját). Az újraindítás a folyamat önmagára cserélése (`exec`), a tunnel előtte leáll; ha a
//! `target/release/scrabble` újabb a futó programnál, az új program indul.

use super::*;
use crate::app::App;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use regex::Regex;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const REMOTE: &str = "origin";
const FETCH_TIMEOUT: u64 = 90;
const GIT_TIMEOUT: u64 = 30;
const BUILD_TIMEOUT: u64 = 3600;
const MAX_INCOMING: usize = 30;
const MAX_FILES: usize = 60;
const MAX_DIRTY: usize = 20;
const RESTART_DELAY: f64 = 1.5;
const SEP: char = '\x1f';

static BRANCH_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._/-]{0,99}$").expect("érvényes regex"));
static CREDENTIALS_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(://)[^/@\s]+@").expect("érvényes regex"));

static UPDATE_LOCK: Mutex<()> = Mutex::new(());
static RESTART_SCHEDULED: Mutex<Option<f64>> = Mutex::new(None);
/// Tesztekhez: hányszor kellett volna a folyamatot lecserélni (`SCRABBLE_TEST_NO_EXEC` esetén nem történik meg).
#[doc(hidden)]
pub static SKIPPED_RESTARTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Tesztekhez: a frissítési zár megfogása (egy másik frissítés futását utánozza).
#[doc(hidden)]
pub fn hold_update_lock() -> parking_lot::MutexGuard<'static, ()> {
    UPDATE_LOCK.lock()
}

/// Egy git parancs nem sikerült (a szöveg már maszkolt).
#[derive(Debug)]
pub struct GitError(pub String);

/// Hozzáférési adatok maszkolása és hossz-korlát a git kimenetében.
fn clean(text: &str) -> String {
    CREDENTIALS_RE.replace_all(text.trim(), "${1}***@").chars().take(300).collect()
}

/// Egy külső parancs futtatása időkorláttal. Visszatér: (sikeres-e, stdout, stderr).
fn run(mut command: Command, timeout: u64) -> Result<(bool, String, String), String> {
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { "a program nem található".to_string() } else { e.to_string() })?;
    let (mut out_pipe, mut err_pipe) = (child.stdout.take(), child.stderr.take());
    let out_thread = std::thread::spawn(move || {
        let mut text = Vec::new();
        if let Some(p) = out_pipe.as_mut() {
            let _ = p.read_to_end(&mut text);
        }
        text
    });
    let err_thread = std::thread::spawn(move || {
        let mut text = Vec::new();
        if let Some(p) = err_pipe.as_mut() {
            let _ = p.read_to_end(&mut text);
        }
        text
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if started.elapsed() > Duration::from_secs(timeout) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("időtúllépés".to_string());
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(e.to_string()),
        }
    };
    let out = String::from_utf8_lossy(&out_thread.join().unwrap_or_default()).to_string();
    let err = String::from_utf8_lossy(&err_thread.join().unwrap_or_default()).to_string();
    Ok((status.success(), out, err))
}

/// git parancs a program mappájában. Visszatér: a kimenet (levágva, `keep`: érintetlenül); hiba esetén GitError.
fn git(app: &App, args: &[&str], timeout: u64, keep: bool) -> Result<String, GitError> {
    let mut command = Command::new("git");
    command.args(args).current_dir(&app.base_dir).env("GIT_TERMINAL_PROMPT", "0").env("GIT_ASKPASS", "echo").env("LC_ALL", "C");
    let (ok, out, err) = run(command, timeout).map_err(|e| GitError(if e == "a program nem található" { "git nem található".to_string() } else { e }))?;
    if !ok {
        let message = clean(if err.trim().is_empty() { &out } else { &err });
        return Err(GitError(if message.is_empty() { "a git hibát jelzett".to_string() } else { message }));
    }
    Ok(if keep { out } else { out.trim().to_string() })
}

fn try_git(app: &App, args: &[&str]) -> Option<String> {
    git(app, args, GIT_TIMEOUT, false).ok()
}

fn require_repo(app: &App) -> AdminResult<()> {
    if try_git(app, &["rev-parse", "--is-inside-work-tree"]).as_deref() != Some("true") {
        return Err(AdminError::new("A program nem git tárból fut: a frissítés nem érhető el.", 409));
    }
    Ok(())
}

fn split_fields(raw: &str, count: usize) -> Vec<String> {
    let mut parts: Vec<String> = raw.split(SEP).map(|s| s.to_string()).collect();
    parts.resize(count, String::new());
    parts
}

fn commit_info(app: &App, reference: &str) -> Value {
    let format = format!("--format=%H{SEP}%h{SEP}%s{SEP}%cI{SEP}%an");
    let Some(raw) = try_git(app, &["log", "-1", &format, reference]).filter(|r| !r.is_empty()) else { return Value::Null };
    let p = split_fields(&raw, 5);
    json!({"hash": p[0], "short": p[1], "subject": p[2].chars().take(200).collect::<String>(), "date": p[3], "author": p[4].chars().take(80).collect::<String>()})
}

fn head(app: &App) -> Option<String> {
    try_git(app, &["rev-parse", "HEAD"])
}

/// A folyamat induláskori commitjának rögzítése: ehhez képest látszik, hogy a lemezen újabb kód van-e.
pub fn remember_running_commit(app: &App) {
    *app.running_commit.lock() = head(app);
}

/// A távoli tár címe hozzáférési adatok nélkül (vagy None).
pub fn remote_url(app: &App) -> Option<String> {
    try_git(app, &["config", "--get", &format!("remote.{REMOTE}.url")]).map(|u| clean(&u))
}

/// A követett, módosított fájlok (a nem követettek — pl. az adatbázis — nem számítanak).
fn dirty_files(app: &App) -> Vec<String> {
    // A sor elején álló szóköz jelentés (" M fájl"), ezért a kimenet nem vágható le
    let out = git(app, &["status", "--porcelain", "--untracked-files=no"], GIT_TIMEOUT, true).unwrap_or_default();
    out.lines().filter(|l| !l.trim().is_empty()).map(|l| l.chars().skip(3).collect::<String>().trim().to_string()).collect()
}

fn current_branch(app: &App) -> Option<String> {
    try_git(app, &["symbolic-ref", "--short", "-q", "HEAD"]) // None: leválasztott HEAD
}

/// A tár állapota hálózat nélkül: ág, commit, távoli cím, módosítások, újraindítás szükséges-e.
pub fn status(app: &App) -> Value {
    if try_git(app, &["rev-parse", "--is-inside-work-tree"]).as_deref() != Some("true") {
        return json!({"repo": false});
    }
    let head = head(app);
    let dirty = dirty_files(app);
    let running = app.running_commit.lock().clone();
    json!({
        "repo": true, "branch": current_branch(app), "commit": commit_info(app, "HEAD"), "remote": remote_url(app),
        "dirty": dirty.iter().take(MAX_DIRTY).collect::<Vec<_>>(), "dirty_count": dirty.len(),
        "running_commit": running.as_ref().map(|r| r.chars().take(7).collect::<String>()),
        "restart_needed": running.is_some() && head.is_some() && running != head,
        "restart_scheduled": RESTART_SCHEDULED.lock().is_some(),
    })
}

/// A GitHubról lekért ágak: [{name, hash, short, subject, date}], a legfrissebb elöl.
fn remote_branches(app: &App) -> Vec<Value> {
    let format = format!("--format=%(refname:short){SEP}%(objectname){SEP}%(contents:subject){SEP}%(committerdate:iso-strict)");
    let out = try_git(app, &["for-each-ref", "--sort=-committerdate", &format, &format!("refs/remotes/{REMOTE}")]).unwrap_or_default();
    let mut branches = Vec::new();
    for line in out.lines() {
        let p = split_fields(line, 4);
        let name = p[0].strip_prefix(&format!("{REMOTE}/")).unwrap_or("").to_string();
        if !name.is_empty() && name != "HEAD" {
            branches.push(json!({"name": name, "hash": p[1], "short": p[1].chars().take(7).collect::<String>(), "subject": p[2].chars().take(200).collect::<String>(), "date": p[3]}));
        }
    }
    branches
}

fn default_branch(app: &App, branches: &[Value]) -> Option<String> {
    if let Some(head) = try_git(app, &["symbolic-ref", "--short", "-q", &format!("refs/remotes/{REMOTE}/HEAD")]) {
        if let Some(name) = head.strip_prefix(&format!("{REMOTE}/")) {
            return Some(name.to_string());
        }
    }
    let names: Vec<&str> = branches.iter().filter_map(|b| b["name"].as_str()).collect();
    ["main", "master"].iter().find(|n| names.contains(n)).map(|n| n.to_string()).or_else(|| names.first().map(|n| n.to_string()))
}

/// Forrásfájl-e, amelynek változása újrafordítást igényel.
fn is_build_input(path: &str) -> bool {
    path == "Cargo.toml" || path == "Cargo.lock" || path == "build.rs" || (path.starts_with("src/") && path.ends_with(".rs"))
}

/// Mi változna, ha a `branch`-re frissítenénk: hány új / helyi-only commit, bejövő commitok, érintett fájlok.
fn plan(app: &App, branch: &str) -> Value {
    let target = format!("{REMOTE}/{branch}");
    let count = |range: String| try_git(app, &["rev-list", "--count", &range]).and_then(|c| c.parse::<i64>().ok()).unwrap_or(0);
    let behind = count(format!("HEAD..{target}"));
    let ahead = count(format!("{target}..HEAD"));
    let incoming: Vec<Value> = try_git(app, &["log", &format!("--format=%h{SEP}%s{SEP}%cI{SEP}%an"), &format!("--max-count={MAX_INCOMING}"), &format!("HEAD..{target}")])
        .unwrap_or_default()
        .lines()
        .map(|line| {
            let p = split_fields(line, 4);
            json!({"short": p[0], "subject": p[1].chars().take(200).collect::<String>(), "date": p[2], "author": p[3].chars().take(80).collect::<String>()})
        })
        .collect();
    let files: Vec<(String, String)> = try_git(app, &["diff", "--name-status", "HEAD", &target])
        .unwrap_or_default()
        .lines()
        .map(|line| {
            let (code, path) = line.split_once('\t').unwrap_or((line, ""));
            (code.chars().take(1).collect(), path.chars().take(200).collect())
        })
        .collect();
    let current = current_branch(app);
    json!({
        "branch": branch, "behind": behind, "ahead": ahead, "incoming": incoming,
        "files": files.iter().take(MAX_FILES).map(|(s, p)| json!({"status": s, "path": p})).collect::<Vec<_>>(),
        "files_total": files.len(),
        // a „függőségek telepítése” helyett a Rust változatban az újrafordítás (a kliens ugyanezt a jelzőt használja)
        "requirements_changed": files.iter().any(|(_, p)| is_build_input(p)),
        "switch": current.as_deref() != Some(branch), "up_to_date": behind == 0 && ahead == 0 && current.as_deref() == Some(branch),
    })
}

/// GitHubról lekéri az ágakat (`git fetch`), és megmondja, mi változna a kiválasztott ágra váltva / frissítve. Az ág
/// megadása nélkül a jelenlegi ág (ha megvan a GitHubon), különben az alapértelmezett ág az alapértelmezés. `fetch`
/// hamis: nem megy a hálózatra (egy másik ág terve a már lekért adatokból).
pub fn check(app: &App, branch: Option<&str>, fetch: bool) -> AdminResult<Value> {
    require_repo(app)?;
    if fetch {
        let _guard = UPDATE_LOCK.lock();
        if let Err(e) = git(app, &["fetch", "--prune", REMOTE], FETCH_TIMEOUT, false) {
            return Err(AdminError::new("A GitHubról való lekérés nem sikerült.", 502).with("detail", json!(e.0)));
        }
    }
    let branches = remote_branches(app);
    if branches.is_empty() {
        return Err(AdminError::new("A GitHubon nincs elérhető ág.", 409));
    }
    let names: Vec<String> = branches.iter().filter_map(|b| b["name"].as_str().map(|s| s.to_string())).collect();
    let current = current_branch(app);
    let default = default_branch(app, &branches);
    let branch = match branch {
        None => match current.as_ref().filter(|c| names.contains(c)) {
            Some(c) => c.clone(),
            None => default.clone().unwrap_or_default(),
        },
        Some(b) if names.iter().any(|n| n == b) => b.to_string(),
        Some(_) => return Err(AdminError::field("Ez az ág nincs a GitHubon.", 400, "branch")),
    };
    let mut result = status(app);
    result["branches"] = json!(branches
        .into_iter()
        .map(|mut b| {
            let is_current = current.as_deref() == b["name"].as_str();
            b["current"] = json!(is_current);
            b
        })
        .collect::<Vec<_>>());
    result["default_branch"] = json!(default);
    result["plan"] = plan(app, &branch);
    Ok(result)
}

fn valid_branch(branch: Option<&Value>) -> AdminResult<String> {
    let invalid = || AdminError::field("Érvénytelen ág.", 400, "branch");
    let branch = branch.and_then(|b| b.as_str()).ok_or_else(invalid)?;
    if !BRANCH_RE.is_match(branch) || branch.contains("..") || branch.contains("//") || branch.ends_with('/') || branch.ends_with(".lock") || branch.ends_with('.') {
        return Err(invalid());
    }
    Ok(branch.to_string())
}

/// Visszaállás az előző állapotra (ág + commit). Visszatér: sikerült-e.
fn restore(app: &App, old_head: &str, old_branch: Option<&str>) -> bool {
    let result = (|| -> Result<(), GitError> {
        match old_branch {
            Some(branch) => git(app, &["checkout", branch, "--"], GIT_TIMEOUT, false)?,
            None => git(app, &["checkout", "--detach", old_head], GIT_TIMEOUT, false)?,
        };
        git(app, &["reset", "--hard", old_head], GIT_TIMEOUT, false)?;
        Ok(())
    })();
    result.is_ok()
}

/// A `cargo build --release` futtatása. Hiba esetén a fordító kimenetének utolsó sorai (maszkolva).
fn cargo_build(app: &App) -> Result<(), String> {
    let mut command = Command::new("cargo");
    command.args(["build", "--release"]).current_dir(&app.base_dir);
    match run(command, BUILD_TIMEOUT) {
        Ok((true, _, _)) => Ok(()),
        Ok((false, out, err)) => {
            let text = if err.trim().is_empty() { out } else { err };
            let tail: Vec<&str> = text.lines().rev().take(8).collect();
            Err(clean(&tail.into_iter().rev().collect::<Vec<_>>().join("\n")))
        }
        Err(e) => Err(e),
    }
}

fn record_system(app: &App, ctx: &AdminContext, name: &str, details: Value) -> AdminResult<()> {
    app.db.with(|tx| record(tx, ctx, name, Some("system"), Some("git"), &details).map(|_| ()))?;
    Ok(())
}

/// Frissítés a GitHub `branch` ágára (fast-forward). A naplósor a művelet ELŐTT íródik (indoklás kötelező). Hibánál
/// (eltérő előzmény) a tár az előző állapotban marad.
pub fn apply(app: &Arc<App>, ctx: &AdminContext, branch: Option<&Value>, reason: Option<&Value>, install: bool, restart: bool) -> AdminResult<Value> {
    require_repo(app)?;
    let branch = valid_branch(branch)?;
    normalize_reason(reason)?; // indoklás nélkül a hálózati lekérés sem indul el
    let Some(_guard) = UPDATE_LOCK.try_lock() else {
        return Err(AdminError::new("Már fut egy frissítés.", 409));
    };
    if let Err(e) = git(app, &["fetch", "--prune", REMOTE], FETCH_TIMEOUT, false) {
        return Err(AdminError::new("A GitHubról való lekérés nem sikerült.", 502).with("detail", json!(e.0)));
    }
    if !remote_branches(app).iter().any(|b| b["name"].as_str() == Some(branch.as_str())) {
        return Err(AdminError::field("Ez az ág nincs a GitHubon.", 400, "branch"));
    }
    let dirty = dirty_files(app);
    if !dirty.is_empty() {
        return Err(AdminError::new("A programmappában helyi módosítások vannak: a frissítés nem írja felül őket.", 409).with("files", json!(dirty.iter().take(MAX_DIRTY).collect::<Vec<_>>())));
    }
    let old_head = head(app).unwrap_or_default();
    let old_branch = current_branch(app);
    let plan = plan(app, &branch);
    if plan["up_to_date"] == json!(true) {
        return Err(AdminError::new("Már a legfrissebb állapoton vagy.", 409));
    }

    action(
        &app.db,
        ctx,
        "system.update",
        Some("system"),
        Some("git".to_string()),
        reason,
        json!({"branch": branch, "from_branch": old_branch, "from": old_head, "incoming": plan["behind"], "install": install, "restart": restart}),
        true,
        |_| Ok(()), // a naplósor előbb íródik, mint a kód cseréje
    )?;

    let target = format!("{REMOTE}/{branch}");
    let switched = (|| -> Result<(), GitError> {
        if plan["switch"] == json!(true) {
            if try_git(app, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")]).is_some() {
                git(app, &["checkout", &branch, "--"], GIT_TIMEOUT, false)?;
            } else {
                git(app, &["checkout", "-b", &branch, "--track", &target], GIT_TIMEOUT, false)?;
            }
        }
        git(app, &["merge", "--ff-only", &target], GIT_TIMEOUT, false)?;
        Ok(())
    })();
    if let Err(e) = switched {
        restore(app, &old_head, old_branch.as_deref());
        return Err(AdminError::new("A frissítés nem végezhető el: a helyi ág eltér a GitHubon lévőtől.", 409).with("detail", json!(e.0)));
    }

    let new_head = head(app).unwrap_or_default();
    let changed: Vec<String> = try_git(app, &["diff", "--name-only", &old_head, &new_head]).unwrap_or_default().lines().map(|l| l.to_string()).collect();
    let installed = if install && changed.iter().any(|p| is_build_input(p)) {
        if let Err(detail) = cargo_build(app) {
            // az új kód nem fordul le: vissza az előző állapotra (a futó program és a lemez egyezzen)
            let restored = restore(app, &old_head, old_branch.as_deref());
            record_system(app, ctx, "system.update_failed", json!({"branch": branch, "from": old_head, "to": new_head, "restored": restored, "reason": "build"}))?;
            return Err(AdminError::new("Az új kód nem fordítható le, a frissítés visszaállt az előző állapotra.", 422).with("detail", json!(detail)));
        }
        Some(true)
    } else {
        None
    };
    record_system(app, ctx, "system.update_done", json!({"branch": branch, "from": old_head, "to": new_head, "files": changed.len(), "installed": installed}))?;
    drop(_guard);
    let scheduled = if restart { schedule_restart(app) } else { false };
    let mut result = status(app);
    result["from"] = json!(old_head.chars().take(7).collect::<String>());
    result["changed_files"] = json!(changed.len());
    result["installed"] = json!(installed);
    result["restarting"] = json!(scheduled);
    Ok(result)
}

// ===== Újraindítás =====

/// Az indítandó program: a friss `target/release/scrabble`, ha újabb a futónál, különben a futó program.
fn restart_executable(app: &App) -> Option<std::path::PathBuf> {
    let current = std::env::current_exe().ok()?;
    let built = app.base_dir.join("target/release/scrabble");
    let newer = |a: &std::path::Path, b: &std::path::Path| match (std::fs::metadata(a).and_then(|m| m.modified()), std::fs::metadata(b).and_then(|m| m.modified())) {
        (Ok(x), Ok(y)) => x > y,
        _ => false,
    };
    if built.is_file() && newer(&built, &current) { Some(built) } else { Some(current) }
}

/// A folyamat cseréje önmagára (azonos parancssorral). Az `exec` nem tér vissza; a tesztek ezt kikapcsolhatják.
fn restart_process(app: &App) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        if std::env::var_os("SCRABBLE_TEST_NO_EXEC").is_some() {
            SKIPPED_RESTARTS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return;
        }
        app.tunnel.stop();
        super::system::restore_output();
        if let Some(exe) = restart_executable(app) {
            let args: Vec<String> = std::env::args().skip(1).collect();
            let error = Command::new(exe).args(args).exec();
            eprintln!("Az újraindítás nem sikerült: {error}");
        }
    }
}

/// Az újraindítás ütemezése röviddel később, hogy a HTTP válasz még kimehessen. Visszatér: ütemezve lett-e.
pub fn schedule_restart(app: &Arc<App>) -> bool {
    {
        let mut scheduled = RESTART_SCHEDULED.lock();
        if scheduled.is_some() {
            return false;
        }
        *scheduled = Some(util::now());
    }
    let app = app.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs_f64(RESTART_DELAY)).await;
        let a = app.clone();
        let _ = tokio::task::spawn_blocking(move || restart_process(&a)).await;
        *RESTART_SCHEDULED.lock() = None; // csak akkor érünk ide, ha az újraindítás nem sikerült
    });
    true
}

/// Újraindítás (kötelező indoklással). A naplósor az újraindítás ELŐTT íródik.
pub fn restart(app: &Arc<App>, ctx: &AdminContext, reason: Option<&Value>) -> AdminResult<Value> {
    if RESTART_SCHEDULED.lock().is_some() {
        return Err(AdminError::new("Az újraindítás már folyamatban van.", 409));
    }
    let running = app.running_commit.lock().clone();
    action(&app.db, ctx, "system.restart", Some("system"), Some("server".to_string()), reason, json!({"commit": running, "head": head(app)}), true, |_| Ok(()))?;
    schedule_restart(app);
    Ok(json!({"restarting": true}))
}
