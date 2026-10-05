//! Admin panel: frissítés GitHubról (egy ág előretekerése) és az újraindítás (a Python `test_admin_update.py`
//! megfelelője).
//!
//! Valódi git-tárakkal dolgozik: egy helyi „GitHub” (bare `origin`), a program mappája (klón) és egy harmadik klón,
//! ahonnan új commitokat „pusholunk”. Hálózat nincs. Az újrafordítást egy apró, valódi cargo-projekt teszi próbára;
//! az újraindítás (`exec`) ki van kapcsolva (`SCRABBLE_TEST_NO_EXEC`), csak számoljuk.
//!
//! A frissítési zár és az újraindítás-jelző folyamat-szintű, ezért a tesztek egymás után futnak (`serial`).

mod common;
use common::admin::*;
use common::*;
use scrabble::admin::update::{self, SKIPPED_RESTARTS};
use scrabble::admin::{AdminContext, AdminError};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::Ordering;

const REASON: &str = "Frissítés a tesztből";

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn serial() -> tokio::sync::MutexGuard<'static, ()> {
    let guard = SERIAL.lock().await;
    static ENV: std::sync::Once = std::sync::Once::new();
    // a tesztek zárja alatt, még mielőtt bármelyik teszt git-et vagy újraindítást indítana
    ENV.call_once(|| unsafe { std::env::set_var("SCRABBLE_TEST_NO_EXEC", "1") });
    guard
}

fn git(dir: &Path, args: &[&str]) -> String {
    let done = Command::new("git").args(args).current_dir(dir).output().expect("git");
    assert!(done.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&done.stderr));
    String::from_utf8_lossy(&done.stdout).trim().to_string()
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

struct Repos {
    origin: PathBuf,
    seed: PathBuf,
    app: PathBuf,
    server: TestServer,
}

impl Repos {
    /// Új commit a „GitHubra” (a seed klónból). Visszatér: a commit azonosítója.
    fn push(&self, filename: &str, text: &str, message: &str, branch: &str) -> String {
        if git(&self.seed, &["rev-parse", "--abbrev-ref", "HEAD"]) != branch {
            let exists = Command::new("git").args(["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")]).current_dir(&self.seed).status().unwrap().success();
            git(&self.seed, &if exists { vec!["checkout", branch] } else { vec!["checkout", "-b", branch] });
        }
        write(&self.seed.join(filename), text);
        git(&self.seed, &["add", "-A"]);
        git(&self.seed, &["commit", "-m", message]);
        git(&self.seed, &["push", "origin", branch]);
        git(&self.seed, &["rev-parse", "HEAD"])
    }

    fn head(&self) -> String {
        git(&self.app, &["rev-parse", "HEAD"])
    }

    fn branch(&self) -> String {
        git(&self.app, &["rev-parse", "--abbrev-ref", "HEAD"])
    }

    fn ctx(&self) -> AdminContext {
        AdminContext { admin_user_id: 1, ip: "127.0.0.1".into(), user_agent: "teszt".into() }
    }

    async fn status(&self) -> Value {
        let app = self.server.app.clone();
        tokio::task::spawn_blocking(move || update::status(&app)).await.unwrap()
    }

    async fn check(&self, branch: Option<&str>, fetch: bool) -> Result<Value, AdminError> {
        let app = self.server.app.clone();
        let branch = branch.map(|b| b.to_string());
        tokio::task::spawn_blocking(move || update::check(&app, branch.as_deref(), fetch)).await.unwrap()
    }

    async fn apply_raw(&self, branch: Option<Value>, reason: Option<Value>, install: bool, restart: bool) -> Result<Value, AdminError> {
        let app = self.server.app.clone();
        let ctx = self.ctx();
        tokio::task::spawn_blocking(move || update::apply(&app, &ctx, branch.as_ref(), reason.as_ref(), install, restart)).await.unwrap()
    }

    async fn apply(&self, branch: &str, install: bool) -> Result<Value, AdminError> {
        self.apply_raw(Some(json!(branch)), Some(json!(REASON)), install, false).await
    }

    fn system_audit(&self) -> Vec<(String, Value)> {
        audit_rows(&self.server)
            .into_iter()
            .rev()
            .filter(|r| r["action"].as_str().unwrap_or("").starts_with("system."))
            .map(|r| (r["action"].as_str().unwrap().to_string(), r["details"].clone()))
            .collect()
    }
}

/// origin (bare) · seed (innen megy fel új tartalom) · app (a program mappája, a szerver `base_dir`-je).
async fn repos() -> Repos {
    let root = temp_dir("update");
    let (origin, seed, app) = (root.join("origin.git"), root.join("seed"), root.join("app"));
    git(&root, &["init", "--bare", origin.to_str().unwrap()]);
    git(&origin, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    git(&root, &["init", seed.to_str().unwrap()]);
    git(&seed, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    git(&seed, &["config", "user.name", "Teszt"]);
    git(&seed, &["config", "user.email", "teszt@example.com"]);
    write(&seed.join("VERSION"), "1\n");
    write(&seed.join("Cargo.toml"), "[package]\nname = \"minta\"\nversion = \"0.1.0\"\nedition = \"2021\"\n");
    write(&seed.join("src/main.rs"), "fn main() {}\n");
    git(&seed, &["add", "-A"]);
    git(&seed, &["commit", "-m", "Első verzió"]);
    git(&seed, &["remote", "add", "origin", origin.to_str().unwrap()]);
    git(&seed, &["push", "origin", "main"]);
    git(&root, &["clone", origin.to_str().unwrap(), app.to_str().unwrap()]);
    git(&app, &["config", "user.name", "Teszt"]);
    git(&app, &["config", "user.email", "teszt@example.com"]);
    let server = TestServer::start_in(Some(app.clone()), |_| {}).await;
    update::remember_running_commit(&server.app);
    Repos { origin, seed, app, server }
}

fn audit_names(repos: &Repos) -> Vec<String> {
    repos.system_audit().into_iter().map(|(a, _)| a).collect()
}

/// Megvárja, hogy a függő újraindítás lezáruljon (a következő teszt ne ütközzön vele).
async fn wait_restart_done(repos: &Repos) {
    for _ in 0..200 {
        if repos.status().await["restart_scheduled"] == json!(false) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("az újraindítás nem zárult le");
}

// ===================================================================================================
// Állapot
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn reports_branch_commit_and_remote() {
    let _serial = serial().await;
    let r = repos().await;
    let status = r.status().await;
    assert_eq!((status["repo"].clone(), status["branch"].clone()), (json!(true), json!("main")));
    assert_eq!(status["commit"]["subject"], "Első verzió");
    assert!(status["commit"]["short"].as_str().unwrap().len() >= 7);
    assert_eq!(status["remote"], json!(r.origin.to_str().unwrap()));
    assert_eq!((status["dirty"].clone(), status["restart_needed"].clone()), (json!([]), json!(false)));
}

#[tokio::test(flavor = "multi_thread")]
async fn not_a_git_repository() {
    let _serial = serial().await;
    let dir = temp_dir("nogit");
    let server = TestServer::start_in(Some(dir), |_| {}).await;
    let app = server.app.clone();
    assert_eq!(update::status(&app), json!({"repo": false}));
    let error = update::check(&app, None, true).unwrap_err();
    assert_eq!(error.status, 409);
}

#[tokio::test(flavor = "multi_thread")]
async fn local_changes_are_listed_but_untracked_files_are_not() {
    let _serial = serial().await;
    let r = repos().await;
    write(&r.app.join("VERSION"), "99\n");
    write(&r.app.join("scrabble.db"), "x");
    let status = r.status().await;
    assert_eq!((status["dirty"].clone(), status["dirty_count"].clone()), (json!(["VERSION"]), json!(1)));
}

#[tokio::test(flavor = "multi_thread")]
async fn credentials_in_the_remote_url_are_masked() {
    let _serial = serial().await;
    let r = repos().await;
    git(&r.app, &["remote", "set-url", "origin", "https://huba:ghp_secrettoken@github.com/pirelike/scrabble.git"]);
    let remote = r.status().await["remote"].as_str().unwrap().to_string();
    assert!(!remote.contains("ghp_secrettoken") && !remote.contains("huba"), "{remote}");
    assert_eq!(remote, "https://***@github.com/pirelike/scrabble.git");
}

#[tokio::test(flavor = "multi_thread")]
async fn restart_is_needed_when_the_disk_is_newer_than_the_process() {
    let _serial = serial().await;
    let r = repos().await;
    *r.server.app.running_commit.lock() = Some("a".repeat(40));
    let status = r.status().await;
    assert_eq!((status["restart_needed"].clone(), status["running_commit"].clone()), (json!(true), json!("aaaaaaa")));
}

// ===================================================================================================
// Ellenőrzés (git fetch)
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn lists_the_branches_and_what_would_change() {
    let _serial = serial().await;
    let r = repos().await;
    r.push("VERSION", "2\n", "Második verzió", "main");
    r.push("feature.txt", "X\n", "Kísérlet", "feature");
    let before = r.head();
    let result = r.check(None, true).await.unwrap();
    let mut names: Vec<String> = result["branches"].as_array().unwrap().iter().map(|b| b["name"].as_str().unwrap().to_string()).collect();
    names.sort();
    assert_eq!(names, vec!["feature", "main"]);
    assert_eq!(result["default_branch"], "main");
    let plan = &result["plan"];
    assert_eq!((plan["branch"].clone(), plan["behind"].clone(), plan["ahead"].clone(), plan["up_to_date"].clone()), (json!("main"), json!(1), json!(0), json!(false)));
    assert_eq!(plan["incoming"].as_array().unwrap().iter().map(|c| c["subject"].clone()).collect::<Vec<_>>(), vec![json!("Második verzió")]);
    assert_eq!(plan["files"], json!([{"status": "M", "path": "VERSION"}]));
    assert_eq!(plan["requirements_changed"], false);
    assert_eq!(r.head(), before); // a munkafa nem változott
}

#[tokio::test(flavor = "multi_thread")]
async fn without_fetch_it_works_from_the_already_downloaded_data() {
    let _serial = serial().await;
    let r = repos().await;
    r.push("feature.txt", "X\n", "Kísérlet", "feature");
    r.check(None, true).await.unwrap();
    git(&r.app, &["remote", "set-url", "origin", &format!("{}-missing", r.origin.to_str().unwrap())]); // a hálózat nem elérhető
    assert_eq!(r.check(Some("feature"), false).await.unwrap()["plan"]["branch"], "feature");
    assert!(r.check(Some("feature"), true).await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn another_branch_is_a_switch() {
    let _serial = serial().await;
    let r = repos().await;
    r.push("feature.txt", "X\n", "Kísérlet", "feature");
    let plan = r.check(Some("feature"), true).await.unwrap()["plan"].clone();
    assert_eq!((plan["switch"].clone(), plan["branch"].clone(), plan["files"][0]["path"].clone()), (json!(true), json!("feature"), json!("feature.txt")));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_branch_is_refused() {
    let _serial = serial().await;
    let r = repos().await;
    let error = r.check(Some("nincs-ilyen"), true).await.unwrap_err();
    assert_eq!((error.status, error.extra["field"].clone()), (400, json!("branch")));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_build_input_change_is_flagged() {
    let _serial = serial().await;
    let r = repos().await;
    r.push("src/main.rs", "fn main() { println!(\"ú\"); }\n", "Új kód", "main");
    assert_eq!(r.check(None, true).await.unwrap()["plan"]["requirements_changed"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unreachable_remote_is_a_bad_gateway() {
    let _serial = serial().await;
    let r = repos().await;
    git(&r.app, &["remote", "set-url", "origin", &format!("{}-missing", r.origin.to_str().unwrap())]);
    let error = r.check(None, true).await.unwrap_err();
    assert_eq!(error.status, 502);
    assert!(error.extra["detail"].as_str().is_some_and(|d| !d.is_empty()));
}

// ===================================================================================================
// Frissítés
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn fast_forwards_the_current_branch() {
    let _serial = serial().await;
    let r = repos().await;
    let new = r.push("VERSION", "2\n", "Második verzió", "main");
    let result = r.apply("main", false).await.unwrap();
    assert_eq!(r.head(), new);
    assert_eq!(read(&r.app.join("VERSION")), "2\n");
    assert_eq!((result["changed_files"].clone(), result["restart_needed"].clone(), result["restarting"].clone()), (json!(1), json!(true), json!(false)));
    let audit = r.system_audit();
    assert_eq!(audit.iter().map(|(a, _)| a.as_str()).collect::<Vec<_>>(), vec!["system.update", "system.update_done"]);
    assert_eq!((audit[0].1["reason"].clone(), audit[0].1["incoming"].clone()), (json!(REASON), json!(1)));
    assert_eq!(audit[1].1["to"], json!(new));
    assert!(audit[1].1["from"].as_str().unwrap().starts_with(result["from"].as_str().unwrap()));
}

#[tokio::test(flavor = "multi_thread")]
async fn switches_to_a_specific_branch() {
    let _serial = serial().await;
    let r = repos().await;
    let new = r.push("feature.txt", "X\n", "Kísérlet", "feature");
    r.apply("feature", false).await.unwrap();
    assert_eq!(r.branch(), "feature");
    assert_eq!(r.head(), new);
    assert!(r.app.join("feature.txt").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn switching_back_keeps_both_branches_in_sync() {
    let _serial = serial().await;
    let r = repos().await;
    r.push("feature.txt", "X\n", "Kísérlet", "feature");
    r.apply("feature", false).await.unwrap();
    let main_new = r.push("VERSION", "2\n", "Második verzió", "main");
    r.apply("main", false).await.unwrap();
    assert_eq!(r.branch(), "main");
    assert_eq!(r.head(), main_new);
    assert!(!r.app.join("feature.txt").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn already_up_to_date() {
    let _serial = serial().await;
    let r = repos().await;
    let error = r.apply("main", false).await.unwrap_err();
    assert_eq!(error.status, 409);
    assert!(r.system_audit().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reason_is_required_before_anything_runs() {
    let _serial = serial().await;
    let r = repos().await;
    r.push("VERSION", "2\n", "Második verzió", "main");
    let before = r.head();
    let error = r.apply_raw(Some(json!("main")), Some(json!("")), false, false).await.unwrap_err();
    assert_eq!(error.extra["field"], "reason");
    assert_eq!(r.head(), before);
    assert!(r.system_audit().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn local_modifications_block_the_update() {
    let _serial = serial().await;
    let r = repos().await;
    r.push("VERSION", "2\n", "Második verzió", "main");
    write(&r.app.join("VERSION"), "99 # helyi\n");
    let error = r.apply("main", false).await.unwrap_err();
    assert_eq!((error.status, error.extra["files"].clone()), (409, json!(["VERSION"])));
    assert_eq!(read(&r.app.join("VERSION")), "99 # helyi\n");
    assert!(r.system_audit().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn diverged_history_is_not_overwritten() {
    let _serial = serial().await;
    let r = repos().await;
    r.push("VERSION", "2\n", "Távoli commit", "main");
    write(&r.app.join("local.txt"), "LOCAL\n");
    git(&r.app, &["add", "-A"]);
    git(&r.app, &["commit", "-m", "Helyi commit"]);
    let local = r.head();
    let error = r.apply("main", false).await.unwrap_err();
    assert_eq!(error.status, 409);
    assert_eq!(r.head(), local);
    assert!(r.app.join("local.txt").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn code_that_does_not_build_rolls_the_update_back() {
    let _serial = serial().await;
    let r = repos().await;
    let before = r.head();
    r.push("src/main.rs", "fn main( {\n", "Hibás kód", "main");
    let error = r.apply("main", true).await.unwrap_err();
    assert_eq!(error.status, 422);
    assert!(error.extra["detail"].as_str().is_some_and(|d| !d.is_empty()));
    assert_eq!(r.head(), before);
    assert_eq!(read(&r.app.join("src/main.rs")), "fn main() {}\n");
    assert_eq!(audit_names(&r), vec!["system.update", "system.update_failed"]);
    let failed = &r.system_audit()[1].1;
    assert_eq!((failed["restored"].clone(), failed["reason"].clone()), (json!(true), json!("build")));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_switch_returns_to_the_previous_branch() {
    let _serial = serial().await;
    let r = repos().await;
    r.push("src/main.rs", "fn main( {\n", "Hibás ág", "feature");
    let before = r.head();
    assert!(r.apply("feature", true).await.is_err());
    assert_eq!(r.branch(), "main");
    assert_eq!(r.head(), before);
    assert_eq!(read(&r.app.join("src/main.rs")), "fn main() {}\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn malformed_branch_names_are_refused() {
    let _serial = serial().await;
    let r = repos().await;
    let long = "x".repeat(101);
    let cases: Vec<Option<Value>> = vec![
        None,
        Some(Value::Null),
        Some(json!("")),
        Some(json!(123)),
        Some(json!("--upload-pack=x")),
        Some(json!("-x")),
        Some(json!("../main")),
        Some(json!("a..b")),
        Some(json!("main;ls")),
        Some(json!("main branch")),
        Some(json!(long)),
        Some(json!("feature/")),
        Some(json!("a.lock")),
        Some(json!("főág")),
    ];
    for branch in cases {
        let error = r.apply_raw(branch.clone(), Some(json!(REASON)), false, false).await.unwrap_err();
        assert_eq!((error.status, error.extra["field"].clone()), (400, json!("branch")), "{branch:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_branch_that_is_not_on_github_is_refused() {
    let _serial = serial().await;
    let r = repos().await;
    git(&r.app, &["branch", "csak-helyi"]);
    assert_eq!(r.apply("csak-helyi", false).await.unwrap_err().status, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_build_runs_only_when_asked_and_needed() {
    let _serial = serial().await;
    let r = repos().await;
    // más fájl változik: nincs fordítás, ha kérik sem
    r.push("VERSION", "2\n", "Második verzió", "main");
    assert_eq!(r.apply("main", true).await.unwrap()["installed"], Value::Null);
    // a forrás változik, de nem kérték
    r.push("src/main.rs", "fn main() { let _x = 1; }\n", "Új kód", "main");
    assert_eq!(r.apply("main", false).await.unwrap()["installed"], Value::Null);
    assert!(!r.app.join("target").exists());
    // a forrás változik és kérték: lefordul
    r.push("src/main.rs", "fn main() { let _y = 2; }\n", "Még egy", "main");
    assert_eq!(r.apply("main", true).await.unwrap()["installed"], true);
    assert!(r.app.join("target/release").exists());
    assert_eq!(audit_names(&r).iter().filter(|a| *a == "system.update_done").count(), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_concurrent_update_is_refused() {
    let _serial = serial().await;
    let r = repos().await;
    r.push("VERSION", "2\n", "Második verzió", "main");
    let guard = update::hold_update_lock();
    let error = r.apply("main", false).await.unwrap_err();
    drop(guard);
    assert_eq!(error.status, 409);
    assert!(r.apply("main", false).await.is_ok());
}

// ===================================================================================================
// Újraindítás
// ===================================================================================================

async fn restart(r: &Repos, reason: &str) -> Result<Value, AdminError> {
    let app = r.server.app.clone();
    let ctx = r.ctx();
    let reason = json!(reason);
    tokio::task::spawn_blocking(move || update::restart(&app, &ctx, Some(&reason))).await.unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn restart_is_logged_first_and_runs_in_the_background() {
    let _serial = serial().await;
    let r = repos().await;
    let before = SKIPPED_RESTARTS.load(Ordering::SeqCst);
    assert_eq!(restart(&r, REASON).await.unwrap(), json!({"restarting": true}));
    assert_eq!(audit_names(&r), vec!["system.restart"]);
    assert_eq!(SKIPPED_RESTARTS.load(Ordering::SeqCst), before); // a válasz kimehet: az újraindítás később jön
    wait_restart_done(&r).await;
    assert_eq!(SKIPPED_RESTARTS.load(Ordering::SeqCst), before + 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_request_is_refused_while_it_is_pending() {
    let _serial = serial().await;
    let r = repos().await;
    let before = SKIPPED_RESTARTS.load(Ordering::SeqCst);
    restart(&r, REASON).await.unwrap();
    assert_eq!(r.status().await["restart_scheduled"], true);
    assert_eq!(restart(&r, REASON).await.unwrap_err().status, 409);
    wait_restart_done(&r).await;
    assert_eq!(SKIPPED_RESTARTS.load(Ordering::SeqCst), before + 1); // csak egy újraindítás ütemeződött
}

#[tokio::test(flavor = "multi_thread")]
async fn the_restart_needs_a_reason() {
    let _serial = serial().await;
    let r = repos().await;
    let before = SKIPPED_RESTARTS.load(Ordering::SeqCst);
    assert!(restart(&r, "x").await.is_err());
    assert!(r.system_audit().is_empty());
    assert_eq!(r.status().await["restart_scheduled"], false);
    assert_eq!(SKIPPED_RESTARTS.load(Ordering::SeqCst), before);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_update_can_restart_afterwards() {
    let _serial = serial().await;
    let r = repos().await;
    let before = SKIPPED_RESTARTS.load(Ordering::SeqCst);
    r.push("VERSION", "2\n", "Második verzió", "main");
    let result = r.apply_raw(Some(json!("main")), Some(json!(REASON)), false, true).await.unwrap();
    assert_eq!(result["restarting"], true);
    wait_restart_done(&r).await;
    assert_eq!(SKIPPED_RESTARTS.load(Ordering::SeqCst), before + 1);
}

// ===================================================================================================
// HTTP
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn status_and_check_over_http() {
    let _serial = serial().await;
    let r = repos().await;
    let api = r.server.admin_http().await;
    let data = api.admin_get("/system/update").await.json()["update"].clone();
    assert_eq!((data["repo"].clone(), data["branch"].clone()), (json!(true), json!("main")));
    r.push("VERSION", "2\n", "Második verzió", "main");
    let checked = api.admin_post("/system/update/check", json!({})).await.json()["update"].clone();
    assert_eq!((checked["plan"]["behind"].clone(), checked["branches"][0]["name"].clone()), (json!(1), json!("main")));
    assert_eq!(api.admin_post("/system/update/check", json!({"branch": "nincs"})).await.status, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn apply_needs_sudo_and_a_reason_over_http() {
    let _serial = serial().await;
    let r = repos().await;
    let api = r.server.admin_http().await;
    let new = r.push("VERSION", "2\n", "Második verzió", "main");
    let before = r.head();
    let denied = api.admin_post("/system/update/apply", json!({"branch": "main", "reason": REASON})).await;
    assert_eq!(denied.status, 401);
    assert_eq!(denied.json()["sudo_required"], true);
    api.sudo().await;
    assert_eq!(api.admin_post("/system/update/apply", json!({"branch": "main"})).await.status, 400);
    assert_eq!(r.head(), before);
    let done = api.admin_post("/system/update/apply", json!({"branch": "main", "reason": REASON})).await;
    assert_eq!(done.status, 200, "{}", done.text());
    assert_eq!(done.json()["update"]["restart_needed"], true);
    assert_eq!(r.head(), new);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_restart_needs_sudo_over_http() {
    let _serial = serial().await;
    let r = repos().await;
    let api = r.server.admin_http().await;
    assert_eq!(api.admin_post("/system/restart", json!({"reason": REASON})).await.status, 401);
    api.sudo().await;
    let response = api.admin_post("/system/restart", json!({"reason": REASON})).await;
    assert_eq!(response.json(), json!({"success": true, "restarting": true}));
    wait_restart_done(&r).await;
}
